//! `Transport` over the browser's WebSocket to `/ws` on the page's own
//! origin (the session cookie goes with it). Browsers hide the status of a
//! refused upgrade, so a socket that closes without ever opening is
//! followed by a plain `GET /ws`: 401 there means the session is gone.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use client_core::{ConnState, Transport};
use eframe::egui;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

#[derive(Default)]
struct Shared {
    state: ConnState,
    inbox: VecDeque<String>,
    /// Bumped by every `connect`; events from older sockets are ignored.
    generation: u64,
    opened: bool,
}

pub struct WebSocketTransport {
    ctx: egui::Context,
    shared: Rc<RefCell<Shared>>,
    ws: Option<WebSocket>,
    /// Kept alive as long as their socket.
    _handlers: Option<(Closure<dyn FnMut(Event)>, Closure<dyn FnMut(MessageEvent)>, Closure<dyn FnMut(CloseEvent)>)>,
}

impl WebSocketTransport {
    pub fn new(ctx: egui::Context) -> WebSocketTransport {
        WebSocketTransport { ctx, shared: Rc::new(RefCell::new(Shared::default())), ws: None, _handlers: None }
    }
}

/// `wss://host/ws` on an https page, else `ws://host/ws`.
fn ws_url() -> Option<String> {
    let loc = web_sys::window()?.location();
    let scheme = if loc.protocol().ok()? == "https:" { "wss:" } else { "ws:" };
    Some(format!("{scheme}//{}/ws", loc.host().ok()?))
}

/// Is `/ws` refusing us for want of a session?
async fn unauthorized() -> bool {
    let Some(w) = web_sys::window() else { return false };
    match wasm_bindgen_futures::JsFuture::from(w.fetch_with_str("/ws")).await {
        Ok(r) => r.dyn_into::<web_sys::Response>().is_ok_and(|r| r.status() == 401),
        Err(_) => false,
    }
}

impl Transport for WebSocketTransport {
    fn connect(&mut self) {
        if let Some(old) = self.ws.take() {
            old.set_onopen(None);
            old.set_onmessage(None);
            old.set_onclose(None);
            let _ = old.close();
        }
        let generation = {
            let mut s = self.shared.borrow_mut();
            s.generation += 1;
            s.state = ConnState::Connecting;
            s.inbox.clear();
            s.opened = false;
            s.generation
        };
        let Some(ws) = ws_url().and_then(|u| WebSocket::new(&u).ok()) else {
            self.shared.borrow_mut().state = ConnState::Closed;
            return;
        };
        let (shared, ctx) = (self.shared.clone(), self.ctx.clone());
        let on_open = Closure::<dyn FnMut(Event)>::new(move |_| {
            let mut s = shared.borrow_mut();
            if s.generation == generation {
                s.state = ConnState::Open;
                s.opened = true;
                ctx.request_repaint();
            }
        });
        let (shared, ctx) = (self.shared.clone(), self.ctx.clone());
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            let mut s = shared.borrow_mut();
            if s.generation == generation {
                if let Some(text) = e.data().as_string() {
                    s.inbox.push_back(text);
                    ctx.request_repaint();
                }
            }
        });
        let (shared, ctx) = (self.shared.clone(), self.ctx.clone());
        let on_close = Closure::<dyn FnMut(CloseEvent)>::new(move |_| {
            let opened = {
                let s = shared.borrow();
                if s.generation != generation {
                    return;
                }
                s.opened
            };
            if opened {
                shared.borrow_mut().state = ConnState::Closed;
                ctx.request_repaint();
                return;
            }
            let (shared, ctx) = (shared.clone(), ctx.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let state = if unauthorized().await { ConnState::Unauthorized } else { ConnState::Closed };
                let mut s = shared.borrow_mut();
                if s.generation == generation {
                    s.state = state;
                    ctx.request_repaint();
                }
            });
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        self.ws = Some(ws);
        self._handlers = Some((on_open, on_message, on_close));
    }

    fn state(&self) -> ConnState {
        self.shared.borrow().state
    }

    fn send(&mut self, text: String) {
        if self.state() == ConnState::Open {
            if let Some(ws) = &self.ws {
                let _ = ws.send_with_str(&text);
            }
        }
    }

    fn poll(&mut self) -> Vec<String> {
        self.shared.borrow_mut().inbox.drain(..).collect()
    }
}
