//! Diagram geometry for clients (spec D1 §4.1), read once from the world's
//! `layout` JSON (ts2-import writes it) and cut down to each player's
//! visible set. Names in the JSON that the world does not know are dropped.

use std::collections::{BTreeMap, BTreeSet};

use protocol::{Geometry, LabelGeom, LineGeom, NodeGeom, PlatformGeom, PointsGeom, SignalGeom};
use serde::Deserialize;
use signalbox_core::ids::*;
use signalbox_core::network::{Dir, Network, NodeKind};
use signalbox_core::routes::Exit;
use signalbox_core::world::World;

use crate::areas::Visibility;

/// Labels this far outside the visible drawing are still shown.
pub const LABEL_MARGIN: f64 = 40.0;
/// How many nodes away from a points leg, a signal or an exit to look for a drawn line.
const SEARCH_DEPTH: usize = 4;

#[derive(Deserialize)]
struct RawLayout {
    #[serde(default)]
    lines: Vec<RawLine>,
    #[serde(default)]
    points: Vec<RawPoints>,
    #[serde(default)]
    signals: Vec<RawSignal>,
    #[serde(default)]
    platforms: Vec<PlatformGeom>,
    #[serde(default)]
    labels: Vec<LabelGeom>,
}

#[derive(Deserialize)]
struct RawLine {
    segment: String,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

#[derive(Deserialize)]
struct RawPoints {
    node: String,
    x: f64,
    y: f64,
}

#[derive(Deserialize)]
struct RawSignal {
    signal: String,
    x: f64,
    y: f64,
    berth_x: f64,
    berth_y: f64,
}

/// The whole world's geometry, resolved to ids.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldGeometry {
    lines: Vec<(SegmentId, LineGeom)>,
    points: Vec<(NodeId, PointsGeom)>,
    signals: Vec<(SignalId, SignalGeom)>,
    /// With the segments their (place, platform) covers in the world.
    platforms: Vec<(Vec<SegmentId>, PlatformGeom)>,
    labels: Vec<LabelGeom>,
    /// Route exit nodes and boundary-berth nodes, where a line can be found.
    nodes: BTreeMap<NodeId, NodeGeom>,
}

impl WorldGeometry {
    /// `None` when the world has no usable `layout` (hand-made worlds).
    pub fn from_world(w: &World) -> Option<WorldGeometry> {
        if !w.layout.is_object() {
            return None;
        }
        let raw: RawLayout = serde_json::from_value(w.layout.clone()).ok()?;
        let net = &w.net;
        let lines: Vec<(SegmentId, LineGeom)> = raw
            .lines
            .into_iter()
            .filter_map(|l| {
                let seg = net.segment(&l.segment)?;
                Some((seg, LineGeom { segment: l.segment, x1: l.x1, y1: l.y1, x2: l.x2, y2: l.y2 }))
            })
            .collect();
        let line_of: BTreeMap<SegmentId, usize> = lines.iter().enumerate().map(|(i, (s, _))| (*s, i)).collect();
        let points = raw
            .points
            .into_iter()
            .filter_map(|p| {
                let n = net.node(&p.node)?;
                let NodeKind::Points { toe, normal, reverse, .. } = net.nodes[n.idx()].kind else { return None };
                let leg = |seg: SegmentId| {
                    let s = &net.segments[seg.idx()];
                    let far = if s.a == n { s.b } else { s.a };
                    line_from(net, &lines, &line_of, far, Some(n)).map(|(end, _)| end)
                };
                Some((n, PointsGeom { node: p.node, x: p.x, y: p.y, toe: leg(toe), normal: leg(normal), reverse: leg(reverse) }))
            })
            .collect();
        let signals = raw
            .signals
            .into_iter()
            .filter_map(|s| {
                let id = net.signal(&s.signal)?;
                let facing = facing(net, &lines, &line_of, id);
                Some((id, SignalGeom { signal: s.signal, x: s.x, y: s.y, berth_x: s.berth_x, berth_y: s.berth_y, facing }))
            })
            .collect();
        let platforms = raw
            .platforms
            .into_iter()
            .map(|p| {
                let segs = net
                    .platforms
                    .iter()
                    .filter(|q| q.place == p.place && q.platform == p.platform)
                    .map(|q| q.segment)
                    .collect();
                (segs, p)
            })
            .collect();
        let wanted: BTreeSet<NodeId> = w
            .routes
            .iter()
            .filter_map(|r| match r.exit {
                Exit::Node(n) => Some(n),
                Exit::Signal(_) => None,
            })
            .chain(net.berths.iter().filter_map(|b| b.boundary))
            .collect();
        let nodes = wanted
            .into_iter()
            .filter_map(|n| {
                let ([x, y], _) = line_from(net, &lines, &line_of, n, None)?;
                Some((n, NodeGeom { node: net.nodes[n.idx()].name.clone(), x, y }))
            })
            .collect();
        Some(WorldGeometry { lines, points, signals, platforms, labels: raw.labels, nodes })
    }

    /// What `vis` sees: drawn segments, points and signals it can see,
    /// platforms on a visible segment, labels near all that, and the
    /// positions of its routes' exit nodes and boundary berths.
    pub fn visible(&self, w: &World, vis: &Visibility) -> Geometry {
        let net = &w.net;
        let sections: BTreeSet<SectionId> = vis.sections.iter().copied().collect();
        let seg_visible = |s: SegmentId| sections.contains(&net.segments[s.idx()].section);
        let points_seen: BTreeSet<NodeId> = vis.points.iter().copied().collect();
        let signals_seen: BTreeSet<SignalId> = vis.signals.iter().copied().collect();
        let lines: Vec<LineGeom> = self.lines.iter().filter(|(s, _)| seg_visible(*s)).map(|(_, l)| l.clone()).collect();
        let points: Vec<PointsGeom> =
            self.points.iter().filter(|(n, _)| points_seen.contains(n)).map(|(_, p)| p.clone()).collect();
        let signals: Vec<SignalGeom> =
            self.signals.iter().filter(|(s, _)| signals_seen.contains(s)).map(|(_, g)| g.clone()).collect();
        let platforms: Vec<PlatformGeom> = self
            .platforms
            .iter()
            .filter(|(segs, _)| segs.iter().any(|&s| seg_visible(s)))
            .map(|(_, p)| p.clone())
            .collect();
        let mut bounds = Bounds::default();
        for l in &lines {
            bounds.add(l.x1, l.y1);
            bounds.add(l.x2, l.y2);
        }
        for p in &points {
            bounds.add(p.x, p.y);
        }
        for s in &signals {
            bounds.add(s.x, s.y);
            bounds.add(s.berth_x, s.berth_y);
        }
        for p in &platforms {
            bounds.add(p.x1, p.y1);
            bounds.add(p.x2, p.y2);
        }
        let labels = self.labels.iter().filter(|l| bounds.near(l.x, l.y, LABEL_MARGIN)).cloned().collect();
        let mut wanted: BTreeSet<NodeId> = vis
            .routes
            .iter()
            .filter_map(|r| match w.routes[r.idx()].exit {
                Exit::Node(n) => Some(n),
                Exit::Signal(_) => None,
            })
            .collect();
        wanted.extend(vis.berths.iter().filter_map(|b| net.berths[b.idx()].boundary));
        let nodes = wanted.iter().filter_map(|n| self.nodes.get(n).cloned()).collect();
        Geometry { lines, points, signals, platforms, labels, nodes }
    }
}

#[derive(Default)]
struct Bounds {
    min: Option<(f64, f64)>,
    max: (f64, f64),
}

impl Bounds {
    fn add(&mut self, x: f64, y: f64) {
        match self.min {
            None => {
                self.min = Some((x, y));
                self.max = (x, y);
            }
            Some((mx, my)) => {
                self.min = Some((mx.min(x), my.min(y)));
                self.max = (self.max.0.max(x), self.max.1.max(y));
            }
        }
    }

    fn near(&self, x: f64, y: f64, margin: f64) -> bool {
        self.min.is_some_and(|(mx, my)| {
            x >= mx - margin && x <= self.max.0 + margin && y >= my - margin && y <= self.max.1 + margin
        })
    }
}

/// The nearest drawn line reached from node `start` without passing
/// `avoid`, walking at most `SEARCH_DEPTH` nodes: the line's end at the
/// node it was reached through, and the direction along the line towards
/// that end.
fn line_from(
    net: &Network,
    lines: &[(SegmentId, LineGeom)],
    line_of: &BTreeMap<SegmentId, usize>,
    start: NodeId,
    avoid: Option<NodeId>,
) -> Option<([f64; 2], [f64; 2])> {
    let mut seen: BTreeSet<NodeId> = avoid.into_iter().collect();
    let mut frontier = vec![start];
    for _ in 0..SEARCH_DEPTH {
        let mut next = Vec::new();
        for n in frontier {
            if !seen.insert(n) {
                continue;
            }
            for &seg in &net.nodes[n.idx()].segments {
                let s = &net.segments[seg.idx()];
                if let Some(&i) = line_of.get(&seg) {
                    let l = &lines[i].1;
                    return Some(if s.a == n {
                        ([l.x1, l.y1], [l.x1 - l.x2, l.y1 - l.y2])
                    } else {
                        ([l.x2, l.y2], [l.x2 - l.x1, l.y2 - l.y1])
                    });
                }
                next.push(if s.a == n { s.b } else { s.a });
            }
        }
        frontier = next;
    }
    None
}

/// The direction a train passing signal `s` travels: along its segment's
/// line, or else along the nearest line in rear of it.
fn facing(net: &Network, lines: &[(SegmentId, LineGeom)], line_of: &BTreeMap<SegmentId, usize>, s: SignalId) -> Option<[f64; 2]> {
    let at = &net.signals[s.idx()].at;
    let seg = &net.segments[at.segment.idx()];
    let v = match line_of.get(&at.segment) {
        Some(&i) => {
            let l = &lines[i].1;
            match at.dir {
                Dir::Up => [l.x2 - l.x1, l.y2 - l.y1],
                Dir::Down => [l.x1 - l.x2, l.y1 - l.y2],
            }
        }
        None => line_from(net, lines, line_of, seg.end_node(at.dir.rev()), Some(seg.end_node(at.dir)))?.1,
    };
    (v[0] != 0.0 || v[1] != 0.0).then_some(v)
}
