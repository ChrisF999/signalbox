//! TS2 item chain → signalbox nodes, segments and sections.
//!
//! Lines become segments. Points, signals, buffers and ends are zero-length
//! TS2 items and become nodes; every points node gets three 1 m legs, and two
//! zero-length items that touch are joined by a 1 m spacer segment.

use std::collections::{BTreeMap, BTreeSet};

use signalbox_core::network::Dir;
use signalbox_core::world::file::*;

use crate::report::{self, Report};
use crate::ts2::{Item, Port, Ts2};

/// Length of points legs and spacer segments.
pub const SPACER_M: f64 = 1.0;
const SWING_S: f64 = 5.0;
/// An end with a platform line this close in rear is a buffer stop.
const PLATFORM_SEARCH_M: f64 = 400.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Line,
    Points,
    Signal,
    Buffer,
    End,
}

fn kind(item: &Item) -> Option<Kind> {
    match item {
        Item::LineItem(_) | Item::InvisibleLinkItem(_) => Some(Kind::Line),
        Item::PointsItem(_) => Some(Kind::Points),
        Item::SignalItem(s) if s.signal_type == "BUFFER" => Some(Kind::Buffer),
        Item::SignalItem(_) => Some(Kind::Signal),
        Item::EndItem(_) => Some(Kind::End),
        _ => None,
    }
}

fn ports(k: Kind) -> &'static [Port] {
    match k {
        Kind::Points => &[Port::Prev, Port::Next, Port::Rev],
        Kind::End => &[Port::Prev],
        _ => &[Port::Prev, Port::Next],
    }
}

#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub nodes: Vec<NodeFile>,
    pub segments: Vec<SegmentFile>,
    pub sections: Vec<SectionFile>,
    pub signals: Vec<SignalFile>,
    pub berths: Vec<BerthFile>,
    pub platforms: Vec<PlatformFile>,
    pub area: String,
    /// TS2 signal tiId → signal name (main signals only).
    pub signal_names: BTreeMap<String, String>,
    /// Signal name → TS2 tiId.
    pub signal_ti: BTreeMap<String, String>,
    /// TS2 BUFFER signal tiId → the end node beyond it.
    pub buffer_ends: BTreeMap<String, String>,
    /// TS2 points tiId → node name.
    pub points_nodes: BTreeMap<String, String>,
    /// TS2 line tiId → segment name.
    pub line_segments: BTreeMap<String, String>,
    /// TS2 EndItem tiId → node name.
    pub end_nodes: BTreeMap<String, String>,
    /// Node names of boundary ends.
    pub boundaries: BTreeSet<String>,
}

struct Builder {
    nodes: Vec<NodeFile>,
    segments: Vec<SegmentFile>,
    /// (item, port) of a zero-length item → the segment attached on that side.
    port_seg: BTreeMap<(String, Port), String>,
    /// (line, port) → the node at that end of the line.
    line_end: BTreeMap<(String, Port), String>,
}

impl Builder {
    fn node(&mut self, name: String, kind: NodeKindFile) {
        self.nodes.push(NodeFile { name, kind });
    }

    fn segment(&mut self, name: String, from: String, to: String, length_m: f64, kmh: f64) {
        self.segments.push(SegmentFile {
            name,
            from,
            to,
            length_m,
            line_speed_kmh: kmh,
            gradient: 0.0,
            section: String::new(),
        });
    }
}

/// The node other items attach to at `port` of zero-length item `id`.
fn attach(kinds: &BTreeMap<&str, Kind>, id: &str, port: Port) -> String {
    if kinds[id] == Kind::Points { format!("N{id}.{}", port.tag()) } else { format!("N{id}") }
}

pub fn build(ts2: &Ts2, entry_ends: &BTreeSet<String>, report: &mut Report) -> Result<Graph, String> {
    let items = &ts2.track_items;
    let kinds: BTreeMap<&str, Kind> =
        items.iter().filter_map(|(id, it)| kind(it).map(|k| (id.as_str(), k))).collect();

    // Keep the track connected to something a route or train uses.
    let mut stack: Vec<&str> = ts2
        .routes
        .values()
        .map(|r| r.begin_signal.as_str())
        .chain(ts2.trains.iter().map(|t| t.train_head.track_item.as_str()))
        .filter(|id| kinds.contains_key(id))
        .collect();
    let mut kept: BTreeSet<&str> = BTreeSet::new();
    while let Some(id) = stack.pop() {
        if !kept.insert(id) {
            continue;
        }
        for &p in ports(kinds[id]) {
            if let Some(nb) = items[id].link(p) {
                if kinds.contains_key(nb) && !kept.contains(nb) {
                    stack.push(nb);
                }
            }
        }
    }
    let dropped = kinds.len() - kept.len();
    if dropped > 0 {
        report.warn(report::ORPHANS, format!("{dropped} track items not connected to any route or train were dropped"));
    }

    let mut b = Builder { nodes: vec![], segments: vec![], port_seg: BTreeMap::new(), line_end: BTreeMap::new() };
    let mut signal_nodes: BTreeSet<String> = BTreeSet::new();
    let mut leg_ends: BTreeSet<String> = BTreeSet::new();
    let mut g = Graph { area: if ts2.options.title.is_empty() { "Main".into() } else { ts2.options.title.clone() }, ..Graph::default() };

    // Nodes for the zero-length items.
    for &id in &kept {
        match kinds[id] {
            Kind::Line => {}
            Kind::Points => {
                b.node(
                    format!("N{id}"),
                    NodeKindFile::Points {
                        toe: format!("P{id}p"),
                        normal: format!("P{id}n"),
                        reverse: format!("P{id}r"),
                        swing_s: SWING_S,
                    },
                );
                for p in [Port::Prev, Port::Next, Port::Rev] {
                    let leg_end = format!("N{id}.{}", p.tag());
                    b.node(leg_end.clone(), NodeKindFile::Joint);
                    b.segment(format!("P{id}{}", p.tag()), format!("N{id}"), leg_end.clone(), SPACER_M, 0.0);
                    leg_ends.insert(leg_end);
                }
                g.points_nodes.insert(id.to_string(), format!("N{id}"));
            }
            Kind::Signal | Kind::Buffer => {
                b.node(format!("N{id}"), NodeKindFile::Joint);
                if kinds[id] == Kind::Signal {
                    signal_nodes.insert(format!("N{id}"));
                }
            }
            Kind::End => {
                b.node(format!("N{id}"), NodeKindFile::Boundary);
                g.end_nodes.insert(id.to_string(), format!("N{id}"));
            }
        }
    }

    // Links between items.
    let mut done: BTreeSet<((String, Port), (String, Port))> = BTreeSet::new();
    for &id in &kept {
        let k = kinds[id];
        for &p in ports(k) {
            let nb = items[id].link(p).ok_or_else(|| format!("track item {id} has no {p:?} link"))?;
            let kn = *kinds.get(nb).ok_or_else(|| format!("track item {id} links to {nb}, which is not track"))?;
            let q = items[nb].port_to(id).ok_or_else(|| format!("track items {id} and {nb} are not linked both ways"))?;
            let (a, z) = if (id, p) <= (nb, q) { ((id, p), (nb, q)) } else { ((nb, q), (id, p)) };
            if !done.insert(((a.0.to_string(), a.1), (z.0.to_string(), z.1))) {
                continue;
            }
            let label = format!("{}.{}-{}.{}", a.0, a.1.tag(), z.0, z.1.tag());
            match (k == Kind::Line, kn == Kind::Line) {
                (true, true) => {
                    let j = format!("J{label}");
                    b.node(j.clone(), NodeKindFile::Joint);
                    b.line_end.insert((id.to_string(), p), j.clone());
                    b.line_end.insert((nb.to_string(), q), j);
                }
                (true, false) => {
                    b.line_end.insert((id.to_string(), p), attach(&kinds, nb, q));
                    b.port_seg.insert((nb.to_string(), q), format!("L{id}"));
                }
                (false, true) => {
                    b.line_end.insert((nb.to_string(), q), attach(&kinds, id, p));
                    b.port_seg.insert((id.to_string(), p), format!("L{nb}"));
                }
                (false, false) => {
                    let s = format!("S{label}");
                    b.segment(s.clone(), attach(&kinds, a.0, a.1), attach(&kinds, z.0, z.1), SPACER_M, 0.0);
                    b.port_seg.insert((id.to_string(), p), s.clone());
                    b.port_seg.insert((nb.to_string(), q), s);
                }
            }
        }
    }

    // Line segments.
    let place_speed: BTreeMap<&str, f64> = items
        .values()
        .filter_map(|it| match it {
            Item::Place(p) => p.place_code.as_deref().map(|c| (c, p.max_speed)),
            _ => None,
        })
        .collect();
    for &id in &kept {
        let (Item::LineItem(l) | Item::InvisibleLinkItem(l)) = &items[id] else { continue };
        let from = b.line_end[&(id.to_string(), Port::Prev)].clone();
        let to = b.line_end[&(id.to_string(), Port::Next)].clone();
        let mps = if l.max_speed > 0.0 {
            l.max_speed
        } else {
            l.place_code
                .as_deref()
                .and_then(|c| place_speed.get(c).copied())
                .filter(|&v| v > 0.0)
                .unwrap_or(ts2.options.default_max_speed)
        };
        b.segment(format!("L{id}"), from, to, l.real_length, mps * 3.6);
        g.line_segments.insert(id.to_string(), format!("L{id}"));
        if let (Some(pc), Some(tc)) = (&l.place_code, &l.track_code) {
            if !tc.is_empty() {
                g.platforms.push(PlatformFile {
                    place: pc.clone(),
                    platform: tc.clone(),
                    segment: format!("L{id}"),
                    from_m: 0.0,
                    to_m: l.real_length,
                });
            }
        }
    }
    fill_spacer_speeds(&mut b.segments, ts2.options.default_max_speed * 3.6);

    // Sections.
    let at = node_segments(&b.segments);
    let index: BTreeMap<String, usize> = b.segments.iter().enumerate().map(|(i, s)| (s.name.clone(), i)).collect();
    let mut uf = UnionFind::new(b.segments.len());
    for n in &b.nodes {
        let here = at.get(n.name.as_str()).map(Vec::as_slice).unwrap_or(&[]);
        let joins = match n.kind {
            NodeKindFile::Joint => !signal_nodes.contains(&n.name) && !leg_ends.contains(&n.name),
            NodeKindFile::Points { .. } => true,
            _ => false,
        };
        if joins {
            for w in here.windows(2) {
                uf.union(w[0], w[1]);
            }
        }
    }
    for (i, s) in b.segments.iter().enumerate() {
        if !s.name.starts_with('S') {
            continue;
        }
        for end in [&s.from, &s.to] {
            if leg_ends.contains(end.as_str()) {
                for &j in &at[end.as_str()] {
                    if b.segments[j].name.starts_with('P') {
                        uf.union(i, j);
                    }
                }
            }
        }
    }
    for &id in &kept {
        let Item::LineItem(l) = &items[id] else { continue };
        let Some(c) = l.conflict_ti_id.as_deref() else { continue };
        if id < c && kept.contains(c) {
            uf.union(index[format!("L{id}").as_str()], index[format!("L{c}").as_str()]);
            report.warn(report::CROSSING, format!("lines {id} and {c} cross on the flat and share one section"));
        }
    }
    let mut section_of_root: BTreeMap<usize, String> = BTreeMap::new();
    for i in 0..b.segments.len() {
        let root = uf.find(i);
        if !section_of_root.contains_key(&root) {
            let name = format!("T{}", section_of_root.len() + 1);
            g.sections.push(SectionFile { name: name.clone(), area: g.area.clone() });
            section_of_root.insert(root, name);
        }
        b.segments[i].section = section_of_root[&root].clone();
    }

    // Ends: boundary if trains enter there or no platform is close in rear.
    let platform_segs: BTreeSet<&str> = g.platforms.iter().map(|p| p.segment.as_str()).collect();
    for (ti, node) in &g.end_nodes {
        let boundary = entry_ends.contains(ti) || !near_platform(node, &b.segments, &at, &platform_segs);
        let kind = if boundary { NodeKindFile::Boundary } else { NodeKindFile::BufferStop };
        report.warn(
            report::END_CLASS,
            format!("end {ti} is a {}", if boundary { "boundary" } else { "buffer stop" }),
        );
        if boundary {
            g.boundaries.insert(node.clone());
        }
        if let Some(n) = b.nodes.iter_mut().find(|n| &n.name == node) {
            n.kind = kind;
        }
    }

    // Signals, buffers and berths.
    let mut name_count: BTreeMap<String, usize> = BTreeMap::new();
    let base_name = |id: &str| match &items[id] {
        Item::SignalItem(s) => s.name.clone().unwrap_or_else(|| format!("S{id}")),
        _ => unreachable!("only signals are named here"),
    };
    // BUFFER signals count too: the name repeats in TS2 even though a buffer
    // signal is not emitted (gretz: 512, 806, 808, 810 pair a signal with a buffer).
    for &id in &kept {
        if matches!(kinds[id], Kind::Signal | Kind::Buffer) {
            *name_count.entry(base_name(id)).or_insert(0) += 1;
        }
    }
    let mut odd_types: BTreeSet<String> = BTreeSet::new();
    for &id in &kept {
        let Item::SignalItem(s) = &items[id] else { continue };
        if kinds[id] == Kind::Buffer {
            match s.next_ti_id.as_deref().filter(|n| kinds.get(n) == Some(&Kind::End)) {
                Some(end) => {
                    g.buffer_ends.insert(id.to_string(), format!("N{end}"));
                }
                None => report.warn(report::BUFFER_NOT_AT_END, format!("buffer signal {id} is not in front of an end")),
            }
            continue;
        }
        let base = base_name(id);
        let name = if name_count[&base] > 1 { format!("{base}#{id}") } else { base };
        let seg_name = b.port_seg[&(id.to_string(), Port::Prev)].clone();
        let seg = &b.segments[index[seg_name.as_str()]];
        let (direction, offset_m) =
            if seg.to == format!("N{id}") { (Dir::Up, seg.length_m) } else { (Dir::Down, 0.0) };
        let aspects = if s.signal_type.starts_with("UK_4") {
            4
        } else if s.signal_type.starts_with("UK_3") {
            3
        } else {
            if odd_types.insert(s.signal_type.clone()) {
                report.warn(report::SIGNAL_TYPE, format!("signal type {} is treated as 3-aspect", s.signal_type));
            }
            3
        };
        g.signals.push(SignalFile {
            name: name.clone(),
            area: g.area.clone(),
            segment: seg_name,
            offset_m,
            direction,
            aspects,
            sighting_m: ts2.options.default_signal_visibility,
        });
        g.berths.push(BerthFile { name: format!("B{name}"), signal: Some(name.clone()), boundary: None });
        g.signal_ti.insert(name.clone(), id.to_string());
        g.signal_names.insert(id.to_string(), name);
    }
    for node in &g.boundaries {
        g.berths.push(BerthFile { name: format!("F{node}"), signal: None, boundary: Some(node.clone()) });
    }

    g.nodes = b.nodes;
    g.segments = b.segments;
    Ok(g)
}

/// Node name → indexes of the segments ending there. Owned keys, so the
/// segments can still be edited while this map is in use.
fn node_segments(segs: &[SegmentFile]) -> BTreeMap<String, Vec<usize>> {
    let mut at: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, s) in segs.iter().enumerate() {
        at.entry(s.from.clone()).or_default().push(i);
        at.entry(s.to.clone()).or_default().push(i);
    }
    at
}

/// Legs and spacers (speed 0 so far) take the fastest neighbouring speed.
fn fill_spacer_speeds(segs: &mut [SegmentFile], default_kmh: f64) {
    let at = node_segments(segs);
    for _ in 0..4 {
        let known: Vec<f64> = segs.iter().map(|s| s.line_speed_kmh).collect();
        for i in 0..segs.len() {
            if known[i] > 0.0 {
                continue;
            }
            let best = [&segs[i].from, &segs[i].to]
                .iter()
                .flat_map(|n| at[n.as_str()].iter())
                .map(|&j| known[j])
                .fold(0.0, f64::max);
            if best > 0.0 {
                segs[i].line_speed_kmh = best;
            }
        }
    }
    for s in segs.iter_mut() {
        if s.line_speed_kmh <= 0.0 {
            s.line_speed_kmh = default_kmh;
        }
    }
}

/// Whether a platform line lies within `PLATFORM_SEARCH_M` of `start`.
fn near_platform(
    start: &str,
    segs: &[SegmentFile],
    at: &BTreeMap<String, Vec<usize>>,
    platforms: &BTreeSet<&str>,
) -> bool {
    let mut best: BTreeMap<&str, f64> = BTreeMap::from([(start, 0.0)]);
    let mut queue: Vec<(&str, f64)> = vec![(start, 0.0)];
    while let Some((node, d)) = queue.pop() {
        for &i in at.get(node).map(Vec::as_slice).unwrap_or(&[]) {
            let s = &segs[i];
            if platforms.contains(s.name.as_str()) {
                return true;
            }
            let other = if s.from == node { s.to.as_str() } else { s.from.as_str() };
            let nd = d + s.length_m;
            if nd <= PLATFORM_SEARCH_M && best.get(other).is_none_or(|&b| nd < b) {
                best.insert(other, nd);
                queue.push((other, nd));
            }
        }
    }
    false
}

struct UnionFind(Vec<usize>);

impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind((0..n).collect())
    }

    fn find(&mut self, i: usize) -> usize {
        let mut r = i;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut c = i;
        while self.0[c] != r {
            let n = self.0[c];
            self.0[c] = r;
            c = n;
        }
        r
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra.max(rb)] = ra.min(rb);
        }
    }
}
