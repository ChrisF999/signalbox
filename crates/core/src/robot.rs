//! A robot signaller: sets the next route for each train towards its next
//! call (or an exit), and a soak runner built on it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

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
/// The robot looks at the railway once per this many ticks.
const ROBOT_EVERY_TICKS: u64 = 10;

enum Goal<'a> {
    Platform { place: &'a str, platform: Option<&'a str> },
    Exit,
}

fn goal<'a>(w: &'a World, t: &Train) -> Option<Goal<'a>> {
    let svc = &w.services[t.service.idx()];
    match svc.calls.get(t.next_call) {
        Some(c) => Some(Goal::Platform { place: &c.place, platform: c.platform.as_deref() }),
        None if svc.end == EndAction::Exit => Some(Goal::Exit),
        None => None,
    }
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

/// The first route from `entrance` on the shortest chain of routes to the
/// train's next goal. Prefers the booked platform, then any at that place.
pub fn choose_route(w: &World, t: &Train, entrance: SignalId) -> Option<RouteId> {
    let goal = goal(w, t)?;
    for strict in [true, false] {
        let mut queue: VecDeque<(RouteId, RouteId, usize)> =
            w.routes_from[entrance.idx()].iter().map(|&r| (r, r, 0)).collect();
        let mut seen = BTreeSet::new();
        while let Some((r, first, depth)) = queue.pop_front() {
            if !seen.insert(r) {
                continue;
            }
            let def = &w.routes[r.idx()];
            if reaches(w, def, &goal, strict) {
                return Some(first);
            }
            if depth < MAX_ROUTE_DEPTH {
                if let Exit::Signal(next) = def.exit {
                    queue.extend(w.routes_from[next.idx()].iter().map(|&r2| (r2, first, depth + 1)));
                }
            }
        }
    }
    None
}

/// Route requests for every train facing a red signal with no route set.
/// A route is only requested when no other train occupies its path.
pub fn commands(sim: &Sim) -> Vec<Command> {
    let w = sim.world();
    let mut out = Vec::new();
    let mut claimed: Vec<SectionId> = Vec::new();
    for t in sim.trains() {
        if t.stabled {
            continue;
        }
        let (seg, dir) = t.head();
        let Some((entrance, _)) = w.net.first_signal_ahead(seg, dir, t.head_m, SIGNAL_SEARCH_M, sim.points()) else {
            continue;
        };
        if sim.interlocking().active_route_from(w, entrance).is_some() {
            continue;
        }
        let Some(r) = choose_route(w, t, entrance) else { continue };
        let def = &w.routes[r.idx()];
        let blocked = def.path.iter().any(|&s| {
            claimed.contains(&s) || sim.occupancy().trains_in(s).iter().any(|&o| o != t.id)
        });
        if blocked {
            continue;
        }
        claimed.extend(def.path.iter().copied());
        out.push(Command::SetRoute { entrance, exit: def.exit });
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
}

/// Run `secs` seconds with the robot signalling, and report.
pub fn soak(sim: &mut Sim, secs: f64) -> SoakReport {
    let mut r = SoakReport::default();
    let ticks = (secs / TICK_S).round() as u64;
    // train → (head segment, head position, standing since)
    let mut still: BTreeMap<TrainId, (SegmentId, f64, f64)> = BTreeMap::new();
    for i in 0..ticks {
        if i % ROBOT_EVERY_TICKS == 0 {
            for c in commands(sim) {
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
        for e in sim.step() {
            match e {
                Event::SignalPassedAtDanger { .. } => r.spads += 1,
                Event::Collision { .. } => r.collisions += 1,
                Event::InvariantViolated { .. } => r.invariant_violations += 1,
                Event::CommandRejected { .. } => r.rejected += 1,
                Event::TrainEntered { .. } => r.entered += 1,
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
    r.penalties = sim.scores().total();
    r
}
