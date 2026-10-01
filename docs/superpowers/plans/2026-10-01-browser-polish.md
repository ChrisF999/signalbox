# Browser Polish (D1.2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A diagram legible at every zoom (no text over text, numbers off the track), Drain running the real Waterloo & City Working Timetable for a whole day (from the owner's own PDF; only the reader is committed), old Drain saves showing today's `W…` names, the simplifier opening at "now", and a real-browser check of the WebGL2 fallback.

**Architecture:** `paint::draw` stays pure and now records which texts may move (role + other spots) and what must be kept clear; a new pure `labels` module plans placement greedily by priority and `UiApp` caches the plan per zoom. The converter gains `--wtt`: a reader of `pdftotext -bbox` output of LU WTT No. 7 that checks the timetable against the WTT's own figures and replaces Drain's services, entries and start time (a pure `WorldFile` transform); the image gets that text from the git-ignored `external/wtt/` in a Docker stage. The robot gains one standing rule (core), without which the WTT's peaks gridlock. Resume optionally takes the current layout file and swaps in its display-only `layout` JSON when the saved network matches. A shell + Python script drives Playwright Chromium against a throwaway dev-login front.

**Tech Stack:** Rust 1.98 (Docker `scripts/cargo`), egui 0.36 (headless tests), serde_json, rusqlite, tokio; Playwright 1.55 Chromium (Docker image on ra); poppler-utils `pdftotext` 22.12 (Debian bookworm, in the image's `wtt` stage).

**Spec:** `docs/superpowers/specs/2026-10-01-browser-polish-design.md`

**Base:** `main` **after the `tutorial` branch merges** (this plan was written against `5eb82f0`). Before Task 1, re-check every file and line reference below against the merged code — the tutorial edits `crates/server/src/process.rs`, `crates/server/src/supervisor.rs` and `crates/game/src/game.rs`, which Tasks 6 and 7 touch, and may add lesson highlights to `crates/client-ui/src/paint.rs`. Every Rust and script block in this plan was compiled and its tests run (and `deploy/browser-check.sh` run end to end) on a scratch copy of `5eb82f0`; the `deploy/Dockerfile` loop was not built (the controller's deploy exercises it). Where the merged code differs, keep the intent and the tests. **Amended 2026-10-01:** the tutorial has merged (`main` = `0c0ea67`); Tasks 4a–4c (the WTT, replacing the repeat timetable) and Task 5's screen test were written and run on a scratch copy of `0c0ea67`, including the `wtt` Docker stage (not the full image).

## Global Constraints

- License GPL-2.0-or-later; **no new crates** (no `Cargo.lock` package changes, so the CI runner's offline cache needs no reseed).
- Every cargo command runs through `scripts/cargo` from the repo root (Docker `rust:1.98-slim-bookworm`, repo at `/w`, no environment forwarded); wasm32 builds through `scripts/wasm-build`.
- CI builds with `-D warnings --locked --offline`: no unused imports, variables or dead code; rustfmt and clippy are unavailable, so match the surrounding style by hand (4-space indent, ~120 columns).
- Determinism rules stay for `core`, `game`, `protocol` and `ts2-import`: `BTreeMap`/`BTreeSet`/`Vec` only, no wall clock; converter output byte-identical for the same input. The sim never reads the world's `layout` JSON.
- **No protocol change, no save-schema change** (save schema stays 2). The only new process argument is `signalbox-game --current-layout <world.json>` (resume only).
- `client-core` and `client-ui` never touch the browser, the clock or storage directly; text sizes reach `labels` through a `measure` function.
- Placement depends only on the scene, the zoom and the settings: never on train state, never on the screen edge (spec P3).
- Priority (spec P2): own signal numbers, ○A letters, line names, platform numbers, labels, fringe signal numbers. Headcodes are never moved or hidden.
- The legibility targets (spec §3.4): 0 overlapping texts and 0 covered texts in all 66 renders; at 1280 × 800 Fit every own number drawn in every box view and ≤ 4 numbers tight against track in total.
- **Licence (spec §4.1, owner decision):** TfL's WTT PDF, its text and anything made from it (parsed trips, a converted world) are never committed, logged into a commit message or pasted into a test. `external/wtt/` ignores everything but its `README.md` and `.gitignore`; tests use only the synthetic `wtt-synthetic.bbox.html`; the real-WTT soak is `#[ignore]` and skips without the file. Before every commit in Tasks 4b/4c, `git status --short` must show nothing from `external/wtt/`.
- Drain from the WTT (spec §4.4): Wednesday, 574 services, 5 entries, start 05:40, headcodes `<train>/<trip>` (`201/7` … `202/163`), roads `DPT` 5/6/7 for both the siding and the depot.
- The only core change is the robot's standing rule (Task 4a, spec P22); the sim, protocol and save schema are untouched.
- Infra (deploying, the CI runner, `/opt/stack`, `tailscale serve`) is controller-only, in the final Controller section. Subagents may run `scripts/cargo`, `scripts/wasm-build` and `deploy/browser-check.sh` (it starts and removes its own throwaway container), and must not touch any other container, image, volume or network.

## Review Focus

1. **Nonsense geometry reaching the placer** — coordinates of ±1e9 or NaN from a bad layout, a text that measures NaN: `plan` must return within a second, place nothing at a non-finite offset, and hide what it cannot measure rather than draw it somewhere odd. Pinned in Task 1 (`nonsense_geometry_stays_cheap_and_finite`).
2. **A save whose layout the front no longer lists** (renamed or removed from the image) or whose layout file is unreadable: it must still resume, with its own display data, exactly as today. Pinned in Task 7 (`a_save_of_a_layout_no_longer_listed_still_resumes`) and Task 6 (`a_different_network_or_a_bad_file_keeps_the_saves_own`).
3. **A WTT that is damaged or not WTT No. 7** (another PDF, a different `pdftotext`): the conversion must stop with a message naming the page or the failed check, never produce a partial timetable, and the image build must fail with it. Pinned in Task 4b (`a_wrong_file_is_refused`, `a_day_is_checked_against_what_the_wtt_says`, CLI `wtt_flag_checks_the_timetable_and_writes_nothing_when_it_fails`).
4. **A stale placement plan** applied to a different drawing (numbers toggled, a claim changing the layout, a tutorial pushing extra texts): nothing may be moved by another drawing's offsets. Pinned in Task 1 (`apply_drops_hidden_texts_and_points_the_rest_at_their_new_index`: a plan of the wrong length changes nothing) and Task 3 (the cache key holds game, layout generation, scale and the numbers setting).
5. **Train movement re-placing labels** (flicker): placement must be identical with and without a headcode in a berth and after a pan. Pinned in Task 2 (`a_plan_depends_on_neither_pan_nor_trains`).
6. **The robot standing where it blocks a route to somewhere else** (Task 4a): `may_stand` must refuse sections with points and sections used by a route ending at another signal. Pinned by the Liverpool Street and bot soaks (no stuck trains) and Task 4a's Drain test.

## File Structure

| File | Task | Responsibility |
|---|---|---|
| `crates/client-ui/src/labels.rs` (new) | 1 | Roles, `Movable`, `KeepClear`, `plan`, `apply`, `audit`: pure placement |
| `crates/client-ui/src/lib.rs` | 1 | `pub mod labels;` |
| `crates/client-ui/src/paint.rs` | 1, 2 | `Drawing.movable`/`keep`, `font()`; `draw` records movable texts, alternative spots and keep-clear shapes; ○A letter outward |
| `crates/client-ui/src/hit.rs` | 2 | ○A hidden below the number threshold; berth boxes sized to the longest headcode (`berth_width`, `berth_box`) |
| `crates/client-ui/src/scene.rs` | 2 | `BerthMark.width_px` |
| `crates/client-ui/src/screens.rs` | 3, 5 | Plan cache and `labels::apply` before painting; simplifier scroll to now |
| `crates/client-ui/tests/labels.rs` (new) | 1, 2 | Placement unit tests |
| `crates/client-ui/tests/paint.rs` | 2 | ○A threshold, keep-clear lists, number spots, berth width |
| `crates/client-ui/tests/legibility.rs` (new) | 3 | The spec §3.4 acceptance measurement over the shipped layouts |
| `crates/client-ui/tests/screens.rs` | 3, 5 | Wiring: no text over text on screen; simplifier opens at now |
| `crates/core/src/robot.rs`, `crates/ts2-import/tests/soak.rs` | 4a | `may_stand`: wait at an automatic signal on plain line |
| `crates/ts2-import/src/wtt.rs` (new), `src/lib.rs`, `src/main.rs` | 4b | Read, check and apply the WTT; the `--wtt` flag |
| `crates/ts2-import/tests/wtt.rs`, `tests/data/wtt-synthetic.py`, `tests/data/wtt-synthetic.bbox.html` (new), `tests/cli.rs` | 4b | Reader, checks, Drain, CLI on the synthetic WTT |
| `external/wtt/README.md`, `external/wtt/.gitignore`, `crates/ts2-import/tests/wtt_day.rs` (new), `deploy/Dockerfile`, `deploy/README.md` | 4c | Where the owner's PDF lives; the image's `wtt` stage; the owner-run whole-day soak |
| `crates/client-core/src/simplifier.rs`, `tests/simplifier.rs` | 5 | `last_time`, `now_line` |
| `crates/game/src/save.rs`, `src/game.rs`, `src/lib.rs`, `tests/refresh.rs` (new) | 6 | `refresh_display`, `Game::resume_with_layout`, `Refresh` |
| `crates/server/src/process.rs`, `src/supervisor.rs`, `tests/process.rs`, `tests/supervisor.rs` | 7 | `--current-layout`; the front passes it on resume |
| `deploy/browser-check.sh`, `deploy/browser-check.py` (new), `deploy/README.md` | 8 | The real-browser renderer check |
| `CLAUDE.md` | 3, 4a, 4b, 4c, 6, 7, 8 | One paragraph per change, in the task that makes it |

---

### Task 1: The placer (`labels`)

**Files:**
- Create: `crates/client-ui/src/labels.rs`
- Modify: `crates/client-ui/src/lib.rs` (module list), `crates/client-ui/src/paint.rs` (`Drawing`, `font`, `paint`)
- Test: `crates/client-ui/tests/labels.rs` (new)

**Interfaces:**
- Consumes: `paint::{Drawing, TextItem}` (existing; `TextItem { at, anchor, text, size, colour, monospace }`).
- Produces (later tasks rely on these exact names):
  - `labels::Role { Number, AutoLetter, LineName, Platform, Label, FringeNumber }` (derives `Ord`; declaration order is priority)
  - `labels::Movable { text: usize, role: Role, alts: Vec<(Pos2, Align2)>, within: Option<Rect> }`
  - `labels::KeepClear { bars: Vec<(Pos2, Pos2, f32)>, rounds: Vec<(Pos2, f32)>, boxes: Vec<Rect> }`
  - `labels::Spot = Option<(Vec2, Align2)>`; `labels::Plan { spots: Vec<Spot>, tight: usize, hidden_numbers: Vec<String> }`
  - `labels::plan(d: &Drawing, measure: &mut dyn FnMut(&TextItem) -> Vec2) -> Plan`
  - `labels::apply(d: Drawing, p: &Plan) -> Drawing`
  - `labels::audit(d: &Drawing, measure: &mut dyn FnMut(&TextItem) -> Vec2) -> Audit { overlaps, covered, tight, shown: BTreeMap<String, usize> }`
  - `labels::corner(v: Vec2) -> Align2`, `labels::touches_bar`, `labels::touches_round`, `labels::SLACK`
  - `paint::Drawing { shapes, texts, movable: Vec<Movable>, keep: KeepClear }`, `paint::font(&TextItem) -> FontId`

- [ ] **Step 1: Write the failing tests**

Create `crates/client-ui/tests/labels.rs` with everything below **except** the last test (`a_plan_depends_on_neither_pan_nor_trains`, added in Task 2) and its imports `client_core::{AspectMode, Names}`, `client_ui::camera::Camera`, `client_ui::paint::{PaintState, draw}`, `client_ui::scene::Scene` and `mod common; use common::*;` (also Task 2):

```rust
//! Placing the diagram's texts (polish spec §3.3), on hand-made drawings
//! with a fixed-advance measure: 6 px a character, 10 px high.

mod common;

use client_core::{AspectMode, Names};
use client_ui::camera::Camera;
use client_ui::labels::*;
use client_ui::paint::{Drawing, LABEL, PaintState, TextItem, draw};
use client_ui::scene::Scene;
use common::*;
use egui::{Align2, Pos2, Rect, Vec2, pos2, vec2};

fn measure(t: &TextItem) -> Vec2 {
    vec2(t.text.chars().count() as f32 * 6.0, 10.0)
}

fn text(s: &str, at: Pos2, anchor: Align2) -> TextItem {
    TextItem { at, anchor, text: s.to_string(), size: 10.0, colour: LABEL, monospace: true }
}

/// `d` with `t` added as a movable text of `role`.
fn push(d: &mut Drawing, t: TextItem, role: Role, alts: Vec<(Pos2, Align2)>) {
    d.movable.push(Movable { text: d.texts.len(), role, alts, within: None });
    d.texts.push(t);
}

/// A horizontal track bar along y = 100, 6 px wide.
fn track() -> Drawing {
    Drawing { keep: KeepClear { bars: vec![(pos2(0.0, 100.0), pos2(400.0, 100.0), 6.0)], ..KeepClear::default() }, ..Drawing::default() }
}

fn plan_of(d: &Drawing) -> Plan {
    plan(d, &mut measure)
}

#[test]
fn a_number_on_the_track_moves_to_its_first_clear_spot() {
    let mut d = track();
    // Its own spot (y 89..99) touches the bar (97..103); the first other
    // spot does too; the second is clear.
    let alts = vec![(pos2(50.0, 101.0), Align2::CENTER_BOTTOM), (pos2(50.0, 90.0), Align2::CENTER_BOTTOM)];
    push(&mut d, text("LA11", pos2(50.0, 99.0), Align2::CENTER_BOTTOM), Role::Number, alts);
    let p = plan_of(&d);
    assert_eq!(p.spots, vec![Some((vec2(0.0, -9.0), Align2::CENTER_BOTTOM))]);
    assert_eq!((p.tight, p.hidden_numbers.len()), (0, 0));
    let placed = apply(d, &p);
    assert_eq!(placed.texts[0].at, pos2(50.0, 90.0));
}

#[test]
fn a_number_with_no_clear_spot_is_tight_and_one_with_no_free_spot_is_hidden() {
    let mut d = track();
    d.keep.bars[0].2 = 40.0; // 80..120: no spot or nudge of a 10 px text escapes it
    push(&mut d, text("LA11", pos2(50.0, 100.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    let p = plan_of(&d);
    assert_eq!(p.spots, vec![Some((Vec2::ZERO, Align2::CENTER_CENTER))], "on the track, but readable");
    assert_eq!(p.tight, 1);
    // A lamp under every spot: hidden, and named.
    d.keep.rounds = (0..40).map(|i| (pos2(i as f32 * 5.0, 100.0), 30.0)).collect();
    let p = plan_of(&d);
    assert_eq!(p.spots, vec![None]);
    assert_eq!(p.hidden_numbers, vec!["LA11".to_string()]);
}

#[test]
fn labels_and_fringe_numbers_are_hidden_rather_than_drawn_on_track() {
    let mut d = track();
    d.keep.bars[0].2 = 80.0; // a bar 80 px wide: no nudge escapes it
    push(&mut d, text("BANK", pos2(50.0, 100.0), Align2::LEFT_TOP), Role::Label, vec![]);
    push(&mut d, text("LB72", pos2(150.0, 100.0), Align2::CENTER_CENTER), Role::FringeNumber, vec![]);
    let p = plan_of(&d);
    assert_eq!(p.spots, vec![None, None]);
    assert_eq!(p.tight, 0, "only your own numbers may touch track");
    assert!(p.hidden_numbers.is_empty(), "fringe numbers are not counted as yours");
}

#[test]
fn a_number_wins_its_spot_from_a_label_drawn_first() {
    let mut d = Drawing::default();
    push(&mut d, text("BETHNAL GREEN", pos2(100.0, 50.0), Align2::CENTER_CENTER), Role::Label, vec![]);
    push(&mut d, text("LB72", pos2(100.0, 50.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    let p = plan_of(&d);
    assert_eq!(p.spots[1], Some((Vec2::ZERO, Align2::CENTER_CENTER)), "the number stays");
    assert_eq!(p.spots[0], Some((vec2(0.0, -10.0), Align2::CENTER_CENTER)), "the label moves up a line");
}

#[test]
fn a_line_name_moves_only_up_or_down() {
    let mut d = Drawing::default();
    push(&mut d, text("LB72", pos2(100.0, 50.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    push(&mut d, text("UP MAIN", pos2(100.0, 50.0), Align2::RIGHT_CENTER), Role::LineName, vec![]);
    let p = plan_of(&d);
    let (off, _) = p.spots[1].unwrap();
    assert_eq!(off.x, 0.0);
    assert!(off.y.abs() >= 5.0);
}

#[test]
fn a_platform_number_must_fit_its_block() {
    let mut d = Drawing::default();
    d.movable.push(Movable { text: 0, role: Role::Platform, alts: vec![], within: Some(Rect::from_center_size(pos2(50.0, 50.0), vec2(4.0, 4.0))) });
    d.texts.push(text("12", pos2(50.0, 50.0), Align2::CENTER_CENTER));
    assert_eq!(plan_of(&d).spots, vec![None], "a 12 × 10 text in a 4 px block");
    d.movable[0].within = Some(Rect::from_center_size(pos2(50.0, 50.0), vec2(40.0, 14.0)));
    assert_eq!(plan_of(&d).spots, vec![Some((Vec2::ZERO, Align2::CENTER_CENTER))]);
}

#[test]
fn texts_without_placement_data_are_left_alone_and_ignored() {
    let mut d = Drawing::default();
    d.texts.push(text("1A01", pos2(100.0, 50.0), Align2::CENTER_CENTER)); // a headcode
    push(&mut d, text("LB72", pos2(100.0, 50.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    let p = plan_of(&d);
    assert_eq!(p.spots, vec![Some((Vec2::ZERO, Align2::CENTER_CENTER))], "berth boxes stand in for headcodes");
    let placed = apply(d, &p);
    assert_eq!(placed.texts[0].text, "1A01");
    assert_eq!(placed.texts[0].at, pos2(100.0, 50.0));
}

#[test]
fn apply_drops_hidden_texts_and_points_the_rest_at_their_new_index() {
    let mut d = track();
    d.keep.bars[0].2 = 80.0;
    d.texts.push(text("1A01", pos2(10.0, 10.0), Align2::CENTER_CENTER));
    push(&mut d, text("BANK", pos2(50.0, 100.0), Align2::LEFT_TOP), Role::Label, vec![]);
    push(&mut d, text("LB72", pos2(150.0, 20.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    let p = plan_of(&d);
    let placed = apply(d.clone(), &p);
    assert_eq!(placed.texts.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), ["1A01", "LB72"]);
    assert_eq!(placed.movable.len(), 1);
    assert_eq!((placed.movable[0].text, placed.movable[0].role), (1, Role::Number));
    assert!(placed.movable[0].alts.is_empty());
    // A plan made for another drawing changes nothing.
    assert_eq!(apply(d.clone(), &Plan::default()), d);
}

/// Review focus 1: nonsense coordinates and sizes stay cheap and finite.
#[test]
fn nonsense_geometry_stays_cheap_and_finite() {
    let mut d = Drawing::default();
    d.keep.bars.push((pos2(-1.0e9, 0.0), pos2(1.0e9, 0.0), 6.0));
    d.keep.bars.push((pos2(f32::NAN, 0.0), pos2(5.0, f32::INFINITY), 6.0));
    d.keep.rounds.push((pos2(f32::NAN, f32::NAN), 4.0));
    push(&mut d, text("LA11", pos2(50.0, 50.0), Align2::CENTER_CENTER), Role::Number, vec![(pos2(f32::NAN, 1.0), Align2::LEFT_TOP)]);
    push(&mut d, text("FAR", pos2(4.0e8, -3.0e8), Align2::LEFT_TOP), Role::Label, vec![]);
    let t = std::time::Instant::now();
    let p = plan_of(&d);
    assert!(t.elapsed() < std::time::Duration::from_secs(1), "{:?}", t.elapsed());
    assert_eq!(p.spots[0], Some((Vec2::ZERO, Align2::CENTER_CENTER)));
    assert!(p.spots.iter().flatten().all(|(o, _)| o.is_finite()));
    // A text that cannot be measured is hidden, never drawn somewhere odd.
    let p = plan(&d, &mut |_| vec2(f32::NAN, 10.0));
    assert_eq!(p.spots, vec![None, None]);
}

#[test]
fn the_anchor_for_a_direction() {
    assert_eq!(corner(vec2(-1.0, -1.0)), Align2::RIGHT_BOTTOM, "up and left");
    assert_eq!(corner(vec2(1.0, 0.0)), Align2::LEFT_CENTER);
    assert_eq!(corner(vec2(0.0, 1.0)), Align2::CENTER_TOP);
}

#[test]
fn audit_counts_overlaps_covered_texts_and_tight_numbers() {
    let mut d = track();
    push(&mut d, text("BANK", pos2(50.0, 100.0), Align2::CENTER_CENTER), Role::Label, vec![]);
    push(&mut d, text("LA11", pos2(150.0, 100.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    push(&mut d, text("LA13", pos2(152.0, 100.0), Align2::CENTER_CENTER), Role::Number, vec![]);
    let a = audit(&d, &mut measure);
    assert_eq!((a.overlaps, a.covered, a.tight), (1, 1, 2));
    assert_eq!(a.shown.get("Number"), Some(&2));
}

/// Placement keeps to what the layout and zoom give (spec P3): the same
/// plan after a pan, and with or without a train in a berth.
#[test]
fn a_plan_depends_on_neither_pan_nor_trains() {
    let l = layout_for(None);
    let sc = Scene::build(&l).unwrap();
    let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
    let names = Names::new(&l);
    let (mut empty, mut busy) = (view_for(None), view_for(None));
    empty.berths.clear();
    busy.berths.insert("BW1".into(), "1A01".into());
    let plan_at = |cam: &Camera, v: &protocol::View| {
        let st = PaintState { view: Some(v), selected: None, exits: &[], refused: None, time: 0.0, aspects: AspectMode::RedGreen, numbers: true, names: &names };
        plan(&draw(&sc, cam, screen, &st), &mut measure)
    };
    let cam = Camera::fit(sc.all.unwrap(), screen);
    let mut panned = cam;
    panned.pan(vec2(37.0, -21.0));
    let a = plan_at(&cam, &empty);
    let close = |x: &Plan, y: &Plan| {
        x.spots.len() == y.spots.len()
            && x.spots.iter().zip(&y.spots).all(|(p, q)| match (p, q) {
                (Some((o, a)), Some((u, b))) => a == b && (*o - *u).length() < 1e-3,
                (None, None) => true,
                _ => false,
            })
    };
    assert!(close(&a, &plan_at(&panned, &empty)), "a pan changes nothing");
    assert!(close(&a, &plan_at(&cam, &busy)), "a train in a berth changes nothing");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-ui --test labels`
Expected: compile error — `could not find labels in client_ui` (and `Drawing` has no field `movable`).

- [ ] **Step 3: Write the placer**

Create `crates/client-ui/src/labels.rs`:

```rust
//! Placing the diagram's texts so that none is drawn over another or over
//! the track (polish spec §3). `paint::draw` gives every text that may move
//! its role and its other spots, and lists what must be kept clear; `plan`
//! decides, in priority order, the first clear spot of each or that it is
//! hidden; `apply` moves and drops them. Pure: text sizes come from the
//! caller's `measure`. A plan holds offsets from each text's own spot, so it
//! stays right when the camera pans and is made again only on a zoom.

use std::collections::BTreeMap;

use egui::{Align, Align2, Pos2, Rect, Vec2, vec2};

use crate::paint::{Drawing, TextItem};

/// Who wins a collision: earlier first (spec decision P2). Headcodes are
/// not here: they are never moved or hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// A signal number of yours (every signal's, for a spectator).
    Number,
    /// The `A` beside a ○A button.
    AutoLetter,
    LineName,
    Platform,
    Label,
    /// A signal number of another area.
    FringeNumber,
}

/// A text that may move or be hidden.
#[derive(Clone, Debug, PartialEq)]
pub struct Movable {
    /// Index into `Drawing::texts`.
    pub text: usize,
    pub role: Role,
    /// Other spots (point and anchor), best first, after the text's own.
    pub alts: Vec<(Pos2, Align2)>,
    /// A platform number's block: the text must fit inside it.
    pub within: Option<Rect>,
}

/// What no movable text may touch (signal numbers may touch `bars`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeepClear {
    /// Track bars and points legs: ends and width.
    pub bars: Vec<(Pos2, Pos2, f32)>,
    /// Lamps, second-lamp spots and ○A circles: centre and radius.
    pub rounds: Vec<(Pos2, f32)>,
    /// Every berth's box and every exit square.
    pub boxes: Vec<Rect>,
}

/// Where a movable text goes: an offset from its own spot and the anchor
/// there, or `None` for hidden.
pub type Spot = Option<(Vec2, Align2)>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// One per `Drawing::movable`, in order.
    pub spots: Vec<Spot>,
    /// Signal numbers drawn touching a track bar.
    pub tight: usize,
    /// Signal numbers (yours) that had no spot, by text.
    pub hidden_numbers: Vec<String>,
}

/// Overlaps smaller than this (pixels, both ways) do not count.
pub const SLACK: f32 = 0.5;
/// Grid cell for the collision search, in pixels.
const CELL: f32 = 48.0;
/// An object spanning more cells than this is checked against everything
/// (nonsense coordinates must stay cheap).
const MAX_CELLS: i64 = 4096;

/// Nudges for labels and signal numbers: multiples of the text's width
/// and height, nearest first.
const LABEL_NUDGES: [(f32, f32); 24] = [
    (0.0, -0.5), (0.0, 0.5), (-0.5, 0.0), (0.5, 0.0), (0.0, -1.0), (0.0, 1.0), (-0.5, -0.5), (0.5, -0.5),
    (-0.5, 0.5), (0.5, 0.5), (-1.0, 0.0), (1.0, 0.0), (-0.5, -1.0), (0.5, -1.0), (-0.5, 1.0), (0.5, 1.0),
    (-1.0, -0.5), (1.0, -0.5), (-1.0, 0.5), (1.0, 0.5), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0),
];
/// Line names move only up or down (their arrow stays where it is).
const LINE_NUDGES: [(f32, f32); 4] = [(0.0, -0.5), (0.0, 0.5), (0.0, -1.0), (0.0, 1.0)];

/// The anchor that makes a text extend from its point in direction `v`.
pub fn corner(v: Vec2) -> Align2 {
    let pick = |c: f32| {
        if c > 0.3 {
            Align::Min
        } else if c < -0.3 {
            Align::Max
        } else {
            Align::Center
        }
    };
    Align2([pick(v.x), pick(v.y)])
}

fn overlaps(a: Rect, b: Rect) -> bool {
    let i = a.intersect(b);
    i.width() > SLACK && i.height() > SLACK
}

/// Segment a–b crosses or touches `r` (Liang–Barsky).
fn segment_hits(a: Pos2, b: Pos2, r: Rect) -> bool {
    if r.contains(a) || r.contains(b) {
        return true;
    }
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f32, 1.0_f32);
    for (p, q) in [(-d.x, a.x - r.min.x), (d.x, r.max.x - a.x), (-d.y, a.y - r.min.y), (d.y, r.max.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            if t > t1 {
                return false;
            }
            t0 = t0.max(t);
        } else {
            if t < t0 {
                return false;
            }
            t1 = t1.min(t);
        }
    }
    t0 <= t1
}

pub fn touches_bar(r: Rect, (a, b, w): (Pos2, Pos2, f32)) -> bool {
    segment_hits(a, b, r.expand(w / 2.0 - SLACK))
}

pub fn touches_round(r: Rect, (c, radius): (Pos2, f32)) -> bool {
    let nearest = c.clamp(r.min, r.max);
    nearest.distance(c) < radius - SLACK
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Obj {
    Bar(u32),
    Round(u32),
    Box(u32),
    Placed(u32),
}

/// A uniform grid of what is kept clear and what is placed.
#[derive(Default)]
struct Grid {
    cells: BTreeMap<(i64, i64), Vec<Obj>>,
    everywhere: Vec<Obj>,
}

impl Grid {
    fn span(r: Rect) -> Option<(i64, i64, i64, i64)> {
        if !(r.min.is_finite() && r.max.is_finite()) {
            return None;
        }
        let c = |v: f32| (v / CELL).floor() as i64;
        Some((c(r.min.x), c(r.min.y), c(r.max.x), c(r.max.y)))
    }

    fn insert(&mut self, r: Rect, o: Obj) {
        let Some((x0, y0, x1, y1)) = Grid::span(r) else { return };
        if (x1 - x0 + 1).saturating_mul(y1 - y0 + 1) > MAX_CELLS {
            self.everywhere.push(o);
            return;
        }
        for x in x0..=x1 {
            for y in y0..=y1 {
                self.cells.entry((x, y)).or_default().push(o);
            }
        }
    }

    /// Everything that may meet `r`, each once, in order.
    fn near(&self, r: Rect) -> Vec<Obj> {
        let mut out = self.everywhere.clone();
        if let Some((x0, y0, x1, y1)) = Grid::span(r) {
            if (x1 - x0 + 1).saturating_mul(y1 - y0 + 1) > MAX_CELLS {
                out.extend(self.cells.values().flatten().copied());
            } else {
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        out.extend(self.cells.get(&(x, y)).into_iter().flatten().copied());
                    }
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

fn bar_box((a, b, w): (Pos2, Pos2, f32)) -> Rect {
    Rect::from_two_pos(a, b).expand(w / 2.0)
}

/// What a text at `r` would hit: (anything but track, track).
fn hits(r: Rect, d: &Drawing, grid: &Grid, placed: &[Rect]) -> (bool, bool) {
    let (mut solid, mut track) = (false, false);
    for o in grid.near(r) {
        match o {
            Obj::Bar(i) => track |= touches_bar(r, d.keep.bars[i as usize]),
            Obj::Round(i) => solid |= touches_round(r, d.keep.rounds[i as usize]),
            Obj::Box(i) => solid |= overlaps(r, d.keep.boxes[i as usize]),
            Obj::Placed(i) => solid |= overlaps(r, placed[i as usize]),
        }
        if solid {
            break;
        }
    }
    (solid, track)
}

/// Decide where every movable text goes. Greedy, in role order and then
/// drawing order; a spot is clear when it overlaps no text placed before
/// and touches nothing kept clear. A signal number of yours with no clear
/// spot takes the first that only touches track ("tight"), else it is
/// hidden, as is any other text with no clear spot. Texts without a
/// `Movable` (headcodes, anything added later) are neither moved nor
/// considered: berth boxes stand in for headcodes.
pub fn plan(d: &Drawing, measure: &mut dyn FnMut(&TextItem) -> Vec2) -> Plan {
    let mut grid = Grid::default();
    for (i, &b) in d.keep.bars.iter().enumerate() {
        grid.insert(bar_box(b), Obj::Bar(i as u32));
    }
    for (i, &(c, r)) in d.keep.rounds.iter().enumerate() {
        grid.insert(Rect::from_center_size(c, vec2(r, r) * 2.0), Obj::Round(i as u32));
    }
    for (i, &b) in d.keep.boxes.iter().enumerate() {
        grid.insert(b, Obj::Box(i as u32));
    }
    let mut order: Vec<usize> = (0..d.movable.len()).filter(|&m| d.movable[m].text < d.texts.len()).collect();
    order.sort_by_key(|&m| (d.movable[m].role, d.movable[m].text));
    let mut out = Plan { spots: vec![None; d.movable.len()], ..Plan::default() };
    let mut placed: Vec<Rect> = Vec::new();
    for m in order {
        let mv = &d.movable[m];
        let t = &d.texts[mv.text];
        let size = measure(t);
        if !size.is_finite() {
            continue;
        }
        let own = t.anchor.anchor_size(t.at, size);
        // Its own spot, its other spots, then nudges of its own spot.
        let mut spots: Vec<(Pos2, Align2, Rect)> = vec![(t.at, t.anchor, own)];
        for &(p, a) in &mv.alts {
            spots.push((p, a, a.anchor_size(p, size)));
        }
        let nudges: &[(f32, f32)] = match mv.role {
            Role::Number | Role::Label => &LABEL_NUDGES,
            Role::LineName => &LINE_NUDGES,
            _ => &[],
        };
        for &(dx, dy) in nudges {
            let off = vec2(dx * size.x, dy * size.y);
            spots.push((t.at + off, t.anchor, own.translate(off)));
        }
        let fits = |r: &Rect| mv.within.is_none_or(|w| w.expand(SLACK).contains_rect(*r));
        let looked: Vec<(bool, bool)> =
            spots.iter().map(|(_, _, r)| if fits(r) { hits(*r, d, &grid, &placed) } else { (true, true) }).collect();
        let mut pick = looked.iter().position(|&(solid, track)| !solid && !track);
        if pick.is_none() && mv.role == Role::Number {
            pick = looked.iter().position(|&(solid, _)| !solid);
            out.tight += usize::from(pick.is_some());
        }
        match pick {
            Some(k) => {
                let (p, a, r) = spots[k];
                out.spots[m] = Some((p - t.at, a));
                grid.insert(r, Obj::Placed(placed.len() as u32));
                placed.push(r);
            }
            None if mv.role == Role::Number => out.hidden_numbers.push(t.text.clone()),
            None => {}
        }
    }
    out
}

/// Move and drop the movable texts as `p` says. A plan made for another
/// drawing (a different number of movable texts) changes nothing. The
/// texts' `Movable`s stay, pointing at their new indices, with no
/// alternatives left.
pub fn apply(mut d: Drawing, p: &Plan) -> Drawing {
    if p.spots.len() != d.movable.len() {
        return d;
    }
    let mut keep = vec![true; d.texts.len()];
    for (mv, spot) in d.movable.iter().zip(&p.spots) {
        let Some(t) = d.texts.get_mut(mv.text) else { continue };
        match spot {
            Some((off, anchor)) => {
                t.at += *off;
                t.anchor = *anchor;
            }
            None => keep[mv.text] = false,
        }
    }
    let mut new_index = vec![usize::MAX; d.texts.len()];
    let mut n = 0;
    for (i, k) in keep.iter().enumerate() {
        if *k {
            new_index[i] = n;
            n += 1;
        }
    }
    let mut i = 0;
    d.texts.retain(|_| {
        i += 1;
        keep[i - 1]
    });
    d.movable = d
        .movable
        .into_iter()
        .filter(|mv| new_index.get(mv.text).is_some_and(|&j| j != usize::MAX))
        .map(|mv| Movable { text: new_index[mv.text], alts: Vec::new(), ..mv })
        .collect();
    d
}

/// How legible a drawing is (spec §3.1), for tests and the owner's table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Audit {
    /// Pairs of drawn texts that overlap.
    pub overlaps: usize,
    /// Movable texts other than signal numbers touching track, a lamp, a
    /// ○A circle or a box; signal numbers touching anything but track.
    pub covered: usize,
    /// Signal numbers touching a track bar.
    pub tight: usize,
    /// Texts drawn, by role (headcodes and other fixed texts not counted).
    pub shown: BTreeMap<String, usize>,
}

pub fn audit(d: &Drawing, measure: &mut dyn FnMut(&TextItem) -> Vec2) -> Audit {
    let rects: Vec<Rect> = d.texts.iter().map(|t| t.anchor.anchor_size(t.at, measure(t))).collect();
    let mut a = Audit::default();
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            if overlaps(rects[i], rects[j]) {
                a.overlaps += 1;
            }
        }
    }
    for mv in &d.movable {
        let Some(&r) = rects.get(mv.text) else { continue };
        *a.shown.entry(format!("{:?}", mv.role)).or_default() += 1;
        let track = d.keep.bars.iter().any(|&b| touches_bar(r, b));
        let solid = d.keep.rounds.iter().any(|&c| touches_round(r, c)) || d.keep.boxes.iter().any(|&b| overlaps(r, b));
        match mv.role {
            Role::Number | Role::FringeNumber => {
                a.tight += usize::from(track);
                a.covered += usize::from(solid);
            }
            _ => a.covered += usize::from(track || solid),
        }
    }
    a
}
```

In `crates/client-ui/src/lib.rs`, after `pub mod hit;` add `pub mod labels;`.

In `crates/client-ui/src/paint.rs`:
- add `use crate::labels::{KeepClear, Movable, Role, corner};` after the `use crate::hit::…` line (`Role` and `corner` are used from Task 2 on; until then write only `use crate::labels::{KeepClear, Movable};` so the build has no unused imports, and widen it in Task 2);
- replace the `Drawing` struct with:

```rust
/// The font a text is drawn in.
pub fn font(t: &TextItem) -> FontId {
    if t.monospace { FontId::monospace(t.size) } else { FontId::proportional(t.size) }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawing {
    pub shapes: Vec<Shape>,
    pub texts: Vec<TextItem>,
    /// The texts `labels::plan` may move or hide (polish spec §3.3).
    pub movable: Vec<Movable>,
    /// What those texts must keep clear of.
    pub keep: KeepClear,
}
```

- and in `paint`, replace the loop body that builds `font` inline with:

```rust
    for t in d.texts {
        let f = font(&t);
        p.text(t.at, t.anchor, t.text, f, t.colour);
    }
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-ui --test labels`
Expected: PASS (11 tests). Then `scripts/cargo test -p signalbox-client-ui` — everything else still passes (nothing fills `movable` yet).

- [ ] **Step 5: Commit**

```bash
git add crates/client-ui/src/labels.rs crates/client-ui/src/lib.rs crates/client-ui/src/paint.rs crates/client-ui/tests/labels.rs
git commit -m "feat(client-ui): a pure placer for the diagram's texts, by priority, with what to keep clear"
```

---

### Task 2: `draw` records what may move and what to keep clear

**Files:**
- Modify: `crates/client-ui/src/paint.rs`, `crates/client-ui/src/hit.rs`, `crates/client-ui/src/scene.rs`
- Test: `crates/client-ui/tests/paint.rs`, `crates/client-ui/tests/labels.rs`

**Interfaces:**
- Consumes: Task 1's `labels::{Movable, KeepClear, Role, corner}`, `Drawing.movable`/`keep`.
- Produces: `paint::number_alts(base: Pos2, disc: Pos2, facing: Vec2, track_w: f32, has_auto: bool) -> Vec<(Pos2, Align2)>` (six spots); constants `paint::{NUMBER_CLEAR_PX = 1.5, AUTO_LETTER_PX = 9.0, AUTO_LETTER_GAP_PX = 1.0}`; `hit::{HEADCODE_CHAR_PX = 6.7, BERTH_PAD_PX = 6.0, berth_width(chars: usize) -> f32, berth_box(cam, screen, b: &BerthMark) -> Rect}`; `scene::BerthMark.width_px: f32`; `hit::auto_button` returns `None` below the number threshold.

- [ ] **Step 1: Write the failing tests**

In `crates/client-ui/tests/paint.rs`, in `controlled_signals_carry_a_blue_auto_button_hollow_off_filled_on`, replace the two lines that find the `A` beside the circle with:

```rust
    // Its `A` outward, above the circle, away from the track (polish spec P5).
    let a = d.texts.iter().find(|t| t.text == "A" && close(t.at, c + vec2(0.0, -(AUTO_R + AUTO_LETTER_GAP_PX)))).unwrap();
    assert_eq!((a.colour, a.anchor), (AUTO, Align2::CENTER_BOTTOM));
```

and append to the file:

```rust
// ---- the polish pass: placement data, ○A threshold, berth width ----

/// Polish spec P5: no ○A, drawn or hit, while numbers are too small to draw.
#[test]
fn no_auto_button_below_the_number_threshold() {
    let mut r = Rig::new(Some("West"));
    let w1 = r.sc.signals.iter().find(|s| s.name == "W1").unwrap().clone();
    assert!(client_ui::hit::auto_button(&r.cam, screen(), &w1).is_some());
    r.cam.scale = 0.3; // numbers 4.8 px: hidden
    let d = r.idle();
    assert!(circles(&d).iter().all(|k| k.2 != AUTO && k.3 != AUTO), "no ○A circles: {:?}", circles(&d));
    assert!(d.texts.iter().all(|t| t.text != "A"), "no letters");
    assert_eq!(client_ui::hit::auto_button(&r.cam, screen(), &w1), None, "nothing to click either");
}

/// `draw` lists what may move and what must be kept clear (polish spec §3.3).
#[test]
fn draw_records_movable_texts_and_what_to_keep_clear() {
    use client_ui::labels::Role;
    let r = Rig::new(Some("West"));
    let d = r.idle();
    let role_of = |text: &str| d.movable.iter().find(|m| d.texts[m.text].text == text).map(|m| m.role);
    assert_eq!(role_of("TAW1"), Some(Role::Number));
    assert_eq!(d.movable.iter().filter(|m| m.role == Role::AutoLetter).count(), 3, "W1, A and W2");
    let w1 = d.movable.iter().find(|m| d.texts[m.text].text == "TAW1").unwrap();
    assert_eq!(w1.alts.len(), 6, "six other spots for a number");
    assert!(d.movable.iter().all(|m| d.texts[m.text].text != "1E01"), "headcodes never move");
    // Every drawn track line is a bar; every lamp and ○A a round; every berth and exit a box.
    assert!(d.keep.bars.len() >= r.sc.tracks.len());
    assert!(d.keep.rounds.iter().any(|&(c, rad)| close(c, r.disc("W1")) && rad == LAMP_R));
    let auto = r.disc("W1") + vec2(client_ui::hit::AUTO_AHEAD_PX, 0.0);
    assert!(d.keep.rounds.iter().any(|&(c, rad)| close(c, auto) && rad == AUTO_R));
    assert_eq!(d.keep.boxes.len(), r.sc.berths.len() + r.sc.exits.len(), "every berth, empty or not, and every exit");
    let east = Rig::new(Some("East")).idle();
    let fringe = east.movable.iter().find(|m| east.texts[m.text].text == "TAA").map(|m| m.role);
    assert_eq!(fringe, Some(Role::FringeNumber), "West's A on East's fringe");
}

#[test]
fn a_numbers_other_spots_hug_the_track_then_mirror_it() {
    use client_ui::labels::corner;
    // Travel to the right: left of travel is up the screen.
    let (base, f) = (pos2(100.0, 100.0), vec2(1.0, 0.0));
    let disc = base + vec2(0.0, -POST_PX) + f * (HOOK_PX + LAMP_R);
    let alts = number_alts(base, disc, f, 6.0, false);
    let side = 3.0 + NUMBER_CLEAR_PX;
    assert_eq!(alts[0], (pos2(98.0, 100.0 - side), corner(vec2(-1.0, -1.0))), "behind the post, just clear of the track");
    assert_eq!(alts[0].1, Align2::RIGHT_BOTTOM);
    assert_eq!(alts[1].1, Align2::LEFT_BOTTOM, "ahead of the lamp");
    assert!(alts[1].0.x > disc.x + LAMP_R);
    assert_eq!(alts[4], (pos2(98.0, 100.0 + side), Align2::RIGHT_TOP), "the other side of the track");
    assert_eq!(alts[5], (pos2(102.0, 100.0 + side), Align2::LEFT_TOP));
    // With a ○A, the spot ahead clears the button.
    let with_auto = number_alts(base, disc, f, 6.0, true);
    assert!(with_auto[1].0.x >= disc.x + client_ui::hit::AUTO_AHEAD_PX + AUTO_R);
}

/// Gretz's headcodes are 7 characters: every berth box fits the longest
/// headcode the layout books, and never shrinks below `BERTH_W`.
#[test]
fn berth_boxes_fit_the_longest_headcode() {
    use client_ui::hit::{BERTH_W, berth_width};
    assert_eq!(berth_width(4), BERTH_W);
    assert!(berth_width(7) > 7.0 * 6.6, "{}", berth_width(7));
    let r = Rig::new(Some("West"));
    assert!(r.sc.berths.iter().all(|b| b.width_px == BERTH_W), "twobox's headcodes are 4 characters");
    let mut l = layout_for(Some("West"));
    l.simplifier.push(SimplifierRow { headcode: "W118400".into(), origin: None, destination: None, calls: vec![] });
    let sc = Scene::build(&l).unwrap();
    assert!(sc.berths.iter().all(|b| b.width_px == berth_width(7)));
}
```

In `crates/client-ui/tests/labels.rs`, add the Task-2 imports (`mod common;`, `use client_core::{AspectMode, Names};`, `use client_ui::camera::Camera;`, `use client_ui::paint::{Drawing, LABEL, PaintState, TextItem, draw};`, `use client_ui::scene::Scene;`, `use common::*;`) and the last test of the file shown in Task 1 (`a_plan_depends_on_neither_pan_nor_trains`).

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-ui --test paint --test labels`
Expected: compile errors (`number_alts`, `AUTO_LETTER_GAP_PX`, `NUMBER_CLEAR_PX`, `berth_width`, `width_px` not found).

- [ ] **Step 3: Implement**

`crates/client-ui/src/hit.rs`:
- the paint import becomes `use crate::paint::{AUTO_R, HOOK_PX, LAMP_R, POST_PX, left_of, number_px};` and the scene import `use crate::scene::{BerthMark, Scene, SignalMark, project};`
- `auto_button` (doc and guard):

```rust
/// Where a controlled signal's ○A button is: `AUTO_AHEAD_PX` ahead of its
/// lamp (past a second yellow); `None` for signals without one, and for
/// every signal while numbers are too small to draw (polish spec P5).
pub fn auto_button(cam: &Camera, screen: Rect, s: &SignalMark) -> Option<Pos2> {
    if !s.auto_button || number_px(cam.scale).is_none() {
        return None;
    }
```

- before `berth_rect` (whose doc becomes `/// A \`BERTH_W\` box at \`at\` moved by \`offset_px\`, on screen.`), add:

```rust
/// One headcode character at `paint::HEADCODE_PX` in egui's monospace font,
/// and the knock-out's margin round the text.
pub const HEADCODE_CHAR_PX: f32 = 6.7;
pub const BERTH_PAD_PX: f32 = 6.0;

/// A berth box wide enough for headcodes of `chars` characters, never
/// narrower than `BERTH_W` (Gretz's are 7 characters long).
pub fn berth_width(chars: usize) -> f32 {
    BERTH_W.max(chars as f32 * HEADCODE_CHAR_PX + BERTH_PAD_PX)
}

/// A berth's box on screen, as wide as the layout's longest headcode.
pub fn berth_box(cam: &Camera, screen: Rect, b: &BerthMark) -> Rect {
    Rect::from_center_size(cam.to_screen(screen, b.at) + b.offset_px, vec2(b.width_px, BERTH_H))
}
```

- in `hit_test`'s `berth` closure use `berth_box(cam, screen, b).contains(p)` instead of `berth_rect(cam, screen, b.at, b.offset_px).contains(p)`.

`crates/client-ui/src/scene.rs`:
- `BerthMark` gains, after `operable`:

```rust
    /// Its box's width on screen: the layout's longest headcode fits.
    pub width_px: f32,
```

- in `Scene::build`, right after `let mut sc = Scene::default();`:

```rust
        // Every berth box fits the longest headcode the layout books.
        let chars = l.simplifier.iter().map(|r| r.headcode.chars().count()).max().unwrap_or(0).max(4);
        let berth_w = crate::hit::berth_width(chars);
```

- and both `BerthMark { … }` literals (signal berths and boundary berths) get `width_px: berth_w,` after `operable: b.operable,`.

`crates/client-ui/src/paint.rs` (in order of the file):
1. Imports: `use crate::hit::{AUTO_AHEAD_PX, auto_button, berth_box, signal_disc};` and `use crate::labels::{KeepClear, Movable, Role, corner};`.
2. After `pub const LABEL_PX: f32 = 11.0;`:

```rust
/// A signal number's other spots stand this far clear of the track's edge.
pub const NUMBER_CLEAR_PX: f32 = 1.5;
/// The ○A button's `A`: text size, and its gap from the circle.
pub const AUTO_LETTER_PX: f32 = 9.0;
pub const AUTO_LETTER_GAP_PX: f32 = 1.0;
```

3. After the `Drawing` struct:

```rust
impl Drawing {
    /// Add a text that `labels::plan` may move to one of `alts` or hide.
    fn movable_text(&mut self, t: TextItem, role: Role, alts: Vec<(Pos2, Align2)>, within: Option<Rect>) {
        self.movable.push(Movable { text: self.texts.len(), role, alts, within });
        self.texts.push(t);
    }
}
```

4. In `track_shapes`, after the `bar(&mut d.shapes, a, b, …)` call: `d.keep.bars.push((a, b, w));`
5. In `signal_shapes`, replace the `if let (true, Some(size)) = (st.numbers, number_px(cam.scale)) { … }` block with:

```rust
    d.keep.rounds.push((disc, LAMP_R));
    if s.facing != Vec2::ZERO {
        // A second yellow's spot, kept clear whatever is shown (spec P3).
        d.keep.rounds.push((disc + s.facing * (LAMP_R * 2.2), LAMP_R));
    }
    if let (true, Some(size)) = (st.numbers, number_px(cam.scale)) {
        let side = if s.facing == Vec2::ZERO { vec2(0.0, -1.0) } else { left_of(s.facing) };
        let base = cam.to_screen(screen, s.base);
        let alts = number_alts(base, disc, s.facing, track_w(cam.scale), auto_button(cam, screen, s).is_some());
        let text = TextItem {
            at: disc + side * (LAMP_R + 2.0),
            anchor: anchor_towards(side),
            text: st.names.signal(&s.name),
            size,
            colour: if s.fringe { FRINGE } else { LABEL },
            monospace: true,
        };
        d.movable_text(text, if s.fringe { Role::FringeNumber } else { Role::Number }, alts, None);
    }
```

6. Before `pub fn draw`:

```rust
/// A signal number's other spots, best first (spec §3.3): hugging the track
/// behind the post, ahead of the lamp (past its ○A), one row further out
/// behind and ahead, and the two spots on the other side of the track.
/// `base` is the foot of the post and `disc` the lamp, on screen.
pub fn number_alts(base: Pos2, disc: Pos2, facing: Vec2, track_w: f32, has_auto: bool) -> Vec<(Pos2, Align2)> {
    let f = if facing == Vec2::ZERO { vec2(1.0, 0.0) } else { facing };
    let l = left_of(f);
    let side = track_w / 2.0 + NUMBER_CLEAR_PX;
    let ahead = HOOK_PX + LAMP_R + if has_auto { AUTO_AHEAD_PX + AUTO_R } else { LAMP_R } + 2.0;
    vec![
        (base + l * side - f * 2.0, corner(l - f)),
        (base + l * side + f * ahead, corner(l + f)),
        (disc + l * (LAMP_R + 2.0) - f * (LAMP_R + 2.0), corner(l - f)),
        (disc + l * (LAMP_R + 2.0) + f * (ahead - HOOK_PX - LAMP_R), corner(l + f)),
        (base - l * side - f * 2.0, corner(-l - f)),
        (base - l * side + f * 2.0, corner(-l + f)),
    ]
}
```

7. In `draw`:
   - platforms: replace the `d.texts.push(TextItem { at: r.center(), … })` with
     ```rust
        let text = TextItem { at: r.center(), anchor: Align2::CENTER_CENTER, text: p.label.clone(), size: 9.0, colour: BG, monospace: false };
        d.movable_text(text, Role::Platform, Vec::new(), Some(r));
     ```
   - points: after the `points_shapes(…)` call inside the loop add
     ```rust
        for leg in [p.toe, p.normal, p.reverse].into_iter().flatten() {
            d.keep.bars.push((to(p.at), to(leg), w));
        }
     ```
   - exits: after the `rect_stroke` push add `d.keep.boxes.push(r);`
   - berths: the loop starts
     ```rust
    for b in &scene.berths {
        let r = berth_box(cam, screen, b);
        // Every berth's box is kept clear, holding a headcode or not, so
        // nothing moves as trains run (spec P3).
        d.keep.boxes.push(r);
        let Some(h) = st.view.and_then(|v| v.berths.get(&b.name)) else { continue };
     ```
     (the old `let r = berth_rect(…)` line goes);
   - the ○A letter: replace the `let ahead = …; d.texts.push(TextItem { … "A" … });` with
     ```rust
            d.keep.rounds.push((c, AUTO_R));
            // The `A` outward, away from the track; else ahead, else on the inside.
            let ahead = if s.facing == Vec2::ZERO { vec2(1.0, 0.0) } else { s.facing };
            let out = left_of(ahead);
            let gap = AUTO_R + AUTO_LETTER_GAP_PX;
            let text = TextItem { at: c + out * gap, anchor: corner(out), text: "A".into(), size: AUTO_LETTER_PX, colour, monospace: true };
            let alts = vec![(c + ahead * gap, corner(ahead)), (c - out * gap, corner(-out))];
            d.movable_text(text, Role::AutoLetter, alts, None);
     ```
   - labels: the `None =>` arm becomes
     ```rust
            None => {
                let text = TextItem { at, anchor: Align2::LEFT_TOP, text: l.text.clone(), size: LABEL_PX, colour: LABEL, monospace: false };
                d.movable_text(text, Role::Label, Vec::new(), None);
            }
     ```
     and in the `Some(dir)` arm the `d.texts.push(TextItem { at: at + vec2(gap, 0.0), … })` becomes
     ```rust
                let text = TextItem { at: at + vec2(gap, 0.0), anchor, text: l.text.clone(), size: LABEL_PX, colour: LABEL, monospace: false };
                d.movable_text(text, Role::LineName, Vec::new(), None);
     ```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-ui`
Expected: PASS everywhere (paint 36, labels 12, hit, layouts, screens unchanged: `draw` still emits every text at its own spot; only `screens` will apply plans, in Task 3).

- [ ] **Step 5: Commit**

```bash
git add crates/client-ui/src/paint.rs crates/client-ui/src/hit.rs crates/client-ui/src/scene.rs crates/client-ui/tests/paint.rs crates/client-ui/tests/labels.rs
git commit -m "feat(client-ui): draw lists movable texts and what to keep clear; ○A only where numbers show; berth boxes fit the longest headcode"
```

---

### Task 3: Place the texts on screen, and the legibility measurement

**Files:**
- Modify: `crates/client-ui/src/screens.rs`, `CLAUDE.md`
- Test: `crates/client-ui/tests/legibility.rs` (new), `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: `labels::{plan, apply, Plan}`, `paint::{draw, font}`.
- Produces: `UiApp` field `placement: Option<(PlacementKey, Plan)>` with `type PlacementKey = (String, u64, u32, bool)` — (game id, layout generation, `cam.scale.to_bits()`, numbers on).

- [ ] **Step 1: Write the failing tests**

Create `crates/client-ui/tests/legibility.rs`:

```rust
//! Polish spec §3.4: the shipped layouts, every box and a spectator, at
//! Fit, 2× and 4× in a 1280 × 800 and a 1920 × 1080 window, drawn and
//! placed with egui's own fonts: no text over another, nothing over track,
//! lamps or boxes but tight signal numbers, every own number drawn at
//! 1280 × 800 Fit. Prints the table the owner reads (`--nocapture`).

use client_core::{AspectMode, Names};
use client_ui::camera::Camera;
use client_ui::labels::{self, Audit};
use client_ui::paint::{self, Drawing, PaintState, TextItem, draw};
use client_ui::scene::Scene;
use egui::{Context, RawInput, Rect, Vec2, pos2};
use game::{Game, GameMeta};
use protocol::{ClientMsg, Proposal};
use signalbox_core::world::World;

fn world(name: &str) -> World {
    let dir = env!("CARGO_MANIFEST_DIR");
    let read = |p: String| std::fs::read_to_string(p).unwrap();
    let mut w = ts2_import::convert(&read(format!("{dir}/../ts2-import/tests/data/{name}.json"))).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&read(format!("{dir}/../../layouts/{name}.areas.json"))).unwrap()).unwrap();
    ts2_import::lines::apply(&mut w, &ts2_import::lines::parse(&read(format!("{dir}/../../layouts/{name}.lines.json"))).unwrap()).unwrap();
    World::from_file(w).unwrap()
}

/// The diagram's rectangle in a window of each size, as `UiApp` lays it out
/// (measured with the screens rig: top bars 45 pt, side panel 398 pt).
fn windows() -> [(&'static str, Rect); 2] {
    [
        ("1280x800", Rect::from_min_max(pos2(0.0, 45.0), pos2(882.0, 800.0))),
        ("1920x1080", Rect::from_min_max(pos2(0.0, 45.0), pos2(1522.0, 1080.0))),
    ]
}

/// `d` with only the texts whose anchor is on `screen`: what the owner sees.
fn on_screen(mut d: Drawing, screen: Rect) -> Drawing {
    let keep: Vec<usize> = (0..d.texts.len()).filter(|&i| screen.contains(d.texts[i].at)).collect();
    d.movable.retain(|m| keep.contains(&m.text));
    for m in &mut d.movable {
        m.text = keep.iter().position(|&k| k == m.text).expect("kept");
    }
    d.texts = keep.iter().map(|&i| d.texts[i].clone()).collect();
    d
}

struct Row {
    window: &'static str,
    view: String,
    zoom: f32,
    audit: Audit,
    hidden: Vec<String>,
    /// How long `plan` took (debug builds are several times slower).
    ms: f64,
}

#[test]
fn every_view_is_legible_at_every_zoom() {
    let ctx = Context::default();
    let mut o = ctx.run_ui(RawInput::default(), |_| {});
    o.textures_delta.clear();
    let mut measure = |t: &TextItem| -> Vec2 { ctx.fonts_mut(|f| f.layout_no_wrap(t.text.clone(), paint::font(t), t.colour).size()) };
    let mut rows: Vec<Row> = Vec::new();
    for name in ["liverpool-st", "drain", "gretz-armainvilliers"] {
        let w = world(name);
        let areas: Vec<Option<String>> = std::iter::once(None).chain(w.net.areas.iter().map(|a| Some(a.name.clone()))).collect();
        let mut g = Game::new(w, GameMeta { layout: name.into(), seed: 1 });
        g.connect("sam");
        g.handle("sam", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
        for _ in 0..75 {
            g.advance(1.0);
        }
        for area in areas {
            match &area {
                Some(a) => g.handle("sam", ClientMsg::Claim { area: a.clone() }),
                None => g.handle("sam", ClientMsg::Release),
            };
            let (l, v) = (g.layout_of("sam").unwrap(), g.view_of("sam").unwrap());
            let sc = Scene::build(&l).unwrap();
            let names = Names::new(&l);
            let view = format!("{name} {}", area.as_deref().unwrap_or("spectator"));
            for (window, screen) in windows() {
                let fit = Camera::fit(sc.fit_bounds().unwrap(), screen);
                for zoom in [1.0_f32, 2.0, 4.0] {
                    let cam = Camera { centre: fit.centre, scale: fit.scale * zoom };
                    let st = PaintState {
                        view: Some(&v),
                        selected: None,
                        exits: &[],
                        refused: None,
                        time: 0.0,
                        aspects: AspectMode::RedGreen,
                        numbers: true,
                        names: &names,
                    };
                    let d = draw(&sc, &cam, screen, &st);
                    let t0 = std::time::Instant::now();
                    let plan = labels::plan(&d, &mut measure);
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    let audit = labels::audit(&on_screen(labels::apply(d, &plan), screen), &mut measure);
                    rows.push(Row { window, view: view.clone(), zoom, audit, hidden: plan.hidden_numbers.clone(), ms });
                }
            }
        }
    }
    println!("{:10} {:38} {:>4} {:>8} {:>7} {:>5} {:>6} {:>7}  shown", "window", "view", "zoom", "overlaps", "covered", "tight", "hidden", "plan ms");
    for r in &rows {
        println!(
            "{:10} {:38} {:>4} {:>8} {:>7} {:>5} {:>6} {:>7.2}  {:?} {:?}",
            r.window, r.view, r.zoom, r.audit.overlaps, r.audit.covered, r.audit.tight, r.hidden.len(), r.ms, r.audit.shown, r.hidden
        );
    }
    for r in &rows {
        assert_eq!(r.audit.overlaps, 0, "{} {} x{}: texts overlap", r.window, r.view, r.zoom);
        assert_eq!(r.audit.covered, 0, "{} {} x{}: texts cover track, lamps or boxes", r.window, r.view, r.zoom);
    }
    let fit_small: Vec<&Row> = rows.iter().filter(|r| r.window == "1280x800" && r.zoom == 1.0).collect();
    for r in fit_small.iter().filter(|r| !r.view.ends_with("spectator")) {
        assert!(r.hidden.is_empty(), "{}: every own number drawn at 1280x800 Fit, not {:?}", r.view, r.hidden);
    }
    let tight: usize = fit_small.iter().map(|r| r.audit.tight).sum();
    assert!(tight <= 4, "{tight} numbers tight against track at 1280x800 Fit (baseline 23)");
    let mut o = ctx.run_ui(RawInput::default(), |_| {});
    o.textures_delta.clear();
}
```

Append to `crates/client-ui/tests/screens.rs`:

```rust
/// Polish spec §3: the frame the player sees has no text drawn over
/// another, here Liverpool Street box A at Fit (4 overlaps before).
#[test]
fn the_diagram_never_draws_text_over_text() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let read = |p: String| std::fs::read_to_string(p).unwrap();
    let mut w = ts2_import::convert(&read(format!("{dir}/../ts2-import/tests/data/liverpool-st.json"))).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&read(format!("{dir}/../../layouts/liverpool-st.areas.json"))).unwrap()).unwrap();
    ts2_import::lines::apply(&mut w, &ts2_import::lines::parse(&read(format!("{dir}/../../layouts/liverpool-st.lines.json"))).unwrap()).unwrap();
    let mut r = Rig::in_game(signalbox_core::world::World::from_file(w).unwrap(), Some("Liverpool Street"));
    r.frame();
    let out = r.frame();
    let diagram = r.ui.diagram_rect().unwrap();
    let drawn: Vec<(String, Rect)> = texts(&out).into_iter().filter(|(_, at)| diagram.contains(at.center())).collect();
    assert!(drawn.len() > 50, "the box is drawn: {}", drawn.len());
    let mut overlaps = Vec::new();
    for (i, a) in drawn.iter().enumerate() {
        for b in &drawn[i + 1..] {
            let x = a.1.intersect(b.1);
            if x.width() > 0.5 && x.height() > 0.5 {
                overlaps.push((a.0.clone(), b.0.clone()));
            }
        }
    }
    assert!(overlaps.is_empty(), "{overlaps:?}");
}
```

- [ ] **Step 2: Run them**

Run: `scripts/cargo test -p signalbox-client-ui --test legibility --test screens -- --nocapture`
Expected: `legibility` PASSES already (it calls `labels::plan` itself; read its table — at 1280 × 800 Fit: 0 overlaps and 0 covered everywhere, tight 1 on Liverpool Street B and 1 on Drain Waterloo, nothing hidden in box views). `the_diagram_never_draws_text_over_text` FAILS with overlaps such as `("2", "2"), ("LA61", "BISHOPSGATE TUNNEL")`: the screen does not place yet. If `legibility` fails after the tutorial merge, stop and report the table rather than loosening a threshold.

- [ ] **Step 3: Apply the plan in `UiApp`**

In `crates/client-ui/src/screens.rs`:
- after `use crate::hit::hit_test;` add `use crate::labels::{self, Plan};`
- in `struct UiApp`, after `simplifier_lines`:

```rust
    /// Where the diagram's texts go, for (game, layout generation, scale
    /// bits, numbers on): made again only on a zoom or a settings change.
    placement: Option<(PlacementKey, Plan)>,
```

  and after the struct: `type PlacementKey = (String, u64, u32, bool);`; in `UiApp::new` add `placement: None,`.
- in `diagram_ui`, replace `paint::paint(&painter, paint::draw(scene, &cam, rect, &st));` with:

```rust
        let d = paint::draw(scene, &cam, rect, &st);
        let key = (g.id.clone(), g.layout_gen(), cam.scale.to_bits(), self.settings.numbers);
        if self.placement.as_ref().is_none_or(|(k, p)| *k != key || p.spots.len() != d.movable.len()) {
            let plan = ui.ctx().fonts_mut(|f| {
                labels::plan(&d, &mut |t| f.layout_no_wrap(t.text.clone(), paint::font(t), t.colour).size())
            });
            self.placement = Some((key, plan));
        }
        let d = match &self.placement {
            Some((_, plan)) => labels::apply(d, plan),
            None => d,
        };
        paint::paint(&painter, d);
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-ui`
Expected: PASS (screens 22, legibility 1, everything else).

- [ ] **Step 5: Document**

In `CLAUDE.md`, "Browser client", after the ○A bullet, add:

```markdown
- Legibility (polish spec §3): `paint::draw` emits every text at its own
  spot and records which may move (`labels::Movable`: role, other spots) and
  what must stay clear (`labels::KeepClear`: track bars, lamps, ○A circles,
  every berth box, exits); `labels::plan` places them greedily in priority
  order (own numbers, ○A letters, line names, platform numbers, labels,
  fringe numbers) and hides what has no room; headcodes never move. Plans
  depend only on scene, zoom and settings, and `UiApp` caches one per zoom.
  ○A buttons exist (drawn and hit) only where numbers are drawn.
  `tests/legibility.rs` is the acceptance measurement (`--nocapture` prints it).
```

- [ ] **Step 6: Commit**

```bash
git add crates/client-ui/src/screens.rs crates/client-ui/tests/legibility.rs crates/client-ui/tests/screens.rs CLAUDE.md
git commit -m "feat(client-ui): place the diagram's texts on screen, cached per zoom; legibility measured on the shipped layouts"
```

---

### Task 4a: The robot may wait at an automatic signal on plain line (spec P22)

Replaces the repeat timetable's Task 4 (spec P7/P8 withdrawn). The real WTT (Tasks 4b, 4c) gridlocks under today's
robot at 06:52 (spec §4.6); this is the one core change it needs.

**Files:**
- Modify: `crates/core/src/robot.rs` (`plan`, `shared_sections` → `route_users`, new `may_stand`), `CLAUDE.md`
- Test: `crates/ts2-import/tests/soak.rs`

**Interfaces:**
- Consumes: `robot::{commands, ROBOT_EVERY_TICKS}`, Drain (`crates/ts2-import/tests/data/drain.json`): signals 72/82 (Bank
  platforms 7/8) both route to automatic signal 73 over plain section T19 (`L1000003`); platform 26 is `L1000009`.
- Produces: no new public items; `robot::commands` sets routes to an automatic signal on plain line even where
  routes from several signals (all ending at it) use the sections the train would stand on.

- [ ] **Step 1: Write the failing test**

In `crates/ts2-import/tests/soak.rs` the `use` line becomes
`use signalbox_core::robot::{self, ROBOT_EVERY_TICKS, SoakReport, soak};`, and before the doc comment of
`liverpool_street_runs_three_hours` (`/// Three sim-hours from 05:00:15.`) add:

```rust
/// The robot lets a train wait at an automatic signal on plain line where
/// routes from two platforms meet before it (polish spec §4.6): a westbound
/// train leaves Bank for signal 73 while platform 26 is still occupied,
/// instead of waiting at Bank until it clears.
#[test]
fn drain_trains_wait_at_automatic_signal_73() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let mut w = ts2_import::convert(&std::fs::read_to_string(format!("{dir}/tests/data/drain.json")).unwrap()).unwrap().world;
    // A runs from Bank platform 8 into platform 26 and stands there; B leaves platform 7 behind it.
    let (services, entries) = (
        r#"[{"headcode": "A", "train_type": "UT", "calls": [{"place": "BNK", "platform": "8", "dep": "06:00:00"},
                {"place": "WTL", "platform": "26", "arr": "06:03:00", "dep": "23:00:00"}], "end": {"kind": "stable"}},
            {"headcode": "B", "train_type": "UT", "calls": [{"place": "BNK", "platform": "7", "dep": "06:04:00"},
                {"place": "WTL", "platform": "26", "arr": "06:08:00"}], "end": {"kind": "stable"}}]"#,
        r#"[{"service": "A", "at": {"segment": "L8", "offset_m": 79.0, "direction": "up"}, "time": "06:00:00"},
            {"service": "B", "at": {"segment": "L7", "offset_m": 79.0, "direction": "up"}, "time": "06:00:00"}]"#,
    );
    w.services = serde_json::from_str(services).unwrap();
    w.entries = serde_json::from_str(entries).unwrap();
    let mut sim = Sim::new(World::from_file(w).unwrap(), 7);
    for i in 0..(8 * 600) {
        if i % ROBOT_EVERY_TICKS == 0 {
            for c in robot::commands(&sim) {
                sim.submit(c);
            }
        }
        sim.step();
    }
    let b = sim.trains().iter().find(|t| t.headcode == "B").unwrap();
    let net = &sim.world().net;
    assert_eq!(net.segments[b.head().0.idx()].name, "L1000003", "B waits on the plain line at 73");
    assert_eq!(b.speed, 0.0);
    let a = sim.trains().iter().find(|t| t.headcode == "A").unwrap();
    assert_eq!(net.segments[a.head().0.idx()].name, "L1000009", "A still in platform 26");
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `scripts/cargo test -p ts2-import --test soak drain_trains_wait_at_automatic_signal_73`
Expected: FAIL, `B waits on the plain line at 73` with `left: "L7"` (B is still at Bank).

- [ ] **Step 3: Implement**

In `crates/core/src/robot.rs`, replace `plan` and `shared_sections` (from the doc comment of `plan` up to the doc
comment of `commands`, `/// Route requests for trains facing a red signal…`) with:

```rust
/// The routes from `entrance` the train should have set together: up to its
/// next stopping call or its exit, or to the first signal before that where
/// it could stand without fouling track that routes from other signals use
/// (`may_stand`).
fn plan(w: &World, t: &Train, entrance: SignalId, users: &[Vec<(SignalId, Exit)>]) -> Option<Vec<RouteId>> {
    let next = t.next_call + usize::from(t.dwell.is_some());
    let (full, _) = itinerary(w, t, entrance, next, None)?;
    let mut chain = Vec::new();
    for (r, ends_leg) in full {
        chain.push(r);
        let exit = w.routes[r.idx()].exit;
        let clear = matches!(exit, Exit::Signal(_))
            && footprint(w, &chain, t.length_m).iter().all(|&s| may_stand(w, users, s, exit));
        if ends_leg || clear {
            break;
        }
    }
    (!chain.is_empty()).then_some(chain)
}

/// For each section, the entrance and exit of every route whose path uses it.
fn route_users(w: &World) -> Vec<Vec<(SignalId, Exit)>> {
    let mut users: Vec<Vec<(SignalId, Exit)>> = vec![Vec::new(); w.net.sections.len()];
    for def in &w.routes {
        for &s in &def.path {
            if !users[s.idx()].contains(&(def.entrance, def.exit)) {
                users[s.idx()].push((def.entrance, def.exit));
            }
        }
    }
    users
}

/// Whether a train may stand on section `s` waiting at `exit`: no route
/// from another signal uses `s`, or `s` is plain line (no points) whose
/// routes all end at `exit` and `exit` is an automatic signal (polish spec
/// §4.6). There the train blocks nothing that could go anywhere else: it is
/// waiting in a block section, as on any plain line.
fn may_stand(w: &World, users: &[Vec<(SignalId, Exit)>], s: SectionId, exit: Exit) -> bool {
    let here = &users[s.idx()];
    if here.iter().all(|u| u.0 == here[0].0) {
        return true;
    }
    let automatic = match exit {
        Exit::Signal(x) => {
            let onward = &w.routes_from[x.idx()];
            !onward.is_empty() && onward.iter().all(|&o| w.routes[o.idx()].automatic)
        }
        Exit::Node(_) => false,
    };
    let plain = w.net.sections[s.idx()].segments.iter().all(|g| {
        let sg = &w.net.segments[g.idx()];
        [sg.a, sg.b].iter().all(|n| !matches!(w.net.nodes[n.idx()].kind, NodeKind::Points { .. }))
    });
    automatic && plain && here.iter().all(|u| u.1 == exit)
}
```

and in `commands`: `let shared = shared_sections(w);` becomes `let users = route_users(w);`, and
`let Some(chain) = plan(w, t, entrance, &shared) else { continue };` becomes
`let Some(chain) = plan(w, t, entrance, &users) else { continue };`. (`NodeKind`, `Exit`, `SectionId` and
`SignalId` are already in scope.)

- [ ] **Step 4: Run the tests to see them pass, and the soaks**

Run: `scripts/cargo test -p ts2-import --test soak` → PASS (3 passed, 1 ignored).
Run: `scripts/cargo test -p signalbox-core` → PASS.
Run: `scripts/cargo test --release -p ts2-import --test soak -- --ignored` → PASS (Liverpool Street, 3 h).
Run: `scripts/cargo test --release -p signalbox-bot --test soak -- --ignored` → PASS.
Run: `scripts/cargo test -p signalbox-game` → PASS.
For the branch report: `scripts/cargo run -q -p ts2-import -- crates/ts2-import/tests/data/liverpool-st.json -o /w/target/lst.json`
then `scripts/cargo run -q --release -p sim-cli -- run /w/target/lst.json --robot --hours 3` (scratch, on `0c0ea67`:
entered 76, exited 60, no stuck, `max_fringe_wait_s` 997; before this task 71, 58, none, 686).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Trains and the robot", after `…violations or stuck trains).` add: "The robot sets a train's routes
only all the way to its next stop, or to a signal where it fouls no route from another signal, or
(`robot::may_stand`, polish spec P22) to an automatic signal on plain line whose routes all end there."

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/robot.rs crates/ts2-import/tests/soak.rs CLAUDE.md
git commit -m "feat(core): the robot may hold a train at an automatic signal on plain line"
```

---

### Task 4b: Read the WTT and put it into Drain (`ts2-import --wtt`)

**Files:**
- Create: `crates/ts2-import/src/wtt.rs`, `crates/ts2-import/tests/wtt.rs`,
  `crates/ts2-import/tests/data/wtt-synthetic.py`, `crates/ts2-import/tests/data/wtt-synthetic.bbox.html` (generated)
- Modify: `crates/ts2-import/src/lib.rs`, `crates/ts2-import/src/main.rs`, `crates/ts2-import/tests/cli.rs`, `CLAUDE.md`

**Interfaces:**
- Consumes: `signalbox_core::world::file::{CallFile, EndFile, EntryFile, PositionFile, ServiceFile, WorldFile}`,
  `signalbox_core::time::fmt_hms`, `Sim::new` + `Network::first_signal_ahead` (to stand entering trains facing their
  starting signal), Drain's places `BNK` 7/8, `WTL` 25/26, `DPT` 5/6/7.
- Produces (Task 4c relies on these): `ts2_import::wtt::{parse(&str) -> Result<Vec<Trip>, WttError>, on_day(&[Trip], Day)
  -> Result<Vec<Trip>, WttError>, DAY, check(&[Trip], &Checks) -> Result<CheckReport, WttError>, Checks::waterloo_city(),
  apply(&mut WorldFile, &[Trip]) -> Result<ApplyReport, WttError>, headcode(&Trip) -> String, Trip, Bound, Day,
  WttError}`; CLI flag `--wtt <wtt.bbox.html>` (after `--areas` and `--lines`; exit 1 and nothing written if the
  WTT does not read or fails the checks).
- **Licence (spec §4.1):** nothing made from TfL's WTT is committed in this task or any other. The tests use only the
  synthetic fixture (fictional trains 301–303).

- [ ] **Step 1: The synthetic fixture**

Create `crates/ts2-import/tests/data/wtt-synthetic.py`:

```python
#!/usr/bin/env python3
# Writes wtt-synthetic.bbox.html: a made-up Working Timetable in the form
# `pdftotext -bbox` gives for LU WTTs (words with their boxes), for the tests
# of ts2_import::wtt. Fictional trains 301-303 and times; nothing in it comes
# from TfL's timetable. Rerun after editing:
#   python3 crates/ts2-import/tests/data/wtt-synthetic.py > crates/ts2-import/tests/data/wtt-synthetic.bbox.html
out = []
def word(x0, y0, w, h, t):
    t = t.replace('&', '&amp;')
    out.append(f'    <word xMin="{x0:.6f}" yMin="{y0:.6f}" xMax="{x0+w:.6f}" yMax="{y0+h:.6f}">{t}</word>')
def text(x, y, s, h=6.07, cw=3.6):
    for part in s.split():
        w = cw * len(part)
        word(x, y, w, h, part); x += w + 1.8
def centred(c, y, s, h=4.55):
    width = sum(3.0 * len(p) for p in s.split()) + 1.8 * (len(s.split()) - 1)
    text(c - width / 2, y, s, h, 3.0)
def time(c, y, hh, mm, frac=None, stacked=None, wash=False):
    if wash:
        word(c - 9.87, y, 18.16, 5.99, f'{hh}z{mm}'); x1 = c + 8.29
    else:
        word(c - 9.87, y, 7.18, 5.99, hh); word(c + 1.11, y, 7.18, 5.99, mm); x1 = c + 8.29
    if frac: word(x1, y + 0.17, 1.82, 5.89, frac)
    if stacked:
        n, d = stacked
        word(x1, y + 0.12, 1.63, 3.04, n); word(x1 + 0.2, y + 3.02, 1.63, 3.04, d)
def page(direction, rows, cols):
    out.append('  <page width="595.220000" height="842.000000">')
    text(42.83, 54.31, 'MONDAYS TO FRIDAYS', 8.4)
    text(466.45 if direction == 'WESTBOUND' else 42.83, 54.31 if direction == 'WESTBOUND' else 62.0, direction, 8.4)
    labels = {'train': 'Train No.', 'trip': 'Trip No.', 'crew': 'Crew Running No.', 'notes': 'Notes', 'pf': 'Platform No.',
              'bank': 'BANK', 'arr': 'arr.', 'dep': 'dep.', 'siding': 'Waterloo Siding', 'depot': 'Waterloo Depot',
              'toform': 'To form', 'by': 'By Crew Running No.'}
    for key, y in rows:
        x = 96.14 if key in ('arr', 'dep') else 49.45 if key == 'pf' else 38.2
        text(x, y, labels[key])
        for dots in (118.08,):
            text(dots, y, '.')
        if key == 'arr':
            text(38.2, y + 3.12, 'WATERLOO')
    y = dict(rows)
    for c, col in cols:
        for key, v in col.items():
            if key == 'extra':
                for dy, s in v:
                    centred(c, y['notes'] + dy, s)
            elif isinstance(v, tuple):
                time(c, y[key] + 0.08, *v[:2], **(v[2] if len(v) > 2 else {}))
            else:
                centred(c, y[key] + 0.08, v)
    out.append('  </page>')

out.append('<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.0 Transitional//EN" "http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd"><html xmlns="http://www.w3.org/1999/xhtml">')
out.append('<head>\n<title>Synthetic working timetable (test data)</title>\n</head>\n<body>\n<doc>')
# Page 1: a contents page (no train service).
out.append('  <page width="595.220000" height="842.000000">')
text(200, 100, 'SYNTHETIC LINE WORKING TIMETABLE')
text(200, 120, 'Train Service MONDAYS TO FRIDAYS')
out.append('  </page>')
W = [('train', 73.21), ('trip', 85.71), ('crew', 98.20), ('notes', 116.95), ('pf', 129.54), ('bank', 135.70),
     ('arr', 141.95), ('dep', 148.20), ('siding', 154.53), ('depot', 160.70), ('toform', 173.20), ('by', 179.45)]
page('WESTBOUND', W, [
    (142.0, {'train': '303', 'trip': '1', 'crew': '9', 'notes': 'Ety', 'extra': [(-6.16, 'Start')],
             'arr': 'Pfm 26', 'dep': ('05', '50'), 'depot': ('05', '52'), 'toform': 'Stop'}),
    (185.2, {'train': '302', 'trip': '1', 'crew': '2', 'notes': 'TThX', 'extra': [(-6.16, 'Start')], 'pf': '8',
             'bank': ('06', '05'), 'arr': ('06', '09'), 'dep': ('06', '10'), 'siding': ('06', '11'), 'toform': ('06', '12')}),
    (206.8, {'train': '302', 'trip': '2', 'crew': '2', 'notes': 'TThO', 'extra': [(-6.16, 'Start')], 'pf': '7',
             'bank': ('06', '05'), 'arr': ('06', '08', {'frac': '12'}), 'dep': ('06', '10'), 'siding': ('06', '11'), 'toform': ('06', '12')}),
    (250.0, {'train': '301', 'trip': '2', 'crew': '1', 'pf': '7', 'bank': ('06', '09'), 'arr': ('06', '12', {'frac': '12'}),
             'dep': ('06', '13', {'frac': '12'}), 'siding': ('06', '14', {'frac': '12'}), 'toform': ('06', '16'), 'by': '2'}),
    (336.5, {'train': '302', 'trip': '5', 'crew': '2', 'notes': 'WO', 'pf': '8', 'bank': ('06', '34', {'frac': '14'}),
             'arr': ('06', '38', {'stacked': ('1', '4')}), 'toform': 'Stop'}),
    (293.3, {'train': '301', 'trip': '4', 'crew': '1', 'pf': '7', 'bank': ('06', '24'), 'arr': ('06', '27', {'frac': '12'}),
             'dep': ('06', '28', {'frac': '12'}), 'depot': ('06', '30', {'frac': '12', 'wash': True}),
             'extra': [(49.0, 'Shed Rd')], 'toform': 'Stop'}),
])
E = [('train', 73.21), ('trip', 85.71), ('crew', 98.20), ('notes', 116.95), ('depot', 123.20), ('siding', 129.45),
     ('arr', 135.70), ('dep', 141.95), ('bank', 148.20), ('pf', 154.45), ('toform', 166.95), ('by', 173.20)]
page('EASTBOUND', E, [
    (142.0, {'train': '301', 'trip': '1', 'crew': '1', 'extra': [(-6.16, 'Start')], 'depot': ('06', '00'),
             'arr': ('06', '01', {'frac': '12'}), 'dep': ('06', '03'), 'bank': ('06', '07', {'frac': '14'}), 'pf': '7', 'toform': ('06', '09')}),
    (185.2, {'train': '302', 'trip': '3', 'crew': '2', 'siding': ('06', '12'), 'arr': ('06', '12', {'frac': '34'}),
             'dep': ('06', '13', {'frac': '12'}), 'bank': ('06', '17', {'frac': '12'}), 'pf': '8', 'toform': ('06', '34', {'stacked': ('1', '4')})}),
    (228.4, {'train': '301', 'trip': '3', 'crew': '1', 'siding': ('06', '16'), 'arr': ('06', '16', {'frac': '34'}),
             'dep': ('06', '18'), 'bank': ('06', '22', {'frac': '14'}), 'pf': '7', 'toform': ('06', '24')}),
])
# A Saturday page: never read.
out.append('  <page width="595.220000" height="842.000000">')
text(42.83, 54.31, 'SATURDAYS WESTBOUND', 8.4)
text(38.2, 73.21, 'Train No.'); text(140, 73.29, '309', 4.55)
out.append('  </page>')
out.append('</doc>\n</body>\n</html>')
print('\n'.join(out))
```

Run: `python3 crates/ts2-import/tests/data/wtt-synthetic.py > crates/ts2-import/tests/data/wtt-synthetic.bbox.html`
then `sha256sum crates/ts2-import/tests/data/wtt-synthetic.bbox.html`
Expected: `0d8e2207fe1d872ed5bab8b6ac58c612f9436b9bccaaa16b33094a9fec8e87bb` (256 lines).

- [ ] **Step 2: Write the failing tests**

Create `crates/ts2-import/tests/wtt.rs`:

```rust
//! Reading a Working Timetable (polish spec §4) from `pdftotext -bbox` text,
//! on a hand-made synthetic WTT (`tests/data/wtt-synthetic.bbox.html`,
//! written by `wtt-synthetic.py`: fictional trains 301–303 in the real WTT's
//! layout and notation), and putting it into Drain.

use signalbox_core::robot::soak;
use signalbox_core::sim::Sim;
use signalbox_core::time::parse_hms;
use signalbox_core::world::World;
use signalbox_core::world::file::{EndFile, WorldFile};
use ts2_import::wtt::{self, Bound, Checks, Day, Trip, WttError};

const SYNTHETIC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/wtt-synthetic.bbox.html");
const DRAIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/drain.json");

fn trips() -> Vec<Trip> {
    wtt::parse(&std::fs::read_to_string(SYNTHETIC).unwrap()).unwrap()
}

fn find(ts: &[Trip], train: u16, trip: u16) -> &Trip {
    ts.iter().find(|t| t.train == train && t.trip == trip).unwrap_or_else(|| panic!("no {train}/{trip}"))
}

fn at(s: &str) -> Option<u32> {
    parse_hms(s)
}

/// What the synthetic WTT says about itself.
fn checks() -> Checks {
    Checks {
        running: vec![("7", Bound::West, 210), ("8", Bound::West, 240), ("7", Bound::East, 255), ("8", Bound::East, 240)],
        snapshots: vec![(at("05:51").unwrap(), 0), (at("06:10").unwrap(), 2), (at("06:36").unwrap(), 1)],
        intervals: vec![(at("06:00").unwrap(), at("06:40").unwrap(), 585)],
    }
}

fn drain() -> WorldFile {
    ts2_import::convert(&std::fs::read_to_string(DRAIN).unwrap()).unwrap().world
}

#[test]
fn every_monday_to_friday_column_is_read() {
    let ts = trips();
    // Six westbound columns, three eastbound; the contents and Saturday pages are not timetables.
    assert_eq!(ts.len(), 9);
    assert!(ts.iter().all(|t| t.train != 309));
    let t = find(&ts, 301, 2);
    assert_eq!(t.bound, Bound::West);
    assert_eq!(t.platform.as_deref(), Some("7"));
    assert_eq!((t.bank, t.arr, t.dep, t.siding, t.to_form), (at("06:09"), at("06:12:30"), at("06:13:30"), at("06:14:30"), at("06:16")));
    // Fractions: `12` = ½, `14` = ¼, `34` = ¾, and a numerator stacked on its denominator.
    assert_eq!(find(&ts, 302, 3).arr, at("06:12:45"));
    assert_eq!(find(&ts, 302, 5).bank, at("06:34:15"));
    assert_eq!(find(&ts, 302, 5).arr, at("06:38:15"));
    assert_eq!(find(&ts, 302, 3).to_form, at("06:34:15"));
    // `06z30` is 06:30 with the train-wash mark; `Stop` ends a working.
    let t = find(&ts, 301, 4);
    assert_eq!((t.depot, t.wash, t.to_form), (at("06:30:30"), true, None));
    assert!(t.has("Shed") && t.has("Rd"));
    // `Pfm 26`: a move that starts standing in a Waterloo platform.
    let t = find(&ts, 303, 1);
    assert_eq!((t.starts_in.as_deref(), t.dep, t.depot), (Some("26"), at("05:50"), at("05:52")));
    assert!(t.has("Start") && t.has("Ety"));
    let t = find(&ts, 301, 1);
    assert_eq!((t.bound, t.depot, t.arr, t.dep, t.bank), (Bound::East, at("06:00"), at("06:01:30"), at("06:03"), at("06:07:15")));
}

#[test]
fn day_codes_pick_one_weekday() {
    let ts = trips();
    let wed: Vec<(u16, u16)> = wtt::on_day(&ts, Day::Wed).unwrap().iter().map(|t| (t.train, t.trip)).collect();
    assert!(wed.contains(&(302, 1)) && wed.contains(&(302, 5)) && !wed.contains(&(302, 2)), "{wed:?}");
    let tue: Vec<(u16, u16)> = wtt::on_day(&ts, Day::Tue).unwrap().iter().map(|t| (t.train, t.trip)).collect();
    assert!(tue.contains(&(302, 2)) && !tue.contains(&(302, 1)) && !tue.contains(&(302, 5)), "{tue:?}");
    let mut odd = ts.clone();
    odd[0].notes.push("QO".into());
    assert_eq!(wtt::on_day(&odd, Day::Wed), Err(WttError::DayCode("QO".into(), odd[0].train, odd[0].trip)));
}

#[test]
fn a_day_is_checked_against_what_the_wtt_says() {
    let day = wtt::on_day(&trips(), Day::Wed).unwrap();
    let r = wtt::check(&day, &checks()).unwrap();
    assert_eq!((r.trips, r.trains, r.running_exact, r.running_longer, r.links), (8, 3, 7, 0, 5));
    assert_eq!(r.intervals, vec![(at("06:00").unwrap(), at("06:40").unwrap(), 585)]);
    let mut c = checks();
    c.snapshots[1].1 = 3;
    assert!(matches!(wtt::check(&day, &c), Err(WttError::Check(m)) if m.contains("2 trains in service at 06:10:00")));
    let mut fast = day.clone();
    fast.iter_mut().find(|t| (t.train, t.trip) == (301, 2)).unwrap().arr = at("06:12");
    assert!(matches!(wtt::check(&fast, &checks()), Err(WttError::Check(m)) if m.contains("under the published")));
    let mut broken = day.clone();
    broken.iter_mut().find(|t| (t.train, t.trip) == (301, 2)).unwrap().to_form = at("06:17");
    assert!(matches!(wtt::check(&broken, &checks()), Err(WttError::Check(m)) if m.contains("301 trip 2 forms")));
}

#[test]
fn a_day_goes_into_drain() {
    let day = wtt::on_day(&trips(), Day::Wed).unwrap();
    let mut w = drain();
    let r = wtt::apply(&mut w, &day).unwrap();
    assert_eq!((r.services, r.entries, r.start_time.as_str()), (7, 2, "05:50:00"));
    assert_eq!((r.dropped_empty, r.dropped_trains), (vec!["303/1".to_string()], vec![303]));
    let heads: Vec<&str> = w.services.iter().map(|s| s.headcode.as_str()).collect();
    assert_eq!(heads, ["301/1", "301/2", "301/3", "301/4", "302/1", "302/3", "302/5"]);
    let svc = |h: &str| w.services.iter().find(|s| s.headcode == h).unwrap();
    let calls = |h: &str| svc(h).calls.iter().map(|c| format!("{} {}", c.place, c.platform.clone().unwrap_or_default())).collect::<Vec<_>>();
    assert_eq!(calls("302/1"), ["BNK 8", "WTL 26", "DPT 6"]);
    assert_eq!(calls("302/3"), ["DPT 6", "WTL 25", "BNK 8"]);
    assert_eq!(calls("301/1"), ["DPT 5", "WTL 25", "BNK 7"]);
    assert_eq!(calls("301/4"), ["BNK 7", "WTL 26", "DPT 5"]);
    assert!(matches!(&svc("301/1").end, EndFile::Form { service } if service == "301/2"));
    assert!(matches!(svc("301/4").end, EndFile::Stable), "to the depot for the night");
    let last = svc("302/5").calls.last().unwrap();
    assert_eq!((last.place.as_str(), last.stop, last.arr.as_deref()), ("WTL", false, Some("06:38:15")));
    assert!(matches!(svc("302/5").end, EndFile::Stable), "the last train in stables in platform 26");
    assert_eq!(w.options.start_time, "05:50:00");
    let entry = |h: &str| w.entries.iter().find(|e| e.service == h).unwrap();
    assert_eq!((entry("301/1").time.as_str(), entry("301/1").at.as_ref().unwrap().segment.as_str()), ("05:50:00", "L1000021"));
    assert_eq!((entry("302/1").time.as_str(), entry("302/1").at.as_ref().unwrap().segment.as_str()), ("05:50:00", "L8"));
    let again = {
        let mut w2 = drain();
        wtt::apply(&mut w2, &day).unwrap();
        serde_json::to_string(&w2).unwrap()
    };
    assert_eq!(serde_json::to_string(&w).unwrap(), again, "byte-identical");
    World::from_file(w).unwrap();
}

#[test]
fn the_synthetic_day_runs_under_the_robot() {
    let mut w = drain();
    wtt::apply(&mut w, &wtt::on_day(&trips(), Day::Wed).unwrap()).unwrap();
    let mut sim = Sim::new(World::from_file(w).unwrap(), 7);
    let r = soak(&mut sim, 3600.0);
    assert_eq!((r.spads, r.collisions, r.invariant_violations), (0, 0, 0), "{r:?}");
    assert!(r.stuck.is_empty() && r.still_running.is_empty(), "{r:?}");
    assert_eq!((r.entered, r.stabled), (2, 2), "{r:?}");
}

#[test]
fn a_wrong_file_is_refused() {
    assert_eq!(wtt::parse("<html><body>not a timetable</body></html>"), Err(WttError::NoPages));
    let text = std::fs::read_to_string(SYNTHETIC).unwrap().replacen(">06</word>", ">6a</word>", 1);
    assert!(matches!(wtt::parse(&text), Err(WttError::Format(..))), "a garbled time");
}
```

In `crates/ts2-import/tests/cli.rs`: the first doc line becomes ``//! The converter CLI's `--areas`, `--lines` and `--wtt` flags.``;
after `DRAIN_LINES` add

```rust
const SYNTHETIC_WTT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/wtt-synthetic.bbox.html");
```

and append:

```rust
/// `--wtt` checks the WTT against the Waterloo & City figures before it
/// writes anything: the synthetic test WTT is read but fails them, and the
/// image build stops rather than shipping a timetable that is not the real one.
#[test]
fn wtt_flag_checks_the_timetable_and_writes_nothing_when_it_fails() {
    let dir = temp_dir("wtt");
    let out = dir.join("drain.json");
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--wtt", SYNTHETIC_WTT]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("check failed: 0 trains in service at 09:00:00, the WTT says 5"), "{}", stderr(&o));
    assert!(!out.exists());
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--wtt", dir.join("missing.html").to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(!out.exists());
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--wtt"]);
    assert_eq!(o.status.code(), Some(2), "--wtt needs a value");
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 3: Run them to see them fail**

Run: `scripts/cargo test -p ts2-import --test wtt --test cli`
Expected: compile errors (`could not find wtt in ts2_import`).

- [ ] **Step 4: Implement**

Create `crates/ts2-import/src/wtt.rs`:

```rust
//! The Waterloo & City line's Working Timetable (polish spec §4): read from
//! `pdftotext -bbox` output of the owner's own copy of LU WTT No. 7, checked
//! against the figures the WTT itself publishes, and put into the Drain world
//! as its timetable. Only this code is in the repository: the PDF and anything
//! made from it stay outside it (`external/wtt/`, git-ignored).
//!
//! The WTT's train-service pages are tables, one column per trip, one row per
//! timing point. `pdftotext -bbox` gives every word with its box, so rows and
//! columns are found by position, never by counting spaces: fractions of a
//! minute are small words abutting the minutes (`12` = ½, `14` = ¼, `34` = ¾,
//! or a numerator and denominator stacked), and `23z57` is 23:57 with the
//! train-wash mark. Deterministic: same input, same output.

use std::collections::BTreeMap;

use signalbox_core::network::Dir;
use signalbox_core::sim::Sim;
use signalbox_core::time::fmt_hms;
use signalbox_core::world::World;
use signalbox_core::world::file::{CallFile, EndFile, EntryFile, PositionFile, ServiceFile, WorldFile};

/// Row labels sit left of this (PDF points).
const LABEL_X: f64 = 100.0;
/// Table cells sit right of this.
const DATA_X: f64 = 128.0;
/// A word belongs to the row whose label is at most this far above or below.
const ROW_TOL: f64 = 4.0;
/// A cell belongs to the column whose train number is centred at most this far away.
const COL_TOL: f64 = 10.0;
/// Stacked fraction digits are shorter than this; every other word is taller.
const STACK_H: f64 = 4.0;
/// WTT times before this hour are after midnight (the line is shut 01:00–05:00).
const NIGHT_H: u32 = 4;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum WttError {
    #[error("page {0}: {1}")]
    Format(usize, String),
    #[error("no Monday to Friday train service pages found")]
    NoPages,
    #[error("unknown day code `{0}` on train {1} trip {2}")]
    DayCode(String, u16, u16),
    #[error("check failed: {0}")]
    Check(String),
    #[error("cannot put into the world: {0}")]
    Apply(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bound {
    /// Bank → Waterloo.
    West,
    /// Waterloo → Bank.
    East,
}

/// One column of the train service pages. Times are seconds since the
/// midnight before the service day (so 00:30 is 24:30).
#[derive(Debug, Clone, PartialEq)]
pub struct Trip {
    pub train: u16,
    pub trip: u16,
    pub bound: Bound,
    /// Words in the notes rows: day codes, `Start`, `Ety`, `YW`, `Shed`, `Rd`.
    pub notes: Vec<String>,
    /// Bank platform.
    pub platform: Option<String>,
    pub bank: Option<u32>,
    /// Waterloo arrival (westbound: platform 26; eastbound: platform 25).
    pub arr: Option<u32>,
    /// `Pfm 25`/`Pfm 26` in the arrival row: the move starts standing in that platform.
    pub starts_in: Option<String>,
    pub dep: Option<u32>,
    pub siding: Option<u32>,
    pub depot: Option<u32>,
    /// A time with the train-wash mark `z`.
    pub wash: bool,
    /// The next trip's start; `None` for `Stop`.
    pub to_form: Option<u32>,
}

impl Trip {
    pub fn has(&self, note: &str) -> bool {
        self.notes.iter().any(|n| n == note)
    }

    fn times(&self) -> impl Iterator<Item = u32> + '_ {
        [self.bank, self.arr, self.dep, self.siding, self.depot].into_iter().flatten()
    }

    pub fn first(&self) -> u32 {
        self.times().min().unwrap_or(0)
    }

    pub fn last(&self) -> u32 {
        self.times().max().unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
struct Word {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    text: String,
}

fn attr(tag: &str, name: &str) -> Option<f64> {
    let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[at..].find('"')? + at;
    tag[at..end].parse().ok()
}

fn decode(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The words of each page of `pdftotext -bbox` output.
fn pages(xhtml: &str) -> Vec<Vec<Word>> {
    let mut out = Vec::new();
    for page in xhtml.split("<page ").skip(1) {
        let mut words = Vec::new();
        let mut rest = page;
        while let Some(at) = rest.find("<word ") {
            rest = &rest[at..];
            let (Some(close), Some(end)) = (rest.find('>'), rest.find("</word>")) else { break };
            let tag = &rest[..close];
            if let (Some(x0), Some(y0), Some(x1), Some(y1)) = (attr(tag, "xMin"), attr(tag, "yMin"), attr(tag, "xMax"), attr(tag, "yMax")) {
                words.push(Word { x0, y0, x1, y1, text: decode(&rest[close + 1..end]) });
            }
            rest = &rest[end..];
        }
        out.push(words);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Row {
    Train,
    Trip,
    Crew,
    Notes,
    Platform,
    Bank,
    Arr,
    Dep,
    Siding,
    Depot,
    ToForm,
    ByCrew,
}

/// Row labels (left of `LABEL_X`) by height.
fn anchors(words: &[Word]) -> Vec<(f64, Row)> {
    let mut out = Vec::new();
    for w in words.iter().filter(|w| w.x0 < LABEL_X) {
        let next = words
            .iter()
            .filter(|v| (v.y0 - w.y0).abs() < 0.5 && v.x0 > w.x1 && v.x0 < w.x1 + 5.0)
            .map(|v| v.text.as_str())
            .next();
        let row = match (w.text.as_str(), next) {
            ("Train", Some("No.")) => Row::Train,
            ("Trip", _) => Row::Trip,
            ("Crew", _) => Row::Crew,
            ("Notes", _) => Row::Notes,
            ("Platform", _) => Row::Platform,
            ("BANK", _) => Row::Bank,
            ("arr.", _) => Row::Arr,
            ("dep.", _) => Row::Dep,
            ("Waterloo", Some("Siding")) => Row::Siding,
            ("Waterloo", Some("Depot")) => Row::Depot,
            ("To", Some("form")) => Row::ToForm,
            ("By", _) => Row::ByCrew,
            _ => continue,
        };
        out.push((w.y0, row));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

#[derive(Debug, Clone)]
enum Tok {
    /// Hours, minutes (once read), seconds of fraction, wash mark.
    Time { h: u32, m: Option<u32>, frac: u32, wash: bool, x0: f64, x1: f64, y0: f64 },
    Text { text: String, x0: f64, x1: f64 },
}

impl Tok {
    fn centre(&self) -> f64 {
        match self {
            Tok::Time { x0, x1, .. } | Tok::Text { x0, x1, .. } => (x0 + x1) / 2.0,
        }
    }
}

fn two_digits(s: &str) -> Option<u32> {
    (s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

/// One row's words (sorted by x) as times and texts.
fn tokens(row: Row, words: &[&Word], stacked: &[&Word], page: usize) -> Result<Vec<Tok>, WttError> {
    let timed = matches!(row, Row::Bank | Row::Arr | Row::Dep | Row::Siding | Row::Depot | Row::ToForm);
    let mut out: Vec<Tok> = Vec::new();
    for w in words {
        let t = w.text.as_str();
        if let Some(Tok::Time { m, frac, x1, .. }) = out.last_mut() {
            if m.is_none() && (2.0..5.0).contains(&(w.x0 - *x1)) {
                if let Some(v) = two_digits(t) {
                    *m = Some(v);
                    *x1 = w.x1;
                    continue;
                }
            }
            if m.is_some() && *frac == 0 && (w.x0 - *x1).abs() < 0.4 {
                let f = match t {
                    "14" => Some(15),
                    "12" => Some(30),
                    "34" => Some(45),
                    _ => None,
                };
                if let Some(f) = f {
                    *frac = f;
                    *x1 = w.x1;
                    continue;
                }
            }
        }
        if timed {
            let b = t.as_bytes();
            if b.len() == 5 && b[2] == b'z' {
                if let (Some(h), Some(m)) = (two_digits(&t[..2]), two_digits(&t[3..])) {
                    out.push(Tok::Time { h, m: Some(m), frac: 0, wash: true, x0: w.x0, x1: w.x1, y0: w.y0 });
                    continue;
                }
            }
            let after_pfm = matches!(out.last(), Some(Tok::Text { text, .. }) if text == "Pfm");
            if let (Some(h), false) = (two_digits(t), after_pfm) {
                out.push(Tok::Time { h, m: None, frac: 0, wash: false, x0: w.x0, x1: w.x1, y0: w.y0 });
                continue;
            }
        }
        out.push(Tok::Text { text: w.text.clone(), x0: w.x0, x1: w.x1 });
    }
    for tok in &mut out {
        if let Tok::Time { h, m, frac, x1, y0, .. } = tok {
            if m.is_none() {
                return Err(WttError::Format(page, format!("hours {h:02} without minutes")));
            }
            let mut st: Vec<&&Word> =
                stacked.iter().filter(|s| (s.x0 - *x1).abs() < 0.6 && (-1.0..5.0).contains(&(s.y0 - *y0))).collect();
            if !st.is_empty() {
                st.sort_by(|a, b| a.y0.total_cmp(&b.y0));
                let digits: Vec<u32> = st.iter().filter_map(|s| s.text.parse().ok()).collect();
                *frac = match digits.as_slice() {
                    [1, 4] => 15,
                    [1, 2] => 30,
                    [3, 4] => 45,
                    _ => return Err(WttError::Format(page, format!("odd stacked fraction {digits:?}"))),
                };
            }
        }
    }
    Ok(out)
}

fn seconds(h: u32, m: u32, frac: u32) -> u32 {
    let h = if h < NIGHT_H { h + 24 } else { h };
    h * 3600 + m * 60 + frac
}

/// Every Monday-to-Friday trip, in page and column order.
pub fn parse(xhtml: &str) -> Result<Vec<Trip>, WttError> {
    let mut trips = Vec::new();
    for (pi, words) in pages(xhtml).into_iter().enumerate() {
        let page = pi + 1;
        let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
        let mf = texts.windows(3).any(|w| w == ["MONDAYS", "TO", "FRIDAYS"]) && !texts.contains(&"SATURDAYS");
        let bound = match (texts.contains(&"WESTBOUND"), texts.contains(&"EASTBOUND")) {
            (true, false) => Bound::West,
            (false, true) => Bound::East,
            _ => continue,
        };
        if !mf {
            continue;
        }
        let words: Vec<Word> = words.into_iter().filter(|w| !w.text.chars().all(|c| c == '.')).collect();
        let an = anchors(&words);
        let starts: Vec<f64> = an.iter().filter(|a| a.1 == Row::Train).map(|a| a.0).collect();
        for (bi, &y0) in starts.iter().enumerate() {
            let y1 = starts.get(bi + 1).copied().unwrap_or(f64::INFINITY);
            let rows: Vec<(f64, Row)> = an.iter().copied().filter(|a| a.0 >= y0 - 1.0 && a.0 < y1 - 1.0).collect();
            let data: Vec<&Word> = words.iter().filter(|w| w.x0 >= DATA_X && w.y0 >= y0 - 1.0 && w.y0 < y1 - 1.0).collect();
            let mut cols: Vec<(f64, u16)> = Vec::new();
            for w in data.iter().filter(|w| (w.y0 - y0).abs() < 1.0) {
                let n = w.text.parse().map_err(|_| WttError::Format(page, format!("train number `{}`", w.text)))?;
                cols.push(((w.x0 + w.x1) / 2.0, n));
            }
            cols.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (stacked, plain): (Vec<&Word>, Vec<&Word>) = data
                .iter()
                .copied()
                .partition(|w| w.y1 - w.y0 < STACK_H && w.text.len() == 1 && w.text.as_bytes()[0].is_ascii_digit());
            // Each word to its row (or an unlabelled notes line).
            let mut by_row: BTreeMap<(Option<Row>, i64), Vec<&Word>> = BTreeMap::new();
            for w in plain {
                let near = rows.iter().min_by(|a, b| (a.0 - w.y0).abs().total_cmp(&(b.0 - w.y0).abs()));
                let key = match near {
                    Some(&(y, r)) if (y - w.y0).abs() <= ROW_TOL => (Some(r), 0),
                    _ => (None, w.y0.round() as i64),
                };
                by_row.entry(key).or_default().push(w);
            }
            let mut cells: Vec<BTreeMap<Row, Vec<Tok>>> = vec![BTreeMap::new(); cols.len()];
            let mut extra: Vec<Vec<String>> = vec![Vec::new(); cols.len()];
            for ((row, _), mut ws) in by_row {
                ws.sort_by(|a, b| a.x0.total_cmp(&b.x0));
                for tok in tokens(row.unwrap_or(Row::Notes), &ws, &stacked, page)? {
                    let c = cols
                        .iter()
                        .enumerate()
                        .min_by(|a, b| (a.1.0 - tok.centre()).abs().total_cmp(&(b.1.0 - tok.centre()).abs()))
                        .filter(|(_, c)| (c.0 - tok.centre()).abs() <= COL_TOL)
                        .map(|(i, _)| i)
                        .ok_or_else(|| WttError::Format(page, format!("a cell in no column: {tok:?}")))?;
                    match row {
                        Some(r) => cells[c].entry(r).or_default().push(tok),
                        None => {
                            if let Tok::Text { text, .. } = tok {
                                extra[c].push(text);
                            }
                        }
                    }
                }
            }
            for (c, (cell, extra)) in cells.into_iter().zip(extra).enumerate() {
                trips.push(trip(page, bound, cols[c].1, cell, extra)?);
            }
        }
    }
    if trips.is_empty() {
        return Err(WttError::NoPages);
    }
    Ok(trips)
}

fn trip(page: usize, bound: Bound, train: u16, mut cell: BTreeMap<Row, Vec<Tok>>, extra: Vec<String>) -> Result<Trip, WttError> {
    let err = |what: String| WttError::Format(page, format!("train {train}: {what}"));
    let mut take = |r: Row| cell.remove(&r).unwrap_or_default();
    let text = |toks: &[Tok]| -> Vec<String> {
        toks.iter().filter_map(|t| if let Tok::Text { text, .. } = t { Some(text.clone()) } else { None }).collect()
    };
    let mut wash = false;
    let mut time = |r: Row, toks: Vec<Tok>| -> Result<Option<u32>, WttError> {
        let ts: Vec<u32> = toks
            .iter()
            .filter_map(|t| match t {
                Tok::Time { h, m: Some(m), frac, wash: z, .. } => {
                    wash |= *z;
                    Some(seconds(*h, *m, *frac))
                }
                _ => None,
            })
            .collect();
        match ts.as_slice() {
            [] => Ok(None),
            [t] => Ok(Some(*t)),
            _ => Err(err(format!("{} times in row {r:?}", ts.len()))),
        }
    };
    let number = |toks: &[Tok], r: Row| -> Result<u16, WttError> {
        match text(toks).as_slice() {
            [n] => n.parse().map_err(|_| err(format!("{r:?} `{n}`"))),
            other => Err(err(format!("{r:?} {other:?}"))),
        }
    };
    let trip_no = number(&take(Row::Trip), Row::Trip)?;
    let mut notes = text(&take(Row::Notes));
    notes.extend(extra);
    let platform = text(&take(Row::Platform)).first().cloned();
    let arr_toks = take(Row::Arr);
    let starts_in = match text(&arr_toks).as_slice() {
        [] => None,
        [p] if p.starts_with("Pfm") => p.strip_prefix("Pfm").map(|n| n.trim().to_string()).filter(|n| !n.is_empty()),
        [p, n] if p == "Pfm" => Some(n.clone()),
        other => return Err(err(format!("arrival {other:?}"))),
    };
    let to_form_toks = take(Row::ToForm);
    let stop = text(&to_form_toks) == ["Stop"];
    let bank = time(Row::Bank, take(Row::Bank))?;
    let arr = time(Row::Arr, arr_toks)?;
    let dep = time(Row::Dep, take(Row::Dep))?;
    let siding = time(Row::Siding, take(Row::Siding))?;
    let depot = time(Row::Depot, take(Row::Depot))?;
    let to_form = time(Row::ToForm, to_form_toks)?;
    if stop == to_form.is_some() {
        return Err(err(format!("trip {trip_no}: `To form` must be a time or `Stop`")));
    }
    Ok(Trip { train, trip: trip_no, bound, notes, platform, bank, arr, starts_in, dep, siding, depot, wash, to_form })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Day {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
}

/// The weekday Drain runs (spec P15): Wednesday, the plain midweek day.
pub const DAY: Day = Day::Wed;

/// The days a note like `MO`, `TThX` or `MWO` names, and whether they are
/// the only days (`O`) or the excepted ones (`X`); `None` if it is not a day code.
fn day_code(note: &str) -> Option<(Vec<Day>, bool)> {
    let (body, only) = match note.as_bytes().last()? {
        b'O' => (&note[..note.len() - 1], true),
        b'X' => (&note[..note.len() - 1], false),
        _ => return None,
    };
    let mut days = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        let (d, n) = if rest.starts_with("Th") {
            (Day::Thu, 2)
        } else {
            match rest.as_bytes()[0] {
                b'M' => (Day::Mon, 1),
                b'T' => (Day::Tue, 1),
                b'W' => (Day::Wed, 1),
                b'F' => (Day::Fri, 1),
                _ => return None,
            }
        };
        days.push(d);
        rest = &rest[n..];
    }
    (!days.is_empty()).then_some((days, only))
}

/// Notes that are neither day codes nor one of these are an error, so a
/// different WTT cannot slip an unknown restriction past the importer.
const PLAIN_NOTES: [&str; 6] = ["Start", "Ety", "YW", "Shed", "Rd", "RR"];

/// The trips that run on `day`.
pub fn on_day(trips: &[Trip], day: Day) -> Result<Vec<Trip>, WttError> {
    let mut out = Vec::new();
    for t in trips {
        let mut runs = true;
        for n in &t.notes {
            match day_code(n) {
                Some((days, only)) => runs &= days.contains(&day) == only,
                None if PLAIN_NOTES.contains(&n.as_str()) => {}
                None => return Err(WttError::DayCode(n.clone(), t.train, t.trip)),
            }
        }
        if runs {
            out.push(t.clone());
        }
    }
    Ok(out)
}

/// What the WTT says about itself, to check a parse against.
#[derive(Debug, Clone)]
pub struct Checks {
    /// Bank platform, bound, published running time in seconds (Waterloo ⇄ that platform).
    pub running: Vec<(&'static str, Bound, u32)>,
    /// Time, trains in service.
    pub snapshots: Vec<(u32, usize)>,
    /// From, to, mean interval between Bank departures (seconds).
    pub intervals: Vec<(u32, u32, u32)>,
}

const fn hm(h: u32, m: u32) -> u32 {
    h * 3600 + m * 60
}

impl Checks {
    /// WTT No. 7, page 2. The snapshot table prints 3 trains at 21:00, but its
    /// own workings (page 5: 201 finishes at 21:37) give 4, and so does every
    /// trip; the table is taken to predate the revision that lengthened the
    /// evening peak.
    pub fn waterloo_city() -> Checks {
        Checks {
            running: vec![("7", Bound::West, 210), ("8", Bound::West, 240), ("7", Bound::East, 255), ("8", Bound::East, 240)],
            snapshots: vec![
                (hm(6, 0), 1),
                (hm(9, 0), 5),
                (hm(12, 0), 3),
                (hm(15, 0), 3),
                (hm(18, 0), 5),
                (hm(21, 0), 4),
                (hm(24, 0), 2),
            ],
            intervals: vec![
                (hm(7, 30), hm(9, 30), 165),
                (hm(11, 0), hm(15, 30), 300),
                (hm(16, 30), hm(19, 45), 165),
                (hm(19, 45), hm(21, 30), 210),
                (hm(21, 30), hm(23, 30), 360),
                (hm(23, 30), hm(26, 0), 600),
            ],
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CheckReport {
    pub trips: usize,
    pub trains: usize,
    /// Trips run in exactly the published time, and trips given longer.
    pub running_exact: usize,
    pub running_longer: usize,
    pub links: usize,
    pub snapshots: Vec<(u32, usize)>,
    /// From, to, mean interval in seconds (rounded).
    pub intervals: Vec<(u32, u32, u32)>,
}

/// Each train's trips in running order.
fn by_train(trips: &[Trip]) -> BTreeMap<u16, Vec<&Trip>> {
    let mut out: BTreeMap<u16, Vec<&Trip>> = BTreeMap::new();
    for t in trips {
        out.entry(t.train).or_default().push(t);
    }
    for v in out.values_mut() {
        v.sort_by_key(|t| (t.first(), t.trip));
    }
    out
}

fn running(t: &Trip) -> Option<u32> {
    match t.bound {
        Bound::West => Some(t.arr?.checked_sub(t.bank?)?),
        Bound::East => Some(t.bank?.checked_sub(t.dep?)?),
    }
}

/// Check one day's trips against the WTT's own figures (spec §4.3).
pub fn check(trips: &[Trip], c: &Checks) -> Result<CheckReport, WttError> {
    let mut r = CheckReport { trips: trips.len(), ..Default::default() };
    let fail = |s: String| Err(WttError::Check(s));
    for t in trips {
        let (Some(rt), Some(p)) = (running(t), t.platform.as_deref()) else { continue };
        let Some(&(_, _, want)) = c.running.iter().find(|x| x.0 == p && x.1 == t.bound) else {
            return fail(format!("train {} trip {}: no running time for platform {p}", t.train, t.trip));
        };
        match rt.cmp(&want) {
            std::cmp::Ordering::Less => {
                return fail(format!("train {} trip {} runs in {rt} s, under the published {want} s", t.train, t.trip));
            }
            std::cmp::Ordering::Equal => r.running_exact += 1,
            std::cmp::Ordering::Greater => r.running_longer += 1,
        }
    }
    let trains = by_train(trips);
    r.trains = trains.len();
    // Service periods: runs of trips linked by `To form`, with whether any carries passengers.
    let mut periods: Vec<(u32, u32, bool)> = Vec::new();
    for (n, ts) in &trains {
        let mut cur: Option<(u32, u32, bool)> = None;
        for (i, t) in ts.iter().enumerate() {
            let p = cur.get_or_insert((t.first(), t.last(), false));
            p.1 = t.last();
            p.2 |= !t.has("Ety");
            match (t.to_form, ts.get(i + 1)) {
                (Some(f), Some(next)) => {
                    if next.first() != f || next.bound == t.bound || t.last() > f {
                        return fail(format!("train {n} trip {} forms {} at {}, not trip {}", t.trip, fmt_hms(f.into()), fmt_hms(next.first().into()), next.trip));
                    }
                    r.links += 1;
                }
                (Some(f), None) => return fail(format!("train {n} trip {} forms a trip at {} that is not there", t.trip, fmt_hms(f.into()))),
                (None, next) => {
                    if let Some(next) = next {
                        if !next.has("Start") || next.first() < t.last() {
                            return fail(format!("train {n} trip {} stops but trip {} does not start", t.trip, next.trip));
                        }
                    }
                    periods.extend(cur.take());
                }
            }
        }
        periods.extend(cur);
    }
    for &(at, want) in &c.snapshots {
        let n = periods.iter().filter(|p| p.2 && p.0 <= at && at < p.1).count();
        r.snapshots.push((at, n));
        if n != want {
            return fail(format!("{} trains in service at {}, the WTT says {want}", n, fmt_hms(at.into())));
        }
    }
    let mut deps: Vec<u32> = trips.iter().filter(|t| t.bound == Bound::West).filter_map(|t| t.bank).collect();
    deps.sort();
    for &(from, to, want) in &c.intervals {
        let xs: Vec<u32> = deps.iter().copied().filter(|d| (from..=to).contains(d)).collect();
        if xs.len() < 2 {
            return fail(format!("fewer than two Bank departures {}–{}", fmt_hms(from.into()), fmt_hms(to.into())));
        }
        let mean = f64::from(xs[xs.len() - 1] - xs[0]) / (xs.len() - 1) as f64;
        r.intervals.push((from, to, mean.round() as u32));
        if (mean - f64::from(want)).abs() > 6.0 {
            return fail(format!("Bank departures {}–{} every {mean:.0} s, the WTT says {want} s", fmt_hms(from.into()), fmt_hms(to.into())));
        }
    }
    Ok(r)
}

/// Where the WTT's places are on Drain (spec §4.4).
pub const BANK: &str = "BNK";
pub const WATERLOO: &str = "WTL";
pub const ARRIVAL: &str = "26";
pub const DEPARTURE: &str = "25";
/// Waterloo roads 5, 6 and 7 behind the platforms: both the reversing siding and the depot.
pub const ROADS: (&str, [&str; 3]) = ("DPT", ["5", "6", "7"]);
/// Minimum time between one train leaving a road and the next arriving in it.
const ROAD_GAP_S: u32 = 60;
/// A train coming out of the depot appears this long before it leaves.
const APPEAR_S: u32 = 600;

/// The headcode of a trip (spec P18): train number and trip number.
pub fn headcode(t: &Trip) -> String {
    format!("{}/{}", t.train, t.trip)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApplyReport {
    pub services: usize,
    pub entries: usize,
    /// Empty moves left out (train/trip).
    pub dropped_empty: Vec<String>,
    /// Trains left out because they only run empty.
    pub dropped_trains: Vec<u16>,
    /// Trains ended early to free a road for the night, and where they stable.
    pub shortened: Vec<String>,
    /// Stays in Waterloo roads, by road.
    pub road_use: Vec<(String, usize)>,
    pub start_time: String,
}

/// A stay in a Waterloo road: (train index in `chains`, trip index of the trip that arrives, or
/// `None` for a train that appears there), from, to (`None`: to the end of the day).
#[derive(Debug, Clone, Copy)]
struct Stay {
    chain: usize,
    arrives: Option<usize>,
    from: u32,
    to: Option<u32>,
}

/// Put one day's trips into `w` (Drain) in place of its timetable (spec §4.4).
pub fn apply(w: &mut WorldFile, day: &[Trip]) -> Result<ApplyReport, WttError> {
    let mut rep = ApplyReport::default();
    let fail = |s: String| WttError::Apply(s);
    let mut chains: Vec<Vec<Trip>> = Vec::new();
    for (n, ts) in by_train(day) {
        let kept: Vec<Trip> = ts.iter().filter(|t| !t.has("Ety")).map(|t| (*t).clone()).collect();
        rep.dropped_empty.extend(ts.iter().filter(|t| t.has("Ety")).map(|t| headcode(t)));
        if kept.is_empty() {
            rep.dropped_trains.push(n);
        } else {
            chains.push(kept);
        }
    }
    for ch in &chains {
        if let Some(w) = ch.windows(2).find(|w| w[0].bound == w[1].bound) {
            return Err(fail(format!("{} and {} run the same way one after the other", headcode(&w[0]), headcode(&w[1]))));
        }
        for t in ch {
            let ok = match t.bound {
                Bound::West => t.bank.is_some() && t.arr.is_some() && t.platform.is_some() && t.starts_in.is_none(),
                Bound::East => t.bank.is_some() && t.dep.is_some() && t.platform.is_some() && (t.siding.is_some() || t.depot.is_some()),
            };
            if !ok {
                return Err(fail(format!("trip {} is not a Bank–Waterloo run", headcode(t))));
            }
        }
    }
    let first_dep = chains.iter().map(|c| c[0].first()).min().ok_or_else(|| fail("no trips".into()))?;
    let start = (first_dep.saturating_sub(APPEAR_S)) / 300 * 300;
    // Allocate roads; a train whose last stay leaves no road for later ones ends at Bank instead.
    let roads = loop {
        let stays = stays(&chains, start);
        match allocate(&stays) {
            Ok(r) => break r.into_iter().zip(stays).collect::<Vec<_>>(),
            Err(blocked_at) => {
                let open: Vec<&Stay> = stays.iter().filter(|s| s.to.is_none() && s.from <= blocked_at).collect();
                let Some(&&last) = open.iter().max_by_key(|s| s.from) else {
                    return Err(fail(format!("no Waterloo road free at {}", fmt_hms(blocked_at.into()))));
                };
                rep.shortened.push(shorten(&mut chains, last.chain)?);
            }
        }
    };
    let mut road_of: BTreeMap<(usize, Option<usize>), &str> = BTreeMap::new();
    let mut use_count: BTreeMap<&str, usize> = BTreeMap::new();
    for (r, s) in &roads {
        road_of.insert((s.chain, s.arrives), ROADS.1[*r]);
        *use_count.entry(ROADS.1[*r]).or_default() += 1;
    }
    rep.road_use = use_count.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    let fmt = |t: Option<u32>| t.map(|v| fmt_hms(v.into()));
    let call = |place: &str, pf: &str, arr: Option<u32>, dep: Option<u32>| CallFile {
        place: place.into(),
        platform: Some(pf.into()),
        arr: fmt(arr),
        dep: fmt(dep),
        stop: true,
    };
    let train_type = w.train_types.first().map(|t| t.code.clone()).ok_or_else(|| fail("no train type".into()))?;
    let mut services = Vec::new();
    let mut entries = Vec::new();
    for (ci, ch) in chains.iter().enumerate() {
        for (i, t) in ch.iter().enumerate() {
            let bank_pf = t.platform.as_deref().expect("checked above");
            let calls = match t.bound {
                Bound::West => {
                    let mut c = vec![call(BANK, bank_pf, None, t.bank), call(WATERLOO, ARRIVAL, t.arr, t.dep)];
                    if let Some(at) = t.siding.or(t.depot) {
                        let road = road_of.get(&(ci, Some(i))).ok_or_else(|| fail(format!("{}: no road", headcode(t))))?;
                        c.push(call(ROADS.0, road, Some(at), None));
                    } else {
                        // Its last call: run into platform 26 and stand at its starting
                        // signal. A booked stop there would wait for that signal to clear
                        // before the train could stable; a timed pass does not.
                        c[1].dep = None;
                        c[1].stop = false;
                    }
                    c
                }
                Bound::East => {
                    let road = match i {
                        0 => road_of.get(&(ci, None)),
                        _ => road_of.get(&(ci, Some(i - 1))),
                    }
                    .ok_or_else(|| fail(format!("{}: no road", headcode(t))))?;
                    vec![call(ROADS.0, road, None, t.siding.or(t.depot)), call(WATERLOO, DEPARTURE, t.arr, t.dep), call(BANK, bank_pf, t.bank, None)]
                }
            };
            let end = match ch.get(i + 1) {
                Some(n) => EndFile::Form { service: headcode(n) },
                None => EndFile::Stable,
            };
            services.push(ServiceFile { headcode: headcode(t), train_type: train_type.clone(), calls, end });
        }
        let first = &services[services.len() - ch.len()];
        let (place, pf, time) = match ch[0].bound {
            Bound::West => (BANK, ch[0].platform.clone().unwrap_or_default(), start),
            Bound::East => {
                let road = road_of[&(ci, None)];
                (ROADS.0, road.to_string(), ch[0].first().saturating_sub(APPEAR_S).max(start))
            }
        };
        entries.push(EntryFile {
            service: first.headcode.clone(),
            boundary: None,
            at: Some(stand(w, place, &pf)?),
            time: fmt_hms(time.into()),
            speed_kmh: 0.0,
            on_demand: false,
        });
    }
    entries.sort_by(|a, b| a.time.cmp(&b.time).then(a.service.cmp(&b.service)));
    rep.services = services.len();
    rep.entries = entries.len();
    rep.start_time = fmt_hms(start.into());
    w.services = services;
    w.entries = entries;
    w.options.start_time = rep.start_time.clone();
    w.options.min_dwell_s = [20, 30];
    World::from_file(w.clone()).map_err(|e| fail(format!("the world no longer loads: {e}")))?;
    Ok(rep)
}

/// Every stay in a Waterloo road, in time order.
fn stays(chains: &[Vec<Trip>], start: u32) -> Vec<Stay> {
    let mut out = Vec::new();
    for (ci, ch) in chains.iter().enumerate() {
        if ch[0].bound == Bound::East {
            let leave = ch[0].first();
            out.push(Stay { chain: ci, arrives: None, from: leave.saturating_sub(APPEAR_S).max(start), to: Some(leave) });
        }
        for (i, t) in ch.iter().enumerate() {
            if let (Bound::West, Some(at)) = (t.bound, t.siding.or(t.depot)) {
                out.push(Stay { chain: ci, arrives: Some(i), from: at, to: ch.get(i + 1).map(|n| n.first()) });
            }
        }
    }
    out.sort_by_key(|s| (s.from, s.chain));
    out
}

/// The road (index into `ROADS`) for each stay: the one free longest. `Err`
/// is the time a stay found none.
fn allocate(stays: &[Stay]) -> Result<Vec<usize>, u32> {
    let mut free_from: [Option<u32>; 3] = [Some(0); 3];
    let mut out = Vec::new();
    for s in stays {
        // The road free longest, so a late train is least likely to find its road still taken.
        let r = (0..3).filter(|&r| free_from[r].is_some_and(|f| f <= s.from)).min_by_key(|&r| (free_from[r], r)).ok_or(s.from)?;
        free_from[r] = s.to.map(|t| t + ROAD_GAP_S);
        out.push(r);
    }
    Ok(out)
}

/// End chain `ci` at its last Bank arrival instead, in a Bank platform no
/// later trip uses, so its last road stay is no longer needed.
fn shorten(chains: &mut [Vec<Trip>], ci: usize) -> Result<String, WttError> {
    let ch = &chains[ci];
    let Some(k) = ch.iter().rposition(|t| t.bound == Bound::East) else {
        return Err(WttError::Apply(format!("train {} never reaches Bank", ch[0].train)));
    };
    let arrive = ch[k].bank.unwrap_or(0);
    let used_later = |pf: &str| {
        chains.iter().enumerate().filter(|(i, _)| *i != ci).flat_map(|(_, c)| c).any(|t| t.platform.as_deref() == Some(pf) && t.bank.is_some_and(|b| b >= arrive))
    };
    let pf = ["7", "8"].into_iter().find(|p| !used_later(p)).ok_or_else(|| {
        WttError::Apply(format!("train {}: no Bank platform free from {}", chains[ci][0].train, fmt_hms(arrive.into())))
    })?;
    let ch = &mut chains[ci];
    ch.truncate(k + 1);
    ch[k].platform = Some(pf.to_string());
    ch[k].to_form = None;
    Ok(format!("{} stables at Bank platform {pf} at {}", headcode(&ch[k]), fmt_hms(arrive.into())))
}

/// A train standing in `place` platform `pf`, its head 1 m short of the end
/// that faces the platform's starting signal.
fn stand(w: &WorldFile, place: &str, pf: &str) -> Result<PositionFile, WttError> {
    let p = w
        .platforms
        .iter()
        .find(|p| p.place == place && p.platform == pf)
        .ok_or_else(|| WttError::Apply(format!("Drain has no platform {place} {pf}")))?;
    let mut probe = w.clone();
    probe.services.clear();
    probe.entries.clear();
    let world = World::from_file(probe).map_err(|e| WttError::Apply(e.to_string()))?;
    let sim = Sim::new(world, 0);
    let net = &sim.world().net;
    let seg = net.segments.iter().position(|s| s.name == p.segment).expect("platform segments exist");
    let sg = &net.segments[seg];
    let mut found = Vec::new();
    for (dir, offset) in [(Dir::Up, p.to_m - 1.0), (Dir::Down, p.from_m + 1.0)] {
        let id = signalbox_core::ids::SegmentId::from_idx(seg);
        if let Some((_, d)) = net.first_signal_ahead(id, dir, sg.along(offset, dir), 30.0, sim.points()) {
            found.push((d, dir, offset));
        }
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let &(_, dir, offset_m) = found.first().ok_or_else(|| WttError::Apply(format!("no starting signal for {place} {pf}")))?;
    Ok(PositionFile { segment: p.segment.clone(), offset_m, direction: dir })
}
```

In `crates/ts2-import/src/lib.rs`, after `pub mod ts2;` add `pub mod wtt;`.

`crates/ts2-import/src/main.rs`:
- `use ts2_import::{areas, convert, lines, report, wtt};`
- `USAGE` ends `[--lines <lines.json>] [--wtt <wtt.bbox.html>]";`
- after the `let (mut input, …) = …;` line: `let mut wtt_path = None;`
- a match arm before `"--strict"`:

```rust
            "--wtt" => {
                i += 1;
                match args.get(i) {
                    Some(t) => wtt_path = Some(t.clone()),
                    None => return usage(),
                }
            }
```

- before `let text = match std::fs::read_to_string(&input) {`:

```rust
    let day = match &wtt_path {
        Some(p) => match read_wtt(p) {
            Ok(d) => Some((p.clone(), d)),
            Err(e) => {
                eprintln!("{p}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
```

- before `let json = serde_json::to_string_pretty(&c.world)…` (after the `--lines` block):

```rust
            if let Some((p, (trips, checked))) = &day {
                match wtt::apply(&mut c.world, trips) {
                    Ok(r) => {
                        eprintln!(
                            "{p}: Wednesday, {} trips of {} trains; {} trips at the published running time, {} longer; snapshots {}",
                            checked.trips,
                            checked.trains,
                            checked.running_exact,
                            checked.running_longer,
                            checked.snapshots.iter().map(|(t, n)| format!("{}={n}", &signalbox_core::time::fmt_hms((*t).into())[..5])).collect::<Vec<_>>().join(" ")
                        );
                        eprintln!(
                            "{p}: {} services, {} entries from {}; left out {} empty moves and trains {:?}; roads {:?}",
                            r.services,
                            r.entries,
                            r.start_time,
                            r.dropped_empty.len(),
                            r.dropped_trains,
                            r.road_use
                        );
                        for s in &r.shortened {
                            eprintln!("{p}: {s}");
                        }
                    }
                    Err(e) => {
                        eprintln!("{p}: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
```

- before `fn usage() -> ExitCode {`:

```rust
/// The WTT's Wednesday trips, checked against its own figures.
fn read_wtt(path: &str) -> Result<(Vec<wtt::Trip>, wtt::CheckReport), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let all = wtt::parse(&text).map_err(|e| e.to_string())?;
    let day = wtt::on_day(&all, wtt::DAY).map_err(|e| e.to_string())?;
    let checked = wtt::check(&day, &wtt::Checks::waterloo_city()).map_err(|e| e.to_string())?;
    Ok((day, checked))
}
```

- [ ] **Step 5: Run the tests to see them pass**

Run: `scripts/cargo test -p ts2-import`
Expected: PASS (wtt 6, cli 6, soak 3 + 1 ignored; the convert snapshots unchanged: `convert` is untouched).
Then the CI build: `docker run --rm -u "$(id -u):$(id -g)" -v "$PWD:/w" -w /w -e CARGO_HOME=/w/.cargo-home -e RUSTFLAGS="-D warnings" rust:1.98-slim-bookworm cargo build -p ts2-import --all-targets --locked`
Expected: no warnings.

- [ ] **Step 6: Document**

`CLAUDE.md`:
- Commands block, after the Liverpool Street `ts2-import` line:
  `scripts/cargo run -p ts2-import -- crates/ts2-import/tests/data/drain.json -o /w/target/drain.json --areas /w/layouts/drain.areas.json --lines /w/layouts/drain.lines.json --wtt /w/external/wtt/wtt.bbox.html   # the real WTT: external/wtt/README.md`
- "Multiplayer", after the lines-file sentence: "Drain's timetable can be the real Waterloo & City WTT
  (`ts2-import --wtt`, `ts2_import::wtt`, polish spec §4): it reads `pdftotext -bbox` output of the owner's PDF,
  checks it against the WTT's own figures (running times, workings, trains in service, intervals) and replaces
  Drain's services, entries and start time (Wednesday; headcodes `<train>/<trip>`; the depot and siding are roads
  5–7). The PDF and anything made from it are never committed (`external/wtt/`, git-ignored); CI tests only the
  synthetic `tests/data/wtt-synthetic.bbox.html` (written by `wtt-synthetic.py`)."

- [ ] **Step 7: Commit**

```bash
git add crates/ts2-import CLAUDE.md
git commit -m "feat(ts2-import): read a Waterloo & City WTT (pdftotext -bbox), check it, and make it Drain's timetable"
```

---

### Task 4c: The image gets the owner's WTT; the whole-day soak

**Files:**
- Create: `external/wtt/README.md`, `external/wtt/.gitignore`, `crates/ts2-import/tests/wtt_day.rs`
- Modify: `deploy/Dockerfile`, `deploy/README.md`, `CLAUDE.md`

**Interfaces:**
- Consumes: Task 4b's `wtt::{parse, on_day, DAY, check, Checks::waterloo_city, apply}` and `--wtt`; Task 4a's robot.
- Produces: the `wtt` Docker stage (`/out/wtt.bbox.html` when `external/wtt/` holds exactly one PDF); the image's
  `drain.json` from the WTT when present; `scripts/cargo test --release -p ts2-import --test wtt_day -- --ignored --nocapture`
  (skips without `external/wtt/wtt.bbox.html`).

- [ ] **Step 1: The ignored directory**

Create `external/wtt/.gitignore`:

```text
# The owner's WTT and everything made from it stay out of the repository.
*
!.gitignore
!README.md
```

Create `external/wtt/README.md`:

```markdown
# The Waterloo & City Working Timetable (not in this repository)

Drain can run the real London Underground Waterloo & City line timetable:
WTT No. 7, Mondays to Fridays, from 9 October 2017 (a TfL document). Only the
code that reads it is in this repository (`crates/ts2-import/src/wtt.rs`);
the PDF, its text and anything made from them are never committed (this
directory ignores everything but this file and its `.gitignore`).

To build an image with it, put your copy here as the only PDF:

    external/wtt/wtt-7-waterloo-and-city-2017-10-09.pdf

(any name ending `.pdf`; sha256
`7709d5b56564dd5b0d9acd7d27cb2668fc5a88407d6dae6f389a8e6fb592475b` for the
copy this was written against). `deploy/Dockerfile` turns it into
`pdftotext -bbox` text and converts Drain with `ts2-import --wtt`, which
checks the timetable against the WTT's own figures (running times, train
workings, trains in service, service intervals) and stops the build if they
do not match. Without a PDF the image's Drain keeps its TS2 timetable.

The image then holds a timetable made from TfL's document: keep it on ra,
never push it to a public registry.

To try it outside Docker (poppler-utils installed):

    pdftotext -bbox external/wtt/*.pdf external/wtt/wtt.bbox.html
    scripts/cargo run -p ts2-import -- crates/ts2-import/tests/data/drain.json -o /w/target/drain.json \
      --areas /w/layouts/drain.areas.json --lines /w/layouts/drain.lines.json --wtt /w/external/wtt/wtt.bbox.html
    scripts/cargo test --release -p ts2-import --test wtt_day -- --ignored --nocapture
```

Run: `git status --short --ignored external/`
Expected: only `?? external/` (or the two files once added); a PDF dropped there shows as `!!`.

- [ ] **Step 2: The whole-day soak (owner-run)**

Create `crates/ts2-import/tests/wtt_day.rs`:

```rust
//! The owner's real WTT as Drain's timetable (polish spec §4), run for a
//! whole day under the robot. Needs the git-ignored
//! `external/wtt/wtt.bbox.html` (deploy/README.md says how to make it) and
//! is skipped without it; slow in debug builds:
//! `scripts/cargo test --release -p ts2-import --test wtt_day -- --ignored --nocapture`

use std::collections::BTreeMap;

use signalbox_core::events::Event;
use signalbox_core::robot::{self, ROBOT_EVERY_TICKS, STUCK_S};
use signalbox_core::sim::{Sim, TICK_S};
use signalbox_core::time::fmt_hms;
use signalbox_core::world::World;
use signalbox_core::world::file::WorldFile;
use ts2_import::wtt;

const WTT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../external/wtt/wtt.bbox.html");

fn drain_with_wtt() -> Option<WorldFile> {
    let text = std::fs::read_to_string(WTT).ok()?;
    let dir = env!("CARGO_MANIFEST_DIR");
    let mut w = ts2_import::convert(&std::fs::read_to_string(format!("{dir}/tests/data/drain.json")).unwrap()).unwrap().world;
    let areas = ts2_import::areas::parse(&std::fs::read_to_string(format!("{dir}/../../layouts/drain.areas.json")).unwrap()).unwrap();
    ts2_import::areas::apply(&mut w, &areas).unwrap();
    let day = wtt::on_day(&wtt::parse(&text).unwrap(), wtt::DAY).unwrap();
    wtt::check(&day, &wtt::Checks::waterloo_city()).unwrap();
    wtt::apply(&mut w, &day).unwrap();
    Some(w)
}

#[derive(Debug, Default)]
struct Day {
    spads: usize,
    collisions: usize,
    violations: usize,
    wrong_platform: usize,
    stuck: Vec<String>,
    running: Vec<String>,
    stabled: usize,
    /// Lateness in seconds at each booked stop and each departure, with headcode and time.
    arrivals: Vec<(i64, String, f64)>,
    departures: Vec<(i64, String, f64)>,
}

/// Until 01:00, as `robot::soak` runs a world, also timing every call.
fn run_day(w: WorldFile, seed: u64) -> Day {
    let mut sim = Sim::new(World::from_file(w).unwrap(), seed);
    let ticks = ((25.0 * 3600.0 - sim.now_s()) / TICK_S).round() as u64;
    let mut d = Day::default();
    let mut still: BTreeMap<u32, (usize, f64, f64)> = BTreeMap::new();
    for i in 0..ticks {
        if i % ROBOT_EVERY_TICKS == 0 {
            for c in robot::commands(&sim) {
                sim.submit(c);
            }
            let now = sim.now_s();
            for t in sim.trains() {
                let here = (t.head().0.idx(), t.head_m);
                if still.get(&t.id.0).is_none_or(|s| (s.0, s.1) != here) {
                    still.insert(t.id.0, (here.0, here.1, now));
                }
            }
        }
        let now = sim.now_s();
        for e in sim.step() {
            match e {
                Event::SignalPassedAtDanger { .. } => d.spads += 1,
                Event::Collision { .. } => d.collisions += 1,
                Event::InvariantViolated { .. } => d.violations += 1,
                Event::WrongPlatform { .. } => d.wrong_platform += 1,
                Event::TrainArrived { train, late_s, .. } | Event::TrainPassed { train, late_s, .. } => {
                    let h = sim.trains().iter().find(|t| t.id == train).map(|t| t.headcode.clone()).unwrap_or_default();
                    d.arrivals.push((late_s, h, now));
                }
                Event::TrainDeparted { train, .. } => {
                    // (A train that formed its next service in the same tick has next_call 0.)
                    if let Some(t) = sim.trains().iter().find(|t| t.id == train && t.next_call > 0) {
                        if let Some(dep) = sim.world().services[t.service.idx()].calls[t.next_call - 1].dep_s {
                            d.departures.push(((now - dep).round() as i64, t.headcode.clone(), now));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let now = sim.now_s();
    let standing = |t: &&signalbox_core::trains::Train| still.get(&t.id.0).is_some_and(|s| now - s.2 >= STUCK_S);
    d.stuck = sim.trains().iter().filter(|t| !t.stabled && t.dwell.is_none()).filter(standing).map(|t| t.headcode.clone()).collect();
    d.running = sim.trains().iter().filter(|t| !t.stabled).map(|t| t.headcode.clone()).collect();
    d.stabled = sim.trains().iter().filter(|t| t.stabled).count();
    d
}

fn worst(v: &[(i64, String, f64)]) -> (i64, String) {
    v.iter().max_by_key(|x| x.0).map(|x| (x.0, format!("{} at {}", x.1, fmt_hms(x.2)))).unwrap_or_default()
}

/// No SPADs, collisions or stuck trains; every train stabled by 01:00; no
/// stop more than 3 minutes late, over five seeds (the dwell times differ).
#[test]
#[ignore]
fn the_real_wtt_runs_a_whole_day() {
    let Some(w) = drain_with_wtt() else {
        eprintln!("no {WTT}: skipped");
        return;
    };
    for seed in [1, 2, 3, 7, 42] {
        let d = run_day(w.clone(), seed);
        let mut hourly: BTreeMap<u32, (i64, usize, usize)> = BTreeMap::new();
        for (late, _, at) in &d.arrivals {
            let e = hourly.entry((*at / 3600.0) as u32).or_default();
            e.0 = e.0.max(*late);
            e.1 += usize::from(*late > 60);
            e.2 += 1;
        }
        eprintln!(
            "seed {seed}: spads {} collisions {} violations {} wrong platform {} stuck {:?} running {:?} stabled {}; {} stops, {} over 1 min late, worst {:?}; {} departures, {} over 1 min late, worst {:?}",
            d.spads,
            d.collisions,
            d.violations,
            d.wrong_platform,
            d.stuck,
            d.running,
            d.stabled,
            d.arrivals.len(),
            d.arrivals.iter().filter(|x| x.0 > 60).count(),
            worst(&d.arrivals),
            d.departures.len(),
            d.departures.iter().filter(|x| x.0 > 60).count(),
            worst(&d.departures),
        );
        eprintln!(
            "  by hour (worst s / stops over 1 min late / stops): {}",
            hourly.iter().map(|(h, (m, l, n))| format!("{h:02}h {m}/{l}/{n}")).collect::<Vec<_>>().join(", ")
        );
        assert_eq!((d.spads, d.collisions, d.violations), (0, 0, 0), "seed {seed}");
        assert!(d.stuck.is_empty() && d.running.is_empty(), "seed {seed}: {:?} {:?}", d.stuck, d.running);
        assert_eq!(d.stabled, 5, "seed {seed}");
        assert!(worst(&d.arrivals).0 <= 180 && worst(&d.departures).0 <= 180, "seed {seed}");
    }
}
```

Run: `scripts/cargo test -p ts2-import --test wtt_day`
Expected: PASS (0 run, 1 ignored). Without the PDF, `-- --ignored` prints `no …wtt.bbox.html: skipped` and passes.

- [ ] **Step 3: The image**

`deploy/Dockerfile`:

```diff
diff --git a/deploy/Dockerfile b/deploy/Dockerfile
index 2b7153c..b33c921 100644
--- a/deploy/Dockerfile
+++ b/deploy/Dockerfile
@@ -1,8 +1,9 @@
 # syntax=docker/dockerfile:1
 # signalbox: the front (signalbox-server), the game process (signalbox-game),
 # the browser client, the three converted TS2 layouts (with their areas, box
-# prefixes and line names) and the tutorial lessons. Release build: no
-# dev login.
+# prefixes and line names; Drain with the real Waterloo & City timetable when
+# external/wtt/ holds the owner's WTT) and the tutorial lessons. Release
+# build: no dev login.
 # Build from the repository root:
 #   docker build -f deploy/Dockerfile -t local/signalbox:$(git rev-parse --short HEAD) .
 
@@ -22,17 +23,34 @@ RUN --mount=type=cache,target=/usr/local/cargo/registry \
     --mount=type=cache,target=/src/target \
     scripts/build-web.sh /out/web
 
+# The owner's copy of the Waterloo & City line's Working Timetable, if
+# external/wtt/ holds one (git-ignored: TfL's document and anything made from
+# it never go into the repository; see external/wtt/README.md). ts2-import
+# --wtt reads its words and their positions. Without it Drain keeps its TS2
+# timetable.
+FROM debian:bookworm-slim AS wtt
+RUN apt-get update \
+ && apt-get install -y --no-install-recommends poppler-utils \
+ && rm -rf /var/lib/apt/lists/*
+COPY external/wtt/ /wtt/
+RUN set -eu; mkdir -p /out; set -- /wtt/*.pdf; \
+    if [ "$#" -gt 1 ]; then echo "external/wtt: more than one PDF" >&2; exit 1; fi; \
+    if [ -f "$1" ]; then sha256sum "$1"; pdftotext -bbox "$1" /out/wtt.bbox.html; \
+    else echo "external/wtt: no WTT; Drain keeps its TS2 timetable"; fi
+
 FROM rust:1.98-slim-bookworm AS build
 WORKDIR /src
 COPY . .
+COPY --from=wtt /out/ /wtt/
 RUN --mount=type=cache,target=/usr/local/cargo/registry \
     --mount=type=cache,target=/src/target \
     cargo build --release --locked -p signalbox-server -p ts2-import --bins \
  && mkdir -p /out/bin /out/layouts \
  && cp target/release/signalbox-server target/release/signalbox-game /out/bin/ \
  && for n in liverpool-st drain gretz-armainvilliers; do \
+      wtt=""; if [ "$n" = drain ] && [ -f /wtt/wtt.bbox.html ]; then wtt="--wtt /wtt/wtt.bbox.html"; fi; \
       target/release/ts2-import "crates/ts2-import/tests/data/$n.json" -o "/out/layouts/$n.json" \
-        --areas "layouts/$n.areas.json" --lines "layouts/$n.lines.json" || exit 1; \
+        --areas "layouts/$n.areas.json" --lines "layouts/$n.lines.json" $wtt || exit 1; \
     done \
  && cp -r lessons /out/lessons \
  && chmod -R a+rX /out/lessons
```

Check the stage alone (no PDF in the checkout):
`docker build --progress=plain -f deploy/Dockerfile --target wtt -t local/sbx-wtt-probe:check . 2>&1 | grep external/wtt`
Expected: `external/wtt: no WTT; Drain keeps its TS2 timetable`. Then `docker rmi local/sbx-wtt-probe:check`.
(Scratch, `0c0ea67`: with the owner's PDF the stage wrote the same `wtt.bbox.html` as poppler 25.03 on ra, byte
for byte; with two PDFs it stopped with `external/wtt: more than one PDF`; the build stage's loop, run by hand with
and without the text, wrote Drain with 574 and 16 services.)

- [ ] **Step 4: Document**

`deploy/README.md`:
- the `Dockerfile` row ends "… line names from `layouts/`, and Drain with the real Waterloo & City timetable when
  `external/wtt/` holds the owner's WTT (its `wtt` stage runs `pdftotext -bbox`; without one, Drain's TS2
  timetable) (no dev login)"; the `Dockerfile.dockerignore` row adds "(`external/wtt/` stays in, for that stage)".
- a section after "Build and run":

```markdown
## The Waterloo & City timetable (optional)

Before `docker build`, copy the owner's WTT PDF into `external/wtt/` of the checkout being built (the only PDF
there; `external/wtt/README.md`). The build log shows its sha256 and the converter's summary (`Wednesday, 585
trips of 7 trains; …`, `574 services, 5 entries from 05:40:00; …`); a WTT that fails the checks fails the build.
Without a PDF the log says `Drain keeps its TS2 timetable`. An image built with it holds a timetable made from
TfL's document: it stays on ra and is never pushed to a public registry. Old Drain saves keep the timetable they
were created with.
```

`CLAUDE.md` Commands block, after the `--wtt` line from Task 4b:
`scripts/cargo test --release -p ts2-import --test wtt_day -- --ignored --nocapture   # the real WTT, a whole day under the robot (skips without it)`

- [ ] **Step 5: Commit**

```bash
git add external/wtt/README.md external/wtt/.gitignore crates/ts2-import/tests/wtt_day.rs deploy/Dockerfile deploy/README.md CLAUDE.md
git status --short   # nothing from external/wtt/ but those two files
git commit -m "feat(deploy): the image converts Drain with the owner's WTT when external/wtt/ has it"
```

- [ ] **Step 6 (controller, with the owner's PDF; not for subagents)**

Copy the PDF into `external/wtt/` of the worktree, `pdftotext -bbox external/wtt/*.pdf external/wtt/wtt.bbox.html`
(poppler-utils on ra), then `scripts/cargo test --release -p ts2-import --test wtt_day -- --ignored --nocapture`.
Expected (scratch, `0c0ea67`): for seeds 1, 2, 3, 7, 42: `spads 0 collisions 0 violations 0 wrong platform 0 stuck []
running [] stabled 5; 1721 stops, 137 over 1 min late, worst (103, "204/73 at 19:45:42")`, departures worst 109 s.
Copy the two lines of one seed into the branch report. `git status` must still show nothing from `external/wtt/`
but the README and `.gitignore`.

---

### Task 5: The simplifier opens at "now"

**Files:**
- Modify: `crates/client-core/src/simplifier.rs`, `crates/client-ui/src/screens.rs`
- Test: `crates/client-core/tests/simplifier.rs`, `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: Drain's committed TS2 timetable (`crates/ts2-import/tests/data/drain.json`, 06:00–06:43) in the screen test; the WTT is not in CI.
- Produces: `client_core::simplifier::{last_time(&SimplifierRow) -> Option<f64>, now_line(rows: &[&SimplifierRow], now_s: f64) -> usize}`; `UiApp` field `simplifier_scroll: Option<usize>`.

- [ ] **Step 1: Write the failing tests**

In `crates/client-core/tests/simplifier.rs`, add `now_line` to the `use client_core::simplifier::{…}` list and append:

```rust
/// Polish spec §7: the simplifier opens at the first train not yet finished.
#[test]
fn the_simplifier_opens_at_the_first_train_not_yet_finished() {
    let row = |h: &str, times: &[f64]| SimplifierRow {
        headcode: s(h),
        origin: None,
        destination: None,
        calls: times.iter().map(|&t| call("X", None, Some(t), Some(t), true)).collect(),
    };
    let (a, b, c, u) = (row("1A01", &[100.0, 200.0]), row("1A02", &[150.0]), row("1A03", &[300.0, 400.0, 500.0]), row("1A04", &[]));
    let rows = vec![&a, &b, &c, &u];
    assert_eq!(now_line(&rows, 0.0), 0);
    assert_eq!(now_line(&rows, 160.0), 0, "1A01 is still running");
    assert_eq!(now_line(&rows, 250.0), 3, "after 1A01's two lines and 1A02's one");
    assert_eq!(now_line(&rows, 600.0), 6, "a row with no times never finishes");
}
```

Append to `crates/client-ui/tests/screens.rs`:

```rust
/// Polish spec §7: half an hour into Drain's TS2 timetable the simplifier
/// opens at the trains still running, not at 06:00's.
#[test]
fn the_simplifier_opens_at_now() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let w = ts2_import::convert(&std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/drain.json")).unwrap()).unwrap().world;
    let mut r = Rig::in_game(signalbox_core::world::World::from_file(w).unwrap(), None);
    r.game.handle("ann", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
    for _ in 0..225 {
        r.game.advance(1.0); // 06:00 to 06:30 at 8x
    }
    for (p, m) in r.game.resync("ann") {
        if p == "ann" {
            r.h.push(ServerFrame::Game(m));
        }
    }
    r.frame();
    let out = r.frame();
    click_text(&mut r, &out, "SIMPLIFIER");
    r.frame();
    let out = r.frame();
    let side = side_texts(&r, &out);
    assert!(!side.iter().any(|t| t == "BW01"), "BW01 ran at 06:00: {side:?}");
    assert!(side.iter().any(|t| t == "BW06") && side.iter().any(|t| t == "BW07"), "06:30's trains: {side:?}");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-core --test simplifier` → compile error (`now_line` not found).
Run: `scripts/cargo test -p signalbox-client-ui --test screens the_simplifier_opens_at_now` → FAIL: `BW01 ran at 06:00: [… "BW01" …]` (checked on `0c0ea67` with only the scroll line left out).

- [ ] **Step 3: Implement**

`crates/client-core/src/simplifier.rs`, before `/// Rows whose headcode contains \`search\``:

```rust
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
```

`crates/client-ui/src/screens.rs`:
- `UiApp` gains, after `placement` (Task 3; on `0c0ea67` without Task 3: after `simplifier_lines`):

```rust
    /// The simplifier line to scroll to once, set when its lines are built.
    simplifier_scroll: Option<usize>,
```

  and `simplifier_scroll: None,` in `UiApp::new`.
- in `side`, the SIMPLIFIER tab (on `0c0ea67` the tab is `let simplifier = ui.selectable_value(…)` followed by its
  `mark(…)` line for lesson highlights); after that `mark` line add:

```rust
            if simplifier.clicked() {
                // Opened again: build the lines afresh and scroll to now.
                self.simplifier_lines = None;
            }
```

- in `simplifier_ui`, the rebuild becomes:

```rust
        if self.simplifier_lines.as_ref().map(|(k, _)| k) != Some(&key) {
            let rows = simplifier::rows(l, &self.search);
            self.simplifier_scroll = g.view().map(|v| simplifier::now_line(&rows, v.sim_time));
            let lines = rows
                .into_iter()
                .flat_map(|r| simplifier::lines(r).into_iter().enumerate().map(|(i, line)| (line, i == 0)))
                .collect();
            self.simplifier_lines = Some((key, lines));
        }
```

- and the scroll area:

```rust
        let mut area = egui::ScrollArea::vertical().id_salt("simplifier").max_height(height);
        if let Some(line) = self.simplifier_scroll.take() {
            area = area.vertical_scroll_offset(line as f32 * (row_h + ui.spacing().item_spacing.y));
        }
        area.show_rows(ui, row_h, lines.len(), |ui, range| {
```

  (the closure body and its closing `});` are unchanged).

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-core --test simplifier` and `scripts/cargo test -p signalbox-client-ui --test screens`
Expected: PASS (the existing `the_simplifier_tab_lists_searches_and_shows_lateness` still sees 1E01 at 07:00: it has not finished).

- [ ] **Step 5: Commit**

```bash
git add crates/client-core/src/simplifier.rs crates/client-core/tests/simplifier.rs crates/client-ui/src/screens.rs crates/client-ui/tests/screens.rs
git commit -m "feat(client): the simplifier opens at the first train not yet finished"
```

---

### Task 6: Resume with today's display data

**Files:**
- Modify: `crates/game/src/save.rs`, `crates/game/src/game.rs`, `crates/game/src/lib.rs`, `CLAUDE.md`
- Test: `crates/game/tests/refresh.rs` (new)

**Interfaces:**
- Produces: `game::save::{NETWORK_KEYS: [&str; 8], refresh_display(saved: &str, current: &str) -> Result<String, String>}`; `game::Refresh { NotAsked, Refreshed, Kept(String) }` (re-exported from `game`); `Game::resume_with_layout(path: &Path, current: Option<&str>) -> Result<(Game, Refresh), GameError>`; `Game::resume(path)` unchanged in signature (passes `None`). Task 7 uses `resume_with_layout` and `Refresh`.

- [ ] **Step 1: Write the failing tests**

Create `crates/game/tests/refresh.rs`:

```rust
//! Old saves get today's display data (polish spec §5): the layout file's
//! drawing and names when its network matches the save's, and the same
//! simulation either way.

mod common;

use std::path::PathBuf;

use common::*;
use game::save::refresh_display;
use game::{Game, GameMeta, Refresh};
use protocol::{ClientMsg, Proposal};
use serde_json::Value;

/// Drain as the image converts it now: box `W`, line names.
fn drain_now() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let read = |p: String| std::fs::read_to_string(p).unwrap();
    let mut w = ts2_import::convert(&read(format!("{dir}/../ts2-import/tests/data/drain.json"))).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&read(format!("{dir}/../../layouts/drain.areas.json"))).unwrap()).unwrap();
    ts2_import::lines::apply(&mut w, &ts2_import::lines::parse(&read(format!("{dir}/../../layouts/drain.lines.json"))).unwrap()).unwrap();
    serde_json::to_string(&w).unwrap()
}

/// Drain as saves from before the realism pass hold it: no box prefix,
/// workstation letters or line names, so the prefix falls back to `L`.
fn drain_before() -> String {
    let mut v: Value = serde_json::from_str(&drain_now()).unwrap();
    let l = v["layout"].as_object_mut().unwrap();
    l.remove("box_prefix");
    l.remove("workstations");
    l.get_mut("labels").unwrap().as_array_mut().unwrap().retain(|x| x.get("arrow").is_none());
    v.to_string()
}

fn temp_save(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("signalbox-refresh-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.sqlite"));
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
    path
}

/// A saved Drain game from before the realism pass, a minute in.
fn old_save(name: &str) -> PathBuf {
    let path = temp_save(name);
    let mut g = Game::create(&path, &drain_before(), GameMeta { layout: "drain".into(), seed: 3 }).unwrap();
    run(&mut g, 60.0, 0.1);
    assert!(g.save_now().is_empty());
    path
}

fn prefix(g: &mut Game) -> String {
    g.connect("ann");
    g.layout_of("ann").unwrap().box_prefix
}

#[test]
fn the_drawing_and_names_come_from_the_layout_when_the_network_matches() {
    let mut before: Value = serde_json::from_str(&drain_before()).unwrap();
    before["options"]["start_time"] = "07:30:00".into();
    before["services"].as_array_mut().unwrap().truncate(1);
    let now: Value = serde_json::from_str(&drain_now()).unwrap();
    let out: Value = serde_json::from_str(&refresh_display(&before.to_string(), &now.to_string()).unwrap()).unwrap();
    assert_eq!(out["layout"], now["layout"], "today's drawing and names");
    assert_eq!(out["options"]["start_time"], "07:30:00", "the save's own start");
    assert_eq!(out["services"].as_array().unwrap().len(), 1, "the save's own timetable");
}

#[test]
fn a_different_network_or_a_bad_file_keeps_the_saves_own() {
    let before = drain_before();
    let mut now: Value = serde_json::from_str(&drain_now()).unwrap();
    now["routes"].as_array_mut().unwrap().pop();
    assert!(refresh_display(&before, &now.to_string()).unwrap_err().contains("`routes`"));
    let mut now: Value = serde_json::from_str(&drain_now()).unwrap();
    now["areas"][0]["name"] = "City".into();
    assert!(refresh_display(&before, &now.to_string()).unwrap_err().contains("`areas`"));
    assert!(refresh_display(&before, "{not json").unwrap_err().contains("unreadable"));
    let mut now: Value = serde_json::from_str(&drain_now()).unwrap();
    now.as_object_mut().unwrap().remove("layout");
    assert!(refresh_display(&before, &now.to_string()).unwrap_err().contains("no drawing"));
}

#[test]
fn an_old_drain_save_resumes_with_todays_names_and_the_same_sim() {
    let path = old_save("drain");
    let (mut old, r) = Game::resume_with_layout(&path, None).unwrap();
    assert_eq!((r, prefix(&mut old)), (Refresh::NotAsked, "L".to_string()), "the bug: the title's first letter");
    let (mut new, r) = Game::resume_with_layout(&path, Some(&drain_now())).unwrap();
    assert_eq!((r, prefix(&mut new)), (Refresh::Refreshed, "W".to_string()));
    // Display data never reaches the sim: both run on identically.
    for g in [&mut old, &mut new] {
        send(g, "ann", ClientMsg::Vote { proposal: Proposal::Resume });
        assert!(!g.clock().paused);
        run(g, 120.0, 0.1);
    }
    assert_eq!(old.sim().tick(), new.sim().tick());
    assert_eq!(old.sim().state_hash(), new.sim().state_hash());
    // Nothing was written back: a plain resume still shows the old prefix.
    let mut again = Game::resume(&path).unwrap();
    assert_eq!(prefix(&mut again), "L");
}

#[test]
fn a_layout_that_does_not_match_is_reported_and_ignored() {
    let path = old_save("mismatch");
    let (mut g, r) = Game::resume_with_layout(&path, Some(&liverpool_json())).unwrap();
    let Refresh::Kept(why) = r else { panic!("{r:?}") };
    assert!(why.contains("differ"), "{why}");
    assert_eq!(prefix(&mut g), "L");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-game --test refresh`
Expected: compile errors (`refresh_display`, `Refresh`, `resume_with_layout` not found).

- [ ] **Step 3: Implement**

`crates/game/src/save.rs`, before `/// Rebuild a saved game's sim (spec §7.3)`:

```rust
/// The world keys a layout's drawing and names describe (polish spec §5.2).
pub const NETWORK_KEYS: [&str; 8] = ["areas", "sections", "nodes", "segments", "signals", "berths", "platforms", "routes"];

/// The saved world with its display data (`layout`) taken from `current`,
/// the layout file the game was made from as it is now, when the two have
/// exactly the same network (polish spec §5.2); otherwise why not. The sim
/// never reads `layout`, so this cannot change a replay; services, entries
/// and options stay the save's.
pub fn refresh_display(saved: &str, current: &str) -> Result<String, String> {
    let mut s: serde_json::Value = serde_json::from_str(saved).map_err(|e| format!("the saved world is unreadable: {e}"))?;
    let c: serde_json::Value = serde_json::from_str(current).map_err(|e| format!("the layout file is unreadable: {e}"))?;
    if let Some(k) = NETWORK_KEYS.iter().find(|k| s.get(**k) != c.get(**k)) {
        return Err(format!("the layout's `{k}` differ from the save's"));
    }
    let layout = c.get("layout").filter(|l| l.is_object()).cloned().ok_or("the layout file has no drawing")?;
    s.as_object_mut().ok_or("the saved world is not an object")?.insert("layout".into(), layout);
    Ok(serde_json::to_string(&s).expect("JSON values serialise"))
}
```

`crates/game/src/game.rs`:
- the save import: `use crate::save::{Logged, SaveDb, SaveError, refresh_display, resume_sim};`
- after `pub type Out = (String, ServerMsg);`:

```rust
/// Where a resumed game's display data came from (polish spec §5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refresh {
    /// No current layout was given: the save's own.
    NotAsked,
    /// The current layout file's (same network as the save).
    Refreshed,
    /// The save's own, and why the current layout's could not be used.
    Kept(String),
}
```

- `resume` becomes a wrapper and its body moves into `resume_with_layout`; only the world-loading line and the return value change (the rest is the old body, verbatim):

```rust
    /// Resume the game saved at `path`: paused at 1x, every area unclaimed.
    pub fn resume(path: &Path) -> Result<Game, GameError> {
        Game::resume_with_layout(path, None).map(|(g, _)| g)
    }

    /// `resume`, taking the display data from `current` (the layout file's
    /// world JSON as it is now) when its network matches the save's.
    pub fn resume_with_layout(path: &Path, current: Option<&str>) -> Result<(Game, Refresh), GameError> {
        let db = SaveDb::open(path)?;
        let saved = db.load()?;
        let (world, refresh) = match current.map(|c| refresh_display(&saved.world_json, c)) {
            None => (World::from_json(&saved.world_json)?, Refresh::NotAsked),
            Some(Err(why)) => (World::from_json(&saved.world_json)?, Refresh::Kept(why)),
            Some(Ok(json)) => match World::from_json(&json) {
                Ok(w) => (w, Refresh::Refreshed),
                Err(e) => (World::from_json(&saved.world_json)?, Refresh::Kept(format!("the refreshed world does not load: {e}"))),
            },
        };
        let snapshot_tick = saved.snapshot.tick;
        let (sim, robot_ran) =
            resume_sim(world, saved.snapshot, saved.last_seq, &saved.commands).map_err(GameError::Resume)?;
        let mut g = Game::from_sim(sim, saved.meta, true);
        if robot_ran {
            g.robot_ran_at = Some(g.sim.tick());
        }
        // The sim's queue holds the commands the snapshot held at this tick
        // (logged at or under `last_seq`) followed by those replayed here;
        // name their senders so a sim rejection still goes back to them. If
        // the snapshot's part disagrees with the log (a failed append), that
        // part is left unattributed.
        let queue = g.sim.snapshot().queue;
        let at_end = |c: &&Logged| c.tick == g.sim.tick();
        let held: Vec<&Logged> = saved.commands.iter().filter(at_end).filter(|c| c.seq <= saved.last_seq).collect();
        let replayed: Vec<&Logged> = saved.commands.iter().filter(at_end).filter(|c| c.seq > saved.last_seq).collect();
        let n_held = queue.len().saturating_sub(replayed.len());
        let mut senders: Vec<String> = if held.iter().map(|l| &l.command).eq(queue[..n_held].iter()) {
            held.iter().map(|l| l.player.clone()).collect()
        } else {
            vec![ROBOT.to_string(); n_held]
        };
        senders.extend(replayed.iter().map(|l| l.player.clone()));
        senders.resize(queue.len(), ROBOT.to_string());
        g.queued = senders.into_iter().zip(queue).collect();
        g.save = Some(db);
        g.last_snapshot = Some(snapshot_tick);
        Ok((g, refresh))
    }
```

`crates/game/src/lib.rs`: add `Refresh` to the `pub use game::{…}` list (after `ROBOT`).

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-game`
Expected: PASS (refresh 4; save, status and every other file unchanged).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Multiplayer", after the Resume bullet:

```markdown
- `Game::resume_with_layout(path, Some(current))` (polish spec §5) takes the
  world's display-only `layout` from the layout file as it is now when the
  saved network (`game::save::NETWORK_KEYS`) is identical, so old saves get
  today's prefixes, line names and drawing; in memory only, never written
  back, and the sim never reads it. Services, entries and options stay the save's.
```

- [ ] **Step 6: Commit**

```bash
git add crates/game CLAUDE.md
git commit -m "feat(game): resume with the layout file's display data when the saved network matches"
```

---

### Task 7: The front passes the current layout on resume

**Files:**
- Modify: `crates/server/src/process.rs`, `crates/server/src/supervisor.rs`, `CLAUDE.md`
- Test: `crates/server/tests/process.rs`, `crates/server/tests/supervisor.rs`

**Interfaces:**
- Consumes: Task 6's `Game::resume_with_layout`, `Refresh`.
- Produces: `process::Args.current_layout: Option<PathBuf>`; `signalbox-game --current-layout <world.json>` (refused with `--create`); `Start::Resume { current: Option<PathBuf> }` in the supervisor.

- [ ] **Step 1: Write the failing tests**

In `crates/server/tests/process.rs`, before `fn start_times_are_normalised_and_bounded`:

```rust
/// Polish spec §5.3: a resume may name the layout file as it is now.
#[test]
fn resuming_may_name_the_current_layout() {
    let a = Args::parse(&args(&["--save", "g.sqlite", "--socket", "g.sock", "--current-layout", "/l/drain.json"])).unwrap();
    assert_eq!((a.create, a.current_layout), (None, Some(PathBuf::from("/l/drain.json"))));
    let err = Args::parse(&args(&[
        "--save", "x", "--socket", "y", "--create", "--layout", "w.json", "--layout-name", "drain", "--seed", "1",
        "--current-layout", "w.json",
    ]))
    .unwrap_err();
    assert_eq!(err, "--current-layout is for resuming, not with --create");
}
```

In `crates/server/tests/supervisor.rs`, before `async fn lobby_rejects_bad_layouts_starts_and_ids` (its `#[tokio::test]` line):

```rust
/// Polish spec §5: a resumed save takes the listed layout's display data
/// when the network matches (here a stale box prefix).
#[tokio::test]
async fn a_resumed_save_shows_the_layouts_current_names() {
    let rig = rig("refresh", 600);
    let with_prefix = |p: &str| {
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(TWOBOX).unwrap()).unwrap();
        v["layout"] = serde_json::json!({ "box_prefix": p });
        v.to_string()
    };
    std::fs::write(rig.root.join("layouts/twobox.json"), with_prefix("T")).unwrap();
    let id = "g-cccccccccccc";
    drop(Game::create(&rig.saves().join(format!("{id}.sqlite")), &with_prefix("Q"), GameMeta { layout: s("twobox"), seed: 2 }).unwrap());
    let ann = rig.attach("ann");
    rig.lobby(&ann, LobbyMsg::Join { game: s(id) });
    let got = until(&ann, |f| matches!(f, ServerFrame::Game(ServerMsg::Layout(_)))).await;
    let Some(ServerFrame::Game(ServerMsg::Layout(l))) = got.last() else { unreachable!() };
    assert_eq!(l.box_prefix, "T", "today's prefix, not the save's Q");
}

/// Review focus 2: a save whose layout the front no longer lists resumes
/// with its own display data.
#[tokio::test]
async fn a_save_of_a_layout_no_longer_listed_still_resumes() {
    let rig = rig("unlisted", 600);
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(TWOBOX).unwrap()).unwrap();
    v["layout"] = serde_json::json!({ "box_prefix": "Q" });
    let id = "g-dddddddddddd";
    drop(Game::create(&rig.saves().join(format!("{id}.sqlite")), &v.to_string(), GameMeta { layout: s("gone"), seed: 2 }).unwrap());
    let ann = rig.attach("ann");
    rig.lobby(&ann, LobbyMsg::Join { game: s(id) });
    let got = until(&ann, |f| matches!(f, ServerFrame::Game(ServerMsg::Layout(_)))).await;
    let Some(ServerFrame::Game(ServerMsg::Layout(l))) = got.last() else { unreachable!() };
    assert_eq!(l.box_prefix, "Q");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-server --test process --test supervisor`
Expected: compile error (no field `current_layout` on `Args`). Without the supervisor change, `a_resumed_save_shows_the_layouts_current_names` would fail with `left: "Q"`.

- [ ] **Step 3: Implement**

`crates/server/src/process.rs`:
- `use game::{Game, GameMeta, GameStatus, Out, Refresh};`
- `USAGE`'s create group ends `… [--creator <user>] | --current-layout <world.json>]";`
- `Args` gains after `create`:

```rust
    /// Resuming: the layout file as it is now, for its display data (polish spec §5).
    pub current_layout: Option<PathBuf>,
```

- in `parse`: `let mut current_layout = None;` after the second `let (…)` line; the arm `"--current-layout" => current_layout = Some(PathBuf::from(value()?)),` before `other =>`; before `let create = if create {`:

```rust
        if create && current_layout.is_some() {
            return Err("--current-layout is for resuming, not with --create".into());
        }
```

  and the result `Ok(Args { save, socket, empty_exit: Duration::from_secs(empty_exit_s), create, current_layout })`.
- in `open_game`, the resume arm:

```rust
        None => {
            // An unreadable layout file only costs the refresh, never the game.
            let current = args.current_layout.as_ref().and_then(|p| match std::fs::read_to_string(p) {
                Ok(t) => Some(t),
                Err(e) => {
                    eprintln!("kept the saved display data: {}: {e}", p.display());
                    None
                }
            });
            let (g, refresh) = Game::resume_with_layout(&args.save, current.as_deref()).map_err(|e| e.to_string())?;
            match refresh {
                Refresh::Refreshed => eprintln!("display data from layout {}", g.meta().layout),
                Refresh::Kept(why) => eprintln!("kept the saved display data: {why}"),
                Refresh::NotAsked => {}
            }
            Ok(g)
        }
```

`crates/server/src/supervisor.rs`:
- `enum Start`'s `Resume` becomes

```rust
    /// `current`: the layout file the save was made from, if still listed.
    Resume { current: Option<PathBuf> },
```

- in `join`, just before `let rx = Self::insert_starting(&mut st, &game, layout);` add `let current = self.layouts.path(&layout);`, and spawn with `Start::Resume { current }`;
- in `run_game`, after the `if let Start::Create { … } = &start { … }` block:

```rust
        if let Start::Resume { current: Some(p) } = &start {
            cmd.arg("--current-layout").arg(p);
        }
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-server` and `scripts/cargo test -p signalbox-server --features dev-auth`
Expected: PASS (process 17, supervisor 31, the rest unchanged).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Server", first bullet, after "…the crash reason the lobby shows).": "On a resume the front passes `--current-layout <world.json>` when it still lists the save's layout; the process logs whether it took that file's display data (Task 6's rule)."

- [ ] **Step 6: Commit**

```bash
git add crates/server CLAUDE.md
git commit -m "feat(server): a resumed game gets the listed layout file for today's display data"
```

---

### Task 8: The real-browser renderer check

**Files:**
- Create: `deploy/browser-check.sh` (executable), `deploy/browser-check.py`
- Modify: `deploy/README.md`, `CLAUDE.md`

**Interfaces:**
- Consumes: `scripts/wasm-build`, the dev-auth front, Drain's committed TS2 timetable (the check converts Drain with its areas and lines only, never the WTT: three trains stand in the platforms at 06:00), the client's console line `signalbox: drawing with <backend>` (already in `crates/client-web/src/lib.rs`).
- Produces: `deploy/browser-check.sh [--no-build]` → one `ok`/`FAIL` line per case (`webgl2`, `webgpu`, `none`), exit 0/1 (2 for missing build outputs with `--no-build`); outputs in `target/browser-check/`.

- [ ] **Step 1: Write the check**

Create `deploy/browser-check.py`:

```python
"""Real-browser check of signalbox's renderers (polish spec section 6).

usage: python3 browser-check.py BASE OUTDIR

Runs headless Chromium three ways against a dev-login front at BASE and
prints one ok/FAIL line per case; exits 1 if any case fails. Screenshots and
each case's browser console go to OUTDIR.

  webgl2  no flags: Chromium has the WebGPU API but no adapter, so the client
          must fall back to WebGL2 (asserted first, so the case cannot pass
          vacuously); the lobby and a Drain game must be drawn.
  webgpu  --enable-unsafe-webgpu: a software WebGPU adapter; the client must
          draw with it (headless WebGPU canvases screenshot blank, so no
          pixels are checked).
  none    --disable-webgl: neither renderer; the page must explain why.
"""

import json
import struct
import sys
import zlib

from playwright.sync_api import sync_playwright

BASE, OUT = sys.argv[1], sys.argv[2]
TRACK_GREY = (0x7D, 0x7D, 0x7D)
HEADCODE_CYAN = (0x39, 0xE0, 0xFF)
NO_ADAPTER = "async () => !(navigator.gpu && await navigator.gpu.requestAdapter())"


def pixels(png):
    """(width, height, rows of (r, g, b)) of an 8-bit, non-interlaced RGB or
    RGBA PNG, as Chromium's screenshots are."""
    assert png[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, data, width, height, channels = 8, b"", 0, 0, 0
    while pos < len(png):
        (length,) = struct.unpack(">I", png[pos : pos + 4])
        kind, body = png[pos + 4 : pos + 8], png[pos + 8 : pos + 8 + length]
        if kind == b"IHDR":
            width, height, depth, colour, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and colour in (2, 6) and interlace == 0, "unexpected PNG format"
            channels = 3 if colour == 2 else 4
        elif kind == b"IDAT":
            data += body
        pos += 12 + length
    raw, stride, rows, prev = zlib.decompress(data), width * channels, [], bytearray(width * channels)
    for y in range(height):
        start = y * (stride + 1)
        kind, line = raw[start], bytearray(raw[start + 1 : start + 1 + stride])
        for i in range(stride):
            a = line[i - channels] if i >= channels else 0
            b, c = prev[i], prev[i - channels] if i >= channels else 0
            if kind == 1:
                line[i] = (line[i] + a) & 0xFF
            elif kind == 2:
                line[i] = (line[i] + b) & 0xFF
            elif kind == 3:
                line[i] = (line[i] + (a + b) // 2) & 0xFF
            elif kind == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 0xFF
        rows.append([tuple(line[x * channels : x * channels + 3]) for x in range(width)])
        prev = line
    return width, height, rows


def count(rows, colour, tolerance=2):
    return sum(1 for row in rows for p in row if all(abs(p[k] - colour[k]) <= tolerance for k in range(3)))


def not_blank(rows):
    """Share of pixels that differ from the most common colour."""
    seen = {}
    for row in rows:
        for p in row:
            seen[p] = seen.get(p, 0) + 1
    total = sum(seen.values())
    return 1.0 - max(seen.values()) / total


def run_case(p, name, flags):
    """Returns a list of failure reasons (empty: ok) and writes OUT/name-*.png
    and OUT/name-console.txt."""
    fails, logs, sockets = [], [], []
    browser = p.chromium.launch(args=flags)
    page = browser.new_page(viewport={"width": 1280, "height": 800})
    page.on("console", lambda m: logs.append(f"{m.type}: {m.text}"))
    page.on("pageerror", lambda e: logs.append(f"pageerror: {e}"))
    # Every frame passes through untouched; the page's socket is kept so the
    # check can send a lobby message as the client would.
    page.route_web_socket("**/ws", lambda ws: sockets.append(ws.connect_to_server()))
    page.goto(f"{BASE}/auth/dev?user=check")
    no_adapter = page.evaluate(NO_ADAPTER)
    page.wait_for_timeout(4000)
    page.screenshot(path=f"{OUT}/{name}-lobby.png")
    if name == "webgl2":
        if not no_adapter:
            fails.append("this Chromium has a WebGPU adapter: the WebGL2 fallback was not exercised")
        if not any("signalbox: drawing with Gl" in line for line in logs):
            fails.append("the client did not say it draws with Gl")
        _, _, rows = pixels(page.screenshot())
        if not_blank(rows) < 0.01:
            fails.append("the lobby is blank")
        if not sockets:
            fails.append("the client opened no socket")
        else:
            sockets[-1].send(json.dumps({"type": "create_game", "layout": "drain", "seed": 1}))
            page.wait_for_timeout(6000)
            shot = page.screenshot(path=f"{OUT}/{name}-game.png")
            _, _, rows = pixels(shot)
            grey, cyan = count(rows, TRACK_GREY), count(rows, HEADCODE_CYAN)
            if grey < 2000:
                fails.append(f"only {grey} track-grey pixels in the game")
            if cyan < 20:
                fails.append(f"only {cyan} headcode-cyan pixels in the game")
    elif name == "webgpu":
        if no_adapter:
            fails.append("no WebGPU adapter even with --enable-unsafe-webgpu")
        if not any("signalbox: drawing with BrowserWebGpu" in line for line in logs):
            fails.append("the client did not say it draws with BrowserWebGpu")
    else:
        shown = page.evaluate(
            "() => { const f = document.getElementById('fallback'), g = document.getElementById('fallback_gpu');"
            " return [!!f && !f.hidden, !!g && !g.hidden, document.body.innerText]; }"
        )
        if not (shown[0] and shown[1] and "could not start" in shown[2]):
            fails.append(f"no renderer explanation shown: {shown[2][:200]!r}")
    for line in logs:
        if "panicked" in line or line.startswith("pageerror"):
            fails.append(f"console: {line[:300]}")
    with open(f"{OUT}/{name}-console.txt", "w") as f:
        f.write("\n".join(logs) + "\n")
    browser.close()
    return fails


CASES = [
    ("webgl2", ["--enable-unsafe-swiftshader"]),
    ("webgpu", ["--enable-unsafe-webgpu"]),
    ("none", ["--disable-webgl"]),
]

failed = False
with sync_playwright() as p:
    for name, flags in CASES:
        fails = run_case(p, name, flags)
        print(f"{'ok  ' if not fails else 'FAIL'} {name} {' '.join(flags)}")
        for why in fails:
            print(f"     {why}")
        failed |= bool(fails)
sys.exit(1 if failed else 0)
```

Create `deploy/browser-check.sh` and `chmod +x deploy/browser-check.sh`:

```bash
#!/usr/bin/env bash
# Real-browser check of the web client's renderers (polish spec section 6):
# builds the client and a dev-login front from this checkout, runs the front
# on 127.0.0.1 in the stock Rust image, then headless Chromium from the
# Playwright image three ways (WebGL2 fallback, WebGPU, neither; see
# browser-check.py). Prints one ok/FAIL line per case and exits 1 on any
# failure; screenshots and consoles land in target/browser-check/.
# Needs Docker and the Playwright image (pip fetches the matching playwright).
# usage: deploy/browser-check.sh [--no-build]
#   --no-build: use the existing target/web-dist and target/debug binaries
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
port="${SIGNALBOX_CHECK_PORT:-19162}"
image="${SIGNALBOX_PLAYWRIGHT_IMAGE:-mcr.microsoft.com/playwright/python:v1.55.0-noble}"
out="$root/target/browser-check"
name="sbx-browser-check-$$"
if [[ "${1:-}" != "--no-build" ]]; then
  scripts/wasm-build
  scripts/cargo build -q -p signalbox-server --features dev-auth --bins
  scripts/cargo build -q -p ts2-import --bins
fi
for f in target/web-dist/index.html target/debug/signalbox-server target/debug/signalbox-game target/debug/ts2-import; do
  [[ -e $f ]] || { echo "browser-check: $f is missing (run without --no-build)" >&2; exit 2; }
done
rm -rf "$out"
mkdir -p "$out/layouts" "$out/data"
scripts/cargo run -q -p ts2-import -- crates/ts2-import/tests/data/drain.json -o /w/target/browser-check/layouts/drain.json \
  --areas /w/layouts/drain.areas.json --lines /w/layouts/drain.lines.json 2>/dev/null
cleanup() {
  docker stop "$name" >/dev/null 2>&1 || true
  # The browser's files are root-owned: give them back.
  docker run --rm -v "$out:/out" alpine chown -R "$(id -u):$(id -g)" /out >/dev/null 2>&1 || true
}
trap cleanup EXIT
docker run --rm -d --name "$name" --network host -u "$(id -u):$(id -g)" -v "$root:/w" -w /w \
  -e SIGNALBOX_ADDR="127.0.0.1:$port" -e SIGNALBOX_DATA=/w/target/browser-check/data \
  -e SIGNALBOX_LAYOUTS=/w/target/browser-check/layouts -e SIGNALBOX_WEB=/w/target/web-dist \
  -e SIGNALBOX_GAME_BIN=/w/target/debug/signalbox-game -e SIGNALBOX_SESSION_KEY="$(openssl rand -hex 64)" \
  rust:1.98-slim-bookworm target/debug/signalbox-server >/dev/null
for _ in $(seq 100); do
  curl -s -o /dev/null "http://127.0.0.1:$port/auth/logout" && break
  sleep 0.1
done
docker run --rm --network host -v "$root/deploy/browser-check.py:/check.py:ro" -v "$out:/out" "$image" \
  sh -c "pip install -q playwright==1.55.0 >/dev/null 2>&1 && python3 /check.py http://127.0.0.1:$port /out"
```

- [ ] **Step 2: Run it**

Run: `deploy/browser-check.sh` (first run builds the client and the dev-auth front: several minutes)
Expected output ends:

```
ok   webgl2 --enable-unsafe-swiftshader
ok   webgpu --enable-unsafe-webgpu
ok   none --disable-webgl
```

with exit code 0, and `target/browser-check/webgl2-game.png` showing the Drain spectator view (three cyan headcodes in the platforms). Look at that screenshot and at `webgl2-lobby.png`, `none-lobby.png` (the explanation page).

- [ ] **Step 3: Prove it can fail**

Run: `SIGNALBOX_CHECK_PORT=19163 deploy/browser-check.sh --no-build` after temporarily changing the webgl2 case's flags in `browser-check.py` to `["--enable-unsafe-webgpu"]`.
Expected: `FAIL webgl2 …` with `this Chromium has a WebGPU adapter: the WebGL2 fallback was not exercised` and exit code 1. Revert the change (`git diff deploy/browser-check.py` must be empty afterwards). `docker ps` shows no `sbx-browser-check-*` container left behind.

- [ ] **Step 4: Document**

`deploy/README.md`: add `browser-check.sh`, `browser-check.py` to the file table ("real-browser check of the web client's renderers against a throwaway dev-login front built from the checkout") and, in the deploy steps right after `smoke.sh`, a step: "Before building the release image: `deploy/browser-check.sh` from the checkout being deployed; all three cases must say `ok`. It needs Docker and `mcr.microsoft.com/playwright/python:v1.55.0-noble`; it cannot run in Forge CI (the runner has no Docker and no internet)."

`CLAUDE.md`, Commands block, after `scripts/wasm-build`:

```bash
deploy/browser-check.sh                                       # Chromium: WebGL2 fallback, WebGPU, no renderer (Docker, Playwright image)
```

and in "Browser client", after the `client-web` bullet: "`deploy/browser-check.sh` proves the renderers in a real browser: plain headless Chromium has the WebGPU API but no adapter (the WebGL2 fallback), `--enable-unsafe-webgpu` gives WebGPU, `--disable-webgl` the explanation page; it puts the page into a game by sending `create_game` on the page's own socket (Playwright `route_web_socket`)."

- [ ] **Step 5: Commit**

```bash
git add deploy/browser-check.sh deploy/browser-check.py deploy/README.md CLAUDE.md
git commit -m "feat(deploy): real-browser check of the WebGL2 fallback, WebGPU and the no-renderer page"
```

---

### Task 9: Final verification

**Files:** none (fixes go back to the task that owns them).

- [ ] **Step 1: The CI gate's builds, warnings as errors**

Run (CLAUDE.md's way of passing environment into the container):

```bash
docker run --rm -u "$(id -u):$(id -g)" -v "$PWD:/w" -w /w -e CARGO_HOME=/w/.cargo-home -e RUSTFLAGS="-D warnings" \
  rust:1.98-slim-bookworm sh -c 'cargo build --workspace --all-targets --locked && cargo test --workspace --locked \
  && cargo build -p signalbox-bot --no-default-features --locked \
  && cargo test -p signalbox-server --features dev-auth --locked'
```

Expected: all green, no warnings.

- [ ] **Step 2: The browser client and the browser check**

Run: `scripts/wasm-build && deploy/browser-check.sh --no-build`
Expected: the three `ok` lines.

- [ ] **Step 3: The numbers for the owner**

Run: `scripts/cargo test --release -p signalbox-client-ui --test legibility -- --nocapture`
Expected: PASS; copy the 1280x800 and 1920x1080 Fit rows and the plan times into the branch report, with the browser-check screenshots.

- [ ] **Step 4: The robot soaks and the WTT (controller)**

Run: `scripts/cargo test --release -p ts2-import --test soak -- --ignored` and `scripts/cargo test --release -p signalbox-bot --test soak -- --ignored`
Expected: PASS. Then Task 4c Step 6 with the owner's PDF, and `git status --short --ignored external/` shows the PDF and its text only as ignored (`!!`).

---

## Controller section (after the branch's final review; not for subagents)

The owner has agreed to redeploy as the realism pass did.

1. **CI cache:** nothing to reseed (no new crates).
2. **Deploy:** first copy the owner's WTT PDF into `external/wtt/` of the checkout being built (`deploy/README.md`, "The Waterloo & City timetable"); the build log must show its sha256 (`7709d5b5…2475b`) and `574 services, 5 entries from 05:40:00`. Then as "Build and run" from the merged commit, and `deploy/smoke.sh https://ra.tail3e0c1e.ts.net:50160 303`. The image now carries Drain's WTT timetable (private: never push it) and the front passes `--current-layout` on resume. Roll back by retagging the previous `<rev>`.
3. **Old saves:** join an existing pre-realism Drain save in the lobby; its signals must read `WA…`/`WB…` and `docker logs signalbox` show `display data from layout drain`. Its timetable still ends at 06:43 (spec P9).
4. **Owner's look (morning):** the `legibility` table and the browser-check screenshots in the branch report; then in Chrome/Edge and Firefox on the tailnet: Liverpool Street box A at Fit (numbers clear of the next platform road), the spectator view of Gretz (no pile-ups; ○A appears one zoom step in), a new Drain game (starts 05:40, 203 in Bank 8, headcodes like `202/1`; through the morning peak the trains keep running, up to about 1½ minutes late under the robot; the simplifier opens at now).
