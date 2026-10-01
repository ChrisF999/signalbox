//! The game: players, claims, area checks, the robot for unclaimed areas,
//! the clock, notices and per-player views (spec §6.1). Its only I/O is
//! the optional save (`crate::save`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use protocol::{ClientMsg, Layout, Notice, PlayerCommand, Proposal, Rejection, ServerMsg, View, VoteOutcome, codes};
use signalbox_core::events::{Command, Event};
use signalbox_core::ids::AreaId;
use signalbox_core::robot;
use signalbox_core::sim::{Sim, SimState};
use signalbox_core::world::file::WorldFile;
use signalbox_core::world::{LoadError, World};

use crate::areas::{AreaMap, Visibility};
use crate::clock::{GameClock, VoteError};
use crate::display::Display;
use crate::geometry::WorldGeometry;
use crate::layout::build_layout;
use crate::names::{resolve, to_player_command, valid_headcode};
use crate::notices::area_notices;
use crate::save::{Logged, SaveDb, SaveError, refresh_display, resume_sim};
use crate::view::{Shared, build_view};

/// The robot's player name, reserved: it holds every unclaimed area.
pub const ROBOT: &str = "robot";
/// Real seconds a disconnected holder keeps their area.
pub const GRACE_S: f64 = 120.0;
/// A stalled caller never makes one `advance` run away.
pub const MAX_TICKS_PER_ADVANCE: u64 = 800;
/// Real seconds of running between autosave snapshots.
pub const SNAPSHOT_EVERY_S: f64 = 60.0;

#[derive(Debug, thiserror::Error)]
pub enum GameError {
    #[error("world: {0}")]
    World(#[from] LoadError),
    #[error("save: {0}")]
    Save(#[from] SaveError),
    #[error("resume: {0}")]
    Resume(String),
}

/// A message for one player.
pub type Out = (String, ServerMsg);

/// Where a resumed game's display data came from (polish spec §5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refresh {
    /// No current layout was given: the save's own.
    NotAsked,
    /// The current layout file's (same network as the save).
    Refreshed,
    /// The save's own, and why the current layout's could not be used.
    Kept(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameMeta {
    /// Layout name, for the lobby.
    pub layout: String,
    pub seed: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameStats {
    pub spads: usize,
    pub collisions: usize,
    pub invariant_violations: usize,
    /// Commands from players that reached the sim.
    pub player_commands: usize,
    /// Commands from the robot that reached the sim.
    pub robot_commands: usize,
    /// Commands the sim refused.
    pub sim_rejections: usize,
}

/// What the process shell and the lobby need to know about a running game.
#[derive(Clone, Debug, PartialEq)]
pub struct GameStatus {
    /// Seconds since midnight.
    pub sim_time: f64,
    pub tick: u64,
    pub paused: bool,
    pub speed: u8,
    /// Every area, by name, with its holder (`None` = the robot).
    pub holders: BTreeMap<String, Option<String>>,
    /// Every known player in name order, and whether they are connected
    /// (a disconnected holder stays listed through the grace period).
    pub players: Vec<(String, bool)>,
    /// How many players are connected.
    pub connected: usize,
    pub stats: GameStats,
    /// Wall time spent writing the save so far.
    pub save_busy: Duration,
}

/// Everything a tutorial's Restart step puts back (tutorial spec §3): the
/// sim, the clock, the commands queued for the next tick, and whether the
/// robot already ran at this tick. Players, holders and stats stay as they are.
#[derive(Clone, Debug)]
pub struct GameSnapshot {
    sim: SimState,
    paused: bool,
    speed: u8,
    queued: Vec<(String, Command)>,
    robot_ran_at: Option<u64>,
}

struct Player {
    area: Option<AreaId>,
    connected: bool,
    /// Real seconds since a holder disconnected.
    gone_s: f64,
    vis: Visibility,
    /// The last view sent (the base of the next delta).
    last: Option<View>,
}

pub struct Game {
    sim: Sim,
    meta: GameMeta,
    map: AreaMap,
    spectator: Visibility,
    /// Visibility of each area's holder, by area.
    by_area: Vec<Visibility>,
    /// The diagram, read once from the world.
    geometry: Option<WorldGeometry>,
    /// Prefixes and simplifiers, built once from the world.
    display: Display,
    /// Holder of each area, by area; `None` = the robot.
    holders: Vec<Option<String>>,
    players: BTreeMap<String, Player>,
    clock: GameClock,
    /// Commands queued for the next tick, with who sent them.
    queued: Vec<(String, Command)>,
    /// The robot already ran at this tick (a resumed log ended with its commands).
    robot_ran_at: Option<u64>,
    stats: GameStats,
    save: Option<SaveDb>,
    /// Real seconds of running since the last snapshot.
    since_snapshot_s: f64,
    /// Tick of the newest snapshot written or loaded.
    last_snapshot: Option<u64>,
    /// Save failures not yet taken by the caller.
    save_errors: Vec<String>,
    /// A command was submitted that the log does not hold: snapshot after
    /// every step until a snapshot succeeds (until then a resume could
    /// replay later logged commands over the gap).
    log_lost: bool,
    /// A retried covering snapshot failed and was reported: later failures
    /// of it are not reported again.
    retry_reported: bool,
    /// Clock proposals that ended since the last `flush`, told to every
    /// player there (polish spec M8).
    vote_ended: Vec<(Proposal, VoteOutcome)>,
}

fn error(player: &str, code: &str, message: &str) -> Out {
    (player.to_string(), ServerMsg::Notice(Notice::Error { code: code.to_string(), message: message.to_string() }))
}

fn notice(player: &str, n: Notice) -> Out {
    (player.to_string(), ServerMsg::Notice(n))
}

impl Game {
    /// A new game, running at 1x.
    pub fn new(world: World, meta: GameMeta) -> Game {
        let sim = Sim::new(world, meta.seed);
        Game::from_sim(sim, meta, false)
    }

    /// A new game saved at `path`, which must not exist: world and meta are
    /// written, then a first snapshot at tick 0. Runs at 1x.
    pub fn create(path: &Path, world_json: &str, meta: GameMeta) -> Result<Game, GameError> {
        // Parsed once: the save's meta comes from the same `WorldFile`.
        let file: WorldFile = serde_json::from_str(world_json).map_err(|e| LoadError::Json(e.to_string()))?;
        let areas: Vec<String> = file.areas.iter().map(|a| a.name.clone()).collect();
        let start = file.options.start_time.clone();
        let world = World::from_file(file)?;
        let db = SaveDb::create_with(path, &meta, world_json, &areas, &start)?;
        let mut g = Game::new(world, meta);
        db.write_snapshot(&g.sim.snapshot())?;
        g.save = Some(db);
        g.last_snapshot = Some(0);
        Ok(g)
    }

    /// Resume the game saved at `path`: paused at 1x, every area unclaimed.
    pub fn resume(path: &Path) -> Result<Game, GameError> {
        Game::resume_with_layout(path, None).map(|(g, _)| g)
    }

    /// `resume`, taking the display data from `current` (the layout file's
    /// world JSON as it is now) when its network matches the save's.
    pub fn resume_with_layout(path: &Path, current: Option<&str>) -> Result<(Game, Refresh), GameError> {
        let db = SaveDb::open(path)?;
        let saved = db.load()?;
        let (world, refresh) = match current.map(|c| refresh_display(&saved.world_json, c)) {
            None => (World::from_json(&saved.world_json)?, Refresh::NotAsked),
            Some(Err(why)) => (World::from_json(&saved.world_json)?, Refresh::Kept(why)),
            Some(Ok(json)) => match World::from_json(&json) {
                Ok(w) => (w, Refresh::Refreshed),
                Err(e) => (World::from_json(&saved.world_json)?, Refresh::Kept(format!("the refreshed world does not load: {e}"))),
            },
        };
        let snapshot_tick = saved.snapshot.tick;
        let (sim, robot_ran) =
            resume_sim(world, saved.snapshot, saved.last_seq, &saved.commands).map_err(GameError::Resume)?;
        let mut g = Game::from_sim(sim, saved.meta, true);
        if robot_ran {
            g.robot_ran_at = Some(g.sim.tick());
        }
        // The sim's queue holds the commands the snapshot held at this tick
        // (logged at or under `last_seq`) followed by those replayed here;
        // name their senders so a sim rejection still goes back to them. If
        // the snapshot's part disagrees with the log (a failed append), that
        // part is left unattributed.
        let queue = g.sim.snapshot().queue;
        let at_end = |c: &&Logged| c.tick == g.sim.tick();
        let held: Vec<&Logged> = saved.commands.iter().filter(at_end).filter(|c| c.seq <= saved.last_seq).collect();
        let replayed: Vec<&Logged> = saved.commands.iter().filter(at_end).filter(|c| c.seq > saved.last_seq).collect();
        let n_held = queue.len().saturating_sub(replayed.len());
        let mut senders: Vec<String> = if held.iter().map(|l| &l.command).eq(queue[..n_held].iter()) {
            held.iter().map(|l| l.player.clone()).collect()
        } else {
            vec![ROBOT.to_string(); n_held]
        };
        senders.extend(replayed.iter().map(|l| l.player.clone()));
        senders.resize(queue.len(), ROBOT.to_string());
        g.queued = senders.into_iter().zip(queue).collect();
        g.save = Some(db);
        g.last_snapshot = Some(snapshot_tick);
        Ok((g, refresh))
    }

    /// Record `user` as the game's creator in its save (owner decision 13);
    /// nothing for a game without a save.
    pub fn set_creator(&mut self, user: &str) -> Result<(), GameError> {
        if let Some(db) = &self.save {
            db.set_creator(user)?;
        }
        Ok(())
    }

    /// Whether `player` is connected now.
    pub fn connected(&self, player: &str) -> bool {
        self.players.get(player).is_some_and(|p| p.connected)
    }

    /// Pause or run the clock without a vote (a tutorial's `pause` and
    /// `run`); any open proposal is dropped.
    pub fn set_paused(&mut self, paused: bool) {
        self.clock.paused = paused;
        self.clock.vote = None;
    }

    /// Set the speed without a vote (a tutorial's `speed`); `false` for a
    /// speed that is not 1, 2, 4 or 8.
    pub fn set_speed(&mut self, x: u8) -> bool {
        if !crate::clock::SPEEDS.contains(&x) {
            return false;
        }
        self.clock.speed = x;
        self.clock.vote = None;
        true
    }

    /// Offer the world's on-demand entry `entry` now (a tutorial's `spawn`).
    pub fn offer_entry(&mut self, entry: usize) -> Result<(), String> {
        self.sim.offer_entry(entry).map(|_| ())
    }

    /// Queue `cmd` for the next tick whoever holds its subject (a tutorial
    /// demonstrating). A sim refusal is counted but told to nobody.
    pub fn demonstrate(&mut self, cmd: &PlayerCommand) -> Result<(), Rejection> {
        let core = resolve(self.sim.world(), cmd).ok_or(Rejection::UnknownId)?;
        self.submit(ROBOT, core);
        Ok(())
    }

    /// What `restore` puts back.
    pub fn snapshot(&self) -> GameSnapshot {
        GameSnapshot {
            sim: self.sim.snapshot(),
            paused: self.clock.paused,
            speed: self.clock.speed,
            queued: self.queued.clone(),
            robot_ran_at: self.robot_ran_at,
        }
    }

    /// Back to `snap` (taken from this game): the next flush sends every
    /// player what changed.
    pub fn restore(&mut self, snap: &GameSnapshot) -> Result<(), GameError> {
        self.sim = Sim::restore(self.sim.world().clone(), snap.sim.clone()).map_err(GameError::Resume)?;
        self.clock.paused = snap.paused;
        self.clock.speed = snap.speed;
        self.clock.vote = None;
        self.queued = snap.queued.clone();
        self.robot_ran_at = snap.robot_ran_at;
        Ok(())
    }

    /// Snapshot now and restart the autosave timer. A failure goes to every
    /// connected player as `save_failed` and is kept for
    /// `take_save_errors`; the game carries on.
    pub fn save_now(&mut self) -> Vec<Out> {
        self.since_snapshot_s = 0.0;
        if self.save.is_none() {
            return vec![];
        }
        match self.write_snapshot() {
            Ok(()) => vec![],
            Err(e) => self.save_failed(&e.to_string()),
        }
    }

    /// Write a snapshot; one that succeeds closes any gap in the log.
    fn write_snapshot(&mut self) -> Result<(), SaveError> {
        let Some(db) = &self.save else { return Ok(()) };
        db.write_snapshot(&self.sim.snapshot())?;
        self.last_snapshot = Some(self.sim.tick());
        self.log_lost = false;
        self.retry_reported = false;
        Ok(())
    }

    /// After a step while the log has a gap: the covering snapshot, retried
    /// every tick until it succeeds; only its first failure is reported.
    fn cover_gap(&mut self) -> Vec<Out> {
        match self.write_snapshot() {
            Ok(()) => vec![],
            Err(_) if self.retry_reported => vec![],
            Err(e) => {
                self.retry_reported = true;
                self.save_failed(&format!("snapshot after a lost command: {e}"))
            }
        }
    }

    /// Tick of the newest snapshot this game wrote or resumed from; `None`
    /// for a game without a save.
    pub fn last_snapshot_tick(&self) -> Option<u64> {
        self.last_snapshot
    }

    /// Save failures since the last call, oldest first (each was also sent
    /// to the connected players as `save_failed`).
    pub fn take_save_errors(&mut self) -> Vec<String> {
        std::mem::take(&mut self.save_errors)
    }

    /// Pause the clock without a vote and drop any open proposal: the last
    /// player has left (spec section 2.2). Players resume it by vote.
    pub fn pause_for_empty(&mut self) {
        self.clock.paused = true;
        self.clock.vote = None;
    }

    pub fn status(&self) -> GameStatus {
        let net = &self.sim.world().net;
        GameStatus {
            sim_time: self.sim.now_s(),
            tick: self.sim.tick(),
            paused: self.clock.paused,
            speed: self.clock.speed,
            holders: net.areas.iter().zip(&self.holders).map(|(a, h)| (a.name.clone(), h.clone())).collect(),
            players: self.players.iter().map(|(n, p)| (n.clone(), p.connected)).collect(),
            connected: self.players.values().filter(|p| p.connected).count(),
            stats: self.stats.clone(),
            save_busy: self.save.as_ref().map_or(Duration::ZERO, SaveDb::busy),
        }
    }

    fn save_failed(&mut self, why: &str) -> Vec<Out> {
        self.save_errors.push(why.to_string());
        self.players
            .iter()
            .filter(|(_, p)| p.connected)
            .map(|(name, _)| error(name, codes::SAVE_FAILED, why))
            .collect()
    }

    fn from_sim(sim: Sim, meta: GameMeta, paused: bool) -> Game {
        let w = sim.world();
        let map = AreaMap::new(w);
        let spectator = Visibility::spectator(w, &map);
        let by_area = (0..w.net.areas.len()).map(|a| Visibility::of_area(w, &map, AreaId::from_idx(a))).collect();
        let holders = vec![None; w.net.areas.len()];
        let geometry = WorldGeometry::from_world(w);
        let display = Display::from_world(w);
        Game {
            sim,
            meta,
            map,
            spectator,
            by_area,
            geometry,
            display,
            holders,
            players: BTreeMap::new(),
            clock: GameClock::new(paused),
            queued: Vec::new(),
            robot_ran_at: None,
            stats: GameStats::default(),
            save: None,
            since_snapshot_s: 0.0,
            last_snapshot: None,
            save_errors: Vec::new(),
            log_lost: false,
            retry_reported: false,
            vote_ended: Vec::new(),
        }
    }

    pub fn sim(&self) -> &Sim {
        &self.sim
    }

    pub fn meta(&self) -> &GameMeta {
        &self.meta
    }

    pub fn stats(&self) -> &GameStats {
        &self.stats
    }

    pub fn clock(&self) -> &GameClock {
        &self.clock
    }

    /// Who holds `area` (`None` for the robot or an unknown area).
    pub fn holder(&self, area: &str) -> Option<&str> {
        let a = self.sim.world().net.area(area)?;
        self.holders[a.idx()].as_deref()
    }

    /// The area `player` holds.
    pub fn area_of(&self, player: &str) -> Option<&str> {
        let a = self.players.get(player)?.area?;
        Some(self.sim.world().net.areas[a.idx()].name.as_str())
    }

    /// The full view `player` should hold now, numbered like the last one sent.
    pub fn view_of(&self, player: &str) -> Option<View> {
        let p = self.players.get(player)?;
        let seq = p.last.as_ref()?.seq;
        Some(build_view(&self.sim, &p.vis, &self.shared(), seq))
    }

    pub fn layout_of(&self, player: &str) -> Option<Layout> {
        let p = self.players.get(player)?;
        let mut layout = build_layout(self.sim.world(), &self.map, &p.vis, player, self.geometry.as_ref());
        self.display.fill(&mut layout, p.vis.area);
        Some(layout)
    }

    /// A player (re)connects and gets the layout and a full view.
    ///
    /// Players are identified by name only (C2 contract): when a new socket
    /// replaces an old one for the same name, the front forwards the new
    /// `connect` and must NOT forward a `disconnect` for the old socket, or
    /// the replacing connection would be marked gone.
    pub fn connect(&mut self, player: &str) -> Vec<Out> {
        if player == ROBOT || player == crate::seed::SEED {
            return vec![error(player, codes::RESERVED_NAME, &format!("`{player}` is a reserved name"))];
        }
        let spectator = self.spectator.clone();
        let p = self.players.entry(player.to_string()).or_insert_with(|| Player {
            area: None,
            connected: true,
            gone_s: 0.0,
            vis: spectator,
            last: None,
        });
        p.connected = true;
        p.gone_s = 0.0;
        self.settle_vote();
        self.resync(player)
    }

    /// A spectator is forgotten; a holder keeps their area for `GRACE_S`
    /// real seconds from their first disconnect (a repeat is ignored, so it
    /// cannot restart the grace period).
    ///
    /// Players are identified by name only (C2 contract): the front forwards
    /// a disconnect only when the player's current connection closes, never
    /// for a socket another connection has replaced.
    pub fn disconnect(&mut self, player: &str) {
        let Some(p) = self.players.get_mut(player) else { return };
        if !p.connected {
            return;
        }
        if p.area.is_none() {
            self.players.remove(player);
            self.settle_vote();
            return;
        }
        p.connected = false;
        p.gone_s = 0.0;
        p.last = None;
    }

    pub fn handle(&mut self, player: &str, msg: ClientMsg) -> Vec<Out> {
        if !self.players.get(player).is_some_and(|p| p.connected) {
            return vec![];
        }
        match msg {
            ClientMsg::Claim { area } => self.claim(player, &area),
            ClientMsg::Release => self.release(player),
            ClientMsg::Command { cmd } => self.command(player, cmd),
            ClientMsg::Vote { proposal } => self.vote(player, proposal),
            ClientMsg::VoteDecline => self.decline(player),
            ClientMsg::Resync => self.resync(player),
            // Only a tutorial (`crate::lesson::Runner`) acts on these.
            ClientMsg::LessonNext | ClientMsg::LessonRestartStep | ClientMsg::LessonRestart | ClientMsg::LessonUi { .. } => {
                vec![]
            }
        }
    }

    /// The layout and a full view; the view becomes the next delta's base.
    pub fn resync(&mut self, player: &str) -> Vec<Out> {
        let shared = self.shared();
        let Some(p) = self.players.get_mut(player) else { return vec![] };
        if !p.connected {
            return vec![];
        }
        let seq = p.last.as_ref().map_or(1, |v| v.seq + 1);
        let mut layout = build_layout(self.sim.world(), &self.map, &p.vis, player, self.geometry.as_ref());
        self.display.fill(&mut layout, p.vis.area);
        let view = build_view(&self.sim, &p.vis, &shared, seq);
        p.last = Some(view.clone());
        vec![(player.to_string(), ServerMsg::Layout(layout)), (player.to_string(), ServerMsg::View(view))]
    }

    /// Run the clock for `real_dt` seconds of real time.
    pub fn advance(&mut self, real_dt: f64) -> Vec<Out> {
        self.advance_with(real_dt, |_, _, _| Vec::new())
    }

    /// `advance`, calling `after_tick` after every tick with that tick's
    /// events and messages (a tutorial checks its step there). It may change
    /// the game; once it pauses the clock, no more ticks run.
    pub fn advance_with(
        &mut self,
        real_dt: f64,
        mut after_tick: impl FnMut(&mut Game, &[Event], &[Out]) -> Vec<Out>,
    ) -> Vec<Out> {
        let dt = if real_dt.is_finite() && real_dt > 0.0 { real_dt } else { 0.0 };
        let mut out = Vec::new();
        if let Some(p) = self.clock.lapse(dt) {
            self.vote_ended.push((p, VoteOutcome::Lapsed));
        }
        self.expire_grace(dt);
        let n = self.clock.ticks_for(dt).min(MAX_TICKS_PER_ADVANCE);
        for _ in 0..n {
            if self.clock.paused {
                break;
            }
            let (outs, events) = self.tick();
            let more = after_tick(self, &events, &outs);
            out.extend(outs);
            out.extend(more);
        }
        if self.save.is_some() && !self.clock.paused {
            self.since_snapshot_s += dt;
            if self.since_snapshot_s >= SNAPSHOT_EVERY_S {
                out.extend(self.save_now());
            }
        }
        out
    }

    /// Deltas for every connected player whose view changed, after a notice
    /// to each for every clock proposal that ended (polish spec M8).
    pub fn flush(&mut self) -> Vec<Out> {
        let shared = self.shared();
        let mut out = Vec::new();
        for (proposal, outcome) in std::mem::take(&mut self.vote_ended) {
            for (name, _) in self.players.iter().filter(|(_, p)| p.connected) {
                out.push(notice(name, Notice::VoteEnded { proposal, outcome: outcome.clone() }));
            }
        }
        for (name, p) in self.players.iter_mut() {
            if !p.connected {
                continue;
            }
            let Some(last) = &p.last else { continue };
            let view = build_view(&self.sim, &p.vis, &shared, last.seq + 1);
            if let Some(d) = protocol::diff(last, &view) {
                out.push((name.clone(), ServerMsg::Delta(d)));
                p.last = Some(view);
            }
        }
        out
    }

    fn shared(&self) -> Shared {
        let net = &self.sim.world().net;
        Shared {
            sim_time: self.sim.now_s(),
            speed: self.clock.speed,
            paused: self.clock.paused,
            vote: self.clock.vote_view(&self.voters()),
            holders: net
                .areas
                .iter()
                .zip(&self.holders)
                .map(|(a, h)| (a.name.clone(), h.clone().unwrap_or_else(|| ROBOT.to_string())))
                .collect(),
        }
    }

    fn area_name(&self, a: AreaId) -> String {
        self.sim.world().net.areas[a.idx()].name.clone()
    }

    /// Who decides the clock (owner decision 12, amending spec §3.5): every
    /// holder, connected or in their grace period; while nobody holds an
    /// area, every connected player. The robot never votes.
    pub fn voters(&self) -> BTreeSet<String> {
        let holders: BTreeSet<String> = self.holders.iter().flatten().cloned().collect();
        if !holders.is_empty() {
            return holders;
        }
        self.players.iter().filter(|(_, p)| p.connected).map(|(name, _)| name.clone()).collect()
    }

    /// The voters changed: an open proposal may now be complete, or have
    /// nobody left to agree to it.
    fn settle_vote(&mut self) {
        let voters = self.voters();
        if let Some(p) = self.clock.settle(&voters) {
            self.vote_ended.push((p, VoteOutcome::Passed));
        }
    }

    fn claim(&mut self, player: &str, area: &str) -> Vec<Out> {
        let Some(a) = self.sim.world().net.area(area) else {
            return vec![error(player, codes::UNKNOWN_AREA, &format!("no area `{area}`"))];
        };
        if let Some(h) = self.holders[a.idx()].clone() {
            if h == player {
                return self.resync(player);
            }
            return vec![notice(player, Notice::AreaTaken { area: area.to_string(), holder: h })];
        }
        if let Some(old) = self.players[player].area {
            self.holders[old.idx()] = None;
        }
        self.holders[a.idx()] = Some(player.to_string());
        let vis = self.by_area[a.idx()].clone();
        let p = self.players.get_mut(player).expect("connected players exist");
        p.area = Some(a);
        p.vis = vis;
        self.settle_vote();
        self.resync(player)
    }

    fn release(&mut self, player: &str) -> Vec<Out> {
        let Some(a) = self.players[player].area else {
            return vec![error(player, codes::NOT_HOLDING, "you hold no area")];
        };
        self.holders[a.idx()] = None;
        let vis = self.spectator.clone();
        let p = self.players.get_mut(player).expect("connected players exist");
        p.area = None;
        p.vis = vis;
        self.settle_vote();
        self.resync(player)
    }

    fn command(&mut self, player: &str, cmd: PlayerCommand) -> Vec<Out> {
        let reject = |reason| vec![notice(player, Notice::Rejected { cmd: cmd.clone(), reason, by: None })];
        let Some(core) = resolve(self.sim.world(), &cmd) else { return reject(Rejection::UnknownId) };
        if let PlayerCommand::Interpose { headcode, .. } = &cmd {
            if !valid_headcode(headcode) {
                return vec![error(player, codes::BAD_HEADCODE, "a headcode is 1 to 10 letters or digits")];
            }
        }
        let Some(area) = self.map.subject(&core) else { return reject(Rejection::NotPoints) };
        if self.players[player].area != Some(area) {
            return vec![notice(player, Notice::NotYourArea { area: self.area_name(area) })];
        }
        self.stats.player_commands += 1;
        self.submit(player, core)
    }

    fn vote(&mut self, player: &str, proposal: Proposal) -> Vec<Out> {
        let voters = self.voters();
        match self.clock.vote(player, proposal, &voters) {
            Ok(passed) => {
                // A lone voter's proposal applies at once: nothing to tell.
                if voters.len() > 1 {
                    self.vote_ended.extend(passed.map(|p| (p, VoteOutcome::Passed)));
                }
                vec![]
            }
            Err(VoteError::NotAVoter) => {
                vec![error(player, codes::NOT_A_HOLDER, "while anyone holds an area, only holders vote")]
            }
            Err(VoteError::BadSpeed) => vec![error(player, codes::BAD_SPEED, "speed must be 1, 2, 4 or 8")],
        }
    }

    fn decline(&mut self, player: &str) -> Vec<Out> {
        match self.clock.decline(player, &self.voters()) {
            Ok(declined) => {
                self.vote_ended.extend(declined.map(|p| (p, VoteOutcome::Declined { by: player.to_string() })));
                vec![]
            }
            Err(_) => vec![error(player, codes::NOT_A_HOLDER, "while anyone holds an area, only holders vote")],
        }
    }

    /// Queue a command for the next tick, logging it to the save first;
    /// `player` is `ROBOT` for the robot.
    fn submit(&mut self, player: &str, cmd: Command) -> Vec<Out> {
        let mut out = Vec::new();
        if let Err(e) = self.log_command(player, &cmd) {
            self.log_lost = true;
            out = self.save_failed(&e.to_string());
        }
        self.enqueue(player, cmd);
        out
    }

    /// Append `cmd` to the save's command log (nothing without a save).
    fn log_command(&self, player: &str, cmd: &Command) -> Result<(), SaveError> {
        let Some(db) = &self.save else { return Ok(()) };
        let area = self.map.subject(cmd).map(|a| self.area_name(a)).unwrap_or_default();
        db.append_command(self.sim.tick(), player, &area, cmd)
    }

    fn enqueue(&mut self, player: &str, cmd: Command) {
        self.queued.push((player.to_string(), cmd.clone()));
        self.sim.submit(cmd);
    }

    fn expire_grace(&mut self, dt: f64) {
        let mut expired = Vec::new();
        for (name, p) in self.players.iter_mut() {
            if !p.connected {
                p.gone_s += dt;
                if p.gone_s >= GRACE_S {
                    expired.push(name.clone());
                }
            }
        }
        if expired.is_empty() {
            return;
        }
        for name in &expired {
            if let Some(a) = self.players.remove(name).and_then(|p| p.area) {
                self.holders[a.idx()] = None;
            }
        }
        self.settle_vote();
    }

    /// The robot's commands for areas nobody holds, logged in one
    /// transaction that is committed before the sim steps with them.
    ///
    /// A run is saved whole or not at all. If any part of its log fails
    /// (SQLite may have rolled the whole transaction back by itself), the
    /// rest of the run is not logged and the batch is dropped: with no
    /// robot rows at this tick a resume runs the robot here again (the same
    /// commands, unless players held areas then: a resume leaves every area
    /// unclaimed). One `save_failed` names the run, and every tick ends
    /// with a snapshot attempt until one succeeds (`log_lost`), so later
    /// logged commands never replay over the gap.
    fn run_robot(&mut self, sender: &str) -> Vec<Out> {
        let mut out = Vec::new();
        let cmds: Vec<Command> = robot::commands(&self.sim)
            .into_iter()
            .filter(|cmd| self.map.subject(cmd).is_some_and(|area| self.holders[area.idx()].is_none()))
            .collect();
        if cmds.is_empty() {
            return out;
        }
        let tick = self.sim.tick();
        let mut failed: Option<SaveError> = self.save.as_ref().and_then(|db| db.begin_batch().err());
        for cmd in cmds {
            self.stats.robot_commands += 1;
            if failed.is_none() {
                failed = self.log_command(sender, &cmd).err();
            }
            self.enqueue(sender, cmd);
        }
        if let Some(db) = &self.save {
            let result = match failed {
                Some(e) => db.rollback_batch().and(Err(e)),
                None => db.commit_batch(),
            };
            if let Err(e) = result {
                self.log_lost = true;
                out.extend(self.save_failed(&format!("robot run at tick {tick} not saved: {e}")));
            }
        }
        out
    }

    /// One tick of seeding (`crate::seed`): the robot's run for every area
    /// (nobody holds one yet) logged as `seed`, then the step. Save
    /// failures are left in `take_save_errors`.
    pub(crate) fn seed_tick(&mut self) {
        if self.sim.tick() % robot::ROBOT_EVERY_TICKS == 0 {
            self.run_robot(crate::seed::SEED);
        }
        self.queued.clear();
        self.sim.step();
        if self.log_lost {
            self.cover_gap();
        }
    }

    /// The save, taken out (seeding moves its file).
    pub(crate) fn take_save(&mut self) -> Option<SaveDb> {
        self.save.take()
    }

    pub(crate) fn put_save(&mut self, db: SaveDb) {
        self.save = Some(db);
    }

    fn tick(&mut self) -> (Vec<Out>, Vec<Event>) {
        let mut out = Vec::new();
        let t = self.sim.tick();
        if t % robot::ROBOT_EVERY_TICKS == 0 && self.robot_ran_at != Some(t) {
            out.extend(self.run_robot(ROBOT));
        }
        let before = self.sim.describer().berths.clone();
        let queued = std::mem::take(&mut self.queued);
        let events = self.sim.step();
        // A command the log lost ran anyway: a snapshot now holds it, so a
        // resume does not replay later commands over the gap.
        if self.log_lost {
            out.extend(self.cover_gap());
        }
        let mut cursor = 0;
        for e in &events {
            match e {
                Event::SignalPassedAtDanger { .. } => self.stats.spads += 1,
                Event::Collision { .. } => self.stats.collisions += 1,
                Event::InvariantViolated { .. } => self.stats.invariant_violations += 1,
                Event::CommandRejected { cmd, reason, by } => {
                    self.stats.sim_rejections += 1;
                    if let Some(i) = (cursor..queued.len()).find(|&i| queued[i].1 == *cmd) {
                        cursor = i + 1;
                        let who = &queued[i].0;
                        if self.players.get(who).is_some_and(|p| p.connected) {
                            let named = to_player_command(self.sim.world(), cmd);
                            // The route in the way, found when the sim refused it (polish spec M4).
                            let by = by.map(|r| self.sim.world().routes[r.idx()].name.clone());
                            out.push(notice(who, Notice::Rejected { cmd: named, reason: *reason, by }));
                        }
                    }
                }
                _ => {}
            }
        }
        for (area, n) in area_notices(&self.sim, &self.map, &before, &events) {
            if let Some(h) = &self.holders[area.idx()] {
                if self.players.get(h).is_some_and(|p| p.connected) {
                    out.push(notice(h, n));
                }
            }
        }
        (out, events)
    }
}
