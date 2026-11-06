//! Actor mailboxes: a bounded channel of the actor's message enum, reached through its handle.

use core::fmt::Debug;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender, TrySendError};

pub const MAILBOX_CAPACITY: usize = 4;

pub struct Mailbox<M>(Channel<CriticalSectionRawMutex, M, MAILBOX_CAPACITY>);
pub struct MailboxSender<M: 'static>(Sender<'static, CriticalSectionRawMutex, M, MAILBOX_CAPACITY>);
pub struct MailboxReceiver<M: 'static>(
    Receiver<'static, CriticalSectionRawMutex, M, MAILBOX_CAPACITY>,
);

impl<M> Mailbox<M> {
    pub const fn new() -> Self {
        Self(Channel::new())
    }

    pub fn sender(&'static self) -> MailboxSender<M> {
        MailboxSender(self.0.sender())
    }

    pub fn receiver(&'static self) -> MailboxReceiver<M> {
        MailboxReceiver(self.0.receiver())
    }
}

impl<M> MailboxSender<M> {
    pub fn try_send(&self, msg: M) -> Result<(), TrySendError<M>> {
        self.0.try_send(msg)
    }

    pub async fn send(&self, msg: M) {
        self.0.send(msg).await
    }
}

impl<M: Debug> MailboxSender<M> {
    /// For fire-and-forget messages: a full mailbox drops the message with a warning
    pub fn post_or_warn(&self, msg: M) {
        if let Err(TrySendError::Full(msg)) = self.try_send(msg) {
            log::warn!("Mailbox full, dropped {msg:?}");
        }
    }
}

impl<M> Clone for MailboxSender<M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M> Copy for MailboxSender<M> {}

impl<M> MailboxReceiver<M> {
    pub async fn receive(&self) -> M {
        self.0.receive().await
    }

    pub fn clear(&self) {
        self.0.clear();
    }
}
