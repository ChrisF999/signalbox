//! The game: players, claims, area checks, the robot for unclaimed areas,
//! the clock, notices and per-player views (spec §6.1). Its only I/O is
//! the optional save (`crate::save`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use protocol::{ClientMsg, Layout, Notice, PlayerCommand, Proposal, Rejection, ServerMsg, View, codes};
use signalbox_core::events::{Command, Event};
use signalbox_core::ids::AreaId;
use signalbox_core::robot;
use signalbox_core::sim::Sim;
use signalbox_core::world::{LoadError, World};

use crate::areas::{AreaMap, Visibility};
use crate::clock::{GameClock, VoteError};
use crate::layout::build_layout;
use crate::names::{resolve, to_player_command, valid_headcode};
use crate::notices::area_notices;
use crate::save::{Logged, SaveDb, SaveError, resume_sim};
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
        let world = World::from_json(world_json)?;
        let db = SaveDb::create(path, &meta, world_json)?;
        let mut g = Game::new(world, meta);
        db.write_snapshot(&g.sim.snapshot())?;
        g.save = Some(db);
        Ok(g)
    }

    /// Resume the game saved at `path`: paused at 1x, every area unclaimed.
    pub fn resume(path: &Path) -> Result<Game, GameError> {
        let db = SaveDb::open(path)?;
        let saved = db.load()?;
        let world = World::from_json(&saved.world_json)?;
        let (sim, robot_ran) = resume_sim(world, saved.snapshot, &saved.commands_after).map_err(GameError::Resume)?;
        let mut g = Game::from_sim(sim, saved.meta, true);
        if robot_ran {
            g.robot_ran_at = Some(g.sim.tick());
        }
        // The sim's queue holds the commands logged at its tick; name their
        // senders so a sim rejection still goes back to them. A log that
        // disagrees with the queue (a failed append) leaves them unattributed.
        let queue = g.sim.snapshot().queue;
        let logged: Vec<&Logged> = saved.commands_after.iter().filter(|c| c.tick == g.sim.tick()).collect();
        let senders: Vec<String> = if logged.iter().map(|l| &l.command).eq(queue.iter()) {
            logged.iter().map(|l| l.player.clone()).collect()
        } else {
            vec![ROBOT.to_string(); queue.len()]
        };
        g.queued = senders.into_iter().zip(queue).collect();
        g.save = Some(db);
        Ok(g)
    }

    /// Snapshot now and restart the autosave timer. A failure goes to every
    /// connected player as `save_failed`; the game carries on.
    pub fn save_now(&mut self) -> Vec<Out> {
        self.since_snapshot_s = 0.0;
        let Some(db) = &self.save else { return vec![] };
        match db.write_snapshot(&self.sim.snapshot()) {
            Ok(()) => vec![],
            Err(e) => self.save_failed(&e.to_string()),
        }
    }

    fn save_failed(&self, why: &str) -> Vec<Out> {
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
        Game {
            sim,
            meta,
            map,
            spectator,
            by_area,
            holders,
            players: BTreeMap::new(),
            clock: GameClock::new(paused),
            queued: Vec::new(),
            robot_ran_at: None,
            stats: GameStats::default(),
            save: None,
            since_snapshot_s: 0.0,
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
        Some(build_layout(self.sim.world(), &self.map, &p.vis, player))
    }

    pub fn connect(&mut self, player: &str) -> Vec<Out> {
        if player == ROBOT {
            return vec![error(player, codes::RESERVED_NAME, "`robot` is a reserved name")];
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
        self.resync(player)
    }

    /// A spectator is forgotten; a holder keeps their area for `GRACE_S`.
    pub fn disconnect(&mut self, player: &str) {
        let Some(p) = self.players.get_mut(player) else { return };
        if p.area.is_none() {
            self.players.remove(player);
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
            ClientMsg::Resync => self.resync(player),
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
        let layout = build_layout(self.sim.world(), &self.map, &p.vis, player);
        let view = build_view(&self.sim, &p.vis, &shared, seq);
        p.last = Some(view.clone());
        vec![(player.to_string(), ServerMsg::Layout(layout)), (player.to_string(), ServerMsg::View(view))]
    }

    /// Run the clock for `real_dt` seconds of real time.
    pub fn advance(&mut self, real_dt: f64) -> Vec<Out> {
        let dt = if real_dt.is_finite() && real_dt > 0.0 { real_dt } else { 0.0 };
        let mut out = Vec::new();
        self.clock.lapse(dt);
        self.expire_grace(dt);
        let n = self.clock.ticks_for(dt).min(MAX_TICKS_PER_ADVANCE);
        for _ in 0..n {
            out.extend(self.tick());
        }
        if self.save.is_some() && !self.clock.paused {
            self.since_snapshot_s += dt;
            if self.since_snapshot_s >= SNAPSHOT_EVERY_S {
                out.extend(self.save_now());
            }
        }
        out
    }

    /// Deltas for every connected player whose view changed.
    pub fn flush(&mut self) -> Vec<Out> {
        let shared = self.shared();
        let mut out = Vec::new();
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
            vote: self.clock.vote_view(),
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

    fn holder_set(&self) -> BTreeSet<String> {
        self.holders.iter().flatten().cloned().collect()
    }

    fn settle_vote(&mut self) {
        let holders = self.holder_set();
        self.clock.settle(&holders);
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
        let reject = |reason| vec![notice(player, Notice::Rejected { cmd: cmd.clone(), reason })];
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
        let holders = self.holder_set();
        match self.clock.vote(player, proposal, &holders) {
            Ok(_) => vec![],
            Err(VoteError::NotAHolder) => vec![error(player, codes::NOT_A_HOLDER, "only players holding an area vote")],
            Err(VoteError::BadSpeed) => vec![error(player, codes::BAD_SPEED, "speed must be 1, 2, 4 or 8")],
        }
    }

    /// Queue a command for the next tick, logging it to the save first;
    /// `player` is `ROBOT` for the robot.
    fn submit(&mut self, player: &str, cmd: Command) -> Vec<Out> {
        let mut out = Vec::new();
        if let Some(db) = &self.save {
            let area = self.map.subject(&cmd).map(|a| self.area_name(a)).unwrap_or_default();
            if let Err(e) = db.append_command(self.sim.tick(), player, &area, &cmd) {
                out = self.save_failed(&e.to_string());
            }
        }
        self.queued.push((player.to_string(), cmd.clone()));
        self.sim.submit(cmd);
        out
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

    /// The robot's commands for areas nobody holds.
    fn run_robot(&mut self) -> Vec<Out> {
        let mut out = Vec::new();
        for cmd in robot::commands(&self.sim) {
            let Some(area) = self.map.subject(&cmd) else { continue };
            if self.holders[area.idx()].is_none() {
                self.stats.robot_commands += 1;
                out.extend(self.submit(ROBOT, cmd));
            }
        }
        out
    }

    fn tick(&mut self) -> Vec<Out> {
        let mut out = Vec::new();
        let t = self.sim.tick();
        if t % robot::ROBOT_EVERY_TICKS == 0 && self.robot_ran_at != Some(t) {
            out.extend(self.run_robot());
        }
        let before = self.sim.describer().berths.clone();
        let queued = std::mem::take(&mut self.queued);
        let events = self.sim.step();
        let mut cursor = 0;
        for e in &events {
            match e {
                Event::SignalPassedAtDanger { .. } => self.stats.spads += 1,
                Event::Collision { .. } => self.stats.collisions += 1,
                Event::InvariantViolated { .. } => self.stats.invariant_violations += 1,
                Event::CommandRejected { cmd, reason } => {
                    self.stats.sim_rejections += 1;
                    if let Some(i) = (cursor..queued.len()).find(|&i| queued[i].1 == *cmd) {
                        cursor = i + 1;
                        let who = &queued[i].0;
                        if self.players.get(who).is_some_and(|p| p.connected) {
                            let named = to_player_command(self.sim.world(), cmd);
                            out.push(notice(who, Notice::Rejected { cmd: named, reason: *reason }));
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
        out
    }
}
