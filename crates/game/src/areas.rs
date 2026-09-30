//! Which area each command subject belongs to (spec §3.4), and what each
//! player sees: their area plus the fringe of their neighbours (§4.5).

use std::collections::BTreeSet;

use signalbox_core::events::Command;
use signalbox_core::ids::*;
use signalbox_core::network::{Network, NodeKind};
use signalbox_core::world::World;

/// The areas of command subjects, resolved once per world.
#[derive(Clone, Debug)]
pub struct AreaMap {
    /// Each signal's area (routes belong to their entrance signal's).
    pub signal: Vec<AreaId>,
    /// The section each berth hangs on: its signal's section, or for a
    /// boundary berth the section at the boundary.
    pub berth_section: Vec<SectionId>,
    /// Each berth's area: its signal's, or its boundary section's.
    pub berth: Vec<AreaId>,
    /// Each node's points area (the points section's), `None` for other nodes.
    pub points: Vec<Option<AreaId>>,
}

/// The section a signal's segment belongs to.
pub fn signal_section(net: &Network, s: SignalId) -> SectionId {
    net.segments[net.signals[s.idx()].at.segment.idx()].section
}

/// The node a signal stands at: the nearer end of its segment.
pub fn signal_node(net: &Network, s: SignalId) -> NodeId {
    let sig = &net.signals[s.idx()];
    let seg = &net.segments[sig.at.segment.idx()];
    if sig.at.offset_m <= seg.length_m / 2.0 { seg.a } else { seg.b }
}

impl AreaMap {
    pub fn new(w: &World) -> AreaMap {
        let net = &w.net;
        let signal: Vec<AreaId> = net.signals.iter().map(|s| s.area).collect();
        let mut berth_section = Vec::with_capacity(net.berths.len());
        let mut berth = Vec::with_capacity(net.berths.len());
        for b in &net.berths {
            let (sec, area) = match (b.signal, b.boundary) {
                (Some(s), _) => (signal_section(net, s), signal[s.idx()]),
                (None, Some(n)) => {
                    let sec = net.segments[net.nodes[n.idx()].segments[0].idx()].section;
                    (sec, net.sections[sec.idx()].area)
                }
                (None, None) => unreachable!("the loader gives every berth a signal or a boundary"),
            };
            berth_section.push(sec);
            berth.push(area);
        }
        let points = (0..net.nodes.len())
            .map(|n| net.points_section(NodeId::from_idx(n)).map(|sec| net.sections[sec.idx()].area))
            .collect();
        AreaMap { signal, berth_section, berth, points }
    }

    /// The area of the thing a command acts on.
    pub fn subject(&self, cmd: &Command) -> Option<AreaId> {
        match cmd {
            Command::SetRoute { entrance, .. }
            | Command::CancelRoute { entrance }
            | Command::SetAutoWorking { entrance, .. } => self.signal.get(entrance.idx()).copied(),
            Command::SwingPoints { points, .. } => self.points.get(points.idx()).copied().flatten(),
            Command::Interpose { berth, .. } | Command::CancelBerth { berth } => self.berth.get(berth.idx()).copied(),
        }
    }
}

/// What one player sees. Lists are in index order.
#[derive(Clone, Debug, PartialEq)]
pub struct Visibility {
    /// The area they hold; `None` for a spectator.
    pub area: Option<AreaId>,
    pub sections: Vec<SectionId>,
    /// Visible sections outside `area` (empty for a spectator).
    pub fringe: BTreeSet<SectionId>,
    pub signals: Vec<SignalId>,
    pub points: Vec<NodeId>,
    pub berths: Vec<BerthId>,
    pub routes: Vec<RouteId>,
}

impl Visibility {
    pub fn spectator(w: &World, map: &AreaMap) -> Visibility {
        Self::from_sections(w, map, None, vec![true; w.net.sections.len()], BTreeSet::new())
    }

    pub fn of_area(w: &World, map: &AreaMap, a: AreaId) -> Visibility {
        let fringe = fringe(w, a);
        let visible = (0..w.net.sections.len())
            .map(|i| w.net.sections[i].area == a || fringe.contains(&SectionId::from_idx(i)))
            .collect();
        Self::from_sections(w, map, Some(a), visible, fringe)
    }

    /// Whether this player may work things in area `a`.
    pub fn operable(&self, a: AreaId) -> bool {
        self.area == Some(a)
    }

    fn from_sections(w: &World, map: &AreaMap, area: Option<AreaId>, visible: Vec<bool>, fringe: BTreeSet<SectionId>) -> Visibility {
        let net = &w.net;
        let sections = (0..net.sections.len()).filter(|&i| visible[i]).map(SectionId::from_idx).collect();
        let signals = (0..net.signals.len())
            .map(SignalId::from_idx)
            .filter(|&s| visible[signal_section(net, s).idx()])
            .collect();
        let points = (0..net.nodes.len())
            .map(NodeId::from_idx)
            .filter(|&n| net.points_section(n).is_some_and(|sec| visible[sec.idx()]))
            .collect();
        let berths = (0..net.berths.len())
            .map(BerthId::from_idx)
            .filter(|&b| visible[map.berth_section[b.idx()].idx()])
            .collect();
        let routes = (0..w.routes.len())
            .map(RouteId::from_idx)
            .filter(|&r| visible[signal_section(net, w.routes[r.idx()].entrance).idx()])
            .collect();
        Visibility { area, sections, fringe, signals, points, berths, routes }
    }
}

/// Segments a train arriving at node `n` on segment `came` can run on to:
/// through points only from the toe to a leg or from a leg to the toe.
fn onward(net: &Network, n: NodeId, came: SegmentId) -> Vec<SegmentId> {
    let node = &net.nodes[n.idx()];
    match node.kind {
        NodeKind::Points { toe, normal, reverse, .. } => {
            if came == toe {
                vec![normal, reverse]
            } else if came == normal || came == reverse {
                vec![toe]
            } else {
                vec![]
            }
        }
        _ => node.segments.iter().copied().filter(|&s| s != came).collect(),
    }
}

/// Sections of other areas a player of `a` sees (amendment 6): walk the
/// track away from `a` as a train could, and stop after a segment whose far
/// node holds any signal.
pub fn fringe(w: &World, a: AreaId) -> BTreeSet<SectionId> {
    let net = &w.net;
    let signal_nodes: BTreeSet<NodeId> =
        (0..net.signals.len()).map(|s| signal_node(net, SignalId::from_idx(s))).collect();
    let area_of = |g: SegmentId| net.sections[net.segments[g.idx()].section.idx()].area;
    // (segment entered, node it was entered from)
    let mut stack: Vec<(SegmentId, NodeId)> = Vec::new();
    for i in 0..net.segments.len() {
        let g = SegmentId::from_idx(i);
        if area_of(g) != a {
            continue;
        }
        for n in [net.segments[i].a, net.segments[i].b] {
            for h in onward(net, n, g) {
                if area_of(h) != a {
                    stack.push((h, n));
                }
            }
        }
    }
    let mut seen: BTreeSet<(SegmentId, NodeId)> = BTreeSet::new();
    let mut out = BTreeSet::new();
    while let Some((h, n)) = stack.pop() {
        if !seen.insert((h, n)) {
            continue;
        }
        let seg = &net.segments[h.idx()];
        out.insert(seg.section);
        let far = if seg.a == n { seg.b } else { seg.a };
        if signal_nodes.contains(&far) {
            continue;
        }
        for k in onward(net, far, h) {
            if area_of(k) != a {
                stack.push((k, far));
            }
        }
    }
    out
}
