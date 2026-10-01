//! The client's state (spec D1 §2, §5): the connection with its reconnect
//! backoff, the lobby, and the game you are in (layout and view kept by a
//! `bot::Bot`, the log, the selection). Pure: the shell calls `tick` with
//! its clock every frame and reads the state back; nothing here waits,
//! sleeps or panics on anything the server sends.

use bot::Bot;
use protocol::{
    ClientFrame, ClientMsg, GameInfo, Layout, LayoutInfo, LessonInfo, LessonView, LobbyMsg, LobbyReply, Notice, PlayerCommand, Proposal, ServerFrame,
    ServerMsg, View, codes,
};

use crate::log::Log;
use crate::names::Names;
use crate::text::notice_text;
use crate::transport::{ConnState, Transport};

/// The first retry waits this long; each failure doubles it up to `MAX_BACKOFF_S`.
pub const FIRST_BACKOFF_S: f64 = 0.5;
pub const MAX_BACKOFF_S: f64 = 10.0;
/// How long a refused command's entrance signal stays outlined (a steady
/// outline, not a flash: realism spec owner decision 9).
pub const REFUSED_S: f64 = 2.0;
/// In a game with an open connection and nothing received for this long,
/// join the game again: the front answers with a fresh layout and view, or
/// with the error that ends the game (a lost `game_crashed`, say). If that
/// join goes unanswered for as long again (even a paused game answers it),
/// the connection is dead though it never closed: reconnect as if it had.
pub const WATCHDOG_S: f64 = 20.0;
/// The lobby banner when a tutorial ends under the player (stopped, or gone on rejoin).
const TUTORIAL_ENDED: &str = "The tutorial ended. Start it again from Tutorials.";

/// The holder the view names for an area nobody holds (`game::ROBOT`).
pub const ROBOT: &str = "robot";

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
    pub(crate) refused: Option<(String, f64)>,
    /// The entrance of the route in the way of that command (polish spec M4).
    pub(crate) blocking: Option<String>,
    pub(crate) log: Log,
    /// Display names for the layout held (rebuilt with every layout).
    pub(crate) names: Names,
    /// The lesson, in a tutorial (the last `lesson` received).
    pub(crate) lesson: Option<LessonView>,
    /// The (tab, selected entrance) last told to the lesson.
    pub(crate) screen_sent: Option<(Option<String>, Option<String>)>,
}

impl InGame {
    fn new(id: String, you: String) -> InGame {
        InGame {
            id,
            you,
            bot: Bot::new(),
            layout_gen: 0,
            selected: None,
            refused: None,
            blocking: None,
            log: Log::default(),
            names: Names::default(),
            lesson: None,
            screen_sent: None,
        }
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

    /// Whether your clock votes count (owner decision 12): you hold an
    /// area, or nobody does (every area is the robot's).
    pub fn can_vote(&self) -> bool {
        self.area().is_some() || self.view().is_some_and(|v| v.holders.values().all(|h| h == ROBOT))
    }

    /// The chosen entrance signal.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The entrance of the route that was in the way of the command just
    /// refused, outlined with it (polish spec M4).
    pub fn blocking(&self) -> Option<&str> {
        self.refused.as_ref().and(self.blocking.as_deref())
    }

    /// The entrance of a command just refused, outlined for `REFUSED_S`.
    pub fn refused(&self) -> Option<&str> {
        self.refused.as_ref().map(|(s, _)| s.as_str())
    }

    /// How this layout's signals are shown (plain names before any layout).
    pub fn names(&self) -> &Names {
        &self.names
    }

    /// The lesson, when this game is a tutorial.
    pub fn lesson(&self) -> Option<&LessonView> {
        self.lesson.as_ref()
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

    /// Log `text` (not an alarm) unless it is already the newest line, so a
    /// player clicking again and again gets one line.
    pub(crate) fn log_once(&mut self, text: String) {
        if self.log.entries().next_back().is_some_and(|e| e.text == text) {
            return;
        }
        let t = self.sim_time();
        self.log.push(t, text, false);
    }
}

/// A `join` or `create_game` whose `joined` has not come yet.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Joining {
    /// The game asked for; `None` for a new game, whose id only `joined` gives.
    pub(crate) game: Option<String>,
    /// Sent by a reconnect: if it fails, the game is over for this client.
    pub(crate) rejoin: bool,
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
    pub(crate) lessons: Vec<LessonInfo>,
    /// A message for the lobby screen (a crash, a failed rejoin, an error).
    pub(crate) lobby_note: Option<String>,
    pub(crate) game: Option<InGame>,
    /// The game to rejoin after a reconnect.
    pub(crate) rejoin: Option<String>,
    /// A join was sent and neither its `joined` nor its layout or view has come.
    pub(crate) joining: Option<Joining>,
    /// Your name, from the last `joined`.
    pub(crate) me: Option<String>,
    /// When the last frame arrived (or the connection opened), for the watchdog.
    pub(crate) last_frame: f64,
    /// The watchdog sent a join on this connection and no frame has come since.
    pub(crate) watchdog_join_sent: bool,
    /// The area to claim once the game just created sends its first layout
    /// (polish spec H2: the creator signals at once instead of watching).
    /// With a late start that layout comes only when the game is ready, so
    /// nothing is claimed while it is being prepared.
    pub(crate) claim_on_join: Option<String>,
    /// The area that claim asked for, until the answer shows: a refusal
    /// then says the claim failed rather than who now holds the area.
    pub(crate) claim_sent: Option<String>,
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
            lessons: Vec::new(),
            lobby_note: None,
            game: None,
            rejoin: None,
            joining: None,
            me: None,
            last_frame: now,
            watchdog_join_sent: false,
            claim_on_join: None,
            claim_sent: None,
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
                self.last_frame = now;
                self.watchdog_join_sent = false;
                self.on_open();
            }
            (Link::Connecting | Link::Open, ConnState::Closed) => self.lost(),
            (Link::Connecting | Link::Open, ConnState::Unauthorized) => self.link = Link::LoginNeeded,
            (Link::Waiting { retry_at }, _) if now >= retry_at => {
                self.transport.connect();
                self.link = Link::Connecting;
            }
            _ => {}
        }
        if self.link == Link::Open && now - self.last_frame >= WATCHDOG_S {
            if self.watchdog_join_sent {
                // Unanswered: the socket is dead; the next connection rejoins.
                self.lost();
            } else if let Some(game) = self.game.as_ref().map(|g| g.id.clone()) {
                self.last_frame = now;
                self.watchdog_join_sent = true;
                // A rejoin: if the game is gone, its error ends the game.
                self.joining = Some(Joining { game: Some(game.clone()), rejoin: true });
                self.send(ClientFrame::Lobby(LobbyMsg::Join { game }));
            }
        }
        if let Some(g) = self.game.as_mut() {
            if g.refused.as_ref().is_some_and(|(_, until)| now >= *until) {
                g.refused = None;
            }
        }
    }

    /// The connection is gone: wait out the backoff, then connect again.
    fn lost(&mut self) {
        self.link = Link::Waiting { retry_at: self.now + backoff_s(self.attempt) };
        self.attempt = self.attempt.saturating_add(1);
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
                self.joining = Some(Joining { game: Some(game.clone()), rejoin: true });
                self.send(ClientFrame::Lobby(LobbyMsg::Join { game }));
            }
            None => {
                self.send(ClientFrame::Lobby(LobbyMsg::ListLayouts));
                self.send(ClientFrame::Lobby(LobbyMsg::ListLessons));
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
        self.claim_on_join = None;
        self.claim_sent = None;
        self.game = None;
        self.rejoin = None;
        self.joining = None;
        self.lobby_note = note;
        self.send(ClientFrame::Lobby(LobbyMsg::ListGames));
    }

    fn receive(&mut self, text: &str) {
        if self.link == Link::Open {
            self.attempt = 0;
        }
        self.last_frame = self.now;
        self.watchdog_join_sent = false;
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
                None => {
                    self.lobby_note = Some(format!("Unreadable message from the server ({e})"));
                    self.send(ClientFrame::Lobby(LobbyMsg::ListGames));
                }
            },
        }
    }

    fn lobby_reply(&mut self, r: LobbyReply) {
        match r {
            LobbyReply::Games { games } => self.games = games,
            LobbyReply::Layouts { layouts } => self.layouts = layouts,
            LobbyReply::Lessons { lessons } => self.lessons = lessons,
            LobbyReply::Joined { game, you } => {
                self.joining = None;
                self.me = Some(you.clone());
                self.rejoin = Some(game.clone());
                self.lobby_note = None;
                if self.game.as_ref().is_none_or(|g| g.id != game) {
                    self.game = Some(InGame::new(game, you));
                }
            }
            LobbyReply::Error { code, message } => {
                let joining = self.joining.take();
                if joining.as_ref().is_some_and(|j| j.game.is_none()) {
                    // The front refused a create: its claim goes with it.
                    self.claim_on_join = None;
                }
                let tutorial = self.in_lesson();
                if tutorial && (code == codes::GAME_STOPPED || joining.as_ref().is_some_and(|j| j.rejoin)) {
                    self.to_lobby(Some(TUTORIAL_ENDED.into()));
                } else if code == codes::GAME_STOPPED {
                    self.to_lobby(Some("The game stopped. Join it again to resume it.".into()));
                } else if code == codes::SEED_TOO_SLOW || code == codes::NOT_CREATED {
                    // The game being prepared for us was not created.
                    self.to_lobby(Some(message));
                } else if joining.is_some_and(|j| j.rejoin) {
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
                g.log.push(t, notice_text(&Notice::Replaced, &g.names).0, true);
            }
            return;
        }
        if matches!(m, ServerMsg::Layout(_) | ServerMsg::View(_)) {
            self.joined_by_frame(&m);
        }
        let Some(g) = self.game.as_mut() else { return };
        match &m {
            ServerMsg::Notice(Notice::GameCrashed) => {
                return self.to_lobby(Some("The game stopped unexpectedly. Join it again to resume it.".into()));
            }
            ServerMsg::Notice(n) => {
                let (mut text, alarm) = notice_text(n, &g.names);
                if let Notice::AreaTaken { area, holder } = n {
                    if self.claim_sent.take().is_some_and(|a| &a == area) {
                        text = format!("Could not claim {area}: {holder} took it first");
                    }
                }
                let t = g.sim_time();
                g.log.push(t, text, alarm);
                if let Notice::Rejected { cmd, by, .. } = n {
                    g.blocking = by.as_deref().and_then(|r| g.names.route_entrance(r)).map(str::to_string);
                    match entrance_of(cmd) {
                        Some(e) => g.refused = Some((e.to_string(), self.now + REFUSED_S)),
                        // Points (and berths) have no entrance: outline the blocking
                        // route alone, or nothing; the newest refusal wins.
                        None => g.refused = g.blocking.clone().map(|b| (b, self.now + REFUSED_S)),
                    }
                }
            }
            ServerMsg::Layout(l) => {
                g.layout_gen += 1;
                // A layout that is no longer a spectator's: the claim worked.
                if l.area.is_some() {
                    self.claim_sent = None;
                }
            }
            ServerMsg::Lesson(v) => {
                g.lesson = Some(v.clone());
                // Tell the new step what the screen shows.
                g.screen_sent = None;
            }
            ServerMsg::View(_) | ServerMsg::Delta(_) => {}
        }
        let is_layout = matches!(m, ServerMsg::Layout(_));
        let reply = g.bot.receive(m);
        g.bot.take_notices();
        let mut claim = None;
        if let (true, Some(l)) = (is_layout, g.bot.layout()) {
            g.names = Names::new(l);
            // The creator's chosen area, once, if it is still a spectator's layout.
            if let Some(a) = self.claim_on_join.take().filter(|a| l.area.is_none() && l.areas.contains(a)) {
                claim = Some(ClientMsg::Claim { area: a });
            }
        }
        if let (Some(sel), Some(l)) = (g.selected.as_deref(), g.bot.layout()) {
            if !crate::select::can_enter(l, sel) {
                g.selected = None;
            }
        }
        if let Some(r) = reply {
            self.send_game(r);
        }
        if let Some(c) = claim {
            if let ClientMsg::Claim { area } = &c {
                self.claim_sent = Some(area.clone());
            }
            self.send_game(c);
        }
    }

    /// A layout or view while a join waits for its `joined`: the `joined`
    /// may have been lost (an overflowing outbox on the front), and what
    /// follows it says as much. A view only counts from the lobby, since a
    /// view of the game being left can still be on its way; a layout only
    /// comes after a `joined`.
    fn joined_by_frame(&mut self, m: &ServerMsg) {
        let Some(Joining { game: Some(id), .. }) = self.joining.clone() else { return };
        let you = match m {
            ServerMsg::Layout(l) => l.you.clone(),
            _ => self.me.clone().unwrap_or_default(),
        };
        match &self.game {
            Some(g) if g.id == id => {}
            Some(_) if !matches!(m, ServerMsg::Layout(_)) => return,
            _ => self.game = Some(InGame::new(id.clone(), you)),
        }
        self.joining = None;
        self.rejoin = Some(id);
        self.lobby_note = None;
    }

    // ---- lobby ----

    pub fn games(&self) -> &[GameInfo] {
        &self.games
    }

    /// The game we are in, while the front lists it as being prepared.
    pub fn preparing(&self) -> Option<protocol::Preparing> {
        let id = &self.game.as_ref()?.id;
        self.games.iter().find(|g| &g.id == id)?.preparing
    }

    pub fn layouts(&self) -> &[LayoutInfo] {
        &self.layouts
    }

    /// The tutorial lessons the front offers.
    pub fn lessons(&self) -> &[LessonInfo] {
        &self.lessons
    }

    /// Start a private tutorial of `lesson` (answered like `create_game`).
    pub fn start_lesson(&mut self, lesson: &str) {
        if self.send(ClientFrame::Lobby(LobbyMsg::StartLesson { lesson: lesson.to_string() })) {
            self.joining = Some(Joining { game: None, rejoin: false });
        }
    }

    pub fn lobby_note(&self) -> Option<&str> {
        self.lobby_note.as_deref()
    }

    pub fn refresh(&mut self) {
        self.send(ClientFrame::Lobby(LobbyMsg::ListGames));
    }

    /// `start` is "HH:MM" or "HH:MM:SS"; the front checks it.
    pub fn create_game(&mut self, layout: &str, seed: Option<u64>, start: Option<String>) {
        self.claim_on_join = None;
        if self.send(ClientFrame::Lobby(LobbyMsg::CreateGame { layout: layout.to_string(), seed, start })) {
            self.joining = Some(Joining { game: None, rejoin: false });
        }
    }

    /// `create_game`, then claim `area` as soon as the game's first layout
    /// comes (polish spec H2); `None` watches, as `create_game` does.
    pub fn create_game_in(&mut self, layout: &str, seed: Option<u64>, start: Option<String>, area: Option<&str>) {
        self.create_game(layout, seed, start);
        if self.joining.is_some() {
            self.claim_on_join = area.map(str::to_string);
        }
    }

    /// Delete a saved or crashed game (owner decision 13). The front checks
    /// who may and answers with the new games list, or an error for the lobby.
    pub fn delete_game(&mut self, game: &str) {
        self.send(ClientFrame::Lobby(LobbyMsg::DeleteGame { game: game.to_string() }));
    }

    pub fn join(&mut self, game: &str) {
        self.claim_on_join = None;
        if self.send(ClientFrame::Lobby(LobbyMsg::Join { game: game.to_string() })) {
            self.joining = Some(Joining { game: Some(game.to_string()), rejoin: false });
        }
    }

    /// Back to the lobby (the front answers with the games list). An area you
    /// hold is released first, so it does not wait out the disconnect grace
    /// as yours (polish spec M9); a tutorial ends anyway.
    pub fn leave(&mut self) {
        self.claim_on_join = None;
        if self.game.as_ref().is_some_and(|g| g.area().is_some() && g.lesson.is_none()) {
            self.send(ClientFrame::Game(ClientMsg::Release));
        }
        self.send(ClientFrame::Lobby(LobbyMsg::Leave));
        self.game = None;
        self.rejoin = None;
        self.joining = None;
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

    /// Agree to `proposal` if it is still the open one (task 12 review M2).
    pub fn agree_vote(&mut self, proposal: Proposal) {
        self.send_game(ClientMsg::VoteAgree { proposal });
    }

    /// Turn the open proposal down, or take back one's agreement (polish
    /// spec M8).
    pub fn decline_vote(&mut self) {
        self.send_game(ClientMsg::VoteDecline);
    }

    pub fn command(&mut self, cmd: PlayerCommand) {
        self.send_game(ClientMsg::Command { cmd });
    }

    // ---- tutorial ----

    fn in_lesson(&self) -> bool {
        self.game.as_ref().is_some_and(|g| g.lesson.is_some())
    }

    /// The step said "press Next".
    pub fn lesson_next(&mut self) {
        if self.in_lesson() {
            self.send_game(ClientMsg::LessonNext);
        }
    }

    pub fn lesson_restart_step(&mut self) {
        if self.in_lesson() {
            self.send_game(ClientMsg::LessonRestartStep);
        }
    }

    /// Back to the first step. The game forgets what the screen showed, so
    /// the chosen entrance is dropped here too: the `lesson` that answers
    /// makes the next `report_screen` tell the lesson afresh, and a stale
    /// selection would complete a `selected` step at once.
    pub fn lesson_restart(&mut self) {
        if self.in_lesson() {
            // Only a restart that went out forgets the screen.
            if self.link == Link::Open {
                self.send_game(ClientMsg::LessonRestart);
                if let Some(g) = self.game.as_mut() {
                    g.selected = None;
                    g.screen_sent = None;
                }
            } else {
                self.send_game(ClientMsg::LessonRestart);
            }
        }
    }

    /// Call every frame with the side panel's tab: in a tutorial, the
    /// lesson hears whenever the tab or the chosen entrance changes. Silent
    /// while the link is down or a (re)join is unanswered; the `lesson` that
    /// follows the join makes the next call report afresh.
    pub fn report_screen(&mut self, tab: &str) {
        let Some(g) = self.game.as_ref() else { return };
        if g.lesson.as_ref().is_none_or(|v| v.done) {
            return;
        }
        let now = (Some(tab.to_string()), g.selected.clone());
        if g.screen_sent.as_ref() == Some(&now) || self.link != Link::Open || self.joining.is_some() {
            return;
        }
        let (tab, selected) = now.clone();
        self.send_game(ClientMsg::LessonUi { tab, selected });
        if let Some(g) = self.game.as_mut() {
            g.screen_sent = Some(now);
        }
    }
}

/// The signal a command was about, outlined when it is refused.
pub fn entrance_of(cmd: &PlayerCommand) -> Option<&str> {
    match cmd {
        PlayerCommand::SetRoute { entrance, .. }
        | PlayerCommand::CancelRoute { entrance }
        | PlayerCommand::SetAutoWorking { entrance, .. } => Some(entrance),
        _ => None,
    }
}
