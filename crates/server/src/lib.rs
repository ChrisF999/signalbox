//! signalbox's server side: the game process (`process`, run by the
//! `signalbox-game` binary) and the front (`signalbox-server`): config,
//! sessions, layouts, the supervisor of game processes, and the web routes.

pub mod config;
pub mod layouts;
pub mod limit;
pub mod outbox;
pub mod process;
pub mod session;
pub mod supervisor;
pub mod web;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum_extra::extract::cookie::Key;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::layouts::Layouts;
use crate::session::Sessions;
use crate::supervisor::{Supervisor, SupervisorConfig};
use crate::web::AppState;

/// How long games get to save and exit when the front stops (spec §2.2).
pub const STOP_GRACE: Duration = Duration::from_secs(10);

/// A front serving on `addr`.
pub struct Running {
    pub addr: SocketAddr,
    pub sup: Arc<Supervisor>,
    pub sessions: Arc<Sessions>,
    stop: Arc<Notify>,
    server: JoinHandle<()>,
}

/// Start the front: data directories, layouts, supervisor, listener.
pub async fn start(cfg: Config) -> Result<Running, String> {
    let layouts = Layouts::load(&cfg.layouts_dir)?;
    let sup = Supervisor::new(
        SupervisorConfig {
            game_bin: cfg.game_bin.clone(),
            saves_dir: cfg.data_dir.join("saves"),
            sockets_dir: cfg.data_dir.join("sockets"),
            empty_exit_s: process::EMPTY_EXIT_S,
        },
        layouts,
    )?;
    let sessions = Arc::new(Sessions::new());
    let state = AppState { sup: sup.clone(), sessions: sessions.clone(), key: Key::from(&cfg.session_key) };
    let listener = tokio::net::TcpListener::bind(cfg.addr).await.map_err(|e| format!("{}: {e}", cfg.addr))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let stop = Arc::new(Notify::new());
    let app = web::router(state);
    let server = tokio::spawn({
        let stop = stop.clone();
        async move {
            let _ = axum::serve(listener, app).with_graceful_shutdown(async move { stop.notified().await }).await;
        }
    });
    Ok(Running { addr, sup, sessions, stop, server })
}

impl Running {
    /// `http://<addr>`.
    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Shut every game down (≤ `STOP_GRACE`), then stop serving.
    pub async fn stop(self) {
        self.sup.shutdown_all(STOP_GRACE).await;
        self.stop.notify_one();
        let _ = tokio::time::timeout(Duration::from_secs(2), self.server).await;
    }
}

/// The binary: serve until SIGTERM or SIGINT, then stop cleanly.
pub async fn run(cfg: Config) -> Result<(), String> {
    let running = start(cfg).await?;
    eprintln!("signalbox-server: listening on {}", running.addr);
    let mut term = signal(SignalKind::terminate()).map_err(|e| e.to_string())?;
    let mut int = signal(SignalKind::interrupt()).map_err(|e| e.to_string())?;
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
    eprintln!("signalbox-server: stopping");
    running.stop().await;
    Ok(())
}
