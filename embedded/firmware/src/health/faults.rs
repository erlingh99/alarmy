use core::cell::Cell;
use core::fmt::Debug;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::{Receiver, Watch};

/// The display
const SUBSCRIBERS: usize = 1;
static FAULTS: Watch<CriticalSectionRawMutex, Faults, SUBSCRIBERS> = Watch::new();

/// Receives the set of active faults, each time it changes
pub fn subscribe() -> Receiver<'static, CriticalSectionRawMutex, Faults, SUBSCRIBERS> {
    FAULTS
        .receiver()
        .expect("more fault subscribers than SUBSCRIBERS allows")
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fault {
    WifiCredentials,
    Wifi,
    Backend,
    TimeSync,
    AlarmFetch,
    Ota,
    Display,
}

impl Fault {
    const ALL: [Fault; 7] = [
        Fault::WifiCredentials,
        Fault::Wifi,
        Fault::Backend,
        Fault::TimeSync,
        Fault::AlarmFetch,
        Fault::Ota,
        Fault::Display,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Fault::WifiCredentials => "WiFi creds",
            Fault::Wifi => "WiFi",
            Fault::Backend => "Backend",
            Fault::TimeSync => "Time sync",
            Fault::AlarmFetch => "Alarm fetch",
            Fault::Ota => "Update",
            Fault::Display => "Display",
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// The faults currently active
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Faults(u8);

impl Faults {
    pub fn any(self) -> bool {
        self.0 != 0
    }

    pub fn iter(self) -> impl Iterator<Item = Fault> {
        Fault::ALL
            .into_iter()
            .filter(move |fault| self.0 & fault.bit() != 0)
    }
}

/// Marks `fault` active. Only the first report of a fault is logged as an error.
pub fn report(fault: Fault, cause: impl Debug) {
    if update(|faults| faults.0 |= fault.bit()) {
        log::error!("{} failed: {cause:?}", fault.label());
    } else {
        log::debug!("{} still failing: {cause:?}", fault.label());
    }
}

pub fn clear(fault: Fault) {
    if update(|faults| faults.0 &= !fault.bit()) {
        log::info!("{} recovered", fault.label());
    }
}

/// Whether `change` changed the faults
fn update(change: impl Fn(&mut Faults)) -> bool {
    let changed = Cell::new(false);
    FAULTS.sender().send_if_modified(|value| {
        let faults = value.get_or_insert_default();
        let before = *faults;
        change(faults);
        changed.set(*faults != before);
        changed.get()
    });
    changed.get()
}
