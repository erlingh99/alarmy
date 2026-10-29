use embassy_rp::clocks::RoscRng;
use embassy_rp::peripherals::{DMA_CH1, PIN_16, PIO1};
use embassy_rp::pio::Pio;
use embassy_rp::pio_programs::ws2812::{Grb, PioWs2812, PioWs2812Program};
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Ticker};
use fastrand;
use smart_leds::RGB8;

use crate::alarm::model::IntensityCurve;
use crate::config::led_config::{BLINK_INTERVAL_S, NUM_BLINKS, NUM_LEDS, UPDATE_INTERVAL_S};
use crate::interrupts::Irqs;

#[derive(Debug, Clone, PartialEq)]
pub enum LedError {
    LedRunning,
}

pub struct LedPinning {
    pub pio_pin: Peri<'static, PIO1>,
    pub data_pin: Peri<'static, PIN_16>,
    pub dma: Peri<'static, DMA_CH1>,
}

pub struct LedDriver {
    ws2812: Mutex<CriticalSectionRawMutex, PioWs2812<'static, PIO1, 1, NUM_LEDS, Grb>>,
}

impl LedDriver {
    pub fn new(led_pinning: LedPinning) -> Self {
        let Pio {
            mut common, sm1, ..
        } = Pio::new(led_pinning.pio_pin, Irqs);

        let program = PioWs2812Program::new(&mut common);
        let ws2812 = PioWs2812::new(
            &mut common,
            sm1,
            led_pinning.dma,
            Irqs,
            led_pinning.data_pin,
            &program,
        );
        let m = Mutex::new(ws2812);
        LedDriver { ws2812: m }
    }

    pub async fn follow_intensity_curve(
        &self,
        color: &RGB8,
        intensity_curve: &IntensityCurve,
        length_min: u8,
        elapsed_secs: u32,
    ) -> Result<(), LedError> {
        let mut ws2812 = self.ws2812.try_lock().map_err(|_e| LedError::LedRunning)?;

        let length_s = length_min as u16 * 60;

        let mut ticker = Ticker::every(Duration::from_secs(UPDATE_INTERVAL_S as u64));

        let num_ticks = length_s / UPDATE_INTERVAL_S;
        let start_tick = (elapsed_secs / UPDATE_INTERVAL_S as u32).min(num_ticks as u32) as u16;

        for i in start_tick..num_ticks {
            let fraction = (i + 1) as f32 / num_ticks as f32;

            let intensity = intensity_curve.intensity(fraction);
            let data = [(
                (color.r as f32 * intensity).min(255.0) as u8,
                (color.g as f32 * intensity).min(255.0) as u8,
                (color.b as f32 * intensity).min(255.0) as u8,
            )
                .into(); NUM_LEDS];

            ws2812.write(&data).await;

            ticker.next().await;
        }
        ws2812.write(&[(0u8, 0u8, 0u8).into(); NUM_LEDS]).await;

        Ok(())
    }

    pub async fn blink(&self, color: &RGB8) -> Result<(), LedError> {
        let mut ws2812 = self.ws2812.try_lock().map_err(|_e| LedError::LedRunning)?;

        let mut blink_ticker = Ticker::every(Duration::from_secs(BLINK_INTERVAL_S));
        // blink LEDS
        for i in 0..NUM_BLINKS {
            let on = i % 2;
            let data = [(color.r * on, color.g * on, color.b * on).into(); NUM_LEDS];
            ws2812.write(&data).await;
            blink_ticker.next().await;
        }
        ws2812.write(&[(0u8, 0u8, 0u8).into(); NUM_LEDS]).await;

        Ok(())
    }

    pub async fn run_wheel_once(&self) -> Result<(), LedError> {
        let mut ws2812 = self.ws2812.try_lock().map_err(|_e| LedError::LedRunning)?;
        let mut data = [RGB8::default(); NUM_LEDS];
        let mut ticker = Ticker::every(Duration::from_millis(10));

        //show the LEDs are working
        for j in 0..(256 * 5) {
            for (i, d) in data.iter_mut().enumerate().take(NUM_LEDS) {
                *d =
                    LedDriver::wheel((((i * 256) as u16 / NUM_LEDS as u16 + j as u16) & 255) as u8);
            }
            ws2812.write(&data).await;
            ticker.next().await;
        }
        ws2812.write(&[(0u8, 0u8, 0u8).into(); NUM_LEDS]).await;
        Ok(())
    }

    pub async fn off(&self) -> Result<(), LedError> {
        let mut ws2812 = self.ws2812.try_lock().map_err(|_e| LedError::LedRunning)?;
        ws2812.write(&[(0u8, 0u8, 0u8).into(); NUM_LEDS]).await;
        Ok(())
    }

    pub async fn fire(&self) -> Result<(), LedError> {
        let mut ws2812 = self.ws2812.try_lock().map_err(|_| LedError::LedRunning)?;
        let mut data = [RGB8::default(); NUM_LEDS];
        let mut ticker = Ticker::every(Duration::from_millis(50));
        let seed = RoscRng.next_u64();
        let mut rng = fastrand::Rng::with_seed(seed);
        // Smoothing buffer to reduce flicker intensity
        let mut heat = [0u8; NUM_LEDS];

        loop {
            for (i, d) in data.iter_mut().enumerate().take(NUM_LEDS) {
                // Slower, gentler heat changes
                let base = 80 + i as i32;
                let flicker = rng.i32(-20..=25);
                let new_heat = (base + flicker).clamp(0, 180) as u8;

                // Smooth transition - blend old and new heat values
                heat[i] = ((heat[i] as u16 * 6 + new_heat as u16) / 7) as u8;

                let intensity = heat[i];

                // Warmer, less yellow fire colors
                let (r, g, b) = if intensity < 60 {
                    (intensity * 4, 0, 0)
                } else if intensity < 130 {
                    // Red to orange (less yellow)
                    let progress = intensity - 60;
                    (255, progress * 2, 0)
                } else {
                    // Orange to warm yellow (capped to avoid white)
                    (255, 200, (intensity - 130) / 2)
                };

                *d = RGB8::new(r, g, b);
            }

            ws2812.write(&data).await;
            ticker.next().await;
        }
    }

    pub async fn color(&self, color: RGB8) -> Result<(), LedError> {
        let mut ws2812 = self.ws2812.try_lock().map_err(|_e| LedError::LedRunning)?;
        ws2812.write(&[color; NUM_LEDS]).await;
        Ok(())
    }

    pub async fn rainbow(&self) -> Result<(), LedError> {
        let mut ws2812 = self.ws2812.try_lock().map_err(|_| LedError::LedRunning)?;
        let mut data = [RGB8::default(); NUM_LEDS];
        let mut ticker = Ticker::every(Duration::from_millis(20));
        loop {
            for j in 0..(256 * 5) {
                for (i, d) in data.iter_mut().enumerate().take(NUM_LEDS) {
                    *d = LedDriver::wheel(
                        (((i * 256) as u16 / NUM_LEDS as u16 + j as u16) & 255) as u8,
                    );
                }
                ws2812.write(&data).await;
                ticker.next().await;
            }
        }
    }

    /// Input a value 0 to 255 to get a color value
    /// The colours are a transition r - g - b - back to r.
    fn wheel(mut wheel_pos: u8) -> RGB8 {
        wheel_pos = 255 - wheel_pos;
        if wheel_pos < 85 {
            return (255 - wheel_pos * 3, 0, wheel_pos * 3).into();
        }
        if wheel_pos < 170 {
            wheel_pos -= 85;
            return (0, wheel_pos * 3, 255 - wheel_pos * 3).into();
        }
        wheel_pos -= 170;
        (wheel_pos * 3, 255 - wheel_pos * 3, 0).into()
    }
}
