use embassy_executor::Spawner;
use embassy_futures::select::{select3, Either3};
use embassy_time::{Duration, Instant, Timer};

use super::driver::{DisplayDriver, DisplayError, DisplayInfo, DisplayPinning};
use super::info_provider::{self, InfoProviderHandle};
use crate::config::display_config::REFRESH_INTERVAL_S;
use crate::hardware::{hw_changed_receiver, lock_hw_state, HardwareState};
use crate::mailbox::{Mailbox, MailboxReceiver, MailboxSender, ReplyTo, Responder};

static MAILBOX: Mailbox<DisplayMsg> = Mailbox::new();

static SELF_TEST_REPLY: ReplyTo<Result<(), DisplayError>> = ReplyTo::new();

#[derive(Debug)]
enum DisplayMsg {
    Toggle,
    Info(DisplayInfo),
    SelfTest {
        respond_to: Responder<Result<(), DisplayError>>,
    },
}

#[derive(Clone, Copy)]
pub struct DisplayHandle {
    mailbox: MailboxSender<DisplayMsg>,
}

pub fn handle() -> DisplayHandle {
    DisplayHandle {
        mailbox: MAILBOX.sender(),
    }
}

impl DisplayHandle {
    /// Turns the screen off, or back on
    pub fn toggle(&self) {
        self.mailbox.post_or_warn(DisplayMsg::Toggle);
    }

    /// Initialises the screen and shows a splash. Nothing else is drawn until this has run.
    pub async fn self_test(&self) -> Result<(), DisplayError> {
        self.mailbox
            .request(&SELF_TEST_REPLY, |respond_to| DisplayMsg::SelfTest {
                respond_to,
            })
            .await
            .map_err(|_| DisplayError::Unavailable)?
    }

    pub(super) fn show_info(&self, info: DisplayInfo) {
        self.mailbox.post_or_warn(DisplayMsg::Info(info));
    }
}

struct DisplayActor {
    driver: DisplayDriver,
    mailbox: MailboxReceiver<DisplayMsg>,
    provider: InfoProviderHandle,
    initialized: bool,
    on: bool,
    /// Set by the provider's first info, so nothing is requested before it runs
    provider_ready: bool,
    info: DisplayInfo,
    next_refresh: Instant,
}

pub fn spawn(spawner: Spawner, pinning: DisplayPinning) {
    let actor = DisplayActor {
        driver: DisplayDriver::new(pinning),
        mailbox: MAILBOX.receiver(),
        provider: info_provider::handle(),
        initialized: false,
        on: true,
        provider_ready: false,
        info: DisplayInfo::new(),
        next_refresh: Instant::now(),
    };
    spawner.spawn(display_task(actor).unwrap());
}

impl DisplayActor {
    async fn run(&mut self) -> ! {
        let mut hw_changed = hw_changed_receiver();
        loop {
            // While off, only a message can change anything, so nothing is requested or redrawn
            if !self.on {
                let msg = self.mailbox.receive().await;
                self.handle_message(msg).await;
                continue;
            }
            match select3(
                self.mailbox.receive(),
                hw_changed.changed(),
                self.refresh_due(),
            )
            .await
            {
                Either3::First(msg) => self.handle_message(msg).await,
                Either3::Second(()) => self.render().await,
                Either3::Third(()) => self.request_info(),
            }
        }
    }

    async fn handle_message(&mut self, msg: DisplayMsg) {
        match msg {
            DisplayMsg::Toggle => {
                self.on = !self.on;
                if !self.on {
                    self.blank().await;
                    return;
                }
                self.request_info();
            }
            DisplayMsg::Info(info) => {
                self.info = info;
                if !self.provider_ready {
                    self.provider_ready = true;
                    self.next_refresh = Instant::now() + refresh_interval();
                }
            }
            DisplayMsg::SelfTest { respond_to } => {
                let result = self.driver.self_test().await;
                self.initialized = result.is_ok();
                respond_to.send(result);
            }
        }
        self.render().await;
    }

    /// Never resolves before the provider is ready
    async fn refresh_due(&self) {
        if self.provider_ready {
            Timer::at(self.next_refresh).await
        } else {
            core::future::pending().await
        }
    }

    fn request_info(&mut self) {
        if self.provider_ready {
            self.provider.request();
            self.next_refresh = Instant::now() + refresh_interval();
        }
    }

    async fn render(&mut self) {
        if !self.on || !self.initialized {
            return;
        }
        let hw_state = lock_hw_state().await.unwrap_or(HardwareState::Idle);
        if let Err(e) = self.driver.render(&self.info, &hw_state).await {
            log::warn!("Display render failed: {e:?}");
        }
    }

    async fn blank(&mut self) {
        if !self.initialized {
            return;
        }
        if let Err(e) = self.driver.blank().await {
            log::warn!("Display blank failed: {e:?}");
        }
    }
}

fn refresh_interval() -> Duration {
    Duration::from_secs(REFRESH_INTERVAL_S)
}

#[embassy_executor::task]
async fn display_task(mut actor: DisplayActor) -> ! {
    actor.run().await
}
