//! Words on the screen.

use client_core::Names;
use client_core::text::*;
use protocol::*;

fn s(x: &str) -> String {
    x.to_string()
}

#[test]
fn clock_times() {
    assert_eq!(fmt_hms(25_215.9), "07:00:15");
    assert_eq!(fmt_hms(0.0), "00:00:00");
    assert_eq!(fmt_hms(86_399.0), "23:59:59");
    assert_eq!(fmt_hms(86_400.0 + 61.0), "00:01:01", "wraps at midnight");
    assert_eq!(fmt_hms(f64::NAN), "00:00:00");
    assert_eq!(fmt_hms(-5.0), "00:00:00");
}

#[test]
fn commands_and_refusals() {
    let c = PlayerCommand::SetRoute { entrance: s("39,1V1"), exit: ExitName::Node(s("N12")) };
    let plain = Names::default();
    assert_eq!(command_text(&c, &plain), "set route 39,1V1 to N12");
    assert_eq!(
        notice_text(&Notice::Rejected { cmd: c, reason: Rejection::PointsLocked, by: None }, &plain),
        (s("Refused: set route 39,1V1 to N12 (points locked)"), true)
    );
    assert_eq!(
        command_text(&PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse }, &plain),
        "swing P reverse"
    );
    assert_eq!(
        notice_text(&Notice::Late { train: s("1A01"), place: s("EST"), platform: s("1"), late_s: 125 }, &plain),
        (s("1A01 at EST 1, 2 min late"), false)
    );
    assert_eq!(notice_text(&Notice::Error { code: s("bad_speed"), message: s("no") }, &plain), (s("Error: no"), true));
}

#[test]
fn votes() {
    let v = VoteView { proposal: Proposal::Speed { x: 4 }, agreed: vec![s("ann"), s("bob")], expires_in_s: 25 };
    assert_eq!(vote_text(&v), "Vote: 4× — ann, bob agreed, 25 s left");
    assert_eq!(proposal_text(Proposal::Pause), "pause");
}
