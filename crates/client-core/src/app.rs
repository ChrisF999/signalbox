//! The client's state (spec D1 §2, §5): the connection with its reconnect
//! backoff, the lobby, and the game you are in (layout and view kept by a
//! `bot::Bot`, the log, the selection). Pure: the shell calls `tick` with
//! its clock every frame and reads the state back; nothing here waits,
//! sleeps or panics on anything the server sends.

use bot::Bot;
use protocol::{
    ClientFrame, ClientMsg, GameInfo, Layout, LayoutInfo, LobbyMsg, LobbyReply, Notice, PlayerCommand, Proposal, ServerFrame,
    ServerMsg, View,
};

use crate::log::Log;
use crate::text::notice_text;
use crate::transport::{ConnState, Transport};

/// The first retry waits this long; each failure doubles it up to `MAX_BACKOFF_S`.
pub const FIRST_BACKOFF_S: f64 = 0.5;
pub const MAX_BACKOFF_S: f64 = 10.0;
/// How long a refused command's entrance signal flashes.
pub const FLASH_S: f64 = 2.0;

/// Seconds to wait before retry number `attempt` (0-based).
pub fn backoff_s(attempt: u32) -> f64 {
    (FIRST_BACKOFF_S * f64::from(1u32 << attempt.min(8))).min(MAX_BACKOFF_S)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Link {
    Connecting,
    Open,
    /// Lost; the next attempt is at `retry_at` on the shell's clock.
    Waiting { retry_at: f64 },
    /// The session has expired: the shell sends the browser to `/auth/login`.
    LoginNeeded,
    /// This login was opened elsewhere; nothing reconnects until `reconnect_now`.
    Replaced,
}

/// The game you are in.
pub struct InGame {
    pub id: String,
    pub you: String,
    pub(crate) bot: Bot,
    pub(crate) layout_gen: u64,
    pub(crate) selected: Option<String>,
    pub(crate) flash: Option<(String, f64)>,
    pub(crate) log: Log,
}

impl InGame {
    fn new(id: String, you: String) -> InGame {
        InGame { id, you, bot: Bot::new(), layout_gen: 0, selected: None, flash: None, log: Log::default() }
    }

    pub fn layout(&self) -> Option<&Layout> {
        self.bot.layout()
    }

    pub fn view(&self) -> Option<&View> {
        self.bot.view()
    }

    /// Bumped by every layout received: redraw caches keyed on it.
    pub fn layout_gen(&self) -> u64 {
        self.layout_gen
    }

    /// The area you hold; `None` while spectating.
    pub fn area(&self) -> Option<&str> {
        self.bot.area()
    }

    /// The chosen entrance signal.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The signal flashing for a refused command.
    pub fn flashing(&self) -> Option<&str> {
        self.flash.as_ref().map(|(s, _)| s.as_str())
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    /// Resyncs asked for in this game (gaps and unreadable frames).
    pub fn resyncs(&self) -> usize {
        self.bot.resyncs()
    }

    fn sim_time(&self) -> Option<f64> {
        self.bot.view().map(|v| v.sim_time)
    }
}

pub struct App {
    pub(crate) transport: Box<dyn Transport>,
    pub(crate) link: Link,
    /// Failed attempts since a connection last carried a frame.
    pub(crate) attempt: u32,
    /// A connection was open at least once (so "reconnecting", not "connecting").
    pub(crate) was_open: bool,
    pub(crate) now: f64,
    pub(crate) games: Vec<GameInfo>,
    pub(crate) layouts: Vec<LayoutInfo>,
    /// A message for the lobby screen (a crash, a failed rejoin, an error).
    pub(crate) lobby_note: Option<String>,
    pub(crate) game: Option<InGame>,
    /// The game to rejoin after a reconnect.
    pub(crate) rejoin: Option<String>,
    /// A rejoin was sent and its `joined` has not come yet.
    pub(crate) rejoining: bool,
}

impl App {
    /// Starts connecting at once. `now` is the shell's clock in seconds.
    pub fn new(mut transport: Box<dyn Transport>, now: f64) -> App {
        transport.connect();
        App {
            transport,
            link: Link::Connecting,
            attempt: 0,
            was_open: false,
            now,
            games: Vec::new(),
            layouts: Vec::new(),
            lobby_note: None,
            game: None,
            rejoin: None,
            rejoining: false,
        }
    }

    /// Take what arrived and move the connection on. Call every frame.
    pub fn tick(&mut self, now: f64) {
        self.now = now;
        for text in self.transport.poll() {
            self.receive(&text);
        }
        match (self.link, self.transport.state()) {
            (Link::Replaced | Link::LoginNeeded, _) => {}
            (Link::Connecting, ConnState::Open) => {
                self.link = Link::Open;
                self.was_open = true;
                self.on_open();
            }
            (Link::Connecting | Link::Open, ConnState::Closed) => {
                self.link = Link::Waiting { retry_at: now + backoff_s(self.attempt) };
                self.attempt = self.attempt.saturating_add(1);
            }
            (Link::Connecting | Link::Open, ConnState::Unauthorized) => self.link = Link::LoginNeeded,
            (Link::Waiting { retry_at }, _) if now >= retry_at => {
                self.transport.connect();
                self.link = Link::Connecting;
            }
            _ => {}
        }
        if let Some(g) = self.game.as_mut() {
            if g.flash.as_ref().is_some_and(|(_, until)| now >= *until) {
                g.flash = None;
            }
        }
    }

    pub fn link(&self) -> Link {
        self.link
    }

    /// The shell should send the browser to `/auth/login`.
    pub fn wants_login(&self) -> bool {
        self.link == Link::LoginNeeded
    }

    /// Words for the connection banner; `None` when all is well.
    pub fn banner(&self) -> Option<String> {
        match self.link {
            Link::Open => None,
            Link::Connecting if !self.was_open => Some("Connecting…".into()),
            Link::Connecting => Some("Reconnecting…".into()),
            Link::Waiting { retry_at } => {
                Some(format!("Connection lost. Reconnecting in {} s…", (retry_at - self.now).max(0.0).ceil()))
            }
            Link::LoginNeeded => Some("Your session has expired. Signing in again…".into()),
            Link::Replaced => Some("This login is now open in another tab or window.".into()),
        }
    }

    /// Connect again now (after `Replaced`, or to skip a backoff wait).
    pub fn reconnect_now(&mut self) {
        if matches!(self.link, Link::Replaced | Link::Waiting { .. }) {
            self.attempt = 0;
            self.transport.connect();
            self.link = Link::Connecting;
        }
    }

    fn on_open(&mut self) {
        match self.rejoin.clone() {
            // The full layout and view that follow `joined` are the one resync.
            Some(game) => {
                self.rejoining = true;
                self.send(ClientFrame::Lobby(LobbyMsg::Join { game }));
            }
            None => {
                self.send(ClientFrame::Lobby(LobbyMsg::ListLayouts));
                self.send(ClientFrame::Lobby(LobbyMsg::ListGames));
            }
        }
    }

    pub(crate) fn send(&mut self, f: ClientFrame) -> bool {
        if self.link != Link::Open {
            return false;
        }
        self.transport.send(f.to_json());
        true
    }

    /// Send a game message; logged as an alarm when there is no connection.
    pub(crate) fn send_game(&mut self, m: ClientMsg) {
        if !self.send(ClientFrame::Game(m)) {
            if let Some(g) = self.game.as_mut() {
                let t = g.sim_time();
                g.log.push(t, "Not connected: nothing was sent".into(), true);
            }
        }
    }

    fn to_lobby(&mut self, note: Option<String>) {
        self.game = None;
        self.rejoin = None;
        self.rejoining = false;
        self.lobby_note = note;
        self.send(ClientFrame::Lobby(LobbyMsg::ListGames));
    }

    fn receive(&mut self, text: &str) {
        if self.link == Link::Open {
            self.attempt = 0;
        }
        match ServerFrame::from_json(text) {
            Ok(ServerFrame::Lobby(r)) => self.lobby_reply(r),
            Ok(ServerFrame::Game(m)) => self.game_msg(m),
            Err(e) => match self.game.as_mut() {
                Some(g) => {
                    let t = g.sim_time();
                    g.log.push(t, format!("Unreadable message from the server ({e}); resyncing"), false);
                    if let Some(m) = g.bot.request_resync() {
                        self.send_game(m);
                    }
                }
                None => self.lobby_note = Some(format!("Unreadable message from the server ({e})")),
            },
        }
    }

    fn lobby_reply(&mut self, r: LobbyReply) {
        match r {
            LobbyReply::Games { games } => self.games = games,
            LobbyReply::Layouts { layouts } => self.layouts = layouts,
            LobbyReply::Joined { game, you } => {
                self.rejoining = false;
                self.rejoin = Some(game.clone());
                self.lobby_note = None;
                if self.game.as_ref().is_none_or(|g| g.id != game) {
                    self.game = Some(InGame::new(game, you));
                }
            }
            LobbyReply::Error { message, .. } => {
                if self.rejoining {
                    self.to_lobby(Some(format!("Could not rejoin the game: {message}")));
                } else if let Some(g) = self.game.as_mut() {
                    let t = g.sim_time();
                    g.log.push(t, format!("Error: {message}"), true);
                } else {
                    self.lobby_note = Some(message);
                }
            }
        }
    }

    fn game_msg(&mut self, m: ServerMsg) {
        if m == ServerMsg::Notice(Notice::Replaced) {
            self.link = Link::Replaced;
            if let Some(g) = self.game.as_mut() {
                let t = g.sim_time();
                g.log.push(t, notice_text(&Notice::Replaced).0, true);
            }
            return;
        }
        let Some(g) = self.game.as_mut() else { return };
        match &m {
            ServerMsg::Notice(Notice::GameCrashed) => {
                return self.to_lobby(Some("The game stopped unexpectedly. Join it again to resume it.".into()));
            }
            ServerMsg::Notice(n) => {
                let (text, alarm) = notice_text(n);
                let t = g.sim_time();
                g.log.push(t, text, alarm);
                if let Notice::Rejected { cmd, .. } = n {
                    if let Some(e) = entrance_of(cmd) {
                        g.flash = Some((e.to_string(), self.now + FLASH_S));
                    }
                }
            }
            ServerMsg::Layout(_) => g.layout_gen += 1,
            ServerMsg::View(_) | ServerMsg::Delta(_) => {}
        }
        let reply = g.bot.receive(m);
        g.bot.take_notices();
        if let Some(r) = reply {
            self.send_game(r);
        }
    }

    // ---- lobby ----

    pub fn games(&self) -> &[GameInfo] {
        &self.games
    }

    pub fn layouts(&self) -> &[LayoutInfo] {
        &self.layouts
    }

    pub fn lobby_note(&self) -> Option<&str> {
        self.lobby_note.as_deref()
    }

    pub fn refresh(&mut self) {
        self.send(ClientFrame::Lobby(LobbyMsg::ListGames));
    }

    /// `start` is "HH:MM" or "HH:MM:SS"; the front checks it.
    pub fn create_game(&mut self, layout: &str, seed: Option<u64>, start: Option<String>) {
        self.send(ClientFrame::Lobby(LobbyMsg::CreateGame { layout: layout.to_string(), seed, start }));
    }

    pub fn join(&mut self, game: &str) {
        self.send(ClientFrame::Lobby(LobbyMsg::Join { game: game.to_string() }));
    }

    /// Back to the lobby (the front answers with the games list).
    pub fn leave(&mut self) {
        self.send(ClientFrame::Lobby(LobbyMsg::Leave));
        self.game = None;
        self.rejoin = None;
        self.rejoining = false;
    }

    // ---- game ----

    pub fn game(&self) -> Option<&InGame> {
        self.game.as_ref()
    }

    pub fn claim(&mut self, area: &str) {
        self.send_game(ClientMsg::Claim { area: area.to_string() });
    }

    pub fn release(&mut self) {
        self.send_game(ClientMsg::Release);
    }

    /// Propose a clock change, or agree to the open proposal.
    pub fn vote(&mut self, proposal: Proposal) {
        self.send_game(ClientMsg::Vote { proposal });
    }

    pub fn command(&mut self, cmd: PlayerCommand) {
        self.send_game(ClientMsg::Command { cmd });
    }
}

/// The signal a command was about, to flash when it is refused.
pub fn entrance_of(cmd: &PlayerCommand) -> Option<&str> {
    match cmd {
        PlayerCommand::SetRoute { entrance, .. }
        | PlayerCommand::CancelRoute { entrance }
        | PlayerCommand::SetAutoWorking { entrance, .. } => Some(entrance),
        _ => None,
    }
}
