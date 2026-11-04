use embassy_time::Instant;
use smart_leds::RGB8;

use crate::alarm::{model::Alarm, AlarmError};
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
pub enum SystemState {
    Initializing,
    Running(RunState),
    Transition(RunState),
    Error(StateError),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StateError {
    Alarm(AlarmError),
    Hardware(HardwareError),
}

impl From<AlarmError> for StateError {
    fn from(e: AlarmError) -> Self {
        StateError::Alarm(e)
    }
}

impl From<HardwareError> for StateError {
    fn from(e: HardwareError) -> Self {
        StateError::Hardware(e)
    }
}
