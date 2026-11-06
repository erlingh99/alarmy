use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::{Receiver, Watch};
use embassy_time::{Instant, Timer};
use time::{Duration, OffsetDateTime, UtcOffset};

use crate::config::api_config::TIME_SYNC_INTERVAL_S;
use crate::health::{self, Backoff, Fault};
use crate::net::HttpClient;

// Could use the onboard RTC also, but little gain

/// The display and the alarm scheduler.
const SUBSCRIBERS: usize = 2;
static TIME_SYNC: Watch<CriticalSectionRawMutex, TimeSync, SUBSCRIBERS> = Watch::new();

pub fn subscribe() -> Receiver<'static, CriticalSectionRawMutex, TimeSync, SUBSCRIBERS> {
    TIME_SYNC
        .receiver()
        .expect("more time sync subscribers than SUBSCRIBERS allows")
}

#[derive(Debug)]
pub enum TimeError {
    NetworkError,
    InvalidResponse,
}

/// The time at a known instant
#[derive(Debug, Clone, Copy)]
pub struct TimeSync {
    utc: OffsetDateTime,
    at: Instant,
    utc_offset: UtcOffset,
}

impl TimeSync {
    pub fn utc_now(&self) -> OffsetDateTime {
        let since = self.at.elapsed().as_micros() as i64;
        self.utc + Duration::microseconds(since)
    }

    pub fn local_now(&self) -> OffsetDateTime {
        self.utc_now().to_offset(self.utc_offset)
    }

    pub fn instant_at(&self, utc: OffsetDateTime) -> Instant {
        // Rounded up, so a timer at this instant never wakes early
        let micros = (utc - self.utc + Duration::nanoseconds(999))
            .whole_microseconds()
            .max(0) as u64;
        self.at + embassy_time::Duration::from_micros(micros)
    }
}

struct Clock {
    http_client: &'static HttpClient,
}

impl Clock {
    async fn run(&self) -> ! {
        log::info!("Starting time sync");
        let mut backoff = Backoff::new();
        loop {
            match self.sync().await {
                Ok(sync) => {
                    log::info!("Time resync successful");
                    health::clear(Fault::TimeSync);
                    TIME_SYNC.sender().send(sync);
                    backoff.reset();
                    Timer::after_secs(TIME_SYNC_INTERVAL_S).await;
                }
                Err(e) => {
                    health::report(Fault::TimeSync, e);
                    backoff.wait().await;
                }
            };
        }
    }

    async fn sync(&self) -> Result<TimeSync, TimeError> {
        let time_data = self
            .http_client
            .get_time()
            .await
            .map_err(|_| TimeError::NetworkError)?;
        let at = Instant::now();
        let utc = OffsetDateTime::from_unix_timestamp(time_data.timestamp as i64)
            .map_err(|_| TimeError::InvalidResponse)?;
        let utc_offset = UtcOffset::from_whole_seconds(time_data.utc_offset_seconds)
            .map_err(|_| TimeError::InvalidResponse)?;
        Ok(TimeSync {
            utc,
            at,
            utc_offset,
        })
    }
}

pub fn spawn(spawner: Spawner, http_client: &'static HttpClient) {
    spawner.spawn(clock_task(Clock { http_client }).unwrap());
}

#[embassy_executor::task]
async fn clock_task(clock: Clock) -> ! {
    clock.run().await
}
