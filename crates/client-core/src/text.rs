//! Words for the screen: times, commands, refusals, notices, votes.
//! Signals are named as the screen shows them (`Names`).

use protocol::{ExitName, Notice, PlayerCommand, PointsPos, Preparing, Proposal, Rejection, TrainState, VoteView};

use crate::names::Names;

/// Seconds since midnight as `HH:MM:SS` (wrapping at 24 h; bad input is 00:00:00).
pub fn fmt_hms(s: f64) -> String {
    let t = if s.is_finite() && s >= 0.0 { s.floor() as u64 % 86_400 } else { 0 };
    format!("{:02}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
}

/// A game still being prepared (timetables spec §3.4), for the lobby and
/// the wait before its layout: "Preparing 05:40 to 07:30…" (the spec's
/// arrow has no glyph in egui's default fonts).
pub fn preparing_text(p: &Preparing) -> String {
    let hm = |s: f64| fmt_hms(s)[..5].to_string();
    format!("Preparing {} to {}…", hm(p.from), hm(p.to))
}

pub fn exit_text(e: &ExitName) -> &str {
    match e {
        ExitName::Signal(s) | ExitName::Node(s) => s,
    }
}

pub fn pos_text(p: PointsPos) -> &'static str {
    match p {
        PointsPos::Normal => "normal",
        PointsPos::Reverse => "reverse",
    }
}

pub fn command_text(c: &PlayerCommand, names: &Names) -> String {
    match c {
        PlayerCommand::SetRoute { entrance, exit } => {
            format!("set route {} to {}", names.signal(entrance), names.exit(exit))
        }
        PlayerCommand::CancelRoute { entrance } => format!("cancel route from {}", names.signal(entrance)),
        PlayerCommand::SetAutoWorking { entrance, on } => {
            format!("auto-working {} at {}", if *on { "on" } else { "off" }, names.signal(entrance))
        }
        PlayerCommand::SwingPoints { points, to } => format!("swing {} {}", names.points(points), pos_text(*to)),
        PlayerCommand::Interpose { berth, headcode } => format!("interpose {headcode} at {}", berth_words(names, berth)),
        PlayerCommand::CancelBerth { berth } => format!("cancel the headcode at {}", berth_words(names, berth)),
    }
}

/// A berth in a sentence: its signal's name, `the edge berth` at a boundary.
fn berth_words(names: &Names, berth: &str) -> String {
    match names.berth(berth).as_str() {
        "edge" => "the edge berth".to_string(),
        b => b.to_string(),
    }
}

pub fn rejection_text(r: Rejection) -> &'static str {
    match r {
        Rejection::UnknownId => "unknown name",
        Rejection::NoSuchRoute => "no such route",
        Rejection::AlreadySet => "already set",
        Rejection::ConflictingRoute => "conflicts with a route already set",
        Rejection::PointsLocked => "points locked",
        Rejection::PointsOccupied => "points occupied",
        Rejection::RouteNotSet => "no route set",
        Rejection::RouteIsAutomatic => "route is automatic",
        Rejection::NotPoints => "not points",
    }
}

/// A notice as one log line, and whether it is an alarm.
pub fn notice_text(n: &Notice, names: &Names) -> (String, bool) {
    match n {
        Notice::Rejected { cmd, reason, by } => {
            let by = by.as_ref().map(|r| format!(": {}", names.route(r))).unwrap_or_default();
            (format!("Refused: {} ({}{by})", command_text(cmd, names), rejection_text(*reason)), true)
        }
        Notice::NotYourArea { area } => (format!("Not your area: that is in {area}"), true),
        Notice::Spad { signal, train } => (format!("SPAD: {} passed {} at danger", names.headcode(train), names.signal(signal)), true),
        Notice::Collision { section } => (format!("COLLISION: {}", names.track(section)), true),
        Notice::Late { train, place, platform, late_s } => {
            (format!("{} at {} {platform}, {} min late", names.headcode(train), names.place(place), late_s / 60), false)
        }
        Notice::WrongPlatform { train, place, platform, expected } => {
            (format!("{} at {} platform {platform}, booked {expected}", names.headcode(train), names.place(place)), true)
        }
        Notice::Handover { headcode, from_area } => {
            (format!("{} offered from {from_area}", names.headcode(headcode)), false)
        }
        Notice::AreaTaken { area, holder } => (format!("{area} is now {holder}'s"), false),
        Notice::Replaced => ("This login was opened somewhere else".to_string(), true),
        Notice::GameCrashed => ("The game stopped unexpectedly".to_string(), true),
        Notice::Error { message, .. } => (format!("Error: {message}"), true),
    }
}

/// A train-list state in words.
pub fn train_state_text(s: TrainState) -> &'static str {
    match s {
        TrainState::AtPlatform => "at platform",
        TrainState::InArea => "in area",
        TrainState::Approaching => "approaching",
        TrainState::Due => "due",
    }
}

pub fn proposal_text(p: Proposal) -> String {
    match p {
        Proposal::Pause => "pause".to_string(),
        Proposal::Resume => "resume".to_string(),
        Proposal::Speed { x } => format!("{x}×"),
    }
}

/// `Vote: 4× — ann, bob agreed, 25 s left`
pub fn vote_text(v: &VoteView) -> String {
    format!("Vote: {} — {} agreed, {} s left", proposal_text(v.proposal), v.agreed.join(", "), v.expires_in_s)
}
