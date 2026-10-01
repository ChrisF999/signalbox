//! Seeding (timetables spec §3.4, decisions P7 and P8): a game created with
//! a start later than its world's is run from the world's own start to the
//! chosen time with the robot signalling every area, exactly as an
//! uninterrupted robot-run game would be. Its commands are logged with the
//! sender `seed`, so the save replays bit-exactly from tick 0; a snapshot is
//! written at the chosen time, the meta row `seed_to` records it, and only
//! then does anyone play. The world keeps its own start.
//!
//! The save is built at `<save>.seeding` and renamed into place once ready:
//! the lobby lists only `*.sqlite` files, so a game that never finished
//! preparing (a crash, the budget, a shutdown) is never listed half-made.
//!
//! Nothing here reads the clock: the caller's `keep_going` decides when to
//! give up (the process layer measures P8's 60 s budget).

use std::path::{Path, PathBuf};

use serde::Deserialize;
use signalbox_core::sim::TICK_S;
use signalbox_core::time::{fmt_hms, parse_hms};

use crate::game::{Game, GameError, GameMeta};
use crate::save::{SaveDb, SaveError};

/// The command log's sender for the robot's runs while seeding.
pub const SEED: &str = "seed";

/// How a `fast_forward` ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// The game is at the start time.
    Reached,
    /// `keep_going` said no first.
    Stopped,
}

/// Where a save is built while its game is seeded.
pub fn temp_path(save: &Path) -> PathBuf {
    let mut name = save.as_os_str().to_owned();
    name.push(".seeding");
    PathBuf::from(name)
}

/// Remove what a seeding that never finished left of the save at `save`
/// (the half-built file and SQLite's files beside it); nothing if none.
pub fn remove_partial(save: &Path) {
    let temp = temp_path(save);
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut name = temp.as_os_str().to_owned();
        name.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(name));
    }
}

#[derive(Deserialize)]
struct Head {
    #[serde(default)]
    options: HeadOptions,
}

#[derive(Default, Deserialize)]
struct HeadOptions {
    #[serde(default)]
    start_time: Option<String>,
}

/// The world's own start, seconds since midnight, read from its JSON.
pub fn world_start(world_json: &str) -> Result<u32, String> {
    let head: Head = serde_json::from_str(world_json).map_err(|e| format!("world: {e}"))?;
    let text = head.options.start_time.ok_or("world: no options.start_time")?;
    parse_hms(&text).ok_or_else(|| format!("world: bad options.start_time `{text}`"))
}

/// Whether a game of this world starting at `start` (seconds since
/// midnight) must be seeded: only a start later than the world's.
pub fn needs_seeding(world_json: &str, start: u32) -> Result<bool, String> {
    Ok(start > world_start(world_json)?)
}

/// Run `game` until its sim reaches `until_tick`, the robot signalling every
/// area every `ROBOT_EVERY_TICKS` and its runs logged (one batch per run)
/// with the sender `SEED`. `keep_going` is asked before every tick. A save
/// failure stops it with an error: a seeded save must replay whole.
pub fn fast_forward(game: &mut Game, until_tick: u64, mut keep_going: impl FnMut(&Game) -> bool) -> Result<Progress, GameError> {
    while game.sim().tick() < until_tick {
        if !keep_going(game) {
            return Ok(Progress::Stopped);
        }
        game.seed_tick();
        if let Some(e) = game.take_save_errors().into_iter().next() {
            return Err(GameError::Save(SaveError::Bad(format!("while preparing: {e}"))));
        }
    }
    Ok(Progress::Reached)
}

/// A game being prepared: its save is at `temp_path(path)` until `finish`.
pub struct Seeding {
    game: Game,
    path: PathBuf,
    from_s: f64,
    to_s: f64,
    until_tick: u64,
}

impl Seeding {
    /// Start preparing a game of `world_json` to be saved at `path` (which
    /// must not exist), starting at `start` seconds since midnight, later
    /// than the world's own start. A half-built save left at the temporary
    /// path by an earlier attempt for the same `path` is replaced.
    pub fn create(path: &Path, world_json: &str, meta: GameMeta, start: u32) -> Result<Seeding, GameError> {
        if path.exists() {
            return Err(GameError::Save(SaveError::Bad(format!("{} already exists", path.display()))));
        }
        let from = world_start(world_json).map_err(|e| GameError::Save(SaveError::Bad(e)))?;
        if start <= from {
            let why = format!("{} is not later than the world's start {}", fmt_hms(start.into()), fmt_hms(from.into()));
            return Err(GameError::Save(SaveError::Bad(why)));
        }
        remove_partial(path);
        let game = Game::create(&temp_path(path), world_json, meta)?;
        let from_s = game.sim().world().options.start_s;
        let to_s = f64::from(start);
        let until_tick = ((to_s - from_s) / TICK_S).round() as u64;
        Ok(Seeding { game, path: path.to_path_buf(), from_s, to_s, until_tick })
    }

    /// The world's start, seconds since midnight.
    pub fn from_s(&self) -> f64 {
        self.from_s
    }

    /// The chosen start, seconds since midnight.
    pub fn to_s(&self) -> f64 {
        self.to_s
    }

    /// The tick the game opens at.
    pub fn until_tick(&self) -> u64 {
        self.until_tick
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    /// Record `user` as the game's creator (see `Game::set_creator`).
    pub fn set_creator(&mut self, user: &str) -> Result<(), GameError> {
        self.game.set_creator(user)
    }

    /// `fast_forward` to the start time.
    pub fn run(&mut self, keep_going: impl FnMut(&Game) -> bool) -> Result<Progress, GameError> {
        fast_forward(&mut self.game, self.until_tick, keep_going)
    }

    /// The game at its start time: a snapshot there, the `seed_to` row,
    /// and the save moved to its real path. Fails if `run` has not
    /// `Reached` the start (the half-built save is then removed).
    pub fn finish(self) -> Result<Game, GameError> {
        let path = self.path.clone();
        let r = self.finish_inner();
        if r.is_err() {
            remove_partial(&path);
        }
        r
    }

    fn finish_inner(mut self) -> Result<Game, GameError> {
        let bad = |m: String| GameError::Save(SaveError::Bad(m));
        if self.game.sim().tick() != self.until_tick {
            return Err(bad(format!("prepared to tick {} of {}", self.game.sim().tick(), self.until_tick)));
        }
        self.game.save_now();
        if let Some(e) = self.game.take_save_errors().into_iter().next() {
            return Err(bad(format!("the snapshot at the start: {e}")));
        }
        let db = self.game.take_save().ok_or_else(|| bad("no save".into()))?;
        db.set_seed_to(&fmt_hms(self.to_s))?;
        db.close()?;
        if self.path.exists() {
            return Err(bad(format!("{} already exists", self.path.display())));
        }
        let temp = temp_path(&self.path);
        std::fs::rename(&temp, &self.path).map_err(|e| bad(format!("{}: {e}", temp.display())))?;
        remove_partial(&self.path);
        self.game.put_save(SaveDb::open(&self.path)?);
        Ok(self.game)
    }

    /// Give up: the half-built save is removed.
    pub fn abandon(self) {
        let path = self.path.clone();
        drop(self);
        remove_partial(&path);
    }
}
