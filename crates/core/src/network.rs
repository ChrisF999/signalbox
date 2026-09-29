//! Static track layout: what exists and how it connects.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::*;

/// Direction relative to a segment: `Up` runs from node `a` to node `b`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dir {
    Up,
    Down,
}

impl Dir {
    pub fn rev(self) -> Dir {
        match self {
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointsPos {
    Normal,
    Reverse,
}

#[derive(Clone, Debug)]
pub struct Segment {
    pub name: String,
    pub a: NodeId,
    pub b: NodeId,
    pub length_m: f64,
    /// Metres per second.
    pub line_speed: f64,
    /// Per mille, rising in the `Up` direction. Always 0 in v1 data.
    pub gradient: f64,
    pub section: SectionId,
}

impl Segment {
    /// The node reached by travelling along this segment in `dir`.
    pub fn end_node(&self, dir: Dir) -> NodeId {
        match dir {
            Dir::Up => self.b,
            Dir::Down => self.a,
        }
    }

    /// Distance from the segment's start *in direction `dir`* to the point
    /// `offset_m` from node `a`.
    pub fn along(&self, offset_m: f64, dir: Dir) -> f64 {
        match dir {
            Dir::Up => offset_m,
            Dir::Down => self.length_m - offset_m,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Joint,
    BufferStop,
    Boundary,
    Points { toe: SegmentId, normal: SegmentId, reverse: SegmentId, swing_s: f64 },
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub kind: NodeKind,
    pub segments: Vec<SegmentId>,
}

#[derive(Clone, Debug)]
pub struct Section {
    pub name: String,
    pub area: AreaId,
    pub segments: Vec<SegmentId>,
}

#[derive(Clone, Debug)]
pub struct Area {
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub segment: SegmentId,
    /// Metres from the segment's node `a`.
    pub offset_m: f64,
    pub dir: Dir,
}

#[derive(Clone, Debug)]
pub struct Signal {
    pub name: String,
    pub area: AreaId,
    /// Where the signal stands and which way it faces.
    pub at: Position,
    /// 2, 3 or 4.
    pub aspects: u8,
    pub sighting_m: f64,
    /// The describer berth in rear of this signal.
    pub berth: Option<BerthId>,
}

#[derive(Clone, Debug)]
pub struct Berth {
    pub name: String,
    pub signal: Option<SignalId>,
    pub boundary: Option<NodeId>,
}

#[derive(Clone, Debug)]
pub struct Platform {
    pub place: String,
    pub platform: String,
    pub segment: SegmentId,
    pub from_m: f64,
    pub to_m: f64,
}

/// Read access to points positions. `None` means moving (not detected).
pub trait PointsView {
    fn position(&self, node: NodeId) -> Option<PointsPos>;
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Names {
    pub nodes: BTreeMap<String, NodeId>,
    pub segments: BTreeMap<String, SegmentId>,
    pub sections: BTreeMap<String, SectionId>,
    pub signals: BTreeMap<String, SignalId>,
    pub berths: BTreeMap<String, BerthId>,
    pub areas: BTreeMap<String, AreaId>,
}

#[derive(Clone, Debug, Default)]
pub struct Network {
    pub areas: Vec<Area>,
    pub sections: Vec<Section>,
    pub nodes: Vec<Node>,
    pub segments: Vec<Segment>,
    pub signals: Vec<Signal>,
    pub berths: Vec<Berth>,
    pub platforms: Vec<Platform>,
    /// Signals standing on each segment (indexed by segment).
    pub signals_on: Vec<Vec<SignalId>>,
    /// Platforms on each segment (indexed by segment).
    pub platforms_on: Vec<Vec<PlatformId>>,
    pub(crate) names: Names,
}

impl Network {
    pub fn node(&self, name: &str) -> Option<NodeId> {
        self.names.nodes.get(name).copied()
    }

    pub fn segment(&self, name: &str) -> Option<SegmentId> {
        self.names.segments.get(name).copied()
    }

    pub fn section(&self, name: &str) -> Option<SectionId> {
        self.names.sections.get(name).copied()
    }

    pub fn signal(&self, name: &str) -> Option<SignalId> {
        self.names.signals.get(name).copied()
    }

    pub fn berth(&self, name: &str) -> Option<BerthId> {
        self.names.berths.get(name).copied()
    }

    pub fn area(&self, name: &str) -> Option<AreaId> {
        self.names.areas.get(name).copied()
    }

    /// The section a points node sits in (its toe segment's section).
    pub fn points_section(&self, node: NodeId) -> Option<SectionId> {
        match self.nodes[node.idx()].kind {
            NodeKind::Points { toe, .. } => Some(self.segments[toe.idx()].section),
            _ => None,
        }
    }

    pub fn swing_s(&self, node: NodeId) -> f64 {
        match self.nodes[node.idx()].kind {
            NodeKind::Points { swing_s, .. } => swing_s,
            _ => 0.0,
        }
    }

    pub fn boundary_berth(&self, node: NodeId) -> Option<BerthId> {
        self.berths.iter().position(|b| b.boundary == Some(node)).map(BerthId::from_idx)
    }

    /// A platform's near and far ends, as distances along its segment in `dir`.
    pub fn platform_along(&self, p: PlatformId, dir: Dir) -> (f64, f64) {
        let pl = &self.platforms[p.idx()];
        let seg = &self.segments[pl.segment.idx()];
        let (x, y) = (seg.along(pl.from_m, dir), seg.along(pl.to_m, dir));
        (x.min(y), x.max(y))
    }
}

/// One segment reached while walking ahead. The part of the segment ahead
/// starts at `from_along` (measured along `dir`); a point at along-distance `x`
/// on this segment is `d_start + (x - from_along)` metres from the origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Step {
    pub seg: SegmentId,
    pub dir: Dir,
    pub d_start: f64,
    pub from_along: f64,
}

/// The track ended (buffer stop, boundary, or points not set for us) at `node`,
/// `d` metres from the origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WalkEnd {
    pub node: NodeId,
    pub d: f64,
}

/// Guard against endless loops on circular layouts.
const MAX_WALK_STEPS: usize = 10_000;

impl Network {
    /// The segment and direction entered after leaving `seg` in `dir`, or
    /// `None` at a buffer stop, a boundary, moving points, or points lying
    /// against us.
    pub fn next(&self, seg: SegmentId, dir: Dir, pts: &impl PointsView) -> Option<(SegmentId, Dir)> {
        let node_id = self.segments[seg.idx()].end_node(dir);
        let node = &self.nodes[node_id.idx()];
        let next = match node.kind {
            NodeKind::Joint => node.segments.iter().copied().find(|&s| s != seg)?,
            NodeKind::BufferStop | NodeKind::Boundary => return None,
            NodeKind::Points { toe, normal, reverse, .. } => {
                let leg = match pts.position(node_id)? {
                    PointsPos::Normal => normal,
                    PointsPos::Reverse => reverse,
                };
                if seg == toe {
                    leg
                } else if seg == leg {
                    toe
                } else {
                    return None;
                }
            }
        };
        let s = &self.segments[next.idx()];
        let d = if s.a == node_id { Dir::Up } else { Dir::Down };
        Some((next, d))
    }

    /// Walk ahead from `from_along` metres into `(seg, dir)` until `max_dist`
    /// or the end of the track.
    pub fn walk_ahead(
        &self,
        seg: SegmentId,
        dir: Dir,
        from_along: f64,
        max_dist: f64,
        pts: &impl PointsView,
    ) -> (Vec<Step>, Option<WalkEnd>) {
        let mut steps = Vec::new();
        let (mut s, mut d, mut d0, mut fa) = (seg, dir, 0.0, from_along);
        loop {
            steps.push(Step { seg: s, dir: d, d_start: d0, from_along: fa });
            let d_end = d0 + (self.segments[s.idx()].length_m - fa);
            if d_end > max_dist || steps.len() >= MAX_WALK_STEPS {
                return (steps, None);
            }
            match self.next(s, d, pts) {
                Some((ns, nd)) => {
                    s = ns;
                    d = nd;
                    d0 = d_end;
                    fa = 0.0;
                }
                None => {
                    let node = self.segments[s.idx()].end_node(d);
                    return (steps, Some(WalkEnd { node, d: d_end }));
                }
            }
        }
    }

    /// The first signal facing us strictly ahead of `from_along`, and its distance.
    pub fn first_signal_ahead(
        &self,
        seg: SegmentId,
        dir: Dir,
        from_along: f64,
        max_dist: f64,
        pts: &impl PointsView,
    ) -> Option<(SignalId, f64)> {
        let (steps, _) = self.walk_ahead(seg, dir, from_along, max_dist, pts);
        for (i, st) in steps.iter().enumerate() {
            let sg = &self.segments[st.seg.idx()];
            let best = self.signals_on[st.seg.idx()]
                .iter()
                .copied()
                .filter(|&s| self.signals[s.idx()].at.dir == st.dir)
                .map(|s| (s, sg.along(self.signals[s.idx()].at.offset_m, st.dir)))
                .filter(|&(_, a)| if i == 0 { a > st.from_along } else { a >= st.from_along })
                .min_by(|x, y| x.1.total_cmp(&y.1));
            if let Some((s, a)) = best {
                let d = st.d_start + (a - st.from_along);
                return (d <= max_dist).then_some((s, d));
            }
        }
        None
    }

    /// Sections within `dist` metres in rear of a point facing `at.dir`.
    pub fn sections_in_rear(&self, at: Position, dist: f64, pts: &impl PointsView) -> Vec<SectionId> {
        let back = at.dir.rev();
        let along = self.segments[at.segment.idx()].along(at.offset_m, back);
        let (steps, _) = self.walk_ahead(at.segment, back, along, dist, pts);
        let mut out: Vec<SectionId> = Vec::new();
        for st in steps {
            let s = self.segments[st.seg.idx()].section;
            if !out.contains(&s) {
                out.push(s);
            }
        }
        out
    }

    /// Whether two sections meet at a node.
    pub fn sections_touch(&self, a: SectionId, b: SectionId) -> bool {
        let ends = |s: SectionId| -> Vec<NodeId> {
            self.sections[s.idx()]
                .segments
                .iter()
                .flat_map(|&g| [self.segments[g.idx()].a, self.segments[g.idx()].b])
                .collect()
        };
        let ea = ends(a);
        ends(b).iter().any(|n| ea.contains(n))
    }
}
