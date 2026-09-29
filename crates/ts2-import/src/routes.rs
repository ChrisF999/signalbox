//! TS2 routes → signal-to-signal routes with overlaps.

use std::collections::{BTreeMap, BTreeSet};

use signalbox_core::ids::{NodeId, SectionId, SignalId};
use signalbox_core::network::{Network, PointsPos};
use signalbox_core::routes::Exit;
use signalbox_core::world::file::*;
use signalbox_core::world::{LoadError, World};

use crate::ConvertError;
use crate::graph::Graph;
use crate::report::{self, Report};
use crate::ts2::{Item, Port, Ts2};

/// Overlap target length beyond an exit signal.
const OVERLAP_M: f64 = 180.0;
const MAX_OVERLAP_SECTIONS: usize = 8;
/// How far the TS2 walk continues past the end signal (for overlap points).
const WALK_BEYOND_M: f64 = 500.0;
const MAX_STRETCHES: usize = 64;

#[derive(Clone, Debug)]
struct Stretch {
    entrance: SignalId,
    exit: Exit,
    path: Vec<SectionId>,
    /// Points this stretch's path crosses, sorted.
    points: Vec<(NodeId, PointsPos)>,
    /// Every points position the TS2 route implies (for the overlap).
    context: Vec<(NodeId, PointsPos)>,
    overlap: Vec<SectionId>,
    overlap_points: Vec<(NodeId, PointsPos)>,
    automatic: bool,
}

/// Points positions along a TS2 route: facing points from `directions`,
/// trailing points from the leg arrived on. Walks on `beyond_m` past `end`
/// and stops at facing points it has no direction for.
fn ts2_positions(
    ts2: &Ts2,
    begin: &str,
    end: Option<&str>,
    directions: &BTreeMap<String, u8>,
    beyond_m: f64,
) -> BTreeMap<String, PointsPos> {
    let items = &ts2.track_items;
    let mut out = BTreeMap::new();
    let Some(Item::SignalItem(s)) = items.get(begin) else { return out };
    let Some(first) = s.next_ti_id.clone() else { return out };
    let (mut prev, mut cur) = (begin.to_string(), first);
    let mut past_end: Option<f64> = None;
    for _ in 0..10_000 {
        if Some(prev.as_str()) == end {
            past_end.get_or_insert(0.0);
        }
        let Some(item) = items.get(&cur) else { break };
        let came = item.port_to(&prev);
        let next = match item {
            Item::PointsItem(p) => match came {
                Some(Port::Prev) => match directions.get(&cur) {
                    Some(0) => {
                        out.insert(cur.clone(), PointsPos::Normal);
                        p.next_ti_id.clone()
                    }
                    Some(1) => {
                        out.insert(cur.clone(), PointsPos::Reverse);
                        p.reverse_ti_id.clone()
                    }
                    _ => break,
                },
                Some(Port::Next) => {
                    out.insert(cur.clone(), PointsPos::Normal);
                    p.previous_ti_id.clone()
                }
                Some(Port::Rev) => {
                    out.insert(cur.clone(), PointsPos::Reverse);
                    p.previous_ti_id.clone()
                }
                None => break,
            },
            Item::EndItem(_) => break,
            other => match came {
                Some(Port::Prev) => other.link(Port::Next).map(str::to_string),
                Some(Port::Next) => other.link(Port::Prev).map(str::to_string),
                _ => None,
            },
        };
        if let Some(walked) = past_end.as_mut() {
            if let Item::LineItem(l) | Item::InvisibleLinkItem(l) = item {
                *walked += l.real_length;
            }
            if *walked > beyond_m {
                break;
            }
        }
        let Some(n) = next else { break };
        prev = std::mem::replace(&mut cur, n);
    }
    out
}

fn to_nodes(g: &Graph, net: &Network, pos: &BTreeMap<String, PointsPos>) -> Vec<(NodeId, PointsPos)> {
    let mut v: Vec<(NodeId, PointsPos)> = pos
        .iter()
        .filter_map(|(ti, &p)| g.points_nodes.get(ti).and_then(|n| net.node(n)).map(|n| (n, p)))
        .collect();
    v.sort_by_key(|&(n, _)| n);
    v
}

pub fn build(ts2: &Ts2, g: &Graph, world: &World, report: &mut Report) -> Vec<RouteFile> {
    let net = &world.net;
    let signal = |ti: &str| g.signal_names.get(ti).and_then(|n| net.signal(n));
    let mut stretches: Vec<Stretch> = Vec::new();

    for (id, r) in &ts2.routes {
        let Some(entrance) = signal(&r.begin_signal) else {
            report.warn(report::ROUTE_DROPPED, format!("route {id}: begin {} is not a main signal", r.begin_signal));
            continue;
        };
        let end = if let Some(s) = signal(&r.end_signal) {
            Some(s)
        } else if g.buffer_ends.contains_key(&r.end_signal) {
            None
        } else {
            report.warn(report::ROUTE_DROPPED, format!("route {id}: end {} is not a signal or buffer", r.end_signal));
            continue;
        };
        if r.initial_state == 1 {
            report.warn(report::ROUTE_PRESET, format!("route {id} was set once at the start in TS2; here it starts unset"));
        }
        let context = to_nodes(g, net, &ts2_positions(ts2, &r.begin_signal, Some(&r.end_signal), &r.directions, WALK_BEYOND_M));
        let mut cur = entrance;
        for _ in 0..MAX_STRETCHES {
            match net.trace_route(cur, &context, &[], 0) {
                Err(e) => {
                    report.warn(report::ROUTE_DROPPED, format!("route {id} from {}: {e}", net.signals[cur.idx()].name));
                    break;
                }
                Ok(t) => {
                    let points: Vec<_> = context.iter().copied().filter(|(n, _)| t.points_crossed.contains(n)).collect();
                    stretches.push(Stretch {
                        entrance: cur,
                        exit: t.exit,
                        path: t.path,
                        points,
                        context: context.clone(),
                        overlap: vec![],
                        overlap_points: vec![],
                        automatic: r.initial_state == 2,
                    });
                    match t.exit {
                        Exit::Signal(next) if Some(next) != end => cur = next,
                        Exit::Signal(_) => break,
                        Exit::Node(_) => {
                            if end.is_some() {
                                report.warn(report::ROUTE_SHORT, format!("route {id} reached the end of the track before its end signal"));
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    // Merge identical stretches from different TS2 routes.
    let mut merged: Vec<Stretch> = Vec::new();
    for s in stretches {
        match merged.iter_mut().find(|m| m.entrance == s.entrance && m.exit == s.exit) {
            Some(m) if m.points == s.points => m.automatic |= s.automatic,
            Some(_) => report.warn(
                report::ROUTE_DROPPED,
                format!("a second route from {} to the same exit needs other points; dropped", net.signals[s.entrance.idx()].name),
            ),
            None => merged.push(s),
        }
    }

    // Signals still without a route: plain-line routes to the next signal or end.
    for i in 0..net.signals.len() {
        let s = SignalId::from_idx(i);
        if merged.iter().any(|m| m.entrance == s) {
            continue;
        }
        let name = &net.signals[i].name;
        let context = g
            .signal_ti
            .get(name)
            .map(|ti| to_nodes(g, net, &ts2_positions(ts2, ti, None, &BTreeMap::new(), 0.0)))
            .unwrap_or_default();
        match net.trace_route(s, &context, &[], 0) {
            Ok(t) => {
                report.warn(report::ROUTE_GENERATED, format!("signal {name} begins no TS2 route; generated one"));
                let points = context.iter().copied().filter(|(n, _)| t.points_crossed.contains(n)).collect();
                merged.push(Stretch {
                    entrance: s,
                    exit: t.exit,
                    path: t.path,
                    points,
                    context,
                    overlap: vec![],
                    overlap_points: vec![],
                    automatic: false,
                });
            }
            Err(e) => report.warn(report::SIGNAL_NO_ROUTE, format!("signal {name} begins no route: {e}")),
        }
    }

    let section_len: Vec<f64> = net
        .sections
        .iter()
        .map(|sec| sec.segments.iter().map(|s| net.segments[s.idx()].length_m).sum())
        .collect();
    let mut points_in: BTreeMap<SectionId, Vec<NodeId>> = BTreeMap::new();
    for i in 0..net.nodes.len() {
        let n = NodeId::from_idx(i);
        if let Some(sec) = net.points_section(n) {
            points_in.entry(sec).or_default().push(n);
        }
    }
    for m in &mut merged {
        let (o, op) = overlap(net, m, &section_len, &points_in, report);
        m.overlap = o;
        m.overlap_points = op;
    }
    demote_clashing_automatics(&mut merged, net, report);
    drop_overlap_before_controlled_signals(&mut merged, net, report);

    let sec = |s: &SectionId| net.sections[s.idx()].name.clone();
    let req = |v: &[(NodeId, PointsPos)]| -> Vec<PointsReqFile> {
        v.iter().map(|&(n, p)| PointsReqFile { points: net.nodes[n.idx()].name.clone(), position: p }).collect()
    };
    merged
        .iter()
        .map(|m| RouteFile {
            entrance: net.signals[m.entrance.idx()].name.clone(),
            exit: match m.exit {
                Exit::Signal(s) => ExitFile::Signal(net.signals[s.idx()].name.clone()),
                Exit::Node(n) => ExitFile::Node(net.nodes[n.idx()].name.clone()),
            },
            path: m.path.iter().map(sec).collect(),
            points: req(&m.points),
            overlap: m.overlap.iter().map(sec).collect(),
            overlap_points: req(&m.overlap_points),
            automatic: m.automatic,
        })
        .collect()
}

/// Sections beyond a signal exit up to `OVERLAP_M`, using the TS2 route's
/// positions beyond the signal; stops before points it has no position for.
fn overlap(
    net: &Network,
    m: &Stretch,
    section_len: &[f64],
    points_in: &BTreeMap<SectionId, Vec<NodeId>>,
    report: &mut Report,
) -> (Vec<SectionId>, Vec<(NodeId, PointsPos)>) {
    if matches!(m.exit, Exit::Node(_)) {
        return (vec![], vec![]);
    }
    let path_points: BTreeSet<NodeId> = m.points.iter().map(|&(n, _)| n).collect();
    let mut best = (vec![], vec![]);
    for n in 1..=MAX_OVERLAP_SECTIONS {
        match net.trace_route(m.entrance, &m.points, &m.context, n) {
            Ok(t) => {
                let short = t.overlap.len() < n;
                let length: f64 = t.overlap.iter().map(|s| section_len[s.idx()]).sum();
                let pts: Vec<(NodeId, PointsPos)> = m
                    .context
                    .iter()
                    .copied()
                    .filter(|(p, _)| t.points_crossed.contains(p) && !path_points.contains(p))
                    .collect();
                // The core requires every points node inside an overlap section to be
                // listed. Facing points the walk stopped at (nothing sets them) are
                // in their section but unlisted: the overlap stops before that section.
                let cut = t.overlap.iter().position(|s| {
                    points_in.get(s).is_some_and(|v| v.iter().any(|p| !pts.iter().any(|(n, _)| n == p)))
                });
                if let Some(k) = cut {
                    report.warn(
                        report::OVERLAP_CUT,
                        format!(
                            "route from {}: overlap stops before section {} (points inside it are not set by the route)",
                            net.signals[m.entrance.idx()].name,
                            net.sections[t.overlap[k].idx()].name
                        ),
                    );
                    best = match net.trace_route(m.entrance, &m.points, &m.context, k) {
                        Ok(t2) if k > 0 => {
                            let pts2 = m
                                .context
                                .iter()
                                .copied()
                                .filter(|(p, _)| t2.points_crossed.contains(p) && !path_points.contains(p))
                                .collect();
                            (t2.overlap, pts2)
                        }
                        _ => (vec![], vec![]),
                    };
                    break;
                }
                best = (t.overlap, pts);
                if short || length >= OVERLAP_M {
                    break;
                }
            }
            Err(_) => {
                if n == 1 {
                    report.warn(
                        report::OVERLAP_CUT,
                        format!("route from {}: no overlap (points beyond the exit are not set by the route)", net.signals[m.entrance.idx()].name),
                    );
                }
                break;
            }
        }
    }
    best
}

/// An automatic route runs into a controlled signal (the entrance of a
/// non-automatic route): its overlap would hold that signal's throat for ever,
/// so it gets none.
fn drop_overlap_before_controlled_signals(v: &mut [Stretch], net: &Network, report: &mut Report) {
    let controlled: BTreeSet<SignalId> = v.iter().filter(|m| !m.automatic).map(|m| m.entrance).collect();
    for m in v.iter_mut() {
        if m.automatic && matches!(m.exit, Exit::Signal(s) if controlled.contains(&s)) && !m.overlap.is_empty() {
            m.overlap.clear();
            m.overlap_points.clear();
            report.warn(
                report::OVERLAP_CUT,
                format!("route from {}: automatic route before a controlled signal: no overlap", net.signals[m.entrance.idx()].name),
            );
        }
    }
}

/// An automatic route must be the only route from its signal: it is never
/// cancelled, so the signal's other routes could never be set. Two automatic
/// routes may only share sections when one continues the other and the shared
/// sections are the first one's overlap.
fn demote_clashing_automatics(v: &mut [Stretch], net: &Network, report: &mut Report) {
    for j in 0..v.len() {
        if v[j].automatic && v.iter().filter(|m| m.entrance == v[j].entrance).count() > 1 {
            v[j].automatic = false;
            report.warn(
                report::AUTOMATIC_DEMOTED,
                format!("route from {} shares its signal with other routes; set by hand instead", net.signals[v[j].entrance.idx()].name),
            );
        }
    }
    for j in 0..v.len() {
        if !v[j].automatic {
            continue;
        }
        for i in 0..j {
            if !v[i].automatic {
                continue;
            }
            let secs = |s: &Stretch| -> BTreeSet<SectionId> { s.path.iter().chain(s.overlap.iter()).copied().collect() };
            let shared: Vec<SectionId> = secs(&v[i]).intersection(&secs(&v[j])).copied().collect();
            if shared.is_empty() {
                continue;
            }
            let continues = |a: &Stretch, b: &Stretch| {
                a.exit == Exit::Signal(b.entrance) && shared.iter().all(|s| a.overlap.contains(s) && b.path.contains(s))
            };
            if continues(&v[i], &v[j]) || continues(&v[j], &v[i]) {
                continue;
            }
            v[j].automatic = false;
            report.warn(
                report::AUTOMATIC_DEMOTED,
                format!("route from {} clashes with another automatic route; set by hand instead", net.signals[v[j].entrance.idx()].name),
            );
            break;
        }
    }
}

fn route_name(r: &RouteFile) -> String {
    let exit = match &r.exit {
        ExitFile::Signal(n) | ExitFile::Node(n) => n,
    };
    format!("{}-{}", r.entrance, exit)
}

/// Load the world; drop any route the validator rejects (with a warning) and
/// retry. Any other load error is a converter bug.
pub fn finish(mut file: WorldFile, report: &mut Report) -> Result<WorldFile, ConvertError> {
    for _ in 0..=file.routes.len() {
        match World::from_file(file.clone()) {
            Ok(_) => return Ok(file),
            Err(LoadError::BadRoute { route, problem }) => {
                let before = file.routes.len();
                file.routes.retain(|r| route_name(r) != route);
                if file.routes.len() == before {
                    return Err(ConvertError::Invalid(LoadError::BadRoute { route, problem }));
                }
                report.warn(report::ROUTE_DROPPED, format!("route {route}: {problem}"));
            }
            Err(e) => return Err(ConvertError::Invalid(e)),
        }
    }
    Err(ConvertError::Graph("route validation did not settle".into()))
}
