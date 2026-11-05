use core::ops::Add;
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer, with_timeout};

use crate::alarm::model::Alarm;
use crate::hardware::{HardwareError, HardwareHandle};
use crate::mailbox::{Mailbox, MailboxReceiver, MailboxSender};

use super::types::{RunState, StateError, SystemState};

const HW_SELF_TEST_TIMEOUT: Duration = Duration::from_secs(180);

static MAILBOX: Mailbox<StateMsg> = Mailbox::new();

#[derive(Debug)]
enum StateMsg {
    Request(RunState),
}

#[derive(Clone, Copy)]
pub struct StateHandle {
    mailbox: MailboxSender<StateMsg>,
}

pub fn handle() -> StateHandle {
    StateHandle {
        mailbox: MAILBOX.sender(),
    }
}

impl StateHandle {
    pub fn request(&self, state: RunState) {
        self.mailbox.post_or_warn(StateMsg::Request(state));
    }

    /// `elapsed_secs` is how far into the alarm we already are, when resuming one
    pub fn start_alarm(&self, alarm: Alarm, elapsed_secs: u32) {
        let started_at = Instant::now() - Duration::from_secs(elapsed_secs as u64);
        self.request(RunState::Alarm(alarm, started_at));
    }
}

struct SystemStateMachine {
    state: SystemState,
    previous_state: SystemState,
    mailbox: MailboxReceiver<StateMsg>,
    hardware: HardwareHandle,
}

pub fn spawn(spawner: Spawner, hardware: HardwareHandle) {
    let state_machine = SystemStateMachine {
        state: SystemState::Initializing,
        previous_state: SystemState::Initializing,
        mailbox: MAILBOX.receiver(),
        hardware,
    };
    spawner.spawn(state_machine_task(state_machine).unwrap());
}

#[embassy_executor::task]
async fn state_machine_task(mut state_machine: SystemStateMachine) -> ! {
    state_machine.run().await
}

impl SystemStateMachine {
    async fn next_request(&self) -> RunState {
        match self.mailbox.receive().await {
            StateMsg::Request(state) => state,
        }
    }

    async fn run(&mut self) -> ! {
        let mut consecutive_errors: u8 = 0;

        loop {
            let next_state = match &self.state {
                SystemState::Initializing => {
                    match with_timeout(HW_SELF_TEST_TIMEOUT, self.hardware.self_test()).await {
                        Ok(Ok(())) => SystemState::Transition(RunState::Idle),
                        Ok(Err(e)) => {
                            log::error!("Initialization of hardware failed: {e:?}");
                            SystemState::Error(StateError::Hardware(e))
                        }
                        Err(_) => {
                            log::error!("Initialization of hardware timed out");
                            //TODO this is way too much
                            SystemState::Error(StateError::Hardware(HardwareError::Timeout))
                        }
                    }
                }
                SystemState::Transition(next_run_state) => {
                    match self.change_run_state(next_run_state) {
                        Ok(actual_state) => {
                            consecutive_errors = 0;
                            SystemState::Running(actual_state)
                        }
                        Err(e) => {
                            log::error!("State transition failed: {e:?}");
                            SystemState::Error(e)
                        }
                    }
                }
                SystemState::Running(RunState::Alarm(alarm, started_at)) => {
                    // The hardware answers once the ramp, blink and melody are all done
                    let elapsed_secs = started_at.elapsed().as_secs() as u32;
                    let playing = self.hardware.play_alarm(alarm.clone(), elapsed_secs);
                    match select(self.next_request(), playing).await {
                        Either::First(state_request) => {
                            log::info!("Alarm '{}' cancelled", alarm.name);
                            SystemState::Transition(state_request)
                        }
                        Either::Second(Err(e)) => {
                            log::error!("Alarm '{}' failed: {e:?}", alarm.name);
                            SystemState::Transition(RunState::Idle)
                        }
                        Either::Second(Ok(())) => {
                            log::info!("Alarm '{}' completed", alarm.name);
                            SystemState::Transition(RunState::Idle)
                        }
                    }
                }
                SystemState::Running(_) => {
                    let state_request = self.next_request().await;
                    SystemState::Transition(state_request)
                }
                SystemState::Error(e) => {
                    let backoff = 5 << consecutive_errors;
                    self.hardware.idle();
                    log::error!(
                        "Error state ({:?}), retrying {:?} in {}s... (attempt {})",
                        e,
                        self.previous_state,
                        backoff,
                        consecutive_errors + 1
                    );
                    Timer::after_secs(backoff).await;
                    consecutive_errors = consecutive_errors.add(1).min(10);
                    match self.previous_state.clone() {
                        SystemState::Error(_) => SystemState::Initializing,
                        state => state,
                    }
                }
            };

            if let SystemState::Transition(next_run_state) = &next_state
                && let SystemState::Running(current_run_state) = &self.state
                && *next_run_state == *current_run_state
            {
                continue;
            }

            self.set_state(next_state);
        }
    }

    fn change_run_state(&self, run_state: &RunState) -> Result<RunState, StateError> {
        match run_state {
            RunState::Idle => self.hardware.idle(), //TODO might want to wait for transition
            // Played from the Running state, which waits for it to finish
            RunState::Alarm(..) => {}
            RunState::Color(c) => self.hardware.color(*c),
            RunState::Fire => self.hardware.fire(),
            RunState::Rainbow => self.hardware.rainbow(),
        }
        Ok(run_state.clone())
    }

    fn set_state(&mut self, new_state: SystemState) {
        log::info!("State: {:?} -> {:?}", self.state, new_state);
        self.previous_state = self.state.clone();
        self.state = new_state;
    }
}
