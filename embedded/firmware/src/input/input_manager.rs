use embassy_executor::Spawner;
use embassy_futures::select::{select4, Either4};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    channel::{Channel, Receiver, Sender},
    signal::Signal,
};
use embassy_time::{Duration, Instant};

use crate::alarm::model::Alarm;
use crate::net::TcpCommand;
use crate::ota::OTA_TRIGGER;
use crate::state_machine::types::RunState;

static ALARM_UPDATE_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static ALARM_FIRE_CHANNEL: Channel<CriticalSectionRawMutex, (Alarm, u32), 1> = Channel::new();
static CANCEL_ALARM_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static FETCH_REQUEST_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static STATE_CHANGE_SIGNAL: Signal<CriticalSectionRawMutex, RunState> = Signal::new();
static DISPLAY_TOGGLE_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[derive(Debug)]
pub enum Input {
    Tcp(TcpCommand),
    CancelAlarm,
    FetchAlarms,
    AlarmFire(Alarm, u32),
}

pub struct InputManager {
    command_channel: Receiver<'static, CriticalSectionRawMutex, TcpCommand, 1>,
}

impl InputManager {
    pub fn initialize(
        spawner: Spawner,
        command_channel: Receiver<'static, CriticalSectionRawMutex, TcpCommand, 1>,
    ) {
        let input_manager = InputManager { command_channel };
        spawner.spawn(dispatcher(input_manager).unwrap());
    }

    pub fn get_alarm_update_signal() -> &'static Signal<CriticalSectionRawMutex, ()> {
        &ALARM_UPDATE_SIGNAL
    }

    pub fn get_alarm_fire_sender() -> Sender<'static, CriticalSectionRawMutex, (Alarm, u32), 1> {
        ALARM_FIRE_CHANNEL.sender()
    }

    pub fn get_cancel_alarm_signal() -> &'static Signal<CriticalSectionRawMutex, ()> {
        &CANCEL_ALARM_SIGNAL
    }

    pub fn get_fetch_request_signal() -> &'static Signal<CriticalSectionRawMutex, ()> {
        &FETCH_REQUEST_SIGNAL
    }

    pub fn get_state_change_signal() -> &'static Signal<CriticalSectionRawMutex, RunState> {
        &STATE_CHANGE_SIGNAL
    }

    pub fn get_display_toggle_signal() -> &'static Signal<CriticalSectionRawMutex, ()> {
        &DISPLAY_TOGGLE_SIGNAL
    }

    pub async fn next_input(&self) -> Input {
        match select4(
            CANCEL_ALARM_SIGNAL.wait(),
            self.command_channel.receive(),
            ALARM_FIRE_CHANNEL.receiver().receive(),
            FETCH_REQUEST_SIGNAL.wait(),
        )
        .await
        {
            Either4::First(_) => Input::CancelAlarm,
            Either4::Second(cmd) => Input::Tcp(cmd),
            Either4::Third((alarm, elapsed)) => Input::AlarmFire(alarm, elapsed),
            Either4::Fourth(_) => Input::FetchAlarms,
        }
    }

    pub fn handle_input(&self, input: Input) {
        match input {
            Input::CancelAlarm => STATE_CHANGE_SIGNAL.signal(RunState::Idle),
            Input::FetchAlarms => {
                log::info!("Manual alarm fetch requested");
                ALARM_UPDATE_SIGNAL.signal(());
            }
            Input::Tcp(TcpCommand::AlarmsUpdated) => {
                log::info!("Alarm update from backend");
                ALARM_UPDATE_SIGNAL.signal(());
            }
            Input::Tcp(TcpCommand::CheckUpdate) => {
                OTA_TRIGGER.signal(());
            }
            Input::Tcp(TcpCommand::Fire) => {
                log::info!("Command received 'Fire'");
                STATE_CHANGE_SIGNAL.signal(RunState::Fire);
            }
            Input::Tcp(TcpCommand::Color(c)) => {
                log::info!("Command received 'Color' {c}");
                STATE_CHANGE_SIGNAL.signal(RunState::Color(c));
            }
            Input::Tcp(TcpCommand::Rainbow) => {
                log::info!("Command received 'Rainbow'");
                STATE_CHANGE_SIGNAL.signal(RunState::Rainbow);
            }
            Input::Tcp(TcpCommand::Idle) => {
                log::info!("Command received 'Idle'");
                STATE_CHANGE_SIGNAL.signal(RunState::Idle);
            }
            Input::AlarmFire(alarm, elapsed) => {
                let effective_start = Instant::now() - Duration::from_secs(elapsed as u64);
                STATE_CHANGE_SIGNAL.signal(RunState::Alarm(alarm, effective_start));
            }
            _ => {
                log::warn!("Received unhandled command: {input:#?}");
            }
        }
    }
}

#[embassy_executor::task]
async fn dispatcher(input_manager: InputManager) -> ! {
    loop {
        let inp = input_manager.next_input().await;
        input_manager.handle_input(inp);
    }
}
