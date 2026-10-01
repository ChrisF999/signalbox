//! Words for the screen: times, commands, refusals, notices, votes.
//! Signals are named as the screen shows them (`Names`).

use protocol::{ExitName, Notice, PlayerCommand, PointsPos, Proposal, Rejection, VoteView};

use crate::names::Names;

/// Seconds since midnight as `HH:MM:SS` (wrapping at 24 h; bad input is 00:00:00).
pub fn fmt_hms(s: f64) -> String {
    let t = if s.is_finite() && s >= 0.0 { s.floor() as u64 % 86_400 } else { 0 };
    format!("{:02}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
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
        PlayerCommand::SwingPoints { points, to } => format!("swing {points} {}", pos_text(*to)),
        PlayerCommand::Interpose { berth, headcode } => format!("interpose {headcode} in {berth}"),
        PlayerCommand::CancelBerth { berth } => format!("cancel berth {berth}"),
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
        Notice::Rejected { cmd, reason } => {
            (format!("Refused: {} ({})", command_text(cmd, names), rejection_text(*reason)), true)
        }
        Notice::NotYourArea { area } => (format!("Not your area: that is in {area}"), true),
        Notice::Spad { signal, train } => (format!("SPAD: {train} passed {} at danger", names.signal(signal)), true),
        Notice::Collision { section } => (format!("COLLISION on {section}"), true),
        Notice::Late { train, place, platform, late_s } => {
            (format!("{train} at {place} {platform}, {} min late", late_s / 60), false)
        }
        Notice::WrongPlatform { train, place, platform, expected } => {
            (format!("{train} at {place} platform {platform}, booked {expected}"), true)
        }
        Notice::Handover { headcode, from_area } => (format!("{headcode} offered from {from_area}"), false),
        Notice::AreaTaken { area, holder } => (format!("{area} is now {holder}'s"), false),
        Notice::Replaced => ("This login was opened somewhere else".to_string(), true),
        Notice::GameCrashed => ("The game stopped unexpectedly".to_string(), true),
        Notice::Error { message, .. } => (format!("Error: {message}"), true),
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
