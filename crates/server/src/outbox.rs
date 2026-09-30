//! One client's outbound queue (spec §4.3): at most `OUTBOX_CAP` frames. On
//! overflow the queue is cleared and deltas are dropped until the next full
//! view, which the caller asks the game for (`Pushed::Overflowed`).

use std::collections::VecDeque;
use std::sync::Mutex;

use protocol::{ServerFrame, ServerMsg};
use tokio::sync::Notify;

pub const OUTBOX_CAP: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pushed {
    Queued,
    /// A delta while waiting for a full view: dropped.
    Dropped,
    /// The queue was full: it (and this frame) were dropped. Ask the game
    /// for a resync.
    Overflowed,
    /// The client is gone.
    Closed,
}

#[derive(Default)]
struct Inner {
    q: VecDeque<ServerFrame>,
    closed: bool,
    awaiting_view: bool,
}

#[derive(Default)]
pub struct Outbox {
    inner: Mutex<Inner>,
    notify: Notify,
}

impl Outbox {
    pub fn new() -> Outbox {
        Outbox::default()
    }

    pub fn push(&self, frame: ServerFrame) -> Pushed {
        let mut i = self.inner.lock().expect("outbox lock");
        if i.closed {
            return Pushed::Closed;
        }
        if i.awaiting_view {
            match &frame {
                ServerFrame::Game(ServerMsg::Delta(_)) => return Pushed::Dropped,
                ServerFrame::Game(ServerMsg::View(_)) => i.awaiting_view = false,
                _ => {}
            }
        }
        if i.q.len() >= OUTBOX_CAP {
            i.q.clear();
            i.awaiting_view = true;
            return Pushed::Overflowed;
        }
        i.q.push_back(frame);
        drop(i);
        self.notify.notify_one();
        Pushed::Queued
    }

    /// No more frames are accepted; `pop` drains what is queued, then ends.
    pub fn close(&self) {
        self.inner.lock().expect("outbox lock").closed = true;
        self.notify.notify_one();
    }

    pub fn len(&self) -> usize {
        self.inner.lock().expect("outbox lock").q.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The next frame, waiting for one; `None` once closed and drained.
    /// Cancel-safe (a frame is only taken when it is returned).
    pub async fn pop(&self) -> Option<ServerFrame> {
        loop {
            {
                let mut i = self.inner.lock().expect("outbox lock");
                if let Some(f) = i.q.pop_front() {
                    return Some(f);
                }
                if i.closed {
                    return None;
                }
            }
            self.notify.notified().await;
        }
    }
}
