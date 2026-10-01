//! The simplifier (realism spec §3): the area's timetable in running order,
//! shown like the working timetable — `HH:MM` with half minutes as `½`, a
//! platform column, `pass` for passing calls — with live lateness beside a
//! running train. And the headcode enquiry: one headcode's rows and state.

use std::cmp::Ordering;

use protocol::{Layout, SimplifierCall, SimplifierRow, TrainRow, TrainState, View};

use crate::text::train_state_text;

/// `HH:MM`, with `½` for a time 30 s or more past the minute (WTT style);
/// wraps at midnight; nonsense is `00:00`.
pub fn fmt_wtt(s: f64) -> String {
    let t = if s.is_finite() && s >= 0.0 { s.floor() as u64 % 86_400 } else { 0 };
    let half = if t % 60 >= 30 { "½" } else { "" };
    format!("{:02}:{:02}{half}", t / 3600, t / 60 % 60)
}

/// When the row's first listed call is booked (arrival, else departure).
pub fn first_time(r: &SimplifierRow) -> Option<f64> {
    r.calls.iter().find_map(|c| c.arr.or(c.dep))
}

/// When the row's last listed call is booked (departure, else arrival).
pub fn last_time(r: &SimplifierRow) -> Option<f64> {
    r.calls.iter().rev().find_map(|c| c.dep.or(c.arr))
}

/// The line the simplifier opens at (polish spec §7): the first line of the
/// first row not yet finished at `now_s` (its last booked call at or after
/// now, or no times at all), counting each row's lines as `lines` makes
/// them. Past the last line when every row has run.
pub fn now_line(rows: &[&SimplifierRow], now_s: f64) -> usize {
    rows.iter().take_while(|r| last_time(r).is_some_and(|t| t < now_s)).map(|r| r.calls.len().max(1)).sum()
}

/// Rows whose headcode or display headcode contains `search` (trimmed,
/// any case), in running order: by first call (untimed last), then
/// headcode, else as sent.
pub fn rows<'a>(l: &'a Layout, search: &str) -> Vec<&'a SimplifierRow> {
    let want = search.trim().to_ascii_uppercase();
    let matches = |h: &str| h.to_ascii_uppercase().contains(&want);
    let mut out: Vec<&SimplifierRow> = l
        .simplifier
        .iter()
        .filter(|r| matches(&r.headcode) || l.display_headcodes.get(&r.headcode).is_some_and(|d| matches(d)))
        .collect();
    out.sort_by(|a, b| {
        match (first_time(a), first_time(b)) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then_with(|| a.headcode.cmp(&b.headcode))
    });
    out
}

/// One line of the simplifier table: a row's first call carries its
/// headcode, origin and destination; later calls leave them blank.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub headcode: String,
    pub from: String,
    pub to: String,
    pub place: String,
    pub platform: String,
    pub arr: String,
    pub dep: String,
}

fn call_line(c: &SimplifierCall) -> (String, String) {
    let t = |v: Option<f64>| v.map(fmt_wtt).unwrap_or_default();
    if c.stops { (t(c.arr), t(c.dep)) } else { ("pass".to_string(), t(c.dep.or(c.arr))) }
}

pub fn lines(r: &SimplifierRow) -> Vec<Line> {
    if r.calls.is_empty() {
        return vec![Line {
            headcode: r.headcode.clone(),
            from: r.origin.clone().unwrap_or_default(),
            to: r.destination.clone().unwrap_or_default(),
            place: String::new(),
            platform: String::new(),
            arr: String::new(),
            dep: String::new(),
        }];
    }
    r.calls
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (arr, dep) = call_line(c);
            let head = |s: &Option<String>| if i == 0 { s.clone().unwrap_or_default() } else { String::new() };
            Line {
                headcode: if i == 0 { r.headcode.clone() } else { String::new() },
                from: head(&r.origin),
                to: head(&r.destination),
                place: c.place.clone(),
                platform: c.platform.clone().unwrap_or_default(),
                arr,
                dep,
            }
        })
        .collect()
}

/// `OT` under a minute late (or early), else whole minutes late: `3L`.
/// The one lateness style, in the simplifier and the train list (polish spec M6).
pub fn late_text(late_s: i64) -> String {
    if late_s >= 60 { format!("{}L", late_s / 60) } else { "OT".to_string() }
}

/// A running train's lateness, TRUST style: `OT` on time, `3L` three
/// minutes late; `None` when it is not running (not listed, or due).
pub fn lateness(v: Option<&View>, headcode: &str) -> Option<String> {
    let t = v?.trains.get(headcode)?;
    if t.state == TrainState::Due {
        return None;
    }
    Some(late_text(t.late_s))
}

/// The headcode a berth's text stands for: itself when it is a headcode
/// the layout or view knows; else (what a player interposed, e.g. a train
/// number) the one running train whose display headcode it is; else
/// itself.
pub fn resolve(l: &Layout, v: Option<&View>, text: &str) -> String {
    let known = l.simplifier.iter().any(|r| r.headcode == text) || v.is_some_and(|v| v.trains.contains_key(text));
    if !known {
        let mut running = v.into_iter().flat_map(|v| &v.trains).filter(|(h, r)| {
            r.state != TrainState::Due && l.display_headcodes.get(h.as_str()).is_some_and(|d| d == text)
        });
        if let (Some((h, _)), None) = (running.next(), running.next()) {
            return h.clone();
        }
    }
    text.to_string()
}

/// What the enquiry window shows for one headcode.
#[derive(Clone, Debug, PartialEq)]
pub struct Enquiry<'a> {
    pub headcode: String,
    /// Every simplifier row with this headcode (headcodes can repeat).
    pub rows: Vec<&'a SimplifierRow>,
    pub train: Option<&'a TrainRow>,
}

pub fn enquiry<'a>(l: &'a Layout, v: Option<&'a View>, headcode: &str) -> Enquiry<'a> {
    Enquiry {
        headcode: headcode.to_string(),
        rows: l.simplifier.iter().filter(|r| r.headcode == headcode).collect(),
        train: v.and_then(|v| v.trains.get(headcode)),
    }
}

impl Enquiry<'_> {
    /// `in area, 3L`, `due`, or `not in your train list`.
    pub fn live_text(&self) -> String {
        match self.train {
            Some(t) if t.state == TrainState::Due => train_state_text(t.state).to_string(),
            Some(t) => format!("{}, {}", train_state_text(t.state), late_text(t.late_s)),
            None => "not in your train list".to_string(),
        }
    }
}
