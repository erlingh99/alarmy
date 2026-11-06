use embassy_rp::pwm::{Config, Pwm, SetDutyCycle};
use embassy_rp::{clocks, clocks::RoscRng};
use embassy_time::Timer;

use crate::buzzer::notes::*;
use crate::buzzer::songs::{DIE_WITH_A_SMILE, HYMN_FOR_THE_WEEKEND, SHAPE_OF_YOU};

pub struct BuzzerPinning {
    pub pwm: Pwm<'static>,
}

pub struct BuzzerDriver {
    pwm: Pwm<'static>,
}

impl BuzzerDriver {
    pub fn new(pinning: BuzzerPinning) -> Self {
        BuzzerDriver { pwm: pinning.pwm }
    }

    pub async fn play_melody(&mut self) {
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
            self.play_tone(freq, dur_ms).await;
            Timer::after_millis((dur_ms as f32 * 0.2) as u64).await; // Short pause between notes
        }
        self.off();
    }

    async fn play_tone(&mut self, frequency: u32, duration_ms: u64) {
        if frequency == 0 {
            self.off();
            Timer::after_millis(duration_ms).await;
            return;
        }

        let divider = 128u8;
        let top = clocks::clk_sys_freq() / frequency / divider as u32; // System clock is 125 MHz
        let mut config = Config::default();
        config.top = top as u16;
        config.divider = divider.into();
        self.pwm.set_config(&config);
        // Only fails above the PWM's top, which no percentage reaches
        let _ = self.pwm.set_duty_cycle_percent(50);
        Timer::after_millis(duration_ms).await;
        self.off();
    }

    pub async fn play_short_melody(&mut self) {
        let melody = &[
            (NOTE_A4, 500),
            (NOTE_B4, 500),
            (NOTE_C5, 500),
            (NOTE_D5, 500),
            (NOTE_E5, 500),
            (NOTE_F5, 500),
            (NOTE_G5, 500),
        ];

        for &(freq, dur_ms) in melody {
            self.play_tone(freq, dur_ms).await;
            Timer::after_millis(100).await; // Short pause between notes
        }
        self.off();
    }

    pub fn off(&mut self) {
        // A duty cycle of 0 can't fail
        let _ = self.pwm.set_duty_cycle_fully_off();
    }
}
