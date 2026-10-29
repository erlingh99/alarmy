use core::fmt::Write as _;

use embassy_executor::Spawner;
use embassy_futures::select::{select3, Either3};
use embassy_rp::i2c::{Async as I2cAsyncMode, Config as I2cConfig, I2c};
use embassy_rp::peripherals::{I2C1, PIN_26, PIN_27};
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::Timer;
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_6X10};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, Line, PrimitiveStyle};
use embedded_graphics::text::{Baseline, Text};
use heapless::String;
use portable_atomic::Ordering;
use ssd1306::mode::{BufferedGraphicsModeAsync, DisplayConfigAsync};
use ssd1306::prelude::{DisplayRotation, DisplaySize128x64, I2CInterface};
use ssd1306::{I2CDisplayInterface, Ssd1306Async};
use static_cell::StaticCell;
use time::OffsetDateTime;

use crate::alarm::AlarmDriver;
use crate::config::display_config::{I2C_ADDRESS, REFRESH_INTERVAL_S};
use crate::hardware::{hw_changed_receiver, lock_hw_state, HardwareState};
use crate::input::InputManager;
use crate::interrupts::Irqs;
use crate::net::WIFI_CONNECTED;
use crate::time_lib::TimeManager;

type Display = Ssd1306Async<
    I2CInterface<I2c<'static, I2C1, I2cAsyncMode>>,
    DisplaySize128x64,
    BufferedGraphicsModeAsync<DisplaySize128x64>,
>;

static DISPLAY_DRIVER: StaticCell<DisplayDriver> = StaticCell::new();
static DISPLAY_INFO: Mutex<CriticalSectionRawMutex, DisplayInfo> = Mutex::new(DisplayInfo::new());
static DISPLAY_INFO_CHANGED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayError {
    InitFailed,
    I2cError,
}

pub struct DisplayPinning {
    pub i2c: Peri<'static, I2C1>,
    pub sda: Peri<'static, PIN_26>,
    pub scl: Peri<'static, PIN_27>,
}

#[derive(Debug, Clone)]
struct DisplayInfo {
    time: Option<OffsetDateTime>,
    next_alarm: Option<(String<32>, time::Time)>,
    wifi_connected: bool,
    alarms_enabled: bool,
}

impl DisplayInfo {
    const fn new() -> Self {
        DisplayInfo {
            time: None,
            next_alarm: None,
            wifi_connected: false,
            alarms_enabled: true,
        }
    }
}

pub struct DisplayDriver {
    display: Mutex<CriticalSectionRawMutex, Display>,
    ready: Signal<CriticalSectionRawMutex, ()>,
}

impl DisplayDriver {
    fn new(pinning: DisplayPinning) -> Self {
        let i2c = I2c::new_async(
            pinning.i2c,
            pinning.scl,
            pinning.sda,
            Irqs,
            I2cConfig::default(),
        );
        let interface = I2CDisplayInterface::new_custom_address(i2c, I2C_ADDRESS);
        let display = Ssd1306Async::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
            .into_buffered_graphics_mode();

        DisplayDriver {
            display: Mutex::new(display),
            ready: Signal::new(),
        }
    }

    pub fn initialize(spawner: Spawner, pinning: DisplayPinning) -> &'static DisplayDriver {
        let driver = DISPLAY_DRIVER.init(DisplayDriver::new(pinning));
        spawner.spawn(display_render_task(driver).unwrap());
        driver
    }

    pub async fn self_test(&self) -> Result<(), DisplayError> {
        let mut display = self.display.lock().await;
        display.init().await.map_err(|_| DisplayError::InitFailed)?;

        display.clear(BinaryColor::Off).unwrap();
        let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        Text::with_baseline("Alarmy", Point::new(0, 0), style, Baseline::Top)
            .draw(&mut *display)
            .unwrap();
        display
            .flush()
            .await
            .map_err(|_| DisplayError::InitFailed)?;
        drop(display);

        Timer::after_millis(500).await;
        self.ready.signal(());
        Ok(())
    }

    async fn render(
        &self,
        info: &DisplayInfo,
        hw_state: &HardwareState,
    ) -> Result<(), DisplayError> {
        let mut display = self.display.lock().await;
        display.clear(BinaryColor::Off).unwrap();

        let big = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let small = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

        Text::with_baseline(
            &format_clock(info.time),
            Point::new(0, 0),
            big,
            Baseline::Top,
        )
        .draw(&mut *display)
        .unwrap();
        draw_bell_icon(&mut display, info.alarms_enabled);
        if info.alarms_enabled {
            Text::with_baseline(
                &format_next_alarm(&info.next_alarm),
                Point::new(0, 22),
                small,
                Baseline::Top,
            )
            .draw(&mut *display)
            .unwrap();
        }
        Text::with_baseline(
            if info.wifi_connected {
                "WiFi: OK"
            } else {
                "WiFi: --"
            },
            Point::new(0, 36),
            small,
            Baseline::Top,
        )
        .draw(&mut *display)
        .unwrap();
        Text::with_baseline(
            &format_status(hw_state),
            Point::new(0, 50),
            small,
            Baseline::Top,
        )
        .draw(&mut *display)
        .unwrap();

        display.flush().await.map_err(|_| DisplayError::I2cError)
    }

    async fn blank(&self) -> Result<(), DisplayError> {
        let mut display = self.display.lock().await;
        display.clear(BinaryColor::Off).unwrap();
        display.flush().await.map_err(|_| DisplayError::I2cError)
    }
}

fn format_clock(time: Option<OffsetDateTime>) -> String<8> {
    let mut s = String::new();
    match time {
        Some(t) => {
            let _ = write!(s, "{:02}:{:02}", t.hour(), t.minute());
        }
        None => {
            let _ = write!(s, "--:--");
        }
    }
    s
}

fn format_next_alarm(next_alarm: &Option<(String<32>, time::Time)>) -> String<32> {
    let mut s = String::new();
    match next_alarm {
        Some((name, time)) => {
            let _ = write!(s, "Next: {:02}:{:02} ", time.hour(), time.minute());
            for c in name.chars().take(12) {
                let _ = s.push(c);
            }
        }
        None => {
            let _ = write!(s, "No alarms");
        }
    }
    s
}

fn format_status(state: &HardwareState) -> String<24> {
    let label = match state {
        HardwareState::Initialize => "Init...",
        HardwareState::Idle => "Idle",
        HardwareState::Alarm(_) => "Alarm",
        HardwareState::Color(_) => "Color",
        HardwareState::Fire => "Fire",
        HardwareState::Rainbow => "Rainbow",
        HardwareState::Error(_) => "Error",
    };
    let mut s = String::new();
    let _ = write!(s, "State: {label}");
    s
}

fn draw_bell_icon(display: &mut Display, enabled: bool) {
    let fill_on = PrimitiveStyle::with_fill(BinaryColor::On);
    let stroke_on = PrimitiveStyle::with_stroke(BinaryColor::On, 2);

    Circle::new(Point::new(116, 3), 8)
        .into_styled(fill_on)
        .draw(&mut *display)
        .unwrap();
    Line::new(Point::new(114, 10), Point::new(126, 10))
        .into_styled(stroke_on)
        .draw(&mut *display)
        .unwrap();
    Circle::new(Point::new(119, 12), 2)
        .into_styled(fill_on)
        .draw(&mut *display)
        .unwrap();

    if !enabled {
        let stroke_off = PrimitiveStyle::with_stroke(BinaryColor::Off, 2);
        Line::new(Point::new(113, 1), Point::new(127, 15))
            .into_styled(stroke_off)
            .draw(&mut *display)
            .unwrap();
    }
}

#[embassy_executor::task]
async fn display_render_task(driver: &'static DisplayDriver) -> ! {
    driver.ready.wait().await;

    let display_toggle_signal = InputManager::get_display_toggle_signal();
    let mut hw_changed = hw_changed_receiver();

    let mut on = true;
    loop {
        let woken_by_toggle = matches!(
            select3(
                DISPLAY_INFO_CHANGED.wait(),
                hw_changed.changed(),
                display_toggle_signal.wait(),
            )
            .await,
            Either3::Third(_)
        );

        if woken_by_toggle {
            on = !on;
            if !on {
                if let Err(e) = driver.blank().await {
                    log::warn!("Display blank failed: {e:?}");
                }
                continue;
            }
        }

        if !on {
            continue;
        }

        let info = DISPLAY_INFO.lock().await.clone();
        let hw_state = lock_hw_state().await.unwrap_or(HardwareState::Idle);
        if let Err(e) = driver.render(&info, &hw_state).await {
            log::warn!("Display render failed: {e:?}");
        }
    }
}

/// Polls the core0-owned time/alarm/network state at a low cadence and publishes it for
/// `display_render_task` (on core1) to pick up. Hardware/alarm state is not duplicated here -
/// the render task reads that directly via `lock_hw_state()`.
#[embassy_executor::task]
pub async fn display_info_publisher(
    time_client: &'static TimeManager,
    alarm_driver: &'static AlarmDriver,
) -> ! {
    loop {
        let time = time_client.get_local_time().await.ok();
        let next_alarm = alarm_driver
            .get_next_alarm()
            .await
            .ok()
            .map(|alarm| (alarm.name, alarm.time));
        let wifi_connected = WIFI_CONNECTED.load(Ordering::Relaxed);
        let alarms_enabled = alarm_driver.is_enabled();

        {
            let mut guard = DISPLAY_INFO.lock().await;
            *guard = DisplayInfo {
                time,
                next_alarm,
                wifi_connected,
                alarms_enabled,
            };
        }
        DISPLAY_INFO_CHANGED.signal(());

        Timer::after_secs(REFRESH_INTERVAL_S).await;
    }
}
