//! Warnings collected while converting: everything the target cannot express.

use std::collections::BTreeMap;

use serde::Serialize;

pub const ORPHANS: &str = "orphan track dropped";
pub const END_CLASS: &str = "track end classified";
pub const CROSSING: &str = "flat crossing merged";
pub const SIGNAL_TYPE: &str = "signal type approximated";
pub const BUFFER_NOT_AT_END: &str = "buffer signal not before an end";
pub const CALL_DROPPED: &str = "call dropped";
pub const ACTION: &str = "service action ignored";
pub const FORM_NO_REVERSE: &str = "form without reverse";
pub const EXIT_NO_BOUNDARY: &str = "exit service stabled";
pub const END_NO_STOP: &str = "end action changed to exit";
pub const TRAIN_SKIPPED: &str = "train skipped";
pub const DELAY: &str = "delay approximated";
pub const OPTIONS_IGNORED: &str = "options ignored";
pub const ROUTE_DROPPED: &str = "route dropped";
pub const ROUTE_SHORT: &str = "route ended early";
pub const ROUTE_PRESET: &str = "route pre-set ignored";
pub const ROUTE_GENERATED: &str = "route generated";
pub const SIGNAL_NO_ROUTE: &str = "signal without route";
pub const OVERLAP_CUT: &str = "overlap cut short";
pub const SIGNAL_OFF_BOUNDARY: &str = "signal not on a section boundary";
pub const AUTOMATIC_DEMOTED: &str = "automatic route demoted";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Warning {
    pub kind: &'static str,
    pub detail: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub warnings: Vec<Warning>,
}

impl Report {
    pub fn warn(&mut self, kind: &'static str, detail: impl Into<String>) {
        self.warnings.push(Warning { kind, detail: detail.into() });
    }

    pub fn count(&self, kind: &str) -> usize {
        self.warnings.iter().filter(|w| w.kind == kind).count()
    }

    /// Warning counts by kind.
    pub fn summary(&self) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for w in &self.warnings {
            *m.entry(w.kind).or_insert(0) += 1;
        }
        m
    }

    pub fn render(&self) -> String {
        let mut s = String::from("conversion report\n");
        for (k, n) in self.summary() {
            s += &format!("  {n:5}  {k}\n");
        }
        s += "\ndetails\n";
        for w in &self.warnings {
            s += &format!("  [{}] {}\n", w.kind, w.detail);
        }
        s
    }
}
