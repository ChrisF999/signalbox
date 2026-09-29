//! Resolve names to IDs and validate a `WorldFile`.

use std::collections::BTreeMap;

use super::file::*;
use super::{LoadError, World};
use crate::ids::*;
use crate::network::*;

type Index = BTreeMap<String, u32>;

fn index<T>(kind: &'static str, items: &[T], name: impl Fn(&T) -> &str) -> Result<Index, LoadError> {
    let mut m = Index::new();
    for (i, it) in items.iter().enumerate() {
        if m.insert(name(it).to_string(), i as u32).is_some() {
            return Err(LoadError::Duplicate { kind, name: name(it).to_string() });
        }
    }
    Ok(m)
}

fn get(m: &Index, kind: &'static str, name: &str, from: &str) -> Result<u32, LoadError> {
    m.get(name).copied().ok_or_else(|| unknown(kind, name, from))
}

pub(super) fn unknown(kind: &'static str, name: &str, from: &str) -> LoadError {
    LoadError::UnknownRef { kind, name: name.to_string(), from: from.to_string() }
}

pub(super) fn other(msg: String) -> LoadError {
    LoadError::Other(msg)
}

fn typed<T>(m: Index, f: impl Fn(u32) -> T) -> BTreeMap<String, T> {
    m.into_iter().map(|(k, v)| (k, f(v))).collect()
}

pub(super) fn build(f: WorldFile) -> Result<World, LoadError> {
    if f.schema != SCHEMA_VERSION {
        return Err(LoadError::Schema(f.schema));
    }
    let net = build_network(&f)?;
    Ok(World { title: f.title.clone(), net, layout: f.layout.clone() })
}

pub(super) fn build_network(f: &WorldFile) -> Result<Network, LoadError> {
    let areas = index("area", &f.areas, |a| &a.name)?;
    let sections = index("section", &f.sections, |s| &s.name)?;
    let nodes = index("node", &f.nodes, |n| &n.name)?;
    let segs = index("segment", &f.segments, |s| &s.name)?;
    let signals = index("signal", &f.signals, |s| &s.name)?;
    let berths = index("berth", &f.berths, |b| &b.name)?;

    let mut net = Network::default();
    for a in &f.areas {
        net.areas.push(Area { name: a.name.clone() });
    }
    for s in &f.sections {
        let area = AreaId(get(&areas, "area", &s.area, &s.name)?);
        net.sections.push(Section { name: s.name.clone(), area, segments: vec![] });
    }
    for s in &f.segments {
        let a = NodeId(get(&nodes, "node", &s.from, &s.name)?);
        let b = NodeId(get(&nodes, "node", &s.to, &s.name)?);
        let section = SectionId(get(&sections, "section", &s.section, &s.name)?);
        let id = SegmentId::from_idx(net.segments.len());
        net.sections[section.idx()].segments.push(id);
        net.segments.push(Segment {
            name: s.name.clone(),
            a,
            b,
            length_m: s.length_m,
            line_speed: s.line_speed_kmh / 3.6,
            gradient: s.gradient,
            section,
        });
    }
    for n in &f.nodes {
        let kind = match &n.kind {
            NodeKindFile::Joint => NodeKind::Joint,
            NodeKindFile::BufferStop => NodeKind::BufferStop,
            NodeKindFile::Boundary => NodeKind::Boundary,
            NodeKindFile::Points { toe, normal, reverse, swing_s } => NodeKind::Points {
                toe: SegmentId(get(&segs, "segment", toe, &n.name)?),
                normal: SegmentId(get(&segs, "segment", normal, &n.name)?),
                reverse: SegmentId(get(&segs, "segment", reverse, &n.name)?),
                swing_s: *swing_s,
            },
        };
        net.nodes.push(Node { name: n.name.clone(), kind, segments: vec![] });
    }
    for i in 0..net.segments.len() {
        let (a, b) = (net.segments[i].a, net.segments[i].b);
        net.nodes[a.idx()].segments.push(SegmentId::from_idx(i));
        if b != a {
            net.nodes[b.idx()].segments.push(SegmentId::from_idx(i));
        }
    }
    for n in &net.nodes {
        let bad = |p: &str| LoadError::BadNode { node: n.name.clone(), problem: p.to_string() };
        match &n.kind {
            NodeKind::Joint if n.segments.len() != 2 => return Err(bad("a joint must join exactly 2 segments")),
            NodeKind::BufferStop | NodeKind::Boundary if n.segments.len() != 1 => {
                return Err(bad("an end must have exactly 1 segment"));
            }
            NodeKind::Points { toe, normal, reverse, .. } => {
                let mut want = vec![*toe, *normal, *reverse];
                want.sort();
                want.dedup();
                let mut have = n.segments.clone();
                have.sort();
                if want.len() != 3 || have != want {
                    return Err(bad("points legs must be the 3 attached segments, all different"));
                }
            }
            _ => {}
        }
    }
    for s in &net.sections {
        if s.segments.is_empty() {
            return Err(LoadError::EmptySection(s.name.clone()));
        }
    }
    for s in &f.signals {
        let segment = SegmentId(get(&segs, "segment", &s.segment, &s.name)?);
        let area = AreaId(get(&areas, "area", &s.area, &s.name)?);
        let len = net.segments[segment.idx()].length_m;
        if s.offset_m < 0.0 || s.offset_m > len {
            return Err(LoadError::SignalOffTrack(s.name.clone()));
        }
        if !(2..=4).contains(&s.aspects) {
            return Err(other(format!("signal `{}`: aspects must be 2, 3 or 4", s.name)));
        }
        net.signals.push(Signal {
            name: s.name.clone(),
            area,
            at: Position { segment, offset_m: s.offset_m, dir: s.direction },
            aspects: s.aspects,
            sighting_m: s.sighting_m,
            berth: None,
        });
    }
    for b in &f.berths {
        let signal = b.signal.as_ref().map(|n| get(&signals, "signal", n, &b.name).map(SignalId)).transpose()?;
        let boundary = b.boundary.as_ref().map(|n| get(&nodes, "node", n, &b.name).map(NodeId)).transpose()?;
        if signal.is_some() == boundary.is_some() {
            return Err(LoadError::BerthUnanchored(b.name.clone()));
        }
        if let Some(bn) = boundary {
            if net.nodes[bn.idx()].kind != NodeKind::Boundary {
                return Err(LoadError::BerthUnanchored(b.name.clone()));
            }
        }
        let id = BerthId::from_idx(net.berths.len());
        if let Some(s) = signal {
            net.signals[s.idx()].berth = Some(id);
        }
        net.berths.push(Berth { name: b.name.clone(), signal, boundary });
    }
    for p in &f.platforms {
        let from = format!("platform {} {}", p.place, p.platform);
        let segment = SegmentId(get(&segs, "segment", &p.segment, &from)?);
        let len = net.segments[segment.idx()].length_m;
        if !(0.0 <= p.from_m && p.from_m < p.to_m && p.to_m <= len) {
            return Err(other(format!("{from}: extent {}..{} is not on its segment", p.from_m, p.to_m)));
        }
        net.platforms.push(Platform {
            place: p.place.clone(),
            platform: p.platform.clone(),
            segment,
            from_m: p.from_m,
            to_m: p.to_m,
        });
    }
    net.signals_on = vec![vec![]; net.segments.len()];
    for (i, s) in net.signals.iter().enumerate() {
        net.signals_on[s.at.segment.idx()].push(SignalId::from_idx(i));
    }
    net.platforms_on = vec![vec![]; net.segments.len()];
    for (i, p) in net.platforms.iter().enumerate() {
        net.platforms_on[p.segment.idx()].push(PlatformId::from_idx(i));
    }
    net.names = Names {
        nodes: typed(nodes, NodeId),
        segments: typed(segs, SegmentId),
        sections: typed(sections, SectionId),
        signals: typed(signals, SignalId),
        berths: typed(berths, BerthId),
        areas: typed(areas, AreaId),
    };
    Ok(net)
}
