use embassy_executor::Spawner;
use embassy_rp::{
    gpio::{AnyPin, Input, Level, Pull},
    Peri,
};
use embassy_time::{Duration, Timer};

use crate::input::{ButtonId, InputHandle};

pub struct ButtonPinning {
    pub cancel: Peri<'static, AnyPin>,
    pub fetch: Peri<'static, AnyPin>,
    pub display_toggle: Peri<'static, AnyPin>,
}

pub struct Button {
    input: Input<'static>,
    debounce_wait: Duration,
    id: ButtonId,
    presses: InputHandle,
}

pub fn spawn(
    spawner: Spawner,
    pin: Peri<'static, AnyPin>,
    debounce_wait: Duration,
    id: ButtonId,
    presses: InputHandle,
) {
    let button = Button {
        input: Input::new(pin, Pull::Down),
        debounce_wait,
        id,
        presses,
    };
    spawner.spawn(button_task(button).unwrap());
}

impl Button {
    async fn debounce(&mut self) {
        loop {
            self.input.wait_for_falling_edge().await;
            Timer::after(self.debounce_wait).await;
            if self.input.get_level() == Level::Low {
                return;
            }
        }
    }

    async fn run(&mut self) -> ! {
        loop {
            self.debounce().await;
            log::debug!("button press");
            self.presses.button_pressed(self.id);
            Timer::after_millis(500).await; //don't register double presses
        }
    }
}

#[embassy_executor::task(pool_size = 3)]
async fn button_task(mut button: Button) -> ! {
    button.run().await
}
