//! Display names (realism spec §2, owner decision 11): a signal is shown as
//! `<box><workstation><name>` (`LA9`, `LB72`), without the workstation
//! letter on a single-area layout (`L9`). Display only: everything sent
//! keeps the plain name. Other names (berths, points, track, nodes) are
//! shown as they are.

use std::collections::BTreeMap;

use protocol::{ExitName, Layout};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names {
    /// Plain signal name → displayed name, for the signals the layout lists.
    signals: BTreeMap<String, String>,
    /// Area → workstation letter; empty on a single-area layout.
    workstations: BTreeMap<String, String>,
}

impl Names {
    pub fn new(l: &Layout) -> Names {
        let single = l.areas.len() <= 1;
        let letter = |area: &str| if single { "" } else { l.workstations.get(area).map_or("", String::as_str) };
        let signals = l.signals.iter().map(|s| (s.name.clone(), format!("{}{}{}", l.box_prefix, letter(&s.area), s.name))).collect();
        let workstations = if single { BTreeMap::new() } else { l.workstations.clone() };
        Names { signals, workstations }
    }

    /// How a signal is shown; a name the layout does not list stays plain.
    pub fn signal(&self, name: &str) -> String {
        self.signals.get(name).cloned().unwrap_or_else(|| name.to_string())
    }

    /// A route's exit: a signal as `signal` shows it, a node as it is.
    pub fn exit(&self, e: &ExitName) -> String {
        match e {
            ExitName::Signal(s) => self.signal(s),
            ExitName::Node(n) => n.clone(),
        }
    }

    /// An area's workstation letter; `None` on a single-area layout or when
    /// the server sent none.
    pub fn workstation(&self, area: &str) -> Option<&str> {
        self.workstations.get(area).map(String::as_str).filter(|l| !l.is_empty())
    }
}
