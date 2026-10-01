//! The diagram as shapes in layout coordinates, built once per layout from
//! its geometry and its lists. What the geometry lacks is left out; what
//! has no geometry at all gives no scene.

use std::collections::{BTreeMap, BTreeSet};

use client_core::names::shown_headcode;
use client_core::select;
use egui::{Pos2, Rect, Vec2, pos2, vec2};
use protocol::{ExitName, Layout};

#[derive(Clone, Debug, PartialEq)]
pub struct TrackLine {
    pub segment: String,
    pub section: String,
    pub a: Pos2,
    pub b: Pos2,
    pub fringe: bool,
    /// Sections of the other visible segments meeting this line at `a`
    /// (its segment's `from` node) and at `b`, sorted, without repeats.
    pub a_meets: Vec<String>,
    pub b_meets: Vec<String>,
}

impl TrackLine {
    /// A track-circuit joint at `a`: another section meets the line there.
    pub fn joint_a(&self) -> bool {
        self.a_meets.iter().any(|s| *s != self.section)
    }

    pub fn joint_b(&self) -> bool {
        self.b_meets.iter().any(|s| *s != self.section)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointsMark {
    pub name: String,
    pub section: String,
    pub at: Pos2,
    pub toe: Option<Pos2>,
    pub normal: Option<Pos2>,
    pub reverse: Option<Pos2>,
    /// Sections of the drawn lines that start where each leg ends, sorted,
    /// without repeats (the layout names no leg's far node, so they are
    /// matched by position).
    pub toe_meets: Vec<String>,
    pub normal_meets: Vec<String>,
    pub reverse_meets: Vec<String>,
    pub fringe: bool,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignalMark {
    pub name: String,
    pub at: Pos2,
    /// Where its post leaves the track: `at` moved onto its own segment's
    /// drawn line (TS2 signals are on it already), else `at`.
    pub base: Pos2,
    /// Every route from it you can see (its post is white while one is set).
    pub routes: Vec<String>,
    /// Unit direction of travel past the signal, or zero when unknown.
    pub facing: Vec2,
    pub fringe: bool,
    pub operable: bool,
    /// Automatic routes starting here: a permanently automatic signal, drawn
    /// with a dashed post.
    pub auto_routes: Vec<String>,
    /// Has a ○A button beside it: a controlled signal of yours (or, for a
    /// spectator, any) that starts a route.
    pub auto_button: bool,
    /// Ends a route you can set, so it takes the click that sets it even on the fringe.
    pub route_exit: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BerthMark {
    pub name: String,
    pub at: Pos2,
    /// Drawn this far from `at` on screen: behind its signal along the
    /// track, or inside the track from its boundary.
    pub offset_px: Vec2,
    pub fringe: bool,
    pub operable: bool,
    /// Its box's width on screen: the layout's longest headcode fits.
    pub width_px: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExitMark {
    pub node: String,
    pub at: Pos2,
    /// On no section of your own (dimmed).
    pub fringe: bool,
    /// Ends a route you can set: clickable.
    pub route_exit: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlatformMark {
    /// The station's code, as the timetable names it (`HXC`).
    pub place: String,
    pub rect: Rect,
    /// The platform number, drawn inside the block.
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LabelMark {
    /// In capitals, as IECC labels are.
    pub text: String,
    pub at: Pos2,
    /// A line name's direction of travel (unit), drawn as an arrow at `at`.
    pub arrow: Option<Vec2>,
}

/// A stretch of drawn track through plain joints (nodes where exactly two
/// visible segments meet, both drawn), for the direction arrows.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// Line ends in running order.
    pub points: Vec<Pos2>,
    /// Signals on it face along `points` / against it.
    pub forward: bool,
    pub backward: bool,
    /// Its first / last end is where your visible track stops.
    pub loose_start: bool,
    pub loose_end: bool,
}

/// Where a boundary berth is drawn when no track ends at its node.
pub const BOUNDARY_BERTH_OFFSET_PX: Vec2 = vec2(0.0, -18.0);
/// How far (pixels) behind its signal, or inside its boundary, a berth sits.
pub const BERTH_BACK_PX: f32 = 24.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub tracks: Vec<TrackLine>,
    pub points: Vec<PointsMark>,
    pub signals: Vec<SignalMark>,
    pub berths: Vec<BerthMark>,
    pub exits: Vec<ExitMark>,
    pub platforms: Vec<PlatformMark>,
    pub labels: Vec<LabelMark>,
    pub runs: Vec<Run>,
    /// Bounds of your own area's drawing (`None` for a spectator).
    pub own: Option<Rect>,
    /// Bounds of everything drawn.
    pub all: Option<Rect>,
}

/// Coordinates beyond this are nonsense and left out, so bounds, centres
/// and fits stay finite.
pub const MAX_COORD: f64 = 1.0e7;

/// How near (layout units) a drawn line's end must be to a points leg's
/// end to meet it.
const LEG_MATCH: f32 = 1.0e-3;

fn pt(x: f64, y: f64) -> Option<Pos2> {
    (x.abs() <= MAX_COORD && y.abs() <= MAX_COORD).then(|| pos2(x as f32, y as f32))
}

/// The point of segment a–b nearest to `p`.
pub fn project(p: Pos2, a: Pos2, b: Pos2) -> Pos2 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 == 0.0 {
        return a;
    }
    a + ab * ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
}

fn grow(r: &mut Option<Rect>, p: Pos2) {
    *r = Some(match r {
        Some(r) => r.union(Rect::from_min_max(p, p)),
        None => Rect::from_min_max(p, p),
    });
}

impl Scene {
    /// `None` when the layout carries no geometry.
    pub fn build(l: &Layout) -> Option<Scene> {
        let g = l.geometry.as_ref()?;
        let fringe_of: BTreeMap<&str, bool> = l.sections.iter().map(|s| (s.name.as_str(), s.fringe)).collect();
        let seg_of: BTreeMap<&str, (&str, &str, &str)> =
            l.segments.iter().map(|s| (s.name.as_str(), (s.section.as_str(), s.from.as_str(), s.to.as_str()))).collect();
        let other_area = |area: &str| l.area.as_deref().is_some_and(|mine| mine != area);
        let exit_of_yours = |exit: &ExitName| l.routes.iter().any(|r| r.operable && &r.exit == exit);
        // Node → (segment, section) of every visible segment meeting there.
        let mut at_node: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
        for s in &l.segments {
            for n in [s.from.as_str(), s.to.as_str()] {
                at_node.entry(n).or_default().push((s.name.as_str(), s.section.as_str()));
            }
        }
        let meets = |node: &str, segment: &str| -> Vec<String> {
            let set: BTreeSet<&str> =
                at_node.get(node).into_iter().flatten().filter(|(g, _)| *g != segment).map(|(_, s)| *s).collect();
            set.into_iter().map(str::to_string).collect()
        };
        let mut sc = Scene::default();
        // Every berth box fits the longest headcode the layout books, as it is
        // displayed (spec P18: the WTT's `202/163` shows as `202`).
        let chars = l.simplifier.iter().map(|r| shown_headcode(&l.display_headcodes, &r.headcode).chars().count()).max().unwrap_or(0).max(4);
        let berth_w = crate::hit::berth_width(chars);
        for line in &g.lines {
            let (Some(a), Some(b), Some(&(section, from, to))) = (pt(line.x1, line.y1), pt(line.x2, line.y2), seg_of.get(line.segment.as_str()))
            else {
                continue;
            };
            sc.tracks.push(TrackLine {
                segment: line.segment.clone(),
                section: section.to_string(),
                a,
                b,
                fringe: fringe_of.get(section).copied().unwrap_or(true),
                a_meets: meets(from, &line.segment),
                b_meets: meets(to, &line.segment),
            });
        }
        let line_of: BTreeMap<&str, (Pos2, Pos2)> = sc.tracks.iter().map(|t| (t.segment.as_str(), (t.a, t.b))).collect();
        let ending_at = |q: Option<Pos2>| -> Vec<String> {
            let Some(q) = q else { return Vec::new() };
            let set: BTreeSet<&str> = sc
                .tracks
                .iter()
                .filter(|t| t.a.distance(q) <= LEG_MATCH || t.b.distance(q) <= LEG_MATCH)
                .map(|t| t.section.as_str())
                .collect();
            set.into_iter().map(str::to_string).collect()
        };
        for p in &g.points {
            let (Some(at), Some(info)) = (pt(p.x, p.y), l.points.iter().find(|i| i.name == p.node)) else { continue };
            let leg = |v: Option<[f64; 2]>| v.and_then(|[x, y]| pt(x, y));
            let (toe, normal, reverse) = (leg(p.toe), leg(p.normal), leg(p.reverse));
            sc.points.push(PointsMark {
                name: p.node.clone(),
                section: info.section.clone(),
                at,
                toe,
                normal,
                reverse,
                toe_meets: ending_at(toe),
                normal_meets: ending_at(normal),
                reverse_meets: ending_at(reverse),
                fringe: fringe_of.get(info.section.as_str()).copied().unwrap_or(true),
                operable: info.operable,
            });
        }
        for s in &g.signals {
            let (Some(at), Some(info)) = (pt(s.x, s.y), l.signals.iter().find(|i| i.name == s.signal)) else { continue };
            let facing = s.facing.map(|[x, y]| vec2(x as f32, y as f32)).filter(|v| v.length() > 0.0 && v.is_finite());
            let facing = facing.map_or(Vec2::ZERO, Vec2::normalized);
            let base = line_of.get(info.segment.as_str()).map_or(at, |&(a, b)| project(at, a, b));
            sc.signals.push(SignalMark {
                name: s.signal.clone(),
                at,
                base,
                routes: l.routes.iter().filter(|r| r.entrance == s.signal).map(|r| r.name.clone()).collect(),
                facing,
                fringe: other_area(&info.area),
                operable: info.operable,
                auto_routes: l.routes.iter().filter(|r| r.automatic && r.entrance == s.signal).map(|r| r.name.clone()).collect(),
                auto_button: select::has_auto_button(l, &s.signal),
                route_exit: exit_of_yours(&ExitName::Signal(s.signal.clone())),
            });
            for b in l.berths.iter().filter(|b| b.signal.as_deref() == Some(s.signal.as_str())) {
                // In the track on the approach side of its signal; where the
                // facing is unknown, at TS2's own berth position.
                let (bat, offset_px) = if facing == Vec2::ZERO {
                    (pt(s.berth_x, s.berth_y), Vec2::ZERO)
                } else {
                    (Some(base), -facing * BERTH_BACK_PX)
                };
                if let Some(bat) = bat {
                    sc.berths.push(BerthMark {
                        name: b.name.clone(),
                        at: bat,
                        offset_px,
                        fringe: other_area(&b.area),
                        operable: b.operable,
                        width_px: berth_w,
                    });
                }
            }
        }
        let node_at: BTreeMap<&str, Pos2> =
            g.nodes.iter().filter_map(|n| Some((n.node.as_str(), pt(n.x, n.y)?))).collect();
        let exit_nodes: BTreeSet<&str> = l
            .routes
            .iter()
            .filter_map(|r| match &r.exit {
                ExitName::Node(n) => Some(n.as_str()),
                ExitName::Signal(_) => None,
            })
            .collect();
        for n in &exit_nodes {
            if let Some(&at) = node_at.get(n) {
                // Fringe unless one of your own sections reaches the node
                // (nothing is fringe to a spectator).
                let own = l.area.is_none() || l.segments.iter().any(|s| {
                    (s.from == *n || s.to == *n) && fringe_of.get(s.section.as_str()) == Some(&false)
                });
                sc.exits.push(ExitMark {
                    node: n.to_string(),
                    at,
                    fringe: !own,
                    route_exit: exit_of_yours(&ExitName::Node(n.to_string())),
                });
            }
        }
        // Into the track that ends at a point, if one does.
        let inward = |p: Pos2| {
            sc.tracks.iter().find_map(|t| {
                let d = if t.a.distance(p) < 0.5 {
                    t.b - t.a
                } else if t.b.distance(p) < 0.5 {
                    t.a - t.b
                } else {
                    return None;
                };
                (d.length() > 0.0).then(|| d.normalized())
            })
        };
        let mut boundary_berths = Vec::new();
        for b in &l.berths {
            if let Some(&at) = b.boundary.as_deref().and_then(|n| node_at.get(n)) {
                boundary_berths.push(BerthMark {
                    name: b.name.clone(),
                    at,
                    offset_px: inward(at).map_or(BOUNDARY_BERTH_OFFSET_PX, |d| d * BERTH_BACK_PX),
                    fringe: other_area(&b.area),
                    operable: b.operable,
                    width_px: berth_w,
                });
            }
        }
        sc.berths.extend(boundary_berths);
        for p in &g.platforms {
            if let (Some(a), Some(b)) = (pt(p.x1, p.y1), pt(p.x2, p.y2)) {
                sc.platforms.push(PlatformMark { place: p.place.clone(), rect: Rect::from_two_pos(a, b), label: p.platform.clone() });
            }
        }
        for t in &g.labels {
            if let Some(at) = pt(t.x, t.y) {
                let arrow = t.arrow.map(|[x, y]| vec2(x as f32, y as f32)).filter(|v| v.is_finite() && v.length() > 0.0);
                sc.labels.push(LabelMark { text: t.text.to_uppercase(), at, arrow: arrow.map(Vec2::normalized) });
            }
        }
        let signal_segment: BTreeMap<&str, &str> = l.signals.iter().map(|s| (s.name.as_str(), s.segment.as_str())).collect();
        let ends: Vec<Pos2> = sc.exits.iter().map(|e| e.at).collect();
        sc.runs = runs(&sc.tracks, &at_node, &seg_of, &sc.signals, &signal_segment, &ends);
        for t in &sc.tracks {
            grow(&mut sc.all, t.a);
            grow(&mut sc.all, t.b);
            if !t.fringe && l.area.is_some() {
                grow(&mut sc.own, t.a);
                grow(&mut sc.own, t.b);
            }
        }
        for s in &sc.signals {
            grow(&mut sc.all, s.at);
            if !s.fringe && l.area.is_some() {
                grow(&mut sc.own, s.at);
            }
        }
        for p in &sc.points {
            grow(&mut sc.all, p.at);
        }
        Some(sc)
    }

    /// What "Fit" frames: your own area, or everything.
    pub fn fit_bounds(&self) -> Option<Rect> {
        self.own.or(self.all)
    }
}

/// Chain drawn lines into runs through plain joints, and give each run the
/// directions its signals face. A run's end is loose where your visible
/// track stops: no other segment meets it, or it is a route's exit (a
/// buffer stop or boundary, often behind an undrawn spacer in TS2 data).
fn runs(
    tracks: &[TrackLine],
    at_node: &BTreeMap<&str, Vec<(&str, &str)>>,
    seg_of: &BTreeMap<&str, (&str, &str, &str)>,
    signals: &[SignalMark],
    signal_segment: &BTreeMap<&str, &str>,
    exits: &[Pos2],
) -> Vec<Run> {
    let index: BTreeMap<&str, usize> = tracks.iter().enumerate().map(|(i, t)| (t.segment.as_str(), i)).collect();
    // (from, to) node of each drawn line's segment (every drawn line has one).
    let ends: Vec<(&str, &str)> = tracks.iter().map(|t| seg_of.get(t.segment.as_str()).map_or(("", ""), |e| (e.1, e.2))).collect();
    // The drawn line continuing line `i` through `node`, if the node is a plain joint.
    let next = |node: &str, i: usize| -> Option<usize> {
        let segs = at_node.get(node)?;
        if segs.len() != 2 {
            return None;
        }
        let other = segs.iter().find(|(g, _)| *g != tracks[i].segment)?;
        index.get(other.0).copied()
    };
    let loose = |node: &str| at_node.get(node).is_none_or(|s| s.len() <= 1);
    let mut seen = vec![false; tracks.len()];
    let mut out = Vec::new();
    for start in 0..tracks.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        // (line, reversed): a reversed line is walked from its `to` node.
        let mut chain = std::collections::VecDeque::from([(start, false)]);
        loop {
            let &(i, rev) = chain.back().expect("never empty");
            let exit = if rev { ends[i].0 } else { ends[i].1 };
            match next(exit, i).filter(|&k| !seen[k]) {
                Some(k) => {
                    seen[k] = true;
                    chain.push_back((k, ends[k].0 != exit));
                }
                None => break,
            }
        }
        loop {
            let &(i, rev) = chain.front().expect("never empty");
            let entry = if rev { ends[i].1 } else { ends[i].0 };
            match next(entry, i).filter(|&k| !seen[k]) {
                Some(k) => {
                    seen[k] = true;
                    chain.push_front((k, ends[k].1 != entry));
                }
                None => break,
            }
        }
        let walked = |&(i, rev): &(usize, bool)| if rev { (tracks[i].b, tracks[i].a) } else { (tracks[i].a, tracks[i].b) };
        let mut points = Vec::new();
        for step in &chain {
            let (a, b) = walked(step);
            if points.last() != Some(&a) {
                points.push(a);
            }
            points.push(b);
        }
        let (mut forward, mut backward) = (false, false);
        for s in signals.iter().filter(|s| s.facing != Vec2::ZERO) {
            let Some(step) = signal_segment.get(s.name.as_str()).and_then(|g| chain.iter().find(|(i, _)| tracks[*i].segment == *g)) else {
                continue;
            };
            let (a, b) = walked(step);
            let along = s.facing.dot(b - a);
            forward |= along > 0.0;
            backward |= along < 0.0;
        }
        let (first, last) = (chain.front().expect("never empty"), chain.back().expect("never empty"));
        let start_node = if first.1 { ends[first.0].1 } else { ends[first.0].0 };
        let end_node = if last.1 { ends[last.0].0 } else { ends[last.0].1 };
        let at_exit = |p: Option<&Pos2>| p.is_some_and(|p| exits.iter().any(|e| e.distance(*p) < 1.0));
        let loose_start = loose(start_node) || at_exit(points.first());
        let loose_end = loose(end_node) || at_exit(points.last());
        out.push(Run { points, forward, backward, loose_start, loose_end });
    }
    out
}
