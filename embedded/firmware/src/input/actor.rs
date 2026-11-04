//! Decides what button presses and backend commands mean, which can depend on the state of the
//! other actors.

use embassy_executor::Spawner;
use smart_leds::RGB8;

use crate::alarm::AlarmHandle;
use crate::display::DisplayHandle;
use crate::mailbox::{Mailbox, MailboxReceiver, MailboxSender};
use crate::ota::OtaHandle;
use crate::state_machine::{RunState, StateHandle};

static MAILBOX: Mailbox<InputEvent> = Mailbox::new();

#[derive(Debug, Clone, Copy)]
pub enum ButtonId {
    Cancel,
    Fetch,
    DisplayToggle,
}

#[derive(Debug, Clone, Copy)]
pub enum RemoteCommand {
    AlarmsUpdated,
    CheckUpdate,
    Idle,
    Fire,
    Rainbow,
    Color(RGB8),
}

#[derive(Debug)]
enum InputEvent {
    Button(ButtonId),
    Remote(RemoteCommand),
}

#[derive(Debug)]
pub struct InputQueueFull;

#[derive(Clone, Copy)]
pub struct InputHandle {
    mailbox: MailboxSender<InputEvent>,
}

pub fn handle() -> InputHandle {
    InputHandle {
        mailbox: MAILBOX.sender(),
    }
}

impl InputHandle {
    pub fn button_pressed(&self, button: ButtonId) {
        self.mailbox.post_or_warn(InputEvent::Button(button));
    }

    pub fn remote_command(&self, command: RemoteCommand) -> Result<(), InputQueueFull> {
        self.mailbox
            .post(InputEvent::Remote(command))
            .map_err(|_| InputQueueFull)
    }
}

struct InputActor {
    mailbox: MailboxReceiver<InputEvent>,
    state: StateHandle,
    alarms: AlarmHandle,
    display: DisplayHandle,
    ota: OtaHandle,
}

pub fn spawn(
    spawner: Spawner,
    state: StateHandle,
    alarms: AlarmHandle,
    display: DisplayHandle,
    ota: OtaHandle,
) {
    let actor = InputActor {
        mailbox: MAILBOX.receiver(),
        state,
        alarms,
        display,
        ota,
    };
    spawner.spawn(input_task(actor).unwrap());
}

impl InputActor {
    async fn run(&mut self) -> ! {
        loop {
            match self.mailbox.receive().await {
                InputEvent::Button(button) => self.handle_press(button),
                InputEvent::Remote(command) => self.handle_command(command),
            }
        }
    }

    fn handle_press(&mut self, button: ButtonId) {
        match button {
            //TODO need to make sure the an alarm is not restarted next tick after cancel
            ButtonId::Cancel => self.state.request(RunState::Idle),
            ButtonId::Fetch => {
                log::info!("Manual alarm fetch requested");
                self.alarms.refresh();
            }
            ButtonId::DisplayToggle => self.display.toggle(),
        }
    }

    fn handle_command(&mut self, command: RemoteCommand) {
        log::info!("Command received {command:?}");
        match command {
            RemoteCommand::AlarmsUpdated => self.alarms.refresh(),
            RemoteCommand::CheckUpdate => self.ota.check_for_update(),
            RemoteCommand::Idle => self.state.request(RunState::Idle),
            RemoteCommand::Fire => self.state.request(RunState::Fire),
            RemoteCommand::Rainbow => self.state.request(RunState::Rainbow),
            RemoteCommand::Color(c) => self.state.request(RunState::Color(c)),
        }
    }
}

#[embassy_executor::task]
async fn input_task(mut actor: InputActor) -> ! {
    actor.run().await
}
