use embassy_executor::Spawner;
use embassy_futures::select::{Either4, select4};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::{Receiver, Watch};
use embassy_time::{Duration, Instant, Timer, with_deadline};
use heapless::{String, Vec};
use time::{OffsetDateTime, Time};

use crate::alarm::model::Alarm;
use crate::config::api_config::API_POLLING_INTERVAL_S;
use crate::hardware::HardwareHandle;
use crate::health::{self, Backoff, Fault};
use crate::mailbox::{Mailbox, MailboxReceiver, MailboxSender};
use crate::net::HttpClient;
use crate::time_lib::{self, TimeSync};

const MAX_ALARMS: usize = 4;

static MAILBOX: Mailbox<AlarmMsg> = Mailbox::new();

/// The display
const SUBSCRIBERS: usize = 1;
static STATUS: Watch<CriticalSectionRawMutex, AlarmStatus, SUBSCRIBERS> = Watch::new();

pub fn subscribe() -> Receiver<'static, CriticalSectionRawMutex, AlarmStatus, SUBSCRIBERS> {
    STATUS
        .receiver()
        .expect("more alarm status subscribers than SUBSCRIBERS allows")
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlarmStatus {
    pub enabled: bool,
    /// Name and time of the alarm that triggers next
    pub next: Option<(String<32>, Time)>,
}

#[derive(Debug)]
enum AlarmMsg {
    Refresh,
}

#[derive(Clone, Copy)]
pub struct AlarmHandle {
    mailbox: MailboxSender<AlarmMsg>,
}

pub fn handle() -> AlarmHandle {
    AlarmHandle {
        mailbox: MAILBOX.sender(),
    }
}

impl AlarmHandle {
    /// Fetch the alarms from the backend now
    pub fn refresh(&self) {
        self.mailbox.post_or_warn(AlarmMsg::Refresh);
    }
}

/// Fetches the alarms from the backend and hands each one to the state machine when it is due
struct AlarmScheduler {
    mailbox: MailboxReceiver<AlarmMsg>,
    http_client: &'static HttpClient,
    hardware: HardwareHandle,
    alarms: Vec<Alarm, MAX_ALARMS>,
    /// None until the first fetch
    enabled: Option<bool>,
    next_fetch: Instant,
    fetch_backoff: Backoff,
    /// So a resync moving the clock back doesn't fire the same trigger twice
    last_fired: Option<OffsetDateTime>,
}

impl AlarmScheduler {
    async fn run(&mut self) -> ! {
        log::info!("Starting alarm scheduler");
        let mut time = time_lib::subscribe();
        let mut sync = time.get().await;
        loop {
            let now = sync.utc_now();
            let next = self.next_alarm(self.last_fired.map_or(now, |fired| now.max(fired)));
            self.publish_status(next.as_ref().map(|(alarm, _)| alarm));

            let due = match &next {
                Some((_, trigger)) if self.enabled == Some(true) => sync.instant_at(*trigger),
                _ => Instant::MAX,
            };
            match select4(
                self.mailbox.receive(),
                Timer::at(self.next_fetch),
                Timer::at(due),
                time.changed(),
            )
            .await
            {
                Either4::First(AlarmMsg::Refresh) | Either4::Second(()) => {
                    log::info!("Alarm fetch triggered");
                    // Cut short when the alarm comes due, so a slow fetch can't delay it
                    if with_deadline(due, self.fetch(sync)).await.is_err() {
                        self.next_fetch = Instant::now();
                    }
                }
                Either4::Third(()) => {}
                Either4::Fourth(new_sync) => sync = new_sync,
            }

            // On the wall clock, so a resync or fetch that passed the trigger still fires it
            if let Some((alarm, trigger)) = next
                && self.enabled == Some(true)
                && sync.utc_now() >= trigger
            {
                self.start_if_still_due(&alarm.id, trigger, sync.utc_now())
                    .await;
            }
        }
    }

    /// Starts the stored copy of the alarm, unless a fetch since deleted, deactivated or moved it
    async fn start_if_still_due(&mut self, id: &str, trigger: OffsetDateTime, now: OffsetDateTime) {
        let still_due = self.alarms.iter().find(|alarm| {
            alarm.id == id
                && alarm.next_trigger(trigger - time::Duration::SECOND).ok() == Some(trigger)
        });
        match still_due {
            Some(alarm) => {
                log::info!("Alarm '{}' due", alarm.name);
                let elapsed = (now - trigger).whole_seconds().max(0) as u32;
                self.last_fired = Some(trigger);
                self.hardware.start_alarm(alarm.clone(), elapsed).await;
            }
            None => log::info!("Alarm due, but changed since it was scheduled"),
        }
    }

    async fn fetch(&mut self, time: TimeSync) {
        let wait = match self.http_client.get_alarm_sync().await {
            Ok(sync) => {
                log::info!("Alarms fetched succesfully");
                health::clear(Fault::AlarmFetch);
                self.fetch_backoff.reset();
                let now = time.utc_now();
                self.store_alarms(sync.alarms, now);
                if self.enabled != Some(sync.alarms_enabled) {
                    log::info!("Alarms enabled: {}", sync.alarms_enabled);
                    // On boot and when re-enabled, so an alarm already mid-sequence starts too
                    if sync.alarms_enabled {
                        self.resume_active_alarm(now).await;
                    }
                }
                self.enabled = Some(sync.alarms_enabled);
                API_POLLING_INTERVAL_S
            }
            Err(err) => {
                health::report(Fault::AlarmFetch, err);
                self.fetch_backoff.next_delay_s()
            }
        };
        self.next_fetch = Instant::now() + Duration::from_secs(wait);
    }

    /// Keeps the active alarms that trigger soonest
    fn store_alarms(&mut self, alarms: Vec<Alarm, 8>, now: OffsetDateTime) {
        let secs_until = |alarm: &Alarm| alarm.time_to_next_trigger(now).unwrap_or(u32::MAX);
        self.alarms.clear();
        for alarm in alarms.into_iter().filter(|alarm| alarm.is_active) {
            let Err(alarm) = self.alarms.push(alarm) else {
                continue;
            };
            let (latest_idx, latest) = self
                .alarms
                .iter()
                .map(secs_until)
                .enumerate()
                .max_by_key(|&(_, secs)| secs)
                .unwrap_or((0, u32::MAX));
            if secs_until(&alarm) < latest {
                log::info!("Alarm list full, replaced the alarm triggering last");
                self.alarms[latest_idx] = alarm;
            } else {
                log::warn!("Alarm list full, dropped '{}'", alarm.name);
            }
        }
        log::info!("{} alarms scheduled", self.alarms.len());
    }

    /// The alarm triggering next, and when
    fn next_alarm(&self, now: OffsetDateTime) -> Option<(Alarm, OffsetDateTime)> {
        self.alarms
            .iter()
            .filter_map(|alarm| Some((alarm, alarm.next_trigger(now).ok()?)))
            .min_by_key(|&(_, trigger)| trigger)
            .map(|(alarm, trigger)| {
                log::debug!("Next alarm '{}' at {trigger}", alarm.name);
                (alarm.clone(), trigger)
            })
    }

    async fn resume_active_alarm(&self, now: OffsetDateTime) {
        let active = self
            .alarms
            .iter()
            .find_map(|alarm| Some((alarm.clone(), alarm.elapsed_in_alarm(now)?)));
        if let Some((alarm, elapsed)) = active {
            log::info!("Resuming alarm '{}' at {}s elapsed", alarm.name, elapsed);
            self.hardware.start_alarm(alarm, elapsed).await;
        }
    }

    fn publish_status(&self, next: Option<&Alarm>) {
        // enabled is None untill the first fetch
        let Some(enabled) = self.enabled else {
            return;
        };
        let status = AlarmStatus {
            enabled,
            next: next.map(|alarm| (alarm.name.clone(), alarm.time)),
        };
        if STATUS.try_get().as_ref() != Some(&status) {
            STATUS.sender().send(status);
        }
    }
}

pub fn spawn(spawner: Spawner, http_client: &'static HttpClient, hardware: HardwareHandle) {
    let scheduler = AlarmScheduler {
        mailbox: MAILBOX.receiver(),
        http_client,
        hardware,
        alarms: Vec::new(),
        enabled: None,
        next_fetch: Instant::now(),
        fetch_backoff: Backoff::new(),
        last_fired: None,
    };
    spawner.spawn(alarm_task(scheduler).unwrap());
}

#[embassy_executor::task]
async fn alarm_task(mut scheduler: AlarmScheduler) -> ! {
    scheduler.run().await
}
