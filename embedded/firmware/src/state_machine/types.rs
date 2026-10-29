use embassy_time::Instant;
use smart_leds::RGB8;

use crate::alarm::{model::Alarm, AlarmDriverError};
use crate::hardware::HardwareError;

#[derive(Debug, Clone, PartialEq)]
pub enum RunState {
    Idle,
    Alarm(Alarm, Instant),
    Color(RGB8),
    Fire,
    Rainbow,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum SystemState {
    Initializing,
    Running(RunState),
    Transition(RunState),
    Error(StateError),
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum StateError {
    Alarm(AlarmDriverError),
    Hardware(HardwareError),
}

impl From<AlarmDriverError> for StateError {
    fn from(e: AlarmDriverError) -> Self {
        StateError::Alarm(e)
    }
}

impl From<HardwareError> for StateError {
    fn from(e: HardwareError) -> Self {
        StateError::Hardware(e)
    }
}
