//! A player's dynamic view, built from sim state (spec §4.2). Never from
//! events, so a client that applies every delta can never drift.

use std::collections::BTreeMap;

use protocol::{Held, PointsPos, PointsView, RouteState, RouteView, SectionView, TrainRow, TrainState, View, VoteView};
use signalbox_core::ids::SectionId;
use signalbox_core::interlocking::{Owner, RouteState as IlState};
use signalbox_core::points::PointsState;
use signalbox_core::sim::Sim;
use signalbox_core::timetable::{Call, EntryStart};

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
        trains: build_trains(sim, vis),
    }
}

/// Trains not yet on the railway are listed this long before their entry.
pub const DUE_WINDOW_S: f64 = 30.0 * 60.0;

/// Whole minutes late at `now` against `booked`, as seconds; 0 when early,
/// on time or unbooked. Minutes, so a late train changes its row once a
/// sim minute rather than every tick.
pub fn late_s(now: f64, booked: Option<f64>) -> i64 {
    match booked {
        Some(b) if now > b => ((now - b) / 60.0).floor() as i64 * 60,
        _ => 0,
    }
}

/// A row for a train whose next call is `call`. A train standing at the
/// call (`AtPlatform`: it is dwelling there) is timed against the call's
/// departure, so it is not late while it waits for it (TS2 books a train
/// that starts in a platform in long before it is due out); without a
/// booked departure, against the arrival as for a running train.
fn row(call: Option<&Call>, now: f64, state: TrainState) -> TrainRow {
    let booked = call.and_then(|c| match state {
        TrainState::AtPlatform => c.dep_s.or(c.arr_s),
        _ => c.arr_s.or(c.dep_s),
    });
    TrainRow {
        next_place: call.map(|c| c.place.clone()),
        next_platform: call.and_then(|c| c.platform.clone()),
        booked,
        arr: call.and_then(|c| c.arr_s),
        dep: call.and_then(|c| c.dep_s),
        late_s: late_s(now, booked),
        state,
    }
}

/// The train list (spec D1 §4.2), from sim state only. A player sees every
/// train on their visible track, or whose next call is at a platform in
/// their area, or that is due to enter at a boundary of their area within
/// `DUE_WINDOW_S` (or is waiting there); a spectator sees every train
/// running and every one due within the window. A headcode shown twice
/// keeps its first row (running trains in sim order, then entries).
pub fn build_trains(sim: &Sim, vis: &Visibility) -> BTreeMap<String, TrainRow> {
    let w = sim.world();
    let net = &w.net;
    let now = sim.now_s();
    let mut visible = vec![false; net.sections.len()];
    for s in &vis.sections {
        visible[s.idx()] = true;
    }
    let in_area = |sec: SectionId| vis.area.is_none_or(|a| net.sections[sec.idx()].area == a);
    let platform_in_area = |c: &Call| {
        c.platform.as_deref().is_some_and(|pf| {
            net.platforms.iter().any(|p| p.place == c.place && p.platform == pf && in_area(net.segments[p.segment.idx()].section))
        })
    };
    let mut rows: BTreeMap<String, TrainRow> = BTreeMap::new();
    for t in sim.trains() {
        let call = w.services[t.service.idx()].calls.get(t.next_call);
        let on_visible = t.path.iter().any(|(s, _)| visible[net.segments[s.idx()].section.idx()]);
        if !(vis.area.is_none() || on_visible || call.is_some_and(platform_in_area)) {
            continue;
        }
        let head_in_area = t.path.back().is_some_and(|(s, _)| in_area(net.segments[s.idx()].section));
        let state = if t.dwell.is_some() {
            TrainState::AtPlatform
        } else if head_in_area {
            TrainState::InArea
        } else {
            TrainState::Approaching
        };
        rows.entry(t.headcode.clone()).or_insert_with(|| row(call, now, state));
    }
    let waiting = sim.pending_entries().iter().map(|p| p.entry);
    // On-demand entries are due only once offered (then they are waiting).
    let coming = (sim.next_entry()..w.entries.len())
        .take_while(|&i| w.entries[i].time_s <= now + DUE_WINDOW_S)
        .filter(|&i| !w.entries[i].on_demand);
    for i in waiting.chain(coming) {
        let e = &w.entries[i];
        let sec = match e.start {
            EntryStart::Boundary(n) => net.segments[net.nodes[n.idx()].segments[0].idx()].section,
            EntryStart::At(p) => net.segments[p.segment.idx()].section,
        };
        if !in_area(sec) {
            continue;
        }
        let svc = &w.services[e.service.idx()];
        rows.entry(svc.headcode.clone()).or_insert_with(|| row(svc.calls.first(), now, TrainState::Due));
    }
    rows
}
