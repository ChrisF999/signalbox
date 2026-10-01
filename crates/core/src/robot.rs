//! A robot signaller: sets routes for each train towards its next calls (or
//! an exit), and a soak runner built on it.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use crate::events::{Command, Event};
use crate::ids::*;
use crate::network::NodeKind;
use crate::routes::{Exit, RouteDef};
use crate::sim::{Outcome, Sim, TICK_S};
use crate::timetable::EndAction;
use crate::trains::Train;
use crate::world::World;

/// A train standing still this long (not at a booked stop, not stabled) is stuck.
pub const STUCK_S: f64 = 1800.0;
const SIGNAL_SEARCH_M: f64 = 3_000.0;
const MAX_ROUTE_DEPTH: usize = 30;
/// How long before a dwelling train's departure time the robot sets its road.
const DEPARTURE_LEAD_S: f64 = 30.0;
/// The robot looks at the railway once per this many ticks.
pub const ROBOT_EVERY_TICKS: u64 = 10;

enum Goal<'a> {
    Platform { place: &'a str, platform: Option<&'a str> },
    Exit,
}

fn reaches(w: &World, def: &RouteDef, goal: &Goal, strict: bool) -> bool {
    match goal {
        Goal::Exit => matches!(def.exit, Exit::Node(n) if w.net.nodes[n.idx()].kind == NodeKind::Boundary),
        Goal::Platform { place, platform } => def.path.iter().any(|&sec| {
            w.net.sections[sec.idx()].segments.iter().any(|&sg| {
                w.net.platforms_on[sg.idx()].iter().any(|&p| {
                    let pl = &w.net.platforms[p.idx()];
                    pl.place == *place && (!strict || platform.is_none_or(|want| want == pl.platform))
                })
            })
        }),
    }
}

/// What a journey plan costs, compared in field order: calls missed (skipped
/// as unreachable, or left for later after a stop), then calls reached at
/// the wrong platform, then routes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Cost {
    missed: u32,
    off: u32,
    routes: u32,
}

/// A search state: route `.0` is the last on the chain, calls before `.1`
/// (an index into the journey's goals) are dealt with, and `.2` says route
/// `.0` ends a leg (reaches a stopping call or the exit).
type Key = (RouteId, usize, bool);

/// How a route meets one goal: not at all, at another platform of its
/// place, or as booked. Memoised per search (`Reach::Unknown` until asked).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reach {
    Unknown,
    No,
    Off,
    Booked,
}

/// One journey search: the goals and what each route makes of each goal.
struct Search<'a> {
    w: &'a World,
    goals: Vec<(Goal<'a>, bool)>,
    reach: Vec<Reach>,
}

impl Search<'_> {
    fn reach(&mut self, r: RouteId, k: usize) -> Reach {
        let i = r.idx() * self.goals.len() + k;
        if self.reach[i] == Reach::Unknown {
            let def = &self.w.routes[r.idx()];
            let goal = &self.goals[k].0;
            self.reach[i] = if reaches(self.w, def, goal, true) {
                Reach::Booked
            } else if reaches(self.w, def, goal, false) {
                Reach::Off
            } else {
                Reach::No
            };
        }
        self.reach[i]
    }

    /// Every way route `r`, entered with goals `k..` still to reach at cost
    /// `c`, can deal with them: reach the next goal (at its booked platform,
    /// or at another for one more `off`), skip it as unreachable (one more
    /// `missed`; never the first goal, which the train may be standing at
    /// before its dwell starts, nor the last, so a plan always ends somewhere
    /// the train was going), or leave it for later routes. Reaching a
    /// stopping goal ends the leg: the train stops there, so nothing more is
    /// reached on this route.
    fn absorb(&mut self, r: RouteId, k: usize, c: Cost, out: &mut Vec<(usize, Cost, bool)>) {
        out.push((k, c, false));
        let n = self.goals.len();
        if k >= n {
            return;
        }
        let reached = match self.reach(r, k) {
            Reach::Booked => Some(c),
            Reach::Off => Some(Cost { off: c.off + 1, ..c }),
            _ => None,
        };
        if let Some(c2) = reached {
            if self.goals[k].1 {
                out.push((k + 1, c2, true));
            } else {
                self.absorb(r, k + 1, c2, out);
            }
        }
        if k > 0 && k + 1 < n {
            self.absorb(r, k + 1, Cost { missed: c.missed + 1, ..c }, out);
        }
    }
}

/// The cheapest chain of routes (see `Cost`) from `from` through the
/// service's calls `k0..` and out, for an exit service, planned as one
/// search so that a leg's route choice never strands a later leg off its
/// booked line or platform when the track allows them all. Each route is
/// marked when it ends a leg. After a stopping call the plan may stop short
/// (the rest counted as missed) if nothing further can be reached; the
/// rest is planned again when the train gets there.
///
/// Dijkstra over (route, goals dealt with, ends a leg). The queue orders
/// entries completely (cost, then route id, goal index, flags), so ties
/// always break the same way and the result is deterministic. Each leg may
/// take at most `MAX_ROUTE_DEPTH` routes on average.
fn journey(w: &World, t: &Train, from: SignalId, k0: usize) -> Option<Journey> {
    let svc = &w.services[t.service.idx()];
    let mut goals: Vec<(Goal, bool)> = svc
        .calls
        .iter()
        .skip(k0)
        .map(|c| (Goal::Platform { place: &c.place, platform: c.platform.as_deref() }, c.stop))
        .collect();
    if svc.end == EndAction::Exit {
        goals.push((Goal::Exit, true));
    }
    let n = goals.len();
    if n == 0 {
        return Some(vec![]);
    }
    let mut search = Search { w, goals, reach: vec![Reach::Unknown; w.routes.len() * n] };
    let index = |(r, k, ends): Key| (r.idx() * (n + 1) + k) * 2 + usize::from(ends);
    // state → (cost, the state before it)
    let mut best: Vec<Option<(Cost, Option<Key>)>> = vec![None; w.routes.len() * (n + 1) * 2];
    // (cost, state, is a finished plan); a finished plan costs its state's
    // cost plus the goals it leaves. Stale entries are skipped when popped.
    let mut queue: BinaryHeap<Reverse<(Cost, Key, bool)>> = BinaryHeap::new();
    let finish = |c: Cost, k: usize| Cost { missed: c.missed + (n - k) as u32, ..c };
    let mut outcomes = Vec::new();
    let relax = |best: &mut Vec<Option<(Cost, Option<Key>)>>,
                     queue: &mut BinaryHeap<Reverse<(Cost, Key, bool)>>,
                     r: RouteId,
                     parent: Option<Key>,
                     outcomes: &mut Vec<(usize, Cost, bool)>| {
        for (k, c, ends) in outcomes.drain(..) {
            let key = (r, k, ends);
            let slot = &mut best[index(key)];
            if slot.is_some_and(|(b, _)| b <= c) {
                continue;
            }
            *slot = Some((c, parent));
            if k < n {
                queue.push(Reverse((c, key, false)));
            }
            if k == n || ends {
                queue.push(Reverse((finish(c, k), key, true)));
            }
        }
    };
    for &r in &w.routes_from[from.idx()] {
        search.absorb(r, 0, Cost { routes: 1, ..Cost::default() }, &mut outcomes);
        relax(&mut best, &mut queue, r, None, &mut outcomes);
    }
    while let Some(Reverse((c, key, finished))) = queue.pop() {
        let (r, k, _) = key;
        let Some((b, _)) = best[index(key)] else { continue };
        if c != if finished { finish(b, k) } else { b } {
            continue;
        }
        if finished {
            let mut out = Vec::new();
            let mut at = Some(key);
            while let Some(kk) = at {
                out.push((kk.0, kk.2));
                at = best[index(kk)].and_then(|(_, p)| p);
            }
            out.reverse();
            return Some(out);
        }
        if c.routes as usize >= MAX_ROUTE_DEPTH * (k + 1) {
            continue;
        }
        if let Exit::Signal(next) = w.routes[r.idx()].exit {
            for &r2 in &w.routes_from[next.idx()] {
                search.absorb(r2, k, Cost { routes: c.routes + 1, ..c }, &mut outcomes);
                relax(&mut best, &mut queue, r2, Some(key), &mut outcomes);
            }
        }
    }
    None
}

/// A planned journey: routes, each marked when it ends a leg.
type Journey = Vec<(RouteId, bool)>;

/// The robot's memo of `journey` results by (entrance, service, first call)
/// and of `shared_sections`. Both depend on the static world only, so the
/// cache never changes what the robot does, only how fast it decides; it is
/// not sim state and starts empty in every `Sim`.
#[derive(Default)]
pub struct PlanCache {
    journeys: Mutex<BTreeMap<(SignalId, ServiceId, usize), Option<Journey>>>,
    shared: OnceLock<Vec<bool>>,
}

impl std::fmt::Debug for PlanCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PlanCache")
    }
}

impl PlanCache {
    fn journey(&self, w: &World, t: &Train, from: SignalId, k0: usize) -> Option<Journey> {
        let key = (from, t.service, k0);
        // A poisoned lock only means a panic elsewhere; the map is still valid.
        let mut map = self.journeys.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(key).or_insert_with(|| journey(w, t, from, k0)).clone()
    }

    fn shared(&self, w: &World) -> &[bool] {
        self.shared.get_or_init(|| shared_sections(w))
    }
}

/// The first route from `entrance` towards the train's next goal: the
/// first route of its planned journey (see `journey`), which prefers each
/// call's booked platform, then any at that place.
pub fn choose_route(w: &World, t: &Train, entrance: SignalId) -> Option<RouteId> {
    let next = t.next_call + usize::from(t.dwell.is_some());
    journey(w, t, entrance, next)?.first().map(|&(r, _)| r)
}

/// Sections a train of `length_m` covers standing at the exit of the last
/// route of `chain`, walking back along the chain's paths.
fn footprint(w: &World, chain: &[RouteId], length_m: f64) -> Vec<SectionId> {
    let mut out = Vec::new();
    let mut left = length_m;
    for &r in chain.iter().rev() {
        for &s in w.routes[r.idx()].path.iter().rev() {
            if left <= 0.0 {
                return out;
            }
            out.push(s);
            left -= w.net.sections[s.idx()].segments.iter().map(|g| w.net.segments[g.idx()].length_m).sum::<f64>();
        }
    }
    out
}

/// The routes from `entrance` the train should have set together: up to its
/// next stopping call or its exit, or to the first signal before that where
/// it could stand without fouling track that routes from other signals use.
fn plan(sim: &Sim, t: &Train, entrance: SignalId) -> Option<Vec<RouteId>> {
    let w = sim.world();
    let cache = sim.plan_cache();
    let shared = cache.shared(w);
    let next = t.next_call + usize::from(t.dwell.is_some());
    let full = cache.journey(w, t, entrance, next)?;
    let mut chain = Vec::new();
    for (r, ends_leg) in full {
        chain.push(r);
        let clear = matches!(w.routes[r.idx()].exit, Exit::Signal(_))
            && footprint(w, &chain, t.length_m).iter().all(|s| !shared[s.idx()]);
        if ends_leg || clear {
            break;
        }
    }
    (!chain.is_empty()).then_some(chain)
}

/// Sections in the paths of routes from more than one signal.
fn shared_sections(w: &World) -> Vec<bool> {
    let mut first: Vec<Option<SignalId>> = vec![None; w.net.sections.len()];
    let mut shared = vec![false; w.net.sections.len()];
    for def in &w.routes {
        for &s in &def.path {
            match first[s.idx()] {
                None => first[s.idx()] = Some(def.entrance),
                Some(e) if e != def.entrance => shared[s.idx()] = true,
                Some(_) => {}
            }
        }
    }
    shared
}

/// Route requests for trains facing a red signal with no route set.
///
/// A train only gets routes when the whole chain to where it may next stand
/// (its next stopping call, or its exit) can be set at once: no other train
/// on any of it, and the interlocking would accept every route now. A train
/// is then never left standing on a junction waiting for track that another
/// waiting train holds, so waiting trains cannot lock each other out.
/// Automatic routes on the chain are always set and need nothing.
pub fn commands(sim: &Sim) -> Vec<Command> {
    let w = sim.world();
    let il = sim.interlocking();
    let mut out = Vec::new();
    let mut claimed: BTreeSet<SectionId> = BTreeSet::new();
    for t in sim.trains() {
        if t.stabled {
            continue;
        }
        // A train waiting for its booked departure does not need the road yet.
        if t.dwell.is_some_and(|d| d.depart_at_s - sim.now_s() > DEPARTURE_LEAD_S) {
            continue;
        }
        let (seg, dir) = t.head();
        let Some((entrance, _)) = w.net.first_signal_ahead(seg, dir, t.head_m, SIGNAL_SEARCH_M, sim.points()) else {
            continue;
        };
        if il.active_route_from(w, entrance).is_some() {
            continue;
        }
        let Some(chain) = plan(sim, t, entrance) else { continue };
        // No other train anywhere on the chain (automatic routes included: a
        // train ahead would take the routes set for this one), nothing another
        // train was given this round, and the interlocking accepts each route.
        // A route on the chain that is already set (e.g. left behind when an
        // earlier route of a chain was refused as it was applied) is used
        // as it is, not requested again.
        let free = |s: &SectionId| !claimed.contains(s) && sim.occupancy().trains_in(*s).iter().all(|&o| o == t.id);
        let is_set = |r: RouteId| il.active_route_from(w, w.routes[r.idx()].entrance) == Some(r);
        let settable = chain.iter().all(|&r| {
            let def = &w.routes[r.idx()];
            def.path.iter().chain(&def.overlap).all(free)
                && (def.automatic || is_set(r) || il.check_set_route(w, sim.points(), sim.occupancy(), r).is_ok())
        });
        if !settable {
            continue;
        }
        for r in chain {
            let def = &w.routes[r.idx()];
            if !def.automatic {
                claimed.extend(def.path.iter().chain(&def.overlap).copied());
                if !is_set(r) {
                    out.push(Command::SetRoute { entrance: def.entrance, exit: def.exit });
                }
            }
        }
    }
    out
}

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct SoakReport {
    pub spads: usize,
    pub collisions: usize,
    pub invariant_violations: usize,
    pub rejected: usize,
    pub entered: usize,
    pub exited: usize,
    pub stabled: usize,
    /// Headcodes of trains still running at the end.
    pub still_running: Vec<String>,
    pub waiting_to_enter: usize,
    pub penalties: i64,
    /// Headcodes of trains that had not moved for `STUCK_S` at the end.
    pub stuck: Vec<String>,
    /// Longest time any due entry waited at the fringe during the run.
    pub max_fringe_wait_s: f64,
    /// Timetabled entries booked at or before the end of the run.
    pub entries_due: usize,
    /// Of those, how many entered.
    pub entries_due_entered: usize,
    /// Seconds after its booked time (or the start, if later) each entering
    /// train entered, in order; negative is early.
    #[serde(skip)]
    pub entry_late_s: Vec<i64>,
    /// `late_s` of every arrival at a stopping call, in order.
    #[serde(skip)]
    pub arrival_late_s: Vec<i64>,
    pub wrong_platforms: usize,
}

/// Run `secs` seconds with the robot signalling, and report.
pub fn soak(sim: &mut Sim, secs: f64) -> SoakReport {
    soak_with(sim, secs, commands)
}

/// `soak` with `robot` standing in for `commands` (e.g. to time it).
pub fn soak_with(sim: &mut Sim, secs: f64, mut robot: impl FnMut(&Sim) -> Vec<Command>) -> SoakReport {
    let mut r = SoakReport::default();
    let end_s = sim.now_s() + secs;
    let mut entered: BTreeSet<usize> = BTreeSet::new();
    let ticks = (secs / TICK_S).round() as u64;
    // train → (head segment, head position, standing since)
    let mut still: BTreeMap<TrainId, (SegmentId, f64, f64)> = BTreeMap::new();
    for i in 0..ticks {
        if i % ROBOT_EVERY_TICKS == 0 {
            for c in robot(sim) {
                sim.submit(c);
            }
            let now = sim.now_s();
            for t in sim.trains() {
                let (seg, _) = t.head();
                match still.get(&t.id) {
                    Some(&(s, m, _)) if s == seg && m == t.head_m => {}
                    _ => {
                        still.insert(t.id, (seg, t.head_m, now));
                    }
                }
            }
            still.retain(|id, _| sim.trains().iter().any(|t| t.id == *id));
            r.max_fringe_wait_s = r.max_fringe_wait_s.max(sim.longest_fringe_wait_s());
        }
        let now = sim.now_s();
        for e in sim.step() {
            match e {
                Event::SignalPassedAtDanger { .. } => r.spads += 1,
                Event::Collision { .. } => r.collisions += 1,
                Event::InvariantViolated { .. } => r.invariant_violations += 1,
                Event::CommandRejected { .. } => r.rejected += 1,
                Event::TrainEntered { train, .. } => {
                    r.entered += 1;
                    let w = sim.world();
                    let svc = sim.trains().iter().find(|t| t.id == train).map(|t| t.service);
                    let entry = (0..w.entries.len())
                        .find(|&i| Some(w.entries[i].service) == svc && !entered.contains(&i));
                    if let Some(i) = entry {
                        entered.insert(i);
                        // Trains booked before the start are on time if they enter at once.
                        r.entry_late_s.push((now - w.entries[i].time_s.max(w.options.start_s)).round() as i64);
                    }
                }
                Event::TrainArrived { late_s, .. } => r.arrival_late_s.push(late_s),
                Event::WrongPlatform { .. } => r.wrong_platforms += 1,
                _ => {}
            }
        }
    }
    let now = sim.now_s();
    r.exited = sim.finished().iter().filter(|(_, o)| *o == Outcome::Exited).count();
    r.stabled = sim.finished().iter().filter(|(_, o)| *o == Outcome::Stabled).count();
    r.still_running = sim.trains().iter().filter(|t| !t.stabled).map(|t| t.headcode.clone()).collect();
    r.stuck = sim
        .trains()
        .iter()
        .filter(|t| !t.stabled && t.dwell.is_none())
        .filter(|t| still.get(&t.id).is_some_and(|&(_, _, since)| now - since >= STUCK_S))
        .map(|t| t.headcode.clone())
        .collect();
    r.waiting_to_enter = sim.entries_waiting();
    let w = sim.world();
    let due: Vec<usize> = (0..w.entries.len()).filter(|&i| !w.entries[i].on_demand && w.entries[i].time_s <= end_s).collect();
    r.entries_due = due.len();
    r.entries_due_entered = due.iter().filter(|i| entered.contains(i)).count();
    r.penalties = sim.scores().total();
    r
}
