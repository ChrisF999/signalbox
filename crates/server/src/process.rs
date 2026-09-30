//! The game process (spec §2.2, §6, §9): one `game::Game` behind one Unix
//! socket. `Shell` is the sync logic (what to send for each input, when to
//! exit) and is tested without sockets; `serve` is the tokio loop around it:
//! advance every 0.1 s of real time, flush every 0.2 s, `Status` every 1 s.

use std::path::{Path, PathBuf};
use std::time::Duration;

use game::{Game, GameMeta, GameStatus, Out};
use ipc::{Counters, FromGame, LogLevel, PlayerStatus, StatusMsg, ToGame, read_frame, write_frame};
use signalbox_core::time::{fmt_hms, parse_hms};
use tokio::net::UnixListener;
use tokio::net::unix::OwnedWriteHalf;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior, interval, timeout};

pub const USAGE: &str = "usage: signalbox-game --save <db> --socket <path> [--empty-exit-s <secs>] \
[--create --layout <world.json> --layout-name <name> --seed <u64> [--start HH:MM:SS] [--creator <user>]]";
/// Real seconds a game with nobody connected waits before it saves and exits.
pub const EMPTY_EXIT_S: u64 = 600;
pub const ADVANCE_EVERY: Duration = Duration::from_millis(100);
/// Flush on every second advance: 5 times a second.
pub const FLUSH_EVERY_ADVANCES: u64 = 2;
pub const STATUS_EVERY: Duration = Duration::from_secs(1);
/// How long the game waits for the front to connect.
pub const ACCEPT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateArgs {
    /// A converted world file.
    pub world: PathBuf,
    pub layout_name: String,
    pub seed: u64,
    /// Normalised "HH:MM:SS".
    pub start: Option<String>,
    /// Recorded in the save as its creator (owner decision 13).
    pub creator: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub save: PathBuf,
    pub socket: PathBuf,
    pub empty_exit: Duration,
    /// Create the save first; without it, resume the save.
    pub create: Option<CreateArgs>,
}

impl Args {
    /// Parse the arguments after the program name.
    pub fn parse(args: &[String]) -> Result<Args, String> {
        let (mut save, mut socket, mut empty_exit_s, mut create) = (None, None, EMPTY_EXIT_S, false);
        let (mut world, mut layout_name, mut seed, mut start, mut creator) = (None, None, None, None, None);
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let mut value = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
            match a.as_str() {
                "--save" => save = Some(PathBuf::from(value()?)),
                "--socket" => socket = Some(PathBuf::from(value()?)),
                "--empty-exit-s" => {
                    empty_exit_s = value()?.parse().map_err(|_| "--empty-exit-s takes whole seconds".to_string())?
                }
                "--create" => create = true,
                "--layout" => world = Some(PathBuf::from(value()?)),
                "--layout-name" => layout_name = Some(value()?),
                "--seed" => seed = Some(value()?.parse::<u64>().map_err(|_| "--seed takes a u64".to_string())?),
                "--start" => {
                    let v = value()?;
                    start = Some(normalise_start(&v).ok_or_else(|| format!("bad --start `{v}`"))?);
                }
                "--creator" => creator = Some(value()?),
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        let save = save.ok_or("--save is required")?;
        let socket = socket.ok_or("--socket is required")?;
        let create = if create {
            Some(CreateArgs {
                world: world.ok_or("--create needs --layout")?,
                layout_name: layout_name.ok_or("--create needs --layout-name")?,
                seed: seed.ok_or("--create needs --seed")?,
                start,
                creator,
            })
        } else {
            if world.is_some() || layout_name.is_some() || seed.is_some() || start.is_some() || creator.is_some() {
                return Err("--layout, --layout-name, --seed, --start and --creator need --create".into());
            }
            None
        };
        Ok(Args { save, socket, empty_exit: Duration::from_secs(empty_exit_s), create })
    }
}

/// A game start time as "HH:MM:SS": what `parse_hms` accepts, before 24:00.
pub fn normalise_start(s: &str) -> Option<String> {
    let t = parse_hms(s)?;
    (t < 24 * 3600).then(|| fmt_hms(f64::from(t)))
}

/// The world JSON with `options.start_time` set to `start` (C1 amendment 11).
pub fn set_start_time(world_json: &str, start: &str) -> Result<String, String> {
    let mut v: serde_json::Value = serde_json::from_str(world_json).map_err(|e| format!("world: {e}"))?;
    let root = v.as_object_mut().ok_or("world: not a JSON object")?;
    let options = root.entry("options").or_insert_with(|| serde_json::json!({}));
    let options = options.as_object_mut().ok_or("world: `options` is not an object")?;
    options.insert("start_time".into(), serde_json::Value::String(start.to_string()));
    Ok(serde_json::to_string(&v).expect("JSON values serialise"))
}

/// Create or resume the game the arguments name.
pub fn open_game(args: &Args) -> Result<Game, String> {
    match &args.create {
        Some(c) => {
            let json = std::fs::read_to_string(&c.world).map_err(|e| format!("{}: {e}", c.world.display()))?;
            let json = match &c.start {
                Some(start) => set_start_time(&json, start)?,
                None => json,
            };
            let meta = GameMeta { layout: c.layout_name.clone(), seed: c.seed };
            let mut g = Game::create(&args.save, &json, meta).map_err(|e| format!("create: {e}"))?;
            if let Some(user) = &c.creator {
                g.set_creator(user).map_err(|e| format!("create: {e}"))?;
            }
            Ok(g)
        }
        None => Game::resume(&args.save).map_err(|e| e.to_string()),
    }
}

pub fn status_msg(st: &GameStatus) -> StatusMsg {
    let n = |x: usize| x as u64;
    StatusMsg {
        sim_time: st.sim_time,
        tick: st.tick,
        paused: st.paused,
        speed: st.speed,
        holders: st.holders.clone(),
        players: st.players.iter().map(|(name, connected)| PlayerStatus { name: name.clone(), connected: *connected }).collect(),
        counters: Counters {
            spads: n(st.stats.spads),
            collisions: n(st.stats.collisions),
            invariant_violations: n(st.stats.invariant_violations),
            player_commands: n(st.stats.player_commands),
            robot_commands: n(st.stats.robot_commands),
            save_busy_ms: st.save_busy.as_millis() as u64,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    Continue,
    Exit,
}

/// The game process's decisions, without sockets or clocks.
pub struct Shell {
    game: Game,
    empty_exit_s: f64,
    /// Real seconds with nobody connected.
    empty_s: f64,
    /// The newest snapshot tick already reported with `Saved`.
    reported: Option<u64>,
}

impl Shell {
    pub fn new(game: Game, empty_exit: Duration) -> Shell {
        let reported = game.last_snapshot_tick();
        Shell { game, empty_exit_s: empty_exit.as_secs_f64(), empty_s: 0.0, reported }
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    pub fn on_frame(&mut self, msg: ToGame) -> (Vec<FromGame>, Next) {
        let outs = match msg {
            ToGame::Connect { player } => {
                // Somebody is here, however briefly: the empty clock restarts.
                self.empty_s = 0.0;
                self.game.connect(&player)
            }
            ToGame::Client { player, msg } => self.game.handle(&player, msg),
            ToGame::Disconnect { player } => {
                let before = self.game.status().connected;
                self.game.disconnect(&player);
                if before > 0 && self.game.status().connected == 0 {
                    self.game.pause_for_empty();
                    self.game.save_now()
                } else {
                    vec![]
                }
            }
            ToGame::Shutdown => return (self.shutdown(), Next::Exit),
        };
        (self.wrap(outs), Next::Continue)
    }

    /// Run the game for `dt` real seconds; exits after `empty_exit` with
    /// nobody connected.
    pub fn on_advance(&mut self, dt: f64) -> (Vec<FromGame>, Next) {
        let outs = self.game.advance(dt);
        let mut out = self.wrap(outs);
        if self.game.status().connected == 0 {
            self.empty_s += if dt.is_finite() && dt > 0.0 { dt } else { 0.0 };
            if self.empty_s >= self.empty_exit_s {
                out.extend(self.shutdown());
                return (out, Next::Exit);
            }
        } else {
            self.empty_s = 0.0;
        }
        (out, Next::Continue)
    }

    pub fn on_flush(&mut self) -> Vec<FromGame> {
        let outs = self.game.flush();
        self.wrap(outs)
    }

    pub fn status(&self) -> FromGame {
        FromGame::Status(status_msg(&self.game.status()))
    }

    /// Save now (Shutdown, SIGTERM, the front going away, or empty).
    pub fn shutdown(&mut self) -> Vec<FromGame> {
        let outs = self.game.save_now();
        self.wrap(outs)
    }

    fn wrap(&mut self, outs: Vec<Out>) -> Vec<FromGame> {
        let mut v: Vec<FromGame> = outs.into_iter().map(|(player, msg)| FromGame::ToPlayer { player, msg }).collect();
        for message in self.game.take_save_errors() {
            v.push(FromGame::Log { level: LogLevel::Error, message: format!("save failed: {message}") });
        }
        let tick = self.game.last_snapshot_tick();
        if tick != self.reported {
            self.reported = tick;
            if let Some(tick) = tick {
                v.push(FromGame::Saved { tick });
            }
        }
        v
    }
}

fn log(m: &FromGame) {
    if let FromGame::Log { level, message } = m {
        eprintln!("signalbox-game: {level:?}: {message}");
    }
}

/// Send frames to the front; `Log` frames also go to stderr.
async fn send_all(w: &mut OwnedWriteHalf, out: Vec<FromGame>) -> Result<(), ipc::IpcError> {
    for m in out {
        log(&m);
        write_frame(w, &m).await?;
    }
    Ok(())
}

/// Open the game, listen on the socket, serve one front until told to stop.
pub async fn run(args: Args) -> Result<(), String> {
    let game = open_game(&args)?;
    let _ = std::fs::remove_file(&args.socket);
    let listener = UnixListener::bind(&args.socket).map_err(|e| format!("{}: {e}", args.socket.display()))?;
    serve(Shell::new(game, args.empty_exit), listener, &args.socket).await
}

/// Accept exactly one front connection, then run the game for it.
pub async fn serve(mut shell: Shell, listener: UnixListener, socket: &Path) -> Result<(), String> {
    let accepted = timeout(ACCEPT_TIMEOUT, listener.accept()).await;
    drop(listener);
    let _ = std::fs::remove_file(socket);
    let stream = match accepted {
        Ok(Ok((stream, _))) => stream,
        Ok(Err(e)) => return Err(format!("accept: {e}")),
        Err(_) => {
            shell.shutdown().iter().for_each(log);
            return Err("no front connected within 60 s".into());
        }
    };
    let (mut rd, mut wr) = stream.into_split();
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            match read_frame::<_, ToGame>(&mut rd).await {
                Ok(Some(m)) => {
                    if tx.send(Ok(m)).is_err() {
                        return;
                    }
                }
                Ok(None) => return,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            }
        }
    });
    let mut term = signal(SignalKind::terminate()).map_err(|e| format!("SIGTERM handler: {e}"))?;
    let mut ticker = interval(ADVANCE_EVERY);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last = Instant::now();
    let mut last_status = last;
    let mut advances = 0u64;
    loop {
        let (out, next) = tokio::select! {
            _ = ticker.tick() => {
                let now = Instant::now();
                let (mut out, next) = shell.on_advance((now - last).as_secs_f64());
                last = now;
                advances += 1;
                if advances % FLUSH_EVERY_ADVANCES == 0 {
                    out.extend(shell.on_flush());
                }
                if now - last_status >= STATUS_EVERY {
                    out.push(shell.status());
                    last_status = now;
                }
                (out, next)
            }
            m = rx.recv() => match m {
                Some(Ok(msg)) => shell.on_frame(msg),
                Some(Err(e)) => {
                    eprintln!("signalbox-game: bad frame from the front: {e}");
                    (shell.shutdown(), Next::Exit)
                }
                None => (shell.shutdown(), Next::Exit),
            },
            _ = term.recv() => (shell.shutdown(), Next::Exit),
        };
        if let Err(e) = send_all(&mut wr, out).await {
            eprintln!("signalbox-game: the front went away ({e}); saving and exiting");
            shell.shutdown().iter().for_each(log);
            return Ok(());
        }
        if next == Next::Exit {
            return Ok(());
        }
    }
}
