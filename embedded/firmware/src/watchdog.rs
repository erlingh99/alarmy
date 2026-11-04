//! Hardware watchdog fed by core0. A core1 stall or a missed OTA confirmation misses check-in
//! panics.

use embassy_executor::Spawner;
use embassy_rp::peripherals::WATCHDOG;
use embassy_rp::watchdog::Watchdog;
use embassy_rp::Peri;
use embassy_time::{Duration, Instant, Timer};
use portable_atomic::{AtomicU32, AtomicU64, Ordering};

const TIMEOUT: Duration = Duration::from_secs(8);
const FEED_INTERVAL: Duration = Duration::from_secs(1);
const CORE1_TIMEOUT: Duration = Duration::from_secs(10);
const CORE1_STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

static CORE1_CHECK_INS: AtomicU32 = AtomicU32::new(0);
/// In ticks, `u64::MAX` when none
static CONFIRM_DEADLINE: AtomicU64 = AtomicU64::new(u64::MAX);

/// Call first thing on core0: the bootloader's watchdog is already running
pub fn initialize(spawner: Spawner, watchdog: Peri<'static, WATCHDOG>) {
    let mut watchdog = Watchdog::new(watchdog);
    watchdog.pause_on_debug(true);
    watchdog.start(TIMEOUT);
    spawner.spawn(watchdog_task(watchdog).unwrap());
}

pub fn core1_check_in() {
    CORE1_CHECK_INS.fetch_add(1, Ordering::Relaxed);
}

/// Panics at `deadline` unless `boot_confirmed` is called first
pub fn require_boot_confirmation(deadline: Instant) {
    CONFIRM_DEADLINE.store(deadline.as_ticks(), Ordering::Relaxed);
}

pub fn boot_confirmed() {
    CONFIRM_DEADLINE.store(u64::MAX, Ordering::Relaxed);
}

#[embassy_executor::task]
async fn watchdog_task(mut watchdog: Watchdog) {
    let mut seen_check_ins = 0;
    let mut core1_seen = Instant::now();
    loop {
        Timer::after(FEED_INTERVAL).await;
        let now = Instant::now();

        let check_ins = CORE1_CHECK_INS.load(Ordering::Relaxed);
        if check_ins != seen_check_ins {
            seen_check_ins = check_ins;
            core1_seen = now;
        }
        let core1_timeout = if check_ins == 0 {
            CORE1_STARTUP_TIMEOUT
        } else {
            CORE1_TIMEOUT
        };
        if now - core1_seen > core1_timeout {
            panic!("Watchdog: core1 stopped checking in");
        }
        if now.as_ticks() >= CONFIRM_DEADLINE.load(Ordering::Relaxed) {
            panic!("OTA: Firmware not confirmed in time");
        }

        watchdog.feed(TIMEOUT);
    }
}
