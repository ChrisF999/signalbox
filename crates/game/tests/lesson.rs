//! The lesson format, its checks against the world, and the runner
//! (tutorial spec §2, §3, §6).

mod common;

use common::twobox_json;
use game::lesson::{self, COLLISION_ALERT, Condition, LessonFile, Runner, SPAD_ALERT, parse_lesson};
use game::{Game, Out};
use protocol::{ClientMsg, ExitName, Highlight, LessonView, Notice, PlayerCommand, PointsPos, Proposal, ServerMsg, codes};
use serde_json::{Value, json};
use signalbox_core::interlocking::RouteState;

const ME: &str = "pat";

/// Hollins Cross with train 2H05 (lesson 3's world).
fn hollins() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../lessons/03-running-trains/world.json")).unwrap()
}

/// Kirkby Junction and the robot's Lowfield (lesson 4's world).
fn kirkby() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../lessons/04-junctions-and-handovers/world.json")).unwrap()
}

/// twobox with its first entry (1E01 at W) offered only on demand.
fn twobox_on_demand() -> String {
    let mut w: Value = serde_json::from_str(&twobox_json()).unwrap();
    w["entries"][0]["on_demand"] = json!(true);
    w.to_string()
}

fn lesson_json(area: &str, steps: Value) -> String {
    json!({"schema": 1, "title": "Test", "area": area, "start": "07:00", "steps": steps}).to_string()
}

/// The error for a twobox lesson (area West) with these steps.
fn refused(steps: Value) -> String {
    parse_lesson("t", &lesson_json("West", steps), &twobox_on_demand()).unwrap_err()
}

fn next() -> Value {
    json!({"continue": {}})
}

// ---- the format ----

#[test]
fn every_condition_action_and_highlight_reads() {
    let steps = json!([
        {"say": "All of it.",
         "highlight": [{"signal": "A"}, {"exit": {"kind": "node", "name": "E"}}, {"exit": {"kind": "signal", "name": "A"}},
                       {"points": "P"}, {"berth": "BA"}, {"section": "TW2"}, {"platform": {"place": "EST", "platform": "1"}},
                       {"ui": "settings"}, {"ui": "simplifier"}, {"ui": "trains"}, {"ui": "clock"}, {"ui": "auto:W1"}],
         "do": [{"spawn": {"headcode": "1E01", "entry": "W"}}, {"pause": {}}, {"run": {}}, {"speed": {"x": 4}},
                {"set_route": {"entrance": "C", "exit": {"kind": "signal", "name": "W2"}}}, {"cancel_route": {"entrance": "C"}},
                {"interpose": {"berth": "BA", "headcode": "2X99"}}],
         "wait_for": {"all": [
             {"continue": {}}, {"selected": {"signal": "A"}},
             {"route_set": {"entrance": "A", "exit": {"kind": "node", "name": "E"}}}, {"route_cancelled": {"entrance": "A"}},
             {"points": {"name": "P", "position": "reverse"}}, {"auto_working": {"signal": "W1", "on": true}},
             {"train_at": {"headcode": "1E01", "place": "EST", "platform": "1"}},
             {"train_passed": {"headcode": "1E01", "signal": "A"}}, {"train_left_area": {"headcode": "1E01"}},
             {"berth": {"name": "BA", "headcode": "1E01"}}, {"clock": {"paused": false}}, {"tab": "simplifier"}]}},
        {"say": "Refused.", "wait_for": {"rejected": {}},
         "solution": [{"set_route": {"entrance": "W1", "exit": {"kind": "signal", "name": "A"}}}, {"cancel_route": {"entrance": "W1"}},
                      {"set_auto_working": {"entrance": "W1", "on": false}}, {"interpose": {"berth": "BA", "headcode": "1A01"}}]}
    ]);
    // P lies in East: give West its points for this test.
    let mut w: Value = serde_json::from_str(&twobox_on_demand()).unwrap();
    w["sections"][2]["area"] = json!("West");
    let l = parse_lesson("all-of-it", &lesson_json("West", steps), &w.to_string()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(l.id, "all-of-it");
    assert_eq!(l.file.steps.len(), 2);
    let Condition::All(v) = &l.file.steps[0].wait_for else { panic!() };
    assert_eq!(v.len(), 12);
    assert!(l.file.steps[0].wait_for.needs_next() && !l.file.steps[1].wait_for.needs_next());
    assert_eq!(l.world.options.start_s, 7.0 * 3600.0, "the lesson's start time wins");
    let back: LessonFile = serde_json::from_value(serde_json::to_value(&l.file).unwrap()).unwrap();
    assert_eq!(back, l.file);
}

#[test]
fn a_misspelt_key_or_a_bad_header_is_an_error() {
    let w = twobox_on_demand();
    let bad = |text: String| parse_lesson("t", &text, &w).unwrap_err();
    let step = json!([{"say": "Hi", "wait_for": {"continue": {}}}]);
    assert!(bad(json!({"schema": 1, "title": "T", "area": "West", "start": "07:00", "steps": step, "colour": 1}).to_string())
        .starts_with("lesson.json: unknown field `colour`"));
    assert!(bad(lesson_json("West", json!([{"say": "Hi", "wait_for": {"continu": {}}}]))).contains("unknown variant `continu`"));
    assert!(bad(lesson_json("West", json!([{"say": "Hi", "wait_for": next(), "highlight": [{"teleport": "A"}]}])))
        .contains("unknown variant `teleport`"));
    assert_eq!(
        bad(json!({"schema": 2, "title": "T", "area": "West", "start": "07:00", "steps": step}).to_string()),
        "lesson.json: schema 2 (expected 1)"
    );
    assert_eq!(bad(lesson_json("Nowhere", step.clone())), "lesson.json: no area `Nowhere`");
    assert_eq!(
        bad(json!({"schema": 1, "title": "T", "area": "West", "start": "25:00", "steps": step}).to_string()),
        "lesson.json: bad start `25:00`"
    );
    assert_eq!(bad(lesson_json("West", json!([]))), "lesson.json: a lesson has 1 to 60 steps");
    assert_eq!(bad(lesson_json("West", json!([{"say": " ", "wait_for": next()}]))), "lesson.json: step 1: `say` must be 1 to 1200 characters");
    assert!(parse_lesson("t", &lesson_json("West", step), "{not json").unwrap_err().starts_with("world.json: "));
}

#[test]
fn unknown_names_are_refused_with_their_step() {
    let cases = [
        (json!({"say": "x", "wait_for": next(), "highlight": [{"signal": "Z9"}]}), "no signal `Z9`"),
        (json!({"say": "x", "wait_for": next(), "highlight": [{"points": "J1"}]}), "no points `J1`"),
        (json!({"say": "x", "wait_for": next(), "highlight": [{"exit": {"kind": "node", "name": "J1"}}]}), "no route ends at `J1`"),
        (json!({"say": "x", "wait_for": next(), "highlight": [{"berth": "BZ"}]}), "no berth `BZ`"),
        (json!({"say": "x", "wait_for": next(), "highlight": [{"section": "TZ"}]}), "no section `TZ`"),
        (json!({"say": "x", "wait_for": next(), "highlight": [{"platform": {"place": "EST", "platform": "9"}}]}), "no platform 9 at `EST`"),
        (json!({"say": "x", "wait_for": next(), "highlight": [{"ui": "teapot"}]}), "no screen control `teapot`"),
        (json!({"say": "x", "wait_for": {"tab": "alarms"}}), "no tab `alarms`"),
        (json!({"say": "x", "wait_for": next(), "do": [{"spawn": {"headcode": "2W03", "entry": "E"}}]}), "no on-demand entry of `2W03` at `E`"),
        (json!({"say": "x", "wait_for": next(), "do": [{"spawn": {"headcode": "9Z99", "entry": "W"}}]}), "no service `9Z99`"),
        (json!({"say": "x", "wait_for": next(), "do": [{"speed": {"x": 3}}]}), "speed 3 is not 1, 2, 4 or 8"),
        (json!({"say": "x", "wait_for": next(), "do": [{"interpose": {"berth": "BA", "headcode": "no way"}}]}), "`no way` is not a headcode"),
        (json!({"say": "x", "wait_for": {"route_set": {"entrance": "W1", "exit": {"kind": "node", "name": "E"}}}}), "no route from `W1` to Node(\"E\")"),
        (json!({"say": "x", "wait_for": {"all": []}}), "`all` of nothing"),
    ];
    for (step, why) in cases {
        assert_eq!(refused(json!([{"say": "first", "wait_for": next()}, step])), format!("lesson.json: step 2: {why}"));
    }
}

#[test]
fn a_step_the_player_cannot_complete_is_refused() {
    let cases = [
        (json!({"say": "x", "wait_for": {"selected": {"signal": "C"}}}), "`C` is not a signal the player works"),
        (json!({"say": "x", "wait_for": {"route_cancelled": {"entrance": "C"}}}), "`C` is not a signal the player works"),
        (json!({"say": "x", "wait_for": {"points": {"name": "P", "position": "reverse"}}}), "`P` are not points the player works"),
        (json!({"say": "x", "wait_for": {"auto_working": {"signal": "C", "on": true}}}), "`C` is not a signal the player works"),
        (json!({"say": "x", "wait_for": next(), "highlight": [{"ui": "auto:C"}]}), "`C` is not a signal the player works"),
        (
            json!({"say": "x", "wait_for": {"route_set": {"entrance": "C", "exit": {"kind": "signal", "name": "W2"}}}}),
            "`C` is not a signal the player works",
        ),
        (json!({"say": "x", "wait_for": {"train_left_area": {"headcode": "1E01"}}}), "no train `1E01` runs by now (spawn it in this or an earlier step)"),
        (
            json!({"say": "x", "do": [{"spawn": {"headcode": "1E01", "entry": "W"}}],
                   "wait_for": {"train_at": {"headcode": "1E01", "place": "NST", "platform": "1"}}}),
            "`1E01` never stops at NST platform 1",
        ),
        (json!({"say": "x", "wait_for": {"rejected": {}}}), "`rejected` needs a `solution` for the play-through"),
        (
            json!({"say": "x", "wait_for": {"rejected": {}}, "solution": [{"cancel_route": {"entrance": "C"}}]}),
            "solution CancelRoute { entrance: \"C\" } is not the player's to do",
        ),
    ];
    for (step, why) in cases {
        assert_eq!(refused(json!([step])), format!("lesson.json: step 1: {why}"));
    }
}

#[test]
fn what_the_lesson_does_itself_needs_no_player() {
    // East's route and a timed train: the lesson demonstrates, the train runs.
    let steps = json!([
        {"say": "x", "do": [{"set_route": {"entrance": "C", "exit": {"kind": "signal", "name": "W2"}}}],
         "wait_for": {"route_set": {"entrance": "C", "exit": {"kind": "signal", "name": "W2"}}}},
        {"say": "y", "wait_for": {"train_left_area": {"headcode": "2W03"}}}
    ]);
    parse_lesson("t", &lesson_json("West", steps), &twobox_on_demand()).unwrap();
}

#[test]
fn lesson_ids_name_directories() {
    assert!(lesson::valid_lesson_id("01-reading-the-panel"));
    for bad in ["", "../x", "Lesson", "a b", &"x".repeat(41)] {
        assert!(!lesson::valid_lesson_id(bad), "{bad}");
    }
    let dir = std::env::temp_dir().join(format!("sbx-lesson-{}", std::process::id())).join("Bad Name");
    assert!(lesson::load_lesson(&dir).unwrap_err().contains("is not a lesson id"));
    let dir = dir.with_file_name("missing");
    assert!(lesson::load_lesson(&dir).unwrap_err().starts_with("lesson.json: "));
}

#[test]
fn spawn_names_the_loaded_worlds_sorted_entry() {
    // 2W04 (file order 3) is made on demand and due first: it sorts to index 0.
    let mut w: Value = serde_json::from_str(&twobox_json()).unwrap();
    w["entries"][3]["on_demand"] = json!(true);
    w["entries"][3]["time"] = json!("06:00");
    let l = parse_lesson(
        "t",
        &lesson_json("West", json!([{"say": "x", "do": [{"spawn": {"headcode": "2W04", "entry": "N"}}], "wait_for": next()}])),
        &w.to_string(),
    )
    .unwrap();
    assert_eq!(lesson::spawn_entry(&l.world, "2W04", "N"), Some(0));
    assert!(l.world.entries[0].on_demand);
    assert_eq!(lesson::spawn_entry(&l.world, "2W04", "W"), None);
    assert_eq!(lesson::spawn_entry(&l.world, "1E01", "W"), None, "timed entries are not offered");
}

#[test]
fn an_entry_is_spawned_only_once() {
    let spawn = || json!({"spawn": {"headcode": "1E01", "entry": "W"}});
    let why = "`1E01` is spawned at `W` more than once";
    assert_eq!(
        refused(json!([{"say": "a", "do": [spawn()], "wait_for": next()}, {"say": "b", "do": [spawn()], "wait_for": next()}])),
        format!("lesson.json: step 2: {why}")
    );
    assert_eq!(refused(json!([{"say": "a", "do": [spawn(), spawn()], "wait_for": next()}])), format!("lesson.json: step 1: {why}"));
}

// ---- the runner ----

struct Rig {
    g: Game,
    r: Runner,
}

impl Rig {
    /// A lesson of `steps` on `world` for `area`, `pat` connected.
    fn new(world: &str, area: &str, steps: Value) -> (Rig, Vec<Out>) {
        let l = parse_lesson("t-lesson", &lesson_json(area, steps), world).unwrap_or_else(|e| panic!("{e}"));
        let (g, r) = lesson::start(l);
        let mut rig = Rig { g, r };
        let out = rig.r.connect(&mut rig.g, ME);
        (rig, out)
    }

    fn send(&mut self, msg: ClientMsg) -> Vec<Out> {
        self.r.handle(&mut self.g, ME, msg)
    }

    fn command(&mut self, cmd: PlayerCommand) -> Vec<Out> {
        self.send(ClientMsg::Command { cmd })
    }

    /// `secs` of real time at 1x, in 0.1 s steps.
    fn run(&mut self, secs: f64) -> Vec<Out> {
        let mut out = Vec::new();
        for _ in 0..(secs * 10.0).round() as u64 {
            out.extend(self.r.advance(&mut self.g, 0.1));
        }
        out
    }

    fn route_state(&self, name: &str) -> RouteState {
        let w = self.g.sim().world();
        self.g.sim().interlocking().routes[w.route_by_name(name).unwrap().idx()].state
    }
}

fn lessons(out: &[Out]) -> Vec<LessonView> {
    out.iter()
        .filter_map(|(p, m)| match m {
            ServerMsg::Lesson(v) if p == ME => Some(v.clone()),
            _ => None,
        })
        .collect()
}

fn set(entrance: &str, exit: &str) -> PlayerCommand {
    PlayerCommand::SetRoute { entrance: entrance.into(), exit: ExitName::Signal(exit.into()) }
}

#[test]
fn joining_claims_the_lessons_area_and_shows_the_first_step() {
    let (rig, out) = Rig::new(&hollins(), "Hollins Cross", json!([{"say": "Hello.", "highlight": [{"signal": "3"}], "wait_for": next()}]));
    assert_eq!(rig.g.area_of(ME), Some("Hollins Cross"));
    assert_eq!(rig.r.player(), Some(ME));
    let v = lessons(&out);
    assert_eq!(v.len(), 1);
    assert_eq!(
        v[0],
        LessonView {
            lesson: "t-lesson".into(),
            title: "Test".into(),
            index: 0,
            count: 1,
            say: "Hello.".into(),
            highlight: vec![Highlight::Signal("3".into())],
            needs_next: true,
            done: false,
            alert: None,
            completed: false,
            after: None,
        }
    );
}

#[test]
fn next_moves_on_only_a_continue_step_and_the_last_one_finishes() {
    let steps = json!([
        {"say": "one", "wait_for": next()},
        {"say": "two", "wait_for": {"clock": {"paused": true}}},
        {"say": "three", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", steps);
    let v = lessons(&rig.send(ClientMsg::LessonNext));
    assert_eq!((v[0].index, v[0].say.as_str(), v[0].needs_next), (1, "two", false));
    assert!(rig.send(ClientMsg::LessonNext).is_empty(), "Next does nothing here");
    assert_eq!(rig.r.step(), 1);
    let v = lessons(&rig.send(ClientMsg::Vote { proposal: Proposal::Pause }));
    assert_eq!(v[0].index, 2);
    let v = lessons(&rig.send(ClientMsg::LessonNext));
    assert_eq!((v[0].index, v[0].count, v[0].done, v[0].say.as_str()), (3, 3, true, ""));
    assert!(rig.r.done());
}

#[test]
fn route_points_and_berth_conditions_follow_the_sim() {
    let steps = json!([
        {"say": "set", "wait_for": {"route_set": {"entrance": "3", "exit": {"kind": "signal", "name": "5"}}}},
        {"say": "cancel", "wait_for": {"route_cancelled": {"entrance": "3"}}},
        {"say": "swing", "wait_for": {"points": {"name": "P1", "position": "reverse"}}},
        {"say": "describe", "wait_for": {"berth": {"name": "B9", "headcode": "1X01"}}},
        {"say": "end", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", steps);
    rig.command(set("3", "5"));
    assert_eq!(rig.r.step(), 0, "a route is set by the next tick, not at once");
    rig.run(0.2);
    assert_eq!(rig.route_state("3-5"), RouteState::Locked, "the points already lie normal");
    assert_eq!(rig.r.step(), 1);
    rig.command(PlayerCommand::CancelRoute { entrance: "3".into() });
    rig.run(0.5);
    assert_eq!(rig.r.step(), 2);
    rig.command(PlayerCommand::SwingPoints { points: "P1".into(), to: PointsPos::Reverse });
    rig.run(3.0);
    assert_eq!(rig.r.step(), 2, "still moving");
    rig.run(3.0);
    assert_eq!(rig.r.step(), 3);
    rig.command(PlayerCommand::Interpose { berth: "B9".into(), headcode: "1X01".into() });
    rig.run(0.2);
    assert_eq!(rig.r.step(), 4);
}

#[test]
fn screen_conditions_come_from_the_client() {
    let steps = json!([
        {"say": "choose", "wait_for": {"selected": {"signal": "3"}}},
        {"say": "tab", "wait_for": {"tab": "simplifier"}},
        {"say": "end", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", steps);
    rig.send(ClientMsg::LessonUi { tab: Some("trains".into()), selected: Some("1".into()) });
    assert_eq!(rig.r.step(), 0);
    rig.send(ClientMsg::LessonUi { tab: Some("trains".into()), selected: Some("3".into()) });
    assert_eq!(rig.r.step(), 1);
    // Someone else's screen does not count (the front lets nobody in, but still).
    rig.g.connect("sam");
    rig.r.handle(&mut rig.g, "sam", ClientMsg::LessonUi { tab: Some("simplifier".into()), selected: None });
    assert_eq!(rig.r.step(), 1);
    rig.send(ClientMsg::LessonUi { tab: Some("simplifier".into()), selected: None });
    assert_eq!(rig.r.step(), 2);
}

#[test]
fn a_train_runs_in_and_restart_step_puts_everything_back() {
    let steps = json!([
        {"say": "in", "do": [{"spawn": {"headcode": "2H05", "entry": "W"}}],
         "wait_for": {"train_at": {"headcode": "2H05", "place": "HXC", "platform": "2"}}},
        {"say": "end", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", steps);
    rig.run(0.2);
    assert_eq!(rig.g.sim().trains().len(), 1, "spawned at the step's start");
    rig.command(set("1", "3"));
    rig.command(set("3", "7"));
    rig.run(30.0);
    assert_eq!(rig.route_state("1-3"), RouteState::Locked);
    assert_eq!(rig.r.step(), 0);
    let before = rig.g.sim().now_s();
    let out = rig.send(ClientMsg::LessonRestartStep);
    assert_eq!(lessons(&out)[0].index, 0);
    assert_eq!(rig.route_state("1-3"), RouteState::Idle, "routes as the step found them");
    assert!(rig.g.sim().now_s() < before, "the clock went back");
    rig.run(0.2);
    assert_eq!(rig.g.sim().trains().len(), 1, "the step's spawn ran again, once");
    rig.command(set("1", "3"));
    rig.command(set("3", "7"));
    let out = rig.run(240.0);
    assert_eq!(rig.r.step(), 1, "2H05 stands at platform 2");
    assert_eq!(lessons(&out).last().unwrap().index, 1);
    rig.send(ClientMsg::LessonRestart);
    assert_eq!((rig.r.step(), rig.g.sim().trains().len()), (0, 0), "back to the very start");
}

#[test]
fn the_wrong_platform_keeps_the_step_waiting() {
    let steps = json!([
        {"say": "in", "do": [{"spawn": {"headcode": "2H05", "entry": "W"}}],
         "wait_for": {"train_at": {"headcode": "2H05", "place": "HXC", "platform": "2"}}},
        {"say": "end", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", steps);
    rig.command(set("1", "3"));
    rig.command(set("3", "5"));
    rig.run(240.0);
    assert_eq!(rig.r.step(), 0);
}

#[test]
fn a_refusal_counts_only_in_its_step() {
    let steps = json!([
        {"say": "set", "wait_for": {"route_set": {"entrance": "3", "exit": {"kind": "signal", "name": "5"}}}},
        {"say": "conflict", "wait_for": {"rejected": {}}, "solution": [{"set_route": {"entrance": "7", "exit": {"kind": "signal", "name": "9"}}}]},
        {"say": "end", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", steps);
    rig.command(set("1", "9"));
    rig.run(0.2);
    assert_eq!(rig.r.step(), 0, "a refusal before the step does not count");
    rig.command(set("3", "5"));
    rig.run(7.0);
    assert_eq!(rig.r.step(), 1);
    rig.command(set("7", "9"));
    let out = rig.run(0.2);
    assert!(out.iter().any(|(_, m)| matches!(m, ServerMsg::Notice(Notice::Rejected { .. }))));
    assert_eq!(rig.r.step(), 2);
}

#[test]
fn claim_and_release_are_refused_and_other_areas_are_the_robots() {
    let (mut rig, _) = Rig::new(&kirkby(), "Kirkby", json!([{"say": "x", "wait_for": next()}]));
    for msg in [ClientMsg::Release, ClientMsg::Claim { area: "Lowfield".into() }] {
        let out = rig.send(msg);
        assert!(matches!(&out[..], [(_, ServerMsg::Notice(Notice::Error { code, .. }))] if code == codes::IN_LESSON));
    }
    assert_eq!(rig.g.area_of(ME), Some("Kirkby"));
    assert_eq!(rig.g.holder("Lowfield"), None);
}

#[test]
fn the_clock_stands_still_without_its_player() {
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", json!([{"say": "x", "wait_for": next()}]));
    let t = rig.g.sim().now_s();
    rig.g.disconnect(ME);
    rig.run(5.0);
    assert_eq!(rig.g.sim().now_s(), t);
    let out = rig.r.connect(&mut rig.g, ME);
    assert_eq!(lessons(&out).len(), 1, "a reconnect gets the step again");
    rig.run(1.0);
    assert!(rig.g.sim().now_s() > t);
    let out = rig.send(ClientMsg::Resync);
    assert!(matches!(&out[..], [(_, ServerMsg::Layout(_)), (_, ServerMsg::View(_)), (_, ServerMsg::Lesson(_))]));
}

#[test]
fn a_spad_raises_the_alert_until_the_step_restarts() {
    let steps = json!([
        {"say": "in", "do": [{"spawn": {"headcode": "2H05", "entry": "W"}}], "wait_for": next()},
        {"say": "end", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", steps);
    rig.command(set("1", "3"));
    // Put H1 back to red under the driver's nose: 30 m short of it, at speed.
    let a = rig.g.sim().world().net.segment("a").unwrap();
    for _ in 0..600 {
        rig.run(0.1);
        let t = &rig.g.sim().trains()[0];
        if t.head().0 == a && t.head_m > 570.0 {
            break;
        }
    }
    rig.command(PlayerCommand::CancelRoute { entrance: "1".into() });
    let out = rig.run(30.0);
    assert_eq!(rig.g.stats().spads, 1);
    let v = lessons(&out);
    assert_eq!(v.last().unwrap().alert.as_deref(), Some(SPAD_ALERT));
    assert_eq!(v.last().unwrap().index, 0, "the step stays");
    let v = lessons(&rig.send(ClientMsg::LessonRestartStep));
    assert_eq!(v[0].alert, None);
    assert_ne!(COLLISION_ALERT, SPAD_ALERT);
}

#[test]
fn a_train_that_left_early_does_not_hold_the_lesson_up() {
    let steps = json!([
        {"say": "go", "do": [{"spawn": {"headcode": "1K01", "entry": "W"}}],
         "wait_for": {"route_set": {"entrance": "1", "exit": {"kind": "signal", "name": "5"}}}},
        {"say": "on", "wait_for": next()},
        {"say": "gone", "wait_for": {"train_left_area": {"headcode": "1K01"}}},
        {"say": "end", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&kirkby(), "Kirkby", steps);
    rig.command(set("1", "5"));
    rig.command(set("5", "7"));
    rig.run(120.0);
    assert_eq!(rig.r.step(), 1, "waiting for Next while 1K01 leaves");
    rig.send(ClientMsg::LessonNext);
    assert_eq!(rig.r.step(), 3, "1K01 already left: the step after it is shown at once");
}

#[test]
fn a_spad_holds_the_step_even_when_it_meets_the_condition() {
    // The step waits for 2H05 to pass H1; passing it at danger must not count.
    let steps = json!([
        {"say": "in", "do": [{"spawn": {"headcode": "2H05", "entry": "W"}}],
         "wait_for": {"train_passed": {"headcode": "2H05", "signal": "1"}}},
        {"say": "end", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", steps);
    rig.command(set("1", "3"));
    let a = rig.g.sim().world().net.segment("a").unwrap();
    for _ in 0..600 {
        rig.run(0.1);
        let t = &rig.g.sim().trains()[0];
        if t.head().0 == a && t.head_m > 570.0 {
            break;
        }
    }
    rig.command(PlayerCommand::CancelRoute { entrance: "1".into() });
    let out = rig.run(30.0);
    assert_eq!(rig.g.stats().spads, 1);
    assert_eq!(rig.r.step(), 0, "the step stays while the alert is up");
    assert_eq!(rig.r.view().alert.as_deref(), Some(SPAD_ALERT));
    assert!(lessons(&out).iter().all(|v| v.index == 0));
    rig.send(ClientMsg::LessonNext);
    assert_eq!(rig.r.step(), 0, "nothing moves it on but a restart");
    let v = lessons(&rig.send(ClientMsg::LessonRestartStep));
    assert_eq!((v[0].index, v[0].alert.as_deref()), (0, None));
    assert!(rig.g.sim().trains().is_empty(), "back to before the train came in");
    rig.command(set("1", "3"));
    rig.run(120.0);
    assert_eq!(rig.r.step(), 1, "passed at a proceed aspect this time");
    assert_eq!(rig.r.view().alert, None);
}

#[test]
fn only_an_interlocking_refusal_counts_as_rejected() {
    let steps = json!([
        {"say": "conflict", "wait_for": {"rejected": {}},
         "solution": [{"set_route": {"entrance": "3", "exit": {"kind": "signal", "name": "5"}}}]},
        {"say": "end", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&kirkby(), "Kirkby", steps);
    let out = rig.command(PlayerCommand::SetRoute { entrance: "7".into(), exit: ExitName::Node("E".into()) });
    assert!(matches!(&out[..], [(_, ServerMsg::Notice(Notice::NotYourArea { .. }))]), "{out:?}");
    rig.run(0.2);
    assert_eq!(rig.r.step(), 0, "Lowfield's signal is not the interlocking refusing");
    let out = rig.command(set("Nope", "5"));
    assert!(matches!(&out[..], [(_, ServerMsg::Notice(Notice::Rejected { .. }))]), "{out:?}");
    rig.run(0.2);
    assert_eq!(rig.r.step(), 0, "nor is an unknown name");
    rig.command(set("1", "5"));
    rig.run(7.0);
    assert_eq!(rig.r.step(), 0);
    rig.command(set("3", "5"));
    rig.run(0.2);
    assert_eq!(rig.r.step(), 1, "the conflicting route is refused by the interlocking");
}

#[test]
fn restart_lesson_forgets_what_the_screen_showed() {
    let steps = json!([
        {"say": "one", "wait_for": next()},
        {"say": "choose", "wait_for": {"selected": {"signal": "3"}}},
        {"say": "end", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", steps);
    rig.send(ClientMsg::LessonNext);
    rig.send(ClientMsg::LessonUi { tab: Some("trains".into()), selected: Some("3".into()) });
    assert_eq!(rig.r.step(), 2);
    rig.send(ClientMsg::LessonRestart);
    assert_eq!(rig.r.step(), 0);
    rig.send(ClientMsg::LessonNext);
    assert_eq!(rig.r.step(), 1, "the old selection does not choose for the player");
}

#[test]
fn restart_step_puts_the_game_back_bit_for_bit() {
    let steps = json!([
        {"say": "in", "do": [{"spawn": {"headcode": "2H05", "entry": "W"}}], "wait_for": next()},
        {"say": "watch", "wait_for": {"all": [{"train_passed": {"headcode": "2H05", "signal": "3"}}, next()]}},
        {"say": "end", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", steps);
    rig.command(set("1", "3"));
    rig.command(set("3", "7"));
    rig.run(5.0);
    rig.send(ClientMsg::LessonNext);
    assert_eq!(rig.r.step(), 1);
    let at_start = format!("{:?}", rig.g.snapshot());
    rig.run(150.0);
    assert_eq!(rig.route_state("1-3"), RouteState::Idle, "2H05 is past H3");
    assert_eq!(rig.r.step(), 1, "waiting for Next");
    rig.send(ClientMsg::LessonRestartStep);
    assert_eq!(format!("{:?}", rig.g.snapshot()), at_start, "sim, clock, queue and robot as the step found them");
    rig.send(ClientMsg::LessonNext);
    assert_eq!(rig.r.step(), 1, "what the lesson had seen went back too: 2H05 has not passed H3");
}

#[test]
fn another_users_lesson_messages_are_ignored() {
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", json!([{"say": "a", "wait_for": next()}, {"say": "b", "wait_for": next()}]));
    rig.send(ClientMsg::LessonNext);
    rig.g.connect("sam");
    for msg in [ClientMsg::LessonNext, ClientMsg::LessonRestartStep, ClientMsg::LessonRestart] {
        assert!(rig.r.handle(&mut rig.g, "sam", msg).is_empty());
        assert_eq!(rig.r.step(), 1);
    }
}

#[test]
fn joining_sends_the_layout_and_view_once() {
    let (_, out) = Rig::new(&hollins(), "Hollins Cross", json!([{"say": "x", "wait_for": next()}]));
    assert!(matches!(&out[..], [(_, ServerMsg::Layout(_)), (_, ServerMsg::View(_)), (_, ServerMsg::Lesson(_))]), "{out:?}");
}

/// Polish spec H5: a step with a `done` text, its task done, says so and
/// waits for Next, so the player sees the result; a Next pressed early does
/// not count.
#[test]
fn a_done_step_waits_for_next_after_its_task() {
    let steps = json!([
        {"say": "pause it", "done": "Paused: nothing moves.", "wait_for": {"clock": {"paused": true}}},
        {"say": "end", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", steps);
    let v = rig.r.view();
    assert_eq!((v.completed, v.needs_next, v.after.clone()), (false, false, None));
    assert!(rig.send(ClientMsg::LessonNext).is_empty(), "too early");
    let v = lessons(&rig.send(ClientMsg::Vote { proposal: Proposal::Pause }));
    assert_eq!((v[0].index, v[0].completed, v[0].needs_next, v[0].after.as_deref()), (0, true, true, Some("Paused: nothing moves.")));
    assert_eq!(rig.r.step(), 0, "it waits");
    let v = lessons(&rig.send(ClientMsg::LessonNext));
    assert_eq!((v[0].index, v[0].completed), (1, false));
}

/// Review M3: a `done` text on a step that waits for Next, or an empty one,
/// is refused (it would never show).
#[test]
fn a_done_text_must_be_said_and_must_have_a_task_to_follow() {
    let e = refused(json!([{"say": "x", "done": "Done.", "wait_for": next()}]));
    assert_eq!(e, "lesson.json: step 1: `done` is for a step whose task is not Next");
    let e = refused(json!([{"say": "x", "done": " ", "wait_for": {"clock": {"paused": true}}}]));
    assert_eq!(e, "lesson.json: step 1: `done` must be 1 to 1200 characters");
}

/// Review M4: Restart step on a completed step clears the completion; the
/// task must be done again.
#[test]
fn restart_step_clears_a_completed_step() {
    let steps = json!([
        {"say": "pause it", "done": "Paused.", "wait_for": {"clock": {"paused": true}}},
        {"say": "end", "wait_for": next()}
    ]);
    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", steps);
    let v = lessons(&rig.send(ClientMsg::Vote { proposal: Proposal::Pause }));
    assert!(v[0].completed);
    let v = lessons(&rig.send(ClientMsg::LessonRestartStep));
    assert_eq!((v[0].index, v[0].completed, v[0].after.clone()), (0, false, None));
    rig.send(ClientMsg::LessonNext);
    assert_eq!(rig.r.step(), 0, "Next does nothing until the task is done again");
}
