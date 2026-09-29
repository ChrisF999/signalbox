//! Resolve names to IDs and validate a `WorldFile`.

use std::collections::BTreeMap;

use super::file::*;
use super::{LoadError, World};
use crate::ids::*;
use crate::network::*;
use crate::routes::*;
use crate::time::parse_hms;
use crate::timetable::*;

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
    let routes = build_routes(&f, &net)?;
    let mut routes_from = vec![vec![]; net.signals.len()];
    for (i, r) in routes.iter().enumerate() {
        routes_from[r.entrance.idx()].push(RouteId::from_idx(i));
    }
    let (train_types, services, entries, options) = build_timetable(&f, &net)?;
    Ok(World {
        title: f.title.clone(),
        net,
        routes,
        routes_from,
        train_types,
        services,
        entries,
        options,
        layout: f.layout.clone(),
    })
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
        if !(s.length_m > 0.0) {
            return Err(other(format!("segment `{}`: length_m must be positive", s.name)));
        }
        if !(s.line_speed_kmh > 0.0) {
            return Err(other(format!("segment `{}`: line_speed_kmh must be positive", s.name)));
        }
        let a = NodeId(get(&nodes, "node", &s.from, &s.name)?);
        let b = NodeId(get(&nodes, "node", &s.to, &s.name)?);
        if a == b {
            return Err(other(format!("segment `{}`: from and to must be different nodes", s.name)));
        }
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
            NodeKindFile::Points { toe, normal, reverse, swing_s } if !(*swing_s >= 0.0) => {
                return Err(other(format!("points `{}`: swing_s must not be negative", n.name)));
            }
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
        if !(s.sighting_m >= 0.0) {
            return Err(other(format!("signal `{}`: sighting_m must not be negative", s.name)));
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

fn build_routes(f: &WorldFile, net: &Network) -> Result<Vec<RouteDef>, LoadError> {
    let mut out: Vec<RouteDef> = Vec::new();
    for r in &f.routes {
        let entrance = net.signal(&r.entrance).ok_or_else(|| unknown("signal", &r.entrance, "route"))?;
        let (exit, exit_name) = match &r.exit {
            ExitFile::Signal(n) => (Exit::Signal(net.signal(n).ok_or_else(|| unknown("signal", n, &r.entrance))?), n),
            ExitFile::Node(n) => (Exit::Node(net.node(n).ok_or_else(|| unknown("node", n, &r.entrance))?), n),
        };
        let name = format!("{}-{}", r.entrance, exit_name);
        if out.iter().any(|o| o.entrance == entrance && o.exit == exit) {
            return Err(LoadError::Duplicate { kind: "route", name });
        }
        let bad = |p: &str| LoadError::BadRoute { route: name.clone(), problem: p.to_string() };
        if let Exit::Node(n) = exit {
            if !matches!(net.nodes[n.idx()].kind, NodeKind::BufferStop | NodeKind::Boundary) {
                return Err(bad("exit node must be a buffer stop or boundary"));
            }
        }
        let sections = |names: &[String]| -> Result<Vec<SectionId>, LoadError> {
            names.iter().map(|n| net.section(n).ok_or_else(|| unknown("section", n, &name))).collect()
        };
        let points = |reqs: &[PointsReqFile]| -> Result<Vec<(NodeId, PointsPos)>, LoadError> {
            reqs.iter()
                .map(|q| -> Result<(NodeId, PointsPos), LoadError> {
                    let id = net.node(&q.points).ok_or_else(|| unknown("node", &q.points, &name))?;
                    if net.points_section(id).is_none() {
                        return Err(bad(&format!("`{}` is not points", q.points)));
                    }
                    Ok((id, q.position))
                })
                .collect()
        };
        let path = sections(&r.path)?;
        let overlap = sections(&r.overlap)?;
        let path_points = points(&r.points)?;
        let overlap_points = points(&r.overlap_points)?;
        if path.is_empty() {
            return Err(bad("path is empty"));
        }
        if matches!(exit, Exit::Node(_)) && !overlap.is_empty() {
            return Err(bad("a route to a buffer stop or boundary cannot have an overlap"));
        }
        for &(p, _) in path_points.iter().chain(overlap_points.iter()) {
            let sec = net.points_section(p).expect("checked to be points above");
            if !path.contains(&sec) && !overlap.contains(&sec) {
                return Err(bad(&format!(
                    "points `{}` are required but their section is in neither path nor overlap",
                    net.nodes[p.idx()].name
                )));
            }
        }
        let chain: Vec<SectionId> = path.iter().chain(overlap.iter()).copied().collect();
        for pair in chain.windows(2) {
            if !net.sections_touch(pair[0], pair[1]) {
                return Err(bad(&format!(
                    "sections `{}` and `{}` do not touch",
                    net.sections[pair[0].idx()].name,
                    net.sections[pair[1].idx()].name
                )));
            }
        }
        for (i, n) in net.nodes.iter().enumerate() {
            let id = NodeId::from_idx(i);
            let Some(sec) = net.points_section(id) else { continue };
            if path.contains(&sec) && !path_points.iter().any(|&(p, _)| p == id) {
                return Err(bad(&format!("crosses points `{}` without a required position", n.name)));
            }
            if overlap.contains(&sec) && !overlap_points.iter().any(|&(p, _)| p == id) {
                return Err(bad(&format!("overlap crosses points `{}` without a required position", n.name)));
            }
        }
        let traced = net.trace_route(entrance, &path_points, &overlap_points, overlap.len()).map_err(|e| bad(&e))?;
        let names = |v: &[SectionId]| -> String {
            v.iter().map(|s| net.sections[s.idx()].name.as_str()).collect::<Vec<_>>().join(", ")
        };
        if traced.path != path {
            return Err(bad(&format!(
                "declared path [{}] but the track from the entrance runs through [{}]",
                names(&path),
                names(&traced.path)
            )));
        }
        if traced.exit != exit {
            return Err(bad("the track from the entrance does not end at the declared exit"));
        }
        if traced.overlap != overlap {
            return Err(bad(&format!(
                "declared overlap [{}] but the track beyond the exit runs through [{}]",
                names(&overlap),
                names(&traced.overlap)
            )));
        }
        for &(p, _) in path_points.iter().chain(overlap_points.iter()) {
            if !traced.points_crossed.contains(&p) {
                return Err(bad(&format!("points `{}` are required but the route does not cross them", net.nodes[p.idx()].name)));
            }
        }
        out.push(RouteDef {
            name,
            entrance,
            exit,
            path,
            points: path_points,
            overlap,
            overlap_points,
            automatic: r.automatic,
        });
    }
    check_automatic_clashes(net, &out)?;
    Ok(out)
}

/// Automatic routes are all set at start, so two of them must not want the
/// same track. A route continuing from another's exit signal may take over
/// that route's overlap, which is not a clash, provided the two agree on where
/// the points they share must lie.
fn check_automatic_clashes(net: &Network, routes: &[RouteDef]) -> Result<(), LoadError> {
    let autos: Vec<&RouteDef> = routes.iter().filter(|r| r.automatic).collect();
    for (i, x) in autos.iter().enumerate() {
        for y in &autos[i + 1..] {
            let takeover = |rear: &RouteDef, next: &RouteDef, s: &SectionId| {
                rear.exit == Exit::Signal(next.entrance) && rear.overlap.contains(s) && next.path.contains(s)
            };
            let shared = x.path.iter().chain(x.overlap.iter()).find(|s| {
                (y.path.contains(s) || y.overlap.contains(s)) && !takeover(x, y, s) && !takeover(y, x, s)
            });
            if let Some(s) = shared {
                return Err(LoadError::BadRoute {
                    route: x.name.clone(),
                    problem: format!(
                        "automatic routes `{}` and `{}` both use section `{}`",
                        x.name, y.name, net.sections[s.idx()].name
                    ),
                });
            }
            let clash = x.all_points().find(|&&(p, pos)| y.all_points().any(|&(q, qpos)| q == p && qpos != pos));
            if let Some(&(p, _)) = clash {
                return Err(LoadError::BadRoute {
                    route: x.name.clone(),
                    problem: format!(
                        "automatic routes `{}` and `{}` disagree on points `{}`",
                        x.name, y.name, net.nodes[p.idx()].name
                    ),
                });
            }
        }
    }
    Ok(())
}

type Timetable = (Vec<TrainType>, Vec<Service>, Vec<Entry>, Options);

fn build_timetable(f: &WorldFile, net: &Network) -> Result<Timetable, LoadError> {
    let tt_index = index("train type", &f.train_types, |t| &t.code)?;
    let svc_index = index("service", &f.services, |s| &s.headcode)?;
    let time = |s: &str, from: &str| -> Result<f64, LoadError> {
        parse_hms(s).map(f64::from).ok_or_else(|| other(format!("{from}: bad time `{s}`")))
    };
    for t in &f.train_types {
        for (what, v) in [
            ("max_speed_kmh", t.max_speed_kmh),
            ("accel", t.accel),
            ("service_brake", t.service_brake),
            ("emergency_brake", t.emergency_brake),
            ("length_m", t.length_m),
        ] {
            if !(v.is_finite() && v > 0.0) {
                return Err(other(format!("train type `{}`: {what} must be positive and finite", t.code)));
            }
        }
    }
    let train_types = f
        .train_types
        .iter()
        .map(|t| TrainType {
            code: t.code.clone(),
            max_speed: t.max_speed_kmh / 3.6,
            accel: t.accel,
            service_brake: t.service_brake,
            emergency_brake: t.emergency_brake,
            length_m: t.length_m,
            mass_t: t.mass_t,
        })
        .collect();
    let mut services = Vec::new();
    for s in &f.services {
        let train_type = TrainTypeId(get(&tt_index, "train type", &s.train_type, &s.headcode)?);
        let mut calls = Vec::new();
        for c in &s.calls {
            let exists = net
                .platforms
                .iter()
                .any(|p| p.place == c.place && c.platform.as_ref().is_none_or(|x| *x == p.platform));
            if !exists {
                return Err(other(format!(
                    "{}: no platform {} at `{}`",
                    s.headcode,
                    c.platform.as_deref().unwrap_or("(any)"),
                    c.place
                )));
            }
            calls.push(Call {
                place: c.place.clone(),
                platform: c.platform.clone(),
                arr_s: c.arr.as_deref().map(|t| time(t, &s.headcode)).transpose()?,
                dep_s: c.dep.as_deref().map(|t| time(t, &s.headcode)).transpose()?,
                stop: c.stop,
            });
        }
        let end = match &s.end {
            EndFile::Exit => EndAction::Exit,
            EndFile::Stable => EndAction::Stable,
            EndFile::Form { service } => EndAction::Form(ServiceId(get(&svc_index, "service", service, &s.headcode)?)),
        };
        if end != EndAction::Exit && !calls.iter().any(|c| c.stop) {
            return Err(other(format!("{}: a service that forms or stables must have a stopping call", s.headcode)));
        }
        services.push(Service { headcode: s.headcode.clone(), train_type, calls, end });
    }
    let mut entries = Vec::new();
    for e in &f.entries {
        let service = ServiceId(get(&svc_index, "service", &e.service, "entry")?);
        let start = match (&e.boundary, &e.at) {
            (Some(b), None) => {
                let n = net.node(b).ok_or_else(|| unknown("node", b, &e.service))?;
                if net.nodes[n.idx()].kind != NodeKind::Boundary {
                    return Err(other(format!("entry {}: `{}` is not a boundary", e.service, b)));
                }
                EntryStart::Boundary(n)
            }
            (None, Some(p)) => {
                let segment = net.segment(&p.segment).ok_or_else(|| unknown("segment", &p.segment, &e.service))?;
                let len = net.segments[segment.idx()].length_m;
                if !(p.offset_m.is_finite() && 0.0 <= p.offset_m && p.offset_m <= len) {
                    return Err(other(format!("entry {}: offset_m must lie on segment `{}`", e.service, p.segment)));
                }
                EntryStart::At(Position { segment, offset_m: p.offset_m, dir: p.direction })
            }
            _ => return Err(other(format!("entry {}: give exactly one of `boundary` or `at`", e.service))),
        };
        if !(e.speed_kmh.is_finite() && e.speed_kmh >= 0.0) {
            return Err(other(format!("entry {}: speed_kmh must be finite and not negative", e.service)));
        }
        entries.push(Entry { service, start, time_s: time(&e.time, &e.service)?, speed: e.speed_kmh / 3.6 });
    }
    entries.sort_by(|a, b| a.time_s.total_cmp(&b.time_s));
    let o = &f.options;
    if o.entry_delay_s[0] > o.entry_delay_s[1] || o.min_dwell_s[0] > o.min_dwell_s[1] {
        return Err(other("options: ranges must be [min, max]".into()));
    }
    for (what, v) in [("overlap_release_s", o.overlap_release_s), ("approach_lock_s", o.approach_lock_s)] {
        if !(v.is_finite() && v >= 0.0) {
            return Err(other(format!("options: {what} must be finite and not negative")));
        }
    }
    let options = Options {
        start_s: time(&o.start_time, "options")?,
        entry_delay_s: (o.entry_delay_s[0], o.entry_delay_s[1]),
        min_dwell_s: (o.min_dwell_s[0], o.min_dwell_s[1]),
        overlap_release_s: o.overlap_release_s,
        approach_lock_s: o.approach_lock_s,
        late_penalty_per_min: o.late_penalty_per_min,
        wrong_platform_penalty: o.wrong_platform_penalty,
        spad_penalty: o.spad_penalty,
        collision_penalty: o.collision_penalty,
    };
    Ok((train_types, services, entries, options))
}
