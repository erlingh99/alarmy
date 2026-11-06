use embassy_time::Timer;

use crate::config::api_config::{MAX_BACKOFF_DOUBLINGS, RETRY_DELAY_S};

/// Exponential retry delay, from RETRY_DELAY_S doubling up to RETRY_DELAY_S << max_doublings
pub struct Backoff {
    doublings: u32,
    max_doublings: u32,
}

impl Backoff {
    pub const fn new(max_doublings: u32) -> Self {
        Self {
            doublings: 0,
            max_doublings,
        }
    }

    pub fn reset(&mut self) {
        self.doublings = 0;
    }

    /// The delay before the next retry, in seconds
    pub fn next_delay_s(&mut self) -> u64 {
        let delay = RETRY_DELAY_S << self.doublings;
        self.doublings = (self.doublings + 1).min(self.max_doublings);
        delay
    }

    pub async fn wait(&mut self) {
        Timer::after_secs(self.next_delay_s()).await
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new(MAX_BACKOFF_DOUBLINGS)
    }
}
