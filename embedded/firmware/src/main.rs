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
mod net;
mod ota;
mod state_machine;
mod time_lib;

use core::panic::PanicInfo;
use embassy_executor::{Executor, Spawner};
use embassy_rp::gpio::{AnyPin, Level, Output};
use embassy_rp::multicore::{spawn_core1, Stack};
use embassy_rp::pwm::{Config as PwmConfig, Pwm};
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use portable_atomic::{AtomicBool, Ordering};
use static_cell::StaticCell;

use alarm::AlarmDriver;
use button::{button_task, Button, ButtonPinning};
use buzzer::{BuzzerDriver, BuzzerPinning};
use config::display_config::TOGGLE_DEBOUNCE_MS;
use display::{display_info_publisher, DisplayDriver, DisplayPinning};
use hardware::HardwareStateMachine;
use input::InputManager;
use led::{LedDriver, LedPinning};
use logging::{LoggingPinning, MultiLogger};
use net::{NetworkManager, NetworkPinning};
use ota::{OtaDriver, OtaPinning};
use state_machine::StateMachine;
use time_lib::TimeManager;

static mut CORE1_STACK: Stack<4096> = Stack::new();
static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();
static mut PANIC_LED: Option<Output> = None;
static PANICKED: AtomicBool = AtomicBool::new(false);
static LOGGER_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    cortex_m::interrupt::disable();

    // Guard against infinite recursion, just in case.
    if !PANICKED.load(Ordering::Relaxed) {
        PANICKED.store(true, Ordering::Relaxed);
        unsafe {
            #[allow(static_mut_refs)]
            if let Some(pin) = &mut PANIC_LED {
                pin.set_high();
            }
        }
    }

    cortex_m::asm::udf();
}

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
        watchdog: p.WATCHDOG,
        download_pin: p.PIN_3.into(),
    };

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
    let heartbeat_pinning: Peri<'_, AnyPin> = p.PIN_2.into();
    let panic_pinning: Peri<'_, AnyPin> = p.PIN_4.into();

    let panic_output = Output::new(panic_pinning, Level::Low);

    unsafe {
        #[allow(static_mut_refs)]
        PANIC_LED.replace(panic_output);
    }

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
    let http_log_channel = MultiLogger::initialize(spawner, logging_pinning).await;

    //need to sync this with core1 so that logging is not done before the logger is setup
    LOGGER_READY.signal(());

    //Now that the logger is setup, we can log!
    log::info!("Starting Pico W Alarm Clock");
    log::info!(
        "Version: {}.{}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    );

    let (http_client, tcp_server) =
        NetworkManager::initialize(spawner, network_pinning, http_log_channel).await;

    let time_client = TimeManager::initialize(spawner, http_client);
    InputManager::initialize(spawner, tcp_server.subscribe());

    let alarm_driver = AlarmDriver::initialize(
        spawner,
        http_client,
        time_client,
        InputManager::get_alarm_update_signal(),
        InputManager::get_alarm_fire_sender(),
    );

    spawner.spawn(display_info_publisher(time_client, alarm_driver).unwrap());

    OtaDriver::initialize(spawner, http_client, ota_pinning);
    StateMachine::new(InputManager::get_state_change_signal())
        .run()
        .await
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

    LOGGER_READY.wait().await; //Wait for logging to be up before starting setup

    let led_driver = LedDriver::new(led_pinning);
    let buzzer_driver = BuzzerDriver::new(buzzer_pinning);
    let display_driver = DisplayDriver::initialize(spawner, display_pinning);
    HardwareStateMachine::initialize(spawner, led_driver, buzzer_driver, display_driver);

    let cancel_button = Button::new(button_pinning.cancel, Duration::from_millis(20));
    spawner.spawn(button_task(cancel_button, InputManager::get_cancel_alarm_signal()).unwrap());
    let fetch_button = Button::new(button_pinning.fetch, Duration::from_millis(20));
    spawner.spawn(button_task(fetch_button, InputManager::get_fetch_request_signal()).unwrap());
    let display_toggle_button = Button::new(
        button_pinning.display_toggle,
        Duration::from_millis(TOGGLE_DEBOUNCE_MS),
    );
    spawner.spawn(
        button_task(
            display_toggle_button,
            InputManager::get_display_toggle_signal(),
        )
        .unwrap(),
    );

    let mut heartbeat_pin = Output::new(heartbeat_pinning, Level::Low);
    loop {
        heartbeat_pin.toggle();
        Timer::after_secs(2).await;
    }
}
