//! Helpers for the client-core tests.

#![allow(dead_code)]

use client_core::{App, MemHandle, MemTransport};

pub fn s(x: &str) -> String {
    x.to_string()
}

/// An app whose first connection is open, its lobby requests taken.
pub fn open_app() -> (App, MemHandle) {
    let (t, h) = MemTransport::new();
    let mut app = App::new(Box::new(t), 0.0);
    h.open();
    app.tick(0.0);
    h.take_sent();
    (app, h)
}
