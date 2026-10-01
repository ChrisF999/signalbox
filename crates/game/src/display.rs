//! Display data for clients (realism spec §2.1, §3), built once per game
//! from the world alone: the box prefix, each area's workstation letter,
//! and the simplifier for each area and for spectators.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use protocol::{Layout, SimplifierCall, SimplifierRow};
use signalbox_core::ids::{AreaId, SectionId};
use signalbox_core::timetable::Call;
use signalbox_core::world::World;

/// The first ASCII letter of `title`, as a capital; empty if it has none.
pub fn default_box_prefix(title: &str) -> String {
    title.chars().find(char::is_ascii_alphabetic).map(|c| c.to_ascii_uppercase().to_string()).unwrap_or_default()
}

/// A, B, C… for areas 0, 1, 2…; nothing past Z.
pub fn default_workstation(i: usize) -> String {
    u8::try_from(i).ok().filter(|&i| i < 26).map(|i| char::from(b'A' + i).to_string()).unwrap_or_default()
}

fn capitals(s: &str, len: std::ops::RangeInclusive<usize>) -> bool {
    len.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_uppercase())
}

/// The box prefix and workstation letters from the world's `layout` JSON
/// (`box_prefix`, `workstations`, written by `ts2-import --areas`), each
/// falling back to its default when missing or malformed: older worlds and
/// saves, and hand-made worlds, simply have none.
pub fn prefixes(w: &World) -> (String, BTreeMap<String, String>) {
    let box_prefix = match w.layout.get("box_prefix").and_then(|v| v.as_str()) {
        Some(p) if p.is_empty() || capitals(p, 1..=3) => p.to_string(),
        _ => default_box_prefix(&w.title),
    };
    let given = w.layout.get("workstations");
    let workstations = w
        .net
        .areas
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let letter = match given.and_then(|g| g.get(&a.name)).and_then(|v| v.as_str()) {
                Some(l) if capitals(l, 1..=1) => l.to_string(),
                _ => default_workstation(i),
            };
            (a.name.clone(), letter)
        })
        .collect();
    (box_prefix, workstations)
}

fn call_time(c: &SimplifierCall) -> Option<f64> {
    c.arr.or(c.dep)
}

/// Every service with a call in `area` (a spectator: every service, every
/// call), in running order: by the time of the first listed call (untimed
/// last), then headcode, then world order.
pub fn simplifier(w: &World, area: Option<AreaId>) -> Vec<SimplifierRow> {
    let net = &w.net;
    let in_area = |s: SectionId| area.is_none_or(|a| net.sections[s.idx()].area == a);
    let listed = |c: &Call| {
        area.is_none()
            || net.platforms.iter().any(|p| {
                p.place == c.place
                    && c.platform.as_deref().is_none_or(|pf| p.platform == pf)
                    && in_area(net.segments[p.segment.idx()].section)
            })
    };
    let mut rows: Vec<SimplifierRow> = w
        .services
        .iter()
        .filter_map(|svc| {
            let calls: Vec<SimplifierCall> = svc
                .calls
                .iter()
                .filter(|c| listed(c))
                .map(|c| SimplifierCall {
                    place: c.place.clone(),
                    platform: c.platform.clone(),
                    arr: c.arr_s,
                    dep: c.dep_s,
                    stops: c.stop,
                })
                .collect();
            (area.is_none() || !calls.is_empty()).then(|| SimplifierRow {
                headcode: svc.headcode.clone(),
                origin: svc.calls.first().map(|c| c.place.clone()),
                destination: svc.calls.last().map(|c| c.place.clone()),
                calls,
            })
        })
        .collect();
    let first = |r: &SimplifierRow| r.calls.iter().find_map(call_time);
    // A stable sort: services equal in time and headcode keep world order.
    rows.sort_by(|a, b| {
        match (first(a), first(b)) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then_with(|| a.headcode.cmp(&b.headcode))
    });
    rows
}

/// What every layout of one game shares, per area.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    pub box_prefix: String,
    pub workstations: BTreeMap<String, String>,
    /// Headcode → display headcode, where they differ.
    pub display_headcodes: BTreeMap<String, String>,
    /// Place code → name (polish spec M2).
    pub places: BTreeMap<String, String>,
    spectator: Vec<SimplifierRow>,
    by_area: Vec<Vec<SimplifierRow>>,
}

/// Every service whose display headcode differs from its headcode.
pub fn display_headcodes(w: &World) -> BTreeMap<String, String> {
    w.services.iter().filter(|s| s.display != s.headcode).map(|s| (s.headcode.clone(), s.display.clone())).collect()
}

/// Place code → name, from the world's `layout` JSON (`places`, written by
/// ts2-import); empty when missing, and entries that are not text are skipped.
pub fn places(w: &World) -> BTreeMap<String, String> {
    let Some(m) = w.layout.get("places").and_then(|v| v.as_object()) else { return BTreeMap::new() };
    m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()
}

impl Display {
    pub fn from_world(w: &World) -> Display {
        let (box_prefix, workstations) = prefixes(w);
        let by_area = (0..w.net.areas.len()).map(|a| simplifier(w, Some(AreaId::from_idx(a)))).collect();
        Display { box_prefix, workstations, display_headcodes: display_headcodes(w), places: places(w), spectator: simplifier(w, None), by_area }
    }

    /// The simplifier for `area` (a spectator's for `None`).
    pub fn simplifier(&self, area: Option<AreaId>) -> &[SimplifierRow] {
        match area {
            Some(a) => self.by_area.get(a.idx()).map_or(&[], Vec::as_slice),
            None => &self.spectator,
        }
    }

    /// Put the display data for a player of `area` into their layout.
    pub fn fill(&self, l: &mut Layout, area: Option<AreaId>) {
        l.box_prefix = self.box_prefix.clone();
        l.workstations = self.workstations.clone();
        l.simplifier = self.simplifier(area).to_vec();
        l.display_headcodes = self.display_headcodes.clone();
        l.places = self.places.clone();
    }
}
