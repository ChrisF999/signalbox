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

    pub fn cancel_route(
        &mut self,
        w: &World,
        pts: &PointsTable,
        occ: &Occupancy,
        r: RouteId,
    ) -> Result<Vec<Event>, Rejection> {
        let def = &w.routes[r.idx()];
        if def.automatic {
            return Err(Rejection::RouteIsAutomatic);
        }
        let st = &self.routes[r.idx()];
        if st.state == RouteState::Idle || st.cancel.is_some() {
            return Err(Rejection::RouteNotSet);
        }
        let sig = &w.net.signals[def.entrance.idx()];
        let approaching = w.net.sections_in_rear(sig.at, sig.sighting_m, pts).iter().any(|&s| occ.occupied(s))
            || def.path.iter().any(|&s| occ.occupied(s));
        let st = &mut self.routes[r.idx()];
        st.auto_working = false;
        st.cancel = Some(if approaching { w.options.approach_lock_s } else { 0.0 });
        Ok(vec![Event::RouteCancelled { route: r, approach_locked: approaching }])
    }

    pub fn set_auto_working(&mut self, r: RouteId, on: bool) -> Result<Vec<Event>, Rejection> {
        let st = &mut self.routes[r.idx()];
        if st.state == RouteState::Idle || st.cancel.is_some() {
            return Err(Rejection::RouteNotSet);
        }
        st.auto_working = on;
        Ok(vec![Event::AutoWorking { route: r, on }])
    }

    /// Advance route states by `dt`: locking, sectional release, overlap
    /// release, approach-locking timers, auto-working, invariant checks.
    pub fn update(&mut self, w: &World, pts: &PointsTable, occ: &Occupancy, dt: f64) -> Vec<Event> {
        let mut ev = Vec::new();
        for i in 0..w.routes.len() {
            if self.routes[i].state == RouteState::Idle {
                continue;
            }
            let r = RouteId::from_idx(i);
            let def = &w.routes[i];
            let auto = def.automatic || self.routes[i].auto_working;

            let detected = def.all_points().all(|&(p, pos)| pts.detected(p) == Some(pos));
            if self.routes[i].state == RouteState::Setting && detected {
                self.routes[i].state = RouteState::Locked;
                ev.push(Event::RouteLocked { route: r });
            }

            let releasing = match self.routes[i].cancel.as_mut() {
                Some(left) => {
                    *left -= dt;
                    *left <= 0.0
                }
                None => false,
            };

            // Sectional release: occupy then clear, in running order.
            for k in 0..def.path.len() {
                let s = def.path[k];
                let occupied = occ.occupied(s);
                let rear_touched = k == 0 || self.routes[i].progress[k - 1] != Progress::Untouched;
                let was = self.routes[i].progress[k];
                let now = match was {
                    Progress::Untouched if occupied && (rear_touched || releasing) => Progress::Occupied,
                    Progress::Occupied if !occupied => {
                        if auto && !releasing {
                            Progress::Untouched
                        } else {
                            Progress::Released
                        }
                    }
                    p => p,
                };
                self.routes[i].progress[k] = now;
                if now == Progress::Released && was != Progress::Released {
                    self.release_path_section(w, r, s);
                }
            }
            // A cancelled route with a train on it keeps the sections ahead of
            // the train and its overlap; they go as the train runs on.
            let holding = releasing && self.routes[i].progress.contains(&Progress::Occupied);
            if releasing && !holding {
                for k in 0..def.path.len() {
                    if self.routes[i].progress[k] == Progress::Untouched {
                        self.routes[i].progress[k] = Progress::Released;
                        self.release_path_section(w, r, def.path[k]);
                    }
                }
            }

            // Overlap release.
            let owns_overlap = def.overlap.iter().any(|&s| self.owner[s.idx()] == Some(Owner::Overlap(r)));
            if owns_overlap && !auto {
                let last = def.path.len() - 1;
                let standing =
                    self.routes[i].progress[last] == Progress::Occupied && occ.stationary(def.path[last]);
                self.routes[i].overlap_stood_s = if standing { self.routes[i].overlap_stood_s + dt } else { 0.0 };
                let path_done = self.routes[i].progress.iter().all(|&p| p == Progress::Released);
                if (releasing && !holding) || path_done || self.routes[i].overlap_stood_s >= w.options.overlap_release_s {
                    for &s in &def.overlap {
                        if self.owner[s.idx()] == Some(Owner::Overlap(r)) {
                            self.owner[s.idx()] = None;
                        }
                    }
                    ev.push(Event::OverlapReleased { route: r });
                }
            }
            // An auto-working route takes its overlap back once it is free again.
            if auto && !releasing && self.routes[i].state == RouteState::Locked {
                for &s in &def.overlap {
                    if self.owner[s.idx()].is_none() {
                        self.owner[s.idx()] = Some(Owner::Overlap(r));
                    }
                }
            }

            // Invariant: points under a locked route stay where the route needs them.
            if self.routes[i].state == RouteState::Locked {
                for &(p, pos) in def.all_points() {
                    let sec = w.net.points_section(p).expect("route points are validated at load");
                    if self.owner[sec.idx()].map(Owner::route) == Some(r) && pts.detected(p) != Some(pos) {
                        ev.push(Event::InvariantViolated {
                            what: format!(
                                "route {} is locked but points {} are not {:?}",
                                def.name,
                                w.net.nodes[p.idx()].name,
                                pos
                            ),
                        });
                    }
                }
            }

            // Fully released?
            let overlap_held = def.overlap.iter().any(|&s| self.owner[s.idx()] == Some(Owner::Overlap(r)));
            let path_done = self.routes[i].progress.iter().all(|&p| p == Progress::Released);
            if path_done && !overlap_held {
                self.routes[i] = RouteStatus::idle(def.path.len());
                ev.push(Event::RouteReleased { route: r });
            }
        }
        ev
    }

    /// Let go of path section `s` of route `r`. If a route still set behind
    /// `r` (one whose exit signal is `r`'s entrance) has `s` as its overlap,
    /// the section goes back to that route instead of becoming free.
    fn release_path_section(&mut self, w: &World, r: RouteId, s: SectionId) {
        if self.owner[s.idx()] != Some(Owner::Path(r)) {
            return;
        }
        let entrance = w.routes[r.idx()].entrance;
        let rear = (0..w.routes.len()).find(|&x| {
            x != r.idx()
                && self.routes[x].state != RouteState::Idle
                && w.routes[x].overlap.contains(&s)
                && w.routes[x].exit == Exit::Signal(entrance)
        });
        self.owner[s.idx()] = rear.map(|x| Owner::Overlap(RouteId::from_idx(x)));
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
