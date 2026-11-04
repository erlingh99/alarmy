//! Answers the display's requests for time, alarm and network info. Runs on core0, which owns
//! the clock and the alarm scheduler (they hold the network stack, so they can't cross cores).

use embassy_executor::Spawner;
use portable_atomic::Ordering;

use super::actor::DisplayHandle;
use super::driver::DisplayInfo;
use crate::alarm::AlarmScheduler;
use crate::mailbox::{Mailbox, MailboxReceiver, MailboxSender};
use crate::net::WIFI_CONNECTED;
use crate::time_lib::Clock;

static MAILBOX: Mailbox<InfoMsg> = Mailbox::new();

#[derive(Debug)]
enum InfoMsg {
    Request,
}

#[derive(Clone, Copy)]
pub(super) struct InfoProviderHandle {
    mailbox: MailboxSender<InfoMsg>,
}

pub(super) fn handle() -> InfoProviderHandle {
    InfoProviderHandle {
        mailbox: MAILBOX.sender(),
    }
}

impl InfoProviderHandle {
    pub(super) fn request(&self) {
        self.mailbox.post_or_warn(InfoMsg::Request);
    }
}

struct InfoProvider {
    mailbox: MailboxReceiver<InfoMsg>,
    clock: &'static Clock,
    alarm_scheduler: &'static AlarmScheduler,
    display: DisplayHandle,
}

pub fn spawn(
    spawner: Spawner,
    clock: &'static Clock,
    alarm_scheduler: &'static AlarmScheduler,
    display: DisplayHandle,
) {
    let provider = InfoProvider {
        mailbox: MAILBOX.receiver(),
        clock,
        alarm_scheduler,
        display,
    };
    spawner.spawn(info_provider_task(provider).unwrap());
}

impl InfoProvider {
    async fn run(&self) -> ! {
        // Unprompted once, which tells the display it can start asking
        self.send_info().await;
        loop {
            match self.mailbox.receive().await {
                InfoMsg::Request => self.send_info().await,
            }
        }
    }

    async fn send_info(&self) {
        let next_alarm = self
            .alarm_scheduler
            .get_next_alarm()
            .await
            .ok()
            .map(|alarm| (alarm.name, alarm.time));
        self.display.show_info(DisplayInfo {
            time: self.clock.get_local_time().await.ok(),
            next_alarm,
            wifi_connected: WIFI_CONNECTED.load(Ordering::Relaxed),
            alarms_enabled: self.alarm_scheduler.is_enabled(),
        });
    }
}

#[embassy_executor::task]
async fn info_provider_task(provider: InfoProvider) -> ! {
    provider.run().await
}
