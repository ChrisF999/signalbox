//! Wire JSON for every message (spec §3), pinned and round-tripped.

use std::collections::BTreeMap;

use protocol::*;
use serde_json::{Value, json};

fn check_client(msg: ClientMsg, want: Value) {
    assert_eq!(serde_json::to_value(&msg).unwrap(), want, "{msg:?}");
    let back: ClientMsg = serde_json::from_str(&want.to_string()).unwrap();
    assert_eq!(back, msg);
}

fn check_server(msg: ServerMsg, want: Value) {
    assert_eq!(serde_json::to_value(&msg).unwrap(), want, "{msg:?}");
    let back: ServerMsg = serde_json::from_str(&want.to_string()).unwrap();
    assert_eq!(back, msg);
}

fn s(x: &str) -> String {
    x.to_string()
}

#[test]
fn client_messages() {
    check_client(ClientMsg::Claim { area: s("Hackney & Bow") }, json!({"type": "claim", "area": "Hackney & Bow"}));
    check_client(ClientMsg::Release, json!({"type": "release"}));
    check_client(ClientMsg::Resync, json!({"type": "resync"}));
    check_client(ClientMsg::Vote { proposal: Proposal::Pause }, json!({"type": "vote", "proposal": {"kind": "pause"}}));
    check_client(ClientMsg::Vote { proposal: Proposal::Resume }, json!({"type": "vote", "proposal": {"kind": "resume"}}));
    check_client(
        ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } },
        json!({"type": "vote", "proposal": {"kind": "speed", "x": 8}}),
    );
}

#[test]
fn player_commands() {
    let cases = vec![
        (
            PlayerCommand::SetRoute { entrance: s("39,1V1"), exit: ExitName::Signal(s("512#113")) },
            json!({"cmd": "set_route", "entrance": "39,1V1", "exit": {"kind": "signal", "name": "512#113"}}),
        ),
        (
            PlayerCommand::SetRoute { entrance: s("A"), exit: ExitName::Node(s("E")) },
            json!({"cmd": "set_route", "entrance": "A", "exit": {"kind": "node", "name": "E"}}),
        ),
        (PlayerCommand::CancelRoute { entrance: s("A") }, json!({"cmd": "cancel_route", "entrance": "A"})),
        (
            PlayerCommand::SetAutoWorking { entrance: s("A"), on: true },
            json!({"cmd": "set_auto_working", "entrance": "A", "on": true}),
        ),
        (
            PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse },
            json!({"cmd": "swing_points", "points": "P", "to": "reverse"}),
        ),
        (
            PlayerCommand::Interpose { berth: s("BA"), headcode: s("1A01") },
            json!({"cmd": "interpose", "berth": "BA", "headcode": "1A01"}),
        ),
        (PlayerCommand::CancelBerth { berth: s("BA") }, json!({"cmd": "cancel_berth", "berth": "BA"})),
    ];
    for (cmd, inner) in cases {
        check_client(ClientMsg::Command { cmd }, json!({"type": "command", "cmd": inner}));
    }
}

#[test]
fn notices() {
    let cases = vec![
        (
            Notice::Rejected { cmd: PlayerCommand::CancelRoute { entrance: s("A") }, reason: Rejection::RouteNotSet },
            json!({"kind": "rejected", "cmd": {"cmd": "cancel_route", "entrance": "A"}, "reason": "route_not_set"}),
        ),
        (Notice::NotYourArea { area: s("East") }, json!({"kind": "not_your_area", "area": "East"})),
        (Notice::Spad { signal: s("A"), train: s("1A01") }, json!({"kind": "spad", "signal": "A", "train": "1A01"})),
        (Notice::Collision { section: s("TP") }, json!({"kind": "collision", "section": "TP"})),
        (
            Notice::Late { train: s("1A01"), place: s("EST"), platform: s("1"), late_s: 125 },
            json!({"kind": "late", "train": "1A01", "place": "EST", "platform": "1", "late_s": 125}),
        ),
        (
            Notice::WrongPlatform { train: s("1A01"), place: s("EST"), platform: s("2"), expected: s("1") },
            json!({"kind": "wrong_platform", "train": "1A01", "place": "EST", "platform": "2", "expected": "1"}),
        ),
        (
            Notice::Handover { headcode: s("2W03"), from_area: s("East") },
            json!({"kind": "handover", "headcode": "2W03", "from_area": "East"}),
        ),
        (
            Notice::AreaTaken { area: s("West"), holder: s("alice") },
            json!({"kind": "area_taken", "area": "West", "holder": "alice"}),
        ),
        (Notice::Replaced, json!({"kind": "replaced"})),
        (Notice::GameCrashed, json!({"kind": "game_crashed"})),
        (
            Notice::Error { code: s(codes::BAD_SPEED), message: s("speed must be 1, 2, 4 or 8") },
            json!({"kind": "error", "code": "bad_speed", "message": "speed must be 1, 2, 4 or 8"}),
        ),
    ];
    for (n, mut want) in cases {
        want["type"] = json!("notice");
        check_server(ServerMsg::Notice(n), want);
    }
}

fn small_layout() -> Layout {
    Layout {
        title: s("Two boxes"),
        you: s("alice"),
        area: Some(s("West")),
        areas: vec![s("West"), s("East")],
        sections: vec![SectionInfo { name: s("TP"), area: s("East"), fringe: true }],
        segments: vec![SegmentInfo { name: s("pa"), from: s("J1"), to: s("P"), length_m: 40.0, section: s("TP") }],
        signals: vec![SignalInfo {
            name: s("A"),
            area: s("West"),
            segment: s("w2"),
            offset_m: 1000.0,
            direction: Dir::Up,
            aspects: 3,
            operable: true,
        }],
        points: vec![PointsInfo { name: s("P"), section: s("TP"), area: s("East"), operable: false }],
        berths: vec![BerthInfo { name: s("BW"), signal: None, boundary: Some(s("W")), area: s("West"), operable: true }],
        platforms: vec![PlatformInfo { place: s("EST"), platform: s("1"), segment: s("e"), from_m: 700.0, to_m: 900.0 }],
        routes: vec![RouteInfo {
            name: s("A-E"),
            entrance: s("A"),
            exit: ExitName::Node(s("E")),
            automatic: false,
            operable: true,
        }],
    }
}

#[test]
fn layout() {
    check_server(
        ServerMsg::Layout(small_layout()),
        json!({
            "type": "layout", "title": "Two boxes", "you": "alice", "area": "West", "areas": ["West", "East"],
            "sections": [{"name": "TP", "area": "East", "fringe": true}],
            "segments": [{"name": "pa", "from": "J1", "to": "P", "length_m": 40.0, "section": "TP"}],
            "signals": [{"name": "A", "area": "West", "segment": "w2", "offset_m": 1000.0, "direction": "up",
                         "aspects": 3, "operable": true}],
            "points": [{"name": "P", "section": "TP", "area": "East", "operable": false}],
            "berths": [{"name": "BW", "signal": null, "boundary": "W", "area": "West", "operable": true}],
            "platforms": [{"place": "EST", "platform": "1", "segment": "e", "from_m": 700.0, "to_m": 900.0}],
            "routes": [{"name": "A-E", "entrance": "A", "exit": {"kind": "node", "name": "E"},
                        "automatic": false, "operable": true}]
        }),
    );
}

#[test]
fn view() {
    let v = View {
        seq: 7,
        sim_time: 25215.5,
        speed: 8,
        paused: false,
        vote: Some(VoteView { proposal: Proposal::Pause, agreed: vec![s("alice")], expires_in_s: 30 }),
        holders: BTreeMap::from([(s("East"), s("robot")), (s("West"), s("alice"))]),
        score: Some(5),
        signals: BTreeMap::from([(s("A"), Aspect::DoubleYellow)]),
        routes: BTreeMap::from([(s("A-E"), RouteView { state: RouteState::Locked, auto_working: false })]),
        points: BTreeMap::from([(s("P"), PointsView { position: PointsPos::Normal, moving: false, locked: true })]),
        sections: BTreeMap::from([(s("TP"), SectionView { occupied: true, held: Held::Path })]),
        berths: BTreeMap::from([(s("BA"), s("1E01"))]),
    };
    check_server(
        ServerMsg::View(v),
        json!({
            "type": "view", "seq": 7, "sim_time": 25215.5, "speed": 8, "paused": false,
            "vote": {"proposal": {"kind": "pause"}, "agreed": ["alice"], "expires_in_s": 30},
            "holders": {"East": "robot", "West": "alice"}, "score": 5,
            "signals": {"A": "double_yellow"},
            "routes": {"A-E": {"state": "locked", "auto_working": false}},
            "points": {"P": {"position": "normal", "moving": false, "locked": true}},
            "sections": {"TP": {"occupied": true, "held": "path"}},
            "berths": {"BA": "1E01"}
        }),
    );
}

#[test]
fn delta_sends_only_changes_and_null_for_cleared() {
    let d = Delta {
        seq: 8,
        sim_time: Some(25216.3),
        vote: Some(None),
        signals: BTreeMap::from([(s("A"), Aspect::Red)]),
        routes: BTreeMap::from([(s("A-E"), None)]),
        berths: BTreeMap::from([(s("BA"), None), (s("BW1"), Some(s("2W03")))]),
        ..Delta::default()
    };
    check_server(
        ServerMsg::Delta(d),
        json!({
            "type": "delta", "seq": 8, "sim_time": 25216.3, "vote": null,
            "signals": {"A": "red"}, "routes": {"A-E": null}, "berths": {"BA": null, "BW1": "2W03"}
        }),
    );
    check_server(ServerMsg::Delta(Delta { seq: 9, ..Delta::default() }), json!({"type": "delta", "seq": 9}));
}

#[test]
fn bad_input_is_an_error_not_a_panic() {
    for text in [
        r#"{"type": "fly"}"#,
        r#"{"area": "West"}"#,
        r#"{"type": "claim"}"#,
        r#"{"type": "vote", "proposal": {"kind": "speed", "x": 300}}"#,
        r#"{"type": "command", "cmd": {"cmd": "set_route", "entrance": 5}}"#,
        "[1, 2]",
        "",
    ] {
        assert!(serde_json::from_str::<ClientMsg>(text).is_err(), "{text}");
    }
}
