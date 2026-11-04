//! Actor mailboxes: a bounded channel of the actor's message enum, reached through its handle.

use core::fmt::Debug;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{Channel, Receiver, Sender, TrySendError};
use embassy_sync::signal::Signal;
use portable_atomic::{AtomicU32, Ordering};

pub const MAILBOX_CAPACITY: usize = 4;

pub struct Mailbox<M>(Channel<CriticalSectionRawMutex, M, MAILBOX_CAPACITY>);
pub struct MailboxSender<M: 'static>(Sender<'static, CriticalSectionRawMutex, M, MAILBOX_CAPACITY>);
pub struct MailboxReceiver<M: 'static>(
    Receiver<'static, CriticalSectionRawMutex, M, MAILBOX_CAPACITY>,
);

//TODO FIX THIS CRAP

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
    pub fn post(&self, msg: M) -> Result<(), TrySendError<M>> {
        self.0.try_send(msg)
    }

    /// Sends `msg`, built around a `Responder` for `reply`, and waits for the actor's answer
    pub async fn request<T: Send>(
        &self,
        reply: &'static ReplyTo<T>,
        msg: impl FnOnce(Responder<T>) -> M,
    ) -> Result<T, TrySendError<M>> {
        let id = reply.next_id.fetch_add(1, Ordering::Relaxed);
        self.post(msg(Responder { reply, id }))?;
        loop {
            // Skips answers to earlier requests, even ones still in flight
            let (answer_id, value) = reply.answer.wait().await;
            if answer_id == id {
                return Ok(value);
            }
        }
    }
}

impl<M: Debug> MailboxSender<M> {
    /// For fire-and-forget messages: a full mailbox drops the message with a warning
    pub fn post_or_warn(&self, msg: M) {
        if let Err(TrySendError::Full(msg)) = self.post(msg) {
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
}

/// Where answers to one kind of request arrive. Only one request may wait on it at a time.
pub struct ReplyTo<T: 'static> {
    answer: Signal<CriticalSectionRawMutex, (u32, T)>,
    next_id: AtomicU32,
}

impl<T> ReplyTo<T> {
    pub const fn new() -> Self {
        Self {
            answer: Signal::new(),
            next_id: AtomicU32::new(0),
        }
    }
}

/// Answers one request. Only `MailboxSender::request` creates these.
pub struct Responder<T: 'static> {
    reply: &'static ReplyTo<T>,
    id: u32,
}

impl<T: Send> Responder<T> {
    pub fn send(self, value: T) {
        self.reply.answer.signal((self.id, value));
    }
}

impl<T> Debug for Responder<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Responder").field("id", &self.id).finish()
    }
}
