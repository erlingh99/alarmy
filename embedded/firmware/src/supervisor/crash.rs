//! Crash handling: record the crash in RAM, reset, and report it on the next boot.

use core::fmt::{self, Write};
use core::panic::PanicInfo;
use core::{mem, ptr, slice};

use cortex_m::peripheral::SCB;
use cortex_m_rt::{ExceptionFrame, exception};
use embassy_executor::Spawner;
use embassy_rp::Peri;
use embassy_rp::clocks;
use embassy_rp::gpio::{AnyPin, Level, Output};
use embassy_rp::pac;
use embassy_time::Timer;
use portable_atomic::{AtomicBool, AtomicU32, Ordering};

use crate::config::flash_config::{PANIC_LOG_ADDR, PANIC_LOG_SIZE};
use crate::logging::TruncatingWriter;
use crate::net::HttpClient;

const COUNT_MAGIC: u32 = 0xC0DE_0000;
/// Set in scratch 1 while running, so a stale watchdog reset reason isn't counted as a crash
const RUNNING_MAGIC: u32 = 0xB007_A555;
const MAX_CONSECUTIVE_PANICS: u32 = 5;
const LED_HOLD_S: u32 = 2;
const STABLE_UPTIME_S: u64 = 60;

const RECORD_MAGIC: u32 = 0xDEAD_BEEF;
/// Fits one 256 byte HTTP log line
const MESSAGE_LEN: usize = 200;

/// Only accessed in init, before core1 starts, or inside a critical section
static mut PANIC_LED: Option<Output<'static>> = None;
/// 1-based core handling a crash, 0 if none
static CRASHED_CORE: AtomicU32 = AtomicU32::new(0);
static WATCHDOG_RESET: AtomicBool = AtomicBool::new(false);

#[repr(C)]
struct CrashRecord {
    magic: u32,
    /// FNV-1a over the rest of the record, so leftover RAM is not read as a record
    checksum: u32,
    core: u32,
    message_len: u32,
    message: [u8; MESSAGE_LEN],
}

const _: () = assert!(mem::size_of::<CrashRecord>() <= PANIC_LOG_SIZE);
const _: () = assert!(mem::size_of::<CrashRecord>().is_multiple_of(4));

impl CrashRecord {
    /// Pointer to the crash record stored in `PANIC_LOG` (see memory_layout.toml)
    const PTR: *mut Self = PANIC_LOG_ADDR as *mut Self;

    /// FNV-1a over every word after `checksum`
    fn checksum(&self) -> u32 {
        let words = unsafe {
            slice::from_raw_parts(
                (self as *const Self as *const u32).add(2),
                mem::size_of::<Self>() / 4 - 2,
            )
        };
        // standard FNV seed and prime
        words.iter().fold(0x811C_9DC5, |hash, word| {
            (hash ^ word).wrapping_mul(0x0100_0193)
        })
    }

    fn is_valid(&self) -> bool {
        self.magic == RECORD_MAGIC
            && self.checksum == self.checksum()
            && self.message_len as usize <= MESSAGE_LEN
    }

    fn message(&self) -> &str {
        core::str::from_utf8(&self.message[..self.message_len as usize])
            .unwrap_or("<invalid message>")
    }
}

// Scratch 0-3 are free for applications (2-3 hold the OTA record, see ota/record.rs); the bootrom
// uses 4-7 for its reboot handshake.
fn panic_count() -> u32 {
    let value = pac::WATCHDOG.scratch0().read();
    if value & 0xFFFF_0000 == COUNT_MAGIC {
        value & 0xFFFF
    } else {
        0
    }
}

fn set_panic_count(count: u32) {
    pac::WATCHDOG.scratch0().write_value(COUNT_MAGIC | count);
}

unsafe fn set_panic_led(on: bool) {
    unsafe {
        #[allow(static_mut_refs)]
        if let Some(led) = &mut PANIC_LED {
            led.set_level(on.into());
        }
    }
}

/// Must run before core1 is started. Halts in a crash loop.
pub fn init(panic_pinning: Peri<'static, AnyPin>) {
    let led = Output::new(panic_pinning, Level::Low);
    unsafe {
        #[allow(static_mut_refs)]
        PANIC_LED.replace(led);
    }

    let was_running = pac::WATCHDOG.scratch1().read() == RUNNING_MAGIC;
    pac::WATCHDOG.scratch1().write_value(RUNNING_MAGIC);
    if pac::WATCHDOG.reason().read().timer() && was_running {
        WATCHDOG_RESET.store(true, Ordering::Relaxed);
        set_panic_count(panic_count() + 1);
    }

    let panics = panic_count();
    unsafe { set_panic_led(panics > 0) };
    if panics >= MAX_CONSECUTIVE_PANICS {
        // The bootloader leaves the watchdog running
        pac::WATCHDOG.ctrl().modify(|w| w.set_enable(false));
        loop {
            cortex_m::asm::wfi();
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    match info.location() {
        Some(location) => fatal(format_args!("panicked at {location}: {}", info.message())),
        None => fatal(format_args!("panicked: {}", info.message())),
    }
}

/// Resolve the addresses with `addr2line -e firmware`
#[exception]
unsafe fn HardFault(frame: &ExceptionFrame) -> ! {
    fatal(format_args!(
        "hard fault at pc {:#010x}, lr {:#010x}",
        frame.pc(),
        frame.lr()
    ))
}

fn fatal(message: fmt::Arguments) -> ! {
    cortex_m::interrupt::disable();

    let core = pac::SIO.cpuid().read() + 1;
    // Reentrant on the same core, so a crash inside a critical section doesn't deadlock here.
    // Released before logging, which needs it.
    let claimed = critical_section::with(|_| {
        let claimed = CRASHED_CORE.compare_exchange(0, core, Ordering::AcqRel, Ordering::Relaxed);
        if claimed.is_ok() {
            unsafe {
                set_panic_led(true);
            }
            set_panic_count(panic_count() + 1);
        }
        claimed
    });
    match claimed {
        Ok(_) => {}
        Err(crashed) if crashed == core => reset(),
        // The other core is resetting, reset anyway in case it is blocked on us
        Err(_) => {
            cortex_m::asm::delay(clocks::clk_sys_freq() * (LED_HOLD_S + 1));
            reset();
        }
    }

    let record = unsafe { &mut *CrashRecord::PTR };
    record.core = core - 1;
    let mut writer = TruncatingWriter::new(&mut record.message, Some("..."), None);
    let _ = writer.write_fmt(message);
    record.message_len = writer.finish().len() as u32;
    record.checksum = record.checksum();
    record.magic = RECORD_MAGIC;

    // After the record, so a crash inside the logger still leaves it. Only reaches USB if core1
    // crashed, as core0 sends it.
    log::error!("Crashed on core {}: {}", record.core, record.message());
    cortex_m::asm::delay(clocks::clk_sys_freq() * LED_HOLD_S);
    reset();
}

/// Resets both cores
pub fn reset() -> ! {
    pac::WATCHDOG.scratch1().write_value(0);
    SCB::sys_reset();
}

/// Spawn once the network is up
pub fn spawn_report(spawner: Spawner, http_client: &'static HttpClient) {
    spawner.spawn(crash_report_task(http_client).unwrap());
}

#[embassy_executor::task]
async fn crash_report_task(http_client: &'static HttpClient) {
    let crash = unsafe { ptr::read_volatile(CrashRecord::PTR) };
    let watchdog_reset = WATCHDOG_RESET.load(Ordering::Relaxed);
    if !crash.is_valid() && !watchdog_reset {
        return;
    }
    // The HTTP log drops lines it fails to post
    http_client.wait_for_backend().await;

    if crash.is_valid() {
        log::error!("Crashed on core {}: {}", crash.core, crash.message());
        unsafe { ptr::addr_of_mut!((*CrashRecord::PTR).magic).write_volatile(0) };
    }
    if watchdog_reset {
        log::error!("Reset by the watchdog: core0 hung or a core locked up");
    }
}

/// Spawn at boot, as it clears the crash count after a stable uptime
pub fn spawn_stability(spawner: Spawner) {
    spawner.spawn(stability_task().unwrap());
}

#[embassy_executor::task]
async fn stability_task() {
    let panics = panic_count();
    if panics == 0 {
        return;
    }
    log::warn!("Recovered from a crash ({panics} in a row)");

    Timer::after_secs(STABLE_UPTIME_S).await;
    // Excludes fatal(), so a crash happening now is never cleared
    let cleared = critical_section::with(|_| {
        if CRASHED_CORE.load(Ordering::Acquire) != 0 {
            return false;
        }
        set_panic_count(0);
        unsafe { set_panic_led(false) };
        true
    });
    if cleared {
        log::info!("Running stably again, cleared the crash count");
    }
}
