#![allow(dead_code)]

use std::collections::BTreeMap;

use signalbox_core::ids::AreaId;
use signalbox_core::world::World;

pub const TWOBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/twobox.json");

pub fn twobox_json() -> String {
    std::fs::read_to_string(TWOBOX).unwrap()
}

pub fn twobox() -> World {
    World::from_json(&twobox_json()).unwrap()
}

pub fn area(w: &World, name: &str) -> AreaId {
    w.net.area(name).unwrap_or_else(|| panic!("no area {name}"))
}

pub fn map<V: Clone>(pairs: &[(&str, V)]) -> BTreeMap<String, V> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

/// Liverpool Street converted from TS2 and split by its shipped area file.
pub fn liverpool_json() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/liverpool-st.json")).unwrap();
    let text = std::fs::read_to_string(format!("{dir}/../../layouts/liverpool-st.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&text).unwrap()).unwrap();
    serde_json::to_string(&w).unwrap()
}
