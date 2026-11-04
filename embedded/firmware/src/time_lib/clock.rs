use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Instant, Timer};
use static_cell::StaticCell;
use time::{OffsetDateTime, UtcOffset};

use crate::config::api_config::TIME_SYNC_INTERVAL_S;
use crate::net::HttpClient;

static CLOCK: StaticCell<Clock> = StaticCell::new();
//TODO move over to use the onboard RTC

#[derive(Debug)]
pub enum TimeError {
    NetworkError,
    InvalidResponse,
    NotSynchronized,
}

pub struct Clock {
    http_client: &'static HttpClient,
    time_sync_offset: Mutex<CriticalSectionRawMutex, Option<i64>>,
    utc_offset_seconds: Mutex<CriticalSectionRawMutex, Option<i32>>,
}

pub fn spawn(spawner: Spawner, http_client: &'static HttpClient) -> &'static Clock {
    let clock = CLOCK.init(Clock {
        http_client,
        time_sync_offset: Mutex::new(None),
        utc_offset_seconds: Mutex::new(None),
    });
    spawner.spawn(clock_task(clock).unwrap());
    clock
}

impl Clock {
    /// Synchronize time with time server
    async fn sync_time(&self) -> Result<OffsetDateTime, TimeError> {
        let time_data = self
            .http_client
            .get_time()
            .await
            .map_err(|_| TimeError::NetworkError)?;

        let utc_time = OffsetDateTime::from_unix_timestamp(time_data.timestamp as i64)
            .map_err(|_| TimeError::InvalidResponse)?;

        let mut offset_guard = self.time_sync_offset.lock().await;
        *offset_guard = Some(utc_time.unix_timestamp() - Instant::now().as_secs() as i64);

        let mut utc_offset_guard = self.utc_offset_seconds.lock().await;
        *utc_offset_guard = Some(time_data.utc_offset_seconds);

        Ok(utc_time)
    }

    /// Get current time (requires prior sync)
    pub async fn get_current_time(&self) -> Result<OffsetDateTime, TimeError> {
        let time_offset = self
            .time_sync_offset
            .lock()
            .await
            .ok_or(TimeError::NotSynchronized)?;

        let now_secs = Instant::now().as_secs() as i64;
        let actual_time_secs = now_secs + time_offset;

        OffsetDateTime::from_unix_timestamp(actual_time_secs)
            .map_err(|_| TimeError::InvalidResponse)
    }

    /// Get current local time (requires prior sync)
    pub async fn get_local_time(&self) -> Result<OffsetDateTime, TimeError> {
        let utc_time = self.get_current_time().await?;
        let utc_offset_secs = self
            .utc_offset_seconds
            .lock()
            .await
            .ok_or(TimeError::NotSynchronized)?;
        let offset = UtcOffset::from_whole_seconds(utc_offset_secs)
            .map_err(|_| TimeError::InvalidResponse)?;
        Ok(utc_time.to_offset(offset))
    }
}

/// Background task to periodically sync time
#[embassy_executor::task]
async fn clock_task(clock: &'static Clock) -> ! {
    log::info!("Starting time_sync");
    loop {
        let wait = match clock.sync_time().await {
            Ok(_) => {
                log::info!("Time resync successful");
                TIME_SYNC_INTERVAL_S
            }
            Err(e) => {
                log::error!("Time resync failed: {e:?}");
                60
            }
        };
        Timer::after_secs(wait).await;
    }
}
