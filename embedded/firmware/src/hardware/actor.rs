use crate::alarm::model::Alarm;
use crate::buzzer::{BuzzerDriver, BuzzerError};
use crate::display::{DisplayError, DisplayHandle};
use crate::led::{LedDriver, LedError};
use crate::mailbox::{Mailbox, MailboxReceiver, MailboxSender, ReplyTo, Responder};

use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::watch::{Receiver, Watch};
use embassy_time::{with_timeout, Duration};
use smart_leds::RGB8;

const LOCK_TIMEOUT: Duration = Duration::from_millis(100);

static HW_STATE: Mutex<CriticalSectionRawMutex, HardwareState> = Mutex::new(HardwareState::Idle);

/// Number of independent subscribers to [`HW_CHANGED`]: core1's display actor. Raise this when
/// adding another subscriber.
const HW_CHANGED_CONSUMERS: usize = 1;
static HW_CHANGED: Watch<CriticalSectionRawMutex, (), HW_CHANGED_CONSUMERS> = Watch::new();
pub type HwChangedReceiver = Receiver<'static, CriticalSectionRawMutex, (), HW_CHANGED_CONSUMERS>;

/// Subscribes to `HW_STATE` changes.
pub fn hw_changed_receiver() -> HwChangedReceiver {
    HW_CHANGED
        .receiver()
        .expect("more HW_CHANGED subscribers than HW_CHANGED_CONSUMERS allows")
}

static MAILBOX: Mailbox<HardwareMsg> = Mailbox::new();
static SELF_TEST_REPLY: ReplyTo<Result<(), HardwareError>> = ReplyTo::new();
static ALARM_REPLY: ReplyTo<Result<(), HardwareError>> = ReplyTo::new();

/// Answered when the requested state has run to completion
type Done = Responder<Result<(), HardwareError>>;

#[derive(Debug)]
enum HardwareMsg {
    SelfTest {
        respond_to: Done,
    },
    PlayAlarm {
        alarm: Alarm,
        elapsed_secs: u32,
        respond_to: Done,
    },
    Idle,
    Color(RGB8),
    Fire,
    Rainbow,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HardwareError {
    Led(LedError),
    Buzzer,
    Display(DisplayError),
    LockTimeout,
    /// The hardware actor's mailbox was full
    Unavailable,
    /// The hardware didn't answer in time
    Timeout,
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
    /// The error itself goes to the requester and the log
    Error,
}

enum StateTransition {
    To(HardwareState),
    Stay,
}

#[derive(Clone, Copy)]
pub struct HardwareHandle {
    mailbox: MailboxSender<HardwareMsg>,
}

pub fn handle() -> HardwareHandle {
    HardwareHandle {
        mailbox: MAILBOX.sender(),
    }
}

impl HardwareHandle {
    /// Tests the LEDs, buzzer and display, resolving once they are done
    pub async fn self_test(&self) -> Result<(), HardwareError> {
        self.mailbox
            .request(&SELF_TEST_REPLY, |respond_to| HardwareMsg::SelfTest {
                respond_to,
            })
            .await
            .map_err(|_| HardwareError::Unavailable)?
    }

    /// Resolves when the alarm has finished playing. Never resolves if another command
    /// replaces the alarm first.
    pub async fn play_alarm(&self, alarm: Alarm, elapsed_secs: u32) -> Result<(), HardwareError> {
        self.mailbox
            .request(&ALARM_REPLY, |respond_to| HardwareMsg::PlayAlarm {
                alarm,
                elapsed_secs,
                respond_to,
            })
            .await
            .map_err(|_| HardwareError::Unavailable)?
    }

    pub fn idle(&self) {
        self.mailbox.post_or_warn(HardwareMsg::Idle);
    }

    pub fn color(&self, color: RGB8) {
        self.mailbox.post_or_warn(HardwareMsg::Color(color));
    }

    pub fn fire(&self) {
        self.mailbox.post_or_warn(HardwareMsg::Fire);
    }

    pub fn rainbow(&self) {
        self.mailbox.post_or_warn(HardwareMsg::Rainbow);
    }
}

struct HardwareActor {
    led_driver: LedDriver,
    buzzer_driver: BuzzerDriver,
    display: DisplayHandle,
    mailbox: MailboxReceiver<HardwareMsg>,
    state: HardwareState,
    /// The requester waiting for the current state to finish, if any
    respond_to: Option<Done>,
}

pub async fn lock_hw_state() -> Result<HardwareState, HardwareError> {
    with_timeout(LOCK_TIMEOUT, HW_STATE.lock())
        .await
        .map(|guard| guard.clone())
        .map_err(|_| HardwareError::LockTimeout)
}

pub fn spawn(
    spawner: Spawner,
    led_driver: LedDriver,
    buzzer_driver: BuzzerDriver,
    display: DisplayHandle,
) {
    let actor = HardwareActor {
        led_driver,
        buzzer_driver,
        display,
        mailbox: MAILBOX.receiver(),
        state: HardwareState::Idle,
        respond_to: None,
    };
    spawner.spawn(hardware_task(actor).unwrap());
}

impl HardwareActor {
    /// Main loop.
    ///
    /// # Cancellation contract
    ///
    /// Every state handler (`handle_alarm`, `handle_fire`, `handle_rainbow`, …) runs as one
    /// branch of `select(handler_future, mailbox.receive())`. When a message arrives, the losing
    /// branch (the current handler) is **dropped** at its next `.await` point — within ≤50 ms for
    /// `fire`, ≤20 ms for `rainbow`, and at note-boundary granularity for `alarm` melody. The
    /// dropped handler's requester gets no answer.
    async fn run(&mut self) -> ! {
        loop {
            self.publish_state().await;
            let next_state = match select(self.run_state(), self.mailbox.receive()).await {
                Either::First(Ok(StateTransition::To(state))) => {
                    self.respond(Ok(()));
                    state
                }
                Either::First(Ok(StateTransition::Stay)) => {
                    let msg = self.mailbox.receive().await;
                    self.handle_message(msg)
                }
                Either::First(Err(e)) => {
                    log::error!("Hardware operation failed: {e:?}");
                    self.respond(Err(e));
                    HardwareState::Error
                }
                Either::Second(msg) => self.handle_message(msg),
            };
            self.state = next_state;
        }
    }

    fn handle_message(&mut self, msg: HardwareMsg) -> HardwareState {
        self.respond_to = None;
        match msg {
            HardwareMsg::SelfTest { respond_to } => {
                self.respond_to = Some(respond_to);
                HardwareState::Initialize
            }
            HardwareMsg::PlayAlarm {
                alarm,
                elapsed_secs,
                respond_to,
            } => {
                self.respond_to = Some(respond_to);
                HardwareState::Alarm((alarm, elapsed_secs))
            }
            HardwareMsg::Idle => HardwareState::Idle,
            HardwareMsg::Color(color) => HardwareState::Color(color),
            HardwareMsg::Fire => HardwareState::Fire,
            HardwareMsg::Rainbow => HardwareState::Rainbow,
        }
    }

    fn respond(&mut self, result: Result<(), HardwareError>) {
        if let Some(respond_to) = self.respond_to.take() {
            respond_to.send(result);
        }
    }

    async fn run_state(&self) -> Result<StateTransition, HardwareError> {
        match &self.state {
            HardwareState::Alarm((alarm, elapsed_secs)) => {
                self.handle_alarm(alarm, *elapsed_secs).await
            }
            HardwareState::Initialize => self.handle_initialize().await,
            HardwareState::Color(color) => self.handle_color(*color).await,
            HardwareState::Fire => self.handle_fire().await,
            HardwareState::Rainbow => self.handle_rainbow().await,
            HardwareState::Idle => self.handle_idle().await,
            HardwareState::Error => Ok(StateTransition::Stay),
        }
    }

    /// Lets the display show the current state
    async fn publish_state(&self) {
        if let Ok(mut guard) = with_timeout(LOCK_TIMEOUT, HW_STATE.lock()).await {
            *guard = self.state.clone();
            HW_CHANGED.sender().send(());
        }
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
        self.display.self_test().await?;
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
}

#[embassy_executor::task]
async fn hardware_task(mut actor: HardwareActor) -> ! {
    actor.run().await
}
