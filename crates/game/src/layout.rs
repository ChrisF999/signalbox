//! The static layout a player is sent (spec §4.1).

use std::collections::BTreeSet;

use protocol::{BerthInfo, ExitName, Layout, PlatformInfo, PointsInfo, RouteInfo, SectionInfo, SegmentInfo, SignalInfo};
use signalbox_core::ids::*;
use signalbox_core::routes::Exit;
use signalbox_core::world::World;

use crate::areas::{AreaMap, Visibility};
use crate::geometry::WorldGeometry;

pub fn exit_name(w: &World, e: Exit) -> ExitName {
    match e {
        Exit::Signal(s) => ExitName::Signal(w.net.signals[s.idx()].name.clone()),
        Exit::Node(n) => ExitName::Node(w.net.nodes[n.idx()].name.clone()),
    }
}

/// `geo` is the world's geometry (`WorldGeometry::from_world`), built once per game.
pub fn build_layout(w: &World, map: &AreaMap, vis: &Visibility, you: &str, geo: Option<&WorldGeometry>) -> Layout {
    let net = &w.net;
    let area_name = |a: AreaId| net.areas[a.idx()].name.clone();
    let section_name = |s: SectionId| net.sections[s.idx()].name.clone();
    let visible: BTreeSet<SectionId> = vis.sections.iter().copied().collect();
    Layout {
        title: w.title.clone(),
        you: you.to_string(),
        area: vis.area.map(area_name),
        areas: net.areas.iter().map(|a| a.name.clone()).collect(),
        sections: vis
            .sections
            .iter()
            .map(|&s| SectionInfo {
                name: section_name(s),
                area: area_name(net.sections[s.idx()].area),
                fringe: vis.fringe.contains(&s),
            })
            .collect(),
        segments: net
            .segments
            .iter()
            .filter(|g| visible.contains(&g.section))
            .map(|g| SegmentInfo {
                name: g.name.clone(),
                from: net.nodes[g.a.idx()].name.clone(),
                to: net.nodes[g.b.idx()].name.clone(),
                length_m: g.length_m,
                section: section_name(g.section),
            })
            .collect(),
        signals: vis
            .signals
            .iter()
            .map(|&s| {
                let sig = &net.signals[s.idx()];
                SignalInfo {
                    name: sig.name.clone(),
                    area: area_name(map.signal[s.idx()]),
                    segment: net.segments[sig.at.segment.idx()].name.clone(),
                    offset_m: sig.at.offset_m,
                    direction: sig.at.dir,
                    aspects: sig.aspects,
                    operable: vis.operable(map.signal[s.idx()]),
                }
            })
            .collect(),
        points: vis
            .points
            .iter()
            .map(|&n| {
                let sec = net.points_section(n).expect("visible points are points");
                let a = net.sections[sec.idx()].area;
                PointsInfo {
                    name: net.nodes[n.idx()].name.clone(),
                    section: section_name(sec),
                    area: area_name(a),
                    operable: vis.operable(a),
                }
            })
            .collect(),
        berths: vis
            .berths
            .iter()
            .map(|&b| {
                let berth = &net.berths[b.idx()];
                BerthInfo {
                    name: berth.name.clone(),
                    signal: berth.signal.map(|s| net.signals[s.idx()].name.clone()),
                    boundary: berth.boundary.map(|n| net.nodes[n.idx()].name.clone()),
                    area: area_name(map.berth[b.idx()]),
                    operable: vis.operable(map.berth[b.idx()]),
                }
            })
            .collect(),
        platforms: net
            .platforms
            .iter()
            .filter(|p| visible.contains(&net.segments[p.segment.idx()].section))
            .map(|p| PlatformInfo {
                place: p.place.clone(),
                platform: p.platform.clone(),
                segment: net.segments[p.segment.idx()].name.clone(),
                from_m: p.from_m,
                to_m: p.to_m,
            })
            .collect(),
        routes: vis
            .routes
            .iter()
            .map(|&r| {
                let def = &w.routes[r.idx()];
                RouteInfo {
                    name: def.name.clone(),
                    entrance: net.signals[def.entrance.idx()].name.clone(),
                    exit: exit_name(w, def.exit),
                    automatic: def.automatic,
                    operable: vis.operable(map.signal[def.entrance.idx()]),
                }
            })
            .collect(),
        geometry: geo.map(|g| g.visible(w, vis)),
        // Display data: `Game` adds it from what it builds once per game.
        box_prefix: String::new(),
        workstations: Default::default(),
        simplifier: vec![],
        display_headcodes: Default::default(),
    }
}
