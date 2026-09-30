//! A headless signalbox client (spec §2.1): keeps the layout and view a game
//! sends, applies deltas, and asks for one resync when a delta goes missing.
//! In C1 it talks to an in-process `Game`; `net` puts a WebSocket in between.
//! Without the `net` feature this is `Bot` and `Greedy` only (no tokio), as
//! the browser client uses it.

#[cfg(feature = "net")]
pub mod net;
#[cfg(feature = "net")]
pub mod play;
pub mod strategy;

use protocol::{ClientMsg, Layout, Notice, ServerMsg, View};

#[derive(Clone, Debug, Default)]
pub struct Bot {
    layout: Option<Layout>,
    view: Option<View>,
    /// A resync was asked for; deltas are ignored until the next full view.
    awaiting_resync: bool,
    notices: Vec<Notice>,
    resyncs: usize,
}

impl Bot {
    pub fn new() -> Bot {
        Bot::default()
    }

    /// Take one message from the game; returns what to send back, if anything.
    pub fn receive(&mut self, msg: ServerMsg) -> Option<ClientMsg> {
        match msg {
            ServerMsg::Layout(l) => {
                self.layout = Some(l);
                None
            }
            ServerMsg::View(v) => {
                self.view = Some(v);
                self.awaiting_resync = false;
                None
            }
            ServerMsg::Delta(d) => {
                if self.awaiting_resync {
                    return None;
                }
                let applied = match self.view.as_mut() {
                    Some(v) => v.apply(&d).is_ok(),
                    None => false,
                };
                if applied {
                    return None;
                }
                self.awaiting_resync = true;
                self.resyncs += 1;
                Some(ClientMsg::Resync)
            }
            ServerMsg::Notice(n) => {
                self.notices.push(n);
                None
            }
        }
    }

    pub fn layout(&self) -> Option<&Layout> {
        self.layout.as_ref()
    }

    pub fn view(&self) -> Option<&View> {
        self.view.as_ref()
    }

    /// The area this bot holds, from its layout.
    pub fn area(&self) -> Option<&str> {
        self.layout.as_ref()?.area.as_deref()
    }

    /// How many resyncs this bot has asked for.
    pub fn resyncs(&self) -> usize {
        self.resyncs
    }

    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }

    /// Ask for a resync from outside (e.g. after a frame that would not
    /// parse): the `Resync` to send, or `None` if one is already on its way.
    pub fn request_resync(&mut self) -> Option<ClientMsg> {
        if self.awaiting_resync {
            return None;
        }
        self.awaiting_resync = true;
        self.resyncs += 1;
        Some(ClientMsg::Resync)
    }
}
