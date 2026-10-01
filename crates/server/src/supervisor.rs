//! The front's supervisor (spec §2.2, §3.6, §8): the lobby, one child
//! process per live game, who is in which game, routing between client
//! sockets and game sockets, duplicate logins, crashes and shutdown.
//!
//! All state sits behind one `std::sync::Mutex` that is never held across an
//! `.await`. Each game's `ToGame` channel exists from the moment the game is
//! created or joined (`Starting`), so `Connect` and client messages sent
//! while the child is still starting queue in order.

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use game::ROBOT;
use game::save::read_summary;
use ipc::{FromGame, StatusMsg, ToGame, read_frame, write_frame};
use protocol::{
    AreaHolder, ClientFrame, ClientMsg, GameInfo, GameState, LobbyMsg, LobbyReply, Notice, ServerFrame, ServerMsg, codes,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::OwnedReadHalf;
use tokio::process::{Child, Command};
use tokio::sync::{Notify, mpsc};
use tokio::time::{Instant, sleep, timeout};

use crate::layouts::{Layouts, new_game_id, valid_game_id};
use crate::lessons::Lessons;
use crate::outbox::{Outbox, Pushed};
use crate::process::{EXIT_SEED_TOO_SLOW, PREPARING_FAILED, normalise_start};

/// Game processes running at once, at most (tutorials not counted).
pub const MAX_LIVE_GAMES: usize = 8;
/// Tutorial games running at once, at most (tutorial spec §3).
pub const MAX_TUTORIALS: usize = 8;
/// A tutorial whose player's socket closed waits this long for them to
/// reconnect and rejoin (a dropped connection) before it ends; a page
/// reload lands in the lobby, where tutorials aren't listed, so it does not
/// count. Leaving it from the lobby ends it at once.
pub const TUTORIAL_EMPTY_EXIT_S: u64 = 60;
/// How long a new game process has to start listening.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug)]
pub struct SupervisorConfig {
    pub game_bin: PathBuf,
    pub saves_dir: PathBuf,
    /// 0700; one socket per game.
    pub sockets_dir: PathBuf,
    /// Passed to every game as `--empty-exit-s` (`process::EMPTY_EXIT_S`
    /// outside tests).
    pub empty_exit_s: u64,
    /// May delete any saved or crashed game (owner decision 13).
    pub admins: BTreeSet<String>,
}

/// A client socket's handle on the supervisor.
pub struct Attached {
    /// Identifies this socket among the user's connections over time.
    pub conn: u64,
    pub outbox: Arc<Outbox>,
}

struct Client {
    conn: u64,
    outbox: Arc<Outbox>,
    game: Option<String>,
}

enum Phase {
    Starting,
    Running,
    Crashed(String),
}

struct Entry {
    /// The layout, or a tutorial's lesson id.
    layout: String,
    /// A tutorial's player: nobody else may join it, it is never listed,
    /// and it ends when they leave (tutorial spec §3).
    owner: Option<String>,
    /// A tutorial that has been sent `Shutdown`: nobody may join it.
    ending: bool,
    phase: Phase,
    tx: Option<mpsc::UnboundedSender<ToGame>>,
    status: Option<StatusMsg>,
    pid: Option<u32>,
    /// Notified to kill the process (shutdown grace over).
    kill: Arc<Notify>,
}

#[derive(Default)]
struct State {
    clients: BTreeMap<String, Client>,
    games: BTreeMap<String, Entry>,
    /// Shutting down: no new games.
    closing: bool,
    /// A games-list push is being built (`push_games`).
    scanning: bool,
    /// Who the next games-list push goes to, if one is due.
    push_due: Option<Audience>,
}

/// Who a games-list push goes to; one for everyone covers the lobby.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Audience {
    Lobby,
    Everyone,
}

enum Start {
    Create { world: PathBuf, layout: String, seed: u64, start: Option<String>, creator: String },
    /// `current`: the layout file the save was made from, if still listed.
    Resume { current: Option<PathBuf> },
    /// A tutorial of the lesson in this directory.
    Lesson { dir: PathBuf },
}

pub struct Supervisor {
    cfg: SupervisorConfig,
    layouts: Layouts,
    lessons: Lessons,
    state: Mutex<State>,
    next_conn: AtomicU64,
    /// Added to every games-list scan for a push (tests: a slow disk).
    scan_delay_ms: AtomicU64,
}

fn frame(msg: LobbyReply) -> ServerFrame {
    ServerFrame::Lobby(msg)
}

fn notice(n: Notice) -> ServerFrame {
    ServerFrame::Game(ServerMsg::Notice(n))
}

/// The game refuses `robot` and `seed` as players, but a `Connect` for one
/// would still count as somebody visiting, so the front never lets them
/// into a game.
fn reserved_name(user: &str) -> ServerFrame {
    ServerFrame::error(codes::RESERVED_NAME, format!("`{}` is a reserved name", user.to_ascii_lowercase()))
}

/// `robot` (the automatic signaller) or `seed` (its sender while a game is
/// prepared, timetables spec P7), in any letter case.
pub fn is_reserved(user: &str) -> bool {
    user.eq_ignore_ascii_case(ROBOT) || user.eq_ignore_ascii_case(game::seed::SEED)
}

fn send_to(st: &State, game: &str, msg: ToGame) -> bool {
    st.games.get(game).and_then(|e| e.tx.as_ref()).is_some_and(|tx| tx.send(msg).is_ok())
}

/// Every frame for a client goes through here. A push that overflowed the
/// queue cleared it and left it waiting for a full view, so the client's
/// game (if any) is asked for one.
fn push(st: &State, user: &str, c: &Client, f: ServerFrame) {
    if c.outbox.push(f) == Pushed::Overflowed {
        if let Some(g) = &c.game {
            send_to(st, g, ToGame::Client { player: user.to_string(), msg: ClientMsg::Resync });
        }
    }
}

/// How a child process ended, for the crash reason.
fn exit_description(status: ExitStatus) -> String {
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit status {code}"),
        (None, Some(sig)) => format!("killed by signal {sig}"),
        (None, None) => status.to_string(),
    }
}

/// Games that have a save file in `dir`, with its summary or why it could
/// not be read.
fn scan_saves(dir: &Path) -> BTreeMap<String, Result<game::save::SaveSummary, String>> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(id) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".sqlite")) else { continue };
        if valid_game_id(id) {
            out.insert(id.to_string(), read_summary(&path).map_err(|e| e.to_string()));
        }
    }
    out
}

fn private_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(|e| format!("{}: {e}", path.display()))
}

/// Remove half-built saves (`<id>.sqlite.seeding*`) a game left when it
/// died with the front (a host crash): no game process can own one yet.
fn sweep_half_built(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if entry.file_name().to_str().is_some_and(|n| n.contains(".sqlite.seeding")) {
            match std::fs::remove_file(entry.path()) {
                Ok(()) => eprintln!("signalbox-server: removed the half-built {}", entry.path().display()),
                Err(e) => eprintln!("signalbox-server: cannot remove {}: {e}", entry.path().display()),
            }
        }
    }
}

async fn connect(socket: &Path, child: &mut Child) -> Result<UnixStream, String> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        if let Ok(s) = UnixStream::connect(socket).await {
            return Ok(s);
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("the game exited before listening ({status})"));
        }
        if Instant::now() >= deadline {
            return Err("the game did not listen within 30 s".into());
        }
        sleep(Duration::from_millis(50)).await;
    }
}

impl Supervisor {
    /// Creates the saves and sockets directories (mode 0700) if needed.
    /// No tutorials: see `with_lessons`.
    pub fn new(cfg: SupervisorConfig, layouts: Layouts) -> Result<Arc<Supervisor>, String> {
        Supervisor::with_lessons(cfg, layouts, Lessons::default())
    }

    /// `new`, offering `lessons` as tutorials.
    pub fn with_lessons(cfg: SupervisorConfig, layouts: Layouts, lessons: Lessons) -> Result<Arc<Supervisor>, String> {
        private_dir(&cfg.saves_dir)?;
        private_dir(&cfg.sockets_dir)?;
        sweep_half_built(&cfg.saves_dir);
        let state = Mutex::new(State::default());
        let (next_conn, scan_delay_ms) = (AtomicU64::new(0), AtomicU64::new(0));
        Ok(Arc::new(Supervisor { cfg, layouts, lessons, state, next_conn, scan_delay_ms }))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("supervisor lock")
    }

    pub fn layouts(&self) -> &Layouts {
        &self.layouts
    }

    fn save_path(&self, id: &str) -> PathBuf {
        self.cfg.saves_dir.join(format!("{id}.sqlite"))
    }

    // ---- client sockets ----

    /// A socket for `user` opened. A socket the user already had gets
    /// `notice replaced` and is closed; the new one takes over its game
    /// (the game sees a `Connect` and resyncs, never a `Disconnect`).
    /// While the front is stopping, the socket gets a closed outbox and
    /// is not registered: it replaces nothing and joins nothing.
    pub fn attach(&self, user: &str) -> Attached {
        let conn = self.next_conn.fetch_add(1, Ordering::Relaxed) + 1;
        let outbox = Arc::new(Outbox::new());
        let mut st = self.lock();
        if st.closing {
            outbox.close();
            return Attached { conn, outbox };
        }
        let mut game = st.clients.remove(user).and_then(|old| {
            old.outbox.push(notice(Notice::Replaced));
            old.outbox.close();
            old.game
        });
        if let Some(g) = game.clone() {
            // Deltas already on their way to the old socket must not reach
            // this one before the view the `Connect` below asks for.
            outbox.await_view();
            if send_to(&st, &g, ToGame::Connect { player: user.to_string() }) {
                outbox.push(frame(LobbyReply::Joined { game: g, you: user.to_string() }));
            } else {
                game = None;
            }
        }
        st.clients.insert(user.to_string(), Client { conn, outbox: outbox.clone(), game });
        Attached { conn, outbox }
    }

    /// The socket `conn` of `user` closed. Only the user's current socket
    /// disconnects them from their game.
    pub fn detach(&self, user: &str, conn: u64) {
        let mut st = self.lock();
        if st.clients.get(user).is_some_and(|c| c.conn == conn) {
            if let Some(g) = st.clients.remove(user).and_then(|c| c.game) {
                send_to(&st, &g, ToGame::Disconnect { player: user.to_string() });
            }
        }
    }

    fn current<'a>(st: &'a State, user: &str, conn: u64) -> Option<&'a Client> {
        st.clients.get(user).filter(|c| c.conn == conn)
    }

    /// Send `f` to `user`'s socket `conn`, if it is still their current one.
    pub fn reply(&self, user: &str, conn: u64, f: ServerFrame) {
        let st = self.lock();
        if let Some(c) = Self::current(&st, user, conn) {
            push(&st, user, c, f);
        }
    }

    /// One text frame from a client.
    pub fn handle_text(self: &Arc<Self>, user: &str, conn: u64, text: &str) {
        match ClientFrame::from_json(text) {
            Ok(f) => self.handle_frame(user, conn, f),
            Err(e) => self.reply(user, conn, ServerFrame::error(e.code(), e.to_string())),
        }
    }

    pub fn handle_frame(self: &Arc<Self>, user: &str, conn: u64, f: ClientFrame) {
        match f {
            ClientFrame::Game(msg) => self.to_game(user, conn, msg),
            ClientFrame::Lobby(LobbyMsg::ListGames) => {
                self.reply(user, conn, frame(LobbyReply::Games { games: self.list_games_for(user) }))
            }
            ClientFrame::Lobby(LobbyMsg::ListLayouts) => {
                self.reply(user, conn, frame(LobbyReply::Layouts { layouts: self.layouts.infos() }))
            }
            ClientFrame::Lobby(LobbyMsg::Leave) => {
                {
                    let mut st = self.lock();
                    if Self::current(&st, user, conn).is_some() {
                        Self::leave(&mut st, user, None);
                    }
                }
                self.reply(user, conn, frame(LobbyReply::Games { games: self.list_games_for(user) }));
            }
            ClientFrame::Lobby(LobbyMsg::CreateGame { layout, seed, start }) => self.create(user, conn, layout, seed, start),
            ClientFrame::Lobby(LobbyMsg::Join { game }) => self.join(user, conn, game),
            ClientFrame::Lobby(LobbyMsg::DeleteGame { game }) => self.delete_game(user, conn, game),
            ClientFrame::Lobby(LobbyMsg::ListLessons) => {
                self.reply(user, conn, frame(LobbyReply::Lessons { lessons: self.lessons.infos() }))
            }
            ClientFrame::Lobby(LobbyMsg::StartLesson { lesson }) => self.start_lesson(user, conn, lesson),
        }
    }

    fn to_game(&self, user: &str, conn: u64, msg: ClientMsg) {
        let st = self.lock();
        let Some(c) = Self::current(&st, user, conn) else { return };
        let sent = c.game.as_deref().is_some_and(|g| send_to(&st, g, ToGame::Client { player: user.to_string(), msg }));
        if !sent {
            push(&st, user, c, ServerFrame::error(codes::NOT_IN_GAME, "join a game first"));
        }
    }

    /// `user` leaves their game, and every tutorial of theirs but `keep`
    /// ends: the one they were in, and any a closed socket left waiting
    /// (a reload, then a new lesson or game, is leaving it too).
    fn leave(st: &mut State, user: &str, keep: Option<&str>) {
        if let Some(g) = st.clients.get_mut(user).and_then(|c| c.game.take()) {
            send_to(st, &g, ToGame::Disconnect { player: user.to_string() });
        }
        let mine: Vec<String> = st
            .games
            .iter()
            .filter(|(id, e)| e.owner.as_deref() == Some(user) && !e.ending && Some(id.as_str()) != keep)
            .map(|(id, _)| id.clone())
            .collect();
        for g in &mine {
            // A repeated `Disconnect` is a no-op in the game.
            send_to(st, g, ToGame::Disconnect { player: user.to_string() });
            send_to(st, g, ToGame::Shutdown);
            if let Some(e) = st.games.get_mut(g) {
                e.ending = true;
            }
        }
    }

    /// `game` is a tutorial `user` may not join: someone else's, or one
    /// of theirs that is ending. As far as they know, it does not exist.
    fn hidden_tutorial(st: &State, user: &str, game: &str) -> bool {
        st.games.get(game).is_some_and(|e| e.owner.as_deref().is_some_and(|o| o != user || e.ending))
    }

    fn enter(st: &mut State, user: &str, game: &str) {
        let Some(c) = st.clients.get_mut(user) else { return };
        c.game = Some(game.to_string());
        let c = &st.clients[user];
        push(st, user, c, frame(LobbyReply::Joined { game: game.to_string(), you: user.to_string() }));
        send_to(st, game, ToGame::Connect { player: user.to_string() });
    }

    fn room_for_one_more(st: &State) -> Result<(), ServerFrame> {
        if st.closing {
            return Err(ServerFrame::error(codes::TOO_MANY_GAMES, "the server is stopping"));
        }
        let live = st.games.values().filter(|e| e.owner.is_none() && !matches!(e.phase, Phase::Crashed(_))).count();
        if live >= MAX_LIVE_GAMES {
            return Err(ServerFrame::error(codes::TOO_MANY_GAMES, format!("at most {MAX_LIVE_GAMES} games run at once")));
        }
        Ok(())
    }

    /// `user`'s own running tutorials do not count: starting another ends
    /// them. Ending ones do, until their process exits, so a burst of
    /// starts never runs more than `MAX_TUTORIALS` processes.
    fn room_for_a_tutorial(st: &State, user: &str) -> Result<(), ServerFrame> {
        if st.closing {
            return Err(ServerFrame::error(codes::TOO_MANY_GAMES, "the server is stopping"));
        }
        let counted = st.games.values().filter(|e| e.owner.as_deref().is_some_and(|o| o != user || e.ending)).count();
        if counted >= MAX_TUTORIALS {
            return Err(ServerFrame::error(codes::TOO_MANY_GAMES, format!("at most {MAX_TUTORIALS} tutorials run at once")));
        }
        Ok(())
    }

    fn insert_starting(st: &mut State, id: &str, layout: String, owner: Option<String>) -> mpsc::UnboundedReceiver<ToGame> {
        let (tx, rx) = mpsc::unbounded_channel();
        let kill = Arc::new(Notify::new());
        let entry = Entry { layout, owner, ending: false, phase: Phase::Starting, tx: Some(tx), status: None, pid: None, kill };
        st.games.insert(id.to_string(), entry);
        rx
    }

    /// A new game id: no live game and no save has it.
    fn fresh_id(&self, st: &State) -> String {
        loop {
            let id = new_game_id();
            if !st.games.contains_key(&id) && !self.save_path(&id).exists() {
                return id;
            }
        }
    }

    /// Start a private tutorial of `lesson` for `user` (tutorial spec §3).
    fn start_lesson(self: &Arc<Self>, user: &str, conn: u64, lesson: String) {
        if is_reserved(user) {
            return self.reply(user, conn, reserved_name(user));
        }
        let Some(dir) = self.lessons.path(&lesson) else {
            return self.reply(user, conn, ServerFrame::error(codes::UNKNOWN_LESSON, format!("no lesson `{lesson}`")));
        };
        let mut st = self.lock();
        let Some(c) = Self::current(&st, user, conn) else { return };
        if let Err(f) = Self::room_for_a_tutorial(&st, user) {
            push(&st, user, c, f);
            return;
        }
        let id = self.fresh_id(&st);
        Self::leave(&mut st, user, None);
        let rx = Self::insert_starting(&mut st, &id, lesson, Some(user.to_string()));
        Self::enter(&mut st, user, &id);
        drop(st);
        self.spawn_game(id, rx, Start::Lesson { dir });
    }

    fn create(self: &Arc<Self>, user: &str, conn: u64, layout: String, seed: Option<u64>, start: Option<String>) {
        if is_reserved(user) {
            return self.reply(user, conn, reserved_name(user));
        }
        let Some(world) = self.layouts.path(&layout) else {
            return self.reply(user, conn, ServerFrame::error(codes::UNKNOWN_LAYOUT, format!("no layout `{layout}`")));
        };
        let start = match start.as_deref().map(normalise_start) {
            None => None,
            Some(Some(s)) => Some(s),
            Some(None) => {
                return self.reply(user, conn, ServerFrame::error(codes::BAD_START, "a start time is HH:MM or HH:MM:SS before 24:00"));
            }
        };
        let mut st = self.lock();
        let Some(c) = Self::current(&st, user, conn) else { return };
        if let Err(f) = Self::room_for_one_more(&st) {
            push(&st, user, c, f);
            return;
        }
        let id = self.fresh_id(&st);
        Self::leave(&mut st, user, None);
        let rx = Self::insert_starting(&mut st, &id, layout.clone(), None);
        Self::enter(&mut st, user, &id);
        drop(st);
        let seed = seed.unwrap_or_else(rand::random);
        self.spawn_game(id, rx, Start::Create { world, layout, seed, start, creator: user.to_string() });
    }

    fn join(self: &Arc<Self>, user: &str, conn: u64, game: String) {
        if is_reserved(user) {
            return self.reply(user, conn, reserved_name(user));
        }
        if !valid_game_id(&game) {
            return self.reply(user, conn, ServerFrame::error(codes::UNKNOWN_GAME, format!("no game `{game}`")));
        }
        {
            let mut st = self.lock();
            let Some(c) = Self::current(&st, user, conn) else { return };
            // Someone else's tutorial, or your ending one, does not exist.
            if Self::hidden_tutorial(&st, user, &game) {
                push(&st, user, c, ServerFrame::error(codes::UNKNOWN_GAME, format!("no game `{game}`")));
                return;
            }
            if c.game.as_deref() == Some(game.as_str()) {
                // Already in it: leaving and entering again would look like
                // an empty game to the game (pause, save). A fresh view is
                // all a second `join` can want.
                push(&st, user, c, frame(LobbyReply::Joined { game: game.clone(), you: user.to_string() }));
                send_to(&st, &game, ToGame::Client { player: user.to_string(), msg: ClientMsg::Resync });
                return;
            }
            if matches!(st.games.get(&game).map(|e| &e.phase), Some(Phase::Starting | Phase::Running)) {
                Self::leave(&mut st, user, Some(&game));
                Self::enter(&mut st, user, &game);
                return;
            }
        }
        let path = self.save_path(&game);
        if !path.exists() {
            return self.reply(user, conn, ServerFrame::error(codes::UNKNOWN_GAME, format!("no game `{game}`")));
        }
        let layout = read_summary(&path).map(|s| s.layout).unwrap_or_else(|_| "?".into());
        let mut st = self.lock();
        let Some(c) = Self::current(&st, user, conn) else { return };
        if Self::hidden_tutorial(&st, user, &game) {
            push(&st, user, c, ServerFrame::error(codes::UNKNOWN_GAME, format!("no game `{game}`")));
            return;
        }
        if matches!(st.games.get(&game).map(|e| &e.phase), Some(Phase::Starting | Phase::Running)) {
            // Someone resumed it meanwhile.
            Self::leave(&mut st, user, Some(&game));
            Self::enter(&mut st, user, &game);
            return;
        }
        if !path.exists() {
            // Deleted meanwhile (deletion holds the lock while it unlinks).
            push(&st, user, c, ServerFrame::error(codes::UNKNOWN_GAME, format!("no game `{game}`")));
            return;
        }
        if let Err(f) = Self::room_for_one_more(&st) {
            push(&st, user, c, f);
            return;
        }
        Self::leave(&mut st, user, Some(&game));
        let current = self.layouts.path(&layout);
        let rx = Self::insert_starting(&mut st, &game, layout, None);
        Self::enter(&mut st, user, &game);
        drop(st);
        self.spawn_game(game, rx, Start::Resume { current });
    }

    // ---- deleting (owner decision 13) ----

    /// The game's creator, or an admin.
    fn may_delete(&self, user: &str, creator: Option<&str>) -> bool {
        self.cfg.admins.contains(user) || creator == Some(user)
    }

    /// Delete a saved or crashed game: its save file and the SQLite files
    /// beside it, and a crashed game's entry. Everyone gets the new list.
    fn delete_game(self: &Arc<Self>, user: &str, conn: u64, game: String) {
        if !valid_game_id(&game) {
            return self.reply(user, conn, ServerFrame::error(codes::UNKNOWN_GAME, format!("no game `{game}`")));
        }
        let path = self.save_path(&game);
        let creator = read_summary(&path).ok().and_then(|s| s.creator);
        {
            let mut st = self.lock();
            let Some(c) = Self::current(&st, user, conn) else { return };
            let refuse = |code: &str, why: String| push(&st, user, c, ServerFrame::error(code, why));
            // Tutorials keep no save and end on their own.
            if st.games.get(&game).is_some_and(|e| e.owner.is_some()) {
                return refuse(codes::UNKNOWN_GAME, format!("no game `{game}`"));
            }
            match st.games.get(&game).map(|e| &e.phase) {
                Some(Phase::Starting | Phase::Running) => {
                    return refuse(codes::GAME_RUNNING, format!("`{game}` is running; it can be deleted once it is saved"));
                }
                None if !path.exists() => return refuse(codes::UNKNOWN_GAME, format!("no game `{game}`")),
                Some(Phase::Crashed(_)) | None => {}
            }
            if !self.may_delete(user, creator.as_deref()) {
                return refuse(codes::NOT_ALLOWED, "only its creator or an admin may delete a game".into());
            }
            for suffix in ["", "-wal", "-shm"] {
                let file = self.cfg.saves_dir.join(format!("{game}.sqlite{suffix}"));
                match std::fs::remove_file(&file) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        eprintln!("signalbox-server: cannot delete {}: {e}", file.display());
                        return refuse(codes::SAVE_FAILED, format!("could not delete `{game}`: {e}"));
                    }
                }
            }
            st.games.remove(&game);
        }
        eprintln!("signalbox-server: {user} deleted {game}");
        self.broadcast_games();
    }

    /// `games` as `user` may act on them.
    fn for_user(&self, mut games: Vec<GameInfo>, user: &str) -> Vec<GameInfo> {
        for g in &mut games {
            g.can_delete = g.state != GameState::Running && self.may_delete(user, g.creator.as_deref());
        }
        games
    }

    /// Tests: every games-list scan for a push takes `d` longer (a slow disk).
    pub fn slow_games_scans(&self, d: Duration) {
        self.scan_delay_ms.store(d.as_millis() as u64, Ordering::Relaxed);
    }

    /// No games-list push is being built or waiting to be.
    pub fn games_pushes_settled(&self) -> bool {
        let st = self.lock();
        !st.scanning && st.push_due.is_none()
    }

    /// Every client gets the games list, as they may act on it.
    fn broadcast_games(self: &Arc<Self>) {
        self.push_games(Audience::Everyone);
    }

    /// The games list to every client in the lobby (not in a game).
    fn broadcast_lobby_games(self: &Arc<Self>) {
        self.push_games(Audience::Lobby);
    }

    /// The games list is built on a blocking thread, never on the caller's
    /// task: it reads every save, and the front runs on one thread (polish
    /// spec M9, review M1). One build at a time; whatever asks for a push
    /// meanwhile shares one more, built after it.
    fn push_games(self: &Arc<Self>, to: Audience) {
        {
            let mut st = self.lock();
            st.push_due = st.push_due.max(Some(to));
            if st.scanning {
                return;
            }
            st.scanning = true;
        }
        let sup = self.clone();
        tokio::task::spawn_blocking(move || sup.push_games_while_due());
    }

    fn push_games_while_due(&self) {
        let to_whom = |to: Audience, c: &Client| to == Audience::Everyone || c.game.is_none();
        loop {
            let to = {
                let mut st = self.lock();
                let Some(to) = st.push_due.take() else {
                    st.scanning = false;
                    return;
                };
                // Nobody to tell: nothing is built, so no save is read
                // (ruling D1).
                if !st.clients.values().any(|c| to_whom(to, c)) {
                    continue;
                }
                to
            };
            std::thread::sleep(Duration::from_millis(self.scan_delay_ms.load(Ordering::Relaxed)));
            let games = self.list_games();
            let st = self.lock();
            for (user, c) in st.clients.iter().filter(|(_, c)| to_whom(to, c)) {
                push(&st, user, c, frame(LobbyReply::Games { games: self.for_user(games.clone(), user) }));
            }
        }
    }

    // ---- the lobby ----

    /// The games list as `user` sees it (`can_delete` set for them).
    pub fn list_games_for(&self, user: &str) -> Vec<GameInfo> {
        self.for_user(self.list_games(), user)
    }

    /// Every game: live ones from memory, the rest from their save files
    /// (`can_delete` false: see `list_games_for`).
    pub fn list_games(&self) -> Vec<GameInfo> {
        let saves = scan_saves(&self.cfg.saves_dir);
        let st = self.lock();
        let mut out: BTreeMap<String, GameInfo> = BTreeMap::new();
        for (id, sum) in &saves {
            let info = match sum {
                Ok(s) => GameInfo {
                    id: id.clone(),
                    layout: s.layout.clone(),
                    state: GameState::Saved,
                    sim_time: s.sim_time,
                    areas: s.areas.iter().map(|a| AreaHolder { name: a.clone(), holder: None }).collect(),
                    players: vec![],
                    error: None,
                    creator: s.creator.clone(),
                    can_delete: false,
                    preparing: None,
                },
                Err(e) => GameInfo {
                    id: id.clone(),
                    layout: "?".into(),
                    state: GameState::Crashed,
                    sim_time: 0.0,
                    areas: vec![],
                    players: vec![],
                    error: Some(e.clone()),
                    creator: None,
                    can_delete: false,
                    preparing: None,
                },
            };
            out.insert(id.clone(), info);
        }
        for (id, e) in st.games.iter().filter(|(_, e)| e.owner.is_none()) {
            let base = out.remove(id);
            let areas_order: Vec<String> = match saves.get(id) {
                Some(Ok(s)) => s.areas.clone(),
                _ => e.status.as_ref().map(|s| s.holders.keys().cloned().collect()).unwrap_or_default(),
            };
            let mut info = base.unwrap_or(GameInfo {
                id: id.clone(),
                layout: e.layout.clone(),
                state: GameState::Saved,
                sim_time: 0.0,
                areas: areas_order.iter().map(|a| AreaHolder { name: a.clone(), holder: None }).collect(),
                players: vec![],
                error: None,
                creator: None,
                can_delete: false,
                preparing: None,
            });
            info.layout = e.layout.clone();
            match &e.phase {
                Phase::Starting | Phase::Running => {
                    info.state = GameState::Running;
                    info.error = None;
                    info.preparing = e.status.as_ref().and_then(|s| s.preparing);
                    if let Some(s) = &e.status {
                        info.sim_time = s.sim_time;
                        info.areas = areas_order
                            .iter()
                            .map(|a| AreaHolder { name: a.clone(), holder: s.holders.get(a).cloned().flatten() })
                            .collect();
                        info.players = s.players.iter().filter(|p| p.connected).map(|p| p.name.clone()).collect();
                    }
                }
                Phase::Crashed(why) => {
                    info.state = GameState::Crashed;
                    info.error = Some(why.clone());
                    info.players = vec![];
                }
            }
            out.insert(id.clone(), info);
        }
        out.into_values().collect()
    }

    // ---- game processes ----

    fn spawn_game(self: &Arc<Self>, id: String, rx: mpsc::UnboundedReceiver<ToGame>, start: Start) {
        let sup = self.clone();
        tokio::spawn(async move { sup.run_game(id, rx, start).await });
    }

    async fn run_game(self: Arc<Self>, id: String, rx: mpsc::UnboundedReceiver<ToGame>, start: Start) {
        let socket = self.cfg.sockets_dir.join(format!("{id}.sock"));
        let _ = std::fs::remove_file(&socket);
        let mut cmd = Command::new(&self.cfg.game_bin);
        match &start {
            Start::Lesson { dir } => {
                cmd.arg("--lesson").arg(dir).arg("--socket").arg(&socket);
                cmd.arg("--empty-exit-s").arg(TUTORIAL_EMPTY_EXIT_S.min(self.cfg.empty_exit_s).to_string());
            }
            Start::Create { .. } | Start::Resume { .. } => {
                cmd.arg("--save").arg(self.save_path(&id)).arg("--socket").arg(&socket);
                cmd.arg("--empty-exit-s").arg(self.cfg.empty_exit_s.to_string());
            }
        }
        if let Start::Create { world, layout, seed, start, creator } = &start {
            cmd.arg("--create").arg("--layout").arg(world).arg("--layout-name").arg(layout).arg("--seed").arg(seed.to_string());
            if let Some(s) = start {
                cmd.arg("--start").arg(s);
            }
            cmd.arg("--creator").arg(creator);
        }
        if let Start::Resume { current: Some(p) } = &start {
            cmd.arg("--current-layout").arg(p);
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
        // Its own process group: a terminal's Ctrl-C reaches only the front,
        // which then stops the game in order (save first).
        cmd.process_group(0);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return self.crashed(&id, format!("cannot start {}: {e}", self.cfg.game_bin.display())),
        };
        let last_line = Arc::new(Mutex::new(String::new()));
        let stderr = child.stderr.take().map(|err| {
            let (id, last_line) = (id.clone(), last_line.clone());
            tokio::spawn(async move {
                let mut lines = BufReader::new(err).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    eprintln!("[{id}] {line}");
                    *last_line.lock().expect("stderr lock") = line;
                }
            })
        });
        let kill = match self.lock().games.get(&id) {
            Some(e) => e.kill.clone(),
            None => Arc::new(Notify::new()),
        };
        let connected = tokio::select! {
            s = connect(&socket, &mut child) => Some(s),
            _ = kill.notified() => None,
        };
        let Some(connected) = connected else {
            // The front is stopping and the child never listened: it has
            // done nothing since it was created or resumed, so there is
            // nothing to lose and this is no crash. Its save file (if any)
            // speaks for it in the lobby.
            let _ = child.start_kill();
            let _ = child.wait().await;
            self.finished(&id);
            return;
        };
        let trouble = match connected {
            Ok(stream) => {
                self.set_running(&id, child.id());
                let (mut rd, mut wr) = stream.into_split();
                let mut rx = rx;
                let writer = tokio::spawn(async move {
                    while let Some(m) = rx.recv().await {
                        if write_frame(&mut wr, &m).await.is_err() {
                            break;
                        }
                    }
                });
                let r = tokio::select! {
                    r = self.read_game(&id, &mut rd) => r,
                    _ = kill.notified() => Some("killed: it did not stop in time".to_string()),
                };
                writer.abort();
                r
            }
            Err(e) => Some(e),
        };
        if trouble.is_some() {
            let _ = child.start_kill();
        }
        let status = match timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(s)) => Some(s),
            _ => {
                let _ = child.start_kill();
                child.wait().await.ok()
            }
        };
        if let Some(t) = stderr {
            let _ = timeout(Duration::from_secs(1), t).await;
        }
        let line = last_line.lock().expect("stderr lock").clone();
        // A new game with no save once its process is gone was never
        // created (a ready game always has its save): it failed or was
        // stopped while it was being prepared, and it is not listed
        // (timetables spec §3.4). This holds whatever the exit status, which
        // is lost when the front had to kill a child that had already
        // cleaned up; its last line then still tells `seed_too_slow`.
        if matches!(start, Start::Create { .. }) && !self.save_path(&id).exists() {
            let save = self.save_path(&id);
            game::seed::remove_partial(&save);
            let code = status.and_then(|s| s.code());
            let too_slow = code == Some(i32::from(EXIT_SEED_TOO_SLOW))
                || line.starts_with(&format!("signalbox-game: {}: ", codes::SEED_TOO_SLOW));
            let stopped = !too_slow && trouble.is_none() && code == Some(0);
            let why = match (stopped, line.is_empty(), trouble.clone()) {
                (true, _, _) => None,
                (false, false, _) => Some(line.clone()),
                (false, true, Some(t)) => Some(t),
                (false, true, None) => Some("the game process ended".into()),
            };
            return self.not_created(&id, too_slow, why);
        }
        if trouble.is_none() && status.is_some_and(|s| s.success()) {
            self.finished(&id);
            return;
        }
        let why = match (line.is_empty(), trouble) {
            (false, _) => line,
            (true, Some(t)) => t,
            (true, None) => "the game process ended".into(),
        };
        let why = match status {
            Some(s) => format!("{why} ({})", exit_description(s)),
            None => why,
        };
        self.crashed(&id, why);
    }

    /// Frames from a game until it closes its socket; `Some(why)` if it
    /// sent something unreadable.
    async fn read_game(self: &Arc<Self>, id: &str, rd: &mut OwnedReadHalf) -> Option<String> {
        loop {
            match read_frame::<_, FromGame>(rd).await {
                Ok(Some(m)) => self.from_game(id, m),
                Ok(None) => return None,
                Err(e) => return Some(format!("bad frame from the game: {e}")),
            }
        }
    }

    fn from_game(self: &Arc<Self>, id: &str, m: FromGame) {
        match m {
            FromGame::ToPlayer { player, msg } => {
                let st = self.lock();
                let Some(c) = st.clients.get(&player).filter(|c| c.game.as_deref() == Some(id)) else { return };
                push(&st, &player, c, ServerFrame::Game(msg));
            }
            FromGame::Status(s) => {
                // Everyone sees a game start and stop being prepared; the
                // lobby hears who holds what and who is in at once (polish
                // spec M9). A tutorial is never listed.
                let (mut preparing, mut lobby) = (false, false);
                if let Some(e) = self.lock().games.get_mut(id) {
                    preparing = e.status.as_ref().and_then(|o| o.preparing).is_some() != s.preparing.is_some();
                    lobby = e.owner.is_none()
                        && e.status.as_ref().is_none_or(|o| o.holders != s.holders || o.players != s.players);
                    e.status = Some(s);
                }
                if preparing {
                    self.broadcast_games();
                } else if lobby {
                    self.broadcast_lobby_games();
                }
            }
            // The save file is the record; nothing to keep.
            FromGame::Saved { .. } => {}
            FromGame::Log { level, message } => eprintln!("[{id}] {level:?}: {message}"),
        }
    }

    fn set_running(&self, id: &str, pid: Option<u32>) {
        if let Some(e) = self.lock().games.get_mut(id) {
            e.phase = Phase::Running;
            e.pid = pid;
        }
    }

    /// Everyone in `id` goes back to the lobby with `f`.
    fn evict(st: &mut State, id: &str, f: &ServerFrame) {
        let users: Vec<String> =
            st.clients.iter().filter(|(_, c)| c.game.as_deref() == Some(id)).map(|(u, _)| u.clone()).collect();
        for u in &users {
            if let Some(c) = st.clients.get_mut(u) {
                c.game = None;
            }
        }
        // Out of the game now, so an overflow asks no game for a view.
        for u in &users {
            push(st, u, &st.clients[u], f.clone());
        }
    }

    /// The game exited cleanly: it is saved (a tutorial is simply over) and
    /// leaves the live table.
    fn finished(&self, id: &str) {
        let mut st = self.lock();
        let f = match st.games.remove(id) {
            Some(e) if e.owner.is_some() => {
                ServerFrame::error(codes::GAME_STOPPED, "the tutorial ended; start it again from the lobby")
            }
            _ => ServerFrame::error(codes::GAME_STOPPED, "the game stopped; join it again to resume it"),
        };
        Self::evict(&mut st, id, &f);
    }

    /// A new game failed before it was ready (`too_slow`: its preparing ran
    /// out of time, P8; `why` is `None` when it was stopped, as the front
    /// stops): it has no save and leaves the live table, and whoever was
    /// waiting in it goes back to the lobby with why.
    fn not_created(self: &Arc<Self>, id: &str, too_slow: bool, why: Option<String>) {
        eprintln!("[{id}] not created: {}", why.as_deref().unwrap_or("stopped before it was ready"));
        let detail = |why: &str, prefix: &str| {
            let why = why.strip_prefix("signalbox-game: ").unwrap_or(why);
            why.strip_prefix(&format!("{prefix}: ")).unwrap_or(why).to_string()
        };
        let f = match why {
            Some(why) if too_slow => ServerFrame::error(
                codes::SEED_TOO_SLOW,
                format!("The game could not be prepared in time: {}. Try an earlier start.", detail(&why, codes::SEED_TOO_SLOW)),
            ),
            Some(why) => ServerFrame::error(
                codes::NOT_CREATED,
                format!("The game could not be prepared: {}.", detail(&why, PREPARING_FAILED)),
            ),
            None => ServerFrame::error(codes::NOT_CREATED, "The game was stopped before it was ready; create it again."),
        };
        {
            let mut st = self.lock();
            st.games.remove(id);
            Self::evict(&mut st, id, &f);
        }
        self.broadcast_games();
    }

    /// A game crashed: it is listed as crashed with `why`; a tutorial is
    /// simply gone (nothing to resume).
    fn crashed(&self, id: &str, why: String) {
        eprintln!("[{id}] crashed: {why}");
        let mut st = self.lock();
        match st.games.remove(id) {
            Some(e) if e.owner.is_some() => {}
            e => {
                let layout = e.map_or_else(|| "?".to_string(), |e| e.layout);
                let kill = Arc::new(Notify::new());
                let entry = Entry { layout, owner: None, ending: false, phase: Phase::Crashed(why), tx: None, status: None, pid: None, kill };
                st.games.insert(id.to_string(), entry);
            }
        }
        Self::evict(&mut st, id, &notice(Notice::GameCrashed));
    }

    /// Send `Shutdown` to every game, close every client, wait up to
    /// `grace` for the games to save and exit, then kill the rest.
    pub async fn shutdown_all(&self, grace: Duration) {
        let kills: Vec<Arc<Notify>> = {
            let mut st = self.lock();
            st.closing = true;
            for e in st.games.values() {
                if let Some(tx) = &e.tx {
                    let _ = tx.send(ToGame::Shutdown);
                }
            }
            for c in st.clients.values() {
                c.outbox.close();
            }
            st.games.values().map(|e| e.kill.clone()).collect()
        };
        let deadline = Instant::now() + grace;
        while self.live_count() > 0 && Instant::now() < deadline {
            sleep(Duration::from_millis(50)).await;
        }
        for k in &kills {
            k.notify_one();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.live_count() > 0 && Instant::now() < deadline {
            sleep(Duration::from_millis(50)).await;
        }
    }

    // ---- for tests and the index page ----

    /// Games with a process (starting or running).
    pub fn live_count(&self) -> usize {
        self.lock().games.values().filter(|e| !matches!(e.phase, Phase::Crashed(_))).count()
    }

    pub fn status(&self, game: &str) -> Option<StatusMsg> {
        self.lock().games.get(game)?.status.clone()
    }

    pub fn pid(&self, game: &str) -> Option<u32> {
        self.lock().games.get(game)?.pid
    }

    /// The game `user` is in.
    pub fn game_of(&self, user: &str) -> Option<String> {
        self.lock().clients.get(user)?.game.clone()
    }
}
