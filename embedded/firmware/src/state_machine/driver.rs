use core::ops::Add;
use embassy_futures::select::{select, Either};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::Timer;

use crate::hardware::{
    change_hw_state, is_hw_err, lock_hw_state, wait_for_hw_state, HardwareError, HardwareState,
};
use crate::input::InputManager;

use super::types::{RunState, StateError, SystemState};

pub struct StateMachine {
    state: SystemState,
    previous_state: SystemState,
    state_change_signal: &'static Signal<CriticalSectionRawMutex, RunState>,
}

impl StateMachine {
    pub fn new(state_change_signal: &'static Signal<CriticalSectionRawMutex, RunState>) -> Self {
        Self {
            state: SystemState::Initializing,
            previous_state: SystemState::Initializing,
            state_change_signal,
        }
    }

    pub async fn run(&mut self) -> ! {
        let mut consecutive_errors: u8 = 0;

        loop {
            let next_state = match &self.state {
                SystemState::Initializing => {
                    change_hw_state(HardwareState::Initialize);
                    let _ = wait_for_hw_state(|s| matches!(s, HardwareState::Initialize), 10).await;
                    let res = wait_for_hw_state(|s| matches!(s, HardwareState::Idle), 180).await;
                    if is_hw_err().await || res.is_err() {
                        log::error!("Initialization of hardware failed");
                        let err = match lock_hw_state().await {
                            Ok(HardwareState::Error(e)) => StateError::Hardware(e),
                            _ => StateError::Hardware(HardwareError::LockTimeout),
                        };
                        SystemState::Error(err)
                    } else {
                        SystemState::Transition(RunState::Idle)
                    }
                }
                SystemState::Transition(next_run_state) => {
                    match self.change_run_state(next_run_state).await {
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
                    let total_secs = alarm.length as u64 * 60;
                    let remaining = total_secs.saturating_sub(started_at.elapsed().as_secs());
                    match select(
                        self.state_change_signal.wait(),
                        wait_for_hw_state(|s| matches!(s, HardwareState::Idle), remaining),
                    )
                    .await
                    {
                        Either::First(state_request) => {
                            log::info!("Alarm '{}' cancelled", alarm.name);
                            SystemState::Transition(state_request)
                        }
                        Either::Second(_) => {
                            log::info!("Alarm '{}' completed", alarm.name);
                            SystemState::Transition(RunState::Idle)
                        }
                    }
                }
                SystemState::Running(_) => {
                    let state_request = self.state_change_signal.wait().await;
                    SystemState::Transition(state_request)
                }
                SystemState::Error(e) => {
                    let backoff = 5 << consecutive_errors;
                    change_hw_state(HardwareState::Idle);
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

            if let SystemState::Transition(next_run_state) = &next_state {
                if let SystemState::Running(current_run_state) = &self.state {
                    if *next_run_state == *current_run_state {
                        continue;
                    }
                }
            }

            self.set_state(next_state);
        }
    }

    async fn change_run_state(&self, run_state: &RunState) -> Result<RunState, StateError> {
        match run_state {
            RunState::Idle => {
                change_hw_state(HardwareState::Idle);
                Ok(RunState::Idle)
            }
            RunState::Alarm(alarm, started_at) => {
                let elapsed_secs = started_at.elapsed().as_secs() as u32;
                InputManager::get_cancel_alarm_signal().reset();
                change_hw_state(HardwareState::Alarm((alarm.clone(), elapsed_secs)));
                let _ = wait_for_hw_state(|s| matches!(s, HardwareState::Alarm(_)), 30).await;
                Ok(RunState::Alarm(alarm.clone(), *started_at))
            }
            RunState::Color(c) => {
                change_hw_state(HardwareState::Color(*c));
                Ok(RunState::Color(*c))
            }
            RunState::Fire => {
                change_hw_state(HardwareState::Fire);
                Ok(RunState::Fire)
            }
            RunState::Rainbow => {
                change_hw_state(HardwareState::Rainbow);
                Ok(RunState::Rainbow)
            }
        }
    }

    fn set_state(&mut self, new_state: SystemState) {
        log::info!("State: {:?} -> {:?}", self.state, new_state);
        self.previous_state = self.state.clone();
        self.state = new_state;
    }
}
