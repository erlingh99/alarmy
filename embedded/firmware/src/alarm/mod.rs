pub mod model;
mod scheduler;

pub use scheduler::{handle, spawn, AlarmError, AlarmHandle, AlarmScheduler};
