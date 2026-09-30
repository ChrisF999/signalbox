//! The front's supervisor (spec §2.2, §3.6, §8): the lobby, one child
//! process per live game, who is in which game, routing between client
//! sockets and game sockets, duplicate logins, crashes and shutdown.
//!
//! All state sits behind one `std::sync::Mutex` that is never held across an
//! `.await`. Each game's `ToGame` channel exists from the moment the game is
//! created or joined (`Starting`), so `Connect` and client messages sent
//! while the child is still starting queue in order.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
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
use crate::outbox::{Outbox, Pushed};
use crate::process::normalise_start;

/// Game processes running at once, at most.
pub const MAX_LIVE_GAMES: usize = 8;
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
    layout: String,
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
}

enum Start {
    Create { world: PathBuf, layout: String, seed: u64, start: Option<String> },
    Resume,
}

pub struct Supervisor {
    cfg: SupervisorConfig,
    layouts: Layouts,
    state: Mutex<State>,
    next_conn: AtomicU64,
}

fn frame(msg: LobbyReply) -> ServerFrame {
    ServerFrame::Lobby(msg)
}

fn notice(n: Notice) -> ServerFrame {
    ServerFrame::Game(ServerMsg::Notice(n))
}

/// The game refuses `robot` as a player, but a `Connect` for it would still
/// count as somebody visiting, so the front never lets it into a game.
fn reserved_name() -> ServerFrame {
    ServerFrame::error(codes::RESERVED_NAME, format!("`{ROBOT}` is a reserved name"))
}

/// `robot` in any letter case.
pub fn is_robot(user: &str) -> bool {
    user.eq_ignore_ascii_case(ROBOT)
}

fn send_to(st: &State, game: &str, msg: ToGame) -> bool {
    st.games.get(game).and_then(|e| e.tx.as_ref()).is_some_and(|tx| tx.send(msg).is_ok())
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
    pub fn new(cfg: SupervisorConfig, layouts: Layouts) -> Result<Arc<Supervisor>, String> {
        private_dir(&cfg.saves_dir)?;
        private_dir(&cfg.sockets_dir)?;
        Ok(Arc::new(Supervisor { cfg, layouts, state: Mutex::new(State::default()), next_conn: AtomicU64::new(0) }))
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
        if let Some(c) = Self::current(&self.lock(), user, conn) {
            c.outbox.push(f);
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
            ClientFrame::Lobby(LobbyMsg::ListGames) => self.reply(user, conn, frame(LobbyReply::Games { games: self.list_games() })),
            ClientFrame::Lobby(LobbyMsg::ListLayouts) => {
                self.reply(user, conn, frame(LobbyReply::Layouts { layouts: self.layouts.infos() }))
            }
            ClientFrame::Lobby(LobbyMsg::Leave) => {
                {
                    let mut st = self.lock();
                    if Self::current(&st, user, conn).is_some() {
                        Self::leave(&mut st, user);
                    }
                }
                self.reply(user, conn, frame(LobbyReply::Games { games: self.list_games() }));
            }
            ClientFrame::Lobby(LobbyMsg::CreateGame { layout, seed, start }) => self.create(user, conn, layout, seed, start),
            ClientFrame::Lobby(LobbyMsg::Join { game }) => self.join(user, conn, game),
        }
    }

    fn to_game(&self, user: &str, conn: u64, msg: ClientMsg) {
        let st = self.lock();
        let Some(c) = Self::current(&st, user, conn) else { return };
        let sent = c.game.as_deref().is_some_and(|g| send_to(&st, g, ToGame::Client { player: user.to_string(), msg }));
        if !sent {
            c.outbox.push(ServerFrame::error(codes::NOT_IN_GAME, "join a game first"));
        }
    }

    fn leave(st: &mut State, user: &str) {
        if let Some(g) = st.clients.get_mut(user).and_then(|c| c.game.take()) {
            send_to(st, &g, ToGame::Disconnect { player: user.to_string() });
        }
    }

    fn enter(st: &mut State, user: &str, game: &str) {
        let Some(c) = st.clients.get_mut(user) else { return };
        c.game = Some(game.to_string());
        c.outbox.push(frame(LobbyReply::Joined { game: game.to_string(), you: user.to_string() }));
        send_to(st, game, ToGame::Connect { player: user.to_string() });
    }

    fn room_for_one_more(st: &State) -> Result<(), ServerFrame> {
        if st.closing {
            return Err(ServerFrame::error(codes::TOO_MANY_GAMES, "the server is stopping"));
        }
        let live = st.games.values().filter(|e| !matches!(e.phase, Phase::Crashed(_))).count();
        if live >= MAX_LIVE_GAMES {
            return Err(ServerFrame::error(codes::TOO_MANY_GAMES, format!("at most {MAX_LIVE_GAMES} games run at once")));
        }
        Ok(())
    }

    fn insert_starting(st: &mut State, id: &str, layout: String) -> mpsc::UnboundedReceiver<ToGame> {
        let (tx, rx) = mpsc::unbounded_channel();
        let kill = Arc::new(Notify::new());
        st.games.insert(id.to_string(), Entry { layout, phase: Phase::Starting, tx: Some(tx), status: None, pid: None, kill });
        rx
    }

    fn create(self: &Arc<Self>, user: &str, conn: u64, layout: String, seed: Option<u64>, start: Option<String>) {
        if is_robot(user) {
            return self.reply(user, conn, reserved_name());
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
            c.outbox.push(f);
            return;
        }
        let id = loop {
            let id = new_game_id();
            if !st.games.contains_key(&id) && !self.save_path(&id).exists() {
                break id;
            }
        };
        Self::leave(&mut st, user);
        let rx = Self::insert_starting(&mut st, &id, layout.clone());
        Self::enter(&mut st, user, &id);
        drop(st);
        let seed = seed.unwrap_or_else(rand::random);
        self.spawn_game(id, rx, Start::Create { world, layout, seed, start });
    }

    fn join(self: &Arc<Self>, user: &str, conn: u64, game: String) {
        if is_robot(user) {
            return self.reply(user, conn, reserved_name());
        }
        if !valid_game_id(&game) {
            return self.reply(user, conn, ServerFrame::error(codes::UNKNOWN_GAME, format!("no game `{game}`")));
        }
        {
            let mut st = self.lock();
            if Self::current(&st, user, conn).is_none() {
                return;
            }
            if matches!(st.games.get(&game).map(|e| &e.phase), Some(Phase::Starting | Phase::Running)) {
                Self::leave(&mut st, user);
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
        if matches!(st.games.get(&game).map(|e| &e.phase), Some(Phase::Starting | Phase::Running)) {
            // Someone resumed it meanwhile.
            Self::leave(&mut st, user);
            Self::enter(&mut st, user, &game);
            return;
        }
        if let Err(f) = Self::room_for_one_more(&st) {
            c.outbox.push(f);
            return;
        }
        Self::leave(&mut st, user);
        let rx = Self::insert_starting(&mut st, &game, layout);
        Self::enter(&mut st, user, &game);
        drop(st);
        self.spawn_game(game, rx, Start::Resume);
    }

    // ---- the lobby ----

    /// Every game: live ones from memory, the rest from their save files.
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
                },
                Err(e) => GameInfo {
                    id: id.clone(),
                    layout: "?".into(),
                    state: GameState::Crashed,
                    sim_time: 0.0,
                    areas: vec![],
                    players: vec![],
                    error: Some(e.clone()),
                },
            };
            out.insert(id.clone(), info);
        }
        for (id, e) in &st.games {
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
            });
            info.layout = e.layout.clone();
            match &e.phase {
                Phase::Starting | Phase::Running => {
                    info.state = GameState::Running;
                    info.error = None;
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
        cmd.arg("--save").arg(self.save_path(&id)).arg("--socket").arg(&socket);
        cmd.arg("--empty-exit-s").arg(self.cfg.empty_exit_s.to_string());
        if let Start::Create { world, layout, seed, start } = &start {
            cmd.arg("--create").arg("--layout").arg(world).arg("--layout-name").arg(layout).arg("--seed").arg(seed.to_string());
            if let Some(s) = start {
                cmd.arg("--start").arg(s);
            }
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
        if trouble.is_none() && status.is_some_and(|s| s.success()) {
            self.finished(&id);
            return;
        }
        let line = last_line.lock().expect("stderr lock").clone();
        let why = match (line.is_empty(), trouble) {
            (false, _) => line,
            (true, Some(t)) => t,
            (true, None) => match status {
                Some(s) => format!("the game process ended: {s}"),
                None => "the game process ended".into(),
            },
        };
        self.crashed(&id, why);
    }

    /// Frames from a game until it closes its socket; `Some(why)` if it
    /// sent something unreadable.
    async fn read_game(&self, id: &str, rd: &mut OwnedReadHalf) -> Option<String> {
        loop {
            match read_frame::<_, FromGame>(rd).await {
                Ok(Some(m)) => self.from_game(id, m),
                Ok(None) => return None,
                Err(e) => return Some(format!("bad frame from the game: {e}")),
            }
        }
    }

    fn from_game(&self, id: &str, m: FromGame) {
        match m {
            FromGame::ToPlayer { player, msg } => {
                let st = self.lock();
                let Some(c) = st.clients.get(&player).filter(|c| c.game.as_deref() == Some(id)) else { return };
                if c.outbox.push(ServerFrame::Game(msg)) == Pushed::Overflowed {
                    send_to(&st, id, ToGame::Client { player, msg: ClientMsg::Resync });
                }
            }
            FromGame::Status(s) => {
                if let Some(e) = self.lock().games.get_mut(id) {
                    e.status = Some(s);
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
        for c in st.clients.values_mut() {
            if c.game.as_deref() == Some(id) {
                c.game = None;
                c.outbox.push(f.clone());
            }
        }
    }

    /// The game exited cleanly: it is saved and leaves the live table.
    fn finished(&self, id: &str) {
        let mut st = self.lock();
        st.games.remove(id);
        let f = ServerFrame::error(codes::GAME_STOPPED, "the game stopped; join it again to resume it");
        Self::evict(&mut st, id, &f);
    }

    fn crashed(&self, id: &str, why: String) {
        eprintln!("[{id}] crashed: {why}");
        let mut st = self.lock();
        let layout = st.games.get(id).map_or_else(|| "?".to_string(), |e| e.layout.clone());
        st.games.insert(
            id.to_string(),
            Entry { layout, phase: Phase::Crashed(why), tx: None, status: None, pid: None, kill: Arc::new(Notify::new()) },
        );
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
