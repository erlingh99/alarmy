#![no_std]
#![no_main]

mod alarm;
mod button;
mod buzzer;
mod config;
mod display;
mod hardware;
mod input;
mod interrupts;
mod led;
mod logging;
mod mailbox;
mod net;
mod ota;
mod state_machine;
mod supervisor;
mod time_lib;

use embassy_executor::{Executor, Spawner};
use embassy_rp::gpio::{AnyPin, Level, Output};
use embassy_rp::multicore::{spawn_core1, Stack};
use embassy_rp::pwm::{Config as PwmConfig, Pwm};
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use static_cell::StaticCell;

use button::ButtonPinning;
use buzzer::{BuzzerDriver, BuzzerPinning};
use config::display_config::TOGGLE_DEBOUNCE_MS;
use display::DisplayPinning;
use input::ButtonId;
use led::{LedDriver, LedPinning};
use logging::LoggingPinning;
use net::NetworkPinning;
use ota::{OtaActor, OtaPinning};
use supervisor::{crash, watchdog};

const CORE1_STACK_SIZE: usize = 16 * 1024;
static mut CORE1_STACK: Stack<CORE1_STACK_SIZE> = Stack::new();
static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();
static CORE1_START: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = embassy_rp::init(Default::default());

    let network_pinning = NetworkPinning {
        pio_pin: p.PIO0,
        pwr_pin: p.PIN_23,
        cs_pin: p.PIN_25,
        dio_pin: p.PIN_24,
        clk_pin: p.PIN_29,
        dma0: p.DMA_CH0,
    };

    let led_pinning = LedPinning {
        pio_pin: p.PIO1,
        data_pin: p.PIN_16,
        dma: p.DMA_CH1,
    };

    // GP15 is PWM slice 7, channel B
    let buzzer_pinning = BuzzerPinning {
        pwm: Pwm::new_output_b(p.PWM_SLICE7, p.PIN_15, PwmConfig::default()),
    };

    let ota_pinning = OtaPinning {
        flash: p.FLASH,
        download_pin: p.PIN_3.into(),
    };

    let watchdog_pinning = p.WATCHDOG;

    let display_pinning = DisplayPinning {
        i2c: p.I2C1,
        sda: p.PIN_26,
        scl: p.PIN_27,
    };

    let logging_pinning = LoggingPinning { pin: p.USB };
    let button_pinning = ButtonPinning {
        cancel: p.PIN_14.into(),
        fetch: p.PIN_12.into(),
        display_toggle: p.PIN_7.into(),
    };
    let heartbeat_pinning: Peri<'static, AnyPin> = p.PIN_2.into();
    let panic_pinning: Peri<'static, AnyPin> = p.PIN_4.into();

    crash::init(panic_pinning);

    spawn_core1(
        p.CORE1,
        unsafe { &mut *core::ptr::addr_of_mut!(CORE1_STACK) },
        move || {
            let executor1 = EXECUTOR1.init(Executor::new());
            executor1.run(|spawner| {
                spawner.spawn(
                    core1_main(
                        spawner,
                        led_pinning,
                        buzzer_pinning,
                        display_pinning,
                        button_pinning,
                        heartbeat_pinning,
                    )
                    .unwrap(),
                )
            });
        },
    );

    let executor0 = EXECUTOR0.init(Executor::new());
    executor0.run(|spawner| {
        watchdog::spawn(spawner, watchdog_pinning);
        spawner.spawn(core0_main(spawner, logging_pinning, network_pinning, ota_pinning).unwrap())
    });
}

#[embassy_executor::task]
async fn core0_main(
    spawner: Spawner,
    logging_pinning: LoggingPinning,
    network_pinning: NetworkPinning,
    ota_pinning: OtaPinning,
) {
    let http_log_channel = logging::spawn(spawner, logging_pinning).await;

    //need to sync this with core1 so that logging is not done before the logger is setup
    CORE1_START.signal(());

    //Now that the logger is setup, we can log!
    log::info!("Starting Pico W Alarm Clock");
    log::info!(
        "Version: {}.{}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );
    spawner.spawn(crash::stability_task().unwrap());
    let ota = OtaActor::check_boot(ota_pinning);

    let (http_client, stack) = net::spawn(spawner, network_pinning, http_log_channel).await;
    spawner.spawn(crash::crash_report_task(http_client).unwrap());

    let clock = time_lib::spawn(spawner, http_client);
    let alarm_scheduler = alarm::spawn(spawner, http_client, clock, state_machine::handle());
    //TODO being able to fetch a handle to an actor that is not spawned yet is weird
    display::spawn_info_provider(spawner, clock, alarm_scheduler, display::handle());

    ota::spawn(spawner, ota, http_client);
    net::tcp_server::spawn(spawner, stack, input::handle());
    state_machine::spawn(spawner, hardware::handle());
}

#[embassy_executor::task]
async fn core1_main(
    spawner: Spawner,
    led_pinning: LedPinning,
    buzzer_pinning: BuzzerPinning,
    display_pinning: DisplayPinning,
    button_pinning: ButtonPinning,
    heartbeat_pinning: Peri<'static, AnyPin>,
) {
    //core 1 run Hardware and I/O

    CORE1_START.wait().await; //Wait for logging to be up before starting setup

    let led_driver = LedDriver::new(led_pinning);
    let buzzer_driver = BuzzerDriver::new(buzzer_pinning);
    display::spawn(spawner, display_pinning);
    hardware::spawn(spawner, led_driver, buzzer_driver, display::handle());

    input::spawn(
        spawner,
        state_machine::handle(),
        alarm::handle(),
        display::handle(),
        ota::handle(),
    );
    button::spawn(
        spawner,
        button_pinning.cancel,
        Duration::from_millis(20),
        ButtonId::Cancel,
        input::handle(),
    );
    button::spawn(
        spawner,
        button_pinning.fetch,
        Duration::from_millis(20),
        ButtonId::Fetch,
        input::handle(),
    );
    button::spawn(
        spawner,
        button_pinning.display_toggle,
        Duration::from_millis(TOGGLE_DEBOUNCE_MS),
        ButtonId::DisplayToggle,
        input::handle(),
    );

    let mut heartbeat_pin = Output::new(heartbeat_pinning, Level::Low);
    loop {
        heartbeat_pin.toggle();
        watchdog::core1_check_in();
        Timer::after_secs(2).await;
    }
}
