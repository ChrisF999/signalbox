//! The lesson format and its checks against the world (tutorial spec §2,
//! §6); the runner's tests follow in Task 5.

mod common;

use common::twobox_json;
use game::lesson::{self, Condition, LessonFile, parse_lesson};
use serde_json::{Value, json};

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
