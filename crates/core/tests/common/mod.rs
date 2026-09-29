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
