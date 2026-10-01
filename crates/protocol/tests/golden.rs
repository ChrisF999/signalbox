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
        geometry: None,
        box_prefix: String::new(),
        workstations: BTreeMap::new(),
        simplifier: vec![],
        display_headcodes: Default::default(),
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
                        "automatic": false, "operable": true}],
            "geometry": null, "box_prefix": "", "workstations": {}, "simplifier": []
        }),
    );
}

#[test]
fn layout_geometry() {
    let mut l = small_layout();
    l.geometry = Some(Geometry {
        lines: vec![LineGeom { segment: s("pa"), x1: 10.0, y1: 625.0, x2: 205.5, y2: 625.0 }],
        points: vec![PointsGeom {
            node: s("P"),
            x: 220.0,
            y: 625.0,
            toe: Some([215.0, 625.0]),
            normal: Some([225.0, 625.0]),
            reverse: None,
        }],
        signals: vec![SignalGeom {
            signal: s("A"),
            x: 300.0,
            y: 25.0,
            berth_x: 260.0,
            berth_y: 30.0,
            facing: Some([1.0, 0.0]),
        }],
        platforms: vec![PlatformGeom { place: s("EST"), platform: s("1"), x1: 0.0, y1: 0.0, x2: 300.0, y2: 15.0 }],
        labels: vec![LabelGeom { text: s("Hackney & Bow"), x: -20.0, y: 615.0, arrow: None }],
        nodes: vec![NodeGeom { node: s("E"), x: 400.0, y: 0.0 }],
    });
    let want = json!({
        "lines": [{"segment": "pa", "x1": 10.0, "y1": 625.0, "x2": 205.5, "y2": 625.0}],
        "points": [{"node": "P", "x": 220.0, "y": 625.0, "toe": [215.0, 625.0], "normal": [225.0, 625.0], "reverse": null}],
        "signals": [{"signal": "A", "x": 300.0, "y": 25.0, "berth_x": 260.0, "berth_y": 30.0, "facing": [1.0, 0.0]}],
        "platforms": [{"place": "EST", "platform": "1", "x1": 0.0, "y1": 0.0, "x2": 300.0, "y2": 15.0}],
        "labels": [{"text": "Hackney & Bow", "x": -20.0, "y": 615.0}],
        "nodes": [{"node": "E", "x": 400.0, "y": 0.0}]
    });
    let json = serde_json::to_value(ServerMsg::Layout(l.clone())).unwrap();
    assert_eq!(json["geometry"], want);
    let back: ServerMsg = serde_json::from_value(json).unwrap();
    assert_eq!(back, ServerMsg::Layout(l));
}

/// Realism spec §2.1 and §3: prefixes, workstation letters, the simplifier
/// and a line name's arrow.
#[test]
fn layout_display_data() {
    let mut l = small_layout();
    l.box_prefix = s("L");
    l.workstations = BTreeMap::from([(s("West"), s("A")), (s("East"), s("B"))]);
    l.simplifier = vec![SimplifierRow {
        headcode: s("1A07"),
        origin: Some(s("BOWJ")),
        destination: Some(s("LIVST")),
        calls: vec![
            SimplifierCall { place: s("WSJ"), platform: Some(s("ML_UP")), arr: None, dep: Some(34_170.0), stops: false },
            SimplifierCall { place: s("LIVST"), platform: Some(s("12")), arr: Some(34_380.0), dep: None, stops: true },
        ],
    }];
    l.geometry = Some(Geometry {
        labels: vec![
            LabelGeom { text: s("UP MAIN"), x: 885.0, y: 488.0, arrow: Some([-1.0, 0.0]) },
            LabelGeom { text: s("BANK"), x: 60.0, y: 50.0, arrow: None },
        ],
        ..Geometry::default()
    });
    let json = serde_json::to_value(ServerMsg::Layout(l.clone())).unwrap();
    assert_eq!(json["box_prefix"], "L");
    assert_eq!(json["workstations"], json!({"East": "B", "West": "A"}));
    assert_eq!(
        json["simplifier"],
        json!([{"headcode": "1A07", "origin": "BOWJ", "destination": "LIVST", "calls": [
            {"place": "WSJ", "platform": "ML_UP", "arr": null, "dep": 34170.0, "stops": false},
            {"place": "LIVST", "platform": "12", "arr": 34380.0, "dep": null, "stops": true}
        ]}])
    );
    assert_eq!(
        json["geometry"]["labels"],
        json!([{"text": "UP MAIN", "x": 885.0, "y": 488.0, "arrow": [-1.0, 0.0]}, {"text": "BANK", "x": 60.0, "y": 50.0}]),
        "a label without an arrow is written as before"
    );
    let back: ServerMsg = serde_json::from_value(json).unwrap();
    assert_eq!(back, ServerMsg::Layout(l));
}

#[test]
fn a_layout_or_view_from_before_d1_still_reads() {
    let mut json = serde_json::to_value(ServerMsg::Layout(small_layout())).unwrap();
    for key in ["geometry", "box_prefix", "workstations", "simplifier"] {
        json.as_object_mut().unwrap().remove(key);
    }
    let ServerMsg::Layout(l) = serde_json::from_value(json).unwrap() else { panic!() };
    assert_eq!(l.geometry, None);
    assert_eq!((l.box_prefix.as_str(), l.workstations.len(), l.simplifier.len()), ("", 0, 0));
    let view = json!({
        "type": "view", "seq": 1, "sim_time": 0.0, "speed": 1, "paused": false, "vote": null,
        "holders": {}, "score": null, "signals": {}, "routes": {}, "points": {}, "sections": {}, "berths": {}
    });
    let ServerMsg::View(v) = serde_json::from_value(view).unwrap() else { panic!() };
    assert!(v.trains.is_empty());
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
        trains: BTreeMap::from([
            (
                s("1E01"),
                TrainRow {
                    next_place: Some(s("EST")),
                    next_platform: Some(s("1")),
                    booked: Some(25500.0),
                    late_s: 120,
                    state: TrainState::InArea,
                },
            ),
            (
                s("2W03"),
                TrainRow { next_place: None, next_platform: None, booked: None, late_s: 0, state: TrainState::Due },
            ),
        ]),
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
            "berths": {"BA": "1E01"},
            "trains": {
                "1E01": {"next_place": "EST", "next_platform": "1", "booked": 25500.0, "late_s": 120, "state": "in_area"},
                "2W03": {"next_place": null, "next_platform": null, "booked": null, "late_s": 0, "state": "due"}
            }
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
        trains: BTreeMap::from([
            (s("1E01"), None),
            (
                s("2W03"),
                Some(TrainRow {
                    next_place: Some(s("WST")),
                    next_platform: None,
                    booked: Some(26100.0),
                    late_s: 0,
                    state: TrainState::AtPlatform,
                }),
            ),
        ]),
        ..Delta::default()
    };
    check_server(
        ServerMsg::Delta(d),
        json!({
            "type": "delta", "seq": 8, "sim_time": 25216.3, "vote": null,
            "signals": {"A": "red"}, "routes": {"A-E": null}, "berths": {"BA": null, "BW1": "2W03"},
            "trains": {"1E01": null, "2W03": {"next_place": "WST", "next_platform": null, "booked": 26100.0,
                                             "late_s": 0, "state": "at_platform"}}
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

/// Tutorial spec §3: the lesson messages.
#[test]
fn lesson_client_messages() {
    check_client(ClientMsg::LessonNext, json!({"type": "lesson_next"}));
    check_client(ClientMsg::LessonRestartStep, json!({"type": "lesson_restart_step"}));
    check_client(ClientMsg::LessonRestart, json!({"type": "lesson_restart"}));
    check_client(
        ClientMsg::LessonUi { tab: Some(s("simplifier")), selected: Some(s("39,1V1")) },
        json!({"type": "lesson_ui", "tab": "simplifier", "selected": "39,1V1"}),
    );
    check_client(ClientMsg::LessonUi { tab: None, selected: None }, json!({"type": "lesson_ui"}));
}

#[test]
fn lesson_view() {
    let v = LessonView {
        lesson: s("02-routes"),
        title: s("Setting & cancelling routes"),
        index: 1,
        count: 10,
        say: s("Now click H3."),
        highlight: vec![
            Highlight::Signal(s("3")),
            Highlight::Exit(ExitName::Node(s("E"))),
            Highlight::Points(s("P1")),
            Highlight::Berth(s("B3")),
            Highlight::Section(s("T2")),
            Highlight::Platform { place: s("HXC"), platform: s("2") },
            Highlight::Ui(s("auto:5")),
        ],
        needs_next: false,
        done: false,
        alert: Some(s("Restart the step.")),
    };
    check_server(
        ServerMsg::Lesson(v.clone()),
        json!({"type": "lesson", "lesson": "02-routes", "title": "Setting & cancelling routes", "index": 1, "count": 10,
               "say": "Now click H3.",
               "highlight": [{"signal": "3"}, {"exit": {"kind": "node", "name": "E"}}, {"points": "P1"}, {"berth": "B3"},
                             {"section": "T2"}, {"platform": {"place": "HXC", "platform": "2"}}, {"ui": "auto:5"}],
               "needs_next": false, "done": false, "alert": "Restart the step."}),
    );
    let done = LessonView { index: 10, say: s(""), highlight: vec![], done: true, alert: None, ..v };
    check_server(
        ServerMsg::Lesson(done),
        json!({"type": "lesson", "lesson": "02-routes", "title": "Setting & cancelling routes", "index": 10, "count": 10,
               "say": "", "highlight": [], "needs_next": false, "done": true}),
    );
}

#[test]
fn a_lesson_without_highlights_still_reads_and_junk_is_an_error() {
    let ok: ServerMsg = serde_json::from_value(json!({"type": "lesson", "lesson": "x", "title": "X", "index": 0, "count": 1,
                                                       "say": "Hi", "needs_next": true, "done": false}))
    .unwrap();
    let ServerMsg::Lesson(v) = ok else { panic!() };
    assert!(v.highlight.is_empty() && v.alert.is_none());
    let bad = json!({"type": "lesson", "lesson": "x", "title": "X", "index": 0, "count": 1, "say": "Hi",
                     "highlight": [{"teleport": "3"}], "needs_next": true, "done": false});
    assert!(serde_json::from_value::<ServerMsg>(bad).is_err());
}
