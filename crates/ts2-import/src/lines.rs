//! Line names for converted worlds, from an optional hand-made per-layout
//! file (realism spec §2.1): each named stretch of line becomes one label in
//! the world's `layout`, with an arrow for the direction trains run on it.
//! Names are opaque; unknown ones are hard errors, as in the areas file.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{Value, json};
use signalbox_core::network::Dir;
use signalbox_core::world::file::WorldFile;

/// How far above the end of its stretch a line's label sits, in layout units.
pub const LABEL_ABOVE: f64 = 12.0;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineSpec {
    /// Shown as it is, e.g. `DOWN MAIN`.
    pub name: String,
    /// The world's direction of travel on this line (as its signals' `direction`).
    pub direction: Dir,
    /// Signal or section names along the line.
    pub through: Vec<String>,
}

/// Names quoted, so names containing commas stay readable.
fn list(names: &[String]) -> String {
    names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LinesError {
    #[error("not a lines file: {0}")]
    Parse(String),
    #[error("lines without a name or without `through` names")]
    Empty,
    #[error("unknown names: {}", list(.0))]
    UnknownNames(Vec<String>),
    #[error("signals facing against their line's direction: {}", list(.0))]
    WrongDirection(Vec<String>),
    #[error("the world has no drawing to label")]
    NoDrawing,
    #[error("lines with no drawn track: {}", list(.0))]
    Undrawn(Vec<String>),
}

pub fn parse(json: &str) -> Result<Vec<LineSpec>, LinesError> {
    serde_json::from_str(json).map_err(|e| LinesError::Parse(e.to_string()))
}

#[derive(Deserialize)]
struct DrawnLine {
    segment: String,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
}

/// Add one label per line to the world's `layout` labels: at the end of
/// the stretch trains run towards, `LABEL_ABOVE` units above it, with
/// `arrow` the direction of travel on screen (`[1, 0]` right, `[-1, 0]`
/// left). All or nothing.
pub fn apply(world: &mut WorldFile, lines: &[LineSpec]) -> Result<(), LinesError> {
    if lines.iter().any(|l| l.name.trim().is_empty() || l.through.is_empty()) {
        return Err(LinesError::Empty);
    }
    let drawn: Vec<DrawnLine> = match world.layout.get("lines") {
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| LinesError::NoDrawing)?,
        None => return Err(LinesError::NoDrawing),
    };
    let sections: BTreeSet<&str> = world.sections.iter().map(|s| s.name.as_str()).collect();
    let seg_section: BTreeMap<&str, &str> =
        world.segments.iter().map(|g| (g.name.as_str(), g.section.as_str())).collect();
    let signals: BTreeMap<&str, (&str, Dir)> =
        world.signals.iter().map(|s| (s.name.as_str(), (s.segment.as_str(), s.direction))).collect();
    let (mut unknown, mut against, mut undrawn) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    let mut labels = Vec::new();
    for line in lines {
        let mut secs: BTreeSet<&str> = BTreeSet::new();
        for name in &line.through {
            if let Some(s) = sections.get(name.as_str()) {
                secs.insert(s);
            } else if let Some(&(seg, dir)) = signals.get(name.as_str()) {
                if dir != line.direction {
                    against.insert(name.clone());
                }
                if let Some(s) = seg_section.get(seg) {
                    secs.insert(s);
                }
            } else {
                unknown.insert(name.clone());
            }
        }
        let stretch: Vec<&DrawnLine> =
            drawn.iter().filter(|d| seg_section.get(d.segment.as_str()).is_some_and(|s| secs.contains(s))).collect();
        if stretch.is_empty() {
            undrawn.insert(line.name.clone());
            continue;
        }
        // Up runs from x1 (the segment's `from` node) to x2.
        let dx: f64 = stretch
            .iter()
            .map(|d| match line.direction {
                Dir::Up => d.x2 - d.x1,
                Dir::Down => d.x1 - d.x2,
            })
            .sum();
        let right = dx >= 0.0;
        let ends = stretch.iter().flat_map(|d| [(d.x1, d.y1), (d.x2, d.y2)]);
        let (x, y) = ends
            .reduce(|a, b| if (right && b.0 > a.0) || (!right && b.0 < a.0) { b } else { a })
            .expect("a stretch has lines");
        let arrow = if right { [1.0, 0.0] } else { [-1.0, 0.0] };
        labels.push(json!({"text": line.name, "x": x, "y": y - LABEL_ABOVE, "arrow": arrow}));
    }
    if !unknown.is_empty() {
        return Err(LinesError::UnknownNames(unknown.into_iter().collect()));
    }
    if !against.is_empty() {
        return Err(LinesError::WrongDirection(against.into_iter().collect()));
    }
    if !undrawn.is_empty() {
        return Err(LinesError::Undrawn(undrawn.into_iter().collect()));
    }
    let Some(layout) = world.layout.as_object_mut() else { return Err(LinesError::NoDrawing) };
    match layout.entry("labels").or_insert_with(|| Value::Array(vec![])) {
        Value::Array(all) => all.extend(labels),
        _ => return Err(LinesError::NoDrawing),
    }
    Ok(())
}
