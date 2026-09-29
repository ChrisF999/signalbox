//! Route setting, locking, release and signal aspects.

use serde::{Deserialize, Serialize};

use crate::aspect::Aspect;
use crate::events::{Event, Rejection};
use crate::ids::*;
use crate::occupancy::Occupancy;
use crate::points::PointsTable;
use crate::routes::Exit;
use crate::world::World;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RouteState {
    Idle,
    /// Sections claimed; waiting for points to be detected.
    Setting,
    Locked,
}

/// How far a train has got through one path section of a route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Progress {
    Untouched,
    Occupied,
    Released,
}

/// Which route holds a section, and whether as path or overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Owner {
    Path(RouteId),
    Overlap(RouteId),
}

impl Owner {
    pub fn route(self) -> RouteId {
        match self {
            Owner::Path(r) | Owner::Overlap(r) => r,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteStatus {
    pub state: RouteState,
    /// One entry per path section.
    pub progress: Vec<Progress>,
    pub auto_working: bool,
    /// Set when cancelled: seconds left before release (≤ 0 = releasing).
    pub cancel: Option<f64>,
    /// How long a train has stood at the exit signal (for overlap release).
    pub overlap_stood_s: f64,
}

impl RouteStatus {
    pub fn idle(path_len: usize) -> Self {
        RouteStatus {
            state: RouteState::Idle,
            progress: vec![Progress::Untouched; path_len],
            auto_working: false,
            cancel: None,
            overlap_stood_s: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Interlocking {
    pub routes: Vec<RouteStatus>,
    /// Per section.
    pub owner: Vec<Option<Owner>>,
    /// Per signal, as last computed by `refresh_aspects`.
    pub aspects: Vec<Aspect>,
}

impl Interlocking {
    pub fn new(w: &World) -> Self {
        Interlocking {
            routes: w.routes.iter().map(|r| RouteStatus::idle(r.path.len())).collect(),
            owner: vec![None; w.net.sections.len()],
            aspects: vec![Aspect::Red; w.net.signals.len()],
        }
    }

    /// The route currently set (or setting) from `entrance`, if any.
    pub fn active_route_from(&self, w: &World, entrance: SignalId) -> Option<RouteId> {
        w.routes_from[entrance.idx()].iter().copied().find(|r| self.routes[r.idx()].state != RouteState::Idle)
    }

    pub fn set_route(
        &mut self,
        w: &World,
        pts: &mut PointsTable,
        occ: &Occupancy,
        r: RouteId,
    ) -> Result<Vec<Event>, Rejection> {
        let def = &w.routes[r.idx()];
        if self.routes[r.idx()].state != RouteState::Idle {
            return Err(Rejection::AlreadySet);
        }
        if self.active_route_from(w, def.entrance).is_some() {
            return Err(Rejection::ConflictingRoute);
        }
        for &s in def.path.iter().chain(def.overlap.iter()) {
            match self.owner[s.idx()] {
                None => {}
                // The next route may take over the overlap of the route it continues.
                Some(Owner::Overlap(x)) if w.routes[x.idx()].exit == Exit::Signal(def.entrance) => {}
                Some(_) => return Err(Rejection::ConflictingRoute),
            }
        }
        for &(p, pos) in def.all_points() {
            if pts.detected(p) == Some(pos) {
                continue;
            }
            let sec = w.net.points_section(p).expect("route points are validated at load");
            if matches!(self.owner[sec.idx()], Some(o) if o.route() != r) {
                return Err(Rejection::PointsLocked);
            }
            if occ.occupied(sec) {
                return Err(Rejection::PointsOccupied);
            }
        }
        let mut ev = Vec::new();
        for &(p, pos) in def.all_points() {
            if pts.detected(p) != Some(pos) {
                pts.start_swing(p, pos, w.net.swing_s(p));
                ev.push(Event::PointsMoving { points: p, to: pos });
            }
        }
        for &s in &def.path {
            self.owner[s.idx()] = Some(Owner::Path(r));
        }
        for &s in &def.overlap {
            self.owner[s.idx()] = Some(Owner::Overlap(r));
        }
        let mut st = RouteStatus::idle(def.path.len());
        st.state = RouteState::Setting;
        self.routes[r.idx()] = st;
        ev.push(Event::RouteSetting { route: r });
        Ok(ev)
    }

    /// Advance route states by `dt`. (Task 8 extends this with release.)
    pub fn update(&mut self, w: &World, pts: &PointsTable, _occ: &Occupancy, _dt: f64) -> Vec<Event> {
        let mut ev = Vec::new();
        for i in 0..w.routes.len() {
            let detected = w.routes[i].all_points().all(|&(p, pos)| pts.detected(p) == Some(pos));
            if self.routes[i].state == RouteState::Setting && detected {
                self.routes[i].state = RouteState::Locked;
                ev.push(Event::RouteLocked { route: RouteId::from_idx(i) });
            }
        }
        ev
    }
}
