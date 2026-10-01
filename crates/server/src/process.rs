//! The game process (spec §2.2, §6, §9): one `game::Game` behind one Unix
//! socket. `Shell` is the sync logic (what to send for each input, when to
//! exit) and is tested without sockets; `serve` is the tokio loop around it:
//! advance every 0.1 s of real time, flush every 0.2 s, `Status` every 1 s.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use game::lesson::{self, Runner, load_lesson};
use game::seed::{self, Progress, Seeding};
use game::{Game, GameMeta, GameStatus, Out};
use ipc::{Counters, FromGame, LogLevel, PlayerStatus, StatusMsg, ToGame, read_frame, write_frame};
use protocol::{Preparing, codes};
use signalbox_core::sim::TICK_S;
use signalbox_core::time::{fmt_hms, parse_hms};
use tokio::net::UnixListener;
use tokio::net::unix::OwnedWriteHalf;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior, interval, timeout};

pub const USAGE: &str = "usage: signalbox-game --save <db> --socket <path> [--empty-exit-s <secs>] \
[--create --layout <world.json> --layout-name <name> --seed <u64> [--start HH:MM:SS] [--creator <user>] \
[--seed-budget-ms <ms>]]\n\
       signalbox-game --lesson <dir> --socket <path> [--empty-exit-s <secs>]";
/// Real seconds a game with nobody connected waits before it saves and exits.
pub const EMPTY_EXIT_S: u64 = 600;
pub const ADVANCE_EVERY: Duration = Duration::from_millis(100);
/// Flush on every second advance: 5 times a second.
pub const FLUSH_EVERY_ADVANCES: u64 = 2;
pub const STATUS_EVERY: Duration = Duration::from_secs(1);
/// How long the game waits for the front to connect.
pub const ACCEPT_TIMEOUT: Duration = Duration::from_secs(60);
/// Real time a new game may spend being prepared to a later start
/// (timetables spec P8); after it the creation fails with `seed_too_slow`.
pub const SEED_BUDGET: Duration = Duration::from_secs(60);
/// The exit status of a game process whose preparing took too long.
pub const EXIT_SEED_TOO_SLOW: u8 = 3;
/// The exit status of a game process whose preparing failed any other way
/// (a save error, say); its error starts with `PREPARING_FAILED`.
pub const EXIT_NOT_PREPARED: u8 = 4;
pub const PREPARING_FAILED: &str = "preparing failed";
/// Frames from the front held while a game is prepared, at most; more and
/// the front is misbehaving: the creation fails.
pub const MAX_HELD: usize = 1000;

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
    /// How long preparing a later start may take (`SEED_BUDGET`; tests
    /// pass less with `--seed-budget-ms`).
    pub seed_budget: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// Empty for a lesson, which keeps no save.
    pub save: PathBuf,
    pub socket: PathBuf,
    pub empty_exit: Duration,
    /// Create the save first; without it, resume the save.
    pub create: Option<CreateArgs>,
    /// Run this lesson directory as a tutorial (tutorial spec §3).
    pub lesson: Option<PathBuf>,
}

impl Args {
    /// Parse the arguments after the program name.
    pub fn parse(args: &[String]) -> Result<Args, String> {
        let (mut save, mut socket, mut empty_exit_s, mut create, mut lesson) = (None, None, EMPTY_EXIT_S, false, None);
        let (mut world, mut layout_name, mut seed, mut start, mut creator) = (None, None, None, None, None);
        let mut seed_budget = None;
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
                "--seed-budget-ms" => {
                    let ms = value()?.parse().map_err(|_| "--seed-budget-ms takes whole milliseconds".to_string())?;
                    seed_budget = Some(Duration::from_millis(ms));
                }
                "--lesson" => lesson = Some(PathBuf::from(value()?)),
                other => return Err(format!("unknown argument `{other}`")),
            }
        }
        if let Some(lesson) = lesson {
            let saved = save.is_some() || create || world.is_some() || layout_name.is_some();
            if saved || seed.is_some() || start.is_some() || creator.is_some() || seed_budget.is_some() {
                return Err("--lesson takes only --socket and --empty-exit-s".into());
            }
            let socket = socket.ok_or("--socket is required")?;
            let empty_exit = Duration::from_secs(empty_exit_s);
            return Ok(Args { save: PathBuf::new(), socket, empty_exit, create: None, lesson: Some(lesson) });
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
                seed_budget: seed_budget.unwrap_or(SEED_BUDGET),
            })
        } else {
            if world.is_some() || layout_name.is_some() || seed.is_some() || start.is_some() || creator.is_some() {
                return Err("--layout, --layout-name, --seed, --start and --creator need --create".into());
            }
            if seed_budget.is_some() {
                return Err("--seed-budget-ms needs --create".into());
            }
            None
        };
        Ok(Args { save, socket, empty_exit: Duration::from_secs(empty_exit_s), create, lesson: None })
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

/// The lesson's game and its runner, at the first step.
pub fn open_lesson(dir: &Path) -> Result<(Game, Runner), String> {
    let lesson = load_lesson(dir).map_err(|e| format!("lesson {}: {e}", dir.display()))?;
    Ok(lesson::start(lesson))
}

/// What `open_game` opened.
pub enum Opened {
    Ready(Game),
    /// A new game with a start later than its world's: it is prepared
    /// (timetables spec §3.4) before anyone plays.
    Seeding(Seeding),
}

/// Create or resume the game the arguments name. A start later than the
/// world's is reached by seeding; an earlier one is written into the world
/// as before (C1 amendment 11).
pub fn open_game(args: &Args) -> Result<Opened, String> {
    match &args.create {
        Some(c) => {
            let json = std::fs::read_to_string(&c.world).map_err(|e| format!("{}: {e}", c.world.display()))?;
            let meta = GameMeta { layout: c.layout_name.clone(), seed: c.seed };
            let start = c.start.as_deref().map(|s| parse_hms(s).ok_or_else(|| format!("bad start `{s}`"))).transpose()?;
            if let Some(start) = start.filter(|&t| seed::needs_seeding(&json, t).unwrap_or(false)) {
                let failed = |e: game::GameError| {
                    seed::remove_partial(&args.save);
                    format!("{PREPARING_FAILED}: create: {e}")
                };
                let mut s = Seeding::create(&args.save, &json, meta, start).map_err(failed)?;
                if let Some(user) = &c.creator {
                    if let Err(e) = s.set_creator(user) {
                        s.abandon();
                        return Err(format!("{PREPARING_FAILED}: create: {e}"));
                    }
                }
                return Ok(Opened::Seeding(s));
            }
            let json = match &c.start {
                Some(start) => set_start_time(&json, start)?,
                None => json,
            };
            let mut g = Game::create(&args.save, &json, meta).map_err(|e| format!("create: {e}"))?;
            if let Some(user) = &c.creator {
                g.set_creator(user).map_err(|e| format!("create: {e}"))?;
            }
            Ok(Opened::Ready(g))
        }
        None => Game::resume(&args.save).map(Opened::Ready).map_err(|e| e.to_string()),
    }
}

/// The process's exit status for the error `run` returned: `seed_too_slow`
/// has its own, so the front can tell the lobby why the game is not there.
pub fn exit_status(err: &str) -> u8 {
    if err.starts_with(codes::SEED_TOO_SLOW) {
        EXIT_SEED_TOO_SLOW
    } else if err.starts_with(PREPARING_FAILED) {
        EXIT_NOT_PREPARED
    } else {
        1
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
        preparing: None,
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
    /// A tutorial's lesson (no save; the clock runs only for its player).
    runner: Option<Runner>,
    empty_exit_s: f64,
    /// Real seconds with nobody connected.
    empty_s: f64,
    /// The newest snapshot tick already reported with `Saved`.
    reported: Option<u64>,
}

impl Shell {
    pub fn new(game: Game, empty_exit: Duration) -> Shell {
        let reported = game.last_snapshot_tick();
        Shell { game, runner: None, empty_exit_s: empty_exit.as_secs_f64(), empty_s: 0.0, reported }
    }

    /// A tutorial: `game` run by its lesson's `runner`.
    pub fn lesson(game: Game, runner: Runner, empty_exit: Duration) -> Shell {
        Shell { runner: Some(runner), ..Shell::new(game, empty_exit) }
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    pub fn runner(&self) -> Option<&Runner> {
        self.runner.as_ref()
    }

    pub fn on_frame(&mut self, msg: ToGame) -> (Vec<FromGame>, Next) {
        let outs = match msg {
            ToGame::Connect { player } => {
                // Somebody is here, however briefly: the empty clock restarts.
                self.empty_s = 0.0;
                match self.runner.as_mut() {
                    Some(r) => r.connect(&mut self.game, &player),
                    None => self.game.connect(&player),
                }
            }
            ToGame::Client { player, msg } => match self.runner.as_mut() {
                Some(r) => r.handle(&mut self.game, &player, msg),
                None => self.game.handle(&player, msg),
            },
            ToGame::Disconnect { player } => {
                let before = self.game.status().connected;
                self.game.disconnect(&player);
                // A lesson's clock stops by itself without its player.
                if self.runner.is_none() && before > 0 && self.game.status().connected == 0 {
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
        let outs = match self.runner.as_mut() {
            Some(r) => r.advance(&mut self.game, dt),
            None => self.game.advance(dt),
        };
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

/// Hand the heap's free pages back to the system once the game is open.
/// glibc already returns the large parse buffers (the world JSON is
/// mmapped, the heap top is trimmed); this gets the free holes left in the
/// heap, measured at about 0.2 MB for a Liverpool Street game. Elsewhere
/// (musl, other systems) nothing.
pub fn release_free_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> std::ffi::c_int;
        }
        // SAFETY: malloc_trim only returns unused memory to the system; it
        // is thread-safe and leaves every live allocation where it is.
        unsafe {
            malloc_trim(0);
        }
    }
}

/// What `serve` starts from.
pub enum Begin {
    Ready(Shell),
    /// Prepare the game first (`prepare`), then serve it.
    Seed { seeding: Seeding, empty_exit: Duration, budget: Duration },
}

/// Open the game, listen on the socket, serve one front until told to stop.
pub async fn run(args: Args) -> Result<(), String> {
    let begin = match &args.lesson {
        Some(dir) => {
            let (game, runner) = open_lesson(dir)?;
            Begin::Ready(Shell::lesson(game, runner, args.empty_exit))
        }
        None => match open_game(&args)? {
            Opened::Ready(game) => Begin::Ready(Shell::new(game, args.empty_exit)),
            Opened::Seeding(seeding) => {
                let budget = args.create.as_ref().map_or(SEED_BUDGET, |c| c.seed_budget);
                Begin::Seed { seeding, empty_exit: args.empty_exit, budget }
            }
        },
    };
    release_free_memory();
    let _ = std::fs::remove_file(&args.socket);
    let listener = UnixListener::bind(&args.socket).map_err(|e| format!("{}: {e}", args.socket.display()))?;
    serve(begin, listener, &args.socket).await
}

/// A game prepared to its start, with the front's frames that came meanwhile.
type Prepared = (Game, Vec<ToGame>);

/// Run `seeding` to its start time on a blocking thread, telling the front
/// how far it has got (a `Status` with `preparing`, at once and then every
/// second); the front's frames wait until the game is ready. The budget is
/// real time, measured here: past it the half-built save is removed and the
/// error starts with `seed_too_slow`; any other failure (a save error, a
/// front sending more than `MAX_HELD` frames) starts with
/// `PREPARING_FAILED`. `Shutdown`, SIGTERM or the front going away stop it
/// the same way, as `Ok(None)`: there is nothing to save. One that comes
/// as the seeding ends, too late to stop it, is held for the ready game.
async fn prepare(
    seeding: Seeding,
    budget: Duration,
    wr: &mut OwnedWriteHalf,
    rx: &mut mpsc::UnboundedReceiver<Result<ToGame, ipc::IpcError>>,
    term: &mut tokio::signal::unix::Signal,
) -> Result<Option<Prepared>, String> {
    let (from_s, to_s) = (seeding.from_s(), seeding.to_s());
    let mut base = status_msg(&seeding.game().status());
    base.preparing = Some(Preparing { from: from_s, to: to_s });
    let stop = Arc::new(AtomicBool::new(false));
    let reached = Arc::new(AtomicU64::new(seeding.game().sim().tick()));
    let started = std::time::Instant::now();
    let mut job = tokio::task::spawn_blocking({
        let (stop, reached) = (stop.clone(), reached.clone());
        move || {
            let mut seeding = seeding;
            let r = seeding.run(|g| {
                reached.store(g.sim().tick(), Ordering::Relaxed);
                !stop.load(Ordering::Relaxed) && started.elapsed() < budget
            });
            (seeding, r)
        }
    });
    let mut held = Vec::new();
    let mut stopping = false;
    // Shutdown or SIGTERM (not the front going away, which `serve` sees again).
    let mut told_to_stop = false;
    let mut flooded = false;
    let mut every = interval(STATUS_EVERY);
    let joined = loop {
        tokio::select! {
            r = &mut job => break r,
            _ = every.tick(), if !stopping => {
                let tick = reached.load(Ordering::Relaxed);
                let st = StatusMsg { tick, sim_time: from_s + tick as f64 * TICK_S, ..base.clone() };
                if send_all(wr, vec![FromGame::Status(st)]).await.is_err() {
                    stopping = true;
                }
            }
            m = rx.recv(), if !stopping => match m {
                Some(Ok(ToGame::Shutdown)) => (stopping, told_to_stop) = (true, true),
                Some(Err(_)) | None => stopping = true,
                Some(Ok(_)) if held.len() >= MAX_HELD => (stopping, flooded) = (true, true),
                Some(Ok(m)) => held.push(m),
            },
            _ = term.recv() => (stopping, told_to_stop) = (true, true),
        }
        if stopping {
            stop.store(true, Ordering::Relaxed);
        }
    };
    // A panic leaves the half-built save for the front to remove.
    let (seeding, r) = joined.map_err(|e| format!("{PREPARING_FAILED}: {e}"))?;
    let elapsed = started.elapsed().as_secs_f64();
    match r {
        Ok(_) if flooded => {
            seeding.abandon();
            Err(format!("{PREPARING_FAILED}: the front sent more than {MAX_HELD} frames while the game was prepared"))
        }
        Ok(Progress::Reached) => {
            eprintln!("signalbox-game: prepared {} to {} in {elapsed:.1} s", fmt_hms(from_s), fmt_hms(to_s));
            let game = seeding.finish().map_err(|e| format!("{PREPARING_FAILED}: {e}"))?;
            if told_to_stop {
                // It finished before it could stop: save and exit once ready.
                held.push(ToGame::Shutdown);
            }
            Ok(Some((game, held)))
        }
        Ok(Progress::Stopped) if stopping => {
            seeding.abandon();
            Ok(None)
        }
        Ok(Progress::Stopped) => {
            let got = fmt_hms(seeding.game().sim().now_s());
            seeding.abandon();
            Err(format!(
                "{}: preparing {} to {} took longer than {} s (it reached {got})",
                codes::SEED_TOO_SLOW,
                fmt_hms(from_s),
                fmt_hms(to_s),
                budget.as_secs_f64()
            ))
        }
        Err(e) => {
            seeding.abandon();
            Err(format!("{PREPARING_FAILED}: {e}"))
        }
    }
}

/// Accept exactly one front connection, then run the game for it (after
/// preparing it, for `Begin::Seed`).
pub async fn serve(begin: Begin, listener: UnixListener, socket: &Path) -> Result<(), String> {
    let accepted = timeout(ACCEPT_TIMEOUT, listener.accept()).await;
    drop(listener);
    let _ = std::fs::remove_file(socket);
    let stream = match accepted {
        Ok(Ok((stream, _))) => stream,
        Ok(Err(e)) => return Err(format!("accept: {e}")),
        Err(_) => {
            match begin {
                Begin::Ready(mut shell) => shell.shutdown().iter().for_each(log),
                Begin::Seed { seeding, .. } => seeding.abandon(),
            }
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
    let mut shell = match begin {
        Begin::Ready(shell) => shell,
        Begin::Seed { seeding, empty_exit, budget } => {
            let Some((game, held)) = prepare(seeding, budget, &mut wr, &mut rx, &mut term).await? else {
                return Ok(());
            };
            let mut shell = Shell::new(game, empty_exit);
            // Ready: a status without `preparing`, then what the front sent meanwhile.
            let mut out = vec![shell.status()];
            let mut next = Next::Continue;
            for m in held {
                let (o, n) = shell.on_frame(m);
                out.extend(o);
                if n == Next::Exit {
                    next = n;
                    break;
                }
            }
            if send_all(&mut wr, out).await.is_err() || next == Next::Exit {
                shell.shutdown().iter().for_each(log);
                return Ok(());
            }
            shell
        }
    };
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
