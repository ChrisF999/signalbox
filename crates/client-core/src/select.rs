//! What clicks mean (spec D1 §3.2), as pure functions of the layout and
//! view: route setting by entrance then exit, the right-click menus, and
//! hover text. Only operable things (your own area) can be worked; the
//! fringe and spectators get hover text only. Signals are named as the
//! screen shows them (`Names`).

use protocol::{Aspect, ExitName, Held, Layout, PlayerCommand, PointsPos, RouteInfo, RouteState, View};

use crate::names::Names;
use crate::text::pos_text;

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

/// Active routes from `entrance` that can still be cancelled or auto-worked.
fn live_from<'a>(l: &'a Layout, v: &'a View, entrance: &'a str) -> impl Iterator<Item = &'a RouteInfo> + 'a {
    active_from(l, v, entrance).filter(|r| v.routes[&r.name].state != RouteState::Cancelling)
}

pub fn signal_menu(l: &Layout, v: &View, signal: &str) -> Vec<MenuItem> {
    if !l.signals.iter().any(|s| s.name == signal && s.operable) {
        return vec![];
    }
    let names = Names::new(l);
    let mut items = Vec::new();
    if let Some(r) = live_from(l, v, signal).next() {
        items.push(MenuItem {
            label: format!("Cancel route {} to {}", names.signal(signal), names.exit(&r.exit)),
            cmd: PlayerCommand::CancelRoute { entrance: signal.to_string() },
        });
    }
    if !is_automatic(l, signal)
        && let Some(r) = live_from(l, v, signal).next()
    {
        let on = v.routes[&r.name].auto_working;
        items.push(MenuItem {
            label: format!("Auto-working {}", if on { "off" } else { "on" }),
            cmd: PlayerCommand::SetAutoWorking { entrance: signal.to_string(), on: !on },
        });
    }
    items
}

/// A permanently automatic signal: one with an automatic route from it. It
/// keeps its dashed post and gets no ○A.
pub fn is_automatic(l: &Layout, signal: &str) -> bool {
    l.routes.iter().any(|r| r.automatic && r.entrance == signal)
}

/// Whether `signal` has a ○A button beside it (realism spec decision 6): a
/// controlled signal that starts at least one route, in your own area. A
/// spectator sees every one (read-only); the fringe has none.
pub fn has_auto_button(l: &Layout, signal: &str) -> bool {
    let visible = l.signals.iter().any(|s| s.name == signal && l.area.as_ref().is_none_or(|mine| *mine == s.area));
    visible && !is_automatic(l, signal) && l.routes.iter().any(|r| r.entrance == signal)
}

/// What the ○A button beside `signal` sends when clicked: exactly the
/// signal menu's auto-working command, if it offers one (an operable,
/// controlled signal with a live route set from it).
pub fn auto_toggle(l: &Layout, v: &View, signal: &str) -> Option<PlayerCommand> {
    signal_menu(l, v, signal).into_iter().map(|m| m.cmd).find(|c| matches!(c, PlayerCommand::SetAutoWorking { .. }))
}

/// Whether the live route set from `signal` is auto-working (○A filled).
pub fn auto_working(l: &Layout, v: &View, signal: &str) -> bool {
    live_from(l, v, signal).any(|r| v.routes[&r.name].auto_working)
}

/// The ○A button's hover: on or off, or that a route must be set first;
/// and whose signal it is when not yours (as a signal's hover says), so a
/// dead click on it is explained.
pub fn describe_auto(l: &Layout, v: &View, signal: &str) -> String {
    let state = match live_from(l, v, signal).next() {
        None => "set a route first",
        Some(r) if v.routes[&r.name].auto_working => "on",
        Some(_) => "off",
    };
    let area = l.signals.iter().find(|s| s.name == signal).map(|s| area_note(l, &s.area)).unwrap_or_default();
    format!("Auto-working {}{area}: {state}", Names::new(l).signal(signal))
}

pub fn points_menu(l: &Layout, v: &View, points: &str) -> Vec<MenuItem> {
    if !l.points.iter().any(|p| p.name == points && p.operable) {
        return vec![];
    }
    let Some(pv) = v.points.get(points).filter(|p| !p.locked && !p.moving) else { return vec![] };
    let to = match pv.position {
        PointsPos::Normal => PointsPos::Reverse,
        PointsPos::Reverse => PointsPos::Normal,
    };
    vec![MenuItem { label: format!("Swing {points} {}", pos_text(to)), cmd: PlayerCommand::SwingPoints { points: points.to_string(), to } }]
}

/// How a headcode is shown (`Names::headcode`, without building `Names`).
fn shown_headcode<'a>(l: &'a Layout, h: &'a str) -> &'a str {
    l.display_headcodes.get(h).map_or(h, String::as_str)
}

/// Cancelling a berth's headcode; interposing needs a headcode typed in,
/// so the screen offers it separately (`operable_berth`).
pub fn berth_menu(l: &Layout, v: &View, berth: &str) -> Vec<MenuItem> {
    if !operable_berth(l, berth) {
        return vec![];
    }
    match v.berths.get(berth) {
        Some(h) => vec![MenuItem {
            label: format!("Cancel {}", shown_headcode(l, h)),
            cmd: PlayerCommand::CancelBerth { berth: berth.to_string() },
        }],
        None => vec![],
    }
}

pub fn operable_berth(l: &Layout, berth: &str) -> bool {
    l.berths.iter().any(|b| b.name == berth && b.operable)
}

/// `Interpose` for a typed headcode (trimmed; the game checks its form).
pub fn interpose(berth: &str, typed: &str) -> Option<PlayerCommand> {
    let h = typed.trim();
    // The game's rule: 1 to 10 ASCII letters or digits, case kept.
    ((1..=10).contains(&h.len()) && h.bytes().all(|b| b.is_ascii_alphanumeric())).then(|| PlayerCommand::Interpose { berth: berth.to_string(), headcode: h.to_string() })
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
    let names = Names::new(l);
    let Some(s) = l.signals.iter().find(|s| s.name == signal) else { return format!("Signal {signal}") };
    let aspect = v.signals.get(signal).map_or("?", |a| aspect_text(*a));
    let mut out = format!("Signal {}{}: {aspect}", names.signal(signal), area_note(l, &s.area));
    for r in active_from(l, v, signal) {
        let rv = &v.routes[&r.name];
        let state = match rv.state {
            RouteState::Setting => "setting",
            RouteState::Locked => "set",
            RouteState::Cancelling => "cancelling",
        };
        out.push_str(&format!("; route to {} {state}", names.exit(&r.exit)));
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
    let held = v.berths.get(berth).map_or("empty", |h| shown_headcode(l, h));
    format!("Berth {berth}{area}: {held}")
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
