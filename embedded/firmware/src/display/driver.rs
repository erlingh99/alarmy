use core::fmt::Write as _;

use embassy_rp::Peri;
use embassy_rp::i2c::{Async as I2cAsyncMode, Config as I2cConfig, I2c};
use embassy_rp::peripherals::{I2C1, PIN_26, PIN_27};
use embassy_time::Timer;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::{FONT_6X10, FONT_10X20};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, Line, PrimitiveStyle, Triangle};
use embedded_graphics::text::{Baseline, Text};
use heapless::String;
use ssd1306::mode::{BufferedGraphicsModeAsync, DisplayConfigAsync};
use ssd1306::prelude::{DisplayRotation, DisplaySize128x64, I2CInterface};
use ssd1306::{I2CDisplayInterface, Ssd1306Async};
use time::OffsetDateTime;

use crate::config::display_config::I2C_ADDRESS;
use crate::hardware::HardwareState;
use crate::health::Faults;
use crate::interrupts::Irqs;

type Display = Ssd1306Async<
    I2CInterface<I2c<'static, I2C1, I2cAsyncMode>>,
    DisplaySize128x64,
    BufferedGraphicsModeAsync<DisplaySize128x64>,
>;

#[derive(Debug, Clone, PartialEq)]
pub enum DisplayError {
    InitFailed,
    I2cError,
    /// The init didn't finish in time
    Timeout,
}

pub struct DisplayPinning {
    pub i2c: Peri<'static, I2C1>,
    pub sda: Peri<'static, PIN_26>,
    pub scl: Peri<'static, PIN_27>,
}

#[derive(Debug, Clone)]
pub(super) struct DisplayInfo {
    pub(super) time: Option<OffsetDateTime>,
    pub(super) next_alarm: Option<(String<32>, time::Time)>,
    pub(super) wifi_connected: bool,
    pub(super) alarms_enabled: bool,
    pub(super) hw_state: HardwareState,
    pub(super) faults: Faults,
}

pub(super) struct DisplayDriver {
    display: Display,
}

impl DisplayDriver {
    pub(super) fn new(pinning: DisplayPinning) -> Self {
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

        DisplayDriver { display }
    }

    pub(super) async fn init(&mut self) -> Result<(), DisplayError> {
        self.display
            .init()
            .await
            .map_err(|_| DisplayError::InitFailed)?;

        self.display.clear(BinaryColor::Off).unwrap();
        let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        Text::with_baseline("Alarmy", Point::new(0, 0), style, Baseline::Top)
            .draw(&mut self.display)
            .unwrap();
        self.display
            .flush()
            .await
            .map_err(|_| DisplayError::InitFailed)?;

        Timer::after_millis(500).await;
        Ok(())
    }

    pub(super) async fn render(&mut self, info: &DisplayInfo) -> Result<(), DisplayError> {
        self.display.clear(BinaryColor::Off).unwrap();

        let big = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let small = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

        Text::with_baseline(
            &format_clock(info.time),
            Point::new(0, 0),
            big,
            Baseline::Top,
        )
        .draw(&mut self.display)
        .unwrap();
        draw_bell_icon(&mut self.display, info.alarms_enabled);
        if info.faults.any() {
            draw_warning_icon(&mut self.display);
        }
        if info.alarms_enabled {
            Text::with_baseline(
                &format_next_alarm(&info.next_alarm),
                Point::new(0, 22),
                small,
                Baseline::Top,
            )
            .draw(&mut self.display)
            .unwrap();
        }
        Text::with_baseline(
            &format_health(info.wifi_connected, info.faults),
            Point::new(0, 36),
            small,
            Baseline::Top,
        )
        .draw(&mut self.display)
        .unwrap();
        Text::with_baseline(
            &format_status(info.hw_state),
            Point::new(0, 50),
            small,
            Baseline::Top,
        )
        .draw(&mut self.display)
        .unwrap();

        self.display
            .flush()
            .await
            .map_err(|_| DisplayError::I2cError)
    }

    pub(super) async fn blank(&mut self) -> Result<(), DisplayError> {
        self.display.clear(BinaryColor::Off).unwrap();
        self.display
            .flush()
            .await
            .map_err(|_| DisplayError::I2cError)
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

/// The first active fault, else the WiFi status
fn format_health(wifi_connected: bool, faults: Faults) -> String<24> {
    let mut s = String::new();
    let mut active = faults.iter();
    match active.next() {
        Some(fault) => {
            let _ = write!(s, "! {}", fault.label());
            let more = active.count();
            if more > 0 {
                let _ = write!(s, " +{more}");
            }
        }
        None if wifi_connected => {
            let _ = write!(s, "WiFi: OK");
        }
        None => {
            let _ = write!(s, "WiFi: --");
        }
    }
    s
}

fn format_status(state: HardwareState) -> String<24> {
    let label = state.label();
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

/// A filled triangle with a cut out "!", left of the bell
fn draw_warning_icon(display: &mut Display) {
    let fill_on = PrimitiveStyle::with_fill(BinaryColor::On);
    let stroke_off = PrimitiveStyle::with_stroke(BinaryColor::Off, 1);

    Triangle::new(Point::new(104, 1), Point::new(97, 14), Point::new(111, 14))
        .into_styled(fill_on)
        .draw(&mut *display)
        .unwrap();
    Line::new(Point::new(104, 5), Point::new(104, 9))
        .into_styled(stroke_off)
        .draw(&mut *display)
        .unwrap();
    Line::new(Point::new(104, 11), Point::new(104, 12))
        .into_styled(stroke_off)
        .draw(&mut *display)
        .unwrap();
}
