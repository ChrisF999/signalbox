//! Signalling areas for converted worlds, from a hand-made per-layout file
//! (spec §5). Each area floods the section graph from its seeds; the flood
//! never crosses a node where a boundary signal stands. Names are opaque.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use signalbox_core::world::file::{AreaFile, WorldFile};
use signalbox_core::world::{LoadError, World};

pub const AREAS_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AreasFile {
    pub schema: u32,
    #[serde(default)]
    pub boundaries: Vec<String>,
    pub areas: Vec<AreaSpec>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AreaSpec {
    pub name: String,
    pub seeds: Vec<String>,
}

/// What one area ended up with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaCount {
    pub name: String,
    pub sections: usize,
    pub signals: usize,
}

/// Names quoted, so names containing commas stay readable.
fn list(names: &[String]) -> String {
    names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AreasError {
    #[error("not an areas file: {0}")]
    Parse(String),
    #[error("unsupported areas schema {0} (expected {expected})", expected = AREAS_SCHEMA)]
    Schema(u32),
    #[error("no areas listed")]
    NoAreas,
    #[error("duplicate area names: {}", list(.0))]
    DuplicateAreas(Vec<String>),
    #[error("areas without seeds: {}", list(.0))]
    NoSeeds(Vec<String>),
    #[error("unknown names: {}", list(.0))]
    UnknownNames(Vec<String>),
    #[error("boundaries that are not signals: {}", list(.0))]
    NotSignals(Vec<String>),
    #[error("boundary signals not at a segment end: {}", list(.0))]
    NotAtSegmentEnd(Vec<String>),
    #[error("sections reached by more than one area: {}", list(.0))]
    DoublyReached(Vec<String>),
    #[error("sections reached by no area: {}", list(.0))]
    Unreached(Vec<String>),
    #[error("the world with areas does not load: {0}")]
    Invalid(LoadError),
}

pub fn parse(json: &str) -> Result<AreasFile, AreasError> {
    let f: AreasFile = serde_json::from_str(json).map_err(|e| AreasError::Parse(e.to_string()))?;
    if f.schema != AREAS_SCHEMA {
        return Err(AreasError::Schema(f.schema));
    }
    Ok(f)
}

/// Rewrite the world's areas, each section's area and each signal's area
/// (the area of the section its segment belongs to). All or nothing.
pub fn apply(world: &mut WorldFile, spec: &AreasFile) -> Result<Vec<AreaCount>, AreasError> {
    let owner = assign(world, spec)?;
    let mut out = world.clone();
    out.areas = spec.areas.iter().map(|a| AreaFile { name: a.name.clone() }).collect();
    let mut counts: Vec<AreaCount> =
        spec.areas.iter().map(|a| AreaCount { name: a.name.clone(), sections: 0, signals: 0 }).collect();
    for s in &mut out.sections {
        let i = owner[&s.name];
        s.area = spec.areas[i].name.clone();
        counts[i].sections += 1;
    }
    let seg_section: BTreeMap<&str, &str> =
        world.segments.iter().map(|g| (g.name.as_str(), g.section.as_str())).collect();
    for s in &mut out.signals {
        let sec = seg_section.get(s.segment.as_str()).ok_or_else(|| {
            AreasError::Invalid(LoadError::UnknownRef { kind: "segment", name: s.segment.clone(), from: s.name.clone() })
        })?;
        let i = owner[*sec];
        s.area = spec.areas[i].name.clone();
        counts[i].signals += 1;
    }
    World::from_file(out.clone()).map_err(AreasError::Invalid)?;
    *world = out;
    Ok(counts)
}

/// Section name → index of the area that owns it.
fn assign(world: &WorldFile, spec: &AreasFile) -> Result<BTreeMap<String, usize>, AreasError> {
    if spec.areas.is_empty() {
        return Err(AreasError::NoAreas);
    }
    let mut seen = BTreeSet::new();
    let dup: BTreeSet<String> =
        spec.areas.iter().filter(|a| !seen.insert(a.name.as_str())).map(|a| a.name.clone()).collect();
    if !dup.is_empty() {
        return Err(AreasError::DuplicateAreas(dup.into_iter().collect()));
    }
    let seedless: BTreeSet<String> =
        spec.areas.iter().filter(|a| a.seeds.is_empty()).map(|a| a.name.clone()).collect();
    if !seedless.is_empty() {
        return Err(AreasError::NoSeeds(seedless.into_iter().collect()));
    }

    let sections: BTreeSet<&str> = world.sections.iter().map(|s| s.name.as_str()).collect();
    let segs: BTreeMap<&str, (&str, &str, f64, &str)> = world
        .segments
        .iter()
        .map(|g| (g.name.as_str(), (g.from.as_str(), g.to.as_str(), g.length_m, g.section.as_str())))
        .collect();
    let signals: BTreeMap<&str, (&str, f64)> =
        world.signals.iter().map(|s| (s.name.as_str(), (s.segment.as_str(), s.offset_m))).collect();
    let berths: BTreeMap<&str, (Option<&str>, Option<&str>)> = world
        .berths
        .iter()
        .map(|b| (b.name.as_str(), (b.signal.as_deref(), b.boundary.as_deref())))
        .collect();
    let mut node_secs: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for g in &world.segments {
        node_secs.entry(g.from.as_str()).or_default().insert(g.section.as_str());
        node_secs.entry(g.to.as_str()).or_default().insert(g.section.as_str());
    }
    let signal_section = |s: &str| signals.get(s).and_then(|(seg, _)| segs.get(seg)).map(|g| g.3);
    let resolve = |name: &str| {
        if let Some(s) = sections.get(name) {
            return Some(*s);
        }
        if signals.contains_key(name) {
            return signal_section(name);
        }
        match berths.get(name)? {
            (Some(sig), _) => signal_section(*sig),
            (None, Some(node)) => node_secs.get(*node)?.iter().next().copied(),
            (None, None) => None,
        }
    };

    let mut unknown: BTreeSet<String> = BTreeSet::new();
    let mut not_signals: BTreeSet<String> = BTreeSet::new();
    for b in &spec.boundaries {
        if signals.contains_key(b.as_str()) {
            continue;
        }
        if resolve(b.as_str()).is_some() {
            not_signals.insert(b.clone());
        } else {
            unknown.insert(b.clone());
        }
    }
    let mut seeds: Vec<Vec<&str>> = Vec::new();
    for a in &spec.areas {
        let mut v = Vec::new();
        for s in &a.seeds {
            match resolve(s.as_str()) {
                Some(sec) => v.push(sec),
                None => {
                    unknown.insert(s.clone());
                }
            }
        }
        seeds.push(v);
    }
    if !unknown.is_empty() {
        return Err(AreasError::UnknownNames(unknown.into_iter().collect()));
    }
    if !not_signals.is_empty() {
        return Err(AreasError::NotSignals(not_signals.into_iter().collect()));
    }

    let mut blocked: BTreeSet<&str> = BTreeSet::new();
    let mut off_end: BTreeSet<String> = BTreeSet::new();
    for b in &spec.boundaries {
        let (seg, offset) = signals[b.as_str()];
        let Some(&(from, to, len, _)) = segs.get(seg) else {
            return Err(AreasError::Invalid(LoadError::UnknownRef {
                kind: "segment",
                name: seg.to_string(),
                from: b.clone(),
            }));
        };
        if offset.abs() > 1e-6 && (offset - len).abs() > 1e-6 {
            off_end.insert(b.clone());
        }
        blocked.insert(if offset <= len / 2.0 { from } else { to });
    }
    if !off_end.is_empty() {
        return Err(AreasError::NotAtSegmentEnd(off_end.into_iter().collect()));
    }

    let mut adj: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (n, secs) in &node_secs {
        if blocked.contains(n) {
            continue;
        }
        for &a in secs {
            for &b in secs {
                if a != b {
                    adj.entry(a).or_default().insert(b);
                }
            }
        }
    }
    let mut owners: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
    for (i, start) in seeds.iter().enumerate() {
        let mut reached: BTreeSet<&str> = start.iter().copied().collect();
        let mut stack: Vec<&str> = reached.iter().copied().collect();
        while let Some(c) = stack.pop() {
            for &d in adj.get(c).into_iter().flatten() {
                if reached.insert(d) {
                    stack.push(d);
                }
            }
        }
        for c in reached {
            owners.entry(c).or_default().insert(i);
        }
    }
    let double: BTreeSet<String> = world
        .sections
        .iter()
        .filter(|s| owners.get(s.name.as_str()).is_some_and(|o| o.len() > 1))
        .map(|s| s.name.clone())
        .collect();
    if !double.is_empty() {
        return Err(AreasError::DoublyReached(double.into_iter().collect()));
    }
    let unreached: BTreeSet<String> =
        world.sections.iter().filter(|s| !owners.contains_key(s.name.as_str())).map(|s| s.name.clone()).collect();
    if !unreached.is_empty() {
        return Err(AreasError::Unreached(unreached.into_iter().collect()));
    }
    Ok(owners
        .into_iter()
        .map(|(s, o)| (s.to_string(), *o.iter().next().expect("reached sections have an owner")))
        .collect())
}
