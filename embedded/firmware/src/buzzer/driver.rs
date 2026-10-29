use embassy_rp::peripherals::{PIN_12, PWM_SLICE6};
use embassy_rp::pwm::{Config, Pwm, SetDutyCycle};
use embassy_rp::{clocks, clocks::RoscRng, Peri};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;

use crate::buzzer::notes::*;
use crate::buzzer::songs::{DIE_WITH_A_SMILE, HYMN_FOR_THE_WEEKEND, SHAPE_OF_YOU};

pub struct BuzzerPinning {
    pub pin: Peri<'static, PIN_12>,
    pub pwm_slice: Peri<'static, PWM_SLICE6>,
}

#[derive(Debug, Clone)]
pub enum BuzzerError {
    InvalidDutyCycle,
    BuzzerRunning,
}

pub struct BuzzerDriver {
    pwm: Mutex<CriticalSectionRawMutex, Pwm<'static>>,
}

impl BuzzerDriver {
    pub fn new(pinning: BuzzerPinning) -> Self {
        let config = Config::default();
        let pwm = Pwm::new_output_a(pinning.pwm_slice, pinning.pin, config.clone());
        let m = Mutex::new(pwm);
        BuzzerDriver { pwm: m }
    }

    pub async fn play_melody(&self) -> Result<(), BuzzerError> {
        let mut pwm = self
            .pwm
            .try_lock()
            .map_err(|_| BuzzerError::BuzzerRunning)?;

        // Define melody: (frequency in Hz, duration in ms)
        let idx = (RoscRng::next_u8() % 3) as usize;

        let melody = [
            &SHAPE_OF_YOU[..],
            &DIE_WITH_A_SMILE[..],
            &HYMN_FOR_THE_WEEKEND[..],
        ][idx];

        log::info!(
            "Playing {}...",
            ["Shape of you", "Die with a smile", "Hymn for the weekend"][idx]
        );

        for &(freq, dur_ms) in melody {
            BuzzerDriver::play_tone(&mut pwm, freq, dur_ms)
                .await
                .map_err(|_| BuzzerError::InvalidDutyCycle)?;
            Timer::after_millis((dur_ms as f32 * 0.2) as u64).await; // Short pause between notes
        }
        pwm.set_duty_cycle_fully_off()
            .map_err(|_| BuzzerError::InvalidDutyCycle)?;
        Ok(())
    }

    async fn play_tone(
        pwm: &mut Pwm<'_>,
        frequency: u32,
        duration_ms: u64,
    ) -> Result<(), BuzzerError> {
        if frequency == 0 {
            pwm.set_duty_cycle_fully_off()
                .map_err(|_| BuzzerError::InvalidDutyCycle)?;
            Timer::after_millis(duration_ms).await;
            return Ok(());
        }

        let divider = 128u8;
        let top = clocks::clk_sys_freq() / frequency / divider as u32; // System clock is 125 MHz
        let mut config = Config::default();
        config.top = top as u16;
        config.divider = divider.into();
        pwm.set_config(&config);
        pwm.set_duty_cycle_percent(50)
            .map_err(|_| BuzzerError::InvalidDutyCycle)?; // 50% duty cycle
        Timer::after_millis(duration_ms).await;
        pwm.set_duty_cycle_fully_off()
            .map_err(|_| BuzzerError::InvalidDutyCycle)?; // Stop the sound
        Ok(())
    }

    pub async fn play_short_melody(&self) -> Result<(), BuzzerError> {
        let melody = &[
            (NOTE_A4, 500),
            (NOTE_B4, 500),
            (NOTE_C5, 500),
            (NOTE_D5, 500),
            (NOTE_E5, 500),
            (NOTE_F5, 500),
            (NOTE_G5, 500),
        ];

        let mut pwm = self
            .pwm
            .try_lock()
            .map_err(|_| BuzzerError::BuzzerRunning)?;

        for &(freq, dur_ms) in melody {
            BuzzerDriver::play_tone(&mut pwm, freq, dur_ms).await?;
            Timer::after_millis(100).await; // Short pause between notes
        }
        pwm.set_duty_cycle_fully_off()
            .map_err(|_| BuzzerError::InvalidDutyCycle)?;
        Ok(())
    }

    pub fn off(&self) -> Result<(), BuzzerError> {
        let mut pwm = self
            .pwm
            .try_lock()
            .map_err(|_| BuzzerError::BuzzerRunning)?;

        pwm.set_duty_cycle_fully_off()
            .map_err(|_| BuzzerError::InvalidDutyCycle)
    }
}
