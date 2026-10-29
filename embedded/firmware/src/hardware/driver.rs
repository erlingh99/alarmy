use crate::alarm::model::Alarm;
use crate::buzzer::{BuzzerDriver, BuzzerError};
use crate::display::{DisplayDriver, DisplayError};
use crate::led::{LedDriver, LedError};

use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_sync::watch::{Receiver, Watch};
use embassy_time::{with_timeout, Duration, TimeoutError};
use smart_leds::RGB8;

const LOCK_TIMEOUT: Duration = Duration::from_millis(100);

pub static HW_STATE: Mutex<CriticalSectionRawMutex, HardwareState> =
    Mutex::new(HardwareState::Idle);
pub static HW_REQUEST: Signal<CriticalSectionRawMutex, HardwareState> = Signal::new();

/// Number of independent subscribers to [`HW_CHANGED`]: core0's [`wait_for_hw_state`] and
/// core1's display render task. Raise this when adding another subscriber.
const HW_CHANGED_CONSUMERS: usize = 2;
static HW_CHANGED: Watch<CriticalSectionRawMutex, (), HW_CHANGED_CONSUMERS> = Watch::new();
pub type HwChangedReceiver = Receiver<'static, CriticalSectionRawMutex, (), HW_CHANGED_CONSUMERS>;

/// Subscribes to `HW_STATE` changes.
pub fn hw_changed_receiver() -> HwChangedReceiver {
    HW_CHANGED
        .receiver()
        .expect("more HW_CHANGED subscribers than HW_CHANGED_CONSUMERS allows")
}

#[derive(Debug, Clone, PartialEq)]
pub enum HardwareError {
    Led(LedError),
    Buzzer,
    Display(DisplayError),
    LockTimeout,
}

impl From<LedError> for HardwareError {
    fn from(e: LedError) -> Self {
        HardwareError::Led(e)
    }
}

impl From<BuzzerError> for HardwareError {
    fn from(_: BuzzerError) -> Self {
        HardwareError::Buzzer
    }
}

impl From<DisplayError> for HardwareError {
    fn from(e: DisplayError) -> Self {
        HardwareError::Display(e)
    }
}

#[derive(Debug, Clone)]
pub enum HardwareState {
    Initialize,
    Alarm((Alarm, u32)),
    Color(RGB8),
    Fire,
    Rainbow,
    Idle,
    Error(HardwareError),
}

enum StateTransition {
    To(HardwareState),
    Stay,
}

pub struct HardwareStateMachine {
    led_driver: LedDriver,
    buzzer_driver: BuzzerDriver,
    display_driver: &'static DisplayDriver,
}

pub fn change_hw_state(new_state: HardwareState) {
    HW_REQUEST.signal(new_state);
}

pub async fn is_hw_err() -> bool {
    with_timeout(LOCK_TIMEOUT, HW_STATE.lock())
        .await
        .map(|guard| matches!(*guard, HardwareState::Error(_)))
        .unwrap_or(true)
}

pub async fn lock_hw_state() -> Result<HardwareState, HardwareError> {
    with_timeout(LOCK_TIMEOUT, HW_STATE.lock())
        .await
        .map(|guard| guard.clone())
        .map_err(|_| HardwareError::LockTimeout)
}

pub async fn wait_for_hw_state(
    predicate: impl Fn(&HardwareState) -> bool,
    timeout_secs: u64,
) -> Result<(), TimeoutError> {
    let mut hw_changed = hw_changed_receiver();
    with_timeout(Duration::from_secs(timeout_secs), async {
        loop {
            if let Ok(guard) = with_timeout(LOCK_TIMEOUT, HW_STATE.lock()).await {
                if predicate(&guard) {
                    break;
                }
                if let HardwareState::Error(_) = *guard {
                    break;
                }
            }
            hw_changed.changed().await;
        }
    })
    .await
}

impl HardwareStateMachine {
    pub fn initialize(
        spawner: Spawner,
        led_driver: LedDriver,
        buzzer_driver: BuzzerDriver,
        display_driver: &'static DisplayDriver,
    ) {
        let hardware = HardwareStateMachine {
            led_driver,
            buzzer_driver,
            display_driver,
        };
        spawner.spawn(state_machine_task(hardware).unwrap());
    }

    async fn update_state(&self, new_state: HardwareState) -> Result<(), HardwareError> {
        with_timeout(LOCK_TIMEOUT, HW_STATE.lock())
            .await
            .map(|mut guard| *guard = new_state)
            .map_err(|_| HardwareError::LockTimeout)?;
        HW_CHANGED.sender().send(());
        Ok(())
    }

    async fn handle_alarm(
        &self,
        alarm: &Alarm,
        elapsed_secs: u32,
    ) -> Result<StateTransition, HardwareError> {
        self.led_driver
            .follow_intensity_curve(
                &alarm.color,
                &alarm.intensity_curve,
                alarm.length,
                elapsed_secs,
            )
            .await?;
        if alarm.play_melody {
            let (led_res, buzzer_res) = join(
                self.led_driver.blink(&alarm.color),
                self.buzzer_driver.play_melody(),
            )
            .await;
            led_res?;
            buzzer_res?;
        } else {
            self.led_driver.blink(&alarm.color).await?;
        }
        Ok(StateTransition::To(HardwareState::Idle))
    }

    async fn handle_initialize(&self) -> Result<StateTransition, HardwareError> {
        log::info!("Testing LED driver...");
        self.led_driver.run_wheel_once().await?;
        log::info!("Testing BUZZER driver...");
        self.buzzer_driver.play_short_melody().await?;
        log::info!("Testing DISPLAY driver...");
        self.display_driver.self_test().await?;
        log::info!("All OK");
        Ok(StateTransition::To(HardwareState::Idle))
    }

    async fn handle_color(&self, color: RGB8) -> Result<StateTransition, HardwareError> {
        self.buzzer_driver.off()?;
        self.led_driver.color(color).await?;
        Ok(StateTransition::Stay)
    }

    async fn handle_fire(&self) -> Result<StateTransition, HardwareError> {
        self.buzzer_driver.off()?;
        self.led_driver.fire().await?;
        Ok(StateTransition::To(HardwareState::Idle))
    }

    async fn handle_rainbow(&self) -> Result<StateTransition, HardwareError> {
        self.buzzer_driver.off()?;
        self.led_driver.rainbow().await?;
        Ok(StateTransition::To(HardwareState::Idle))
    }

    async fn handle_idle(&self) -> Result<StateTransition, HardwareError> {
        self.led_driver.off().await?;
        self.buzzer_driver.off()?;
        Ok(StateTransition::Stay)
    }

    /// Main state machine loop.
    ///
    /// # Cancellation contract
    ///
    /// Every state handler (`handle_alarm`, `handle_fire`, `handle_rainbow`, …) runs as one
    /// branch of `select(handler_future, HW_REQUEST.wait())`.  When a new state is
    /// requested via `change_hw_state()`, the signal fires and the losing branch (the current
    /// handler) is **dropped** at its next `.await` point — within ≤50 ms for `fire`, ≤20 ms
    /// for `rainbow`, and at note-boundary granularity for `alarm` melody.
    async fn run(&self) -> ! {
        loop {
            let future = async {
                let state = lock_hw_state().await.unwrap_or(HardwareState::Idle);

                let result = match state {
                    HardwareState::Alarm((alarm, elapsed_secs)) => {
                        self.handle_alarm(&alarm, elapsed_secs).await
                    }
                    HardwareState::Initialize => self.handle_initialize().await,
                    HardwareState::Color(color) => self.handle_color(color).await,
                    HardwareState::Fire => self.handle_fire().await,
                    HardwareState::Rainbow => self.handle_rainbow().await,
                    HardwareState::Idle => self.handle_idle().await,
                    HardwareState::Error(_) => Ok(StateTransition::Stay),
                };

                match result {
                    Ok(next_state) => next_state,
                    Err(e) => {
                        log::error!("Hardware operation failed: {e:?}");
                        StateTransition::To(HardwareState::Error(e))
                    }
                }
            };

            let next_state = match select(future, HW_REQUEST.wait()).await {
                Either::First(StateTransition::To(state)) => state,
                Either::First(StateTransition::Stay) => HW_REQUEST.wait().await,
                Either::Second(state) => state,
            };

            let _ = self.update_state(next_state).await;
        }
    }
}

#[embassy_executor::task]
async fn state_machine_task(state_machine: HardwareStateMachine) -> ! {
    state_machine.run().await
}
