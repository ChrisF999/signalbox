//! The browser shell (spec D1 §2): eframe's web runner on wgpu (WebGPU,
//! falling back to WebGL2), a WebSocket `Transport`, and the hand-off to
//! `/auth/login` when the session has expired. wasm32 only: natively this
//! crate is empty.

#![cfg(target_arch = "wasm32")]

mod store;
mod transport;

use client_core::App;
use client_core::lessons::LESSONS_KEY;
use client_core::settings::SETTINGS_KEY;
use client_ui::UiApp;
use eframe::egui;
use eframe::egui_wgpu::WgpuSetup;
use eframe::wgpu::Backends;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::wasm_bindgen;

use crate::store::LocalStore;
use crate::transport::WebSocketTransport;

struct WebApp {
    ui: UiApp,
    sent_to_login: bool,
    sent_to_logout: bool,
}

impl eframe::App for WebApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui.ui(ui);
        if self.ui.core.wants_login() && !self.sent_to_login {
            self.sent_to_login = true;
            if let Some(w) = web_sys::window() {
                let _ = w.location().set_href("/auth/login");
            }
        }
        // The lobby's Sign out (polish spec M14).
        if self.ui.wants_logout() && !self.sent_to_logout {
            self.sent_to_logout = true;
            if let Some(w) = web_sys::window() {
                let _ = w.location().set_href("/auth/logout");
            }
        }
    }
}

/// Runs when the module is instantiated (`init()` in index.html).
#[wasm_bindgen(start)]
pub fn start() {
    wasm_bindgen_futures::spawn_local(async {
        let canvas = match canvas() {
            Ok(canvas) => canvas,
            Err(why) => return fallback(why, false),
        };
        if let Err(why) = run(canvas).await {
            fallback(&why, true);
        }
    });
}

fn canvas() -> Result<web_sys::HtmlCanvasElement, &'static str> {
    let doc = web_sys::window().and_then(|w| w.document()).ok_or("no document")?;
    doc.get_element_by_id("signalbox_canvas")
        .ok_or("no #signalbox_canvas")?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| "#signalbox_canvas is not a canvas")
}

/// eframe on the canvas; an error here is the renderer's (no WebGPU or WebGL2).
async fn run(canvas: web_sys::HtmlCanvasElement) -> Result<(), String> {
    let mut options = eframe::WebOptions::default();
    // egui-wgpu's own default on the web, stated here: WebGPU where the
    // browser has it (secure contexts only), else WebGL2.
    if let WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        setup.instance_descriptor.backends = Backends::BROWSER_WEBGPU | Backends::GL;
    }
    eframe::WebRunner::new()
        .start(
            canvas,
            options,
            Box::new(|cc| {
                if let Some(rs) = &cc.wgpu_render_state {
                    // Which of WebGPU and WebGL2 we got: for bug reports and the browser check.
                    let backend = format!("signalbox: drawing with {:?}", rs.adapter.get_info().backend);
                    web_sys::console::log_1(&backend.into());
                }
                // A VDU is dark whatever the browser's theme.
                cc.egui_ctx.set_theme(egui::Theme::Dark);
                let now = cc.egui_ctx.input(|i| i.time);
                let transport = WebSocketTransport::new(cc.egui_ctx.clone());
                let settings = Box::new(LocalStore::new(SETTINGS_KEY));
                let lessons = Box::new(LocalStore::new(LESSONS_KEY));
                let ui = UiApp::with_stores(App::new(Box::new(transport), now), settings, lessons);
                Ok(Box::new(WebApp { ui, sent_to_login: false, sent_to_logout: false }))
            }),
        )
        .await
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
}

/// Say in plain HTML why there is no signal box instead of a blank page;
/// `renderer` adds the WebGPU/WebGL2 explanation.
fn fallback(why: &str, renderer: bool) {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    if let Some(c) = doc.get_element_by_id("signalbox_canvas") {
        c.remove();
    }
    if let Some(r) = doc.get_element_by_id("fallback_reason") {
        r.set_text_content(Some(&format!("The signal box could not start: {why}")));
    }
    if renderer {
        if let Some(g) = doc.get_element_by_id("fallback_gpu") {
            let _ = g.remove_attribute("hidden");
        }
    }
    if let Some(f) = doc.get_element_by_id("fallback") {
        let _ = f.remove_attribute("hidden");
    }
}
