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
