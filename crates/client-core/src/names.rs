//! Display names (realism spec §2, owner decision 11): a signal is shown as
//! `<box><workstation><name>` (`LA9`, `LB72`), without the workstation
//! letter on a single-area layout (`L9`). Display only: everything sent
//! keeps the plain name. Points are shown the same way with a `P` before
//! their number (`LAP153` for converted `N153`, `HP1` for a lesson's `P1`),
//! a berth by its signal's name, track only by the platform on it, and a
//! place code by its name where the layout gives one (polish spec M1, M2);
//! nodes are shown as they are. A headcode is shown by its service's display
//! headcode where the layout gives one (`Layout::display_headcodes`: a WTT
//! trip `301/1` shows `301`, polish spec P18 amended).

use std::collections::BTreeMap;

use protocol::{ExitName, Layout};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Names {
    /// Plain signal name → displayed name, for the signals the layout lists.
    signals: BTreeMap<String, String>,
    /// Area → workstation letter; empty on a single-area layout.
    workstations: BTreeMap<String, String>,
    /// Headcode → display headcode, where they differ.
    headcodes: BTreeMap<String, String>,
    /// Plain points name → displayed name.
    points: BTreeMap<String, String>,
    /// Berth → its signal's displayed name; `None` for a boundary berth.
    berths: BTreeMap<String, Option<String>>,
    /// Section → the platforms on it, as `place platform`.
    platforms: BTreeMap<String, Vec<String>>,
    /// Place code → name.
    places: BTreeMap<String, String>,
}

/// A points name's number for display: the digits after a leading `N` or
/// `P` (`N153`, `P1`), else the name itself; always with a `P` in front.
pub fn points_number(name: &str) -> String {
    let rest = name.strip_prefix('N').or_else(|| name.strip_prefix('P')).unwrap_or("");
    if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) { format!("P{rest}") } else { name.to_string() }
}

impl Names {
    pub fn new(l: &Layout) -> Names {
        let single = l.areas.len() <= 1;
        let letter = |area: &str| if single { "" } else { l.workstations.get(area).map_or("", String::as_str) };
        let signals = l.signals.iter().map(|s| (s.name.clone(), format!("{}{}{}", l.box_prefix, letter(&s.area), s.name))).collect();
        let workstations = if single { BTreeMap::new() } else { l.workstations.clone() };
        let points = l.points.iter().map(|p| (p.name.clone(), format!("{}{}{}", l.box_prefix, letter(&p.area), points_number(&p.name)))).collect();
        let signal = |n: &String| l.signals.iter().find(|s| s.name == *n).map(|s| format!("{}{}{}", l.box_prefix, letter(&s.area), s.name));
        let berths = l.berths.iter().map(|b| (b.name.clone(), b.signal.as_ref().and_then(signal))).collect();
        let place = |c: &str| l.places.get(c).map_or(c.to_string(), String::clone);
        let mut platforms: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for p in &l.platforms {
            if let Some(g) = l.segments.iter().find(|g| g.name == p.segment) {
                let text = format!("{} {}", place(&p.place), p.platform);
                let list = platforms.entry(g.section.clone()).or_default();
                if !list.contains(&text) {
                    list.push(text);
                }
            }
        }
        Names { signals, workstations, headcodes: l.display_headcodes.clone(), points, berths, platforms, places: l.places.clone() }
    }

    /// How points are shown (`LAP153`); a name the layout does not list stays plain.
    pub fn points(&self, name: &str) -> String {
        self.points.get(name).cloned().unwrap_or_else(|| name.to_string())
    }

    /// How a berth is shown: by its signal (`LA29`), `edge` for a boundary
    /// berth, plain for one the layout does not list.
    pub fn berth(&self, name: &str) -> String {
        match self.berths.get(name) {
            Some(Some(s)) => s.clone(),
            Some(None) => "edge".to_string(),
            None => name.to_string(),
        }
    }

    /// Track is never shown by its id: `Track at LIVERPOOL STREET 10`, or
    /// just `Track` where no platform is on it.
    pub fn track(&self, section: &str) -> String {
        match self.platforms.get(section) {
            Some(p) => format!("Track at {}", p.join(", ")),
            None => "Track".to_string(),
        }
    }

    /// A place's name (`LIVERPOOL STREET`), else its code.
    pub fn place<'a>(&'a self, code: &'a str) -> &'a str {
        self.places.get(code).map_or(code, String::as_str)
    }

    /// How a headcode is shown: its display headcode, else as it is.
    pub fn headcode<'a>(&'a self, headcode: &'a str) -> &'a str {
        shown_headcode(&self.headcodes, headcode)
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

/// How `headcode` is shown given a layout's `display_headcodes`: its
/// display headcode, else as it is (`Names::headcode` without building
/// `Names`).
pub fn shown_headcode<'a>(display: &'a BTreeMap<String, String>, headcode: &'a str) -> &'a str {
    display.get(headcode).map_or(headcode, String::as_str)
}
