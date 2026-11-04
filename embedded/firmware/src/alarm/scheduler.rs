use embassy_executor::Spawner;
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, signal::Signal};

use embassy_time::Timer;
use heapless::Vec;
use portable_atomic::{AtomicBool, Ordering};
use static_cell::StaticCell;

use crate::alarm::model::Alarm;
use crate::config::api_config::{API_POLLING_INTERVAL_S, API_RETRY_S};
use crate::mailbox::{Mailbox, MailboxReceiver, MailboxSender};
use crate::net::HttpClient;
use crate::state_machine::StateHandle;
use crate::time_lib::Clock;

#[derive(Debug, Clone, PartialEq)]
pub enum AlarmError {
    AlarmListFull,
    AlarmListEmpty,
    AlarmListLocked,
    TimeError,
    NoComingAlarm,
}

static ALARM_SCHEDULER: StaticCell<AlarmScheduler> = StaticCell::new();
static MAILBOX: Mailbox<AlarmMsg> = Mailbox::new();

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
    /// Fetch the alarms from the backend now instead of at the next poll
    pub fn refresh(&self) {
        self.mailbox.post_or_warn(AlarmMsg::Refresh);
    }
}

pub struct AlarmScheduler {
    alarms: Mutex<CriticalSectionRawMutex, Vec<Alarm, 4>>,
    new_alarms: Signal<CriticalSectionRawMutex, ()>,
    mailbox: MailboxReceiver<AlarmMsg>,
    http_client: &'static HttpClient,
    clock: &'static Clock,
    state: StateHandle,
    enabled: AtomicBool,
    /// Signalled whenever `enabled` changes so wait_for_enabled/disabled wake immediately.
    enabled_changed: Signal<CriticalSectionRawMutex, ()>,
}

pub fn spawn(
    spawner: Spawner,
    http_client: &'static HttpClient,
    clock: &'static Clock,
    state: StateHandle,
) -> &'static AlarmScheduler {
    let scheduler = ALARM_SCHEDULER.init(AlarmScheduler {
        alarms: Mutex::new(Vec::new()),
        new_alarms: Signal::new(),
        mailbox: MAILBOX.receiver(),
        http_client,
        clock,
        state,
        enabled: AtomicBool::new(true),
        enabled_changed: Signal::new(),
    });

    spawner.spawn(alarm_fetch_task(scheduler).unwrap());
    spawner.spawn(alarm_task(scheduler).unwrap());
    scheduler
}

impl AlarmScheduler {
    pub fn enable(&self) {
        self.enabled.store(true, Ordering::Relaxed);
        self.enabled_changed.signal(());
    }

    pub fn disable(&self) {
        self.enabled.store(false, Ordering::Relaxed);
        self.enabled_changed.signal(());
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    async fn wait_for_enabled(&self) {
        loop {
            if self.enabled.load(Ordering::Relaxed) {
                log::info!("Alarm scheduler enabled");
                return;
            }
            self.enabled_changed.wait().await;
        }
    }

    async fn wait_for_disabled(&self) {
        loop {
            if !self.enabled.load(Ordering::Relaxed) {
                log::info!("Alarm scheduler disabled");
                return;
            }
            self.enabled_changed.wait().await;
        }
    }

    async fn fetch_alarms(&self) -> u64 {
        match self.http_client.get_alarm_sync().await {
            Ok(sync) => {
                log::info!("Alarms fetched succesfully");
                if sync.alarms_enabled {
                    self.enable();
                } else {
                    self.disable();
                }
                match self.handle_alarms(sync.alarms).await {
                    Ok(_) => {
                        log::info!("Alarms scheduled successfully");
                        self.new_alarms.signal(());
                        API_POLLING_INTERVAL_S
                    }
                    Err(err) => {
                        log::error!("Failed to schedule alarms: {err:?}");
                        API_RETRY_S
                    }
                }
            }
            Err(err) => {
                log::error!("Failed to fetch alarms: {err:?}");
                API_RETRY_S
            }
        }
    }

    async fn handle_alarms(&self, alarms: Vec<Alarm, 8>) -> Result<(), AlarmError> {
        match select(self.alarms.lock(), Timer::after_secs(10)).await {
            Either::First(mut alarm_guard) => {
                alarm_guard.clear();

                let now = self
                    .clock
                    .get_current_time()
                    .await
                    .map_err(|_e| AlarmError::TimeError)?;

                for alarm in alarms {
                    if !alarm.is_active {
                        continue;
                    }

                    log::info!("Handling alarm {}", alarm.id);
                    if !alarm_guard.is_full() {
                        alarm_guard
                            .push(alarm)
                            .map_err(|_e| AlarmError::AlarmListFull)?; //this should never happen
                        log::info!("Added new alarm");
                    } else {
                        // need to find the oldest alarm and replace it if this alarm is newer
                        let (max_idx, max_val) = alarm_guard
                            .iter()
                            .map(|a| a.time_to_next_trigger(now).unwrap_or(u32::MAX))
                            .enumerate()
                            .max_by_key(|&(_idx, time)| time)
                            .unwrap_or((0, u32::MAX));

                        if alarm.time_to_next_trigger(now).unwrap_or(u32::MAX) < max_val {
                            alarm_guard[max_idx] = alarm;
                            log::info!("Replaced oldest alarm with new one");
                        } else {
                            log::warn!("Alarm list full, couldn't add new alarm");
                        }
                    }
                }
            }
            Either::Second(_) => {
                log::error!("Could not get lock on the ALARMS vec");
                return Err(AlarmError::AlarmListLocked);
            }
        }
        Ok(())
    }

    pub async fn get_next_alarm(&self) -> Result<Alarm, AlarmError> {
        let alarm_guard = self.alarms.lock().await;
        if let Ok(now) = self.clock.get_current_time().await {
            return Ok(alarm_guard
                .iter()
                .min_by_key(|a| a.time_to_next_trigger(now).unwrap_or(u32::MAX))
                .ok_or(AlarmError::AlarmListEmpty)?
                .clone());
        }
        Err(AlarmError::NoComingAlarm)
    }

    async fn get_active_alarm(&self) -> Option<(Alarm, u32)> {
        let now = self.clock.get_current_time().await.ok()?;
        let alarm_guard = self.alarms.lock().await;
        alarm_guard.iter().find_map(|alarm| {
            alarm
                .elapsed_in_alarm(now)
                .map(|elapsed| (alarm.clone(), elapsed))
        })
    }

    async fn run_fetch_loop(&self) -> ! {
        log::info!("Starting alarm fetcher task");
        let mut next_fetch = API_RETRY_S;
        loop {
            match select(Timer::after_secs(next_fetch), self.mailbox.receive()).await {
                Either::First(_) => log::info!("Performing periodic alarm fetch"),
                Either::Second(AlarmMsg::Refresh) => log::info!("Alarm fetch triggered"),
            }
            next_fetch = self.fetch_alarms().await;
        }
    }

    async fn run(&self) -> ! {
        log::info!("Starting alarm scheduler task");

        loop {
            self.wait_for_enabled().await;

            if let Some((alarm, elapsed)) = self.get_active_alarm().await {
                log::info!("Resuming alarm '{}' at {}s elapsed", alarm.name, elapsed);
                self.state.start_alarm(alarm, elapsed);
            }

            loop {
                let next_alarm = match self.get_next_alarm().await {
                    Ok(alarm) => {
                        log::info!("Next alarm: '{}'", alarm.name);
                        Some(alarm)
                    }
                    Err(e) => {
                        log::warn!("No alarms available: {e:?}");
                        None
                    }
                };

                let time_to_next_alarm = if let Some(alarm) = &next_alarm {
                    match self.clock.get_current_time().await {
                        Ok(now) => alarm.time_to_next_trigger(now).ok(),
                        Err(e) => {
                            log::error!("Failed to get current time: {e:?}");
                            None
                        }
                    }
                } else {
                    None
                };

                log::debug!("Time to next alarm: {time_to_next_alarm:?} seconds");

                let timeout = time_to_next_alarm.unwrap_or(8 * 3600).min(8 * 3600) as u64;

                match select3(
                    self.new_alarms.wait(),
                    Timer::after_secs(timeout),
                    self.wait_for_disabled(),
                )
                .await
                {
                    Either3::First(_) => {
                        log::info!("Alarms updated, refreshing next alarm");
                    }
                    Either3::Second(_) => {
                        if time_to_next_alarm.unwrap_or(u32::MAX) <= timeout as u32 {
                            if let Some(alarm) = next_alarm {
                                log::info!(
                                    "Alarm '{}' due, handing off to state machine",
                                    alarm.name
                                );
                                self.state.start_alarm(alarm, 0);
                            }
                        }
                    }
                    Either3::Third(_) => {
                        log::info!("Alarm scheduler disabled");
                        break; // Exit inner loop, go back to waiting for enable
                    }
                }
            }
        }
    }
}

#[embassy_executor::task]
async fn alarm_fetch_task(scheduler: &'static AlarmScheduler) -> ! {
    scheduler.run_fetch_loop().await
}

#[embassy_executor::task]
async fn alarm_task(scheduler: &'static AlarmScheduler) -> ! {
    scheduler.run().await
}
