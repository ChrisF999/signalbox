#![allow(dead_code)]

use signalbox_core::ids::*;
use signalbox_core::world::{LoadError, World};

pub fn fixture_json(name: &str) -> serde_json::Value {
    let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap()
}

/// Load a fixture after editing its JSON.
pub fn load_with(name: &str, edit: impl FnOnce(&mut serde_json::Value)) -> Result<World, LoadError> {
    let mut v = fixture_json(name);
    edit(&mut v);
    World::from_json(&v.to_string())
}

pub fn world(name: &str) -> World {
    load_with(name, |_| {}).unwrap()
}

pub fn sig(w: &World, name: &str) -> SignalId {
    w.net.signal(name).unwrap_or_else(|| panic!("no signal {name}"))
}

pub fn sec(w: &World, name: &str) -> SectionId {
    w.net.section(name).unwrap_or_else(|| panic!("no section {name}"))
}

pub fn node(w: &World, name: &str) -> NodeId {
    w.net.node(name).unwrap_or_else(|| panic!("no node {name}"))
}

pub fn seg(w: &World, name: &str) -> SegmentId {
    w.net.segment(name).unwrap_or_else(|| panic!("no segment {name}"))
}

pub fn route(w: &World, name: &str) -> RouteId {
    w.route_by_name(name).unwrap_or_else(|| panic!("no route {name}"))
}

use signalbox_core::occupancy::Occupancy;

/// Occupancy with one pretend train (TrainId 0) in each named section.
pub fn occ(w: &World, sections: &[&str], moving: bool) -> Occupancy {
    let mut o = Occupancy::new(w.net.sections.len());
    for s in sections {
        o.add(sec(w, s), TrainId(0), moving);
    }
    o
}

use signalbox_core::aspect::Aspect;
use signalbox_core::events::{Event, Rejection};
use signalbox_core::interlocking::Interlocking;
use signalbox_core::points::PointsTable;

/// A world plus interlocking and points, without trains.
pub struct Rig {
    pub w: World,
    pub pts: PointsTable,
    pub il: Interlocking,
}

impl Rig {
    pub fn new(name: &str) -> Rig {
        Rig::from_world(world(name))
    }

    pub fn from_world(w: World) -> Rig {
        let pts = PointsTable::new(&w.net);
        let il = Interlocking::new(&w);
        Rig { w, pts, il }
    }

    pub fn set(&mut self, name: &str, occ: &Occupancy) -> Result<Vec<Event>, Rejection> {
        let r = route(&self.w, name);
        self.il.set_route(&self.w, &mut self.pts, occ, r)
    }

    /// Run points and interlocking for `secs` seconds of sim time.
    pub fn run(&mut self, secs: f64, occ: &Occupancy) -> Vec<Event> {
        let mut ev = Vec::new();
        for _ in 0..(secs * 10.0).round() as usize {
            self.pts.tick(0.1);
            ev.extend(self.il.update(&self.w, &self.pts, occ, 0.1));
        }
        ev
    }

    pub fn aspect(&mut self, signal: &str, occ: &Occupancy) -> Aspect {
        self.il.refresh_aspects(&self.w, &self.pts, occ);
        self.il.aspects[sig(&self.w, signal).idx()]
    }

    pub fn empty(&self) -> Occupancy {
        Occupancy::new(self.w.net.sections.len())
    }
}

use signalbox_core::sim::Sim;

/// Step until an event matches `pred`; returns every event up to and
/// including that step. Panics if it never happens.
pub fn run_until(sim: &mut Sim, max_s: f64, mut pred: impl FnMut(&Event) -> bool) -> Vec<Event> {
    let mut all = Vec::new();
    for _ in 0..(max_s * 10.0).round() as u64 {
        let ev = sim.step();
        let hit = ev.iter().any(&mut pred);
        all.extend(ev);
        if hit {
            return all;
        }
    }
    let tail = &all[all.len().saturating_sub(10)..];
    panic!("condition not reached within {max_s} s; last events: {tail:?}");
}

pub fn count(ev: &[Event], pred: impl Fn(&Event) -> bool) -> usize {
    ev.iter().filter(|e| pred(e)).count()
}
