//! How the client talks to the front: text frames over something that can
//! connect, fail and reconnect. The browser shell implements it over a
//! WebSocket; `MemTransport` is an in-memory one for tests.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use protocol::{ClientFrame, ServerFrame};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConnState {
    /// Never connected, or the last connection ended.
    #[default]
    Closed,
    Connecting,
    Open,
    /// The front refused us for want of a session (HTTP 401): log in again.
    Unauthorized,
}

/// A connection to the front's `/ws`. Every method returns at once; the
/// app polls from its frame loop.
pub trait Transport {
    /// Open a new connection, dropping any old one and anything unread.
    fn connect(&mut self);
    fn state(&self) -> ConnState;
    /// Send one text frame; dropped unless the connection is open.
    fn send(&mut self, text: String);
    /// Text frames received since the last call, oldest first.
    fn poll(&mut self) -> Vec<String>;
}

#[derive(Debug, Default)]
struct Mem {
    state: ConnState,
    inbox: VecDeque<String>,
    sent: Vec<String>,
    connects: usize,
}

/// An in-memory transport. The test holds the `MemHandle` and plays the
/// front: it opens and closes the connection, pushes frames, and reads what
/// the app sent.
pub struct MemTransport(Rc<RefCell<Mem>>);

#[derive(Clone)]
pub struct MemHandle(Rc<RefCell<Mem>>);

impl MemTransport {
    pub fn new() -> (MemTransport, MemHandle) {
        let m = Rc::new(RefCell::new(Mem::default()));
        (MemTransport(m.clone()), MemHandle(m))
    }
}

impl Transport for MemTransport {
    fn connect(&mut self) {
        let mut m = self.0.borrow_mut();
        m.connects += 1;
        m.state = ConnState::Connecting;
        m.inbox.clear();
    }

    fn state(&self) -> ConnState {
        self.0.borrow().state
    }

    fn send(&mut self, text: String) {
        let mut m = self.0.borrow_mut();
        if m.state == ConnState::Open {
            m.sent.push(text);
        }
    }

    fn poll(&mut self) -> Vec<String> {
        self.0.borrow_mut().inbox.drain(..).collect()
    }
}

impl MemHandle {
    pub fn open(&self) {
        self.0.borrow_mut().state = ConnState::Open;
    }

    pub fn close(&self) {
        self.0.borrow_mut().state = ConnState::Closed;
    }

    pub fn unauthorized(&self) {
        self.0.borrow_mut().state = ConnState::Unauthorized;
    }

    pub fn push(&self, f: ServerFrame) {
        self.push_text(&f.to_json());
    }

    /// Any text, e.g. a malformed frame.
    pub fn push_text(&self, text: &str) {
        self.0.borrow_mut().inbox.push_back(text.to_string());
    }

    /// Everything the app sent since the last call, parsed.
    pub fn take_sent(&self) -> Vec<ClientFrame> {
        let sent: Vec<String> = self.0.borrow_mut().sent.drain(..).collect();
        sent.iter().map(|t| ClientFrame::from_json(t).expect("the app sends valid frames")).collect()
    }

    /// How many times the app called `connect`.
    pub fn connects(&self) -> usize {
        self.0.borrow().connects
    }
}
