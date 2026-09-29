//! Every file converts, loads, and produces the recorded warning counts.
//! To re-record after an intended change: UPDATE_EXPECTED=1 scripts/cargo test -p ts2-import --test convert

use std::collections::BTreeMap;

use signalbox_core::world::World;

fn data(name: &str) -> String {
    std::fs::read_to_string(format!("{}/tests/data/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn check(name: &str) {
    let c = ts2_import::convert(&data(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    World::from_file(c.world.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let got: BTreeMap<String, usize> = c.report.summary().into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    let path = format!("{}/tests/expected/{name}-summary.json", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("UPDATE_EXPECTED").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
        return;
    }
    let want: BTreeMap<String, usize> =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e} (record it)"))).unwrap();
    assert_eq!(got, want, "{name} warning counts changed:\n{}", c.report.render());
}

#[test]
fn mini() {
    check("mini");
}

#[test]
fn drain() {
    check("drain");
}

#[test]
fn liverpool_street() {
    check("liverpool-st");
}

#[test]
fn gretz() {
    check("gretz-armainvilliers");
}

#[test]
fn conversion_is_deterministic() {
    let a = serde_json::to_string(&ts2_import::convert(&data("liverpool-st")).unwrap().world).unwrap();
    let b = serde_json::to_string(&ts2_import::convert(&data("liverpool-st")).unwrap().world).unwrap();
    assert_eq!(a, b);
}
