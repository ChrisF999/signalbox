//! A robot signaller: sets routes for each train towards its next calls (or
//! an exit), and a soak runner built on it.

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
    // While dwelling, `next_call` is still the call the train stands at.
    let next = t.next_call + usize::from(t.dwell.is_some());
    match svc.calls.get(next) {
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

/// The shortest chain of routes from `entrance` to the first route that
/// reaches `goal`.
fn chain_to(w: &World, entrance: SignalId, goal: &Goal, strict: bool) -> Option<Vec<RouteId>> {
    // route → the route before it on the chain (None for the first).
    let mut parent: BTreeMap<RouteId, Option<RouteId>> = BTreeMap::new();
    let mut queue: VecDeque<(RouteId, usize)> = VecDeque::new();
    for &r in &w.routes_from[entrance.idx()] {
        parent.insert(r, None);
        queue.push_back((r, 0));
    }
    while let Some((r, depth)) = queue.pop_front() {
        let def = &w.routes[r.idx()];
        if reaches(w, def, goal, strict) {
            let mut chain = vec![r];
            while let Some(&Some(p)) = parent.get(chain.last().expect("chain is never empty")) {
                chain.push(p);
            }
            chain.reverse();
            return Some(chain);
        }
        if depth < MAX_ROUTE_DEPTH {
            if let Exit::Signal(next) = def.exit {
                for &r2 in &w.routes_from[next.idx()] {
                    if !parent.contains_key(&r2) {
                        parent.insert(r2, Some(r));
                        queue.push_back((r2, depth + 1));
                    }
                }
            }
        }
    }
    None
}

/// The first route from `entrance` on the shortest chain of routes to the
/// train's next goal. Prefers the booked platform, then any at that place.
pub fn choose_route(w: &World, t: &Train, entrance: SignalId) -> Option<RouteId> {
    let goal = goal(w, t)?;
    [true, false].into_iter().find_map(|strict| chain_to(w, entrance, &goal, strict)).map(|c| c[0])
}

/// The chain of routes from `from` through calls `k..` of the service (and
/// out, for an exit service), each route marked when it ends the leg to a
/// stopping call or the exit. Each call's booked platform is preferred when
/// the rest of the journey can still be made from it. The flag returned says
/// the route before `from` (`last`) already ends a leg.
fn itinerary(w: &World, t: &Train, from: SignalId, k: usize, last: Option<RouteId>) -> Option<(Vec<(RouteId, bool)>, bool)> {
    let svc = &w.services[t.service.idx()];
    let (goal, stop) = match svc.calls.get(k) {
        Some(c) => (Goal::Platform { place: &c.place, platform: c.platform.as_deref() }, c.stop),
        None if svc.end == EndAction::Exit => (Goal::Exit, true),
        None => return Some((vec![], false)),
    };
    for strict in [true, false] {
        let part: Vec<RouteId> = match last {
            Some(r) if reaches(w, &w.routes[r.idx()], &goal, strict) => vec![],
            _ => match chain_to(w, from, &goal, strict) {
                Some(c) => c,
                None => continue,
            },
        };
        let mut out: Vec<(RouteId, bool)> = part.iter().map(|&r| (r, false)).collect();
        if stop {
            return Some(match out.last_mut() {
                Some(l) => {
                    l.1 = true;
                    (out, false)
                }
                None => (out, true),
            });
        }
        let end = part.last().copied().or(last);
        let rest = match end.map(|r| w.routes[r.idx()].exit) {
            Some(Exit::Signal(s)) => itinerary(w, t, s, k + 1, end),
            _ => Some((vec![], false)),
        };
        if let Some((rest, ends_prev)) = rest {
            match out.last_mut() {
                Some(l) => l.1 |= ends_prev,
                None if ends_prev => return Some((rest, true)),
                None => {}
            }
            out.extend(rest);
            return Some((out, false));
        }
    }
    None
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
fn plan(w: &World, t: &Train, entrance: SignalId, shared: &[bool]) -> Option<Vec<RouteId>> {
    let next = t.next_call + usize::from(t.dwell.is_some());
    let (full, _) = itinerary(w, t, entrance, next, None)?;
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
    let shared = shared_sections(w);
    for t in sim.trains() {
        if t.stabled {
            continue;
        }
        let (seg, dir) = t.head();
        let Some((entrance, _)) = w.net.first_signal_ahead(seg, dir, t.head_m, SIGNAL_SEARCH_M, sim.points()) else {
            continue;
        };
        if il.active_route_from(w, entrance).is_some() {
            continue;
        }
        let Some(chain) = plan(w, t, entrance, &shared) else { continue };
        // No other train anywhere on the chain (automatic routes included: a
        // train ahead would take the routes set for this one), nothing another
        // train was given this round, and the interlocking accepts each route.
        let free = |s: &SectionId| !claimed.contains(s) && sim.occupancy().trains_in(*s).iter().all(|&o| o == t.id);
        let settable = chain.iter().all(|&r| {
            let def = &w.routes[r.idx()];
            def.path.iter().chain(&def.overlap).all(free)
                && (def.automatic || il.check_set_route(w, sim.points(), sim.occupancy(), r).is_ok())
        });
        if !settable {
            continue;
        }
        for r in chain {
            let def = &w.routes[r.idx()];
            if !def.automatic {
                claimed.extend(def.path.iter().chain(&def.overlap).copied());
                out.push(Command::SetRoute { entrance: def.entrance, exit: def.exit });
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

