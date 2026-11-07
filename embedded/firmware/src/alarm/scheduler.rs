use embassy_executor::Spawner;
use embassy_futures::select::{Either4, select4};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::{Receiver, Watch};
use embassy_time::{Duration, Instant, Timer, with_deadline};
use heapless::{String, Vec};
use time::{PlainDateTime, Time};

use crate::alarm::model::Alarm;
use crate::config::api_config::API_POLLING_INTERVAL_S;
use crate::hardware::HardwareHandle;
use crate::health::{self, Backoff, Fault};
use crate::mailbox::{Mailbox, MailboxReceiver, MailboxSender};
use crate::net::{HttpClient, NetworkError};
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
    last_fired: Option<PlainDateTime>,
    /// The alarm last handed to the hardware, so a fetch that drops it can stop it. Not cleared
    /// when it finishes, as the hardware ignores a stop for an alarm it isn't playing
    playing: Option<String<36>>,
    /// Whether the last fetch had more active alarms than fit
    alarms_dropped: bool,
}

/// The outcome of paging through the backend's alarms
struct Fetched {
    /// The active alarms that trigger soonest
    alarms: Vec<Alarm, MAX_ALARMS>,
    enabled: bool,
    dropped: bool,
    /// Whether the playing alarm is still active
    playing_kept: bool,
}

impl AlarmScheduler {
    async fn run(&mut self) -> ! {
        log::info!("Starting alarm scheduler");
        let mut time = time_lib::subscribe();
        let mut sync = time.get().await;
        loop {
            let now = sync.local_now();
            let next = self.next_alarm(self.last_fired.map_or(now, |fired| now.max(fired)));
            self.publish_status(next.as_ref().map(|(alarm, _)| alarm));

            let due = match &next {
                Some((_, trigger)) if self.enabled == Some(true) => sync.instant_at(*trigger),
                _ => Instant::MAX,
            };
            match select4(
                Timer::at(due),
                self.mailbox.receive(),
                Timer::at(self.next_fetch),
                time.changed(),
            )
            .await
            {
                Either4::First(()) => {}
                Either4::Second(AlarmMsg::Refresh) | Either4::Third(()) => {
                    log::info!("Alarm fetch triggered");
                    // Cut short when the alarm comes due, so a slow fetch can't delay it
                    if with_deadline(due, self.fetch(sync)).await.is_err() {
                        self.next_fetch = Instant::now();
                    }
                }
                Either4::Fourth(new_sync) => sync = new_sync,
            }

            let now = sync.local_now();
            // On the wall clock, so a resync or fetch that passed the trigger still fires it
            if let Some((alarm, trigger)) = next
                && self.enabled == Some(true)
                && now >= trigger
            {
                self.start_if_still_due(&alarm.id, trigger, now).await;
            }
        }
    }

    /// Starts the stored copy of the alarm, unless a fetch since deleted, deactivated or moved it
    async fn start_if_still_due(&mut self, id: &str, trigger: PlainDateTime, now: PlainDateTime) {
        let still_due = self.alarms.iter().find(|alarm| {
            alarm.id == id
                && alarm.next_trigger(trigger - time::Duration::SECOND).ok() == Some(trigger)
        });
        match still_due {
            Some(alarm) => {
                log::info!("Alarm '{}' due", alarm.name);
                let elapsed = (now - trigger).whole_seconds().max(0) as u32;
                self.last_fired = Some(trigger);
                self.playing = Some(alarm.id.clone());
                self.hardware.start_alarm(alarm.clone(), elapsed).await;
            }
            None => log::info!("Alarm due, but changed since it was scheduled"),
        }
    }

    async fn fetch(&mut self, time: TimeSync) {
        let wait = match self.fetch_alarms(time.local_now()).await {
            Ok(fetched) => {
                log::info!("Alarms fetched succesfully");
                health::clear(Fault::AlarmFetch);
                self.fetch_backoff.reset();
                self.alarms = fetched.alarms;
                self.alarms_dropped = fetched.dropped;
                if (!fetched.enabled || !fetched.playing_kept)
                    && let Some(id) = self.playing.take()
                {
                    self.hardware.stop_alarm(id);
                }
                if self.enabled != Some(fetched.enabled) {
                    log::info!("Alarms enabled: {}", fetched.enabled);
                    // On boot and when re-enabled, so an alarm already mid-sequence starts too
                    if fetched.enabled {
                        // After the fetch, so a trigger passed while fetching still resumes
                        self.resume_active_alarm(time.local_now()).await;
                    }
                }
                self.enabled = Some(fetched.enabled);
                API_POLLING_INTERVAL_S
            }
            Err(err) => {
                health::report(Fault::AlarmFetch, err);
                self.fetch_backoff.next_delay_s()
            }
        };
        self.next_fetch = Instant::now() + Duration::from_secs(wait);
    }

    /// Pages through the backend's active alarms, keeping those that trigger soonest
    async fn fetch_alarms(&self, now: PlainDateTime) -> Result<Fetched, NetworkError> {
        let mut fetched = Fetched {
            alarms: Vec::new(),
            enabled: false,
            dropped: false,
            playing_kept: false,
        };
        let mut after: Option<String<36>> = None;
        loop {
            let page = self.http_client.get_alarm_page(after.as_deref()).await?;
            fetched.enabled = page.alarms_enabled;
            after = page.alarms.last().map(|alarm| alarm.id.clone());
            for alarm in page.alarms.into_iter().filter(|alarm| alarm.is_active) {
                fetched.playing_kept |= self.playing.as_ref() == Some(&alarm.id);
                fetched.dropped |= !keep_soonest(&mut fetched.alarms, alarm, now);
            }
            if !page.has_more || after.is_none() {
                break;
            }
        }
        log::info!("{} alarms scheduled", fetched.alarms.len());
        Ok(fetched)
    }

    /// The alarm triggering next, and when
    fn next_alarm(&self, now: PlainDateTime) -> Option<(Alarm, PlainDateTime)> {
        self.alarms
            .iter()
            .filter_map(|alarm| Some((alarm, alarm.next_trigger(now).ok()?)))
            .min_by_key(|&(_, trigger)| trigger)
            .map(|(alarm, trigger)| {
                log::debug!("Next alarm '{}' at {trigger}", alarm.name);
                (alarm.clone(), trigger)
            })
    }

    async fn resume_active_alarm(&mut self, now: PlainDateTime) {
        let active = self
            .alarms
            .iter()
            .find_map(|alarm| Some((alarm.clone(), alarm.elapsed_in_alarm(now)?)));
        if let Some((alarm, elapsed)) = active {
            log::info!("Resuming alarm '{}' at {}s elapsed", alarm.name, elapsed);
            self.playing = Some(alarm.id.clone());
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
        STATUS.sender().send_if_modified(|value| {
            let changed = value.as_ref() != Some(&status);
            if changed {
                *value = Some(status.clone());
            }
            changed
        });
    }
}

/// Keeps the alarm if there is room or it triggers sooner than one kept, returning false when
/// one is dropped. A running alarm counts as triggering now, so it is never dropped while it plays.
fn keep_soonest(kept: &mut Vec<Alarm, MAX_ALARMS>, alarm: Alarm, now: PlainDateTime) -> bool {
    let secs_until = |alarm: &Alarm| match alarm.elapsed_in_alarm(now) {
        Some(_) => 0,
        None => alarm.time_to_next_trigger(now).unwrap_or(u32::MAX),
    };
    let Err(alarm) = kept.push(alarm) else {
        return true;
    };
    let (latest_idx, latest) = kept
        .iter()
        .map(secs_until)
        .enumerate()
        .max_by_key(|&(_, secs)| secs)
        .unwrap_or((0, u32::MAX));
    if secs_until(&alarm) < latest {
        log::info!("Alarm list full, dropped '{}'", kept[latest_idx].name);
        kept[latest_idx] = alarm;
    } else {
        log::warn!("Alarm list full, dropped '{}'", alarm.name);
    }
    false
}

pub fn spawn(spawner: Spawner, http_client: &'static HttpClient, hardware: HardwareHandle) {
    let scheduler = AlarmScheduler {
        mailbox: MAILBOX.receiver(),
        http_client,
        hardware,
        alarms: Vec::new(),
        enabled: None,
        next_fetch: Instant::now(),
        fetch_backoff: Backoff::default(),
        last_fired: None,
        playing: None,
        alarms_dropped: false,
    };
    spawner.spawn(alarm_task(scheduler).unwrap());
}

#[embassy_executor::task]
async fn alarm_task(mut scheduler: AlarmScheduler) -> ! {
    scheduler.run().await
}
