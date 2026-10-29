use embassy_executor::Spawner;
use embassy_futures::select::{select, select3, Either, Either3};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Sender, mutex::Mutex, signal::Signal,
};

use embassy_time::Timer;
use heapless::Vec;
use portable_atomic::{AtomicBool, Ordering};
use static_cell::StaticCell;

use crate::alarm::model::Alarm;
use crate::config::api_config::{API_POLLING_INTERVAL_S, API_RETRY_S};
use crate::net::HttpClientManager;
use crate::time_lib::TimeManager;

#[derive(Debug, Clone, PartialEq)]
pub enum AlarmDriverError {
    AlarmListFull,
    AlarmListEmpty,
    AlarmListLocked,
    TimeError,
    NoComingAlarm,
}

static ALARM_DRIVER: StaticCell<AlarmDriver> = StaticCell::new();

pub struct AlarmDriver {
    alarms: Mutex<CriticalSectionRawMutex, Vec<Alarm, 4>>,
    new_alarms: Signal<CriticalSectionRawMutex, ()>,
    http_client: &'static HttpClientManager,
    time_client: &'static TimeManager,
    alarm_fire_sender: Sender<'static, CriticalSectionRawMutex, (Alarm, u32), 1>,
    enabled: AtomicBool,
    /// Signalled whenever `enabled` changes so wait_for_enabled/disabled wake immediately.
    enabled_changed: Signal<CriticalSectionRawMutex, ()>,
}

impl AlarmDriver {
    pub fn initialize(
        spawner: Spawner,
        http_client: &'static HttpClientManager,
        time_client: &'static TimeManager,
        alarm_update_signal: &'static Signal<CriticalSectionRawMutex, ()>,
        alarm_fire_sender: Sender<'static, CriticalSectionRawMutex, (Alarm, u32), 1>,
    ) -> &'static AlarmDriver {
        let driver = ALARM_DRIVER.init(Self {
            alarms: Mutex::new(Vec::new()),
            http_client,
            new_alarms: Signal::new(),
            time_client,
            alarm_fire_sender,
            enabled: AtomicBool::new(true),
            enabled_changed: Signal::new(),
        });

        spawner.spawn(alarm_fetcher_task(driver, alarm_update_signal).unwrap());
        spawner.spawn(run_task(driver).unwrap());
        driver
    }

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
                log::info!("Alarm driver enabled");
                return;
            }
            self.enabled_changed.wait().await;
        }
    }

    async fn wait_for_disabled(&self) {
        loop {
            if !self.enabled.load(Ordering::Relaxed) {
                log::info!("Alarm driver disabled");
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

    async fn handle_alarms(&self, alarms: Vec<Alarm, 8>) -> Result<(), AlarmDriverError> {
        match select(self.alarms.lock(), Timer::after_secs(10)).await {
            Either::First(mut alarm_guard) => {
                alarm_guard.clear();

                let now = self
                    .time_client
                    .get_current_time()
                    .await
                    .map_err(|_e| AlarmDriverError::TimeError)?;

                for alarm in alarms {
                    if !alarm.is_active {
                        continue;
                    }

                    log::info!("Handling alarm {}", alarm.id);
                    if !alarm_guard.is_full() {
                        alarm_guard
                            .push(alarm)
                            .map_err(|_e| AlarmDriverError::AlarmListFull)?; //this should never happen
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
                return Err(AlarmDriverError::AlarmListLocked);
            }
        }
        Ok(())
    }

    pub async fn get_next_alarm(&self) -> Result<Alarm, AlarmDriverError> {
        let alarm_guard = self.alarms.lock().await;
        if let Ok(now) = self.time_client.get_current_time().await {
            return Ok(alarm_guard
                .iter()
                .min_by_key(|a| a.time_to_next_trigger(now).unwrap_or(u32::MAX))
                .ok_or(AlarmDriverError::AlarmListEmpty)?
                .clone());
        }
        Err(AlarmDriverError::NoComingAlarm)
    }

    async fn get_active_alarm(&self) -> Option<(Alarm, u32)> {
        let now = self.time_client.get_current_time().await.ok()?;
        let alarm_guard = self.alarms.lock().await;
        alarm_guard.iter().find_map(|alarm| {
            alarm
                .elapsed_in_alarm(now)
                .map(|elapsed| (alarm.clone(), elapsed))
        })
    }
}

#[embassy_executor::task]
async fn alarm_fetcher_task(
    alarm_driver: &'static AlarmDriver,
    alarm_update_signal: &'static Signal<CriticalSectionRawMutex, ()>,
) -> ! {
    log::info!("Starting alarm fetcher task");
    let mut next_fetch = API_RETRY_S;
    loop {
        match select(Timer::after_secs(next_fetch), alarm_update_signal.wait()).await {
            Either::First(_) => log::info!("Performing periodic alarm fetch"),
            Either::Second(_) => log::info!("Alarm fetch triggered"),
        }
        next_fetch = alarm_driver.fetch_alarms().await;
    }
}

#[embassy_executor::task]
async fn run_task(alarm_driver: &'static AlarmDriver) -> ! {
    log::info!("Starting alarm driver task");

    loop {
        alarm_driver.wait_for_enabled().await;

        if let Some((alarm, elapsed)) = alarm_driver.get_active_alarm().await {
            log::info!("Resuming alarm '{}' at {}s elapsed", alarm.name, elapsed);
            alarm_driver.alarm_fire_sender.send((alarm, elapsed)).await;
        }

        loop {
            let next_alarm = match alarm_driver.get_next_alarm().await {
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
                match alarm_driver.time_client.get_current_time().await {
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
                alarm_driver.new_alarms.wait(),
                Timer::after_secs(timeout),
                alarm_driver.wait_for_disabled(),
            )
            .await
            {
                Either3::First(_) => {
                    log::info!("Alarms updated, refreshing next alarm");
                }
                Either3::Second(_) => {
                    if time_to_next_alarm.unwrap_or(u32::MAX) <= timeout as u32 {
                        if let Some(alarm) = &next_alarm {
                            log::info!("Alarm '{}' due, handing off to state machine", alarm.name);
                            alarm_driver
                                .alarm_fire_sender
                                .send((alarm.clone(), 0u32))
                                .await;
                        }
                    }
                }
                Either3::Third(_) => {
                    log::info!("Alarm driver disabled");
                    break; // Exit inner loop, go back to waiting for enable
                }
            }
        }
    }
}
