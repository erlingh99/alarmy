use embassy_rp::{
    gpio::{AnyPin, Input, Level, Pull},
    Peri,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Timer};

pub struct ButtonPinning {
    pub cancel: Peri<'static, AnyPin>,
    pub fetch: Peri<'static, AnyPin>,
    pub display_toggle: Peri<'static, AnyPin>,
}

pub struct Button {
    input: Input<'static>,
    debounce_wait: Duration,
}

impl Button {
    pub fn new(pin: Peri<'static, AnyPin>, debounce_wait: Duration) -> Self {
        Self {
            input: Input::new(pin, Pull::Down),
            debounce_wait,
        }
    }

    pub async fn debounce(&mut self) {
        loop {
            self.input.wait_for_falling_edge().await;
            Timer::after(self.debounce_wait).await;
            if self.input.get_level() == Level::Low {
                return;
            }
        }
    }
}

#[embassy_executor::task(pool_size = 3)]
pub async fn button_task(
    mut button: Button,
    signal: &'static Signal<CriticalSectionRawMutex, ()>,
) -> ! {
    loop {
        button.debounce().await;
        log::debug!("button press");
        signal.signal(());
        Timer::after_millis(500).await; //don't register double presses
    }
}
