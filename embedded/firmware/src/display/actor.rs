use embassy_executor::Spawner;
use embassy_futures::select::{Either3, Either5, select3, select5};
use embassy_time::{Duration, Instant, Timer, with_timeout};

use super::driver::{DisplayDriver, DisplayError, DisplayInfo, DisplayPinning};
use crate::alarm::{self, AlarmStatus};
use crate::hardware::{self, HardwareState};
use crate::health::{self, Backoff, Fault, Faults};
use crate::mailbox::{Mailbox, MailboxReceiver, MailboxSender};
use crate::net::wifi;
use crate::time_lib::{self, TimeSync};

const INIT_TIMEOUT: Duration = Duration::from_secs(5);

static MAILBOX: Mailbox<DisplayMsg> = Mailbox::new();

#[derive(Debug)]
enum DisplayMsg {
    Toggle,
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
}

struct DisplayActor {
    driver: DisplayDriver,
    mailbox: MailboxReceiver<DisplayMsg>,
    on: bool,
    hw_state: HardwareState,
    time: Option<TimeSync>,
    alarms: Option<AlarmStatus>,
    wifi_connected: bool,
    faults: Faults,
}

impl DisplayActor {
    async fn run(&mut self) -> ! {
        self.init().await;
        let mut hw_state = hardware::subscribe();
        let mut time = time_lib::subscribe();
        let mut alarms = alarm::subscribe();
        let mut wifi = wifi::subscribe();
        let mut faults = health::subscribe();
        loop {
            let published = select5(
                hw_state.changed(),
                time.changed(),
                alarms.changed(),
                wifi.changed(),
                faults.changed(),
            );
            match select3(self.mailbox.receive(), published, self.redraw_due()).await {
                Either3::First(msg) => self.handle_message(msg).await,
                Either3::Second(update) => {
                    match update {
                        Either5::First(state) => self.hw_state = state,
                        Either5::Second(sync) => self.time = Some(sync),
                        Either5::Third(status) => self.alarms = Some(status),
                        Either5::Fourth(connected) => self.wifi_connected = connected,
                        Either5::Fifth(active) => self.faults = active,
                    }
                    self.render().await;
                }
                Either3::Third(()) => self.render().await,
            }
        }
    }

    async fn handle_message(&mut self, msg: DisplayMsg) {
        match msg {
            DisplayMsg::Toggle => {
                self.on = !self.on;
                if self.on {
                    self.render().await;
                } else {
                    self.blank().await;
                }
            }
        }
    }

    /// Initialises the screen and shows a splash, retrying until it succeeds. Messages wait in
    /// the mailbox meanwhile.
    async fn init(&mut self) {
        let mut backoff = Backoff::new();
        loop {
            let result = with_timeout(INIT_TIMEOUT, self.driver.init())
                .await
                .unwrap_or(Err(DisplayError::Timeout));
            match result {
                Ok(()) => {
                    log::info!("Display initialized");
                    health::clear(Fault::Display);
                    return;
                }
                Err(e) => health::report(Fault::Display, e),
            }
            backoff.wait().await;
        }
    }

    /// When to redraw the clock face for the next minute. Never resolves while the screen is
    /// off or the time unknown.
    async fn redraw_due(&self) {
        match self.time {
            Some(sync) if self.on => {
                let now = sync.local_now();
                let into_minute =
                    Duration::from_millis(now.second() as u64 * 1_000 + now.millisecond() as u64);
                let next_minute = Instant::now() + Duration::from_secs(60) - into_minute;
                Timer::at(next_minute).await
            }
            _ => core::future::pending().await,
        }
    }

    async fn render(&mut self) {
        if !self.on {
            return;
        }
        let info = DisplayInfo {
            time: self.time.as_ref().map(TimeSync::local_now),
            next_alarm: self.alarms.as_ref().and_then(|status| status.next.clone()),
            wifi_connected: self.wifi_connected,
            alarms_enabled: self.alarms.as_ref().is_none_or(|status| status.enabled),
            hw_state: self.hw_state,
            faults: self.faults,
        };
        let result = self.driver.render(&info).await;
        report(result);
    }

    async fn blank(&mut self) {
        let result = self.driver.blank().await;
        report(result);
    }
}

fn report(result: Result<(), DisplayError>) {
    match result {
        Ok(()) => health::clear(Fault::Display),
        Err(e) => health::report(Fault::Display, e),
    }
}

pub fn spawn(spawner: Spawner, pinning: DisplayPinning) {
    let actor = DisplayActor {
        driver: DisplayDriver::new(pinning),
        mailbox: MAILBOX.receiver(),
        on: true,
        hw_state: HardwareState::Idle,
        time: None,
        alarms: None,
        wifi_connected: false,
        faults: Faults::default(),
    };
    spawner.spawn(display_task(actor).unwrap());
}

#[embassy_executor::task]
async fn display_task(mut actor: DisplayActor) -> ! {
    actor.run().await
}
