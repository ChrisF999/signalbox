//! A player's dynamic view, built from sim state (spec §4.2). Never from
//! events, so a client that applies every delta can never drift.

use std::collections::BTreeMap;

use protocol::{Held, PointsPos, PointsView, RouteState, RouteView, SectionView, View, VoteView};
use signalbox_core::interlocking::{Owner, RouteState as IlState};
use signalbox_core::points::PointsState;
use signalbox_core::sim::Sim;

use crate::areas::Visibility;

/// The parts of a view every player shares, computed once per flush.
#[derive(Clone, Debug, PartialEq)]
pub struct Shared {
    pub sim_time: f64,
    pub speed: u8,
    pub paused: bool,
    pub vote: Option<VoteView>,
    /// Area → holder, or `"robot"`.
    pub holders: BTreeMap<String, String>,
}

pub fn build_view(sim: &Sim, vis: &Visibility, shared: &Shared, seq: u64) -> View {
    let w = sim.world();
    let net = &w.net;
    let il = sim.interlocking();
    let signals = vis.signals.iter().map(|&s| (net.signals[s.idx()].name.clone(), sim.aspect(s))).collect();
    let routes = vis
        .routes
        .iter()
        .filter_map(|&r| {
            let st = &il.routes[r.idx()];
            let state = match (st.state, st.cancel) {
                (IlState::Idle, _) => return None,
                (_, Some(_)) => RouteState::Cancelling,
                (IlState::Setting, None) => RouteState::Setting,
                (IlState::Locked, None) => RouteState::Locked,
            };
            Some((w.routes[r.idx()].name.clone(), RouteView { state, auto_working: st.auto_working }))
        })
        .collect();
    let points = vis
        .points
        .iter()
        .map(|&n| {
            let sec = net.points_section(n).expect("visible points are points");
            let (position, moving) = match sim.points().state(n) {
                Some(PointsState::Set(p)) => (p, false),
                Some(PointsState::Moving { to, .. }) => (to, true),
                None => (PointsPos::Normal, false),
            };
            let locked = il.owner[sec.idx()].is_some();
            (net.nodes[n.idx()].name.clone(), PointsView { position, moving, locked })
        })
        .collect();
    let sections = vis
        .sections
        .iter()
        .map(|&s| {
            let held = match il.owner[s.idx()] {
                None => Held::Free,
                Some(Owner::Path(_)) => Held::Path,
                Some(Owner::Overlap(_)) => Held::Overlap,
            };
            (net.sections[s.idx()].name.clone(), SectionView { occupied: sim.occupancy().occupied(s), held })
        })
        .collect();
    let berths = vis
        .berths
        .iter()
        .filter_map(|&b| sim.describer().get(b).map(|h| (net.berths[b.idx()].name.clone(), h.to_string())))
        .collect();
    View {
        seq,
        sim_time: shared.sim_time,
        speed: shared.speed,
        paused: shared.paused,
        vote: shared.vote.clone(),
        holders: shared.holders.clone(),
        score: vis.area.map(|a| sim.scores().by_area[a.idx()]),
        signals,
        routes,
        points,
        sections,
        berths,
        trains: BTreeMap::new(),
    }
}
