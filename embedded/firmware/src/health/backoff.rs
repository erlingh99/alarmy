use embassy_time::Timer;

use crate::config::api_config::{MAX_BACKOFF_SHIFT, RETRY_DELAY_S};

/// Exponential retry delay, from RETRY_DELAY_S doubling up to RETRY_DELAY_S << MAX_BACKOFF_SHIFT
pub struct Backoff {
    attempts: u32,
}

impl Backoff {
    pub const fn new() -> Self {
        Self { attempts: 0 }
    }

    pub fn reset(&mut self) {
        self.attempts = 0;
    }

    /// The delay before the next retry, in seconds
    pub fn next_delay_s(&mut self) -> u64 {
        let delay = RETRY_DELAY_S << self.attempts;
        self.attempts = (self.attempts + 1).min(MAX_BACKOFF_SHIFT);
        delay
    }

    pub async fn wait(&mut self) {
        Timer::after_secs(self.next_delay_s()).await
    }
}
