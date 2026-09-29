//! Route setting, locking, release and signal aspects.

use serde::{Deserialize, Serialize};

use crate::aspect::{cleared_aspect, Aspect};
use crate::events::{Event, Rejection};
use crate::ids::*;
use crate::network::NodeKind;
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

    /// Whether route `r`'s entrance signal may show a proceed aspect.
    pub fn proceed(&self, w: &World, pts: &PointsTable, occ: &Occupancy, r: RouteId) -> bool {
        let def = &w.routes[r.idx()];
        let st = &self.routes[r.idx()];
        let exit_signal = match def.exit {
            Exit::Signal(s) => Some(s),
            Exit::Node(_) => None,
        };
        let overlap_held = |s: SectionId| match self.owner[s.idx()] {
            Some(Owner::Overlap(x)) if x == r => true,
            // Taken over by a route continuing from our exit signal.
            Some(o) => exit_signal.is_some() && Some(w.routes[o.route().idx()].entrance) == exit_signal,
            None => false,
        };
        st.state == RouteState::Locked
            && st.cancel.is_none()
            && st.progress.iter().all(|&p| p == Progress::Untouched)
            && def.path.iter().all(|&s| !occ.occupied(s) && self.owner[s.idx()] == Some(Owner::Path(r)))
            && def.overlap.iter().all(|&s| !occ.occupied(s) && overlap_held(s))
            && def.all_points().all(|&(p, pos)| pts.detected(p) == Some(pos))
    }

    pub fn compute_aspects(&self, w: &World, pts: &PointsTable, occ: &Occupancy) -> Vec<Aspect> {
        let n = w.net.signals.len();
        let mut memo = vec![None; n];
        let mut visiting = vec![false; n];
        for s in 0..n {
            self.aspect_of(w, pts, occ, SignalId::from_idx(s), &mut memo, &mut visiting);
        }
        memo.into_iter().map(|a| a.expect("every signal computed")).collect()
    }

    fn aspect_of(
        &self,
        w: &World,
        pts: &PointsTable,
        occ: &Occupancy,
        s: SignalId,
        memo: &mut Vec<Option<Aspect>>,
        visiting: &mut Vec<bool>,
    ) -> Aspect {
        if let Some(a) = memo[s.idx()] {
            return a;
        }
        if visiting[s.idx()] {
            // A loop of cleared signals: treat the far end as clear.
            return Aspect::Green;
        }
        visiting[s.idx()] = true;
        let a = match self.active_route_from(w, s) {
            Some(r) if self.proceed(w, pts, occ, r) => {
                let exit = match w.routes[r.idx()].exit {
                    Exit::Signal(e) => self.aspect_of(w, pts, occ, e, memo, visiting),
                    Exit::Node(n) if w.net.nodes[n.idx()].kind == NodeKind::Boundary => Aspect::Green,
                    Exit::Node(_) => Aspect::Red,
                };
                cleared_aspect(w.net.signals[s.idx()].aspects, exit)
            }
            _ => Aspect::Red,
        };
        visiting[s.idx()] = false;
        memo[s.idx()] = Some(a);
        a
    }

    /// Recompute all aspects; returns events for the ones that changed.
    pub fn refresh_aspects(&mut self, w: &World, pts: &PointsTable, occ: &Occupancy) -> Vec<Event> {
        let new = self.compute_aspects(w, pts, occ);
        let ev = self
            .aspects
            .iter()
            .zip(new.iter())
            .enumerate()
            .filter(|(_, (old, a))| old != a)
            .map(|(i, (_, &a))| Event::SignalAspect { signal: SignalId::from_idx(i), aspect: a })
            .collect();
        self.aspects = new;
        ev
    }
}
