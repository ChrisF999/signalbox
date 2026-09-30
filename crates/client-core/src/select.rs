//! What clicks mean (spec D1 §3.2), as pure functions of the layout and
//! view: route setting by entrance then exit, the right-click menus, and
//! hover text. Only operable things (your own area) can be worked; the
//! fringe and spectators get hover text only.

use protocol::{Aspect, ExitName, Held, Layout, PlayerCommand, PointsPos, RouteInfo, RouteState, View};

use crate::text::{exit_text, pos_text};

/// What a left click on a signal or an exit node does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Click {
    /// Nothing selected yet and this can be an entrance: select it.
    Select(String),
    /// Send this (a `SetRoute`); the selection clears.
    Send(PlayerCommand),
    Clear,
    Ignore,
}

/// A right-click menu entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    pub label: String,
    pub cmd: PlayerCommand,
}

/// Operable routes from `entrance`, in layout order.
pub fn routes_from<'a>(l: &'a Layout, entrance: &'a str) -> impl Iterator<Item = &'a RouteInfo> + 'a {
    l.routes.iter().filter(move |r| r.operable && r.entrance == entrance)
}

/// A signal you can start a route from.
pub fn can_enter(l: &Layout, signal: &str) -> bool {
    routes_from(l, signal).next().is_some()
}

/// Where routes from `entrance` can end: the exits to light up.
pub fn exits_from(l: &Layout, entrance: &str) -> Vec<ExitName> {
    let mut out: Vec<ExitName> = Vec::new();
    for r in routes_from(l, entrance) {
        if !out.contains(&r.exit) {
            out.push(r.exit.clone());
        }
    }
    out
}

/// A left click on `target` (a signal, or a buffer stop / boundary node)
/// with `selected` as the chosen entrance, if any.
pub fn click(l: &Layout, selected: Option<&str>, target: &ExitName) -> Click {
    let as_entrance = |t: &ExitName| match t {
        ExitName::Signal(s) if can_enter(l, s) => Some(s.clone()),
        _ => None,
    };
    match selected {
        None => as_entrance(target).map_or(Click::Ignore, Click::Select),
        Some(e) if *target == ExitName::Signal(e.to_string()) => Click::Clear,
        Some(e) if exits_from(l, e).contains(target) => {
            Click::Send(PlayerCommand::SetRoute { entrance: e.to_string(), exit: target.clone() })
        }
        Some(_) => as_entrance(target).map_or(Click::Clear, Click::Select),
    }
}

/// Routes from `entrance` that are set (not idle) in the view.
fn active_from<'a>(l: &'a Layout, v: &'a View, entrance: &'a str) -> impl Iterator<Item = &'a RouteInfo> + 'a {
    l.routes.iter().filter(move |r| r.entrance == entrance && v.routes.contains_key(&r.name))
}

pub fn signal_menu(l: &Layout, v: &View, signal: &str) -> Vec<MenuItem> {
    if !l.signals.iter().any(|s| s.name == signal && s.operable) {
        return vec![];
    }
    let mut items = Vec::new();
    if let Some(r) = active_from(l, v, signal).next() {
        items.push(MenuItem {
            label: format!("Cancel route {signal} to {}", exit_text(&r.exit)),
            cmd: PlayerCommand::CancelRoute { entrance: signal.to_string() },
        });
    }
    if let Some(r) = active_from(l, v, signal).find(|r| r.automatic) {
        let on = v.routes.get(&r.name).is_some_and(|rv| rv.auto_working);
        items.push(MenuItem {
            label: format!("Auto-working {}", if on { "off" } else { "on" }),
            cmd: PlayerCommand::SetAutoWorking { entrance: signal.to_string(), on: !on },
        });
    }
    items
}

pub fn points_menu(l: &Layout, v: &View, points: &str) -> Vec<MenuItem> {
    if !l.points.iter().any(|p| p.name == points && p.operable) {
        return vec![];
    }
    let now = v.points.get(points).map_or(PointsPos::Normal, |p| p.position);
    let to = match now {
        PointsPos::Normal => PointsPos::Reverse,
        PointsPos::Reverse => PointsPos::Normal,
    };
    vec![MenuItem { label: format!("Swing {points} {}", pos_text(to)), cmd: PlayerCommand::SwingPoints { points: points.to_string(), to } }]
}

/// Cancelling a berth's headcode; interposing needs a headcode typed in,
/// so the screen offers it separately (`operable_berth`).
pub fn berth_menu(l: &Layout, v: &View, berth: &str) -> Vec<MenuItem> {
    if !operable_berth(l, berth) {
        return vec![];
    }
    match v.berths.get(berth) {
        Some(h) => vec![MenuItem { label: format!("Cancel {h}"), cmd: PlayerCommand::CancelBerth { berth: berth.to_string() } }],
        None => vec![],
    }
}

pub fn operable_berth(l: &Layout, berth: &str) -> bool {
    l.berths.iter().any(|b| b.name == berth && b.operable)
}

/// `Interpose` for a typed headcode (trimmed; the game checks its form).
pub fn interpose(berth: &str, typed: &str) -> Option<PlayerCommand> {
    let h = typed.trim();
    (!h.is_empty()).then(|| PlayerCommand::Interpose { berth: berth.to_string(), headcode: h.to_string() })
}

fn aspect_text(a: Aspect) -> &'static str {
    match a {
        Aspect::Red => "red",
        Aspect::Yellow => "yellow",
        Aspect::DoubleYellow => "double yellow",
        Aspect::Green => "green",
    }
}

fn area_note(l: &Layout, area: &str) -> String {
    if l.area.as_deref() == Some(area) { String::new() } else { format!(" ({area})") }
}

pub fn describe_signal(l: &Layout, v: &View, signal: &str) -> String {
    let Some(s) = l.signals.iter().find(|s| s.name == signal) else { return format!("Signal {signal}") };
    let mut out = format!("Signal {signal}{}: {}", area_note(l, &s.area), v.signals.get(signal).map_or("?", |a| aspect_text(*a)));
    for r in active_from(l, v, signal) {
        let rv = &v.routes[&r.name];
        let state = match rv.state {
            RouteState::Setting => "setting",
            RouteState::Locked => "set",
            RouteState::Cancelling => "cancelling",
        };
        out.push_str(&format!("; route to {} {state}", exit_text(&r.exit)));
        if rv.auto_working {
            out.push_str(", auto-working");
        }
    }
    out
}

pub fn describe_points(l: &Layout, v: &View, points: &str) -> String {
    let area = l.points.iter().find(|p| p.name == points).map(|p| area_note(l, &p.area)).unwrap_or_default();
    match v.points.get(points) {
        Some(p) => format!(
            "Points {points}{area}: {}{}{}",
            pos_text(p.position),
            if p.moving { ", moving" } else { "" },
            if p.locked { ", locked" } else { "" }
        ),
        None => format!("Points {points}{area}"),
    }
}

pub fn describe_berth(l: &Layout, v: &View, berth: &str) -> String {
    let area = l.berths.iter().find(|b| b.name == berth).map(|b| area_note(l, &b.area)).unwrap_or_default();
    format!("Berth {berth}{area}: {}", v.berths.get(berth).map_or("empty", String::as_str))
}

pub fn describe_section(l: &Layout, v: &View, section: &str) -> String {
    let area = l.sections.iter().find(|s| s.name == section).map(|s| area_note(l, &s.area)).unwrap_or_default();
    let state = match v.sections.get(section) {
        Some(s) if s.occupied => "occupied",
        Some(s) if s.held == Held::Path => "route set",
        Some(s) if s.held == Held::Overlap => "overlap",
        Some(_) => "clear",
        None => "?",
    };
    format!("Track {section}{area}: {state}")
}
