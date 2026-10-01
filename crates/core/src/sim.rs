//! The simulation: world + state, stepped in fixed ticks.

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use crate::aspect::Aspect;
use crate::describer::Describer;
use crate::driver;
use crate::events::{Command, Event, Rejection};
use crate::ids::*;
use crate::interlocking::Interlocking;
use crate::network::{Dir, Network, NodeKind};
use crate::occupancy::Occupancy;
use crate::points::PointsTable;
use crate::robot::PlanCache;
use crate::scoring::Scores;
use crate::timetable::{EndAction, EntryStart, draw_delay, earliest_delay_s};
use crate::trains::{Dwell, Train};
use crate::world::World;

/// The berth a train at `along` on `seg` facing `dir` is described in: the
/// first signal ahead's, or, on a road ending at buffers with no signal
/// ahead, the berth of the platform starter facing back out (see
/// `Network::terminal_signal`), where a terminating train stays shown.
fn berth_ahead(net: &Network, seg: SegmentId, dir: Dir, along: f64, pts: &PointsTable) -> Option<BerthId> {
    match net.first_signal_ahead(seg, dir, along, SIGNAL_SEARCH_M, pts) {
        Some((n, _)) => net.signals[n.idx()].berth,
        None => net.terminal_signal(seg, dir, along, SIGNAL_SEARCH_M, pts).and_then(|n| net.signals[n.idx()].berth),
    }
}

/// Seconds of sim time per tick.
pub const TICK_S: f64 = 0.1;
/// Seconds a train stands after a SPAD before its driver carries on.
pub const SPAD_HOLD_S: f64 = 60.0;
/// How far ahead to look for the next signal (describer stepping).
const SIGNAL_SEARCH_M: f64 = 5_000.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingEntry {
    pub entry: usize,
    pub due_s: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Exited,
    Stabled,
}

/// Everything that changes while the simulation runs. Serialisable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimState {
    pub tick: u64,
    pub points: PointsTable,
    pub il: Interlocking,
    pub trains: Vec<Train>,
    pub next_train_id: u32,
    pub finished: Vec<(TrainId, Outcome)>,
    pub next_entry: usize,
    pub pending: Vec<PendingEntry>,
    pub describer: Describer,
    pub scores: Scores,
    pub queue: Vec<Command>,
    /// Every command applied, with the tick it was applied at.
    pub log: Vec<(u64, Command)>,
    /// Train pairs (lower id first) already reported as collided, sorted.
    #[serde(default)]
    pub collided: Vec<(TrainId, TrainId)>,
    pub rng_seed: [u8; 32],
    /// ChaCha word position (hi, lo), filled in by `snapshot`.
    pub rng_word_pos: (u64, u64),
}

#[derive(Default)]
struct TickOut {
    exited: Vec<usize>,
    stabled: Vec<TrainId>,
    emergency: Vec<TrainId>,
}

pub struct Sim {
    world: World,
    st: SimState,
    rng: ChaCha8Rng,
    /// Derived from trains at the end of every tick.
    occ: Occupancy,
    /// The robot's journey plans, derived from the world alone.
    plans: PlanCache,
}

impl Sim {
    pub fn new(world: World, seed: u64) -> Sim {
        let rng = ChaCha8Rng::seed_from_u64(seed);
        let st = SimState {
            tick: 0,
            points: PointsTable::new(&world.net),
            il: Interlocking::new(&world),
            trains: Vec::new(),
            next_train_id: 0,
            finished: Vec::new(),
            next_entry: 0,
            pending: Vec::new(),
            describer: Describer::new(&world.net),
            scores: Scores::new(&world.net),
            queue: Vec::new(),
            log: Vec::new(),
            collided: Vec::new(),
            rng_seed: rng.get_seed(),
            rng_word_pos: (0, 0),
        };
        let occ = Occupancy::new(world.net.sections.len());
        let mut sim = Sim { world, st, rng, occ, plans: PlanCache::default() };
        for i in 0..sim.world.routes.len() {
            if sim.world.routes[i].automatic {
                // Automatic signals are set from the start. World load rejects automatic
                // routes that share track or disagree on points, so a failure here
                // cannot happen; ignore it.
                let _ = sim.st.il.set_route(&sim.world, &mut sim.st.points, &sim.occ, RouteId::from_idx(i));
            }
        }
        sim
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// The robot's cache of journey plans for this sim's world.
    pub(crate) fn plan_cache(&self) -> &PlanCache {
        &self.plans
    }

    pub fn tick(&self) -> u64 {
        self.st.tick
    }

    pub fn now_s(&self) -> f64 {
        self.world.options.start_s + self.st.tick as f64 * TICK_S
    }

    /// How long the longest-waiting due entry has been held at the fringe.
    pub fn longest_fringe_wait_s(&self) -> f64 {
        let now = self.now_s();
        self.st.pending.iter().map(|p| (now - p.due_s).max(0.0)).fold(0.0, f64::max)
    }

    pub fn trains(&self) -> &[Train] {
        &self.st.trains
    }

    /// Entries already offered at the fringe and waiting to enter, oldest first.
    pub fn pending_entries(&self) -> &[PendingEntry] {
        &self.st.pending
    }

    /// Index into `world().entries` of the first entry not yet offered.
    pub fn next_entry(&self) -> usize {
        self.st.next_entry
    }

    pub fn points(&self) -> &PointsTable {
        &self.st.points
    }

    pub fn interlocking(&self) -> &Interlocking {
        &self.st.il
    }

    pub fn occupancy(&self) -> &Occupancy {
        &self.occ
    }

    pub fn describer(&self) -> &Describer {
        &self.st.describer
    }

    pub fn scores(&self) -> &Scores {
        &self.st.scores
    }

    pub fn finished(&self) -> &[(TrainId, Outcome)] {
        &self.st.finished
    }

    pub fn log(&self) -> &[(u64, Command)] {
        &self.st.log
    }

    /// Entries not yet on the network (not yet due, or waiting at the fringe).
    pub fn entries_waiting(&self) -> usize {
        self.world.entries.len() - self.st.next_entry + self.st.pending.len()
    }

    pub fn aspect(&self, s: SignalId) -> Aspect {
        self.st.il.aspects[s.idx()]
    }

    /// Queue a command; it is applied at the start of the next tick.
    pub fn submit(&mut self, cmd: Command) {
        self.st.queue.push(cmd);
    }

    /// Put a train straight onto the network (tests and tools).
    pub fn insert_train(&mut self, t: Train) {
        self.st.next_train_id = self.st.next_train_id.max(t.id.0 + 1);
        self.st.trains.push(t);
        self.rebuild_occupancy();
    }

    /// The full dynamic state, including the RNG position.
    pub fn snapshot(&self) -> SimState {
        let mut st = self.st.clone();
        let pos = self.rng.get_word_pos();
        st.rng_word_pos = ((pos >> 64) as u64, pos as u64);
        st
    }

    /// FNV-1a hash of the full serialised state (`snapshot()` as JSON), for
    /// comparing runs: equal hashes mean the same state.
    pub fn state_hash(&self) -> u64 {
        let state = serde_json::to_string(&self.snapshot()).expect("state serialises");
        state.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3))
    }

    /// Rebuild a simulation from a world and a snapshot taken on that world.
    /// Fails if the snapshot's tables do not match the world's size.
    pub fn restore(world: World, st: SimState) -> Result<Sim, String> {
        let net = &world.net;
        let checks = [
            ("points", st.points.len(), net.nodes.len()),
            ("interlocking routes", st.il.routes.len(), world.routes.len()),
            ("interlocking owners", st.il.owner.len(), net.sections.len()),
            ("interlocking aspects", st.il.aspects.len(), net.signals.len()),
            ("describer berths", st.describer.berths.len(), net.berths.len()),
            ("scores areas", st.scores.by_area.len(), net.areas.len()),
        ];
        for (what, have, want) in checks {
            if have != want {
                return Err(format!("snapshot does not match the world: {what} has {have} entries, expected {want}"));
            }
        }
        for (r, status) in st.il.routes.iter().zip(&world.routes) {
            if r.progress.len() != status.path.len() {
                return Err(format!("snapshot does not match the world: route {} progress length", status.name));
            }
        }
        let mut rng = ChaCha8Rng::from_seed(st.rng_seed);
        rng.set_word_pos((u128::from(st.rng_word_pos.0) << 64) | u128::from(st.rng_word_pos.1));
        let occ = Occupancy::new(world.net.sections.len());
        let mut sim = Sim { world, st, rng, occ, plans: PlanCache::default() };
        sim.rebuild_occupancy();
        Ok(sim)
    }

    /// Re-run a session from its seed and command log for `ticks` ticks.
    /// The log need not be sorted; commands of one tick keep their order.
    pub fn replay(world: World, seed: u64, log: &[(u64, Command)], ticks: u64) -> Sim {
        let mut sorted = log.to_vec();
        sorted.sort_by_key(|&(tick, _)| tick);
        let log = sorted;
        let mut sim = Sim::new(world, seed);
        let mut i = 0;
        while sim.st.tick < ticks {
            while i < log.len() && log[i].0 < sim.st.tick {
                i += 1;
            }
            while i < log.len() && log[i].0 == sim.st.tick {
                sim.submit(log[i].1.clone());
                i += 1;
            }
            sim.step();
        }
        sim
    }

    pub fn run_for(&mut self, secs: f64) -> Vec<Event> {
        let n = (secs / TICK_S).round() as u64;
        (0..n).flat_map(|_| self.step()).collect()
    }

    /// Advance one tick.
    pub fn step(&mut self) -> Vec<Event> {
        let mut ev = Vec::new();
        let now = self.now_s();
        for cmd in std::mem::take(&mut self.st.queue) {
            self.st.log.push((self.st.tick, cmd.clone()));
            match self.apply(&cmd) {
                Ok(e) => ev.extend(e),
                Err(reason) => ev.push(Event::CommandRejected { cmd, reason }),
            }
        }
        for (p, to) in self.st.points.tick(TICK_S) {
            ev.push(Event::PointsMoved { points: p, to });
        }
        ev.extend(self.spawn_entries(now));
        ev.extend(self.st.il.update(&self.world, &self.st.points, &self.occ, TICK_S));
        ev.extend(self.st.il.refresh_aspects(&self.world, &self.st.points, &self.occ));
        ev.extend(self.move_trains(now));
        self.rebuild_occupancy();
        ev.extend(self.detect_collisions());
        for e in &ev {
            self.st.scores.apply(&self.world, e);
        }
        self.st.tick += 1;
        ev
    }

    fn apply(&mut self, cmd: &Command) -> Result<Vec<Event>, Rejection> {
        let w = &self.world;
        let net = &w.net;
        let signal_ok = |s: &SignalId| if s.idx() < net.signals.len() { Ok(()) } else { Err(Rejection::UnknownId) };
        match cmd {
            Command::SetRoute { entrance, exit } => {
                signal_ok(entrance)?;
                let r = w.find_route(*entrance, *exit).ok_or(Rejection::NoSuchRoute)?;
                self.st.il.set_route(w, &mut self.st.points, &self.occ, r)
            }
            Command::CancelRoute { entrance } => {
                signal_ok(entrance)?;
                let r = self.st.il.active_route_from(w, *entrance).ok_or(Rejection::RouteNotSet)?;
                self.st.il.cancel_route(w, &self.st.points, &self.occ, r)
            }
            Command::SetAutoWorking { entrance, on } => {
                signal_ok(entrance)?;
                let r = self.st.il.active_route_from(w, *entrance).ok_or(Rejection::RouteNotSet)?;
                self.st.il.set_auto_working(r, *on)
            }
            Command::SwingPoints { points, to } => {
                if points.idx() >= net.nodes.len() {
                    return Err(Rejection::UnknownId);
                }
                let sec = net.points_section(*points).ok_or(Rejection::NotPoints)?;
                if self.st.il.owner[sec.idx()].is_some() {
                    return Err(Rejection::PointsLocked);
                }
                if self.occ.occupied(sec) {
                    return Err(Rejection::PointsOccupied);
                }
                if self.st.points.detected(*points) == Some(*to) {
                    return Ok(vec![]);
                }
                self.st.points.start_swing(*points, *to, net.swing_s(*points));
                Ok(vec![Event::PointsMoving { points: *points, to: *to }])
            }
            Command::Interpose { berth, headcode } => {
                if berth.idx() >= net.berths.len() {
                    return Err(Rejection::UnknownId);
                }
                Ok(self.st.describer.interpose(*berth, headcode))
            }
            Command::CancelBerth { berth } => {
                if berth.idx() >= net.berths.len() {
                    return Err(Rejection::UnknownId);
                }
                Ok(self.st.describer.cancel(*berth).into_iter().collect())
            }
        }
    }

    /// Offer due entries to the fringe berth, then put waiting trains on the
    /// network when their entry section is free.
    ///
    /// Each entry is offered (and its delay drawn, in entry order) at its
    /// booked time plus the earliest delay its bands allow (0 without bands,
    /// so worlds without bands behave exactly as before), which lets a
    /// negative delay bring a train in early, as in TS2.
    fn spawn_entries(&mut self, now: f64) -> Vec<Event> {
        let mut ev = Vec::new();
        let lead = f64::from(earliest_delay_s(&self.world.options.entry_delay_bands));
        while self.st.next_entry < self.world.entries.len() && self.world.entries[self.st.next_entry].time_s + lead <= now {
            let i = self.st.next_entry;
            self.st.next_entry += 1;
            if self.world.entries[i].on_demand {
                continue;
            }
            let o = &self.world.options;
            let delay = f64::from(draw_delay(&o.entry_delay_bands, o.entry_delay_s, &mut self.rng));
            ev.extend(self.offer(i, self.world.entries[i].time_s + delay));
        }
        let mut used: Vec<SectionId> = Vec::new();
        let mut k = 0;
        while k < self.st.pending.len() {
            let p = self.st.pending[k].clone();
            let sections: Vec<SectionId> = self
                .entry_train(p.entry, TrainId(u32::MAX))
                .segments()
                .map(|s| self.world.net.segments[s.idx()].section)
                .collect();
            let free = sections
                .iter()
                .all(|&s| !self.occ.occupied(s) && self.st.il.owner[s.idx()].is_none() && !used.contains(&s));
            if now >= p.due_s && free {
                self.st.pending.remove(k);
                used.extend(sections);
                ev.extend(self.spawn(p.entry));
            } else {
                k += 1;
            }
        }
        ev
    }

    /// Offer entry `i` at the fringe, due at `due_s`: its headcode goes into
    /// its boundary's berth, and the train enters once its track is free.
    fn offer(&mut self, i: usize, due_s: f64) -> Vec<Event> {
        let e = &self.world.entries[i];
        self.st.pending.push(PendingEntry { entry: i, due_s });
        let fringe = match e.start {
            EntryStart::Boundary(n) => self.world.net.boundary_berth(n),
            EntryStart::At(_) => None,
        };
        // The fringe berth shows the first entry waiting there; later ones
        // follow as earlier ones enter (see `spawn`).
        match fringe {
            Some(b) if self.st.describer.get(b).is_none() => {
                self.st.describer.interpose(b, &self.world.services[e.service.idx()].headcode)
            }
            _ => vec![],
        }
    }

    /// Offer the on-demand entry `entry` now (the tutorial's `spawn`); it is
    /// due at once and enters at the next tick its track is free. Not a
    /// logged command: worlds with on-demand entries are only run by
    /// tutorials, which are never saved or replayed.
    pub fn offer_entry(&mut self, entry: usize) -> Result<Vec<Event>, String> {
        let e = self.world.entries.get(entry).ok_or_else(|| format!("no entry {entry}"))?;
        if !e.on_demand {
            return Err(format!("entry {entry} is not on demand"));
        }
        let now = self.now_s();
        Ok(self.offer(entry, now))
    }

    /// The train an entry would put on the network (not yet added).
    fn entry_train(&self, entry: usize, id: TrainId) -> Train {
        let w = &self.world;
        let e = &w.entries[entry];
        let svc = &w.services[e.service.idx()];
        let tt = &w.train_types[svc.train_type.idx()];
        let speed = e.speed.min(tt.max_speed);
        match e.start {
            EntryStart::Boundary(n) => {
                let seg = w.net.nodes[n.idx()].segments[0];
                let dir = if w.net.segments[seg.idx()].a == n { Dir::Up } else { Dir::Down };
                Train::new(id, e.service, &svc.headcode, svc.train_type, tt.length_m, seg, dir, speed)
            }
            EntryStart::At(at) => Train::placed(
                id,
                e.service,
                &svc.headcode,
                svc.train_type,
                tt.length_m,
                at,
                speed,
                &w.net,
                &self.st.points,
            ),
        }
    }

    fn spawn(&mut self, entry: usize) -> Vec<Event> {
        let id = TrainId(self.st.next_train_id);
        self.st.next_train_id += 1;
        let t = self.entry_train(entry, id);
        let w = &self.world;
        let e = &w.entries[entry];
        let headcode = w.services[e.service.idx()].headcode.clone();
        let mut ev = vec![Event::TrainEntered { train: id, headcode: headcode.clone() }];
        let (hs, hd) = t.head();
        let next = berth_ahead(&w.net, hs, hd, t.head_m, &self.st.points);
        // The entering train takes its own headcode with it (the fringe
        // berth may show another waiting entry), and the fringe berth then
        // shows the next entry still waiting at that boundary, if any.
        if let EntryStart::Boundary(n) = e.start {
            if let Some(b) = w.net.boundary_berth(n) {
                if self.st.describer.get(b) == Some(headcode.as_str()) {
                    ev.extend(self.st.describer.cancel(b));
                }
                let waiting = self.st.pending.iter().map(|p| &w.entries[p.entry]).find(|x| x.start == e.start);
                if let Some(x) = waiting {
                    if self.st.describer.get(b).is_none() {
                        ev.extend(self.st.describer.interpose(b, &w.services[x.service.idx()].headcode));
                    }
                }
            }
        }
        if let Some(b) = next {
            ev.extend(self.st.describer.interpose(b, &headcode));
        }
        self.st.trains.push(t);
        ev
    }

    fn move_trains(&mut self, now: f64) -> Vec<Event> {
        let mut out = TickOut::default();
        let mut ev = Vec::new();
        for ti in 0..self.st.trains.len() {
            ev.extend(self.move_train(ti, now, &mut out));
        }
        for id in out.emergency {
            if let Some(t) = self.st.trains.iter_mut().find(|t| t.id == id) {
                t.emergency = true;
            }
        }
        for id in out.stabled {
            self.st.finished.push((id, Outcome::Stabled));
        }
        for &ti in out.exited.iter().rev() {
            let t = self.st.trains.remove(ti);
            self.st.finished.push((t.id, Outcome::Exited));
        }
        ev
    }

    fn move_train(&mut self, ti: usize, now: f64, out: &mut TickOut) -> Vec<Event> {
        let w = &self.world;
        let net = &w.net;
        let pts = &self.st.points;
        let aspects = &self.st.il.aspects;
        let t = &mut self.st.trains[ti];
        let mut ev = Vec::new();
        if t.stabled {
            return ev;
        }
        let svc = &w.services[t.service.idx()];
        let tt = &w.train_types[t.train_type.idx()];

        // Departure from a stop.
        if let Some(d) = t.dwell {
            // Only with a proceed aspect ahead: a red signal within sighting holds the train.
            let (hs, hd) = t.head();
            let held_by_red = net
                .first_signal_ahead(hs, hd, t.head_m, SIGNAL_SEARCH_M, pts)
                .is_some_and(|(sg, dist)| dist <= net.signals[sg.idx()].sighting_m && aspects[sg.idx()] == Aspect::Red);
            if now >= d.depart_at_s && !held_by_red {
                t.dwell = None;
                t.next_call += 1;
                ev.push(Event::TrainDeparted { train: t.id, platform: d.platform });
            }
        }
        // After a SPAD the driver stands, then carries on.
        if t.emergency && t.speed == 0.0 {
            t.emergency = false;
            t.hold_until_s = Some(now + SPAD_HOLD_S);
        }
        let held = t.dwell.is_some() || t.hold_until_s.is_some_and(|h| now < h);
        let stop_place = svc.calls.get(t.next_call).filter(|c| c.stop).map(|c| c.place.as_str());
        let target = if held { 0.0 } else { driver::target_speed(t, tt, net, pts, aspects, stop_place) };
        driver::apply_speed(t, tt, net, target, TICK_S);

        let moved = t.advance(net, pts, t.speed * TICK_S);

        for sw in &moved.swept {
            let sg = &net.segments[sw.seg.idx()];
            let mut passed: Vec<(f64, SignalId)> = net.signals_on[sw.seg.idx()]
                .iter()
                .copied()
                .filter(|&s| net.signals[s.idx()].at.dir == sw.dir)
                .map(|s| (sg.along(net.signals[s.idx()].at.offset_m, sw.dir), s))
                .filter(|&(a, _)| sw.from < a && a <= sw.to)
                .collect();
            passed.sort_by(|x, y| x.0.total_cmp(&y.0));
            for (a, s) in passed {
                let aspect = aspects[s.idx()];
                ev.push(Event::SignalPassed { signal: s, train: t.id });
                if aspect == Aspect::Red {
                    ev.push(Event::SignalPassedAtDanger { signal: s, train: t.id });
                    t.emergency = true;
                }
                t.last_passed_aspect = Some(aspect);
                t.last_passed_aspects = net.signals[s.idx()].aspects;
                if let Some(b) = net.signals[s.idx()].berth {
                    // Into a terminal road, the headcode goes to the platform
                    // starter's berth and stays there while the train stands
                    // (forming changes it in place; stabling keeps it).
                    let next = berth_ahead(net, sw.seg, sw.dir, a, pts);
                    ev.extend(self.st.describer.step(b, next));
                }
            }
            // Non-stopping calls are recorded as the train runs through the platform.
            if let Some(c) = svc.calls.get(t.next_call).filter(|c| !c.stop) {
                let hit = net.platforms_on[sw.seg.idx()].iter().copied().find(|&p| {
                    let (near, far) = net.platform_along(p, sw.dir);
                    net.platforms[p.idx()].place == c.place && sw.from < far && near <= sw.to
                });
                if let Some(p) = hit {
                    let sched = c.dep_s.or(c.arr_s).unwrap_or(now);
                    ev.push(Event::TrainPassed { train: t.id, platform: p, late_s: (now - sched).round() as i64 });
                    t.next_call += 1;
                }
            }
        }

        if let Some(node) = moved.end_node {
            if net.nodes[node.idx()].kind == NodeKind::Boundary {
                ev.push(Event::TrainExited { train: t.id, boundary: node });
                out.exited.push(ti);
                return ev;
            }
            t.speed = 0.0;
        }

        // Arrival at a stopping call.
        if t.speed == 0.0 && t.dwell.is_none() {
            if let Some(c) = svc.calls.get(t.next_call).filter(|c| c.stop) {
                if let Some(p) = platform_at_head(net, t, &c.place) {
                    let sched = c.arr_s.or(c.dep_s).unwrap_or(now);
                    ev.push(Event::TrainArrived { train: t.id, platform: p, late_s: (now - sched).round() as i64 });
                    if let Some(want) = &c.platform {
                        if *want != net.platforms[p.idx()].platform {
                            ev.push(Event::WrongPlatform { train: t.id, platform: p, expected: want.clone() });
                        }
                    }
                    let dwell = f64::from(draw_delay(&w.options.min_dwell_bands, w.options.min_dwell_s, &mut self.rng));
                    t.dwell = Some(Dwell { platform: p, depart_at_s: c.dep_s.unwrap_or(0.0).max(now + dwell) });
                }
            }
        }

        // End of the service.
        if t.speed == 0.0 && t.dwell.is_none() && !t.end_done && t.next_call >= svc.calls.len() {
            match &svc.end {
                EndAction::Exit => {}
                EndAction::Stable => {
                    t.stabled = true;
                    t.end_done = true;
                    ev.push(Event::TrainStabled { train: t.id });
                    out.stabled.push(t.id);
                }
                EndAction::Form(next) => {
                    if t.reverse(net) {
                        let next = *next;
                        let nsvc = &w.services[next.idx()];
                        t.service = next;
                        t.headcode = nsvc.headcode.clone();
                        t.train_type = nsvc.train_type;
                        t.next_call = 0;
                        t.last_passed_aspect = None;
                        t.last_passed_aspects = 3;
                        ev.push(Event::TrainFormed { train: t.id, headcode: t.headcode.clone() });
                        let (hs, hd) = t.head();
                        let berth = net
                            .first_signal_ahead(hs, hd, t.head_m, SIGNAL_SEARCH_M, pts)
                            .and_then(|(s, _)| net.signals[s.idx()].berth);
                        if let Some(b) = berth {
                            ev.extend(self.st.describer.interpose(b, &t.headcode));
                        }
                    }
                }
            }
        }
        ev
    }

    /// Every section holding two trains is a collision, reported once per
    /// pair; both trains go into emergency.
    fn detect_collisions(&mut self) -> Vec<Event> {
        let mut ev = Vec::new();
        for si in 0..self.world.net.sections.len() {
            let sec = SectionId::from_idx(si);
            let here = self.occ.trains_in(sec).to_vec();
            for (i, &x) in here.iter().enumerate() {
                for &y in &here[i + 1..] {
                    let pair = (x.min(y), x.max(y));
                    if let Err(at) = self.st.collided.binary_search(&pair) {
                        self.st.collided.insert(at, pair);
                        ev.push(Event::Collision { train: pair.1, other: pair.0, section: sec });
                        for t in self.st.trains.iter_mut().filter(|t| t.id == x || t.id == y) {
                            t.emergency = true;
                        }
                    }
                }
            }
        }
        ev
    }

    fn rebuild_occupancy(&mut self) {
        let mut occ = Occupancy::new(self.world.net.sections.len());
        for t in &self.st.trains {
            for s in t.segments() {
                occ.add(self.world.net.segments[s.idx()].section, t.id, t.speed > 0.0);
            }
        }
        self.occ = occ;
    }
}

/// The platform of `place` the train's head is standing at, if any.
fn platform_at_head(net: &Network, t: &Train, place: &str) -> Option<PlatformId> {
    let (seg, dir) = t.head();
    net.platforms_on[seg.idx()].iter().copied().find(|&p| {
        let (near, far) = net.platform_along(p, dir);
        net.platforms[p.idx()].place == place && near <= t.head_m && t.head_m <= far + 1e-6
    })
}
