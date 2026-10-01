# Browser Polish (D1.2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A diagram legible at every zoom (no text over text, numbers off the track), old Drain saves showing today's `W…` names, the simplifier opening at "now", a real-browser check of the WebGL2 fallback, and the hands-on UI review's High and Medium findings fixed: spectating explained, the panel usable at 1024 px, stable controls, named refusals, votes with Agree/Decline, readable zoom and Fit, a lobby that orients a newcomer, and lessons that wait to show their result.

**Architecture:** `paint::draw` stays pure and now records which texts may move (role + other spots) and what must be kept clear; a pure `labels` module plans placement greedily by priority and `UiApp` caches the plan per zoom. Resume optionally takes the current layout file and swaps in its display-only `layout` JSON when the saved network matches. A shell + Python script drives Playwright Chromium against a throwaway dev-login front. The UI review's fixes are client-core logic (tested natively), client-ui screens (tested headless with synthetic input), a few additive protocol fields and messages, two game-library rules (the route in the way of a refusal; how votes end), a lesson-runner rule (a step may wait after its task), the front's lobby push, and converter display data (place names, layout descriptions).

**Tech Stack:** Rust 1.98 (Docker `scripts/cargo`), egui 0.36 (headless tests), serde_json, rusqlite, tokio; Playwright 1.55 Chromium (Docker image on ra).

**Spec:** `docs/superpowers/specs/2026-10-01-browser-polish-design.md` (§3, §5–§7, §10). Its §4 (the Waterloo & City WTT) and P18 (display headcodes) are the separate plan `docs/superpowers/plans/2026-10-01-drain-wtt.md`, which merges first.

**Base:** `main` **after the `drain-wtt` merge** (which itself follows the `robot-fixes` merge). This plan relies on drain-wtt's `Layout.headcodes`, `Layout: Default`, `Names::headcode`, `simplifier::{shown, lines(l, r)}` and `Line.shown`. Every Rust block was compiled and its tests run (workspace, `--features dev-auth`, `scripts/wasm-build`) on a scratch copy of `main` `0c0ea67` with drain-wtt's P18 change and WTT reader applied, Tasks 1–24 in order; `deploy/browser-check.sh` was run end to end on `5eb82f0` when Task 7 was first written. Re-check every file and line reference on the base before each task: `robot-fixes` changes `crates/game/src/view.rs` (`row`, the lateness of standing trains — Task 15 keeps its rule and adds two fields) and the robot; a **`perf-quick-wins`** branch may merge between drain-wtt and this plan and touches `crates/game/src/save.rs` (Task 5), `crates/game/src/game.rs` around lines 156 and 632–667 (Tasks 5 and 12), `crates/protocol/src/lobby.rs` parsing (Tasks 12, 22), `crates/server/src/layouts.rs` and `assets.rs` (Task 22), `scripts/build-web.sh` and the Dockerfile's `wasm-tools` stage — re-apply those tasks' edits by intent there and keep their tests. The diffs in Tasks 8–24 carry scratch line numbers: apply them by content.

## Global Constraints

- License GPL-2.0-or-later; **no new crates** (no `Cargo.lock` package changes, so the CI runner's offline cache needs no reseed).
- Every cargo command runs through `scripts/cargo` from the repo root (Docker `rust:1.98-slim-bookworm`, repo at `/w`, no environment forwarded); wasm32 builds through `scripts/wasm-build`.
- CI builds with `-D warnings --locked --offline`: no unused imports, variables or dead code; rustfmt and clippy are unavailable, so match the surrounding style by hand (4-space indent, ~120 columns).
- Determinism rules stay for `core`, `game`, `protocol` and `ts2-import`: `BTreeMap`/`BTreeSet`/`Vec` only, no wall clock; converter output byte-identical for the same input. The sim never reads the world's `layout` JSON. **No change to `crates/core`** in this plan.
- **Protocol changes are additive only** (spec §10.3), each with a serde default and omitted when empty: `Layout.places`, `LayoutInfo.{title, description}`, `LobbyReply::Layouts.you`, `GameInfo.last_played`, `TrainRow.{arr, dep}`, `VoteView.waiting`, `LessonView.{completed, after}`, `Notice::Rejected.by`; the only new message types are `ClientMsg::VoteDecline` (`vote_decline`) and `Notice::VoteEnded`. **No save-schema change** (save schema stays 2). The only new process argument is `signalbox-game --current-layout <world.json>` (resume only).
- `client-core` and `client-ui` never touch the browser, the clock or storage directly; text sizes reach `labels` through a `measure` function; Sign out reaches the browser only through `UiApp::wants_logout`, which `client-web` follows.
- Placement depends only on the scene, the zoom, the settings and the lesson's highlights: never on train state, never on the screen edge (spec P3, M16).
- Priority (spec P2): own signal numbers, ○A letters, line names, platform numbers, labels, fringe signal numbers. Headcodes are never moved or hidden.
- The legibility targets (spec §3.4, with §10.2 U19): 0 overlapping texts and 0 covered texts in all 66 renders; at 1280 × 800 Fit, in every box view whose Fit frames the whole area, every own number drawn and ≤ 4 numbers tight against track in total.
- Lesson texts name signals and points as the client shows them; `crates/game/tests/lessons.rs` must play every lesson to the end after every task that touches `lessons/` or the runner.
- Infra (deploying, the CI runner, `/opt/stack`, `tailscale serve`) is controller-only, in the final Controller section. Subagents may run `scripts/cargo`, `scripts/wasm-build` and `deploy/browser-check.sh` (it starts and removes its own throwaway container), and must not touch any other container, image, volume or network.

## Review Focus

1. **Train movement re-placing labels** (flicker): placement must be identical with and without a headcode in a berth and after a pan; a lesson's highlight changes it only when the highlight changes. Pinned in Task 2 (`a_plan_depends_on_neither_pan_nor_trains`) and Task 24 (the cache key holds the highlights).
2. **A stale placement plan** applied to a different drawing (numbers toggled, a claim changing the layout, a tutorial pushing extra texts): nothing may be moved by another drawing's offsets. Pinned in Task 1 (`apply_drops_hidden_texts_and_points_the_rest_at_their_new_index`: a plan of the wrong length changes nothing) and Task 3 (the cache key holds game, layout generation, scale and the numbers setting).
3. **A narrow window** (1024 px laptops, a resized browser): the diagram must stay usable and fitted — the panel hides or narrows, the simplifier scrolls sideways with its header, an untouched Fit follows the window and a moved view is left alone. Pinned in Task 18 (`the_panel_hides_and_the_fit_follows_the_window`).
4. **A save whose layout the front no longer lists** (renamed or removed from the image) or whose layout file is unreadable: it must still resume, with its own display data, exactly as today. Pinned in Task 6 (`a_save_of_a_layout_no_longer_listed_still_resumes`) and Task 5 (`a_different_network_or_a_bad_file_keeps_the_saves_own`).
5. **A lesson player who presses Next early** (before a done step's task is done) or whose task completes while they read: the step must still wait and show its result, never skip it. Pinned in Task 23 (`a_done_step_waits_for_next_after_its_task`: an early Next does nothing).

## File Structure

| File | Task | Responsibility |
|---|---|---|
| `crates/client-ui/src/labels.rs` (new) | 1 | Roles, `Movable`, `KeepClear`, `plan`, `apply`, `audit`: pure placement |
| `crates/client-ui/src/paint.rs` | 1, 2, 11, 19, 20, 24 | Placement data; ○A threshold; blocking outline; growing glyphs; points lie; highlights |
| `crates/client-ui/src/hit.rs`, `src/scene.rs` | 2, 19, 21 | ○A threshold; berth width; glyph-aware hits; readable Fit |
| `crates/client-ui/src/screens.rs` | 3, 4, 9–10, 12–13, 15–19, 22–23 | Plan cache; simplifier; lobby; top bar; side panel; diagram input; enquiry; lesson box |
| `crates/client-ui/tests/{labels,legibility}.rs` (new), `tests/{paint,screens,lesson,layouts,hit}.rs` | 1–4, 8–24 | Placement, measurement, drawing, screens |
| `crates/client-core/src/{names,select,input,text,simplifier,app}.rs`, `src/form.rs` (new) | 4, 8–12, 14–16, 22 | Display names, hints, dead clicks, refusals, votes, Leave, lateness, enquiry, the form |
| `crates/protocol/src/{view,msg,lobby,lesson}.rs` | 8, 11, 12, 15, 22, 23 | The additive fields and the two new messages |
| `crates/game/src/{save,game,display,names,view,clock,layout}.rs`, `src/lesson/{run,file}.rs` | 5, 8, 11, 12, 15, 23 | Resume refresh; places; blocker; vote outcomes; Arr/Dep; done steps |
| `crates/server/src/{process,supervisor,layouts}.rs` | 6, 14, 22 | `--current-layout`; the lobby push; layout titles and descriptions |
| `crates/ts2-import/src/{layout,areas}.rs`, `layouts/*.areas.json` | 8, 22 | Place names; layout descriptions |
| `crates/client-web/src/lib.rs` | 22 | Follow Sign out to `/auth/logout` |
| `lessons/*/lesson.json` | 8, 15, 23 | Points names; lateness words; `done` texts; lesson 1's Real aspects step |
| `deploy/browser-check.sh`, `deploy/browser-check.py` (new), `deploy/README.md` | 7 | The real-browser renderer check |
| `CLAUDE.md` | 3, 5–24 | One paragraph per change, in the task that makes it |

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
        let st = PaintState {
            view: Some(v),
            selected: None,
            exits: &[],
            refused: None,
            time: 0.0,
            aspects: AspectMode::RedGreen,
            numbers: true,
            names: &names,
            highlight: &[],
        };
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

---

### Task 2: `draw` records what may move and what to keep clear

**Files:**
- Modify: `crates/client-ui/src/paint.rs`, `crates/client-ui/src/hit.rs`, `crates/client-ui/src/scene.rs`
- Test: `crates/client-ui/tests/paint.rs`, `crates/client-ui/tests/labels.rs`

**Interfaces:**
- Consumes: Task 1's `labels::{Movable, KeepClear, Role, corner}`, `Drawing.movable`/`keep`.
- Consumes also: the drain-wtt plan's `Layout.headcodes` (headcode → displayed headcode).
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

/// Spec P13 with P18: boxes are sized for display headcodes, not the longer
/// unique ones (`202/163` shows as `202`, which fits `BERTH_W`).
#[test]
fn berth_boxes_fit_display_headcodes() {
    use client_ui::hit::BERTH_W;
    let mut l = layout_for(Some("West"));
    l.simplifier.push(SimplifierRow { headcode: "202/163".into(), origin: None, destination: None, calls: vec![] });
    l.headcodes.insert(s("202/163"), s("202"));
    let sc = Scene::build(&l).unwrap();
    assert!(sc.berths.iter().all(|b| b.width_px == BERTH_W), "`202`, not `202/163`");
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
        // Every berth box fits the longest headcode the layout books, as it is
        // displayed (spec P18: the WTT's `202/163` shows as `202`).
        let shown = |h: &str| l.headcodes.get(h).map_or(h.chars().count(), |d| d.chars().count());
        let chars = l.simplifier.iter().map(|r| shown(&r.headcode)).max().unwrap_or(0).max(4);
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
                        highlight: &[],
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
Expected: `legibility` PASSES already (it calls `labels::plan` itself; read its table — at 1280 × 800 Fit: 0 overlaps and 0 covered everywhere, tight 1 on Liverpool Street B and 1 on Drain Waterloo, nothing hidden in box views; checked again on the scratch copy of `0c0ea67`). `the_diagram_never_draws_text_over_text` FAILS with overlaps such as `("2", "2"), ("LA61", "BISHOPSGATE TUNNEL")`: the screen does not place yet. If `legibility` fails after the tutorial merge, stop and report the table rather than loosening a threshold.

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

---

### Task 4: The simplifier opens at "now"

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
                .flat_map(|r| simplifier::lines(l, r).into_iter().enumerate().map(|(i, line)| (line, i == 0)))
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

---

### Task 5: Resume with today's display data

**Files:**
- Modify: `crates/game/src/save.rs`, `crates/game/src/game.rs`, `crates/game/src/lib.rs`, `CLAUDE.md`
- Test: `crates/game/tests/refresh.rs` (new)

**Interfaces:**
- Produces: `game::save::{NETWORK_KEYS: [&str; 8], refresh_display(saved: &str, current: &str) -> Result<String, String>}`; `game::Refresh { NotAsked, Refreshed, Kept(String) }` (re-exported from `game`); `Game::resume_with_layout(path: &Path, current: Option<&str>) -> Result<(Game, Refresh), GameError>`; `Game::resume(path)` unchanged in signature (passes `None`). Task 6 uses `resume_with_layout` and `Refresh`.

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

---

### Task 6: The front passes the current layout on resume

**Files:**
- Modify: `crates/server/src/process.rs`, `crates/server/src/supervisor.rs`, `CLAUDE.md`
- Test: `crates/server/tests/process.rs`, `crates/server/tests/supervisor.rs`

**Interfaces:**
- Consumes: Task 5's `Game::resume_with_layout`, `Refresh`.
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

`CLAUDE.md`, "Server", first bullet, after "…the crash reason the lobby shows).": "On a resume the front passes `--current-layout <world.json>` when it still lists the save's layout; the process logs whether it took that file's display data (Task 5's rule)."

- [ ] **Step 6: Commit**

```bash
git add crates/server CLAUDE.md
git commit -m "feat(server): a resumed game gets the listed layout file for today's display data"
```

---

---

### Task 7: The real-browser renderer check

**Files:**
- Create: `deploy/browser-check.sh` (executable), `deploy/browser-check.py`
- Modify: `deploy/README.md`, `CLAUDE.md`

**Interfaces:**
- Consumes: `scripts/wasm-build`, the dev-auth front, Drain's committed TS2 timetable (the check converts Drain with its areas and lines only, never the WTT: three trains stand in the platforms at 06:00, their headcodes as they are), the client's console line `signalbox: drawing with <backend>` (already in `crates/client-web/src/lib.rs`).
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

---

### Task 8: Points, berths, track and places by their display names (UI review M1, M2)

Spec §10 M1, M2 (U7, U8). Converter ids never reach the player: points as `<box><workstation>P<number>`, berths by
their signal, track by the platform on it; place codes read as TS2's place names, which the converter now writes into
the layout and the game passes on.

**Files:**
- Modify: `crates/client-core/src/names.rs`
- Modify: `crates/client-core/src/select.rs`
- Modify: `crates/client-core/src/text.rs`
- Modify: `crates/client-ui/src/screens.rs`
- Modify: `crates/game/src/display.rs`
- Modify: `crates/game/src/layout.rs`
- Modify: `crates/protocol/src/view.rs`
- Modify: `crates/ts2-import/src/layout.rs`
- Modify: `lessons/01-reading-the-panel/lesson.json`
- Modify: `lessons/02-setting-routes/lesson.json`
- Modify: `CLAUDE.md`
- Test: `crates/client-core/tests/app.rs`
- Test: `crates/client-core/tests/input.rs`
- Test: `crates/client-core/tests/names.rs`
- Test: `crates/client-ui/tests/screens.rs`
- Test: `crates/game/tests/display.rs`
- Test: `crates/ts2-import/tests/convert.rs`

**Interfaces:**
- Consumes: `Names::new(&Layout)` (realism pass), the drain-wtt plan's `Layout.headcodes` and `Layout: Default`.
- Produces: `protocol::Layout.places: BTreeMap<String, String>`; `game::display::places(&World)`, `Display.places`;
  `client_core::names::points_number(&str) -> String`; `Names::{points(&str) -> String, berth(&str) -> String,
  track(&str) -> String, place(&str) -> &str}`; the converted world's `layout.places`.

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-core/tests/app.rs b/crates/client-core/tests/app.rs
index d2320ea..4d03556 100644
--- a/crates/client-core/tests/app.rs
+++ b/crates/client-core/tests/app.rs
@@ -327,12 +327,12 @@ fn notices_are_logged_with_alarms_and_the_log_is_bounded() {
         ]
     );
     for i in 0..(LOG_CAP + 50) {
-        h.push(notice(Notice::Collision { section: format!("T{i}") }));
+        h.push(notice(Notice::Error { code: s("x"), message: format!("e{i}") }));
     }
     app.tick(3.0);
     let log = app.game().unwrap().log();
     assert_eq!(log.len(), LOG_CAP);
-    assert_eq!(log.entries().last().unwrap().text, format!("COLLISION on T{}", LOG_CAP + 49));
+    assert_eq!(log.entries().last().unwrap().text, format!("Error: e{}", LOG_CAP + 49));
 }
 
 #[test]
diff --git a/crates/client-core/tests/input.rs b/crates/client-core/tests/input.rs
index 7b10f70..f690732 100644
--- a/crates/client-core/tests/input.rs
+++ b/crates/client-core/tests/input.rs
@@ -75,7 +75,7 @@ fn fringe_and_spectators_get_hover_only() {
     t.app.click(&sig("C"));
     assert_eq!(t.app.game().unwrap().selected(), None, "C is East's, seen on West's fringe");
     assert!(t.app.menu(&Target::Points(s("P"))).is_empty());
-    assert_eq!(t.app.describe(&Target::Points(s("P"))), "Points P (East): normal");
+    assert_eq!(t.app.describe(&Target::Points(s("P"))), "Points TBP (East): normal");
     let mut spec = Table::new("sam", None);
     spec.app.click(&sig("W1"));
     assert_eq!(spec.app.game().unwrap().selected(), None);
@@ -93,7 +93,7 @@ fn right_click_cancels_a_route_and_swings_points() {
     let mut t = Table::new("eve", Some("East"));
     assert_eq!(
         t.app.menu(&Target::Points(s("P"))),
-        [MenuItem { label: s("Swing P reverse"), cmd: PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse } }]
+        [MenuItem { label: s("Swing TBP reverse"), cmd: PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse } }]
     );
     assert!(t.app.menu(&sig("C")).is_empty(), "no route set from C");
     t.app.click(&sig("C"));
@@ -149,7 +149,7 @@ fn berths_interpose_a_typed_headcode_and_cancel_it() {
     t.pump();
     t.run(0.5);
     assert_eq!(t.view().berths.get("BA").map(String::as_str), Some("2Z99"));
-    assert_eq!(t.app.describe(&Target::Berth(s("BA"))), "Berth BA: 2Z99");
+    assert_eq!(t.app.describe(&Target::Berth(s("BA"))), "Berth TAA: 2Z99");
     assert_eq!(
         t.app.menu(&Target::Berth(s("BA"))),
         [MenuItem { label: s("Cancel 2Z99"), cmd: PlayerCommand::CancelBerth { berth: s("BA") } }]
@@ -358,8 +358,8 @@ fn the_train_list_puts_platforms_first_then_by_booked_time() {
 #[test]
 fn hover_describes_track_and_names_other_areas() {
     let t = Table::new("ann", Some("West"));
-    assert_eq!(t.app.describe(&Target::Section(s("TW1"))), "Track TW1: clear");
-    assert_eq!(t.app.describe(&Target::Section(s("TP"))), "Track TP (East): clear");
+    assert_eq!(t.app.describe(&Target::Section(s("TW1"))), "Track: clear");
+    assert_eq!(t.app.describe(&Target::Section(s("TP"))), "Track (East): clear");
     assert_eq!(t.app.describe(&Target::Exit(s("W"))), "Exit W");
     assert_eq!(t.app.describe(&sig("nowhere")), "Signal nowhere");
 }
diff --git a/crates/client-core/tests/names.rs b/crates/client-core/tests/names.rs
index 442348b..7526e3d 100644
--- a/crates/client-core/tests/names.rs
+++ b/crates/client-core/tests/names.rs
@@ -91,3 +91,27 @@ fn headcodes_are_shown_as_the_layout_says() {
     let late = Notice::Late { train: s("201/7"), place: s("BNK"), platform: s("7"), late_s: 120 };
     assert_eq!(notice_text(&late, &n).0, "201 at BNK 7, 2 min late");
 }
+
+/// Polish spec M1, M2: points, berths and track are never shown by their
+/// converter ids, and place codes read as their names.
+#[test]
+fn points_berths_track_and_places_have_display_names() {
+    use client_core::names::points_number;
+    assert_eq!((points_number("N153"), points_number("P1"), points_number("P"), points_number("X9")), (s("P153"), s("P1"), s("P"), s("X9")));
+    let mut l = one_box("L", &[]);
+    l.points.push(PointsInfo { name: s("N153"), section: s("T7"), area: s("A"), operable: true });
+    l.berths.push(BerthInfo { name: s("B121"), signal: Some(s("121")), boundary: None, area: s("A"), operable: true });
+    l.berths.push(BerthInfo { name: s("BX"), signal: None, boundary: Some(s("N9")), area: s("A"), operable: true });
+    l.segments.push(SegmentInfo { name: s("L5"), from: s("N1"), to: s("N2"), length_m: 100.0, section: s("T7") });
+    l.platforms.push(PlatformInfo { place: s("LIVST"), platform: s("10"), segment: s("L5"), from_m: 0.0, to_m: 90.0 });
+    l.places.insert(s("LIVST"), s("LIVERPOOL STREET"));
+    let n = Names::new(&l);
+    assert_eq!((n.points("N153"), n.points("unknown")), (s("LP153"), s("unknown")));
+    assert_eq!((n.berth("B121"), n.berth("BX"), n.berth("B0")), (s("L121"), s("edge"), s("B0")));
+    assert_eq!((n.track("T7"), n.track("T8")), (s("Track at LIVERPOOL STREET 10"), s("Track")));
+    assert_eq!((n.place("LIVST"), n.place("BNK")), ("LIVERPOOL STREET", "BNK"));
+    let late = Notice::Late { train: s("1P02"), place: s("LIVST"), platform: s("10"), late_s: 60 };
+    assert_eq!(notice_text(&late, &n).0, "1P02 at LIVERPOOL STREET 10, 1 min late");
+    assert_eq!(command_text(&PlayerCommand::SwingPoints { points: s("N153"), to: PointsPos::Reverse }, &n), "swing LP153 reverse");
+    assert_eq!(notice_text(&Notice::Collision { section: s("T7") }, &n).0, "COLLISION: Track at LIVERPOOL STREET 10");
+}
diff --git a/crates/client-ui/tests/screens.rs b/crates/client-ui/tests/screens.rs
index 9ae5580..9215977 100644
--- a/crates/client-ui/tests/screens.rs
+++ b/crates/client-ui/tests/screens.rs
@@ -287,7 +287,7 @@ fn right_click_opens_the_menu_for_what_is_under_the_pointer() {
     let ba = r.at(200.0, 0.0) - vec2(client_ui::scene::BERTH_BACK_PX, 0.0);
     r.click(ba, PointerButton::Secondary);
     let out = r.frame();
-    assert!(has_text(&out, "Berth BA: empty"), "{:?}", texts(&out));
+    assert!(has_text(&out, "Berth TAA: empty"), "{:?}", texts(&out));
     assert!(has_text(&out, "Interpose"));
 }
 
diff --git a/crates/game/tests/display.rs b/crates/game/tests/display.rs
index 220ed2d..3f7f072 100644
--- a/crates/game/tests/display.rs
+++ b/crates/game/tests/display.rs
@@ -157,3 +157,15 @@ fn display_headcodes_reach_the_layout() {
     assert_eq!(l.headcodes, map(&[("1E01", s("1E"))]));
     assert_eq!(g.sim().world().services[0].headcode, "1E01", "the sim's headcode is unchanged");
 }
+
+/// Polish spec M2: place names from the world's `layout` reach every layout.
+#[test]
+fn place_names_reach_the_layout() {
+    let w = twobox_mut(|j| j["layout"] = json!({"places": {"EST": "EASTON", "NST": 7}}));
+    assert_eq!(game::display::places(&w), map(&[("EST", s("EASTON"))]), "a name that is not text is skipped");
+    let mut g = Game::new(w, meta());
+    let outs = g.connect("ann");
+    let Some(ServerMsg::Layout(l)) = outs.iter().map(|o| &o.1).find(|m| matches!(m, ServerMsg::Layout(_))) else { panic!() };
+    assert_eq!(l.places, map(&[("EST", s("EASTON"))]));
+    assert!(game::display::places(&twobox()).is_empty());
+}
diff --git a/crates/ts2-import/tests/convert.rs b/crates/ts2-import/tests/convert.rs
index 9012369..945ad14 100644
--- a/crates/ts2-import/tests/convert.rs
+++ b/crates/ts2-import/tests/convert.rs
@@ -49,3 +49,13 @@ fn conversion_is_deterministic() {
     let b = serde_json::to_string(&ts2_import::convert(&data("liverpool-st")).unwrap().world).unwrap();
     assert_eq!(a, b);
 }
+
+/// Polish spec M2: the converted layout names TS2's places by their codes.
+#[test]
+fn places_are_named_in_the_layout() {
+    let w = ts2_import::convert(&data("liverpool-st")).unwrap().world;
+    assert_eq!(w.layout["places"]["LIVST"], "LIVERPOOL STREET");
+    assert_eq!(w.layout["places"]["HAKNYNM"], "HACKNEY DOWNS");
+    let w = ts2_import::convert(&data("gretz-armainvilliers")).unwrap().world;
+    assert!(w.layout["places"].as_object().unwrap().keys().all(|k| !k.is_empty()), "places without a code are only labels");
+}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-core --test names --test input -p signalbox-game --test display -p ts2-import --test convert`
Expected: compile errors (`points_number`, `places` not found). (The old expectations in `tests/input.rs` and
`screens.rs` — `Points P`, `Berth BA`, `Track TW1` — are the ones this step updates.)

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-core/src/names.rs b/crates/client-core/src/names.rs
index 5f0db16..d9fd858 100644
--- a/crates/client-core/src/names.rs
+++ b/crates/client-core/src/names.rs
@@ -1,8 +1,11 @@
 //! Display names (realism spec §2, owner decision 11): a signal is shown as
 //! `<box><workstation><name>` (`LA9`, `LB72`), without the workstation
 //! letter on a single-area layout (`L9`). Display only: everything sent
-//! keeps the plain name. Other names (berths, points, track, nodes) are
-//! shown as they are. A headcode is shown as its service's display
+//! keeps the plain name. Points are shown the same way with a `P` before
+//! their number (`LAP153` for converted `N153`, `HP1` for a lesson's `P1`),
+//! a berth by its signal's name, track only by the platform on it, and a
+//! place code by its name where the layout gives one (polish spec M1, M2);
+//! nodes are shown as they are. A headcode is shown as its service's display
 //! headcode when the layout gives one (polish spec P18: `201/7` is `201`).
 
 use std::collections::BTreeMap;
@@ -17,6 +20,21 @@ pub struct Names {
     workstations: BTreeMap<String, String>,
     /// Headcode → display headcode, where they differ.
     headcodes: BTreeMap<String, String>,
+    /// Plain points name → displayed name.
+    points: BTreeMap<String, String>,
+    /// Berth → its signal's displayed name; `None` for a boundary berth.
+    berths: BTreeMap<String, Option<String>>,
+    /// Section → the platforms on it, as `place platform`.
+    platforms: BTreeMap<String, Vec<String>>,
+    /// Place code → name.
+    places: BTreeMap<String, String>,
+}
+
+/// A points name's number for display: the digits after a leading `N` or
+/// `P` (`N153`, `P1`), else the name itself; always with a `P` in front.
+pub fn points_number(name: &str) -> String {
+    let rest = name.strip_prefix('N').or_else(|| name.strip_prefix('P')).unwrap_or("");
+    if !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()) { format!("P{rest}") } else { name.to_string() }
 }
 
 impl Names {
@@ -25,7 +43,21 @@ impl Names {
         let letter = |area: &str| if single { "" } else { l.workstations.get(area).map_or("", String::as_str) };
         let signals = l.signals.iter().map(|s| (s.name.clone(), format!("{}{}{}", l.box_prefix, letter(&s.area), s.name))).collect();
         let workstations = if single { BTreeMap::new() } else { l.workstations.clone() };
-        Names { signals, workstations, headcodes: l.headcodes.clone() }
+        let points = l.points.iter().map(|p| (p.name.clone(), format!("{}{}{}", l.box_prefix, letter(&p.area), points_number(&p.name)))).collect();
+        let signal = |n: &String| l.signals.iter().find(|s| s.name == *n).map(|s| format!("{}{}{}", l.box_prefix, letter(&s.area), s.name));
+        let berths = l.berths.iter().map(|b| (b.name.clone(), b.signal.as_ref().and_then(signal))).collect();
+        let place = |c: &str| l.places.get(c).map_or(c.to_string(), String::clone);
+        let mut platforms: BTreeMap<String, Vec<String>> = BTreeMap::new();
+        for p in &l.platforms {
+            if let Some(g) = l.segments.iter().find(|g| g.name == p.segment) {
+                let text = format!("{} {}", place(&p.place), p.platform);
+                let list = platforms.entry(g.section.clone()).or_default();
+                if !list.contains(&text) {
+                    list.push(text);
+                }
+            }
+        }
+        Names { signals, workstations, headcodes: l.headcodes.clone(), points, berths, platforms, places: l.places.clone() }
     }
 
     /// How a signal is shown; a name the layout does not list stays plain.
@@ -41,6 +73,35 @@ impl Names {
         }
     }
 
+    /// How points are shown (`LAP153`); a name the layout does not list stays plain.
+    pub fn points(&self, name: &str) -> String {
+        self.points.get(name).cloned().unwrap_or_else(|| name.to_string())
+    }
+
+    /// How a berth is shown: by its signal (`LA29`), `edge` for a boundary
+    /// berth, plain for one the layout does not list.
+    pub fn berth(&self, name: &str) -> String {
+        match self.berths.get(name) {
+            Some(Some(s)) => s.clone(),
+            Some(None) => "edge".to_string(),
+            None => name.to_string(),
+        }
+    }
+
+    /// Track is never shown by its id: `Track at LIVERPOOL STREET 10`, or
+    /// just `Track` where no platform is on it.
+    pub fn track(&self, section: &str) -> String {
+        match self.platforms.get(section) {
+            Some(p) => format!("Track at {}", p.join(", ")),
+            None => "Track".to_string(),
+        }
+    }
+
+    /// A place's name (`LIVERPOOL STREET`), else its code.
+    pub fn place<'a>(&'a self, code: &'a str) -> &'a str {
+        self.places.get(code).map_or(code, String::as_str)
+    }
+
     /// How a headcode is shown: its service's display headcode, else as it is.
     pub fn headcode<'a>(&'a self, h: &'a str) -> &'a str {
         self.headcodes.get(h).map_or(h, String::as_str)
diff --git a/crates/client-core/src/select.rs b/crates/client-core/src/select.rs
index 43f6baa..cc816f0 100644
--- a/crates/client-core/src/select.rs
+++ b/crates/client-core/src/select.rs
@@ -147,7 +147,8 @@ pub fn points_menu(l: &Layout, v: &View, points: &str) -> Vec<MenuItem> {
         PointsPos::Normal => PointsPos::Reverse,
         PointsPos::Reverse => PointsPos::Normal,
     };
-    vec![MenuItem { label: format!("Swing {points} {}", pos_text(to)), cmd: PlayerCommand::SwingPoints { points: points.to_string(), to } }]
+    let label = format!("Swing {} {}", Names::new(l).points(points), pos_text(to));
+    vec![MenuItem { label, cmd: PlayerCommand::SwingPoints { points: points.to_string(), to } }]
 }
 
 /// Cancelling a berth's headcode; interposing needs a headcode typed in,
@@ -211,21 +212,26 @@ pub fn describe_signal(l: &Layout, v: &View, signal: &str) -> String {
 
 pub fn describe_points(l: &Layout, v: &View, points: &str) -> String {
     let area = l.points.iter().find(|p| p.name == points).map(|p| area_note(l, &p.area)).unwrap_or_default();
+    let shown = Names::new(l).points(points);
     match v.points.get(points) {
         Some(p) => format!(
-            "Points {points}{area}: {}{}{}",
+            "Points {shown}{area}: {}{}{}",
             pos_text(p.position),
             if p.moving { ", moving" } else { "" },
             if p.locked { ", locked" } else { "" }
         ),
-        None => format!("Points {points}{area}"),
+        None => format!("Points {shown}{area}"),
     }
 }
 
 pub fn describe_berth(l: &Layout, v: &View, berth: &str) -> String {
     let area = l.berths.iter().find(|b| b.name == berth).map(|b| area_note(l, &b.area)).unwrap_or_default();
     let names = Names::new(l);
-    format!("Berth {berth}{area}: {}", v.berths.get(berth).map_or("empty", |h| names.headcode(h)))
+    let what = match names.berth(berth).as_str() {
+        "edge" => "Edge berth".to_string(),
+        b => format!("Berth {b}"),
+    };
+    format!("{what}{area}: {}", v.berths.get(berth).map_or("empty", |h| names.headcode(h)))
 }
 
 pub fn describe_section(l: &Layout, v: &View, section: &str) -> String {
@@ -237,5 +243,5 @@ pub fn describe_section(l: &Layout, v: &View, section: &str) -> String {
         Some(_) => "clear",
         None => "?",
     };
-    format!("Track {section}{area}: {state}")
+    format!("{}{area}: {state}", Names::new(l).track(section))
 }
diff --git a/crates/client-core/src/text.rs b/crates/client-core/src/text.rs
index bf13a1e..1ed9baf 100644
--- a/crates/client-core/src/text.rs
+++ b/crates/client-core/src/text.rs
@@ -33,9 +33,9 @@ pub fn command_text(c: &PlayerCommand, names: &Names) -> String {
         PlayerCommand::SetAutoWorking { entrance, on } => {
             format!("auto-working {} at {}", if *on { "on" } else { "off" }, names.signal(entrance))
         }
-        PlayerCommand::SwingPoints { points, to } => format!("swing {points} {}", pos_text(*to)),
-        PlayerCommand::Interpose { berth, headcode } => format!("interpose {headcode} in {berth}"),
-        PlayerCommand::CancelBerth { berth } => format!("cancel berth {berth}"),
+        PlayerCommand::SwingPoints { points, to } => format!("swing {} {}", names.points(points), pos_text(*to)),
+        PlayerCommand::Interpose { berth, headcode } => format!("interpose {headcode} at {}", names.berth(berth)),
+        PlayerCommand::CancelBerth { berth } => format!("cancel the headcode at {}", names.berth(berth)),
     }
 }
 
@@ -63,12 +63,12 @@ pub fn notice_text(n: &Notice, names: &Names) -> (String, bool) {
         Notice::Spad { signal, train } => {
             (format!("SPAD: {} passed {} at danger", names.headcode(train), names.signal(signal)), true)
         }
-        Notice::Collision { section } => (format!("COLLISION on {section}"), true),
+        Notice::Collision { section } => (format!("COLLISION: {}", names.track(section)), true),
         Notice::Late { train, place, platform, late_s } => {
-            (format!("{} at {place} {platform}, {} min late", names.headcode(train), late_s / 60), false)
+            (format!("{} at {} {platform}, {} min late", names.headcode(train), names.place(place), late_s / 60), false)
         }
         Notice::WrongPlatform { train, place, platform, expected } => {
-            (format!("{} at {place} platform {platform}, booked {expected}", names.headcode(train)), true)
+            (format!("{} at {} platform {platform}, booked {expected}", names.headcode(train), names.place(place)), true)
         }
         Notice::Handover { headcode, from_area } => (format!("{} offered from {from_area}", names.headcode(headcode)), false),
         Notice::AreaTaken { area, holder } => (format!("{area} is now {holder}'s"), false),
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index 348b037..b5de06d 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -557,7 +557,12 @@ impl UiApp {
                         (Some(p), None) => p.clone(),
                         (None, _) => "—".to_string(),
                     };
-                    ui.label(next);
+                    // Codes in the table; the place's name on hover (polish spec M2).
+                    let place = r.next_place.as_deref().map(|p| g.names().place(p).to_string());
+                    let cell = ui.label(next);
+                    if let Some(name) = place {
+                        cell.on_hover_text(name);
+                    }
                     ui.label(r.booked.map_or(String::new(), |b| fmt_hms(b)[..5].to_string()));
                     ui.label(if r.late_s > 0 { format!("+{}", r.late_s / 60) } else { String::new() });
                     ui.end_row();
@@ -631,9 +636,11 @@ impl UiApp {
                 ui.label("Not in the simplifier for this area");
             }
             for r in &e.rows {
-                ui.label(format!("{} to {}", r.origin.as_deref().unwrap_or("?"), r.destination.as_deref().unwrap_or("?")));
+                let names = g.names();
+                let place = |p: Option<&str>| p.map_or("?", |p| names.place(p)).to_string();
+                ui.label(format!("{} to {}", place(r.origin.as_deref()), place(r.destination.as_deref())));
                 for line in simplifier::lines(l, r) {
-                    ui.label(format!("{} {} {} {}", line.place, line.platform, line.arr, line.dep));
+                    ui.label(format!("{} {} {} {}", names.place(&line.place), line.platform, line.arr, line.dep));
                 }
             }
         });
diff --git a/crates/game/src/display.rs b/crates/game/src/display.rs
index 3c44f50..f230f1f 100644
--- a/crates/game/src/display.rs
+++ b/crates/game/src/display.rs
@@ -106,6 +106,13 @@ pub fn simplifier(w: &World, area: Option<AreaId>) -> Vec<SimplifierRow> {
     rows
 }
 
+/// Place code → name, from the world's `layout` JSON (`places`, written by
+/// ts2-import); empty when missing, and entries that are not text are skipped.
+pub fn places(w: &World) -> BTreeMap<String, String> {
+    let Some(m) = w.layout.get("places").and_then(|v| v.as_object()) else { return BTreeMap::new() };
+    m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect()
+}
+
 /// What every layout of one game shares, per area.
 #[derive(Clone, Debug, PartialEq)]
 pub struct Display {
@@ -113,6 +120,8 @@ pub struct Display {
     pub workstations: BTreeMap<String, String>,
     /// Headcode → display headcode, where they differ (polish spec P18).
     pub headcodes: BTreeMap<String, String>,
+    /// Place code → name (polish spec M2).
+    pub places: BTreeMap<String, String>,
     spectator: Vec<SimplifierRow>,
     by_area: Vec<Vec<SimplifierRow>>,
 }
@@ -122,7 +131,7 @@ impl Display {
         let (box_prefix, workstations) = prefixes(w);
         let by_area = (0..w.net.areas.len()).map(|a| simplifier(w, Some(AreaId::from_idx(a)))).collect();
         let headcodes = w.services.iter().filter_map(|s| Some((s.headcode.clone(), s.display.clone()?))).collect();
-        Display { box_prefix, workstations, headcodes, spectator: simplifier(w, None), by_area }
+        Display { box_prefix, workstations, headcodes, places: places(w), spectator: simplifier(w, None), by_area }
     }
 
     /// The simplifier for `area` (a spectator's for `None`).
@@ -138,6 +147,7 @@ impl Display {
         l.box_prefix = self.box_prefix.clone();
         l.workstations = self.workstations.clone();
         l.headcodes = self.headcodes.clone();
+        l.places = self.places.clone();
         l.simplifier = self.simplifier(area).to_vec();
     }
 }
diff --git a/crates/game/src/layout.rs b/crates/game/src/layout.rs
index 765dc24..f607f62 100644
--- a/crates/game/src/layout.rs
+++ b/crates/game/src/layout.rs
@@ -125,5 +125,6 @@ pub fn build_layout(w: &World, map: &AreaMap, vis: &Visibility, you: &str, geo:
         workstations: Default::default(),
         simplifier: vec![],
         headcodes: Default::default(),
+        places: Default::default(),
     }
 }
diff --git a/crates/protocol/src/view.rs b/crates/protocol/src/view.rs
index 5520d73..ea50e42 100644
--- a/crates/protocol/src/view.rs
+++ b/crates/protocol/src/view.rs
@@ -42,6 +42,10 @@ pub struct Layout {
     /// headcode differs (polish spec P18); every other headcode is shown as it is.
     #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
     pub headcodes: BTreeMap<String, String>,
+    /// Place code → its name (`LIVST` → `LIVERPOOL STREET`, polish spec M2),
+    /// where the world gives one.
+    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
+    pub places: BTreeMap<String, String>,
 }
 
 /// One service in the simplifier: where it runs from and to, and its calls
diff --git a/crates/ts2-import/src/layout.rs b/crates/ts2-import/src/layout.rs
index 04f2dde..1ccd07a 100644
--- a/crates/ts2-import/src/layout.rs
+++ b/crates/ts2-import/src/layout.rs
@@ -1,4 +1,5 @@
-//! Diagram geometry for clients, in TS2 scene coordinates.
+//! Diagram geometry for clients, in TS2 scene coordinates, and the names of
+//! TS2's places (polish spec M2: `LIVST` is LIVERPOOL STREET).
 
 use serde_json::{Value, json};
 
@@ -7,6 +8,7 @@ use crate::ts2::{Item, Ts2};
 
 pub fn build(ts2: &Ts2, g: &Graph) -> Value {
     let (mut lines, mut points, mut signals, mut platforms, mut labels) = (vec![], vec![], vec![], vec![], vec![]);
+    let mut places = serde_json::Map::new();
     for (id, it) in &ts2.track_items {
         match it {
             Item::LineItem(l) | Item::InvisibleLinkItem(l) => {
@@ -30,6 +32,9 @@ pub fn build(ts2: &Ts2, g: &Graph) -> Value {
             Item::Place(p) => {
                 if let Some(name) = &p.name {
                     labels.push(json!({"text": name, "x": p.x, "y": p.y}));
+                    if let Some(code) = p.place_code.as_ref().filter(|c| !c.is_empty()) {
+                        places.entry(code.clone()).or_insert_with(|| json!(name));
+                    }
                 }
             }
             Item::TextItem(t) => {
@@ -40,5 +45,5 @@ pub fn build(ts2: &Ts2, g: &Graph) -> Value {
             Item::EndItem(_) => {}
         }
     }
-    json!({"source": "ts2", "lines": lines, "points": points, "signals": signals, "platforms": platforms, "labels": labels})
+    json!({"source": "ts2", "lines": lines, "points": points, "signals": signals, "platforms": platforms, "labels": labels, "places": places})
 }
diff --git a/lessons/01-reading-the-panel/lesson.json b/lessons/01-reading-the-panel/lesson.json
index b4cb6a2..226a381 100644
--- a/lessons/01-reading-the-panel/lesson.json
+++ b/lessons/01-reading-the-panel/lesson.json
@@ -19,7 +19,7 @@
       "wait_for": {"continue": {}}
     },
     {
-      "say": "The ochre block is platform 1 at Saltmarsh station; its number is written on it. You can point at any part of the track to read its name and whether a train is on it.",
+      "say": "The ochre block is platform 1 at Saltmarsh station; its number is written on it. You can point at any part of the track to read whether a train is on it, and at which platform.",
       "highlight": [{"platform": {"place": "SLT", "platform": "1"}}],
       "wait_for": {"continue": {}}
     },
diff --git a/lessons/02-setting-routes/lesson.json b/lessons/02-setting-routes/lesson.json
index 5b8b808..5aad0cc 100644
--- a/lessons/02-setting-routes/lesson.json
+++ b/lessons/02-setting-routes/lesson.json
@@ -42,7 +42,7 @@
       "wait_for": {"continue": {}}
     },
     {
-      "say": "The lesson has cancelled its route. Points can also be moved by hand when no route holds them. Right-click the points P1 (where the line splits before the platforms) and choose 'Swing P1 reverse'. Reverse leads to platform 2.",
+      "say": "The lesson has cancelled its route. Points can also be moved by hand when no route holds them. Right-click the points HP1 (where the line splits before the platforms) and choose 'Swing HP1 reverse'. Reverse leads to platform 2.",
       "highlight": [{"points": "P1"}],
       "do": [{"cancel_route": {"entrance": "3"}}],
       "wait_for": {"points": {"name": "P1", "position": "reverse"}}
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-core -p signalbox-client-ui -p signalbox-game -p ts2-import`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Browser client", after the `Names` sentence ("signals are shown as …"): "Points are shown the same way
with a `P` (`LAP153`), berths by their signal, track only by the platform on it (polish spec M1); place codes by the
names ts2-import writes into `layout.places` (M2): tables keep codes and show names on hover."

- [ ] **Step 6: Commit**

```bash
git add crates/client-core crates/client-core/src/names.rs crates/client-core/src/select.rs crates/client-core/src/text.rs crates/client-ui crates/client-ui/src/screens.rs crates/game crates/game/src/display.rs crates/game/src/layout.rs crates/protocol/src/view.rs crates/ts2-import crates/ts2-import/src/layout.rs lessons/01-reading-the-panel/lesson.json lessons/02-setting-routes/lesson.json CLAUDE.md
git commit -m "feat(client): points, berths, track and places by display names, never converter ids"
```

---

### Task 9: Spectating is explained (UI review H2)

Spec §10 H2 (U1, U2). The New game form chooses where the creator starts; a spectator is told how to signal; a
click that chooses nothing says why, once.

**Files:**
- Modify: `crates/client-core/src/app.rs`
- Modify: `crates/client-core/src/input.rs`
- Modify: `crates/client-core/src/select.rs`
- Modify: `crates/client-ui/src/screens.rs`
- Modify: `CLAUDE.md`
- Test: `crates/client-core/tests/app.rs`
- Test: `crates/client-core/tests/input.rs`
- Test: `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: Task 8's `Names`; `App::create_game`.
- Produces: `App::create_game_in(layout, seed, start, area: Option<&str>)`; `App.claim_on_join` (claims once, on the
  new game's first spectator layout); `select::why_not_entrance(&Layout, &str) -> String`; `InGame::log_once(String)`;
  `UiApp`'s `NewGame.area` (0 = watch).

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-core/tests/app.rs b/crates/client-core/tests/app.rs
index 4d03556..38441d1 100644
--- a/crates/client-core/tests/app.rs
+++ b/crates/client-core/tests/app.rs
@@ -577,3 +577,27 @@ fn an_unreadable_frame_in_the_lobby_refreshes_the_lobby() {
     assert!(app.lobby_note().unwrap().starts_with("Unreadable message from the server"));
     assert_eq!(h.take_sent(), [lobby(LobbyMsg::ListGames)]);
 }
+
+/// Polish spec H2: a game created "to signal" an area claims it as soon as
+/// its first layout comes, once; a plain create stays watching.
+#[test]
+fn a_new_game_claims_the_creators_area_once_its_layout_comes() {
+    let (mut app, h) = open_app();
+    h.take_sent();
+    app.create_game_in("twobox", None, None, Some("West"));
+    assert_eq!(h.take_sent(), [lobby(LobbyMsg::CreateGame { layout: s("twobox"), seed: None, start: None })]);
+    h.push(joined("g-new"));
+    h.push(layout("ann"));
+    app.tick(1.0);
+    assert_eq!(h.take_sent(), [ClientFrame::Game(ClientMsg::Claim { area: s("West") })]);
+    h.push(layout("ann"));
+    app.tick(2.0);
+    assert!(h.take_sent().is_empty(), "only once");
+    let (mut app, h) = open_app();
+    h.take_sent();
+    app.create_game_in("twobox", None, None, None);
+    h.push(joined("g-new"));
+    h.push(layout("ann"));
+    app.tick(1.0);
+    assert_eq!(h.take_sent(), [lobby(LobbyMsg::CreateGame { layout: s("twobox"), seed: None, start: None })], "no claim");
+}
diff --git a/crates/client-core/tests/input.rs b/crates/client-core/tests/input.rs
index f690732..f085bb4 100644
--- a/crates/client-core/tests/input.rs
+++ b/crates/client-core/tests/input.rs
@@ -407,3 +407,20 @@ fn cancelling_routes_and_busy_points_offer_no_menu() {
     v.points.insert(s("P"), PointsView { position: PointsPos::Normal, moving: false, locked: false });
     assert_eq!(select::points_menu(&l, &v, "P").len(), 1);
 }
+
+/// Polish spec H2: a click that chooses nothing says why, once.
+#[test]
+fn a_click_that_chooses_nothing_says_why_once() {
+    let mut spec = Table::new("sam", None);
+    spec.app.click(&sig("W1"));
+    spec.app.click(&sig("W1"));
+    spec.app.click(&Target::Auto(s("W1")));
+    assert_eq!(spec.log_lines(), [(s("You are watching: claim an area to signal"), false)]);
+    let mut t = Table::new("ann", Some("West"));
+    t.app.click(&sig("C"));
+    t.app.click(&Target::Auto(s("W1")));
+    assert_eq!(
+        t.log_lines(),
+        [(s("C is not in your area"), false), (s("Auto-working TAW1: set a route from it first"), false)]
+    );
+}
diff --git a/crates/client-ui/tests/screens.rs b/crates/client-ui/tests/screens.rs
index 9215977..823b350 100644
--- a/crates/client-ui/tests/screens.rs
+++ b/crates/client-ui/tests/screens.rs
@@ -732,3 +732,23 @@ fn the_simplifier_opens_at_now() {
     assert!(!side.iter().any(|t| t == "BW01"), "BW01 ran at 06:00: {side:?}");
     assert!(side.iter().any(|t| t == "BW06") && side.iter().any(|t| t == "BW07"), "06:30's trains: {side:?}");
 }
+
+/// Polish spec H2: the lobby offers an area to signal, a spectator is told
+/// how to start signalling, and a click on a signal while watching says why
+/// nothing happened.
+#[test]
+fn a_spectator_is_told_to_claim_an_area() {
+    let r = Rig::lobby(drawn_twobox());
+    let mut r = r;
+    let out = r.frame();
+    assert!(has_text(&out, "Signal") && has_text(&out, "watch"), "{:?}", texts(&out));
+    let mut r = Rig::in_game(drawn_twobox(), None);
+    let out = r.frame();
+    assert!(has_text(&out, "You are watching. Claim an area to signal:"));
+    let w1 = r.at(100.0, 0.0);
+    r.click(w1, PointerButton::Primary);
+    let out = r.frame();
+    assert!(has_text(&out, "You are watching: claim an area to signal"), "{:?}", side_texts(&r, &out));
+    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
+    assert!(!has_text(&r.frame(), "You are watching"));
+}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-core --test app --test input -p signalbox-client-ui --test screens a_spectator`
Expected: compile errors (`create_game_in` not found).

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-core/src/app.rs b/crates/client-core/src/app.rs
index cb8fd02..6183d5e 100644
--- a/crates/client-core/src/app.rs
+++ b/crates/client-core/src/app.rs
@@ -139,6 +139,16 @@ impl InGame {
     fn sim_time(&self) -> Option<f64> {
         self.bot.view().map(|v| v.sim_time)
     }
+
+    /// Log `text` (not an alarm) unless it is already the newest line, so a
+    /// player clicking again and again gets one line.
+    pub(crate) fn log_once(&mut self, text: String) {
+        if self.log.entries().next_back().is_some_and(|e| e.text == text) {
+            return;
+        }
+        let t = self.sim_time();
+        self.log.push(t, text, false);
+    }
 }
 
 /// A `join` or `create_game` whose `joined` has not come yet.
@@ -174,6 +184,9 @@ pub struct App {
     pub(crate) last_frame: f64,
     /// The watchdog sent a join on this connection and no frame has come since.
     pub(crate) watchdog_join_sent: bool,
+    /// The area to claim once the game just created sends its first layout
+    /// (polish spec H2: the creator signals at once instead of watching).
+    pub(crate) claim_on_join: Option<String>,
 }
 
 impl App {
@@ -196,6 +209,7 @@ impl App {
             me: None,
             last_frame: now,
             watchdog_join_sent: false,
+            claim_on_join: None,
         }
     }
 
@@ -313,6 +327,7 @@ impl App {
     }
 
     fn to_lobby(&mut self, note: Option<String>) {
+        self.claim_on_join = None;
         self.game = None;
         self.rejoin = None;
         self.joining = None;
@@ -416,8 +431,13 @@ impl App {
         let is_layout = matches!(m, ServerMsg::Layout(_));
         let reply = g.bot.receive(m);
         g.bot.take_notices();
+        let mut claim = None;
         if let (true, Some(l)) = (is_layout, g.bot.layout()) {
             g.names = Names::new(l);
+            // The creator's chosen area, once, if it is still a spectator's layout.
+            if let Some(a) = self.claim_on_join.take().filter(|a| l.area.is_none() && l.areas.contains(a)) {
+                claim = Some(ClientMsg::Claim { area: a });
+            }
         }
         if let (Some(sel), Some(l)) = (g.selected.as_deref(), g.bot.layout()) {
             if !crate::select::can_enter(l, sel) {
@@ -427,6 +447,9 @@ impl App {
         if let Some(r) = reply {
             self.send_game(r);
         }
+        if let Some(c) = claim {
+            self.send_game(c);
+        }
     }
 
     /// A layout or view while a join waits for its `joined`: the `joined`
@@ -482,11 +505,21 @@ impl App {
 
     /// `start` is "HH:MM" or "HH:MM:SS"; the front checks it.
     pub fn create_game(&mut self, layout: &str, seed: Option<u64>, start: Option<String>) {
+        self.claim_on_join = None;
         if self.send(ClientFrame::Lobby(LobbyMsg::CreateGame { layout: layout.to_string(), seed, start })) {
             self.joining = Some(Joining { game: None, rejoin: false });
         }
     }
 
+    /// `create_game`, then claim `area` as soon as the game's first layout
+    /// comes (polish spec H2); `None` watches, as `create_game` does.
+    pub fn create_game_in(&mut self, layout: &str, seed: Option<u64>, start: Option<String>, area: Option<&str>) {
+        self.create_game(layout, seed, start);
+        if self.joining.is_some() {
+            self.claim_on_join = area.map(str::to_string);
+        }
+    }
+
     /// Delete a saved or crashed game (owner decision 13). The front checks
     /// who may and answers with the new games list, or an error for the lobby.
     pub fn delete_game(&mut self, game: &str) {
@@ -494,6 +527,7 @@ impl App {
     }
 
     pub fn join(&mut self, game: &str) {
+        self.claim_on_join = None;
         if self.send(ClientFrame::Lobby(LobbyMsg::Join { game: game.to_string() })) {
             self.joining = Some(Joining { game: Some(game.to_string()), rejoin: false });
         }
@@ -501,6 +535,7 @@ impl App {
 
     /// Back to the lobby (the front answers with the games list).
     pub fn leave(&mut self) {
+        self.claim_on_join = None;
         self.send(ClientFrame::Lobby(LobbyMsg::Leave));
         self.game = None;
         self.rejoin = None;
diff --git a/crates/client-core/src/input.rs b/crates/client-core/src/input.rs
index b5c239e..9a8e01a 100644
--- a/crates/client-core/src/input.rs
+++ b/crates/client-core/src/input.rs
@@ -39,7 +39,13 @@ impl App {
                 g.selected = None;
                 self.command(cmd);
             }
-            Click::Ignore => {}
+            Click::Ignore => {
+                // Say why nothing happened (polish spec H2).
+                if let Target::Signal(s) = target {
+                    let why = select::why_not_entrance(l, s);
+                    g.log_once(why);
+                }
+            }
         }
     }
 
@@ -47,8 +53,18 @@ impl App {
     /// one; the selection is left as it is.
     fn toggle_auto(&mut self, signal: &str) {
         let cmd = self.game.as_ref().and_then(|g| select::auto_toggle(g.bot.layout()?, g.bot.view()?, signal));
-        if let Some(cmd) = cmd {
-            self.command(cmd);
+        match cmd {
+            Some(cmd) => self.command(cmd),
+            None => {
+                let Some(g) = self.game.as_mut() else { return };
+                let Some(l) = g.bot.layout() else { return };
+                let why = if l.signals.iter().any(|s| s.name == signal && s.operable) {
+                    format!("Auto-working {}: set a route from it first", g.names.signal(signal))
+                } else {
+                    select::why_not_entrance(l, signal)
+                };
+                g.log_once(why);
+            }
         }
     }
 
diff --git a/crates/client-core/src/select.rs b/crates/client-core/src/select.rs
index cc816f0..dd08877 100644
--- a/crates/client-core/src/select.rs
+++ b/crates/client-core/src/select.rs
@@ -65,6 +65,19 @@ pub fn click(l: &Layout, selected: Option<&str>, target: &ExitName) -> Click {
     }
 }
 
+/// Why a click on `signal` chose nothing (polish spec H2): you are
+/// watching, it is another area's, or no route of yours starts there.
+pub fn why_not_entrance(l: &Layout, signal: &str) -> String {
+    let names = Names::new(l);
+    let shown = names.signal(signal);
+    match (l.area.as_deref(), l.signals.iter().find(|s| s.name == signal)) {
+        (None, _) => "You are watching: claim an area to signal".to_string(),
+        (Some(mine), Some(s)) if s.area != mine => format!("{shown} is worked from {}, not your area", s.area),
+        (Some(_), None) => format!("{shown} is not in your area"),
+        _ => format!("No route of yours starts at {shown}"),
+    }
+}
+
 /// Routes from `entrance` that are set (not idle) in the view.
 fn active_from<'a>(l: &'a Layout, v: &'a View, entrance: &'a str) -> impl Iterator<Item = &'a RouteInfo> + 'a {
     l.routes.iter().filter(move |r| r.entrance == entrance && v.routes.contains_key(&r.name))
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index b5de06d..120f256 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -63,6 +63,8 @@ pub enum SideTab {
 #[derive(Default)]
 struct NewGame {
     layout: usize,
+    /// 0: watch; `i + 1`: claim the layout's area `i` (polish spec H2).
+    area: usize,
     seed: String,
     start: String,
 }
@@ -249,6 +251,7 @@ impl UiApp {
             ui.separator();
             ui.label(RichText::new("New game").strong());
             let layouts: Vec<String> = self.core.layouts().iter().map(|l| l.name.clone()).collect();
+            let areas: Vec<Vec<String>> = self.core.layouts().iter().map(|l| l.areas.clone()).collect();
             if layouts.is_empty() {
                 ui.label("No layouts yet.");
             } else {
@@ -259,6 +262,16 @@ impl UiApp {
                             ui.selectable_value(&mut self.new_game.layout, i, name.as_str());
                         }
                     });
+                    // Where the creator starts (polish spec H2): an area to signal, or watching.
+                    let mine = &areas[self.new_game.layout];
+                    self.new_game.area = self.new_game.area.min(mine.len());
+                    let shown = |i: usize| if i == 0 { "watch".to_string() } else { mine[i - 1].clone() };
+                    ui.label("Signal");
+                    egui::ComboBox::from_id_salt("new_game_area").selected_text(shown(self.new_game.area)).show_ui(ui, |ui| {
+                        for i in 0..=mine.len() {
+                            ui.selectable_value(&mut self.new_game.area, i, shown(i));
+                        }
+                    });
                     ui.label("Seed");
                     ui.add(egui::TextEdit::singleline(&mut self.new_game.seed).desired_width(90.0).hint_text("random"));
                     ui.label("Start");
@@ -266,7 +279,8 @@ impl UiApp {
                     if ui.button("Create").clicked() {
                         let seed = self.new_game.seed.trim().parse().ok();
                         let start = Some(self.new_game.start.trim().to_string()).filter(|s| !s.is_empty());
-                        self.core.create_game(&layouts[self.new_game.layout], seed, start);
+                        let area = self.new_game.area.checked_sub(1).map(|i| mine[i].clone());
+                        self.core.create_game_in(&layouts[self.new_game.layout], seed, start, area.as_deref());
                     }
                 });
             }
@@ -442,6 +456,10 @@ impl UiApp {
                 }
             });
             ui.horizontal_wrapped(|ui| {
+                // Polish spec H2: a spectator's clicks do nothing; say so where they look.
+                if !holding && !lesson {
+                    ui.label(RichText::new("You are watching. Claim an area to signal:").color(paint::YELLOW));
+                }
                 ui.label("Players:");
                 for area in &areas {
                     let holder = view.as_ref().and_then(|v| v.holders.get(area)).map_or("robot", String::as_str);
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Browser client": "A new game's creator may choose an area in the lobby; the client claims it when the
game's first layout comes (`App::create_game_in`, polish spec H2). A click that chooses nothing logs why, once."

- [ ] **Step 6: Commit**

```bash
git add crates/client-core crates/client-core/src/app.rs crates/client-core/src/input.rs crates/client-core/src/select.rs crates/client-ui crates/client-ui/src/screens.rs CLAUDE.md
git commit -m "feat(client): choose an area when creating a game; spectators and dead clicks are told why"
```

---

### Task 10: What is clickable says so (UI review M3)

Spec §10 M3 (U9). A pointing hand over what you can work, hover text ending with what a click does, and a left
click on your points opens their menu.

**Files:**
- Modify: `crates/client-core/src/input.rs`
- Modify: `crates/client-ui/src/screens.rs`
- Modify: `CLAUDE.md`
- Test: `crates/client-core/tests/input.rs`
- Test: `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: `Hit::clickable` (already computed), `select::{exits_from, can_enter, signal_menu, points_menu, operable_berth, auto_toggle}`.
- Produces: `App::hint(&Target) -> Option<&'static str>`; the diagram opens its menu with `egui::Popup::menu(..).open_memory(..)`.

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-core/tests/input.rs b/crates/client-core/tests/input.rs
index f085bb4..8457609 100644
--- a/crates/client-core/tests/input.rs
+++ b/crates/client-core/tests/input.rs
@@ -424,3 +424,21 @@ fn a_click_that_chooses_nothing_says_why_once() {
         [(s("C is not in your area"), false), (s("Auto-working TAW1: set a route from it first"), false)]
     );
 }
+
+/// Polish spec M3: hover text ends with what a click would do, and says
+/// nothing where clicks do nothing for you.
+#[test]
+fn hints_say_what_a_click_does() {
+    let mut t = Table::new("ann", Some("West"));
+    assert_eq!(t.app.hint(&sig("W1")), Some("click: choose as entrance"));
+    t.app.click(&sig("W1"));
+    assert_eq!(t.app.hint(&sig("A")), Some("click: set the route to here"));
+    assert_eq!(t.app.hint(&sig("W1")), Some("click again or Esc: forget the entrance"));
+    assert_eq!(t.app.hint(&Target::Berth(s("BA"))), Some("right-click: interpose or cancel a headcode"));
+    assert_eq!(t.app.hint(&Target::Section(s("TW1"))), None);
+    assert_eq!(t.app.hint(&Target::Points(s("P"))), None, "East's points");
+    let e = Table::new("eve", Some("East"));
+    assert_eq!(e.app.hint(&Target::Points(s("P"))), Some("click: swing them"));
+    let spec = Table::new("sam", None);
+    assert_eq!(spec.app.hint(&sig("W1")), None);
+}
diff --git a/crates/client-ui/tests/screens.rs b/crates/client-ui/tests/screens.rs
index 823b350..9deede6 100644
--- a/crates/client-ui/tests/screens.rs
+++ b/crates/client-ui/tests/screens.rs
@@ -752,3 +752,23 @@ fn a_spectator_is_told_to_claim_an_area() {
     let mut r = Rig::in_game(drawn_twobox(), Some("West"));
     assert!(!has_text(&r.frame(), "You are watching"));
 }
+
+/// Polish spec M3: what you can click shows a pointing hand and says what a
+/// click does; points you work open their menu on a left click too.
+#[test]
+fn clickable_things_say_so_and_points_open_on_a_left_click() {
+    let mut r = Rig::in_game(drawn_twobox(), Some("East"));
+    let p = r.at(207.5, 0.0);
+    r.events.push(Event::PointerMoved(p));
+    r.frame();
+    let out = r.frame();
+    assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::PointingHand);
+    r.click(p, PointerButton::Primary);
+    let out = r.frame();
+    assert!(has_text(&out, "Swing TBP reverse"), "{:?}", texts(&out));
+    // Track is hover only: no hand.
+    let mut r = Rig::in_game(drawn_twobox(), Some("East"));
+    r.events.push(Event::PointerMoved(r.at(150.0, 0.0)));
+    r.frame();
+    assert_eq!(r.frame().platform_output.cursor_icon, egui::CursorIcon::Default);
+}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-core --test input hints -p signalbox-client-ui --test screens clickable`
Expected: compile error (`hint` not found).

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-core/src/input.rs b/crates/client-core/src/input.rs
index 9a8e01a..9ed0f23 100644
--- a/crates/client-core/src/input.rs
+++ b/crates/client-core/src/input.rs
@@ -118,6 +118,27 @@ impl App {
         self.game.as_ref()?.bot.view()?.berths.get(b).cloned()
     }
 
+    /// What a click on `target` would do, for the end of its hover text
+    /// (polish spec M3); `None` where clicks do nothing for you.
+    pub fn hint(&self, target: &Target) -> Option<&'static str> {
+        let g = self.game.as_ref()?;
+        let (l, v) = (g.bot.layout()?, g.bot.view()?);
+        let exit = |e: ExitName| g.selected.as_deref().is_some_and(|s| select::exits_from(l, s).contains(&e));
+        match target {
+            Target::Signal(s) if exit(ExitName::Signal(s.clone())) => Some("click: set the route to here"),
+            Target::Signal(s) if g.selected.as_deref() == Some(s.as_str()) => Some("click again or Esc: forget the entrance"),
+            Target::Signal(s) if select::can_enter(l, s) && !select::signal_menu(l, v, s).is_empty() => {
+                Some("click: choose as entrance · right-click: cancel the route")
+            }
+            Target::Signal(s) if select::can_enter(l, s) => Some("click: choose as entrance"),
+            Target::Exit(n) if exit(ExitName::Node(n.clone())) => Some("click: set the route to here"),
+            Target::Points(p) if !select::points_menu(l, v, p).is_empty() => Some("click: swing them"),
+            Target::Berth(b) if select::operable_berth(l, b) => Some("right-click: interpose or cancel a headcode"),
+            Target::Auto(s) if select::auto_toggle(l, v, s).is_some() => Some("click: auto-working on or off"),
+            _ => None,
+        }
+    }
+
     /// Hover text.
     pub fn describe(&self, target: &Target) -> String {
         let Some(g) = self.game.as_ref() else { return String::new() };
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index 120f256..9820825 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -709,10 +709,17 @@ impl UiApp {
         // Every click goes on, even one on nothing or on what is not yours:
         // a dead click clears the entrance (`App::click` decides what the
         // rest mean, from the same operability `Hit::clickable` shows).
-        let click = resp.clicked().then(|| hit_at(resp.interact_pointer_pos()).map(|h| h.target));
-        if resp.secondary_clicked() {
+        let click = resp.clicked().then(|| hit_at(resp.interact_pointer_pos()));
+        // Points you can work open their menu on a left click too (polish spec M3).
+        let points_menu = matches!(&click, Some(Some(h)) if h.clickable && matches!(h.target, Target::Points(_)));
+        let click = if points_menu { None } else { click.map(|h| h.map(|h| h.target)) };
+        if resp.secondary_clicked() || points_menu {
             self.menu_target = hit_at(resp.interact_pointer_pos()).map(|h| h.target);
         }
+        // What can be clicked shows a pointing hand (polish spec M3).
+        if hover.as_ref().is_some_and(|h| h.clickable) {
+            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
+        }
         let exits = self.core.valid_exits();
         let highlight = self.highlights();
         let Some(g) = self.core.game() else { return };
@@ -754,10 +761,23 @@ impl UiApp {
             self.menu_target = None;
         }
         let resp = match &hover {
-            Some(h) => resp.on_hover_text_at_pointer(self.core.describe(&h.target)),
+            Some(h) => {
+                let text = match self.core.hint(&h.target) {
+                    Some(hint) => format!("{}\n{hint}", self.core.describe(&h.target)),
+                    None => self.core.describe(&h.target),
+                };
+                resp.on_hover_text_at_pointer(text)
+            }
             None => resp,
         };
-        resp.context_menu(|ui| self.menu_ui(ui));
+        let open = if resp.secondary_clicked() || points_menu {
+            Some(egui::SetOpenCommand::Bool(true))
+        } else if resp.clicked() {
+            Some(egui::SetOpenCommand::Bool(false))
+        } else {
+            None
+        };
+        egui::Popup::menu(&resp).open_memory(open).at_pointer_fixed().show(|ui| self.menu_ui(ui));
     }
 
     fn menu_ui(&mut self, ui: &mut Ui) {
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Browser client", after the `hit_test` bullet: "The diagram shows a pointing hand over what you can
work and ends hover text with what a click does (`App::hint`); a left click on your points opens their menu (polish spec M3)."

- [ ] **Step 6: Commit**

```bash
git add crates/client-core crates/client-core/src/input.rs crates/client-ui crates/client-ui/src/screens.rs CLAUDE.md
git commit -m "feat(client): a hand cursor and click hints; points open their menu on a left click"
```

---

### Task 11: A refusal names the route in the way (UI review M4)

Spec §10 M4 (U10). The game finds the route that blocked a refused command; the alarm names it and its entrance
is outlined with yours.

**Files:**
- Modify: `crates/client-core/src/app.rs`
- Modify: `crates/client-core/src/names.rs`
- Modify: `crates/client-core/src/text.rs`
- Modify: `crates/client-ui/src/paint.rs`
- Modify: `crates/client-ui/src/screens.rs`
- Modify: `crates/game/src/game.rs`
- Modify: `crates/game/src/names.rs`
- Modify: `crates/protocol/src/msg.rs`
- Modify: `CLAUDE.md`
- Test: `crates/client-core/tests/input.rs`
- Test: `crates/client-core/tests/text.rs`
- Test: `crates/client-ui/tests/labels.rs`
- Test: `crates/client-ui/tests/layouts.rs`
- Test: `crates/client-ui/tests/legibility.rs`
- Test: `crates/client-ui/tests/paint.rs`
- Test: `crates/game/tests/game.rs`
- Test: `crates/game/tests/save.rs`
- Test: `crates/protocol/tests/golden.rs`

**Interfaces:**
- Consumes: `Interlocking::{owner, active_route_from}`, `RouteDef::all_points`, `Network::points_section`.
- Produces: `protocol::Notice::Rejected { cmd, reason, by: Option<String> }` (`by` omitted when none);
  `game::names::blocker(&World, &Interlocking, &Command, Rejection) -> Option<String>`; `Names::{route(&str) -> String,
  route_entrance(&str) -> Option<&str>}`; `InGame::blocking() -> Option<&str>`; `PaintState.blocking: Option<&str>`
  (every `PaintState { … }` literal, including those Tasks 1–3 added in `tests/labels.rs` and `tests/legibility.rs`,
  gains `blocking: None`).

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-core/tests/input.rs b/crates/client-core/tests/input.rs
index 8457609..456d709 100644
--- a/crates/client-core/tests/input.rs
+++ b/crates/client-core/tests/input.rs
@@ -130,12 +130,14 @@ fn a_refused_command_outlines_its_entrance_and_raises_an_alarm() {
     t.run(0.2);
     let g = t.app.game().unwrap();
     assert_eq!(g.refused(), Some("D"));
+    // Polish spec M4: the route in the way is named and outlined too.
+    assert_eq!(g.blocking(), Some("C"));
     assert_eq!(
         t.log_lines().last().unwrap(),
-        &(s("Refused: set route TBD to TAW2 (conflicts with a route already set)"), true)
+        &(s("Refused: set route TBD to TAW2 (conflicts with a route already set: TBC to TAW2)"), true)
     );
     t.run(REFUSED_S);
-    assert_eq!(t.app.game().unwrap().refused(), None);
+    assert_eq!((t.app.game().unwrap().refused(), t.app.game().unwrap().blocking()), (None, None));
 }
 
 #[test]
diff --git a/crates/client-core/tests/text.rs b/crates/client-core/tests/text.rs
index 51919f3..2f2df20 100644
--- a/crates/client-core/tests/text.rs
+++ b/crates/client-core/tests/text.rs
@@ -24,7 +24,7 @@ fn commands_and_refusals() {
     let plain = Names::default();
     assert_eq!(command_text(&c, &plain), "set route 39,1V1 to N12");
     assert_eq!(
-        notice_text(&Notice::Rejected { cmd: c, reason: Rejection::PointsLocked }, &plain),
+        notice_text(&Notice::Rejected { cmd: c, reason: Rejection::PointsLocked, by: None }, &plain),
         (s("Refused: set route 39,1V1 to N12 (points locked)"), true)
     );
     assert_eq!(
diff --git a/crates/client-ui/tests/labels.rs b/crates/client-ui/tests/labels.rs
index ec88973..ea2707d 100644
--- a/crates/client-ui/tests/labels.rs
+++ b/crates/client-ui/tests/labels.rs
@@ -184,7 +184,7 @@ fn a_plan_depends_on_neither_pan_nor_trains() {
     empty.berths.clear();
     busy.berths.insert("BW1".into(), "1A01".into());
     let plan_at = |cam: &Camera, v: &protocol::View| {
-        let st = PaintState { view: Some(v), selected: None, exits: &[], refused: None, time: 0.0, aspects: AspectMode::RedGreen, numbers: true, names: &names, highlight: &[] };
+        let st = PaintState { view: Some(v), selected: None, exits: &[], refused: None, blocking: None, time: 0.0, aspects: AspectMode::RedGreen, numbers: true, names: &names, highlight: &[] };
         plan(&draw(&sc, cam, screen, &st), &mut measure)
     };
     let cam = Camera::fit(sc.all.unwrap(), screen);
diff --git a/crates/client-ui/tests/layouts.rs b/crates/client-ui/tests/layouts.rs
index 8c9fd5d..8dd5b75 100644
--- a/crates/client-ui/tests/layouts.rs
+++ b/crates/client-ui/tests/layouts.rs
@@ -57,7 +57,7 @@ fn every_shipped_layout_draws_for_every_box() {
             let cam = Camera::fit(sc.fit_bounds().unwrap(), screen);
             let names = Names::new(&l);
             for (time, aspects) in [(0.0, AspectMode::RedGreen), (0.3, AspectMode::Real)] {
-                let st = PaintState { view: Some(&v), selected: None, exits: &[], refused: None, time, aspects, numbers: true, names: &names, highlight: &[] };
+                let st = PaintState { view: Some(&v), selected: None, exits: &[], refused: None, blocking: None, time, aspects, numbers: true, names: &names, highlight: &[] };
                 let d = draw(&sc, &cam, screen, &st);
                 assert!(d.shapes.iter().all(finite), "{name} {area:?}");
                 assert!(d.texts.iter().all(|t| t.at.is_finite() && t.size.is_finite()), "{name} {area:?}: texts");
@@ -113,7 +113,7 @@ fn every_lesson_draws_with_its_highlights() {
             let names = Names::new(&l);
             for step in &steps {
                 let diagram: Vec<_> = step.highlight.iter().filter(|h| !matches!(h, protocol::Highlight::Ui(u) if !u.starts_with("auto:"))).cloned().collect();
-                let st = |highlight| PaintState { view: Some(&v), selected: None, exits: &[], refused: None, time: 0.25, aspects: AspectMode::Real, numbers: true, names: &names, highlight };
+                let st = |highlight| PaintState { view: Some(&v), selected: None, exits: &[], refused: None, blocking: None, time: 0.25, aspects: AspectMode::Real, numbers: true, names: &names, highlight };
                 let dr = draw(&sc, &cam, screen, &st(&diagram));
                 assert!(dr.shapes.iter().all(finite), "{}", d.display());
                 if who == "pat" {
diff --git a/crates/client-ui/tests/legibility.rs b/crates/client-ui/tests/legibility.rs
index 71f8f27..0f2e321 100644
--- a/crates/client-ui/tests/legibility.rs
+++ b/crates/client-ui/tests/legibility.rs
@@ -87,6 +87,7 @@ fn every_view_is_legible_at_every_zoom() {
                         selected: None,
                         exits: &[],
                         refused: None,
+                        blocking: None,
                         time: 0.0,
                         aspects: AspectMode::RedGreen,
                         numbers: true,
diff --git a/crates/client-ui/tests/paint.rs b/crates/client-ui/tests/paint.rs
index 8a8753d..e044924 100644
--- a/crates/client-ui/tests/paint.rs
+++ b/crates/client-ui/tests/paint.rs
@@ -79,6 +79,7 @@ impl Rig {
             selected,
             exits,
             refused,
+            blocking: None,
             time,
             aspects: self.aspects,
             numbers: self.numbers,
@@ -372,6 +373,7 @@ fn no_view_yet_draws_everything_idle() {
         selected: None,
         exits: &[],
         refused: None,
+        blocking: None,
         time: 0.0,
         aspects: AspectMode::RedGreen,
         numbers: true,
@@ -616,7 +618,7 @@ fn absurdly_long_runs_have_a_bounded_number_of_arrows() {
     }];
     sc.tracks.clear();
     let names = Names::new(&r.layout);
-    let st = PaintState { view: None, selected: None, exits: &[], refused: None, time: 0.0, aspects: AspectMode::RedGreen, numbers: true, names: &names, highlight: &[] };
+    let st = PaintState { view: None, selected: None, exits: &[], refused: None, blocking: None, time: 0.0, aspects: AspectMode::RedGreen, numbers: true, names: &names, highlight: &[] };
     let cam = Camera { centre: pos2(0.0, 0.0), scale: client_ui::camera::MAX_SCALE };
     let d = draw(&sc, &cam, screen(), &st);
     let arrows: Vec<Pos2> = d
@@ -644,7 +646,7 @@ fn a_backward_runs_arrow_is_on_the_right_of_its_travel() {
         loose_end: false,
     }];
     let names = Names::new(&r.layout);
-    let st = PaintState { view: None, selected: None, exits: &[], refused: None, time: 0.0, aspects: AspectMode::RedGreen, numbers: true, names: &names, highlight: &[] };
+    let st = PaintState { view: None, selected: None, exits: &[], refused: None, blocking: None, time: 0.0, aspects: AspectMode::RedGreen, numbers: true, names: &names, highlight: &[] };
     let d = draw(&sc, &r.cam, screen(), &st);
     let tips: Vec<Pos2> = d
         .shapes
@@ -701,6 +703,7 @@ fn a_lesson_highlight_outlines_what_it_names_and_pulses() {
             selected: None,
             exits: &[],
             refused: None,
+            blocking: None,
             time,
             aspects: AspectMode::RedGreen,
             numbers: true,
diff --git a/crates/game/tests/game.rs b/crates/game/tests/game.rs
index 3e56c3a..28689d9 100644
--- a/crates/game/tests/game.rs
+++ b/crates/game/tests/game.rs
@@ -149,10 +149,10 @@ fn unknown_names_and_non_points_are_rejected_before_the_sim() {
     join(&mut g, "alice", Some("East"));
     let bad = PlayerCommand::CancelRoute { entrance: s("Z9") };
     let out = command(&mut g, "alice", bad.clone());
-    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: bad, reason: Rejection::UnknownId }]);
+    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: bad, reason: Rejection::UnknownId, by: None }]);
     let joint = PlayerCommand::SwingPoints { points: s("J2"), to: PointsPos::Reverse };
     let out = command(&mut g, "alice", joint.clone());
-    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: joint, reason: Rejection::NotPoints }]);
+    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: joint, reason: Rejection::NotPoints, by: None }]);
     g.advance(0.1);
     assert!(g.sim().log().is_empty());
 }
@@ -165,7 +165,7 @@ fn sim_rejections_go_back_to_the_sender() {
     let cancel = PlayerCommand::CancelRoute { entrance: s("W1") };
     assert!(command(&mut g, "alice", cancel.clone()).is_empty());
     let out = g.advance(0.1);
-    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: cancel, reason: Rejection::RouteNotSet }]);
+    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: cancel, reason: Rejection::RouteNotSet, by: None }]);
     assert!(notices(&out, "bob").is_empty());
     assert_eq!(g.stats().sim_rejections, 1);
 }
@@ -538,3 +538,21 @@ fn a_demonstration_acts_in_any_area_and_tells_nobody() {
     assert!(g.sim().interlocking().active_route_from(w, w.net.signal("C").unwrap()).is_some(), "East's route, while alice holds West");
     assert_eq!(g.stats().sim_rejections, 1);
 }
+
+/// Polish spec M4: a refusal names the route in the way: one holding the
+/// track a route needs, or the points being swung.
+#[test]
+fn a_refusal_names_the_route_in_the_way() {
+    let mut g = game();
+    join(&mut g, "bob", Some("East"));
+    assert!(command(&mut g, "bob", set_route("C", ExitName::Signal(s("W2")))).is_empty());
+    g.advance(0.1);
+    let d = set_route("D", ExitName::Signal(s("W2")));
+    command(&mut g, "bob", d.clone());
+    let out = g.advance(0.1);
+    assert_eq!(notices(&out, "bob"), vec![Notice::Rejected { cmd: d, reason: Rejection::ConflictingRoute, by: Some(s("C-W2")) }]);
+    let swing = PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse };
+    command(&mut g, "bob", swing.clone());
+    let out = g.advance(0.1);
+    assert_eq!(notices(&out, "bob"), vec![Notice::Rejected { cmd: swing, reason: Rejection::PointsLocked, by: Some(s("C-W2")) }]);
+}
diff --git a/crates/game/tests/save.rs b/crates/game/tests/save.rs
index 809ce53..99fdab0 100644
--- a/crates/game/tests/save.rs
+++ b/crates/game/tests/save.rs
@@ -327,7 +327,7 @@ fn rejections_of_commands_queued_before_a_resume_reach_their_sender() {
     let mut resumed = Game::resume(&path).unwrap();
     rejoin(&mut resumed);
     let out = resumed.advance(0.1);
-    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: cancel, reason: Rejection::RouteNotSet }]);
+    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: cancel, reason: Rejection::RouteNotSet, by: None }]);
     assert_eq!(resumed.stats().sim_rejections, 1);
 }
 
diff --git a/crates/protocol/tests/golden.rs b/crates/protocol/tests/golden.rs
index b100d49..05ee0d3 100644
--- a/crates/protocol/tests/golden.rs
+++ b/crates/protocol/tests/golden.rs
@@ -69,9 +69,17 @@ fn player_commands() {
 fn notices() {
     let cases = vec![
         (
-            Notice::Rejected { cmd: PlayerCommand::CancelRoute { entrance: s("A") }, reason: Rejection::RouteNotSet },
+            Notice::Rejected { cmd: PlayerCommand::CancelRoute { entrance: s("A") }, reason: Rejection::RouteNotSet, by: None },
             json!({"kind": "rejected", "cmd": {"cmd": "cancel_route", "entrance": "A"}, "reason": "route_not_set"}),
         ),
+        (
+            Notice::Rejected {
+                cmd: PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse },
+                reason: Rejection::PointsLocked,
+                by: Some(s("A-E")),
+            },
+            json!({"kind": "rejected", "cmd": {"cmd": "swing_points", "points": "P", "to": "reverse"}, "reason": "points_locked", "by": "A-E"}),
+        ),
         (Notice::NotYourArea { area: s("East") }, json!({"kind": "not_your_area", "area": "East"})),
         (Notice::Spad { signal: s("A"), train: s("1A01") }, json!({"kind": "spad", "signal": "A", "train": "1A01"})),
         (Notice::Collision { section: s("TP") }, json!({"kind": "collision", "section": "TP"})),
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-game --test game a_refusal -p signalbox-client-core --test input a_refused`
Expected: compile errors (no field `by`, `blocking` not found).

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-core/src/app.rs b/crates/client-core/src/app.rs
index 6183d5e..1f87672 100644
--- a/crates/client-core/src/app.rs
+++ b/crates/client-core/src/app.rs
@@ -58,6 +58,8 @@ pub struct InGame {
     pub(crate) layout_gen: u64,
     pub(crate) selected: Option<String>,
     pub(crate) refused: Option<(String, f64)>,
+    /// The entrance of the route in the way of that command (polish spec M4).
+    pub(crate) blocking: Option<String>,
     pub(crate) log: Log,
     /// Display names for the layout held (rebuilt with every layout).
     pub(crate) names: Names,
@@ -76,6 +78,7 @@ impl InGame {
             layout_gen: 0,
             selected: None,
             refused: None,
+            blocking: None,
             log: Log::default(),
             names: Names::default(),
             lesson: None,
@@ -112,6 +115,12 @@ impl InGame {
         self.selected.as_deref()
     }
 
+    /// The entrance of the route that was in the way of the command just
+    /// refused, outlined with it (polish spec M4).
+    pub fn blocking(&self) -> Option<&str> {
+        self.refused.as_ref().and(self.blocking.as_deref())
+    }
+
     /// The entrance of a command just refused, outlined for `REFUSED_S`.
     pub fn refused(&self) -> Option<&str> {
         self.refused.as_ref().map(|(s, _)| s.as_str())
@@ -414,9 +423,12 @@ impl App {
                 let (text, alarm) = notice_text(n, &g.names);
                 let t = g.sim_time();
                 g.log.push(t, text, alarm);
-                if let Notice::Rejected { cmd, .. } = n {
-                    if let Some(e) = entrance_of(cmd) {
-                        g.refused = Some((e.to_string(), self.now + REFUSED_S));
+                if let Notice::Rejected { cmd, by, .. } = n {
+                    g.blocking = by.as_deref().and_then(|r| g.names.route_entrance(r)).map(str::to_string);
+                    match entrance_of(cmd) {
+                        Some(e) => g.refused = Some((e.to_string(), self.now + REFUSED_S)),
+                        // Points have no entrance: outline the blocking route alone.
+                        None => g.refused = g.blocking.clone().map(|b| (b, self.now + REFUSED_S)),
                     }
                 }
             }
diff --git a/crates/client-core/src/names.rs b/crates/client-core/src/names.rs
index d9fd858..ef22fca 100644
--- a/crates/client-core/src/names.rs
+++ b/crates/client-core/src/names.rs
@@ -28,6 +28,8 @@ pub struct Names {
     platforms: BTreeMap<String, Vec<String>>,
     /// Place code → name.
     places: BTreeMap<String, String>,
+    /// Route name → (entrance, `LA31 to LA29`).
+    routes: BTreeMap<String, (String, String)>,
 }
 
 /// A points name's number for display: the digits after a leading `N` or
@@ -57,7 +59,9 @@ impl Names {
                 }
             }
         }
-        Names { signals, workstations, headcodes: l.headcodes.clone(), points, berths, platforms, places: l.places.clone() }
+        let mut n = Names { signals, workstations, headcodes: l.headcodes.clone(), points, berths, platforms, places: l.places.clone(), routes: BTreeMap::new() };
+        n.routes = l.routes.iter().map(|r| (r.name.clone(), (r.entrance.clone(), format!("{} to {}", n.signal(&r.entrance), n.exit(&r.exit))))).collect();
+        n
     }
 
     /// How a signal is shown; a name the layout does not list stays plain.
@@ -97,6 +101,16 @@ impl Names {
         }
     }
 
+    /// A route as `LA31 to LA29`; a route the layout does not list is `another route`.
+    pub fn route(&self, name: &str) -> String {
+        self.routes.get(name).map_or_else(|| "another route".to_string(), |r| r.1.clone())
+    }
+
+    /// The entrance signal of a route the layout lists.
+    pub fn route_entrance(&self, name: &str) -> Option<&str> {
+        self.routes.get(name).map(|r| r.0.as_str())
+    }
+
     /// A place's name (`LIVERPOOL STREET`), else its code.
     pub fn place<'a>(&'a self, code: &'a str) -> &'a str {
         self.places.get(code).map_or(code, String::as_str)
diff --git a/crates/client-core/src/text.rs b/crates/client-core/src/text.rs
index 1ed9baf..5f3e617 100644
--- a/crates/client-core/src/text.rs
+++ b/crates/client-core/src/text.rs
@@ -56,8 +56,9 @@ pub fn rejection_text(r: Rejection) -> &'static str {
 /// A notice as one log line, and whether it is an alarm.
 pub fn notice_text(n: &Notice, names: &Names) -> (String, bool) {
     match n {
-        Notice::Rejected { cmd, reason } => {
-            (format!("Refused: {} ({})", command_text(cmd, names), rejection_text(*reason)), true)
+        Notice::Rejected { cmd, reason, by } => {
+            let by = by.as_ref().map(|r| format!(": {}", names.route(r))).unwrap_or_default();
+            (format!("Refused: {} ({}{by})", command_text(cmd, names), rejection_text(*reason)), true)
         }
         Notice::NotYourArea { area } => (format!("Not your area: that is in {area}"), true),
         Notice::Spad { signal, train } => {
diff --git a/crates/client-ui/src/paint.rs b/crates/client-ui/src/paint.rs
index 21060bf..007ea33 100644
--- a/crates/client-ui/src/paint.rs
+++ b/crates/client-ui/src/paint.rs
@@ -181,6 +181,8 @@ pub struct PaintState<'a> {
     pub exits: &'a [ExitName],
     /// The signal outlined for a refused command.
     pub refused: Option<&'a str>,
+    /// The entrance of the route in its way, outlined the same (polish spec M4).
+    pub blocking: Option<&'a str>,
     /// Seconds, for flashing.
     pub time: f64,
     pub aspects: AspectMode,
@@ -417,7 +419,7 @@ fn signal_shapes(d: &mut Drawing, s: &SignalMark, cam: &Camera, screen: Rect, st
     if st.exits.contains(&ExitName::Signal(s.name.clone())) {
         d.shapes.push(Shape::circle_stroke(disc, LAMP_R + 3.5, Stroke::new(2.0, SELECT)));
     }
-    if st.refused == Some(s.name.as_str()) {
+    if st.refused == Some(s.name.as_str()) || st.blocking == Some(s.name.as_str()) {
         d.shapes.push(Shape::circle_stroke(disc, LAMP_R + 6.0, Stroke::new(2.0, REFUSED)));
     }
     d.keep.rounds.push((disc, LAMP_R));
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index 9820825..249a042 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -728,6 +728,7 @@ impl UiApp {
             selected: g.selected(),
             exits: &exits,
             refused: g.refused(),
+            blocking: g.blocking(),
             time: now,
             aspects: self.settings.aspects,
             numbers: self.settings.numbers,
diff --git a/crates/game/src/game.rs b/crates/game/src/game.rs
index b245082..3381ed2 100644
--- a/crates/game/src/game.rs
+++ b/crates/game/src/game.rs
@@ -18,7 +18,7 @@ use crate::clock::{GameClock, VoteError};
 use crate::display::Display;
 use crate::geometry::WorldGeometry;
 use crate::layout::build_layout;
-use crate::names::{resolve, to_player_command, valid_headcode};
+use crate::names::{blocker, resolve, to_player_command, valid_headcode};
 use crate::notices::area_notices;
 use crate::save::{Logged, SaveDb, SaveError, resume_sim};
 use crate::view::{Shared, build_view};
@@ -601,7 +601,7 @@ impl Game {
     }
 
     fn command(&mut self, player: &str, cmd: PlayerCommand) -> Vec<Out> {
-        let reject = |reason| vec![notice(player, Notice::Rejected { cmd: cmd.clone(), reason })];
+        let reject = |reason| vec![notice(player, Notice::Rejected { cmd: cmd.clone(), reason, by: None })];
         let Some(core) = resolve(self.sim.world(), &cmd) else { return reject(Rejection::UnknownId) };
         if let PlayerCommand::Interpose { headcode, .. } = &cmd {
             if !valid_headcode(headcode) {
@@ -698,7 +698,8 @@ impl Game {
                         let who = &queued[i].0;
                         if self.players.get(who).is_some_and(|p| p.connected) {
                             let named = to_player_command(self.sim.world(), cmd);
-                            out.push(notice(who, Notice::Rejected { cmd: named, reason: *reason }));
+                            let by = blocker(self.sim.world(), self.sim.interlocking(), cmd, *reason);
+                            out.push(notice(who, Notice::Rejected { cmd: named, reason: *reason, by }));
                         }
                     }
                 }
diff --git a/crates/game/src/names.rs b/crates/game/src/names.rs
index 33a8366..f515ffa 100644
--- a/crates/game/src/names.rs
+++ b/crates/game/src/names.rs
@@ -1,7 +1,8 @@
 //! Player commands carry names; the sim wants ids.
 
 use protocol::{ExitName, PlayerCommand};
-use signalbox_core::events::Command;
+use signalbox_core::events::{Command, Rejection};
+use signalbox_core::interlocking::{Interlocking, Owner};
 use signalbox_core::routes::Exit;
 use signalbox_core::world::World;
 
@@ -49,6 +50,42 @@ pub fn to_player_command(w: &World, cmd: &Command) -> PlayerCommand {
     }
 }
 
+/// The route in the way of a refused command (polish spec M4), by name: for
+/// a route, the route already set from its entrance or the first route
+/// holding track or points it needs (the owners `set_route` would let it
+/// share are skipped); for points, the route holding them. `None` when the
+/// reason is another, or the route has gone by the time this is asked.
+pub fn blocker(w: &World, il: &Interlocking, cmd: &Command, reason: Rejection) -> Option<String> {
+    let net = &w.net;
+    let x = match (cmd, reason) {
+        (Command::SetRoute { entrance, exit }, Rejection::ConflictingRoute | Rejection::PointsLocked) => {
+            let r = *w.routes_from[entrance.idx()].iter().find(|r| w.routes[r.idx()].exit == *exit)?;
+            let def = &w.routes[r.idx()];
+            let held = def.path.iter().chain(def.overlap.iter()).find_map(|&s| match il.owner[s.idx()] {
+                Some(Owner::Overlap(x)) if w.routes[x.idx()].exit == Exit::Signal(def.entrance) => None,
+                Some(Owner::Path(x)) if def.overlap.contains(&s) && def.exit == Exit::Signal(w.routes[x.idx()].entrance) => None,
+                Some(o) if o.route() != r => Some(o.route()),
+                _ => None,
+            });
+            let points = || {
+                def.all_points().find_map(|&(p, _)| {
+                    let sec = net.points_section(p)?;
+                    il.owner[sec.idx()].map(|o| o.route()).filter(|&o| o != r)
+                })
+            };
+            match reason {
+                Rejection::ConflictingRoute => il.active_route_from(w, *entrance).or(held),
+                _ => points(),
+            }
+        }
+        (Command::SwingPoints { points, .. }, Rejection::PointsLocked) => {
+            il.owner[net.points_section(*points)?.idx()].map(|o| o.route())
+        }
+        _ => None,
+    }?;
+    Some(w.routes[x.idx()].name.clone())
+}
+
 /// 1 to 10 ASCII letters or digits.
 pub fn valid_headcode(h: &str) -> bool {
     (1..=10).contains(&h.len()) && h.bytes().all(|b| b.is_ascii_alphanumeric())
diff --git a/crates/protocol/src/msg.rs b/crates/protocol/src/msg.rs
index adba1ce..ec41438 100644
--- a/crates/protocol/src/msg.rs
+++ b/crates/protocol/src/msg.rs
@@ -81,7 +81,13 @@ pub enum ServerMsg {
 #[serde(tag = "kind", rename_all = "snake_case")]
 pub enum Notice {
     /// The command was refused (unknown name, or by the interlocking).
-    Rejected { cmd: PlayerCommand, reason: Rejection },
+    /// `by`: the route in the way, when the interlocking can say (polish spec M4).
+    Rejected {
+        cmd: PlayerCommand,
+        reason: Rejection,
+        #[serde(default, skip_serializing_if = "Option::is_none")]
+        by: Option<String>,
+    },
     /// The command's subject lies in `area`, which is not yours.
     NotYourArea { area: String },
     Spad { signal: String, train: String },
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-protocol -p signalbox-game -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Multiplayer", after the `Game` bullet: "A refusal names the route in its way when the interlocking
can say (`Notice::Rejected.by`, `game::names::blocker`, polish spec M4); the client outlines that route's entrance too."

- [ ] **Step 6: Commit**

```bash
git add crates/client-core crates/client-core/src/app.rs crates/client-core/src/names.rs crates/client-core/src/text.rs crates/client-ui crates/client-ui/src/paint.rs crates/client-ui/src/screens.rs crates/game crates/game/src/game.rs crates/game/src/names.rs crates/protocol crates/protocol/src/msg.rs CLAUDE.md
git commit -m "feat: a refused command names the route in its way, and the client outlines it"
```

---

### Task 12: Votes: who is waited for, Agree and Decline, and how they end (UI review M8)

Spec §10 M8 (U14). The vote view lists who has still to agree; Decline ends the proposal; every player hears
the outcome; a paused clock says how to restart it.

**Files:**
- Modify: `crates/client-core/src/app.rs`
- Modify: `crates/client-core/src/text.rs`
- Modify: `crates/client-ui/src/screens.rs`
- Modify: `crates/game/src/clock.rs`
- Modify: `crates/game/src/game.rs`
- Modify: `crates/protocol/src/lib.rs`
- Modify: `crates/protocol/src/lobby.rs`
- Modify: `crates/protocol/src/msg.rs`
- Modify: `crates/protocol/src/view.rs`
- Modify: `CLAUDE.md`
- Test: `crates/client-core/tests/text.rs`
- Test: `crates/client-ui/tests/screens.rs`
- Test: `crates/game/tests/clock.rs`
- Test: `crates/game/tests/game.rs`
- Test: `crates/protocol/tests/diff.rs`
- Test: `crates/protocol/tests/golden.rs`

**Interfaces:**
- Consumes: `GameClock::{vote, settle, lapse}`, `Game::voters`.
- Produces: `VoteView.waiting: Vec<String>`; `ClientMsg::VoteDecline` (`"vote_decline"` in `CLIENT_MSG_TYPES`);
  `Notice::VoteEnded { proposal, outcome: VoteOutcome }`, `protocol::VoteOutcome { Passed, Declined { by }, Lapsed }`;
  `GameClock::{lapse -> Option<Proposal>, decline, vote_view(&BTreeSet<String>)}`; `Game.vote_ended` (drained by
  `flush`); `App::decline_vote()`; `text::vote_text` says "waiting for …".

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-core/tests/text.rs b/crates/client-core/tests/text.rs
index 2f2df20..17da4ad 100644
--- a/crates/client-core/tests/text.rs
+++ b/crates/client-core/tests/text.rs
@@ -40,7 +40,14 @@ fn commands_and_refusals() {
 
 #[test]
 fn votes() {
-    let v = VoteView { proposal: Proposal::Speed { x: 4 }, agreed: vec![s("ann"), s("bob")], expires_in_s: 25 };
+    let mut v = VoteView { proposal: Proposal::Speed { x: 4 }, agreed: vec![s("ann"), s("bob")], waiting: vec![], expires_in_s: 25 };
     assert_eq!(vote_text(&v), "Vote: 4× — ann, bob agreed, 25 s left");
+    v.waiting = vec![s("cat")];
+    assert_eq!(vote_text(&v), "Vote: 4× — waiting for cat, 25 s left", "polish spec M8");
+    let plain = Names::default();
+    let ended = |outcome| notice_text(&Notice::VoteEnded { proposal: Proposal::Pause, outcome }, &plain).0;
+    assert_eq!(ended(VoteOutcome::Passed), "Vote passed: pause");
+    assert_eq!(ended(VoteOutcome::Declined { by: s("bob") }), "Vote declined by bob: pause");
+    assert_eq!(ended(VoteOutcome::Lapsed), "Vote lapsed: pause");
     assert_eq!(proposal_text(Proposal::Pause), "pause");
 }
diff --git a/crates/client-ui/tests/screens.rs b/crates/client-ui/tests/screens.rs
index 9deede6..9849cea 100644
--- a/crates/client-ui/tests/screens.rs
+++ b/crates/client-ui/tests/screens.rs
@@ -772,3 +772,25 @@ fn clickable_things_say_so_and_points_open_on_a_left_click() {
     r.frame();
     assert_eq!(r.frame().platform_output.cursor_icon, egui::CursorIcon::Default);
 }
+
+/// Polish spec M8: a vote shows who it waits for, with Agree and Decline
+/// for those who have not agreed, and its end is logged.
+#[test]
+fn a_vote_waits_for_named_players_who_agree_or_decline() {
+    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
+    r.game.connect("bob");
+    r.game.handle("bob", ClientMsg::Claim { area: s("East") });
+    r.game.handle("bob", ClientMsg::Vote { proposal: Proposal::Speed { x: 2 } });
+    for _ in 0..3 {
+        r.frame();
+    }
+    let out = r.frame();
+    assert!(has_text(&out, "waiting for ann"), "{:?}", texts(&out));
+    click_text(&mut r, &out, "Decline");
+    for _ in 0..3 {
+        r.frame();
+    }
+    let out = r.frame();
+    assert!(has_text(&out, "Vote declined by ann: 2×"), "{:?}", texts(&out));
+    assert!(!has_text(&out, "Agree"));
+}
diff --git a/crates/game/tests/clock.rs b/crates/game/tests/clock.rs
index 28f367e..9674fac 100644
--- a/crates/game/tests/clock.rs
+++ b/crates/game/tests/clock.rs
@@ -42,8 +42,9 @@ fn every_holder_must_agree() {
     let mut c = GameClock::new(false);
     assert_eq!(c.vote("alice", Proposal::Speed { x: 4 }, &h), Ok(None));
     assert_eq!(c.speed, 1);
-    let v = c.vote_view().unwrap();
+    let v = c.vote_view(&h).unwrap();
     assert_eq!((v.proposal, v.agreed, v.expires_in_s), (Proposal::Speed { x: 4 }, vec!["alice".to_string()], 30));
+    assert_eq!(v.waiting, ["bob"], "polish spec M8: who has still to agree");
     assert_eq!(c.vote("alice", Proposal::Speed { x: 4 }, &h), Ok(None), "agreeing twice changes nothing");
     assert_eq!(c.vote("bob", Proposal::Speed { x: 4 }, &h), Ok(Some(Proposal::Speed { x: 4 })));
     assert_eq!((c.speed, c.vote.is_none()), (4, true));
@@ -69,7 +70,7 @@ fn a_different_proposal_replaces_the_open_one() {
     c.vote("alice", Proposal::Pause, &h).unwrap();
     c.lapse(10.0);
     assert_eq!(c.vote("bob", Proposal::Speed { x: 2 }, &h), Ok(None));
-    let v = c.vote_view().unwrap();
+    let v = c.vote_view(&h).unwrap();
     assert_eq!((v.proposal, v.agreed, v.expires_in_s), (Proposal::Speed { x: 2 }, vec!["bob".to_string()], 30));
     assert_eq!(c.vote("alice", Proposal::Speed { x: 2 }, &h), Ok(Some(Proposal::Speed { x: 2 })));
     assert!(!c.paused);
@@ -81,8 +82,8 @@ fn votes_lapse_after_thirty_seconds_of_real_time() {
     let mut c = GameClock::new(false);
     c.vote("alice", Proposal::Pause, &h).unwrap();
     c.lapse(20.0);
-    assert_eq!(c.vote_view().unwrap().expires_in_s, 10);
-    c.lapse(VOTE_LAPSE_S - 20.0);
+    assert_eq!(c.vote_view(&h).unwrap().expires_in_s, 10);
+    assert_eq!(c.lapse(VOTE_LAPSE_S - 20.0), Some(Proposal::Pause), "it says what lapsed");
     assert!(c.vote.is_none());
     assert_eq!(c.vote("bob", Proposal::Pause, &h), Ok(None), "a lapsed vote starts again");
 }
@@ -103,3 +104,15 @@ fn with_no_holders_the_open_vote_is_dropped_and_the_clock_stays() {
     assert_eq!(c.settle(&BTreeSet::new()), None);
     assert!(c.vote.is_none() && !c.paused);
 }
+
+/// Polish spec M8: any voter can turn a proposal down; it ends at once.
+#[test]
+fn a_voter_can_decline() {
+    let h = holders(&["alice", "bob"]);
+    let mut c = GameClock::new(false);
+    assert_eq!(c.decline("bob", &h), Ok(None), "nothing open");
+    c.vote("alice", Proposal::Pause, &h).unwrap();
+    assert_eq!(c.decline("sam", &h), Err(VoteError::NotAVoter));
+    assert_eq!(c.decline("bob", &h), Ok(Some(Proposal::Pause)));
+    assert!(c.vote.is_none() && !c.paused);
+}
diff --git a/crates/game/tests/game.rs b/crates/game/tests/game.rs
index 28689d9..086d546 100644
--- a/crates/game/tests/game.rs
+++ b/crates/game/tests/game.rs
@@ -556,3 +556,34 @@ fn a_refusal_names_the_route_in_the_way() {
     let out = g.advance(0.1);
     assert_eq!(notices(&out, "bob"), vec![Notice::Rejected { cmd: swing, reason: Rejection::PointsLocked, by: Some(s("C-W2")) }]);
 }
+
+/// Polish spec M8: every player hears how a vote ended, and who has still
+/// to agree is in the view.
+#[test]
+fn every_player_hears_how_a_vote_ended() {
+    let mut g = game();
+    join(&mut g, "alice", Some("West"));
+    join(&mut g, "bob", Some("East"));
+    g.flush();
+    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Speed { x: 4 } });
+    let out = g.flush();
+    let waiting = out.iter().find_map(|(p, m)| match m {
+        ServerMsg::Delta(d) if p == "alice" => d.vote.clone().flatten().map(|v| v.waiting),
+        _ => None,
+    });
+    assert_eq!(waiting, Some(vec![s("bob")]));
+    send(&mut g, "bob", ClientMsg::VoteDecline);
+    let out = g.flush();
+    let ended = Notice::VoteEnded { proposal: Proposal::Speed { x: 4 }, outcome: VoteOutcome::Declined { by: s("bob") } };
+    assert_eq!((notices(&out, "alice"), notices(&out, "bob")), (vec![ended.clone()], vec![ended]));
+    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Pause });
+    send(&mut g, "bob", ClientMsg::Vote { proposal: Proposal::Pause });
+    let passed = Notice::VoteEnded { proposal: Proposal::Pause, outcome: VoteOutcome::Passed };
+    assert_eq!(notices(&g.flush(), "bob"), vec![passed]);
+    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Resume });
+    g.advance(31.0);
+    let lapsed = Notice::VoteEnded { proposal: Proposal::Resume, outcome: VoteOutcome::Lapsed };
+    assert_eq!(notices(&g.flush(), "alice"), vec![lapsed]);
+    let out = send(&mut g, "sam", ClientMsg::VoteDecline);
+    assert!(out.is_empty(), "sam is not connected");
+}
diff --git a/crates/protocol/tests/diff.rs b/crates/protocol/tests/diff.rs
index e1fa09a..a3b6c4f 100644
--- a/crates/protocol/tests/diff.rs
+++ b/crates/protocol/tests/diff.rs
@@ -14,7 +14,7 @@ fn base() -> View {
         sim_time: 25200.0,
         speed: 1,
         paused: false,
-        vote: Some(VoteView { proposal: Proposal::Pause, agreed: vec![s("alice")], expires_in_s: 12 }),
+        vote: Some(VoteView { proposal: Proposal::Pause, agreed: vec![s("alice")], waiting: vec![], expires_in_s: 12 }),
         holders: BTreeMap::from([(s("East"), s("robot")), (s("West"), s("alice"))]),
         score: Some(0),
         signals: BTreeMap::from([(s("A"), Aspect::Red), (s("W1"), Aspect::Red)]),
diff --git a/crates/protocol/tests/golden.rs b/crates/protocol/tests/golden.rs
index 05ee0d3..dec2994 100644
--- a/crates/protocol/tests/golden.rs
+++ b/crates/protocol/tests/golden.rs
@@ -272,7 +272,7 @@ fn view() {
         sim_time: 25215.5,
         speed: 8,
         paused: false,
-        vote: Some(VoteView { proposal: Proposal::Pause, agreed: vec![s("alice")], expires_in_s: 30 }),
+        vote: Some(VoteView { proposal: Proposal::Pause, agreed: vec![s("alice")], waiting: vec![], expires_in_s: 30 }),
         holders: BTreeMap::from([(s("East"), s("robot")), (s("West"), s("alice"))]),
         score: Some(5),
         signals: BTreeMap::from([(s("A"), Aspect::DoubleYellow)]),
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-game --test clock --test game -p signalbox-client-core --test text`
Expected: compile errors (`waiting`, `VoteDecline`, `VoteEnded`, `decline`).

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-core/src/app.rs b/crates/client-core/src/app.rs
index 1f87672..cb65ac2 100644
--- a/crates/client-core/src/app.rs
+++ b/crates/client-core/src/app.rs
@@ -573,6 +573,11 @@ impl App {
         self.send_game(ClientMsg::Vote { proposal });
     }
 
+    /// Turn the open proposal down (polish spec M8).
+    pub fn decline_vote(&mut self) {
+        self.send_game(ClientMsg::VoteDecline);
+    }
+
     pub fn command(&mut self, cmd: PlayerCommand) {
         self.send_game(ClientMsg::Command { cmd });
     }
diff --git a/crates/client-core/src/text.rs b/crates/client-core/src/text.rs
index 5f3e617..2d9fe8e 100644
--- a/crates/client-core/src/text.rs
+++ b/crates/client-core/src/text.rs
@@ -1,7 +1,7 @@
 //! Words for the screen: times, commands, refusals, notices, votes.
 //! Signals are named as the screen shows them (`Names`).
 
-use protocol::{ExitName, Notice, PlayerCommand, PointsPos, Proposal, Rejection, TrainState, VoteView};
+use protocol::{ExitName, Notice, PlayerCommand, PointsPos, Proposal, Rejection, TrainState, VoteOutcome, VoteView};
 
 use crate::names::Names;
 
@@ -73,6 +73,15 @@ pub fn notice_text(n: &Notice, names: &Names) -> (String, bool) {
         }
         Notice::Handover { headcode, from_area } => (format!("{} offered from {from_area}", names.headcode(headcode)), false),
         Notice::AreaTaken { area, holder } => (format!("{area} is now {holder}'s"), false),
+        Notice::VoteEnded { proposal, outcome } => {
+            let p = proposal_text(*proposal);
+            let text = match outcome {
+                VoteOutcome::Passed => format!("Vote passed: {p}"),
+                VoteOutcome::Declined { by } => format!("Vote declined by {by}: {p}"),
+                VoteOutcome::Lapsed => format!("Vote lapsed: {p}"),
+            };
+            (text, false)
+        }
         Notice::Replaced => ("This login was opened somewhere else".to_string(), true),
         Notice::GameCrashed => ("The game stopped unexpectedly".to_string(), true),
         Notice::Error { message, .. } => (format!("Error: {message}"), true),
@@ -97,7 +106,9 @@ pub fn proposal_text(p: Proposal) -> String {
     }
 }
 
-/// `Vote: 4× — ann, bob agreed, 25 s left`
+/// `Vote: 4× — waiting for bob, 25 s left` (polish spec M8); a server
+/// that sends no `waiting` gets the old `ann agreed`.
 pub fn vote_text(v: &VoteView) -> String {
-    format!("Vote: {} — {} agreed, {} s left", proposal_text(v.proposal), v.agreed.join(", "), v.expires_in_s)
+    let who = if v.waiting.is_empty() { format!("{} agreed", v.agreed.join(", ")) } else { format!("waiting for {}", v.waiting.join(", ")) };
+    format!("Vote: {} — {who}, {} s left", proposal_text(v.proposal), v.expires_in_s)
 }
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index 249a042..823cbed 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -404,6 +404,7 @@ impl UiApp {
         let areas: Vec<String> = g.layout().map(|l| l.areas.clone()).unwrap_or_default();
         let holding = g.area().is_some();
         let can_vote = g.can_vote();
+        let me = g.you.clone();
         let mut act: Vec<Box<dyn FnOnce(&mut App)>> = Vec::new();
         let mut settings = self.settings;
         egui::Panel::top("bar").show(ui, |ui| {
@@ -412,7 +413,10 @@ impl UiApp {
                 if let Some(v) = &view {
                     let clock = ui.label(RichText::new(fmt_hms(v.sim_time)).monospace().size(16.0));
                     mark(ui, &clock, marked("clock"), now);
-                    ui.label(if v.paused { "paused".to_string() } else { format!("{}×", v.speed) });
+                    let state = ui.label(if v.paused { "paused".to_string() } else { format!("{}×", v.speed) });
+                    if v.paused && v.vote.is_none() {
+                        state.on_hover_text("The clock is paused (a resumed game starts paused). Press resume to propose running it.");
+                    }
                     // Only voters get the buttons (owner decision 12).
                     if can_vote {
                         let pause = if v.paused { Proposal::Resume } else { Proposal::Pause };
@@ -427,6 +431,17 @@ impl UiApp {
                     }
                     if let Some(vote) = &v.vote {
                         ui.label(RichText::new(vote_text(vote)).color(paint::YELLOW));
+                        // Polish spec M8: say yes or no explicitly.
+                        if can_vote {
+                            let p = vote.proposal;
+                            if !vote.agreed.contains(&me) && ui.button("Agree").clicked() {
+                                act.push(Box::new(move |a| a.vote(p)));
+                            }
+                            let no = if vote.agreed.contains(&me) { "Withdraw" } else { "Decline" };
+                            if ui.button(no).clicked() {
+                                act.push(Box::new(|a| a.decline_vote()));
+                            }
+                        }
                     }
                     // A tutorial keeps no score (tutorial spec §3).
                     if let Some(score) = v.score.filter(|_| !lesson) {
diff --git a/crates/game/src/clock.rs b/crates/game/src/clock.rs
index fac081f..6be6dd1 100644
--- a/crates/game/src/clock.rs
+++ b/crates/game/src/clock.rs
@@ -99,8 +99,8 @@ impl GameClock {
         Some(p)
     }
 
-    /// Let `real_dt` seconds pass for the open proposal.
-    pub fn lapse(&mut self, real_dt: f64) {
+    /// Let `real_dt` seconds pass for the open proposal; returns it if it lapsed.
+    pub fn lapse(&mut self, real_dt: f64) -> Option<Proposal> {
         let lapsed = match self.vote.as_mut() {
             Some(v) => {
                 v.left_s -= real_dt;
@@ -108,15 +108,25 @@ impl GameClock {
             }
             None => false,
         };
-        if lapsed {
-            self.vote = None;
+        if lapsed { self.vote.take().map(|v| v.proposal) } else { None }
+    }
+
+    /// `voter` turns the open proposal down: it ends at once (polish spec
+    /// M8). Returns it, or `None` when none is open.
+    pub fn decline(&mut self, voter: &str, voters: &BTreeSet<String>) -> Result<Option<Proposal>, VoteError> {
+        if !voters.contains(voter) {
+            return Err(VoteError::NotAVoter);
         }
+        Ok(self.vote.take().map(|v| v.proposal))
     }
 
-    pub fn vote_view(&self) -> Option<VoteView> {
+    /// The open proposal as players see it; `waiting` lists the `voters`
+    /// who have not agreed yet.
+    pub fn vote_view(&self, voters: &BTreeSet<String>) -> Option<VoteView> {
         self.vote.as_ref().map(|v| VoteView {
             proposal: v.proposal,
             agreed: v.agreed.iter().cloned().collect(),
+            waiting: voters.iter().filter(|n| !v.agreed.contains(*n)).cloned().collect(),
             expires_in_s: v.left_s.max(0.0).ceil() as u32,
         })
     }
diff --git a/crates/game/src/game.rs b/crates/game/src/game.rs
index 3381ed2..f05ad7c 100644
--- a/crates/game/src/game.rs
+++ b/crates/game/src/game.rs
@@ -6,7 +6,7 @@ use std::collections::{BTreeMap, BTreeSet};
 use std::path::Path;
 use std::time::Duration;
 
-use protocol::{ClientMsg, Layout, Notice, PlayerCommand, Proposal, Rejection, ServerMsg, View, codes};
+use protocol::{ClientMsg, Layout, Notice, PlayerCommand, Proposal, Rejection, ServerMsg, View, VoteOutcome, codes};
 use signalbox_core::events::{Command, Event};
 use signalbox_core::ids::AreaId;
 use signalbox_core::robot;
@@ -134,6 +134,9 @@ pub struct Game {
     last_snapshot: Option<u64>,
     /// Save failures not yet taken by the caller.
     save_errors: Vec<String>,
+    /// Clock proposals that ended since the last `flush`, told to every
+    /// player there (polish spec M8).
+    vote_ended: Vec<(Proposal, VoteOutcome)>,
 }
 
 fn error(player: &str, code: &str, message: &str) -> Out {
@@ -350,6 +353,7 @@ impl Game {
             since_snapshot_s: 0.0,
             last_snapshot: None,
             save_errors: Vec::new(),
+            vote_ended: Vec::new(),
         }
     }
 
@@ -450,6 +454,7 @@ impl Game {
             ClientMsg::Release => self.release(player),
             ClientMsg::Command { cmd } => self.command(player, cmd),
             ClientMsg::Vote { proposal } => self.vote(player, proposal),
+            ClientMsg::VoteDecline => self.decline(player),
             ClientMsg::Resync => self.resync(player),
             // Only a tutorial (`crate::lesson::Runner`) acts on these.
             ClientMsg::LessonNext | ClientMsg::LessonRestartStep | ClientMsg::LessonRestart | ClientMsg::LessonUi { .. } => {
@@ -488,7 +493,9 @@ impl Game {
     ) -> Vec<Out> {
         let dt = if real_dt.is_finite() && real_dt > 0.0 { real_dt } else { 0.0 };
         let mut out = Vec::new();
-        self.clock.lapse(dt);
+        if let Some(p) = self.clock.lapse(dt) {
+            self.vote_ended.push((p, VoteOutcome::Lapsed));
+        }
         self.expire_grace(dt);
         let n = self.clock.ticks_for(dt).min(MAX_TICKS_PER_ADVANCE);
         for _ in 0..n {
@@ -509,10 +516,16 @@ impl Game {
         out
     }
 
-    /// Deltas for every connected player whose view changed.
+    /// Deltas for every connected player whose view changed, after a notice
+    /// to each for every clock proposal that ended (polish spec M8).
     pub fn flush(&mut self) -> Vec<Out> {
         let shared = self.shared();
         let mut out = Vec::new();
+        for (proposal, outcome) in std::mem::take(&mut self.vote_ended) {
+            for (name, _) in self.players.iter().filter(|(_, p)| p.connected) {
+                out.push(notice(name, Notice::VoteEnded { proposal, outcome: outcome.clone() }));
+            }
+        }
         for (name, p) in self.players.iter_mut() {
             if !p.connected {
                 continue;
@@ -533,7 +546,7 @@ impl Game {
             sim_time: self.sim.now_s(),
             speed: self.clock.speed,
             paused: self.clock.paused,
-            vote: self.clock.vote_view(),
+            vote: self.clock.vote_view(&self.voters()),
             holders: net
                 .areas
                 .iter()
@@ -562,7 +575,9 @@ impl Game {
     /// nobody left to agree to it.
     fn settle_vote(&mut self) {
         let voters = self.voters();
-        self.clock.settle(&voters);
+        if let Some(p) = self.clock.settle(&voters) {
+            self.vote_ended.push((p, VoteOutcome::Passed));
+        }
     }
 
     fn claim(&mut self, player: &str, area: &str) -> Vec<Out> {
@@ -619,7 +634,13 @@ impl Game {
     fn vote(&mut self, player: &str, proposal: Proposal) -> Vec<Out> {
         let voters = self.voters();
         match self.clock.vote(player, proposal, &voters) {
-            Ok(_) => vec![],
+            Ok(passed) => {
+                // A lone voter's proposal applies at once: nothing to tell.
+                if voters.len() > 1 {
+                    self.vote_ended.extend(passed.map(|p| (p, VoteOutcome::Passed)));
+                }
+                vec![]
+            }
             Err(VoteError::NotAVoter) => {
                 vec![error(player, codes::NOT_A_HOLDER, "while anyone holds an area, only holders vote")]
             }
@@ -627,6 +648,16 @@ impl Game {
         }
     }
 
+    fn decline(&mut self, player: &str) -> Vec<Out> {
+        match self.clock.decline(player, &self.voters()) {
+            Ok(declined) => {
+                self.vote_ended.extend(declined.map(|p| (p, VoteOutcome::Declined { by: player.to_string() })));
+                vec![]
+            }
+            Err(_) => vec![error(player, codes::NOT_A_HOLDER, "while anyone holds an area, only holders vote")],
+        }
+    }
+
     /// Queue a command for the next tick, logging it to the save first;
     /// `player` is `ROBOT` for the robot.
     fn submit(&mut self, player: &str, cmd: Command) -> Vec<Out> {
diff --git a/crates/protocol/src/lib.rs b/crates/protocol/src/lib.rs
index ae57390..9a298af 100644
--- a/crates/protocol/src/lib.rs
+++ b/crates/protocol/src/lib.rs
@@ -11,7 +11,7 @@ pub mod view;
 pub use diff::{SeqGap, diff};
 pub use lesson::{Highlight, LessonInfo, LessonView};
 pub use lobby::{AreaHolder, ClientFrame, FrameError, GameInfo, GameState, LayoutInfo, LobbyMsg, LobbyReply, ServerFrame};
-pub use msg::{ClientMsg, ExitName, Notice, PlayerCommand, Proposal, ServerMsg};
+pub use msg::{ClientMsg, ExitName, Notice, PlayerCommand, Proposal, ServerMsg, VoteOutcome};
 pub use signalbox_core::aspect::Aspect;
 pub use signalbox_core::events::Rejection;
 pub use signalbox_core::network::{Dir, PointsPos};
diff --git a/crates/protocol/src/lobby.rs b/crates/protocol/src/lobby.rs
index 216dc90..7dc5c8f 100644
--- a/crates/protocol/src/lobby.rs
+++ b/crates/protocol/src/lobby.rs
@@ -98,11 +98,12 @@ pub struct LayoutInfo {
 pub const LOBBY_MSG_TYPES: [&str; 8] =
     ["list_games", "list_layouts", "create_game", "join", "leave", "delete_game", "list_lessons", "start_lesson"];
 /// `"type"` tags of `ClientMsg`.
-pub const CLIENT_MSG_TYPES: [&str; 9] = [
+pub const CLIENT_MSG_TYPES: [&str; 10] = [
     "claim",
     "release",
     "command",
     "vote",
+    "vote_decline",
     "resync",
     "lesson_next",
     "lesson_restart_step",
diff --git a/crates/protocol/src/msg.rs b/crates/protocol/src/msg.rs
index ec41438..add5f0b 100644
--- a/crates/protocol/src/msg.rs
+++ b/crates/protocol/src/msg.rs
@@ -17,6 +17,8 @@ pub enum ClientMsg {
     Command { cmd: PlayerCommand },
     /// Propose, or agree to, a clock change.
     Vote { proposal: Proposal },
+    /// Turn the open proposal down: it ends at once (polish spec M8).
+    VoteDecline,
     /// Ask for the layout and a full view.
     Resync,
     /// In a tutorial: the step said "press Next".
@@ -57,6 +59,15 @@ pub enum ExitName {
     Node(String),
 }
 
+/// How a clock proposal ended.
+#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
+#[serde(tag = "how", rename_all = "snake_case")]
+pub enum VoteOutcome {
+    Passed,
+    Declined { by: String },
+    Lapsed,
+}
+
 #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
 #[serde(tag = "kind", rename_all = "snake_case")]
 pub enum Proposal {
@@ -97,6 +108,8 @@ pub enum Notice {
     /// A berth in your area was filled by a step from `from_area`'s berth.
     Handover { headcode: String, from_area: String },
     AreaTaken { area: String, holder: String },
+    /// A clock proposal ended (polish spec M8); sent to every player.
+    VoteEnded { proposal: Proposal, outcome: VoteOutcome },
     Replaced,
     GameCrashed,
     Error { code: String, message: String },
diff --git a/crates/protocol/src/view.rs b/crates/protocol/src/view.rs
index ea50e42..9440bf3 100644
--- a/crates/protocol/src/view.rs
+++ b/crates/protocol/src/view.rs
@@ -262,6 +262,9 @@ pub struct TrainRow {
 pub struct VoteView {
     pub proposal: Proposal,
     pub agreed: Vec<String>,
+    /// Voters who have not agreed yet (polish spec M8).
+    #[serde(default, skip_serializing_if = "Vec::is_empty")]
+    pub waiting: Vec<String>,
     /// Whole seconds of real time before it lapses (rounded up).
     pub expires_in_s: u32,
 }
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-protocol -p signalbox-game -p signalbox-client-core -p signalbox-client-ui -p signalbox-server`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Multiplayer", after the clock-votes bullet: "A vote lists who has still to agree (`VoteView.waiting`);
any voter may Decline it (`vote_decline`), ending it at once; `flush` tells every player how each vote ended
(`Notice::VoteEnded`), except a lone voter's (polish spec M8)."

- [ ] **Step 6: Commit**

```bash
git add crates/client-core crates/client-core/src/app.rs crates/client-core/src/text.rs crates/client-ui crates/client-ui/src/screens.rs crates/game crates/game/src/clock.rs crates/game/src/game.rs crates/protocol crates/protocol/src/lib.rs crates/protocol/src/lobby.rs crates/protocol/src/msg.rs crates/protocol/src/view.rs CLAUDE.md
git commit -m "feat: votes say who they wait for, can be declined, and tell everyone how they ended"
```

---

### Task 13: A top bar that does not move; Release asks first (UI review M5, M10)

Spec §10 M5, M10 (U11). Row one keeps the buttons right-aligned in a fixed order with fixed-width clock controls;
row two carries the vote (Task 12) or the spectator's hint (Task 9); Release area needs a second click.

**Files:**
- Modify: `crates/client-ui/src/screens.rs`
- Modify: `CLAUDE.md`
- Test: `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: Task 12's vote row, Task 9's hint.
- Produces: `UiApp.confirm_release`; constants `CLOCK_STATE_W = 52.0`, `PAUSE_W = 64.0`, `LAYOUT_COMBO_W = 180.0`;
  the test helper `text_at(&FullOutput, &str) -> Rect` in `tests/screens.rs`.

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-ui/tests/screens.rs b/crates/client-ui/tests/screens.rs
index 9849cea..5d30a9c 100644
--- a/crates/client-ui/tests/screens.rs
+++ b/crates/client-ui/tests/screens.rs
@@ -794,3 +794,55 @@ fn a_vote_waits_for_named_players_who_agree_or_decline() {
     assert!(has_text(&out, "Vote declined by ann: 2×"), "{:?}", texts(&out));
     assert!(!has_text(&out, "Agree"));
 }
+
+fn text_at(out: &FullOutput, want: &str) -> Rect {
+    texts(out).into_iter().find(|(t, _)| t == want).unwrap_or_else(|| panic!("no {want:?} in {:?}", texts(out))).1
+}
+
+/// Polish spec M5: the bar's buttons stay put while a vote opens, the
+/// clock pauses and the title changes; the pause button keeps its place.
+#[test]
+fn the_top_bar_does_not_move_under_the_pointer() {
+    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
+    r.game.connect("bob");
+    r.game.handle("bob", ClientMsg::Claim { area: s("East") });
+    for _ in 0..3 {
+        r.frame();
+    }
+    let out = r.frame();
+    let (leave, fit, pause) = (text_at(&out, "Leave"), text_at(&out, "Fit"), text_at(&out, "pause"));
+    click_text(&mut r, &out, "pause");
+    for _ in 0..3 {
+        r.frame();
+    }
+    let out = r.frame();
+    assert!(has_text(&out, "waiting for bob"), "{:?}", texts(&out));
+    assert_eq!((text_at(&out, "Leave"), text_at(&out, "Fit")), (leave, fit), "a vote opened");
+    assert!(text_at(&out, "Vote: pause — waiting for bob, 30 s left").min.y > leave.max.y, "on the second row");
+    r.game.handle("bob", ClientMsg::Vote { proposal: Proposal::Pause });
+    for _ in 0..3 {
+        r.frame();
+    }
+    let out = r.frame();
+    assert_eq!((text_at(&out, "Leave"), text_at(&out, "Fit")), (leave, fit), "paused");
+    assert!((text_at(&out, "resume").center().x - pause.center().x).abs() < 1.0, "the same button, the same place");
+}
+
+/// Polish spec M10: Release area asks first.
+#[test]
+fn releasing_an_area_asks_first() {
+    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
+    let out = r.frame();
+    click_text(&mut r, &out, "Release area");
+    let out = r.frame();
+    assert!(r.ui.core.game().unwrap().area().is_some(), "not yet");
+    click_text(&mut r, &out, "Cancel");
+    let out = r.frame();
+    click_text(&mut r, &out, "Release area");
+    let out = r.frame();
+    click_text(&mut r, &out, "Yes, release");
+    for _ in 0..3 {
+        r.frame();
+    }
+    assert_eq!(r.ui.core.game().unwrap().area(), None);
+}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-ui --test screens top_bar releasing`
Expected: FAIL — `Leave` moves when the vote opens; Release area releases at once.

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index 823cbed..c846ea3 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -47,6 +47,12 @@ const SIMPLIFIER_WIDTH: f32 = {
 /// margins and a scroll bar, so the table never scrolls sideways (its
 /// header would slip off its columns) and the tabs never resize the panel.
 const SIDE_W: f32 = SIMPLIFIER_WIDTH + 24.0;
+/// The top bar's fixed widths (polish spec M5): the clock state (`paused`,
+/// `8×`) and the pause/resume button.
+const CLOCK_STATE_W: f32 = 52.0;
+/// The lobby's layout list, wide enough for every name, so Create never moves.
+const LAYOUT_COMBO_W: f32 = 180.0;
+const PAUSE_W: f32 = 64.0;
 /// Repaint at least this often (ms): clocks, flashing, reconnect timers.
 const REPAINT_MS: u64 = 250;
 /// While a tutorial highlight shows: often enough for a smooth 1 Hz pulse.
@@ -84,6 +90,8 @@ pub struct UiApp {
     new_game: NewGame,
     /// The game whose Delete was pressed and awaits "Yes, delete".
     confirm_delete: Option<String>,
+    /// Release area was pressed and awaits "Yes, release" (polish spec M10).
+    confirm_release: bool,
     settings: Settings,
     /// Where the settings are kept between visits (none in most tests).
     store: Option<Box<dyn SettingsStore>>,
@@ -124,6 +132,7 @@ impl UiApp {
             headcode: String::new(),
             new_game: NewGame::default(),
             confirm_delete: None,
+            confirm_release: false,
             settings: Settings::default(),
             store: None,
             side_tab: SideTab::default(),
@@ -257,7 +266,7 @@ impl UiApp {
             } else {
                 self.new_game.layout = self.new_game.layout.min(layouts.len() - 1);
                 ui.horizontal(|ui| {
-                    egui::ComboBox::from_label("Layout").selected_text(layouts[self.new_game.layout].as_str()).show_ui(ui, |ui| {
+                    egui::ComboBox::from_label("Layout").width(LAYOUT_COMBO_W).selected_text(layouts[self.new_game.layout].as_str()).show_ui(ui, |ui| {
                         for (i, name) in layouts.iter().enumerate() {
                             ui.selectable_value(&mut self.new_game.layout, i, name.as_str());
                         }
@@ -387,6 +396,11 @@ impl UiApp {
         self.enquiry_window(ui);
     }
 
+    /// The top bar (polish spec M5: nothing moves under the pointer). Row
+    /// one: the title and clock on the left, the buttons right-aligned in a
+    /// fixed order; the pause button and the speed are fixed widths. Row
+    /// two: an open vote with Agree and Decline (M8), else the spectator's
+    /// hint (H2), then the players and Claim.
     fn top_bar(&mut self, ui: &mut Ui, now: f64) {
         let Some(g) = self.core.game() else { return };
         let lesson = g.lesson().is_some();
@@ -407,20 +421,23 @@ impl UiApp {
         let me = g.you.clone();
         let mut act: Vec<Box<dyn FnOnce(&mut App)>> = Vec::new();
         let mut settings = self.settings;
+        let mut confirm_release = self.confirm_release && holding;
+        let mut refit = false;
         egui::Panel::top("bar").show(ui, |ui| {
-            ui.horizontal_wrapped(|ui| {
+            ui.horizontal(|ui| {
                 ui.label(RichText::new(title).strong());
                 if let Some(v) = &view {
                     let clock = ui.label(RichText::new(fmt_hms(v.sim_time)).monospace().size(16.0));
                     mark(ui, &clock, marked("clock"), now);
-                    let state = ui.label(if v.paused { "paused".to_string() } else { format!("{}×", v.speed) });
+                    let text = if v.paused { "paused".to_string() } else { format!("{}×", v.speed) };
+                    let state = ui.add_sized([CLOCK_STATE_W, 18.0], egui::Label::new(text));
                     if v.paused && v.vote.is_none() {
                         state.on_hover_text("The clock is paused (a resumed game starts paused). Press resume to propose running it.");
                     }
                     // Only voters get the buttons (owner decision 12).
                     if can_vote {
                         let pause = if v.paused { Proposal::Resume } else { Proposal::Pause };
-                        if ui.button(proposal_text(pause)).clicked() {
+                        if ui.add_sized([PAUSE_W, 18.0], egui::Button::new(proposal_text(pause))).clicked() {
                             act.push(Box::new(move |a| a.vote(pause)));
                         }
                         for x in [1u8, 2, 4, 8] {
@@ -429,7 +446,47 @@ impl UiApp {
                             }
                         }
                     }
-                    if let Some(vote) = &v.vote {
+                    // A tutorial keeps no score (tutorial spec §3).
+                    if let Some(score) = v.score.filter(|_| !lesson) {
+                        ui.label(format!("Penalty {score}"));
+                    }
+                }
+                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
+                    if ui.button("Leave").clicked() {
+                        act.push(Box::new(|a| a.leave()));
+                    }
+                    // A tutorial's player keeps the lesson's area. Releasing
+                    // asks first (polish spec M10).
+                    if holding && !lesson {
+                        if confirm_release {
+                            if ui.button("Cancel").clicked() {
+                                confirm_release = false;
+                            }
+                            if ui.button(RichText::new("Yes, release").color(ALARM)).clicked() {
+                                confirm_release = false;
+                                act.push(Box::new(|a| a.release()));
+                            }
+                        } else if ui.button("Release area").clicked() {
+                            confirm_release = true;
+                        }
+                    }
+                    let menu = ui.menu_button("Settings", |ui| {
+                        ui.label(RichText::new("Signal aspects").strong());
+                        ui.radio_value(&mut settings.aspects, AspectMode::RedGreen, "Red/green (panel)");
+                        ui.radio_value(&mut settings.aspects, AspectMode::Real, "Real aspects");
+                        ui.separator();
+                        ui.checkbox(&mut settings.enquiry, "Headcode enquiry");
+                        ui.checkbox(&mut settings.numbers, "Signal numbers");
+                    });
+                    mark(ui, &menu.response, marked("settings"), now);
+                    if ui.button("Fit").clicked() {
+                        refit = true;
+                    }
+                });
+            });
+            ui.horizontal_wrapped(|ui| {
+                match view.as_ref().and_then(|v| v.vote.as_ref()) {
+                    Some(vote) => {
                         ui.label(RichText::new(vote_text(vote)).color(paint::YELLOW));
                         // Polish spec M8: say yes or no explicitly.
                         if can_vote {
@@ -443,37 +500,11 @@ impl UiApp {
                             }
                         }
                     }
-                    // A tutorial keeps no score (tutorial spec §3).
-                    if let Some(score) = v.score.filter(|_| !lesson) {
-                        ui.label(format!("Penalty {score}"));
+                    // Polish spec H2: a spectator's clicks do nothing; say so where they look.
+                    None if !holding && !lesson => {
+                        ui.label(RichText::new("You are watching. Claim an area to signal:").color(paint::YELLOW));
                     }
-                }
-                if ui.button("Fit").clicked() {
-                    self.fitted = None;
-                }
-                let menu = ui.menu_button("Settings", |ui| {
-                    ui.label(RichText::new("Signal aspects").strong());
-                    ui.radio_value(&mut settings.aspects, AspectMode::RedGreen, "Red/green (panel)");
-                    ui.radio_value(&mut settings.aspects, AspectMode::Real, "Real aspects");
-                    ui.separator();
-                    ui.checkbox(&mut settings.enquiry, "Headcode enquiry");
-                    ui.checkbox(&mut settings.numbers, "Signal numbers");
-                });
-                mark(ui, &menu.response, marked("settings"), now);
-                // A tutorial's player keeps the lesson's area.
-                if holding && !lesson {
-                    if ui.button("Release area").clicked() {
-                        act.push(Box::new(|a| a.release()));
-                    }
-                }
-                if ui.button("Leave").clicked() {
-                    act.push(Box::new(|a| a.leave()));
-                }
-            });
-            ui.horizontal_wrapped(|ui| {
-                // Polish spec H2: a spectator's clicks do nothing; say so where they look.
-                if !holding && !lesson {
-                    ui.label(RichText::new("You are watching. Claim an area to signal:").color(paint::YELLOW));
+                    None => {}
                 }
                 ui.label("Players:");
                 for area in &areas {
@@ -486,6 +517,10 @@ impl UiApp {
                 }
             });
         });
+        self.confirm_release = confirm_release;
+        if refit {
+            self.fitted = None;
+        }
         self.set_settings(settings);
         for f in act {
             f(&mut self.core);
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Browser client": "The top bar never reflows (polish spec M5): buttons right-aligned in a fixed order,
fixed-width clock controls, the vote on the second row; Release area asks first (M10)."

- [ ] **Step 6: Commit**

```bash
git add crates/client-ui crates/client-ui/src/screens.rs CLAUDE.md
git commit -m "feat(client-ui): a top bar that does not move under the pointer; Release area asks first"
```

---

### Task 14: Leave releases; the lobby hears changes (UI review M9)

Spec §10 M9 (U15). Leave gives a held area back before leaving; the front sends the games list to players in the
lobby whenever a game's holders or players change.

**Files:**
- Modify: `crates/client-core/src/app.rs`
- Modify: `crates/server/src/supervisor.rs`
- Modify: `CLAUDE.md`
- Test: `crates/client-core/tests/app.rs`
- Test: `crates/server/tests/supervisor.rs`

**Interfaces:**
- Consumes: `Supervisor::{list_games, for_user}`, `FromGame::Status`.
- Produces: `Supervisor::broadcast_lobby_games()`; `App::leave` sends `Release` first when holding an area outside a
  tutorial; the supervisor test helper `listed` drains pushed lists before asking.

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-core/tests/app.rs b/crates/client-core/tests/app.rs
index 38441d1..a459acc 100644
--- a/crates/client-core/tests/app.rs
+++ b/crates/client-core/tests/app.rs
@@ -601,3 +601,16 @@ fn a_new_game_claims_the_creators_area_once_its_layout_comes() {
     app.tick(1.0);
     assert_eq!(h.take_sent(), [lobby(LobbyMsg::CreateGame { layout: s("twobox"), seed: None, start: None })], "no claim");
 }
+
+/// Polish spec M9: Leave gives a held area back before leaving.
+#[test]
+fn leave_releases_a_held_area_first() {
+    let mut t = Table::new("ann", Some("West"));
+    t.h.take_sent();
+    t.app.leave();
+    assert_eq!(t.h.take_sent(), [ClientFrame::Game(ClientMsg::Release), lobby(LobbyMsg::Leave)]);
+    let mut spec = Table::new("sam", None);
+    spec.h.take_sent();
+    spec.app.leave();
+    assert_eq!(spec.h.take_sent(), [lobby(LobbyMsg::Leave)], "nothing to release");
+}
diff --git a/crates/server/tests/supervisor.rs b/crates/server/tests/supervisor.rs
index 32ce001..b53ed34 100644
--- a/crates/server/tests/supervisor.rs
+++ b/crates/server/tests/supervisor.rs
@@ -694,6 +694,9 @@ async fn games_list(sock: &Sock) -> Vec<GameInfo> {
 }
 
 async fn listed(rig: &Rig, sock: &Sock) -> Vec<GameInfo> {
+    // Lists the lobby was sent as holders and players changed (polish spec
+    // M9) may be waiting: drop them, so the answer read is this request's.
+    while let Ok(Some(_)) = timeout(Duration::from_millis(100), sock.me.outbox.pop()).await {}
     rig.lobby(sock, LobbyMsg::ListGames);
     games_list(sock).await
 }
@@ -1073,3 +1076,23 @@ async fn a_burst_of_lesson_starts_never_runs_more_than_the_cap() {
     let bob = rig.attach("bob");
     start_lesson(&rig, &bob).await;
 }
+
+/// Polish spec M9: players in the lobby see a game's holders and players
+/// change without pressing Refresh.
+#[tokio::test]
+async fn the_lobby_hears_when_holders_change() {
+    let rig = rig("lobbypush", 600);
+    let ann = rig.attach("ann");
+    let id = create(&rig, &ann).await;
+    let cat = rig.attach("cat");
+    rig.lobby(&cat, LobbyMsg::ListGames);
+    rig.game_msg(&ann, ClientMsg::Claim { area: s("West") });
+    let held = |f: &ServerFrame| match f {
+        ServerFrame::Lobby(LobbyReply::Games { games, .. }) => games
+            .iter()
+            .any(|g| g.id == id && g.areas.iter().any(|a| a.holder.as_deref() == Some("ann"))),
+        _ => false,
+    };
+    until(&cat, held).await;
+    rig.sup.shutdown_all(Duration::from_secs(10)).await;
+}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-core --test app leave -p signalbox-server --test supervisor the_lobby_hears`
Expected: FAIL — no `Release` sent; `cat` never sees ann holding West (the test times out after 10 s).

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-core/src/app.rs b/crates/client-core/src/app.rs
index cb65ac2..9dd85b2 100644
--- a/crates/client-core/src/app.rs
+++ b/crates/client-core/src/app.rs
@@ -545,9 +545,14 @@ impl App {
         }
     }
 
-    /// Back to the lobby (the front answers with the games list).
+    /// Back to the lobby (the front answers with the games list). An area you
+    /// hold is released first, so it does not wait out the disconnect grace
+    /// as yours (polish spec M9); a tutorial ends anyway.
     pub fn leave(&mut self) {
         self.claim_on_join = None;
+        if self.game.as_ref().is_some_and(|g| g.area().is_some() && g.lesson.is_none()) {
+            self.send(ClientFrame::Game(ClientMsg::Release));
+        }
         self.send(ClientFrame::Lobby(LobbyMsg::Leave));
         self.game = None;
         self.rejoin = None;
diff --git a/crates/server/src/supervisor.rs b/crates/server/src/supervisor.rs
index e171ea4..d6f7672 100644
--- a/crates/server/src/supervisor.rs
+++ b/crates/server/src/supervisor.rs
@@ -585,6 +585,15 @@ impl Supervisor {
         }
     }
 
+    /// The games list to every client in the lobby (not in a game).
+    fn broadcast_lobby_games(&self) {
+        let games = self.list_games();
+        let st = self.lock();
+        for (user, c) in st.clients.iter().filter(|(_, c)| c.game.is_none()) {
+            push(&st, user, c, frame(LobbyReply::Games { games: self.for_user(games.clone(), user) }));
+        }
+    }
+
     // ---- the lobby ----
 
     /// The games list as `user` sees it (`can_delete` set for them).
@@ -803,8 +812,19 @@ impl Supervisor {
                 push(&st, &player, c, ServerFrame::Game(msg));
             }
             FromGame::Status(s) => {
-                if let Some(e) = self.lock().games.get_mut(id) {
-                    e.status = Some(s);
+                // Who holds what and who is in: the lobby hears at once
+                // (polish spec M9); a tutorial is never listed.
+                let changed = match self.lock().games.get_mut(id) {
+                    Some(e) => {
+                        let changed = e.owner.is_none()
+                            && e.status.as_ref().is_none_or(|o| o.holders != s.holders || o.players != s.players);
+                        e.status = Some(s);
+                        changed
+                    }
+                    None => false,
+                };
+                if changed {
+                    self.broadcast_lobby_games();
                 }
             }
             // The save file is the record; nothing to keep.
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-core -p signalbox-server`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Server": "The front sends the games list to every client in the lobby when a game's holders or
connected players change (`broadcast_lobby_games`, polish spec M9)."

- [ ] **Step 6: Commit**

```bash
git add crates/client-core crates/client-core/src/app.rs crates/server crates/server/src/supervisor.rs CLAUDE.md
git commit -m "feat: Leave releases a held area; the lobby's list updates itself"
```

---

### Task 15: The train list: headings, Arr and Dep, one lateness style (UI review M6)

Spec §10 M6 (U12). The list is headed, shows the next call's booked arrival and departure, writes lateness as the
simplifier does, and says when it is empty. The lateness value is what `robot-fixes`' H1 rule gives (`game::view::row`
on the base): only the style changes here. Lesson 3's last step no longer says "+minutes".

**Files:**
- Modify: `crates/client-core/src/simplifier.rs`
- Modify: `crates/client-ui/src/screens.rs`
- Modify: `crates/game/src/view.rs`
- Modify: `crates/protocol/src/view.rs`
- Modify: `lessons/03-running-trains/lesson.json`
- Modify: `CLAUDE.md`
- Test: `crates/client-core/tests/input.rs`
- Test: `crates/client-ui/tests/screens.rs`
- Test: `crates/game/tests/trains.rs`
- Test: `crates/protocol/tests/diff.rs`
- Test: `crates/protocol/tests/golden.rs`

**Interfaces:**
- Consumes: `simplifier::fmt_wtt`; `robot-fixes`' `row` in `crates/game/src/view.rs` (keep its lateness rule, add the two fields).
- Produces: `TrainRow.{arr, dep}: Option<f64>` (omitted when none); `pub simplifier::late_text(i64) -> String`.

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-core/tests/input.rs b/crates/client-core/tests/input.rs
index 456d709..3a99d18 100644
--- a/crates/client-core/tests/input.rs
+++ b/crates/client-core/tests/input.rs
@@ -345,7 +345,7 @@ fn an_auto_worked_route_stays_set_after_a_train_passes() {
 
 #[test]
 fn the_train_list_puts_platforms_first_then_by_booked_time() {
-    let row = |state, booked: Option<f64>| TrainRow { next_place: None, next_platform: None, booked, late_s: 0, state };
+    let row = |state, booked: Option<f64>| TrainRow { next_place: None, next_platform: None, booked, arr: None, dep: None, late_s: 0, state };
     let mut v = empty_view();
     v.trains.insert(s("1A"), row(TrainState::Due, Some(100.0)));
     v.trains.insert(s("1B"), row(TrainState::InArea, Some(300.0)));
diff --git a/crates/client-ui/tests/screens.rs b/crates/client-ui/tests/screens.rs
index 5d30a9c..1c122e7 100644
--- a/crates/client-ui/tests/screens.rs
+++ b/crates/client-ui/tests/screens.rs
@@ -846,3 +846,21 @@ fn releasing_an_area_asks_first() {
     }
     assert_eq!(r.ui.core.game().unwrap().area(), None);
 }
+
+/// Polish spec M6: the train list has headings, the next call's Arr and
+/// Dep, the simplifier's lateness style, and says when it is empty.
+#[test]
+fn the_train_list_is_headed_and_late_as_the_simplifier_says() {
+    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
+    let out = r.frame();
+    let side = side_texts(&r, &out);
+    for want in ["Train", "State", "Next", "Arr", "Dep", "Late", "07:04", "07:05"] {
+        assert!(side.iter().any(|t| t == want), "{want} in {side:?}");
+    }
+    assert!(side.iter().any(|t| t == "OT"), "1E01 is running, on time: {side:?}");
+    let empty = drawn_twobox_with(|w| {
+        w["entries"] = serde_json::json!([]);
+    });
+    let mut r = Rig::in_game(empty, Some("West"));
+    assert!(has_text(&r.frame(), "No trains here or due in the next 30 minutes"));
+}
diff --git a/crates/game/tests/trains.rs b/crates/game/tests/trains.rs
index bd754e4..9e8fa93 100644
--- a/crates/game/tests/trains.rs
+++ b/crates/game/tests/trains.rs
@@ -43,6 +43,8 @@ fn before_anything_enters_each_area_sees_what_is_due_at_its_boundaries() {
             next_place: Some("EST".into()),
             next_platform: Some("1".into()),
             booked: Some(25_440.0),
+            arr: Some(25_440.0),
+            dep: Some(25_500.0),
             late_s: 0,
             state: TrainState::Due,
         }
diff --git a/crates/protocol/tests/diff.rs b/crates/protocol/tests/diff.rs
index a3b6c4f..0f94f6b 100644
--- a/crates/protocol/tests/diff.rs
+++ b/crates/protocol/tests/diff.rs
@@ -30,7 +30,7 @@ fn base() -> View {
 }
 
 fn row(state: TrainState, late_s: i64) -> TrainRow {
-    TrainRow { next_place: Some(s("EST")), next_platform: Some(s("1")), booked: Some(25500.0), late_s, state }
+    TrainRow { next_place: Some(s("EST")), next_platform: Some(s("1")), booked: Some(25500.0), arr: None, dep: None, late_s, state }
 }
 
 fn changed() -> View {
diff --git a/crates/protocol/tests/golden.rs b/crates/protocol/tests/golden.rs
index dec2994..92bc47d 100644
--- a/crates/protocol/tests/golden.rs
+++ b/crates/protocol/tests/golden.rs
@@ -287,13 +287,15 @@ fn view() {
                     next_place: Some(s("EST")),
                     next_platform: Some(s("1")),
                     booked: Some(25500.0),
+                    arr: None,
+                    dep: None,
                     late_s: 120,
                     state: TrainState::InArea,
                 },
             ),
             (
                 s("2W03"),
-                TrainRow { next_place: None, next_platform: None, booked: None, late_s: 0, state: TrainState::Due },
+                TrainRow { next_place: None, next_platform: None, booked: None, arr: None, dep: None, late_s: 0, state: TrainState::Due },
             ),
         ]),
     };
@@ -333,6 +335,8 @@ fn delta_sends_only_changes_and_null_for_cleared() {
                     next_place: Some(s("WST")),
                     next_platform: None,
                     booked: Some(26100.0),
+                    arr: None,
+                    dep: None,
                     late_s: 0,
                     state: TrainState::AtPlatform,
                 }),
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-game --test trains -p signalbox-client-ui --test screens the_train_list`
Expected: compile errors (no fields `arr`, `dep`).

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-core/src/simplifier.rs b/crates/client-core/src/simplifier.rs
index 8d43f58..60291da 100644
--- a/crates/client-core/src/simplifier.rs
+++ b/crates/client-core/src/simplifier.rs
@@ -116,7 +116,8 @@ pub fn lines(l: &Layout, r: &SimplifierRow) -> Vec<Line> {
 }
 
 /// `OT` under a minute late (or early), else whole minutes late: `3L`.
-fn late_text(late_s: i64) -> String {
+/// The one lateness style, in the simplifier and the train list (polish spec M6).
+pub fn late_text(late_s: i64) -> String {
     if late_s >= 60 { format!("{}L", late_s / 60) } else { "OT".to_string() }
 }
 
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index c846ea3..0107ee8 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -15,7 +15,7 @@ use egui::{
     Align, Align2, Color32, CornerRadius, FontId, Frame, Key, Layout, PointerButton, Rect, Response, RichText, Sense, Stroke,
     StrokeKind, Ui, vec2,
 };
-use protocol::{GameState, Highlight, Proposal};
+use protocol::{GameState, Highlight, Proposal, TrainState};
 
 use crate::camera::Camera;
 use crate::hit::hit_test;
@@ -602,14 +602,26 @@ impl UiApp {
         }
     }
 
+    /// The train list (polish spec M6): headed columns, the next call's
+    /// booked arrival and departure, lateness as the simplifier shows it
+    /// (`OT`, `3L`; blank while due), and a line when it is empty.
     fn trains_ui(&mut self, ui: &mut Ui, height: f32) {
         let Some(g) = self.core.game() else { return };
         let enquiry = self.settings.enquiry;
         let mut open = None;
         egui::ScrollArea::vertical().id_salt("trains").max_height(height).show(ui, |ui| {
             let Some(v) = g.view() else { return };
+            let rows = train_list(v);
+            if rows.is_empty() {
+                ui.label(RichText::new("No trains here or due in the next 30 minutes").color(paint::LABEL));
+                return;
+            }
             egui::Grid::new("train_list").striped(true).show(ui, |ui| {
-                for (h, r) in train_list(v) {
+                for h in ["Train", "State", "Next", "Arr", "Dep", "Late"] {
+                    ui.label(RichText::new(h).strong());
+                }
+                ui.end_row();
+                for (h, r) in rows {
                     let code = RichText::new(g.names().headcode(h)).monospace().color(paint::HEADCODE);
                     // With the enquiry on, a headcode opens its window.
                     if enquiry {
@@ -631,8 +643,10 @@ impl UiApp {
                     if let Some(name) = place {
                         cell.on_hover_text(name);
                     }
-                    ui.label(r.booked.map_or(String::new(), |b| fmt_hms(b)[..5].to_string()));
-                    ui.label(if r.late_s > 0 { format!("+{}", r.late_s / 60) } else { String::new() });
+                    ui.label(r.arr.map(simplifier::fmt_wtt).unwrap_or_default());
+                    ui.label(r.dep.map(simplifier::fmt_wtt).unwrap_or_default());
+                    let late = if r.state == TrainState::Due { String::new() } else { simplifier::late_text(r.late_s) };
+                    ui.label(RichText::new(&late).color(if late == "OT" { paint::LABEL } else { ALARM }));
                     ui.end_row();
                 }
             });
diff --git a/crates/game/src/view.rs b/crates/game/src/view.rs
index 8782583..bbddfe9 100644
--- a/crates/game/src/view.rs
+++ b/crates/game/src/view.rs
@@ -109,6 +109,8 @@ fn row(call: Option<&Call>, now: f64, state: TrainState) -> TrainRow {
         next_place: call.map(|c| c.place.clone()),
         next_platform: call.and_then(|c| c.platform.clone()),
         booked,
+        arr: call.and_then(|c| c.arr_s),
+        dep: call.and_then(|c| c.dep_s),
         late_s: late_s(now, booked),
         state,
     }
diff --git a/crates/protocol/src/view.rs b/crates/protocol/src/view.rs
index 9440bf3..ee2d6b4 100644
--- a/crates/protocol/src/view.rs
+++ b/crates/protocol/src/view.rs
@@ -253,6 +253,12 @@ pub struct TrainRow {
     pub next_platform: Option<String>,
     /// Booked time at the next call (arrival, else departure), seconds since midnight.
     pub booked: Option<f64>,
+    /// The next call's booked arrival and departure (polish spec M6: the
+    /// train list's Arr and Dep columns).
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub arr: Option<f64>,
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub dep: Option<f64>,
     /// How late against `booked` right now, in whole minutes, as seconds; never negative.
     pub late_s: i64,
     pub state: TrainState,
diff --git a/lessons/03-running-trains/lesson.json b/lessons/03-running-trains/lesson.json
index 41d2ec5..5d6b2b2 100644
--- a/lessons/03-running-trains/lesson.json
+++ b/lessons/03-running-trains/lesson.json
@@ -50,7 +50,7 @@
       "wait_for": {"train_left_area": {"headcode": "2H05"}}
     },
     {
-      "say": "Well done. A train that runs behind its booked time is late: the train list shows +minutes and the simplifier shows how late (OT on time, 3L three minutes late). In a real game late trains cost penalty points; tutorials keep no score. Next lesson: junctions, auto-working and handing trains to the next box. Press Next to finish.",
+      "say": "Well done. A train that runs behind its booked time is late: the train list and the simplifier show how late (OT on time, 3L three minutes late). In a real game late trains cost penalty points; tutorials keep no score. Next lesson: junctions, auto-working and handing trains to the next box. Press Next to finish.",
       "wait_for": {"continue": {}}
     }
   ]
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-protocol -p signalbox-game -p signalbox-client-core -p signalbox-client-ui && scripts/cargo test -p signalbox-game --test lessons`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Browser client": "The train list is headed (Train, State, Next, Arr, Dep, Late) and writes lateness as
the simplifier does, `OT`/`3L` (polish spec M6)."

- [ ] **Step 6: Commit**

```bash
git add crates/client-core crates/client-core/src/simplifier.rs crates/client-ui crates/client-ui/src/screens.rs crates/game crates/game/src/view.rs crates/protocol crates/protocol/src/view.rs lessons/03-running-trains/lesson.json CLAUDE.md
git commit -m "feat: the train list is headed, shows Arr and Dep, and writes lateness as the simplifier does"
```

---

### Task 16: The enquiry window: beside the click, labelled (UI review M7)

Spec §10 M7 (U13). It opens beside where it was asked for, with a labelled grid and what the train does next.

**Files:**
- Modify: `crates/client-core/src/simplifier.rs`
- Modify: `crates/client-ui/src/screens.rs`
- Test: `crates/client-core/tests/simplifier.rs`
- Test: `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: Task 15's `TrainRow.{arr, dep}`, Task 8's `Names::place`.
- Produces: `Enquiry::next_text(&Names) -> Option<String>`; `UiApp.enquiry_at`; `ENQUIRY_OFFSET_PX = 16.0`.

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-core/tests/simplifier.rs b/crates/client-core/tests/simplifier.rs
index ae08a4f..aec5a58 100644
--- a/crates/client-core/tests/simplifier.rs
+++ b/crates/client-core/tests/simplifier.rs
@@ -130,3 +130,30 @@ fn display_headcodes_are_shown_and_searched() {
     let first = &lines(&l, rows(&l, "201")[0])[0];
     assert_eq!((first.headcode.as_str(), first.shown.as_str()), ("1E01", "201"));
 }
+
+/// Polish spec M7: the enquiry says what the train does next.
+#[test]
+fn the_enquiry_says_what_the_train_does_next() {
+    let mut t = Table::new("eve", Some("East"));
+    let row = |state, arr, dep| TrainRow {
+        next_place: Some(s("EST")),
+        next_platform: Some(s("1")),
+        booked: arr,
+        arr,
+        dep,
+        late_s: 0,
+        state,
+    };
+    let names = client_core::Names::default();
+    let l = t.layout().clone();
+    let mut v = t.view().clone();
+    v.trains.insert(s("1E01"), row(TrainState::AtPlatform, Some(25_440.0), Some(25_500.0)));
+    assert_eq!(enquiry(&l, Some(&v), "1E01").next_text(&names).as_deref(), Some("depart EST 1 at 07:05"));
+    v.trains.insert(s("1E01"), row(TrainState::InArea, Some(25_440.0), Some(25_500.0)));
+    assert_eq!(enquiry(&l, Some(&v), "1E01").next_text(&names).as_deref(), Some("arrive EST 1 at 07:04"));
+    v.trains.insert(s("1E01"), row(TrainState::Approaching, None, Some(25_530.0)));
+    assert_eq!(enquiry(&l, Some(&v), "1E01").next_text(&names).as_deref(), Some("pass EST 1 at 07:05½"));
+    v.trains.insert(s("1E01"), row(TrainState::Due, Some(25_440.0), None));
+    assert_eq!(enquiry(&l, Some(&v), "1E01").next_text(&names), None);
+    t.run(0.1);
+}
diff --git a/crates/client-ui/tests/screens.rs b/crates/client-ui/tests/screens.rs
index 1c122e7..a58e2c9 100644
--- a/crates/client-ui/tests/screens.rs
+++ b/crates/client-ui/tests/screens.rs
@@ -510,7 +510,15 @@ fn a_headcode_in_the_train_list_opens_the_enquiry() {
     r.click(at, PointerButton::Primary);
     let out = r.frame();
     assert!(has_text(&out, "Train 1E01"), "{:?}", texts(&out));
-    assert!(has_text(&out, "EST to EST") && has_text(&out, "EST 1 07:04 07:05"), "East's simplifier row");
+    // Polish spec M7: labelled, with what the train does next, and opened
+    // beside the click, clear of the top bar's Players row.
+    for want in ["State", "Next", "arrive EST 1 at 07:04", "Runs", "EST to EST", "Place", "Plat", "07:04", "07:05"] {
+        assert!(has_text(&out, want), "{want} in {:?}", texts(&out));
+    }
+    let win = r.ctx.memory(|m| m.area_rect(egui::Id::new("enquiry"))).unwrap();
+    let players = texts(&out).into_iter().find(|(t, _)| t == "Players:").unwrap().1;
+    assert!(win.min.y > players.max.y, "{win:?} below {players:?}");
+    assert!((win.min.y - at.y).abs() < 40.0, "{win:?} level with {at:?} (kept on screen sideways)");
 }
 
 // ---- fix round 1: per-game state, the simplifier's cache and scroll, the enquiry's ways out ----
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-core --test simplifier the_enquiry -p signalbox-client-ui --test screens a_headcode_in_the_train_list`
Expected: compile error (`next_text` not found).

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-core/src/simplifier.rs b/crates/client-core/src/simplifier.rs
index 60291da..635800f 100644
--- a/crates/client-core/src/simplifier.rs
+++ b/crates/client-core/src/simplifier.rs
@@ -149,6 +149,24 @@ pub fn enquiry<'a>(l: &'a Layout, v: Option<&'a View>, headcode: &str) -> Enquir
 }
 
 impl Enquiry<'_> {
+    /// What the train does next (polish spec M7): `depart LIVERPOOL STREET 10
+    /// at 06:00` standing at a platform, else `arrive … at …` (or `pass …
+    /// at …`); `None` when not running or its timetable is done.
+    pub fn next_text(&self, names: &crate::Names) -> Option<String> {
+        let t = self.train.filter(|t| t.state != TrainState::Due)?;
+        let place = names.place(t.next_place.as_deref()?);
+        let at = match &t.next_platform {
+            Some(pf) => format!("{place} {pf}"),
+            None => place.to_string(),
+        };
+        let time = |v: Option<f64>| v.map(|s| format!(" at {}", fmt_wtt(s))).unwrap_or_default();
+        Some(match (t.state, t.arr) {
+            (TrainState::AtPlatform, _) => format!("depart {at}{}", time(t.dep)),
+            (_, Some(_)) => format!("arrive {at}{}", time(t.arr)),
+            (_, None) => format!("pass {at}{}", time(t.dep)),
+        })
+    }
+
     /// `in area, 3L`, `due`, or `not in your train list`.
     pub fn live_text(&self) -> String {
         match self.train {
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index 0107ee8..f5e40c8 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -47,6 +47,8 @@ const SIMPLIFIER_WIDTH: f32 = {
 /// margins and a scroll bar, so the table never scrolls sideways (its
 /// header would slip off its columns) and the tabs never resize the panel.
 const SIDE_W: f32 = SIMPLIFIER_WIDTH + 24.0;
+/// The enquiry window opens this far right of and below where it was asked for.
+const ENQUIRY_OFFSET_PX: f32 = 16.0;
 /// The top bar's fixed widths (polish spec M5): the clock state (`paused`,
 /// `8×`) and the pause/resume button.
 const CLOCK_STATE_W: f32 = 52.0;
@@ -100,6 +102,9 @@ pub struct UiApp {
     search: String,
     /// The headcode whose enquiry window is open.
     enquiry: Option<String>,
+    /// Where the pointer was when it opened: the window opens beside it
+    /// (polish spec M7).
+    enquiry_at: Option<egui::Pos2>,
     /// The game drawn last frame; another (or the lobby) forgets the
     /// enquiry, the search and the simplifier lines.
     shown_game: Option<String>,
@@ -138,6 +143,7 @@ impl UiApp {
             side_tab: SideTab::default(),
             search: String::new(),
             enquiry: None,
+            enquiry_at: None,
             shown_game: None,
             simplifier_lines: None,
             placement: None,
@@ -653,6 +659,7 @@ impl UiApp {
         });
         if open.is_some() {
             self.enquiry = open;
+            self.enquiry_at = ui.ctx().pointer_interact_pos();
         }
     }
 
@@ -706,24 +713,55 @@ impl UiApp {
         });
     }
 
+    /// The headcode enquiry (realism spec §3; polish spec M7): opened beside
+    /// where it was asked for, its facts in a labelled grid, then the
+    /// timetable rows with headed columns.
     fn enquiry_window(&mut self, ui: &mut Ui) {
         let Some(h) = self.enquiry.clone() else { return };
         let mut open = true;
         let Some(g) = self.core.game() else { return };
         let (Some(l), v) = (g.layout(), g.view()) else { return };
         let e = simplifier::enquiry(l, v, &h);
-        egui::Window::new(format!("Train {}", g.names().headcode(&h))).id(egui::Id::new("enquiry")).open(&mut open).resizable(false).show(ui.ctx(), |ui| {
-            ui.label(e.live_text());
+        let names = g.names();
+        let place = |p: Option<&str>| p.map_or("?", |p| names.place(p)).to_string();
+        let mut w = egui::Window::new(format!("Train {}", names.headcode(&h))).id(egui::Id::new("enquiry")).open(&mut open).resizable(false);
+        if let Some(at) = self.enquiry_at {
+            w = w.default_pos(at + vec2(ENQUIRY_OFFSET_PX, ENQUIRY_OFFSET_PX));
+        }
+        w.show(ui.ctx(), |ui| {
+            egui::Grid::new("enquiry_facts").num_columns(2).show(ui, |ui| {
+                ui.label(RichText::new("State").strong());
+                ui.label(e.live_text());
+                ui.end_row();
+                if let Some(next) = e.next_text(names) {
+                    ui.label(RichText::new("Next").strong());
+                    ui.label(next);
+                    ui.end_row();
+                }
+                if let Some(r) = e.rows.first() {
+                    ui.label(RichText::new("Runs").strong());
+                    ui.label(format!("{} to {}", place(r.origin.as_deref()), place(r.destination.as_deref())));
+                    ui.end_row();
+                }
+            });
             if e.rows.is_empty() {
                 ui.label("Not in the simplifier for this area");
             }
-            for r in &e.rows {
-                let names = g.names();
-                let place = |p: Option<&str>| p.map_or("?", |p| names.place(p)).to_string();
-                ui.label(format!("{} to {}", place(r.origin.as_deref()), place(r.destination.as_deref())));
-                for line in simplifier::lines(l, r) {
-                    ui.label(format!("{} {} {} {}", names.place(&line.place), line.platform, line.arr, line.dep));
-                }
+            for (i, r) in e.rows.iter().enumerate() {
+                ui.separator();
+                egui::Grid::new(("enquiry_calls", i)).striped(true).show(ui, |ui| {
+                    for head in ["Place", "Plat", "Arr", "Dep"] {
+                        ui.label(RichText::new(head).strong());
+                    }
+                    ui.end_row();
+                    for line in simplifier::lines(l, r) {
+                        ui.label(names.place(&line.place));
+                        ui.label(&line.platform);
+                        ui.label(&line.arr);
+                        ui.label(&line.dep);
+                        ui.end_row();
+                    }
+                });
             }
         });
         if !open {
@@ -815,7 +853,10 @@ impl UiApp {
         match click {
             // With the enquiry on, a headcode opens its window and nothing else.
             Some(Some(t)) => match self.core.headcode_at(&t).filter(|_| self.settings.enquiry) {
-                Some(h) => self.enquiry = Some(h),
+                Some(h) => {
+                    self.enquiry = Some(h);
+                    self.enquiry_at = ui.ctx().pointer_interact_pos();
+                }
                 None => self.core.click(&t),
             },
             Some(None) => self.core.escape(),
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Commit**

```bash
git add crates/client-core crates/client-core/src/simplifier.rs crates/client-ui crates/client-ui/src/screens.rs
git commit -m "feat(client): the enquiry opens beside the click, labelled, with what the train does next"
```

---

### Task 17: The simplifier fits the layout's headcodes (UI review H3)

Spec §10 H3 (U3). The Train column fits the longest displayed headcode, the panel starts wide enough, and every
cell shows its whole text (places by name) on hover.

**Files:**
- Modify: `crates/client-ui/src/screens.rs`
- Modify: `CLAUDE.md`
- Test: `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: `simplifier::shown` (drain-wtt plan), Task 8's `Names::place`.
- Produces: `pub screens::{simplifier_columns(f32) -> [f32; 8], table_width(&[f32; 8]) -> f32}`; `UiApp.simplifier_cols`;
  `simplifier_row(ui, row_h, cells, &cols, hovers)`; the test helper `converted(name) -> World` in `tests/screens.rs`.

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-ui/tests/screens.rs b/crates/client-ui/tests/screens.rs
index a58e2c9..eea8b53 100644
--- a/crates/client-ui/tests/screens.rs
+++ b/crates/client-ui/tests/screens.rs
@@ -872,3 +872,29 @@ fn the_train_list_is_headed_and_late_as_the_simplifier_says() {
     let mut r = Rig::in_game(empty, Some("West"));
     assert!(has_text(&r.frame(), "No trains here or due in the next 30 minutes"));
 }
+
+fn converted(name: &str) -> signalbox_core::world::World {
+    let dir = env!("CARGO_MANIFEST_DIR");
+    let read = |p: String| std::fs::read_to_string(p).unwrap();
+    let mut w = ts2_import::convert(&read(format!("{dir}/../ts2-import/tests/data/{name}.json"))).unwrap().world;
+    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&read(format!("{dir}/../../layouts/{name}.areas.json"))).unwrap()).unwrap();
+    signalbox_core::world::World::from_file(w).unwrap()
+}
+
+/// Polish spec H3: Gretz's headcodes (up to 8 characters) fit the simplifier's
+/// Train column (the panel starts wider for them); none is cut short.
+#[test]
+fn the_simplifier_fits_the_layouts_longest_headcode() {
+    let mut r = Rig::in_game(converted("gretz-armainvilliers"), Some("Gretz"));
+    let out = r.frame();
+    click_text(&mut r, &out, "SIMPLIFIER");
+    r.frame();
+    let out = r.frame();
+    let side = side_texts(&r, &out);
+    let l = r.ui.core.game().unwrap().layout().unwrap().clone();
+    let most = l.simplifier.iter().map(|x| x.headcode.chars().count()).max().unwrap();
+    assert_eq!(most, 8, "Gretz's longest, `W118412a`");
+    assert!(side.iter().any(|t| t.chars().count() == most && l.simplifier.iter().any(|x| x.headcode == *t)), "{side:?}");
+    assert!(side.iter().all(|t| !t.ends_with('…')), "nothing cut short: {side:?}");
+    assert!(1280.0 - r.ui.diagram_rect().unwrap().max.x > 398.0, "the panel grew");
+}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-ui --test screens the_simplifier_fits`
Expected: FAIL — `W118412a` shows as `W118…`.

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index f5e40c8..3a37a25 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -43,10 +43,24 @@ const SIMPLIFIER_WIDTH: f32 = {
     }
     w
 };
-/// The side panel's least width: the simplifier's columns plus the panel's
-/// margins and a scroll bar, so the table never scrolls sideways (its
-/// header would slip off its columns) and the tabs never resize the panel.
-const SIDE_W: f32 = SIMPLIFIER_WIDTH + 24.0;
+/// The side panel's margins and a scroll bar, beside the simplifier.
+const SIDE_PAD: f32 = 24.0;
+/// The side panel's width with the shortest headcodes: the simplifier's
+/// columns and `SIDE_PAD`, so the tabs never resize the panel.
+const SIDE_W: f32 = SIMPLIFIER_WIDTH + SIDE_PAD;
+
+/// The simplifier's columns with the Train column at least `train_w` wide
+/// (polish spec H3: Gretz's 7-character headcodes fit, not `118…`).
+pub fn simplifier_columns(train_w: f32) -> [f32; 8] {
+    let mut c = SIMPLIFIER_COLUMNS;
+    c[0] = c[0].max(train_w.ceil());
+    c
+}
+
+/// The columns and the gaps between them.
+pub fn table_width(cols: &[f32; 8]) -> f32 {
+    cols.iter().sum::<f32>() + CELL_GAP * (cols.len() - 1) as f32
+}
 /// The enquiry window opens this far right of and below where it was asked for.
 const ENQUIRY_OFFSET_PX: f32 = 16.0;
 /// The top bar's fixed widths (polish spec M5): the clock state (`paused`,
@@ -108,6 +122,8 @@ pub struct UiApp {
     /// The game drawn last frame; another (or the lobby) forgets the
     /// enquiry, the search and the simplifier lines.
     shown_game: Option<String>,
+    /// The simplifier's columns for the layout shown (polish spec H3).
+    simplifier_cols: [f32; 8],
     /// The simplifier's lines (each marked if it is its row's first) for
     /// (layout generation, search).
     simplifier_lines: Option<((u64, String), Vec<(Line, bool)>)>,
@@ -145,6 +161,7 @@ impl UiApp {
             enquiry: None,
             enquiry_at: None,
             shown_game: None,
+            simplifier_cols: SIMPLIFIER_COLUMNS,
             simplifier_lines: None,
             placement: None,
             simplifier_scroll: None,
@@ -391,7 +408,11 @@ impl UiApp {
 
     fn game(&mut self, ui: &mut Ui, now: f64) {
         self.top_bar(ui, now);
-        egui::Panel::right("side").default_size(SIDE_W).min_size(SIDE_W).show(ui, |ui| self.side(ui, now));
+        // The panel fits the simplifier for the longest headcode it shows (polish
+        // spec H3); a layout that needs it wider starts it wider.
+        self.simplifier_cols = simplifier_columns(self.train_column_w(ui));
+        let side_w = table_width(&self.simplifier_cols) + SIDE_PAD;
+        egui::Panel::right(egui::Id::new(("side", side_w.round() as i32))).default_size(side_w).min_size(SIDE_W).show(ui, |ui| self.side(ui, now));
         // After the side panel, so a tab clicked this frame is told at once.
         let tab = match self.side_tab {
             SideTab::Trains => "trains",
@@ -533,6 +554,16 @@ impl UiApp {
         }
     }
 
+    /// How wide the simplifier's Train column must be for the longest
+    /// headcode the layout shows (as displayed), in the monospace font.
+    fn train_column_w(&self, ui: &Ui) -> f32 {
+        let Some(l) = self.core.game().and_then(|g| g.layout()) else { return 0.0 };
+        let longest = l.simplifier.iter().map(|r| simplifier::shown(l, &r.headcode)).max_by_key(|h| h.chars().count());
+        let Some(h) = longest else { return 0.0 };
+        let font = egui::TextStyle::Monospace.resolve(ui.style());
+        ui.fonts_mut(|f| f.layout_no_wrap(h.to_string(), font, Color32::WHITE).size().x) + 2.0
+    }
+
     /// In a tutorial the lesson box on top; then the train list or the
     /// simplifier; below them the alarms, always in view.
     fn side(&mut self, ui: &mut Ui, now: f64) {
@@ -689,7 +720,9 @@ impl UiApp {
         let v = g.view();
         let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
         let header = ["Train", "Late", "From", "To", "At", "Plat", "Arr", "Dep"];
-        simplifier_row(ui, row_h, header.map(|h| RichText::new(h).strong()));
+        let cols = self.simplifier_cols;
+        let names = g.names();
+        simplifier_row(ui, row_h, header.map(|h| RichText::new(h).strong()), &cols, [""; 8].map(String::from));
         let mut area = egui::ScrollArea::vertical().id_salt("simplifier").max_height(height);
         if let Some(line) = self.simplifier_scroll.take() {
             area = area.vertical_scroll_offset(line as f32 * (row_h + ui.spacing().item_spacing.y));
@@ -708,7 +741,19 @@ impl UiApp {
                     RichText::new(&line.arr),
                     RichText::new(&line.dep),
                 ];
-                simplifier_row(ui, row_h, cells);
+                // Every cell's whole text on hover, places by name (polish spec H3, M2).
+                let place = |p: &str| if p.is_empty() { String::new() } else { names.place(p).to_string() };
+                let hovers = [
+                    line.shown.clone(),
+                    String::new(),
+                    place(&line.from),
+                    place(&line.to),
+                    place(&line.place),
+                    line.platform.clone(),
+                    String::new(),
+                    String::new(),
+                ];
+                simplifier_row(ui, row_h, cells, &cols, hovers);
             }
         });
     }
@@ -930,14 +975,18 @@ fn mark(ui: &Ui, r: &Response, on: bool, now: f64) {
     }
 }
 
-/// One simplifier line in fixed-width cells.
-fn simplifier_row(ui: &mut Ui, row_h: f32, cells: [RichText; 8]) {
+/// One simplifier line in the cells `cols` give, each with its hover text
+/// (none where empty).
+fn simplifier_row(ui: &mut Ui, row_h: f32, cells: [RichText; 8], cols: &[f32; 8], hovers: [String; 8]) {
     ui.horizontal(|ui| {
         ui.spacing_mut().item_spacing.x = CELL_GAP;
-        for (text, w) in cells.into_iter().zip(SIMPLIFIER_COLUMNS) {
+        for ((text, w), hover) in cells.into_iter().zip(*cols).zip(hovers) {
             ui.allocate_ui_with_layout(vec2(w, row_h), Layout::left_to_right(Align::Center), |ui| {
                 ui.set_min_width(w);
-                ui.add(egui::Label::new(text).truncate());
+                let r = ui.add(egui::Label::new(text).truncate());
+                if !hover.is_empty() {
+                    r.on_hover_text(hover);
+                }
             });
         }
     });
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Browser client": replace "The side panel's minimum width is 398 pt (it fits the simplifier)." with "The
side panel starts as wide as the simplifier for the layout's longest displayed headcode (polish spec H3)."

- [ ] **Step 6: Commit**

```bash
git add crates/client-ui crates/client-ui/src/screens.rs CLAUDE.md
git commit -m "feat(client-ui): the simplifier fits the layout's longest headcode; cells show their whole text on hover"
```

---

### Task 18: The side panel hides and narrows; Fit follows the window (UI review H7)

Spec §10 H7 (U3, U6). Hide panel / Show panel, a 240 pt minimum with the simplifier scrolling sideways, and an
untouched Fit refitted when the diagram changes size.

**Files:**
- Modify: `crates/client-ui/src/screens.rs`
- Modify: `CLAUDE.md`
- Test: `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: Task 17's `table_width`, Task 13's right-aligned button group.
- Produces: `UiApp.{side_open, cam_moved, fit_size}`; `SIDE_MIN_W = 240.0`; the screens `Rig.size` (window size).

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-ui/tests/screens.rs b/crates/client-ui/tests/screens.rs
index eea8b53..44e18fd 100644
--- a/crates/client-ui/tests/screens.rs
+++ b/crates/client-ui/tests/screens.rs
@@ -19,6 +19,8 @@ struct Rig {
     events: Vec<Event>,
     /// Lobby frames the app sent (game frames go to the game).
     lobby_sent: Vec<LobbyMsg>,
+    /// The window, 1280 × 800 unless a test resizes it.
+    size: egui::Vec2,
 }
 
 impl Rig {
@@ -37,7 +39,7 @@ impl Rig {
             Some(st) => UiApp::with_store(core, Box::new(st)),
             None => UiApp::new(core),
         };
-        let mut r = Rig { ctx: egui::Context::default(), ui, h, game, t: 0.0, events: vec![], lobby_sent: vec![] };
+        let mut r = Rig { ctx: egui::Context::default(), ui, h, game, t: 0.0, events: vec![], lobby_sent: vec![], size: vec2(1280.0, 800.0) };
         r.frame();
         r.lobby_sent.clear();
         r.h.push(ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")] }] }));
@@ -70,7 +72,7 @@ impl Rig {
     fn frame(&mut self) -> FullOutput {
         self.t += 0.1;
         let input = RawInput {
-            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0))),
+            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
             time: Some(self.t),
             events: std::mem::take(&mut self.events),
             ..RawInput::default()
@@ -898,3 +900,38 @@ fn the_simplifier_fits_the_layouts_longest_headcode() {
     assert!(side.iter().all(|t| !t.ends_with('…')), "nothing cut short: {side:?}");
     assert!(1280.0 - r.ui.diagram_rect().unwrap().max.x > 398.0, "the panel grew");
 }
+
+/// Polish spec H7: the side panel can be hidden and dragged narrower, and
+/// an untouched Fit follows the window's size; a view the player has moved
+/// is left alone.
+#[test]
+fn the_panel_hides_and_the_fit_follows_the_window() {
+    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
+    r.size = vec2(1024.0, 700.0);
+    r.frame();
+    r.frame();
+    let narrow = (r.ui.diagram_rect().unwrap(), r.ui.camera().unwrap());
+    let out = r.frame();
+    click_text(&mut r, &out, "Hide panel");
+    r.frame();
+    let out = r.frame();
+    let wide = r.ui.diagram_rect().unwrap();
+    assert!(wide.width() > narrow.0.width() + 200.0, "{wide:?} vs {:?}", narrow.0);
+    assert!(r.ui.camera().unwrap().scale > narrow.1.scale, "fitted again, larger");
+    assert!(side_texts(&r, &out).iter().all(|t| t != "TRAINS"));
+    click_text(&mut r, &out, "Show panel");
+    // A moved view stays where the player put it.
+    let start = r.at(150.0, 0.0);
+    r.events.push(Event::PointerMoved(start));
+    r.events.push(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::default() });
+    r.frame();
+    r.events.push(Event::PointerMoved(start + vec2(40.0, 0.0)));
+    r.frame();
+    r.events.push(Event::PointerButton { pos: start + vec2(40.0, 0.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::default() });
+    r.frame();
+    let moved = r.ui.camera().unwrap();
+    r.size = vec2(1280.0, 800.0);
+    r.frame();
+    r.frame();
+    assert_eq!(r.ui.camera().unwrap(), moved, "not refitted after a pan");
+}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-ui --test screens the_panel_hides`
Expected: FAIL — no `Hide panel`.

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index 3a37a25..7ca03c7 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -33,22 +33,11 @@ const ZOOM_PER_POINT: f32 = 1.0 / 200.0;
 const SIMPLIFIER_COLUMNS: [f32; 8] = [38.0, 26.0, 50.0, 50.0, 56.0, 48.0, 46.0, 46.0];
 /// Between two simplifier cells.
 const CELL_GAP: f32 = 2.0;
-/// The simplifier's columns and the gaps between them.
-const SIMPLIFIER_WIDTH: f32 = {
-    let mut w = CELL_GAP * (SIMPLIFIER_COLUMNS.len() - 1) as f32;
-    let mut i = 0;
-    while i < SIMPLIFIER_COLUMNS.len() {
-        w += SIMPLIFIER_COLUMNS[i];
-        i += 1;
-    }
-    w
-};
+/// The side panel can be dragged this narrow (polish spec H7); the
+/// simplifier then scrolls sideways, its header with it.
+const SIDE_MIN_W: f32 = 240.0;
 /// The side panel's margins and a scroll bar, beside the simplifier.
 const SIDE_PAD: f32 = 24.0;
-/// The side panel's width with the shortest headcodes: the simplifier's
-/// columns and `SIDE_PAD`, so the tabs never resize the panel.
-const SIDE_W: f32 = SIMPLIFIER_WIDTH + SIDE_PAD;
-
 /// The simplifier's columns with the Train column at least `train_w` wide
 /// (polish spec H3: Gretz's 7-character headcodes fit, not `118…`).
 pub fn simplifier_columns(train_w: f32) -> [f32; 8] {
@@ -99,6 +88,12 @@ pub struct UiApp {
     cam: Option<Camera>,
     /// (game, area) the camera was fitted for; a change fits again.
     fitted: Option<(String, Option<String>)>,
+    /// The diagram's size when it was last fitted, and whether the player has
+    /// panned or zoomed since: an untouched fit follows a resize (polish spec H7).
+    fit_size: Option<egui::Vec2>,
+    cam_moved: bool,
+    /// The side panel is shown (polish spec H7: it can be hidden).
+    side_open: bool,
     diagram: Option<Rect>,
     /// What the open right-click menu is about.
     menu_target: Option<Target>,
@@ -148,6 +143,9 @@ impl UiApp {
             scene_key: None,
             cam: None,
             fitted: None,
+            fit_size: None,
+            cam_moved: false,
+            side_open: true,
             diagram: None,
             menu_target: None,
             headcode: String::new(),
@@ -412,7 +410,12 @@ impl UiApp {
         // spec H3); a layout that needs it wider starts it wider.
         self.simplifier_cols = simplifier_columns(self.train_column_w(ui));
         let side_w = table_width(&self.simplifier_cols) + SIDE_PAD;
-        egui::Panel::right(egui::Id::new(("side", side_w.round() as i32))).default_size(side_w).min_size(SIDE_W).show(ui, |ui| self.side(ui, now));
+        if self.side_open {
+            egui::Panel::right(egui::Id::new(("side", side_w.round() as i32)))
+                .default_size(side_w)
+                .min_size(SIDE_MIN_W)
+                .show(ui, |ui| self.side(ui, now));
+        }
         // After the side panel, so a tab clicked this frame is told at once.
         let tab = match self.side_tab {
             SideTab::Trains => "trains",
@@ -450,6 +453,7 @@ impl UiApp {
         let mut settings = self.settings;
         let mut confirm_release = self.confirm_release && holding;
         let mut refit = false;
+        let mut side_open = self.side_open;
         egui::Panel::top("bar").show(ui, |ui| {
             ui.horizontal(|ui| {
                 ui.label(RichText::new(title).strong());
@@ -506,6 +510,10 @@ impl UiApp {
                         ui.checkbox(&mut settings.numbers, "Signal numbers");
                     });
                     mark(ui, &menu.response, marked("settings"), now);
+                    // Polish spec H7: the panel can make way for the diagram.
+                    if ui.button(if side_open { "Hide panel" } else { "Show panel" }).clicked() {
+                        side_open = !side_open;
+                    }
                     if ui.button("Fit").clicked() {
                         refit = true;
                     }
@@ -545,6 +553,7 @@ impl UiApp {
             });
         });
         self.confirm_release = confirm_release;
+        self.side_open = side_open;
         if refit {
             self.fitted = None;
         }
@@ -722,39 +731,42 @@ impl UiApp {
         let header = ["Train", "Late", "From", "To", "At", "Plat", "Arr", "Dep"];
         let cols = self.simplifier_cols;
         let names = g.names();
-        simplifier_row(ui, row_h, header.map(|h| RichText::new(h).strong()), &cols, [""; 8].map(String::from));
-        let mut area = egui::ScrollArea::vertical().id_salt("simplifier").max_height(height);
-        if let Some(line) = self.simplifier_scroll.take() {
-            area = area.vertical_scroll_offset(line as f32 * (row_h + ui.spacing().item_spacing.y));
-        }
-        area.show_rows(ui, row_h, lines.len(), |ui, range| {
-            for (line, first) in &lines[range] {
-                let late = if *first { simplifier::lateness(v, &line.headcode) } else { None };
-                let late = late.as_deref().unwrap_or("");
-                let cells = [
-                    RichText::new(&line.shown).monospace().color(paint::HEADCODE),
-                    RichText::new(late).color(if late == "OT" { paint::LABEL } else { ALARM }),
-                    RichText::new(&line.from),
-                    RichText::new(&line.to),
-                    RichText::new(&line.place),
-                    RichText::new(&line.platform),
-                    RichText::new(&line.arr),
-                    RichText::new(&line.dep),
-                ];
-                // Every cell's whole text on hover, places by name (polish spec H3, M2).
-                let place = |p: &str| if p.is_empty() { String::new() } else { names.place(p).to_string() };
-                let hovers = [
-                    line.shown.clone(),
-                    String::new(),
-                    place(&line.from),
-                    place(&line.to),
-                    place(&line.place),
-                    line.platform.clone(),
-                    String::new(),
-                    String::new(),
-                ];
-                simplifier_row(ui, row_h, cells, &cols, hovers);
+        // Narrower than its columns, the table scrolls sideways, header and all.
+        egui::ScrollArea::horizontal().id_salt("simplifier_wide").show(ui, |ui| {
+            simplifier_row(ui, row_h, header.map(|h| RichText::new(h).strong()), &cols, [""; 8].map(String::from));
+            let mut area = egui::ScrollArea::vertical().id_salt("simplifier").max_height(height);
+            if let Some(line) = self.simplifier_scroll.take() {
+                area = area.vertical_scroll_offset(line as f32 * (row_h + ui.spacing().item_spacing.y));
             }
+            area.show_rows(ui, row_h, lines.len(), |ui, range| {
+                for (line, first) in &lines[range] {
+                    let late = if *first { simplifier::lateness(v, &line.headcode) } else { None };
+                    let late = late.as_deref().unwrap_or("");
+                    let cells = [
+                        RichText::new(&line.shown).monospace().color(paint::HEADCODE),
+                        RichText::new(late).color(if late == "OT" { paint::LABEL } else { ALARM }),
+                        RichText::new(&line.from),
+                        RichText::new(&line.to),
+                        RichText::new(&line.place),
+                        RichText::new(&line.platform),
+                        RichText::new(&line.arr),
+                        RichText::new(&line.dep),
+                    ];
+                    // Every cell's whole text on hover, places by name (polish spec H3, M2).
+                    let place = |p: &str| if p.is_empty() { String::new() } else { names.place(p).to_string() };
+                    let hovers = [
+                        line.shown.clone(),
+                        String::new(),
+                        place(&line.from),
+                        place(&line.to),
+                        place(&line.place),
+                        line.platform.clone(),
+                        String::new(),
+                        String::new(),
+                    ];
+                    simplifier_row(ui, row_h, cells, &cols, hovers);
+                }
+            });
         });
     }
 
@@ -832,21 +844,29 @@ impl UiApp {
             painter.text(rect.center(), Align2::CENTER_CENTER, msg, FontId::proportional(16.0), paint::LABEL);
             return;
         };
-        if self.fitted.as_ref() != Some(&fit_key) || self.cam.is_none() {
+        // Fit again for a new game or area, after Fit, and when the diagram
+        // changes size while the player has not moved the view (polish spec H7).
+        let resized = self.fit_size.is_some_and(|s| (s - rect.size()).length() > 0.5);
+        if self.fitted.as_ref() != Some(&fit_key) || self.cam.is_none() || (resized && !self.cam_moved) {
             self.cam = Some(scene.fit_bounds().map_or(Camera { centre: rect.center(), scale: 1.0 }, |b| Camera::fit(b, rect)));
             self.fitted = Some(fit_key);
+            self.cam_moved = false;
         }
+        self.fit_size = Some(rect.size());
         let Some(cam) = self.cam.as_mut() else { return };
         if resp.dragged_by(PointerButton::Primary) {
             cam.pan(resp.drag_delta());
+            self.cam_moved = true;
         }
         if let Some(p) = resp.hover_pos() {
             let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
             if scroll != 0.0 {
                 cam.zoom_at(rect, p, (scroll * ZOOM_PER_POINT).exp());
+                self.cam_moved = true;
             }
             if zoom != 1.0 {
                 cam.zoom_at(rect, p, zoom);
+                self.cam_moved = true;
             }
         }
         let cam = *cam;
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Browser client": "The side panel can be hidden (top bar) or dragged to 240 pt; an untouched Fit follows
the window's size (polish spec H7)."

- [ ] **Step 6: Commit**

```bash
git add crates/client-ui crates/client-ui/src/screens.rs CLAUDE.md
git commit -m "feat(client-ui): hide the side panel or narrow it; an untouched Fit follows the window"
```

---

### Task 19: Zoom: finer steps, buttons and keys, signals that grow (UI review M11)

Spec §10 M11 (U16). Merges with the placer: the grown glyphs are kept clear as before, so the legibility
measurement still holds at 2× and 4× Fit.

**Files:**
- Modify: `crates/client-ui/src/hit.rs`
- Modify: `crates/client-ui/src/paint.rs`
- Modify: `crates/client-ui/src/screens.rs`
- Modify: `CLAUDE.md`
- Test: `crates/client-ui/tests/hit.rs`
- Test: `crates/client-ui/tests/paint.rs`
- Test: `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: Task 2's `number_alts` (gains a glyph argument), `number_px`, `signal_disc`, `auto_button`.
- Produces: `paint::{glyph(f32) -> f32, GLYPH_FROM_SCALE, GLYPH_MAX}`; `number_alts(base, disc, facing, track_w, has_auto, g)`;
  `pub screens::{ZOOM_PER_POINT = 1/600, ZOOM_STEP = 1.25, VIEW_HINT}`. The paint tests' `Rig` clamps its camera to
  `GLYPH_FROM_SCALE` so they keep testing base-size glyphs.

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-ui/tests/hit.rs b/crates/client-ui/tests/hit.rs
index ee61821..20cc727 100644
--- a/crates/client-ui/tests/hit.rs
+++ b/crates/client-ui/tests/hit.rs
@@ -83,7 +83,9 @@ fn the_auto_button_is_its_own_target() {
     let (sc, cam, screen) = setup(Some("West"));
     let w1 = sc.signals.iter().find(|s| s.name == "W1").unwrap();
     let c = auto_button(&cam, screen, w1).expect("W1 is a controlled signal with a route");
-    assert_eq!(c, signal_disc(&cam, screen, w1) + vec2(AUTO_AHEAD_PX, 0.0));
+    // Zoomed in past the track's widest, the button sits further out with
+    // the bigger glyphs (polish spec M11).
+    assert_eq!(c, signal_disc(&cam, screen, w1) + vec2(AUTO_AHEAD_PX * client_ui::paint::glyph(cam.scale), 0.0));
     assert_eq!(hit_test(&sc, None, &cam, screen, c), hit(Target::Auto(s("W1")), true));
     assert_eq!(hit_test(&sc, None, &cam, screen, signal_disc(&cam, screen, w1)), hit(Target::Signal(s("W1")), true));
     let mut l = layout_for(Some("West"));
diff --git a/crates/client-ui/tests/paint.rs b/crates/client-ui/tests/paint.rs
index e044924..aeb1ba8 100644
--- a/crates/client-ui/tests/paint.rs
+++ b/crates/client-ui/tests/paint.rs
@@ -68,7 +68,10 @@ impl Rig {
 
     fn of(layout: Layout, view: View) -> Rig {
         let sc = Scene::build(&layout).unwrap();
-        let cam = Camera::fit(sc.all.unwrap(), screen());
+        // Fitted, but no closer than the zoom where signal glyphs start to grow
+        // (polish spec M11): these tests check the glyphs at their base size.
+        let mut cam = Camera::fit(sc.all.unwrap(), screen());
+        cam.scale = cam.scale.min(GLYPH_FROM_SCALE);
         let names = Names::new(&layout);
         Rig { layout, sc, cam, view, names, aspects: AspectMode::RedGreen, numbers: true }
     }
@@ -787,7 +790,7 @@ fn a_numbers_other_spots_hug_the_track_then_mirror_it() {
     // Travel to the right: left of travel is up the screen.
     let (base, f) = (pos2(100.0, 100.0), vec2(1.0, 0.0));
     let disc = base + vec2(0.0, -POST_PX) + f * (HOOK_PX + LAMP_R);
-    let alts = number_alts(base, disc, f, 6.0, false);
+    let alts = number_alts(base, disc, f, 6.0, false, 1.0);
     let side = 3.0 + NUMBER_CLEAR_PX;
     assert_eq!(alts[0], (pos2(98.0, 100.0 - side), corner(vec2(-1.0, -1.0))), "behind the post, just clear of the track");
     assert_eq!(alts[0].1, Align2::RIGHT_BOTTOM);
@@ -796,7 +799,7 @@ fn a_numbers_other_spots_hug_the_track_then_mirror_it() {
     assert_eq!(alts[4], (pos2(98.0, 100.0 + side), Align2::RIGHT_TOP), "the other side of the track");
     assert_eq!(alts[5], (pos2(102.0, 100.0 + side), Align2::LEFT_TOP));
     // With a ○A, the spot ahead clears the button.
-    let with_auto = number_alts(base, disc, f, 6.0, true);
+    let with_auto = number_alts(base, disc, f, 6.0, true, 1.0);
     assert!(with_auto[1].0.x >= disc.x + client_ui::hit::AUTO_AHEAD_PX + AUTO_R);
 }
 
@@ -829,3 +832,19 @@ fn berths_show_display_headcodes_and_fit_them() {
     let d = r.idle();
     assert!(d.texts.iter().any(|t| t.text == "202") && d.texts.iter().all(|t| t.text != "202/163"));
 }
+
+/// Polish spec M11: zoomed in past the track's widest, the signal glyphs
+/// grow with the zoom, up to twice their size; numbers too.
+#[test]
+fn signal_glyphs_grow_when_zoomed_in() {
+    assert_eq!((glyph(0.5), glyph(GLYPH_FROM_SCALE)), (1.0, 1.0));
+    assert!((glyph(GLYPH_FROM_SCALE * 1.5) - 1.5).abs() < 1e-5);
+    assert_eq!((glyph(100.0), glyph(f32::NAN)), (GLYPH_MAX, 1.0));
+    assert_eq!(number_px(GLYPH_FROM_SCALE * 2.0), Some(NUMBER_MAX_PX * 2.0));
+    let mut r = Rig::new(Some("West"));
+    r.cam.scale = GLYPH_FROM_SCALE * 2.0;
+    let d = r.idle();
+    assert!(circles(&d).iter().any(|k| close(k.0, r.disc("W1")) && k.1 == LAMP_R * 2.0), "{:?}", circles(&d));
+    let n = d.texts.iter().find(|t| t.text == "TAW1").unwrap();
+    assert_eq!(n.size, NUMBER_MAX_PX * 2.0);
+}
diff --git a/crates/client-ui/tests/screens.rs b/crates/client-ui/tests/screens.rs
index 44e18fd..4706ada 100644
--- a/crates/client-ui/tests/screens.rs
+++ b/crates/client-ui/tests/screens.rs
@@ -935,3 +935,21 @@ fn the_panel_hides_and_the_fit_follows_the_window() {
     r.frame();
     assert_eq!(r.ui.camera().unwrap(), moved, "not refitted after a pan");
 }
+
+/// Polish spec M11: + and - buttons and keys zoom in steps about the
+/// middle, and the diagram says how to move it until the player has.
+#[test]
+fn the_diagram_zooms_with_buttons_and_keys_and_says_how() {
+    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
+    let out = r.frame();
+    assert!(has_text(&out, client_ui::screens::VIEW_HINT));
+    let fit = r.ui.camera().unwrap().scale;
+    click_text(&mut r, &out, "+");
+    let zoomed = r.ui.camera().unwrap().scale;
+    assert!((zoomed / fit - client_ui::screens::ZOOM_STEP).abs() < 1e-4, "{fit} → {zoomed}");
+    r.events.push(Event::Key { key: Key::Minus, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::default() });
+    r.frame();
+    assert!((r.ui.camera().unwrap().scale - fit).abs() < 1e-3, "back out");
+    assert!(!has_text(&r.frame(), client_ui::screens::VIEW_HINT), "moved: the hint goes");
+    assert!((client_ui::screens::ZOOM_PER_POINT * 100.0).exp() < 1.2, "a wheel notch is a small step");
+}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-ui --test paint signal_glyphs -p signalbox-client-ui --test screens zooms`
Expected: compile errors (`glyph`, `VIEW_HINT`).

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-ui/src/hit.rs b/crates/client-ui/src/hit.rs
index 155fc27..050e923 100644
--- a/crates/client-ui/src/hit.rs
+++ b/crates/client-ui/src/hit.rs
@@ -8,7 +8,7 @@ use egui::{Pos2, Rect, vec2};
 use protocol::View;
 
 use crate::camera::Camera;
-use crate::paint::{AUTO_R, HOOK_PX, LAMP_R, POST_PX, left_of, number_px};
+use crate::paint::{AUTO_R, HOOK_PX, LAMP_R, POST_PX, glyph, left_of, number_px};
 use crate::scene::{BerthMark, Scene, SignalMark, project};
 
 /// How near (pixels) the pointer must be to a signal, exit, points or track.
@@ -31,7 +31,8 @@ pub fn signal_disc(cam: &Camera, screen: Rect, s: &SignalMark) -> Pos2 {
     if s.facing == egui::Vec2::ZERO {
         return cam.to_screen(screen, s.at);
     }
-    cam.to_screen(screen, s.base) + left_of(s.facing) * POST_PX + s.facing * (HOOK_PX + LAMP_R)
+    let g = glyph(cam.scale);
+    cam.to_screen(screen, s.base) + left_of(s.facing) * (POST_PX * g) + s.facing * ((HOOK_PX + LAMP_R) * g)
 }
 
 /// Where a controlled signal's ○A button is: `AUTO_AHEAD_PX` ahead of its
@@ -42,7 +43,7 @@ pub fn auto_button(cam: &Camera, screen: Rect, s: &SignalMark) -> Option<Pos2> {
         return None;
     }
     let ahead = if s.facing == egui::Vec2::ZERO { vec2(1.0, 0.0) } else { s.facing };
-    Some(signal_disc(cam, screen, s) + ahead * AUTO_AHEAD_PX)
+    Some(signal_disc(cam, screen, s) + ahead * (AUTO_AHEAD_PX * glyph(cam.scale)))
 }
 
 /// How far ahead of its lamp a signal's ○A button sits.
@@ -84,7 +85,7 @@ where
 pub fn hit_test(scene: &Scene, view: Option<&View>, cam: &Camera, screen: Rect, p: Pos2) -> Option<Hit> {
     let at = |q: Pos2| cam.to_screen(screen, q);
     // The ○A buttons first: they sit just ahead of their lamps.
-    let button = |s: &SignalMark| auto_button(cam, screen, s).map(|c| c.distance(p)).filter(|d| *d <= AUTO_R + 2.0);
+    let button = |s: &SignalMark| auto_button(cam, screen, s).map(|c| c.distance(p)).filter(|d| *d <= AUTO_R * glyph(cam.scale) + 2.0);
     if let Some(s) = scene.signals.iter().filter_map(|s| Some((s, button(s)?))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(s, _)| s) {
         return Some(Hit { target: Target::Auto(s.name.clone()), clickable: s.operable });
     }
diff --git a/crates/client-ui/src/paint.rs b/crates/client-ui/src/paint.rs
index 007ea33..092ead7 100644
--- a/crates/client-ui/src/paint.rs
+++ b/crates/client-ui/src/paint.rs
@@ -86,6 +86,18 @@ pub const NUMBER_CLEAR_PX: f32 = 1.5;
 pub const AUTO_LETTER_PX: f32 = 9.0;
 pub const AUTO_LETTER_GAP_PX: f32 = 1.0;
 
+/// Signal glyphs (lamp, post, ○A, numbers, platform text) grow with the
+/// zoom once the track is at its widest, up to `GLYPH_MAX` times their size
+/// (polish spec M11): zoomed in, a signal is no longer a speck.
+pub const GLYPH_FROM_SCALE: f32 = TRACK_MAX_PX / TRACK_UNITS;
+pub const GLYPH_MAX: f32 = 2.0;
+
+/// How much bigger than their base size the signal glyphs are at `scale`.
+pub fn glyph(scale: f32) -> f32 {
+    let g = scale / GLYPH_FROM_SCALE;
+    if g.is_finite() { g.clamp(1.0, GLYPH_MAX) } else { 1.0 }
+}
+
 pub fn track_w(scale: f32) -> f32 {
     let w = TRACK_UNITS * scale;
     if w.is_finite() { w.clamp(TRACK_MIN_PX, TRACK_MAX_PX) } else { TRACK_MIN_PX }
@@ -93,7 +105,7 @@ pub fn track_w(scale: f32) -> f32 {
 
 /// Signal numbers' text size at this zoom; `None` when too small to read.
 pub fn number_px(scale: f32) -> Option<f32> {
-    let px = (NUMBER_UNITS * scale).min(NUMBER_MAX_PX);
+    let px = (NUMBER_UNITS * scale).min(NUMBER_MAX_PX * glyph(scale));
     (px >= NUMBER_MIN_PX).then_some(px)
 }
 
@@ -380,10 +392,12 @@ fn signal_shapes(d: &mut Drawing, s: &SignalMark, cam: &Camera, screen: Rect, st
     let routes: Vec<RouteState> =
         s.routes.iter().filter_map(|r| st.view.and_then(|v| v.routes.get(r)).map(|rv| rv.state)).collect();
     let disc = signal_disc(cam, screen, s);
+    let g = glyph(cam.scale);
+    let lamp = LAMP_R * g;
     if s.facing != Vec2::ZERO {
         let base = cam.to_screen(screen, s.base);
-        let top = base + left_of(s.facing) * POST_PX;
-        let hook = top + s.facing * HOOK_PX;
+        let top = base + left_of(s.facing) * (POST_PX * g);
+        let hook = top + s.facing * (HOOK_PX * g);
         // Fringe signals are grey whatever is set from them.
         let colour = if s.fringe {
             FRINGE
@@ -402,37 +416,37 @@ fn signal_shapes(d: &mut Drawing, s: &SignalMark, cam: &Camera, screen: Rect, st
     }
     let cancelling = routes.contains(&RouteState::Cancelling);
     if s.fringe {
-        d.shapes.push(Shape::circle_filled(disc, LAMP_R, FRINGE));
+        d.shapes.push(Shape::circle_filled(disc, lamp, FRINGE));
     } else if cancelling && !blink_on(st.time) {
         // Approach locking timing out: the lamp flashes red.
-        d.shapes.push(Shape::circle_stroke(disc, LAMP_R, Stroke::new(1.0, RED)));
+        d.shapes.push(Shape::circle_stroke(disc, lamp, Stroke::new(1.0, RED)));
     } else {
         let (first, second) = signal_lamps(aspect, st.aspects);
-        d.shapes.push(Shape::circle_filled(disc, LAMP_R, first));
+        d.shapes.push(Shape::circle_filled(disc, lamp, first));
         if let Some(c) = second {
-            d.shapes.push(Shape::circle_filled(disc + s.facing * (LAMP_R * 2.2), LAMP_R, c));
+            d.shapes.push(Shape::circle_filled(disc + s.facing * (lamp * 2.2), lamp, c));
         }
     }
     if st.selected == Some(s.name.as_str()) && blink_on(st.time) {
-        d.shapes.push(Shape::circle_stroke(disc, LAMP_R + 3.5, Stroke::new(2.0, SELECT)));
+        d.shapes.push(Shape::circle_stroke(disc, lamp + 3.5, Stroke::new(2.0, SELECT)));
     }
     if st.exits.contains(&ExitName::Signal(s.name.clone())) {
-        d.shapes.push(Shape::circle_stroke(disc, LAMP_R + 3.5, Stroke::new(2.0, SELECT)));
+        d.shapes.push(Shape::circle_stroke(disc, lamp + 3.5, Stroke::new(2.0, SELECT)));
     }
     if st.refused == Some(s.name.as_str()) || st.blocking == Some(s.name.as_str()) {
-        d.shapes.push(Shape::circle_stroke(disc, LAMP_R + 6.0, Stroke::new(2.0, REFUSED)));
+        d.shapes.push(Shape::circle_stroke(disc, lamp + 6.0, Stroke::new(2.0, REFUSED)));
     }
-    d.keep.rounds.push((disc, LAMP_R));
+    d.keep.rounds.push((disc, lamp));
     if s.facing != Vec2::ZERO {
         // A second yellow's spot, kept clear whatever is shown (spec P3).
-        d.keep.rounds.push((disc + s.facing * (LAMP_R * 2.2), LAMP_R));
+        d.keep.rounds.push((disc + s.facing * (lamp * 2.2), lamp));
     }
     if let (true, Some(size)) = (st.numbers, number_px(cam.scale)) {
         let side = if s.facing == Vec2::ZERO { vec2(0.0, -1.0) } else { left_of(s.facing) };
         let base = cam.to_screen(screen, s.base);
-        let alts = number_alts(base, disc, s.facing, track_w(cam.scale), auto_button(cam, screen, s).is_some());
+        let alts = number_alts(base, disc, s.facing, track_w(cam.scale), auto_button(cam, screen, s).is_some(), g);
         let text = TextItem {
-            at: disc + side * (LAMP_R + 2.0),
+            at: disc + side * (lamp + 2.0),
             anchor: anchor_towards(side),
             text: st.names.signal(&s.name),
             size,
@@ -446,17 +460,19 @@ fn signal_shapes(d: &mut Drawing, s: &SignalMark, cam: &Camera, screen: Rect, st
 /// A signal number's other spots, best first (spec §3.3): hugging the track
 /// behind the post, ahead of the lamp (past its ○A), one row further out
 /// behind and ahead, and the two spots on the other side of the track.
-/// `base` is the foot of the post and `disc` the lamp, on screen.
-pub fn number_alts(base: Pos2, disc: Pos2, facing: Vec2, track_w: f32, has_auto: bool) -> Vec<(Pos2, Align2)> {
+/// `base` is the foot of the post and `disc` the lamp, on screen; `g` the
+/// glyph size (`glyph`).
+pub fn number_alts(base: Pos2, disc: Pos2, facing: Vec2, track_w: f32, has_auto: bool, g: f32) -> Vec<(Pos2, Align2)> {
     let f = if facing == Vec2::ZERO { vec2(1.0, 0.0) } else { facing };
     let l = left_of(f);
     let side = track_w / 2.0 + NUMBER_CLEAR_PX;
-    let ahead = HOOK_PX + LAMP_R + if has_auto { AUTO_AHEAD_PX + AUTO_R } else { LAMP_R } + 2.0;
+    let (lamp, hook) = (LAMP_R * g, HOOK_PX * g);
+    let ahead = hook + lamp + if has_auto { (AUTO_AHEAD_PX + AUTO_R) * g } else { lamp } + 2.0;
     vec![
         (base + l * side - f * 2.0, corner(l - f)),
         (base + l * side + f * ahead, corner(l + f)),
-        (disc + l * (LAMP_R + 2.0) - f * (LAMP_R + 2.0), corner(l - f)),
-        (disc + l * (LAMP_R + 2.0) + f * (ahead - HOOK_PX - LAMP_R), corner(l + f)),
+        (disc + l * (lamp + 2.0) - f * (lamp + 2.0), corner(l - f)),
+        (disc + l * (lamp + 2.0) + f * (ahead - hook - lamp), corner(l + f)),
         (base - l * side - f * 2.0, corner(-l - f)),
         (base - l * side + f * 2.0, corner(-l + f)),
     ]
@@ -470,7 +486,7 @@ pub fn draw(scene: &Scene, cam: &Camera, screen: Rect, st: &PaintState) -> Drawi
     for p in &scene.platforms {
         let r = Rect::from_two_pos(to(p.rect.min), to(p.rect.max));
         d.shapes.push(Shape::rect_filled(r, CornerRadius::ZERO, PLATFORM));
-        let text = TextItem { at: r.center(), anchor: Align2::CENTER_CENTER, text: p.label.clone(), size: 9.0, colour: BG, monospace: false };
+        let text = TextItem { at: r.center(), anchor: Align2::CENTER_CENTER, text: p.label.clone(), size: 9.0 * glyph(cam.scale), colour: BG, monospace: false };
         d.movable_text(text, Role::Platform, Vec::new(), Some(r));
     }
     for t in &scene.tracks {
@@ -525,19 +541,20 @@ pub fn draw(scene: &Scene, cam: &Camera, screen: Rect, st: &PaintState) -> Drawi
             let on = st.view.is_some_and(|v| {
                 s.routes.iter().filter_map(|r| v.routes.get(r)).any(|rv| rv.auto_working && rv.state != RouteState::Cancelling)
             });
+            let auto_r = AUTO_R * glyph(cam.scale);
             // Blue where you can press it; a spectator's are grey, read-only.
             let colour = if s.operable { AUTO } else { FRINGE };
             d.shapes.push(if on {
-                Shape::circle_filled(c, AUTO_R, colour)
+                Shape::circle_filled(c, auto_r, colour)
             } else {
-                Shape::circle_stroke(c, AUTO_R, Stroke::new(1.5, colour))
+                Shape::circle_stroke(c, auto_r, Stroke::new(1.5, colour))
             });
-            d.keep.rounds.push((c, AUTO_R));
+            d.keep.rounds.push((c, auto_r));
             // The `A` outward, away from the track; else ahead, else on the inside.
             let ahead = if s.facing == Vec2::ZERO { vec2(1.0, 0.0) } else { s.facing };
             let out = left_of(ahead);
-            let gap = AUTO_R + AUTO_LETTER_GAP_PX;
-            let text = TextItem { at: c + out * gap, anchor: corner(out), text: "A".into(), size: AUTO_LETTER_PX, colour, monospace: true };
+            let gap = auto_r + AUTO_LETTER_GAP_PX;
+            let text = TextItem { at: c + out * gap, anchor: corner(out), text: "A".into(), size: AUTO_LETTER_PX * glyph(cam.scale), colour, monospace: true };
             let alts = vec![(c + ahead * gap, corner(ahead)), (c - out * gap, corner(-out))];
             d.movable_text(text, Role::AutoLetter, alts, None);
         }
@@ -593,7 +610,7 @@ fn highlight_shapes(d: &mut Drawing, scene: &Scene, cam: &Camera, screen: Rect,
         match h {
             Highlight::Signal(s) | Highlight::Exit(ExitName::Signal(s)) => {
                 if let Some(m) = signal(s) {
-                    ring(d, signal_disc(cam, screen, m), LAMP_R + HIGHLIGHT_GAP_PX + 4.0);
+                    ring(d, signal_disc(cam, screen, m), LAMP_R * glyph(cam.scale) + HIGHLIGHT_GAP_PX + 4.0);
                 }
             }
             Highlight::Exit(ExitName::Node(n)) => {
@@ -628,7 +645,7 @@ fn highlight_shapes(d: &mut Drawing, scene: &Scene, cam: &Camera, screen: Rect,
             }
             Highlight::Ui(u) => {
                 if let Some(c) = u.strip_prefix("auto:").and_then(signal).and_then(|m| auto_button(cam, screen, m)) {
-                    ring(d, c, AUTO_R + HIGHLIGHT_GAP_PX);
+                    ring(d, c, AUTO_R * glyph(cam.scale) + HIGHLIGHT_GAP_PX);
                 }
             }
         }
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index 7ca03c7..95d8653 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -25,8 +25,16 @@ use crate::scene::Scene;
 
 /// Alarms and the connection banner.
 pub const ALARM: Color32 = Color32::from_rgb(0xFF, 0x5A, 0x5A);
-/// How far one wheel "line" (egui points of scroll) zooms.
-const ZOOM_PER_POINT: f32 = 1.0 / 200.0;
+/// How far one wheel "line" (egui points of scroll) zooms: about ×1.2 a
+/// notch (polish spec M11; it was ×1.8).
+pub const ZOOM_PER_POINT: f32 = 1.0 / 600.0;
+/// One press of the zoom buttons or keys.
+pub const ZOOM_STEP: f32 = 1.25;
+/// The zoom buttons: this big, this far in from the diagram's corner.
+const ZOOM_BUTTON: f32 = 26.0;
+const ZOOM_INSET: f32 = 8.0;
+/// Until the player first moves the view, the diagram says how.
+pub const VIEW_HINT: &str = "Drag to move · wheel, + or - to zoom · Fit shows it all";
 /// Simplifier columns, in points: headcode, lateness, from, to, at,
 /// platform, arrival, departure (wide enough for `BTHNLGR`, `ML_UP` and
 /// `05:03½`).
@@ -858,6 +866,19 @@ impl UiApp {
             cam.pan(resp.drag_delta());
             self.cam_moved = true;
         }
+        // Buttons and keys zoom about the middle (polish spec M11); not while
+        // typing in the simplifier's search.
+        let keys = if ui.ctx().egui_wants_keyboard_input() {
+            0
+        } else {
+            ui.input(|i| {
+                i32::from(i.key_pressed(Key::Plus) || i.key_pressed(Key::Equals)) - i32::from(i.key_pressed(Key::Minus))
+            })
+        };
+        if keys != 0 {
+            cam.zoom_at(rect, rect.center(), ZOOM_STEP.powi(keys));
+            self.cam_moved = true;
+        }
         if let Some(p) = resp.hover_pos() {
             let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
             if scroll != 0.0 {
@@ -915,6 +936,18 @@ impl UiApp {
             None => d,
         };
         paint::paint(&painter, d);
+        if !self.cam_moved {
+            painter.text(rect.left_bottom() + vec2(ZOOM_INSET, -ZOOM_INSET), Align2::LEFT_BOTTOM, VIEW_HINT, FontId::proportional(12.0), paint::LABEL);
+        }
+        // The zoom buttons, on top of the diagram (polish spec M11).
+        let corner = |k: f32| rect.right_top() + vec2(-(ZOOM_INSET + ZOOM_BUTTON) * k, ZOOM_INSET);
+        let plus = ui.put(Rect::from_min_size(corner(2.0) - vec2(4.0, 0.0), vec2(ZOOM_BUTTON, ZOOM_BUTTON)), egui::Button::new("+"));
+        let minus = ui.put(Rect::from_min_size(corner(1.0), vec2(ZOOM_BUTTON, ZOOM_BUTTON)), egui::Button::new("-"));
+        let steps = i32::from(plus.clicked()) - i32::from(minus.clicked());
+        if let (Some(c), true) = (self.cam.as_mut(), steps != 0) {
+            c.zoom_at(rect, rect.center(), ZOOM_STEP.powi(steps));
+            self.cam_moved = true;
+        }
         match click {
             // With the enquiry on, a headcode opens its window and nothing else.
             Some(Some(t)) => match self.core.headcode_at(&t).filter(|_| self.settings.enquiry) {
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Browser client": "Zoom: ×1.2 a wheel notch, + and − buttons and keys ×1.25; signal glyphs grow with the
zoom once the track is at its widest, up to twice their size (`paint::glyph`, polish spec M11)."

- [ ] **Step 6: Commit**

```bash
git add crates/client-ui crates/client-ui/src/hit.rs crates/client-ui/src/paint.rs crates/client-ui/src/screens.rs CLAUDE.md
git commit -m "feat(client-ui): finer zoom with buttons and keys; signal glyphs grow when zoomed in"
```

---

### Task 20: Which way points lie (UI review M12)

Spec §10 M12 (U17). The unused leg is thin, the leg points move to flashes, and an unused crossover middle is thin.

**Files:**
- Modify: `crates/client-ui/src/paint.rs`
- Test: `crates/client-ui/tests/layouts.rs`
- Test: `crates/client-ui/tests/paint.rs`

**Interfaces:**
- Consumes: `PointsMark.{normal_meets, reverse_meets}`.
- Produces: `paint::{UNUSED_W = 0.4, unused_crossovers(&Scene, Option<&View>) -> BTreeSet<String>}`.

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-ui/tests/layouts.rs b/crates/client-ui/tests/layouts.rs
index 8dd5b75..d5895b7 100644
--- a/crates/client-ui/tests/layouts.rs
+++ b/crates/client-ui/tests/layouts.rs
@@ -134,3 +134,19 @@ fn every_lesson_draws_with_its_highlights() {
         }
     }
 }
+
+/// Polish spec M12: Liverpool Street's crossovers are found, and a
+/// crossover's middle stops counting as unused once an end lies over it.
+#[test]
+fn crossovers_neither_end_of_which_is_set_are_found() {
+    let mut g = Game::new(world("liverpool-st"), GameMeta { layout: "liverpool-st".into(), seed: 1 });
+    g.connect("sam");
+    let (l, mut v) = (g.layout_of("sam").unwrap(), g.view_of("sam").unwrap());
+    let sc = Scene::build(&l).unwrap();
+    let unused = client_ui::paint::unused_crossovers(&sc, Some(&v));
+    assert!(unused.len() >= 4, "{unused:?}");
+    let middle = unused.iter().next().unwrap().clone();
+    let end = sc.points.iter().find(|p| p.reverse_meets.contains(&middle)).unwrap();
+    v.points.get_mut(&end.name).unwrap().position = protocol::PointsPos::Reverse;
+    assert!(!client_ui::paint::unused_crossovers(&sc, Some(&v)).contains(&middle), "{} lies over it", end.name);
+}
diff --git a/crates/client-ui/tests/paint.rs b/crates/client-ui/tests/paint.rs
index aeb1ba8..8176e7c 100644
--- a/crates/client-ui/tests/paint.rs
+++ b/crates/client-ui/tests/paint.rs
@@ -237,15 +237,19 @@ fn points_show_the_lying_leg_whole_and_a_gap_in_the_other() {
     let (n_end, rv_end) = (short(n, c), short(rv, c));
     let legs = lines_of(&r.idle(), TRACK_FREE, w);
     assert!(has(&legs, c, n_end), "normal lies: whole up to its joint {legs:?}");
-    assert!(has(&legs, c + (rv - c) * GAP, rv_end), "reverse: from the gap");
+    // Polish spec M12: the other leg is thin as well as short of the points.
+    let thin = lines_of(&r.idle(), TRACK_FREE, w * UNUSED_W);
+    assert!(has(&thin, c + (rv - c) * GAP, rv_end), "reverse: thin, from the gap {thin:?}");
+    assert!(!has(&legs, c + (rv - c) * GAP, rv_end), "not at full width");
+    // Moving to reverse: the reverse leg flashes, whole then gapped; normal is thin.
     r.view.points.insert(s("P"), PointsView { position: PointsPos::Reverse, moving: true, locked: false });
-    let open = lines_of(&r.draw(None, &[], None, 0.0), TRACK_FREE, w);
-    assert!(has(&open, c, rv_end) && has(&open, c + (n - c) * GAP, n_end));
-    assert!(!has(&open, c, c + (n - c) * GAP), "the gap open");
-    let shut = lines_of(&r.draw(None, &[], None, 0.3), TRACK_FREE, w);
-    assert!(has(&shut, c, c + (n - c) * GAP), "while moving, the gap flashes");
+    let lit = r.draw(None, &[], None, 0.0);
+    assert!(has(&lines_of(&lit, TRACK_FREE, w), c, rv_end), "bright half: whole");
+    assert!(has(&lines_of(&lit, TRACK_FREE, w * UNUSED_W), c + (n - c) * GAP, n_end));
+    let dark = lines_of(&r.draw(None, &[], None, 0.3), TRACK_FREE, w);
+    assert!(has(&dark, c + (rv - c) * GAP, rv_end) && !has(&dark, c, rv_end), "dark half: gapped");
     r.view.points.get_mut("P").unwrap().moving = false;
-    assert!(!has(&lines_of(&r.draw(None, &[], None, 0.3), TRACK_FREE, w), c, c + (n - c) * GAP));
+    assert!(has(&lines_of(&r.draw(None, &[], None, 0.3), TRACK_FREE, w), c, rv_end), "swung: steady");
 }
 
 /// W1 faces right (+x): its post goes up (the left of travel, y grows
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-ui --test paint points_show --test layouts crossovers`
Expected: compile errors (`UNUSED_W`, `unused_crossovers` not found).

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-ui/src/paint.rs b/crates/client-ui/src/paint.rs
index 092ead7..a7db69c 100644
--- a/crates/client-ui/src/paint.rs
+++ b/crates/client-ui/src/paint.rs
@@ -65,6 +65,9 @@ pub const NUMBER_MAX_PX: f32 = 11.0;
 pub const NUMBER_MIN_PX: f32 = 7.0;
 /// Where the non-lying leg of points starts, as a fraction of its length.
 pub const GAP: f32 = 0.5;
+/// The non-lying leg, and a crossover's middle while neither end lies over
+/// it, are drawn this fraction of the track's width (polish spec M12).
+pub const UNUSED_W: f32 = 0.4;
 /// The ○A button's circle.
 pub const AUTO_R: f32 = 4.0;
 /// Headcodes: text size in pixels.
@@ -258,31 +261,53 @@ fn points_shapes(out: &mut Vec<Shape>, p: &PointsMark, to: &dyn Fn(Pos2) -> Pos2
     let c = to(p.at);
     // The toe and the lying leg carry the route; an overlap held here ends
     // in a tick at the far end of each that nothing held goes on from.
-    for (leg, meets) in [(p.toe, &p.toe_meets), lie] {
+    // While the points move, the leg they move to flashes: its first half
+    // shows only in the bright half of the blink (polish spec M12).
+    for (i, (leg, meets)) in [(p.toe, &p.toe_meets), lie].into_iter().enumerate() {
         let Some(l) = leg else { continue };
         let end = leg_end(p, c, to(l), meets);
-        bar(out, c, end, w, colour, p.fringe);
+        if i == 1 && moving && !blink_on(st.time) {
+            bar(out, c + (to(l) - c) * GAP, end, w, colour, p.fringe);
+        } else {
+            bar(out, c, end, w, colour, p.fringe);
+        }
         if overlap && !meets.iter().any(|m| held(m)) {
             tick(out, end, end - c, w);
         }
     }
+    // The other leg: thin, and short of the points (polish spec M12).
     if let (Some(o), meets) = other {
         let far = to(o);
         let end = leg_end(p, c, far, meets);
         let gap_end = c + (far - c) * GAP;
-        bar(out, gap_end, end, w, colour, p.fringe);
-        // While moving the gap flashes: closed in the dark half of the blink.
-        if moving && !blink_on(st.time) {
-            bar(out, c, gap_end, w, colour, p.fringe);
+        bar(out, gap_end, end, w * UNUSED_W, colour, p.fringe);
+    }
+}
+
+/// Sections lying beyond the non-lying leg of two or more points: the middle
+/// of a crossover neither end of which is set over it (polish spec M12).
+pub fn unused_crossovers(scene: &Scene, view: Option<&View>) -> std::collections::BTreeSet<String> {
+    let mut seen: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
+    for p in &scene.points {
+        let lying = view.and_then(|v| v.points.get(&p.name)).map_or(PointsPos::Normal, |v| v.position);
+        let other = match lying {
+            PointsPos::Normal => &p.reverse_meets,
+            PointsPos::Reverse => &p.normal_meets,
+        };
+        for m in other {
+            *seen.entry(m.as_str()).or_default() += 1;
         }
     }
+    seen.into_iter().filter(|(_, n)| *n >= 2).map(|(s, _)| s.to_string()).collect()
 }
 
-fn track_shapes(d: &mut Drawing, t: &TrackLine, to: &dyn Fn(Pos2) -> Pos2, st: &PaintState, w: f32) {
+fn track_shapes(d: &mut Drawing, t: &TrackLine, to: &dyn Fn(Pos2) -> Pos2, st: &PaintState, w: f32, unused: bool) {
     let section = |name: &str| st.view.and_then(|v| v.sections.get(name));
     let held = |name: &str| section(name).is_some_and(|s| s.held != Held::Free);
     let (a, b) = trimmed(t, to(t.a), to(t.b));
-    bar(&mut d.shapes, a, b, w, track_colour(section(&t.section)), t.fringe);
+    // An unused crossover's middle is thin unless something is on it or holds it.
+    let thin = unused && section(&t.section).is_none_or(|s| !s.occupied && s.held == Held::Free);
+    bar(&mut d.shapes, a, b, if thin { w * UNUSED_W } else { w }, track_colour(section(&t.section)), t.fringe);
     d.keep.bars.push((a, b, w));
     // End of overlap: an end of an overlap section where nothing held goes on.
     if section(&t.section).is_some_and(|s| s.held == Held::Overlap) {
@@ -489,8 +514,9 @@ pub fn draw(scene: &Scene, cam: &Camera, screen: Rect, st: &PaintState) -> Drawi
         let text = TextItem { at: r.center(), anchor: Align2::CENTER_CENTER, text: p.label.clone(), size: 9.0 * glyph(cam.scale), colour: BG, monospace: false };
         d.movable_text(text, Role::Platform, Vec::new(), Some(r));
     }
+    let unused = unused_crossovers(scene, st.view);
     for t in &scene.tracks {
-        track_shapes(&mut d, t, &to, st, w);
+        track_shapes(&mut d, t, &to, st, w, unused.contains(&t.section));
     }
     for p in &scene.points {
         points_shapes(&mut d.shapes, p, &to, st, track_colour(section(&p.section)), w);
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Commit**

```bash
git add crates/client-ui crates/client-ui/src/paint.rs
git commit -m "feat(client-ui): points show which way they lie; unused crossover middles are thin"
```

---

### Task 21: Fit keeps a long area readable (UI review M13)

Spec §10 M13 (U18, U19). Merges with P1: the legibility measurement uses the same Fit and reports, not asserts,
criterion 3 where Fit shows part of an area.

**Files:**
- Modify: `crates/client-ui/src/scene.rs`
- Modify: `crates/client-ui/src/screens.rs`
- Modify: `CLAUDE.md`
- Test: `crates/client-ui/tests/legibility.rs`
- Test: `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: `Scene::fit_bounds`, `Camera::fit`.
- Produces: `scene::FIT_MIN_SCALE = 0.55`; `Scene.focus: Option<Pos2>`; `Scene::fit_camera(Rect) -> Option<Camera>`;
  `legibility.rs` rows gain `readable` and count only on-screen hidden numbers.

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-ui/tests/legibility.rs b/crates/client-ui/tests/legibility.rs
index 0f2e321..ab09c2d 100644
--- a/crates/client-ui/tests/legibility.rs
+++ b/crates/client-ui/tests/legibility.rs
@@ -51,6 +51,9 @@ struct Row {
     hidden: Vec<String>,
     /// How long `plan` took (debug builds are several times slower).
     ms: f64,
+    /// Fit showed part of the area round its busiest station (polish spec
+    /// M13), not all of it: reported, not held to criterion 3.
+    readable: bool,
 }
 
 #[test]
@@ -79,7 +82,8 @@ fn every_view_is_legible_at_every_zoom() {
             let names = Names::new(&l);
             let view = format!("{name} {}", area.as_deref().unwrap_or("spectator"));
             for (window, screen) in windows() {
-                let fit = Camera::fit(sc.fit_bounds().unwrap(), screen);
+                let fit = sc.fit_camera(screen).unwrap();
+                let readable = fit.scale > Camera::fit(sc.fit_bounds().unwrap(), screen).scale;
                 for zoom in [1.0_f32, 2.0, 4.0] {
                     let cam = Camera { centre: fit.centre, scale: fit.scale * zoom };
                     let st = PaintState {
@@ -98,24 +102,45 @@ fn every_view_is_legible_at_every_zoom() {
                     let t0 = std::time::Instant::now();
                     let plan = labels::plan(&d, &mut measure);
                     let ms = t0.elapsed().as_secs_f64() * 1000.0;
+                    // Own numbers hidden where the player looks (a readable Fit,
+                    // polish spec M13, shows only part of a long area).
+                    let hidden: Vec<String> = d
+                        .movable
+                        .iter()
+                        .zip(&plan.spots)
+                        .filter(|(m, spot)| m.role == labels::Role::Number && spot.is_none() && screen.contains(d.texts[m.text].at))
+                        .map(|(m, _)| d.texts[m.text].text.clone())
+                        .collect();
                     let audit = labels::audit(&on_screen(labels::apply(d, &plan), screen), &mut measure);
-                    rows.push(Row { window, view: view.clone(), zoom, audit, hidden: plan.hidden_numbers.clone(), ms });
+                    rows.push(Row { window, view: view.clone(), zoom, audit, hidden, ms, readable });
                 }
             }
         }
     }
-    println!("{:10} {:38} {:>4} {:>8} {:>7} {:>5} {:>6} {:>7}  shown", "window", "view", "zoom", "overlaps", "covered", "tight", "hidden", "plan ms");
+    println!("{:10} {:38} {:>4} {:>8} {:>7} {:>5} {:>6} {:>7} {:>4}  shown", "window", "view", "zoom", "overlaps", "covered", "tight", "hidden", "plan ms", "fit");
     for r in &rows {
         println!(
-            "{:10} {:38} {:>4} {:>8} {:>7} {:>5} {:>6} {:>7.2}  {:?} {:?}",
-            r.window, r.view, r.zoom, r.audit.overlaps, r.audit.covered, r.audit.tight, r.hidden.len(), r.ms, r.audit.shown, r.hidden
+            "{:10} {:38} {:>4} {:>8} {:>7} {:>5} {:>6} {:>7.2} {:>4}  {:?} {:?}",
+            r.window,
+            r.view,
+            r.zoom,
+            r.audit.overlaps,
+            r.audit.covered,
+            r.audit.tight,
+            r.hidden.len(),
+            r.ms,
+            if r.readable { "read" } else { "all" },
+            r.audit.shown,
+            r.hidden
         );
     }
     for r in &rows {
         assert_eq!(r.audit.overlaps, 0, "{} {} x{}: texts overlap", r.window, r.view, r.zoom);
         assert_eq!(r.audit.covered, 0, "{} {} x{}: texts cover track, lamps or boxes", r.window, r.view, r.zoom);
     }
-    let fit_small: Vec<&Row> = rows.iter().filter(|r| r.window == "1280x800" && r.zoom == 1.0).collect();
+    // Criterion 3 holds where Fit frames the whole area; a readable Fit (Gretz's
+    // boxes, which drew no numbers at all before) is reported above.
+    let fit_small: Vec<&Row> = rows.iter().filter(|r| r.window == "1280x800" && r.zoom == 1.0 && !r.readable).collect();
     for r in fit_small.iter().filter(|r| !r.view.ends_with("spectator")) {
         assert!(r.hidden.is_empty(), "{}: every own number drawn at 1280x800 Fit, not {:?}", r.view, r.hidden);
     }
diff --git a/crates/client-ui/tests/screens.rs b/crates/client-ui/tests/screens.rs
index 4706ada..59200f5 100644
--- a/crates/client-ui/tests/screens.rs
+++ b/crates/client-ui/tests/screens.rs
@@ -953,3 +953,22 @@ fn the_diagram_zooms_with_buttons_and_keys_and_says_how() {
     assert!(!has_text(&r.frame(), client_ui::screens::VIEW_HINT), "moved: the hint goes");
     assert!((client_ui::screens::ZOOM_PER_POINT * 100.0).exp() < 1.2, "a wheel notch is a small step");
 }
+
+/// Polish spec M13: Gretz's long areas are not a thin strip at Fit: Fit
+/// shows them at a readable scale round their busiest station; short areas
+/// and spectators still see everything.
+#[test]
+fn fit_keeps_a_long_area_readable() {
+    use client_ui::scene::FIT_MIN_SCALE;
+    let mut r = Rig::in_game(converted("gretz-armainvilliers"), Some("Gretz"));
+    r.frame();
+    let cam = r.ui.camera().unwrap();
+    let sc = client_ui::scene::Scene::build(r.ui.core.game().unwrap().layout().unwrap()).unwrap();
+    assert_eq!((cam.scale, Some(cam.centre)), (FIT_MIN_SCALE, sc.focus), "{cam:?}");
+    let mut r = Rig::in_game(converted("gretz-armainvilliers"), None);
+    r.frame();
+    assert!(r.ui.camera().unwrap().scale < FIT_MIN_SCALE, "a spectator sees the whole layout");
+    let mut r = Rig::in_game(drawn_twobox(), Some("West"));
+    r.frame();
+    assert!(r.ui.camera().unwrap().scale > FIT_MIN_SCALE);
+}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-ui --test screens fit_keeps`
Expected: compile error (`FIT_MIN_SCALE`).

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-ui/src/scene.rs b/crates/client-ui/src/scene.rs
index 5d8c526..d78b314 100644
--- a/crates/client-ui/src/scene.rs
+++ b/crates/client-ui/src/scene.rs
@@ -8,6 +8,8 @@ use client_core::select;
 use egui::{Pos2, Rect, Vec2, pos2, vec2};
 use protocol::{ExitName, Layout};
 
+use crate::camera::Camera;
+
 #[derive(Clone, Debug, PartialEq)]
 pub struct TrackLine {
     pub segment: String,
@@ -147,8 +149,15 @@ pub struct Scene {
     pub own: Option<Rect>,
     /// Bounds of everything drawn.
     pub all: Option<Rect>,
+    /// Your area's busiest station (the most calls in your simplifier): the
+    /// middle of its platforms. `None` for a spectator (polish spec M13).
+    pub focus: Option<Pos2>,
 }
 
+/// Fit never shows a player's area smaller than this (pixels per layout
+/// unit), just above where signal numbers appear (polish spec M13).
+pub const FIT_MIN_SCALE: f32 = 0.55;
+
 /// Coordinates beyond this are nonsense and left out, so bounds, centres
 /// and fits stay finite.
 pub const MAX_COORD: f64 = 1.0e7;
@@ -368,15 +377,44 @@ impl Scene {
         for p in &sc.points {
             grow(&mut sc.all, p.at);
         }
+        sc.focus = busiest(l, &sc);
         Some(sc)
     }
 
+    /// The "Fit" camera: your own area (or everything) framed in `screen`; a
+    /// player's area too long to read that way is shown at `FIT_MIN_SCALE`
+    /// round its busiest station instead (polish spec M13).
+    pub fn fit_camera(&self, screen: Rect) -> Option<Camera> {
+        let fit = Camera::fit(self.fit_bounds()?, screen);
+        Some(match self.focus {
+            Some(centre) if fit.scale < FIT_MIN_SCALE => Camera { centre, scale: FIT_MIN_SCALE },
+            _ => fit,
+        })
+    }
+
     /// What "Fit" frames: your own area, or everything.
     pub fn fit_bounds(&self) -> Option<Rect> {
         self.own.or(self.all)
     }
 }
 
+/// The middle of the platforms of the place your simplifier calls at most
+/// (ties: the first in name order), among places with a platform in your own
+/// area; `None` for a spectator or with no such place.
+fn busiest(l: &Layout, sc: &Scene) -> Option<Pos2> {
+    let own = sc.own?;
+    l.area.as_ref()?;
+    let mut calls: BTreeMap<&str, usize> = BTreeMap::new();
+    for c in l.simplifier.iter().flat_map(|r| &r.calls) {
+        *calls.entry(c.place.as_str()).or_default() += 1;
+    }
+    let mine = |place: &str| -> Option<Rect> {
+        sc.platforms.iter().filter(|p| p.place == place && own.contains(p.rect.center())).map(|p| p.rect).reduce(|a, b| a.union(b))
+    };
+    let (place, _) = calls.iter().filter(|(p, _)| mine(p).is_some()).max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))?;
+    Some(mine(place)?.center())
+}
+
 /// Chain drawn lines into runs through plain joints, and give each run the
 /// directions its signals face. A run's end is loose where your visible
 /// track stops: no other segment meets it, or it is a route's exit (a
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index 95d8653..242a30a 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -856,7 +856,7 @@ impl UiApp {
         // changes size while the player has not moved the view (polish spec H7).
         let resized = self.fit_size.is_some_and(|s| (s - rect.size()).length() > 0.5);
         if self.fitted.as_ref() != Some(&fit_key) || self.cam.is_none() || (resized && !self.cam_moved) {
-            self.cam = Some(scene.fit_bounds().map_or(Camera { centre: rect.center(), scale: 1.0 }, |b| Camera::fit(b, rect)));
+            self.cam = Some(scene.fit_camera(rect).unwrap_or(Camera { centre: rect.center(), scale: 1.0 }));
             self.fitted = Some(fit_key);
             self.cam_moved = false;
         }
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Browser client": "Fit shows a player's area at no less than 0.55 px per unit, round its busiest station
when it is too long (`Scene::fit_camera`, polish spec M13)."

- [ ] **Step 6: Commit**

```bash
git add crates/client-ui crates/client-ui/src/scene.rs crates/client-ui/src/screens.rs CLAUDE.md
git commit -m "feat(client-ui): Fit keeps a long area readable round its busiest station"
```

---

### Task 22: The lobby orients a newcomer and checks the form in place (UI review M14, M15)

Spec §10 M14, M15 (U20, U21). Additive protocol (titles, descriptions, your name, last played), the layouts'
one-line descriptions, Sign out through the web shell, and a form checked as typed.

**Files:**
- Modify: `crates/client-core/src/app.rs`
- Modify: `crates/client-core/src/form.rs`
- Modify: `crates/client-core/src/lib.rs`
- Modify: `crates/client-ui/src/screens.rs`
- Modify: `crates/client-web/src/lib.rs`
- Modify: `crates/protocol/src/lobby.rs`
- Modify: `crates/server/src/layouts.rs`
- Modify: `crates/server/src/supervisor.rs`
- Modify: `crates/ts2-import/src/areas.rs`
- Modify: `layouts/drain.areas.json`
- Modify: `layouts/gretz-armainvilliers.areas.json`
- Modify: `layouts/liverpool-st.areas.json`
- Modify: `CLAUDE.md`
- Test: `crates/client-core/tests/app.rs`
- Test: `crates/client-core/tests/form.rs`
- Test: `crates/client-ui/tests/screens.rs`
- Test: `crates/protocol/tests/lobby.rs`
- Test: `crates/server/tests/front.rs`
- Test: `crates/server/tests/supervisor.rs`
- Test: `crates/server/tests/units.rs`
- Test: `crates/ts2-import/tests/areas.rs`

**Interfaces:**
- Consumes: Task 9's area list (moved into the new lobby), `SaveSummary.last_played`.
- Produces: `LayoutInfo.{title, description}`, `LobbyReply::Layouts.you`, `GameInfo.last_played`; the areas file's
  `description`; `App::me()`; `client_core::form::{seed, start, utc}`; `UiApp::wants_logout()` (the web shell goes to
  `/auth/logout`); `LOBBY_W = 900.0`, `FORM_NOTE_W = 230.0`, `pub LOBBY_INTRO`.

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-core/tests/app.rs b/crates/client-core/tests/app.rs
index a459acc..00612e3 100644
--- a/crates/client-core/tests/app.rs
+++ b/crates/client-core/tests/app.rs
@@ -81,10 +81,11 @@ fn the_lobby_lists_games_and_layouts_and_sends_what_you_ask() {
         players: vec![s("bob")],
         error: None,
         creator: Some(s("bob")),
+        last_played: None,
         can_delete: false,
     };
     h.push(ServerFrame::Lobby(LobbyReply::Games { games: vec![info.clone()] }));
-    h.push(ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West")] }] }));
+    h.push(ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West")], title: String::new(), description: String::new() }], you: None }));
     app.tick(1.0);
     assert_eq!(app.games(), [info]);
     assert_eq!(app.layouts()[0].name, "twobox");
diff --git a/crates/client-core/tests/form.rs b/crates/client-core/tests/form.rs
new file mode 100644
index 0000000..b88c4da
--- /dev/null
+++ b/crates/client-core/tests/form.rs
@@ -0,0 +1,24 @@
+//! The lobby form's checks (polish spec M15) and the Last played date (M14).
+
+use client_core::form::{seed, start, utc};
+
+#[test]
+fn seeds_are_whole_numbers_or_blank() {
+    assert_eq!((seed(""), seed(" 42 ")), (Ok(None), Ok(Some(42))));
+    assert!(seed("abc").is_err() && seed("-1").is_err() && seed("1.5").is_err());
+}
+
+#[test]
+fn starts_are_clock_times_or_blank() {
+    assert_eq!((start(" "), start("8:00"), start("07:30:15")), (Ok(None), Ok(Some("8:00".into())), Ok(Some("07:30:15".into()))));
+    for bad in ["25:00", "7", "07:60", "x:10", "07:30:15:00", "0730"] {
+        assert!(start(bad).is_err(), "{bad}");
+    }
+}
+
+#[test]
+fn unix_times_read_as_utc_dates() {
+    assert_eq!(utc(0), "1970-01-01 00:00 UTC");
+    assert_eq!(utc(1_790_865_900), "2026-10-01 14:45 UTC");
+    assert_eq!(utc(951_782_400), "2000-02-29 00:00 UTC");
+}
diff --git a/crates/client-ui/tests/screens.rs b/crates/client-ui/tests/screens.rs
index 59200f5..b08190d 100644
--- a/crates/client-ui/tests/screens.rs
+++ b/crates/client-ui/tests/screens.rs
@@ -42,7 +42,7 @@ impl Rig {
         let mut r = Rig { ctx: egui::Context::default(), ui, h, game, t: 0.0, events: vec![], lobby_sent: vec![], size: vec2(1280.0, 800.0) };
         r.frame();
         r.lobby_sent.clear();
-        r.h.push(ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")] }] }));
+        r.h.push(ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")], title: String::new(), description: String::new() }], you: None }));
         r.frame();
         r
     }
@@ -147,6 +147,7 @@ fn the_lobby_lists_games_and_creates_one() {
         players: vec![s("bob")],
         error: Some(s("disk full")),
         creator: Some(s("bob")),
+        last_played: None,
         can_delete: false,
     };
     r.h.push(ServerFrame::Lobby(LobbyReply::Games { games: vec![info] }));
@@ -174,6 +175,7 @@ fn deleting_a_game_asks_first() {
         players: vec![],
         error: None,
         creator: Some(s("ann")),
+        last_played: None,
         can_delete,
     };
     r.h.push(ServerFrame::Lobby(LobbyReply::Games { games: vec![game("g-mine", true), game("g-theirs", false)] }));
@@ -972,3 +974,50 @@ fn fit_keeps_a_long_area_readable() {
     r.frame();
     assert!(r.ui.camera().unwrap().scale > FIT_MIN_SCALE);
 }
+
+/// Polish spec M14, M15: the lobby says what this is and who you are, signs
+/// out, describes the layout, explains a bad seed beside its field, and
+/// will not send the form until it is right.
+#[test]
+fn the_lobby_orients_a_newcomer_and_checks_the_form_in_place() {
+    let mut r = Rig::lobby(drawn_twobox());
+    r.h.push(ServerFrame::Lobby(LobbyReply::Layouts {
+        layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West")], title: s("Two boxes"), description: s("Two small boxes.") }],
+        you: Some(s("ann")),
+    }));
+    let saved = GameInfo {
+        id: s("g-abc"),
+        layout: s("twobox"),
+        state: GameState::Saved,
+        sim_time: 25_300.0,
+        areas: vec![],
+        players: vec![],
+        error: None,
+        creator: Some(s("bob")),
+        last_played: Some(1_790_865_900),
+        can_delete: false,
+    };
+    r.h.push(ServerFrame::Lobby(LobbyReply::Games { games: vec![saved] }));
+    r.frame();
+    let out = r.frame();
+    for want in [client_ui::screens::LOBBY_INTRO, "Signed in as ann", "Two boxes", "Two small boxes.", "By", "bob", "2026-10-01 14:45 UTC"] {
+        assert!(has_text(&out, want), "{want} in {:?}", texts(&out));
+    }
+    let create = text_at(&out, "Create");
+    let seed = text_at(&out, "random").center();
+    r.click(seed, PointerButton::Primary);
+    r.events.push(Event::Text("x1".into()));
+    r.frame();
+    let out = r.frame();
+    assert!(has_text(&out, "a whole number, or blank for random"));
+    assert_eq!(text_at(&out, "Create"), create, "nothing moved");
+    r.click(create.center(), PointerButton::Primary);
+    assert!(r.lobby_sent.is_empty(), "not sent: {:?}", r.lobby_sent);
+    click_text(&mut r, &out, "Sign out");
+    assert!(r.ui.wants_logout());
+    // A wide window centres the column.
+    r.size = vec2(1920.0, 1080.0);
+    r.frame();
+    let out = r.frame();
+    assert!(text_at(&out, "signalbox").min.x > 400.0, "{:?}", text_at(&out, "signalbox"));
+}
diff --git a/crates/protocol/tests/lobby.rs b/crates/protocol/tests/lobby.rs
index 770eecf..f4e1148 100644
--- a/crates/protocol/tests/lobby.rs
+++ b/crates/protocol/tests/lobby.rs
@@ -93,6 +93,7 @@ fn lobby_replies() {
                     players: vec![s("ann"), s("sam")],
                     error: None,
                     creator: None,
+                    last_played: None,
                     can_delete: false,
                 },
                 GameInfo {
@@ -104,6 +105,7 @@ fn lobby_replies() {
                     players: vec![],
                     error: Some(s("resume: bad snapshot")),
                     creator: Some(s("sam")),
+                    last_played: None,
                     can_delete: true,
                 },
             ],
@@ -118,10 +120,19 @@ fn lobby_replies() {
     );
     check_server(
         ServerFrame::Lobby(LobbyReply::Layouts {
-            layouts: vec![LayoutInfo { name: s("drain"), areas: vec![s("Drain"), s("Lambeth")] }],
+            layouts: vec![LayoutInfo { name: s("drain"), areas: vec![s("Drain"), s("Lambeth")], title: String::new(), description: String::new() }],
+            you: None,
         }),
         json!({"type": "layouts", "layouts": [{"name": "drain", "areas": ["Drain", "Lambeth"]}]}),
     );
+    // Polish spec M14: the signed-in name, a layout's title and description, when known.
+    check_server(
+        ServerFrame::Lobby(LobbyReply::Layouts {
+            layouts: vec![LayoutInfo { name: s("drain"), areas: vec![s("Bank")], title: s("W&C"), description: s("A shuttle") }],
+            you: Some(s("ann")),
+        }),
+        json!({"type": "layouts", "you": "ann", "layouts": [{"name": "drain", "areas": ["Bank"], "title": "W&C", "description": "A shuttle"}]}),
+    );
     check_server(
         ServerFrame::Lobby(LobbyReply::Joined { game: s("g-abcdefgh2345"), you: s("ann") }),
         json!({"type": "joined", "game": "g-abcdefgh2345", "you": "ann"}),
diff --git a/crates/server/tests/front.rs b/crates/server/tests/front.rs
index cfab6f8..58820fa 100644
--- a/crates/server/tests/front.rs
+++ b/crates/server/tests/front.rs
@@ -60,7 +60,10 @@ async fn the_lobby_and_a_game_over_websockets() {
     ann.send(&lobby(LobbyMsg::ListLayouts)).await.unwrap();
     assert_eq!(
         next(&mut ann).await.unwrap(),
-        ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")] }] })
+        ServerFrame::Lobby(LobbyReply::Layouts {
+            layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")], title: s("Two boxes"), description: String::new() }],
+            you: Some(s("ann")),
+        })
     );
     let id = create(&mut ann, "twobox").await;
     let mut bob = f.connect("bob").await;
diff --git a/crates/server/tests/supervisor.rs b/crates/server/tests/supervisor.rs
index b53ed34..e8992dc 100644
--- a/crates/server/tests/supervisor.rs
+++ b/crates/server/tests/supervisor.rs
@@ -174,7 +174,7 @@ fn layouts_are_read_once_and_only_valid_names_count() {
     let root = temp_dir("layouts");
     let dir = layouts_dir(&root);
     let l = Layouts::load(&dir).unwrap();
-    assert_eq!(l.infos(), [LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")] }]);
+    assert_eq!(l.infos(), [LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")], title: s("Two boxes"), description: String::new() }]);
     assert_eq!(l.path("twobox"), Some(dir.join("twobox.json")));
     assert_eq!(l.path("../layouts/twobox"), None);
     assert_eq!(l.path("Bad Name"), None);
@@ -266,7 +266,10 @@ async fn the_lobby_lists_layouts_and_saved_games() {
     rig.lobby(&ann, LobbyMsg::ListLayouts);
     assert_eq!(
         next(&ann).await.unwrap(),
-        ServerFrame::Lobby(LobbyReply::Layouts { layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")] }] })
+        ServerFrame::Lobby(LobbyReply::Layouts {
+            layouts: vec![LayoutInfo { name: s("twobox"), areas: vec![s("West"), s("East")], title: s("Two boxes"), description: String::new() }],
+            you: Some(s("ann")),
+        })
     );
     rig.lobby(&ann, LobbyMsg::ListGames);
     let Some(ServerFrame::Lobby(LobbyReply::Games { games })) = next(&ann).await else { panic!() };
diff --git a/crates/server/tests/units.rs b/crates/server/tests/units.rs
index 7fbd7ce..fce6e4f 100644
--- a/crates/server/tests/units.rs
+++ b/crates/server/tests/units.rs
@@ -156,9 +156,10 @@ fn the_placeholder_page_escapes_every_name() {
         players: vec![],
         error: Some("<b>bad</b>".into()),
         creator: None,
+        last_played: None,
         can_delete: false,
     }];
-    let page = index_page("a<b", &games, &[LayoutInfo { name: "drain".into(), areas: vec![] }]);
+    let page = index_page("a<b", &games, &[LayoutInfo { name: "drain".into(), areas: vec![], title: String::new(), description: String::new() }]);
     assert!(!page.contains("<script>") && !page.contains("<b>bad"), "{page}");
     assert!(page.contains("Hackney &amp; Bow: robot") && page.contains("Signed in as a&lt;b"), "{page}");
 }
diff --git a/crates/ts2-import/tests/areas.rs b/crates/ts2-import/tests/areas.rs
index deef2a4..7af58f9 100644
--- a/crates/ts2-import/tests/areas.rs
+++ b/crates/ts2-import/tests/areas.rs
@@ -15,6 +15,7 @@ fn spec(boundaries: &[&str], areas: &[(&str, Vec<&str>)]) -> AreasFile {
     AreasFile {
         schema: 1,
         prefix: None,
+        description: None,
         boundaries: boundaries.iter().map(|s| s.to_string()).collect(),
         areas: areas
             .iter()
@@ -260,3 +261,17 @@ fn gretz_has_three_boxes() {
     assert_eq!(w.layout["box_prefix"], "G");
     assert_eq!(w.layout["workstations"], json!({"Gretz": "A", "Tournan & Marles": "B", "Mortcerf & Coulommiers": "C"}));
 }
+
+/// Polish spec M14: a layout's one-line description reaches the world's
+/// `layout`, for the lobby; every shipped areas file has one.
+#[test]
+fn the_description_goes_into_the_layout() {
+    for name in ["liverpool-st", "drain", "gretz-armainvilliers"] {
+        let dir = env!("CARGO_MANIFEST_DIR");
+        let read = |p: String| std::fs::read_to_string(p).unwrap();
+        let mut w = ts2_import::convert(&read(format!("{dir}/tests/data/{name}.json"))).unwrap().world;
+        ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&read(format!("{dir}/../../layouts/{name}.areas.json"))).unwrap()).unwrap();
+        let d = w.layout["description"].as_str().unwrap_or_default();
+        assert!((20..=120).contains(&d.len()), "{name}: {d:?}");
+    }
+}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-protocol --test lobby -p signalbox-client-core --test form -p signalbox-client-ui --test screens the_lobby -p ts2-import --test areas`
Expected: compile errors (`title`, `you`, `last_played`, `form`).

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-core/src/app.rs b/crates/client-core/src/app.rs
index 9dd85b2..6d2910e 100644
--- a/crates/client-core/src/app.rs
+++ b/crates/client-core/src/app.rs
@@ -372,7 +372,12 @@ impl App {
     fn lobby_reply(&mut self, r: LobbyReply) {
         match r {
             LobbyReply::Games { games } => self.games = games,
-            LobbyReply::Layouts { layouts } => self.layouts = layouts,
+            LobbyReply::Layouts { layouts, you } => {
+                self.layouts = layouts;
+                if you.is_some() {
+                    self.me = you;
+                }
+            }
             LobbyReply::Lessons { lessons } => self.lessons = lessons,
             LobbyReply::Joined { game, you } => {
                 self.joining = None;
@@ -507,6 +512,11 @@ impl App {
         }
     }
 
+    /// Your signed-in name, once the front has said it.
+    pub fn me(&self) -> Option<&str> {
+        self.me.as_deref()
+    }
+
     pub fn lobby_note(&self) -> Option<&str> {
         self.lobby_note.as_deref()
     }
diff --git a/crates/client-core/src/form.rs b/crates/client-core/src/form.rs
new file mode 100644
index 0000000..aee47ca
--- /dev/null
+++ b/crates/client-core/src/form.rs
@@ -0,0 +1,41 @@
+//! The lobby's New game form, checked where it is typed (polish spec M15):
+//! a seed is a whole number or blank, a start time `HH:MM` or `HH:MM:SS` or
+//! blank; anything else is said beside the field, never sent.
+
+/// A typed seed: `Ok(None)` blank (random), `Ok(Some(n))`, or why not.
+pub fn seed(typed: &str) -> Result<Option<u64>, &'static str> {
+    let t = typed.trim();
+    if t.is_empty() {
+        return Ok(None);
+    }
+    t.parse().map(Some).map_err(|_| "a whole number, or blank for random")
+}
+
+/// A typed start time: `Ok(None)` blank (the layout's own), `Ok(Some(t))`
+/// as typed, or why not. The front checks it again.
+pub fn start(typed: &str) -> Result<Option<String>, &'static str> {
+    let t = typed.trim();
+    if t.is_empty() {
+        return Ok(None);
+    }
+    let parts: Vec<&str> = t.split(':').collect();
+    let num = |s: &str, max: u32| s.len() <= 2 && !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<u32>().is_ok_and(|v| v <= max);
+    let ok = matches!(parts.as_slice(), [h, m] if num(h, 23) && num(m, 59)) || matches!(parts.as_slice(), [h, m, s] if num(h, 23) && num(m, 59) && num(s, 59));
+    if ok { Ok(Some(t.to_string())) } else { Err("HH:MM, or blank for the layout's start") }
+}
+
+/// Unix seconds as `2026-10-01 14:05 UTC` (the lobby's Last played).
+pub fn utc(unix_s: u64) -> String {
+    let (days, secs) = (unix_s / 86_400, unix_s % 86_400);
+    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
+    let z = days as i64 + 719_468;
+    let era = z.div_euclid(146_097);
+    let doe = z - era * 146_097;
+    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
+    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
+    let mp = (5 * doy + 2) / 153;
+    let d = doy - (153 * mp + 2) / 5 + 1;
+    let m = if mp < 10 { mp + 3 } else { mp - 9 };
+    let y = yoe + era * 400 + i64::from(m <= 2);
+    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", secs / 3600, secs / 60 % 60)
+}
diff --git a/crates/client-core/src/lib.rs b/crates/client-core/src/lib.rs
index 8894ce8..78031ef 100644
--- a/crates/client-core/src/lib.rs
+++ b/crates/client-core/src/lib.rs
@@ -4,6 +4,7 @@
 //! it in a browser.
 
 pub mod app;
+pub mod form;
 pub mod input;
 pub mod lessons;
 pub mod log;
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index 242a30a..38f7b27 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -63,6 +63,13 @@ const ENQUIRY_OFFSET_PX: f32 = 16.0;
 /// The top bar's fixed widths (polish spec M5): the clock state (`paused`,
 /// `8×`) and the pause/resume button.
 const CLOCK_STATE_W: f32 = 52.0;
+/// The lobby's column, centred in a wide window (polish spec M14).
+const LOBBY_W: f32 = 900.0;
+/// The slot beside a form field for what is wrong with it (polish spec M15).
+const FORM_NOTE_W: f32 = 230.0;
+/// The lobby's first line (polish spec M14).
+pub const LOBBY_INTRO: &str =
+    "Run a signal box: set routes for trains on real layouts, alone or with friends. New here? Start with the tutorial below.";
 /// The lobby's layout list, wide enough for every name, so Create never moves.
 const LAYOUT_COMBO_W: f32 = 180.0;
 const PAUSE_W: f32 = 64.0;
@@ -111,6 +118,8 @@ pub struct UiApp {
     confirm_delete: Option<String>,
     /// Release area was pressed and awaits "Yes, release" (polish spec M10).
     confirm_release: bool,
+    /// Sign out was pressed: the shell goes to `/auth/logout` (polish spec M14).
+    wants_logout: bool,
     settings: Settings,
     /// Where the settings are kept between visits (none in most tests).
     store: Option<Box<dyn SettingsStore>>,
@@ -160,6 +169,7 @@ impl UiApp {
             new_game: NewGame::default(),
             confirm_delete: None,
             confirm_release: false,
+            wants_logout: false,
             settings: Settings::default(),
             store: None,
             side_tab: SideTab::default(),
@@ -230,6 +240,11 @@ impl UiApp {
         }
     }
 
+    /// Sign out was pressed: the shell should send the browser to `/auth/logout`.
+    pub fn wants_logout(&self) -> bool {
+        self.wants_logout
+    }
+
     /// The headcode whose enquiry window is open.
     pub fn enquiry(&self) -> Option<&str> {
         self.enquiry.as_deref()
@@ -278,113 +293,153 @@ impl UiApp {
         ui.ctx().request_repaint_after(Duration::from_millis(every));
     }
 
+    /// The lobby (polish spec M14, M15): a centred column with a line of
+    /// orientation, who you are and Sign out; the tutorials; the New game
+    /// form, labels first, each field's problem beside it and the front's
+    /// answer in a fixed line below (nothing shifts); the games with who
+    /// made them and when they were last played.
     fn lobby(&mut self, ui: &mut Ui) {
         egui::CentralPanel::default().show(ui, |ui| {
+            let side = ((ui.available_width() - LOBBY_W) / 2.0).max(0.0);
+            ui.horizontal_top(|ui| {
+                ui.add_space(side);
+                ui.vertical(|ui| {
+                    ui.set_max_width(LOBBY_W);
+                    self.lobby_column(ui);
+                });
+            });
+        });
+    }
+
+    fn lobby_column(&mut self, ui: &mut Ui) {
+        ui.horizontal(|ui| {
             ui.heading("signalbox");
-            if let Some(n) = self.core.lobby_note() {
-                ui.label(RichText::new(n).color(ALARM));
-            }
-            ui.separator();
-            self.tutorials(ui);
-            ui.separator();
-            ui.label(RichText::new("New game").strong());
-            let layouts: Vec<String> = self.core.layouts().iter().map(|l| l.name.clone()).collect();
-            let areas: Vec<Vec<String>> = self.core.layouts().iter().map(|l| l.areas.clone()).collect();
-            if layouts.is_empty() {
-                ui.label("No layouts yet.");
-            } else {
-                self.new_game.layout = self.new_game.layout.min(layouts.len() - 1);
-                ui.horizontal(|ui| {
-                    egui::ComboBox::from_label("Layout").width(LAYOUT_COMBO_W).selected_text(layouts[self.new_game.layout].as_str()).show_ui(ui, |ui| {
-                        for (i, name) in layouts.iter().enumerate() {
-                            ui.selectable_value(&mut self.new_game.layout, i, name.as_str());
-                        }
-                    });
-                    // Where the creator starts (polish spec H2): an area to signal, or watching.
-                    let mine = &areas[self.new_game.layout];
-                    self.new_game.area = self.new_game.area.min(mine.len());
-                    let shown = |i: usize| if i == 0 { "watch".to_string() } else { mine[i - 1].clone() };
-                    ui.label("Signal");
-                    egui::ComboBox::from_id_salt("new_game_area").selected_text(shown(self.new_game.area)).show_ui(ui, |ui| {
-                        for i in 0..=mine.len() {
-                            ui.selectable_value(&mut self.new_game.area, i, shown(i));
-                        }
-                    });
-                    ui.label("Seed");
-                    ui.add(egui::TextEdit::singleline(&mut self.new_game.seed).desired_width(90.0).hint_text("random"));
-                    ui.label("Start");
-                    ui.add(egui::TextEdit::singleline(&mut self.new_game.start).desired_width(70.0).hint_text("HH:MM"));
-                    if ui.button("Create").clicked() {
-                        let seed = self.new_game.seed.trim().parse().ok();
-                        let start = Some(self.new_game.start.trim().to_string()).filter(|s| !s.is_empty());
-                        let area = self.new_game.area.checked_sub(1).map(|i| mine[i].clone());
-                        self.core.create_game_in(&layouts[self.new_game.layout], seed, start, area.as_deref());
+            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
+                if let Some(me) = self.core.me() {
+                    if ui.button("Sign out").clicked() {
+                        self.wants_logout = true;
+                    }
+                    ui.label(format!("Signed in as {me}"));
+                }
+            });
+        });
+        ui.label(LOBBY_INTRO);
+        ui.separator();
+        self.tutorials(ui);
+        ui.separator();
+        ui.label(RichText::new("New game").strong());
+        let layouts = self.core.layouts().to_vec();
+        if layouts.is_empty() {
+            ui.label("No layouts yet.");
+        } else {
+            self.new_game.layout = self.new_game.layout.min(layouts.len() - 1);
+            let chosen = &layouts[self.new_game.layout];
+            let shown = |l: &protocol::LayoutInfo| if l.title.is_empty() { l.name.clone() } else { l.title.clone() };
+            ui.horizontal(|ui| {
+                ui.label("Layout");
+                egui::ComboBox::from_id_salt("new_game_layout").width(LAYOUT_COMBO_W).selected_text(shown(chosen)).show_ui(ui, |ui| {
+                    for (i, l) in layouts.iter().enumerate() {
+                        ui.selectable_value(&mut self.new_game.layout, i, shown(l));
                     }
                 });
-            }
-            ui.separator();
+                // Where the creator starts (polish spec H2): an area to signal, or watching.
+                let mine = &chosen.areas;
+                self.new_game.area = self.new_game.area.min(mine.len());
+                let area = |i: usize| if i == 0 { "watch".to_string() } else { mine[i - 1].clone() };
+                ui.label("Signal");
+                egui::ComboBox::from_id_salt("new_game_area").selected_text(area(self.new_game.area)).show_ui(ui, |ui| {
+                    for i in 0..=mine.len() {
+                        ui.selectable_value(&mut self.new_game.area, i, area(i));
+                    }
+                });
+            });
+            ui.label(RichText::new(&chosen.description).color(paint::LABEL));
+            let seed = client_core::form::seed(&self.new_game.seed);
+            let start = client_core::form::start(&self.new_game.start);
             ui.horizontal(|ui| {
-                ui.label(RichText::new("Games").strong());
-                if ui.button("Refresh").clicked() {
-                    self.core.refresh();
+                ui.label("Seed").on_hover_text("The same seed gives the same delays and dwell times; blank for random.");
+                ui.add(egui::TextEdit::singleline(&mut self.new_game.seed).desired_width(90.0).hint_text("random"));
+                ui.add_sized([FORM_NOTE_W, 18.0], egui::Label::new(RichText::new(*seed.as_ref().err().unwrap_or(&"")).color(ALARM)));
+                ui.label("Start").on_hover_text("The time on the sim clock when the game begins; blank for the layout's own.");
+                ui.add(egui::TextEdit::singleline(&mut self.new_game.start).desired_width(70.0).hint_text("HH:MM"));
+                ui.add_sized([FORM_NOTE_W, 18.0], egui::Label::new(RichText::new(*start.as_ref().err().unwrap_or(&"")).color(ALARM)));
+            });
+            ui.horizontal(|ui| {
+                let ok = seed.is_ok() && start.is_ok();
+                if ui.add_enabled(ok, egui::Button::new("Create")).clicked() {
+                    if let (Ok(seed), Ok(start)) = (seed, start.clone()) {
+                        let area = self.new_game.area.checked_sub(1).map(|i| chosen.areas[i].clone());
+                        self.core.create_game_in(&chosen.name, seed, start, area.as_deref());
+                    }
                 }
             });
-            let games = self.core.games().to_vec();
-            if games.is_empty() {
-                ui.label("No games yet.");
-                return;
+        }
+        // The front's answers and the lobby's news, in a line that is always there.
+        ui.label(RichText::new(self.core.lobby_note().unwrap_or(" ")).color(ALARM));
+        ui.separator();
+        ui.horizontal(|ui| {
+            ui.label(RichText::new("Games").strong());
+            if ui.button("Refresh").clicked() {
+                self.core.refresh();
             }
-            let mut join = None;
-            let mut delete = None;
-            egui::Grid::new("games").striped(true).show(ui, |ui| {
-                for h in ["Game", "Layout", "State", "Time", "Areas", "Players", ""] {
-                    ui.label(RichText::new(h).strong());
-                }
-                ui.end_row();
-                for g in &games {
-                    ui.label(&g.id);
-                    ui.label(&g.layout);
-                    let state = match g.state {
-                        GameState::Running => "running".to_string(),
-                        GameState::Saved => "saved".to_string(),
-                        GameState::Crashed => format!("crashed: {}", g.error.as_deref().unwrap_or("?")),
-                    };
-                    ui.label(state);
-                    ui.label(fmt_hms(g.sim_time));
-                    let areas: Vec<String> =
-                        g.areas.iter().map(|a| format!("{} ({})", a.name, a.holder.as_deref().unwrap_or("robot"))).collect();
-                    ui.label(areas.join(", "));
-                    ui.label(g.players.join(", "));
-                    ui.horizontal(|ui| {
-                        if ui.button(if g.state == GameState::Running { "Join" } else { "Resume" }).clicked() {
-                            join = Some(g.id.clone());
-                        }
-                        // Owner decision 13: the front re-checks all of it.
-                        if g.can_delete {
-                            if self.confirm_delete.as_deref() == Some(g.id.as_str()) {
-                                ui.label(RichText::new("Delete for good?").color(ALARM));
-                                if ui.button("Yes, delete").clicked() {
-                                    delete = Some(g.id.clone());
-                                }
-                                if ui.button("Cancel").clicked() {
-                                    self.confirm_delete = None;
-                                }
-                            } else if ui.button("Delete").clicked() {
-                                self.confirm_delete = Some(g.id.clone());
+        });
+        let games = self.core.games().to_vec();
+        if games.is_empty() {
+            ui.label("No games yet.");
+            return;
+        }
+        let mut join = None;
+        let mut delete = None;
+        egui::Grid::new("games").striped(true).show(ui, |ui| {
+            for h in ["Game", "Layout", "By", "Last played", "State", "Time", "Areas", "Players", ""] {
+                ui.label(RichText::new(h).strong());
+            }
+            ui.end_row();
+            for g in &games {
+                ui.label(&g.id);
+                ui.label(&g.layout);
+                ui.label(g.creator.as_deref().unwrap_or("—"));
+                ui.label(g.last_played.map(client_core::form::utc).unwrap_or_default());
+                let state = match g.state {
+                    GameState::Running => "running".to_string(),
+                    GameState::Saved => "saved".to_string(),
+                    GameState::Crashed => format!("crashed: {}", g.error.as_deref().unwrap_or("?")),
+                };
+                ui.label(state);
+                ui.label(fmt_hms(g.sim_time));
+                let areas: Vec<String> =
+                    g.areas.iter().map(|a| format!("{} ({})", a.name, a.holder.as_deref().unwrap_or("robot"))).collect();
+                ui.label(areas.join(", "));
+                ui.label(g.players.join(", "));
+                ui.horizontal(|ui| {
+                    if ui.button(if g.state == GameState::Running { "Join" } else { "Resume" }).clicked() {
+                        join = Some(g.id.clone());
+                    }
+                    // Owner decision 13: the front re-checks all of it.
+                    if g.can_delete {
+                        if self.confirm_delete.as_deref() == Some(g.id.as_str()) {
+                            ui.label(RichText::new("Delete for good?").color(ALARM));
+                            if ui.button("Yes, delete").clicked() {
+                                delete = Some(g.id.clone());
+                            }
+                            if ui.button("Cancel").clicked() {
+                                self.confirm_delete = None;
                             }
+                        } else if ui.button("Delete").clicked() {
+                            self.confirm_delete = Some(g.id.clone());
                         }
-                    });
-                    ui.end_row();
-                }
-            });
-            if let Some(id) = join {
-                self.core.join(&id);
-            }
-            if let Some(id) = delete {
-                self.confirm_delete = None;
-                self.core.delete_game(&id);
+                    }
+                });
+                ui.end_row();
             }
         });
+        if let Some(id) = join {
+            self.core.join(&id);
+        }
+        if let Some(id) = delete {
+            self.confirm_delete = None;
+            self.core.delete_game(&id);
+        }
     }
 
     /// The lobby's Tutorial list: each lesson, ticked once done here.
diff --git a/crates/client-web/src/lib.rs b/crates/client-web/src/lib.rs
index e23e114..b817db6 100644
--- a/crates/client-web/src/lib.rs
+++ b/crates/client-web/src/lib.rs
@@ -24,6 +24,7 @@ use crate::transport::WebSocketTransport;
 struct WebApp {
     ui: UiApp,
     sent_to_login: bool,
+    sent_to_logout: bool,
 }
 
 impl eframe::App for WebApp {
@@ -35,6 +36,13 @@ impl eframe::App for WebApp {
                 let _ = w.location().set_href("/auth/login");
             }
         }
+        // The lobby's Sign out (polish spec M14).
+        if self.ui.wants_logout() && !self.sent_to_logout {
+            self.sent_to_logout = true;
+            if let Some(w) = web_sys::window() {
+                let _ = w.location().set_href("/auth/logout");
+            }
+        }
     }
 }
 
@@ -85,7 +93,7 @@ async fn run(canvas: web_sys::HtmlCanvasElement) -> Result<(), String> {
                 let settings = Box::new(LocalStore::new(SETTINGS_KEY));
                 let lessons = Box::new(LocalStore::new(LESSONS_KEY));
                 let ui = UiApp::with_stores(App::new(Box::new(transport), now), settings, lessons);
-                Ok(Box::new(WebApp { ui, sent_to_login: false }))
+                Ok(Box::new(WebApp { ui, sent_to_login: false, sent_to_logout: false }))
             }),
         )
         .await
diff --git a/crates/protocol/src/lobby.rs b/crates/protocol/src/lobby.rs
index 7dc5c8f..01637ad 100644
--- a/crates/protocol/src/lobby.rs
+++ b/crates/protocol/src/lobby.rs
@@ -43,7 +43,12 @@ pub enum LobbyMsg {
 #[serde(tag = "type", rename_all = "snake_case")]
 pub enum LobbyReply {
     Games { games: Vec<GameInfo> },
-    Layouts { layouts: Vec<LayoutInfo> },
+    /// `you`: the signed-in name, for the lobby to show (polish spec M14).
+    Layouts {
+        layouts: Vec<LayoutInfo>,
+        #[serde(default, skip_serializing_if = "Option::is_none")]
+        you: Option<String>,
+    },
     /// You are in `game` as `you`; its layout and view follow.
     Joined { game: String, you: String },
     Error { code: String, message: String },
@@ -75,6 +80,9 @@ pub struct GameInfo {
     /// Who created it; `None` for saves from before owner decision 13.
     #[serde(default, skip_serializing_if = "Option::is_none")]
     pub creator: Option<String>,
+    /// When it was last played, Unix seconds (saved games; polish spec M14).
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub last_played: Option<u64>,
     /// Whether the user this list was sent to may delete it now.
     #[serde(default, skip_serializing_if = "std::ops::Not::not")]
     pub can_delete: bool,
@@ -92,6 +100,12 @@ pub struct AreaHolder {
 pub struct LayoutInfo {
     pub name: String,
     pub areas: Vec<String>,
+    /// The world's title and the layout's one-line description (polish spec
+    /// M14); empty when the file has none.
+    #[serde(default, skip_serializing_if = "String::is_empty")]
+    pub title: String,
+    #[serde(default, skip_serializing_if = "String::is_empty")]
+    pub description: String,
 }
 
 /// `"type"` tags of `LobbyMsg`.
diff --git a/crates/server/src/layouts.rs b/crates/server/src/layouts.rs
index a0d2bae..c18c893 100644
--- a/crates/server/src/layouts.rs
+++ b/crates/server/src/layouts.rs
@@ -46,7 +46,9 @@ impl Layouts {
                 .as_array()
                 .and_then(|a| a.iter().map(|x| x["name"].as_str().map(str::to_string)).collect::<Option<Vec<String>>>())
                 .ok_or_else(|| format!("{}: no named areas", path.display()))?;
-            list.push(LayoutInfo { name: name.to_string(), areas });
+            let text_at = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
+            let (title, description) = (text_at(&v["title"]), text_at(&v["layout"]["description"]));
+            list.push(LayoutInfo { name: name.to_string(), areas, title, description });
         }
         list.sort_by(|a, b| a.name.cmp(&b.name));
         Ok(Layouts { dir: dir.to_path_buf(), list })
diff --git a/crates/server/src/supervisor.rs b/crates/server/src/supervisor.rs
index d6f7672..4e46835 100644
--- a/crates/server/src/supervisor.rs
+++ b/crates/server/src/supervisor.rs
@@ -295,7 +295,7 @@ impl Supervisor {
                 self.reply(user, conn, frame(LobbyReply::Games { games: self.list_games_for(user) }))
             }
             ClientFrame::Lobby(LobbyMsg::ListLayouts) => {
-                self.reply(user, conn, frame(LobbyReply::Layouts { layouts: self.layouts.infos() }))
+                self.reply(user, conn, frame(LobbyReply::Layouts { layouts: self.layouts.infos(), you: Some(user.to_string()) }))
             }
             ClientFrame::Lobby(LobbyMsg::Leave) => {
                 {
@@ -618,6 +618,7 @@ impl Supervisor {
                     players: vec![],
                     error: None,
                     creator: s.creator.clone(),
+                    last_played: Some(s.last_played),
                     can_delete: false,
                 },
                 Err(e) => GameInfo {
@@ -629,6 +630,7 @@ impl Supervisor {
                     players: vec![],
                     error: Some(e.clone()),
                     creator: None,
+                    last_played: None,
                     can_delete: false,
                 },
             };
@@ -649,6 +651,7 @@ impl Supervisor {
                 players: vec![],
                 error: None,
                 creator: None,
+                last_played: None,
                 can_delete: false,
             });
             info.layout = e.layout.clone();
diff --git a/crates/ts2-import/src/areas.rs b/crates/ts2-import/src/areas.rs
index eade2d7..45407db 100644
--- a/crates/ts2-import/src/areas.rs
+++ b/crates/ts2-import/src/areas.rs
@@ -24,6 +24,10 @@ pub struct AreasFile {
     #[serde(default)]
     pub boundaries: Vec<String>,
     pub areas: Vec<AreaSpec>,
+    /// One line for the lobby (polish spec M14), written into the world's
+    /// `layout` as `description`.
+    #[serde(default)]
+    pub description: Option<String>,
 }
 
 #[derive(Debug, Clone, PartialEq, Deserialize)]
@@ -141,6 +145,9 @@ pub fn apply(world: &mut WorldFile, spec: &AreasFile) -> Result<Vec<AreaCount>,
             spec.areas.iter().zip(letters).map(|(a, l)| (a.name.clone(), Value::String(l))).collect();
         layout.insert("box_prefix".into(), Value::String(prefix));
         layout.insert("workstations".into(), Value::Object(ws));
+        if let Some(d) = &spec.description {
+            layout.insert("description".into(), Value::String(d.clone()));
+        }
     }
     *world = out;
     Ok(counts)
diff --git a/layouts/drain.areas.json b/layouts/drain.areas.json
index 2cfc34b..dab7d31 100644
--- a/layouts/drain.areas.json
+++ b/layouts/drain.areas.json
@@ -1,5 +1,6 @@
 {
   "schema": 1,
+  "description": "The Waterloo & City line: two stations and a shuttle between them. The gentlest place to start.",
   "prefix": "W",
   "boundaries": ["73", "84"],
   "areas": [
diff --git a/layouts/gretz-armainvilliers.areas.json b/layouts/gretz-armainvilliers.areas.json
index 6ca520c..8418fcd 100644
--- a/layouts/gretz-armainvilliers.areas.json
+++ b/layouts/gretz-armainvilliers.areas.json
@@ -1,5 +1,6 @@
 {
   "schema": 1,
+  "description": "Gretz-Armainvilliers, east of Paris: a long main line and its branches, three signal boxes.",
   "prefix": "G",
   "boundaries": ["39,1V1", "39,1V2", "52,1"],
   "areas": [
diff --git a/layouts/liverpool-st.areas.json b/layouts/liverpool-st.areas.json
index 2439ed5..8b82aad 100644
--- a/layouts/liverpool-st.areas.json
+++ b/layouts/liverpool-st.areas.json
@@ -1,5 +1,6 @@
 {
   "schema": 1,
+  "description": "London Liverpool Street: a busy terminus and its approaches, worked from three workstations.",
   "prefix": "L",
   "boundaries": ["61", "64", "63", "66", "65", "68", "91", "90", "93", "92", "95", "94"],
   "areas": [
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-protocol -p signalbox-server -p signalbox-client-core -p signalbox-client-ui -p ts2-import -p signalbox-bot && scripts/cargo test -p signalbox-server --features dev-auth && scripts/wasm-build`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Browser client": "The lobby (polish spec M14, M15): who you are and Sign out (`UiApp::wants_logout`,
followed by `client-web`), layouts by title with the areas file's `description`, By and Last played, and a form
checked as typed (`client_core::form`)."

- [ ] **Step 6: Commit**

```bash
git add crates/client-core crates/client-core/src/app.rs crates/client-core/src/form.rs crates/client-core/src/lib.rs crates/client-ui crates/client-ui/src/screens.rs crates/client-web/src/lib.rs crates/protocol crates/protocol/src/lobby.rs crates/server crates/server/src/layouts.rs crates/server/src/supervisor.rs crates/ts2-import crates/ts2-import/src/areas.rs layouts/drain.areas.json layouts/gretz-armainvilliers.areas.json layouts/liverpool-st.areas.json CLAUDE.md
git commit -m "feat: a lobby that orients a newcomer, signs out, and checks the form where it is typed"
```

---

### Task 23: Lessons: a done step waits, words for both aspect modes, a box that does not move (UI review H4, H5, H6)

Spec §10 H4, H5, H6 (U4, U5, U6). A step may carry a `done` text: its task done, it says so and waits for Next. The
lesson box's buttons sit in a row above the text that never moves. Lesson 1's Real aspects step comes before the
train, with a signal showing proceed. The CI play-through presses Next on a done step, as a player would.

**Files:**
- Modify: `crates/client-ui/src/screens.rs`
- Modify: `crates/game/src/lesson/file.rs`
- Modify: `crates/game/src/lesson/run.rs`
- Modify: `crates/protocol/src/lesson.rs`
- Modify: `lessons/01-reading-the-panel/lesson.json`
- Modify: `lessons/02-setting-routes/lesson.json`
- Modify: `lessons/03-running-trains/lesson.json`
- Modify: `lessons/04-junctions-and-handovers/lesson.json`
- Modify: `CLAUDE.md`
- Test: `crates/client-core/tests/lessons.rs`
- Test: `crates/client-ui/tests/lesson.rs`
- Test: `crates/game/tests/lesson.rs`
- Test: `crates/game/tests/lessons.rs`
- Test: `crates/protocol/tests/golden.rs`

**Interfaces:**
- Consumes: `lesson::Runner::{settle, view}`, the lessons' `lesson.json` files, Task 8's points names in lesson 2.
- Produces: `Step.done: Option<String>`; `LessonView.{completed: bool, after: Option<String>}`; the runner's
  `Progress.completed` (a Next pressed before it does not count); `LESSON_NEXT_W = 72.0`; Enter presses Next.

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-core/tests/lessons.rs b/crates/client-core/tests/lessons.rs
index 6e8a7eb..ac2c021 100644
--- a/crates/client-core/tests/lessons.rs
+++ b/crates/client-core/tests/lessons.rs
@@ -19,6 +19,8 @@ fn lesson(index: u32, done: bool) -> ServerFrame {
         needs_next: false,
         done,
         alert: None,
+        completed: false,
+        after: None,
     }))
 }
 
diff --git a/crates/client-ui/tests/lesson.rs b/crates/client-ui/tests/lesson.rs
index b72d932..3057ed5 100644
--- a/crates/client-ui/tests/lesson.rs
+++ b/crates/client-ui/tests/lesson.rs
@@ -189,9 +189,14 @@ fn a_lesson_is_followed_through_the_lesson_box_and_the_diagram() {
     for gone in ["Penalty", "Release area", "Claim"] {
         assert!(!has_text(&out, gone), "{gone} is not offered in a lesson");
     }
-    r.click(find(&out, "Next"));
+    let next = find(&out, "Next");
+    r.click(next);
     let out = r.until("Step 2 of 10");
-    assert!(!has_text(&out, "Next"), "this step waits for a click on the diagram");
+    // Polish spec H4: Next stays where it was, greyed while the step waits
+    // for the diagram; pressing it does nothing.
+    assert_eq!(find(&out, "Next"), next, "the button row never moves");
+    r.click(next);
+    assert_eq!(r.step(), 1, "a greyed Next does nothing");
     // The step's highlight pulses round H3.
     let rings = out.shapes.iter().filter(|c| matches!(&c.shape, Shape::Circle(cs) if is_highlight(cs.stroke.color))).count();
     assert_eq!(rings, 1, "a highlight ring round H3");
@@ -204,6 +209,10 @@ fn a_lesson_is_followed_through_the_lesson_box_and_the_diagram() {
     r.until("Step 4 of 10");
     assert_eq!(r.ui.core.game().unwrap().selected(), Some("3"), "H3 is still the entrance");
     r.click(r.at(405.0, 0.0));
+    // Polish spec H5: the route is set and the step says so, waiting for Next.
+    let out = r.until("Done: the route is white");
+    assert_eq!((r.step(), find(&out, "Next")), (3, next));
+    r.events.push(Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::default() });
     r.until("Step 5 of 10");
     let out = r.frame();
     r.click(find(&out, "Restart step"));
diff --git a/crates/game/tests/lesson.rs b/crates/game/tests/lesson.rs
index a0d4284..3c9d48c 100644
--- a/crates/game/tests/lesson.rs
+++ b/crates/game/tests/lesson.rs
@@ -279,6 +279,8 @@ fn joining_claims_the_lessons_area_and_shows_the_first_step() {
             needs_next: true,
             done: false,
             alert: None,
+            completed: false,
+            after: None,
         }
     );
 }
@@ -602,3 +604,23 @@ fn joining_sends_the_layout_and_view_once() {
     let (_, out) = Rig::new(&hollins(), "Hollins Cross", json!([{"say": "x", "wait_for": next()}]));
     assert!(matches!(&out[..], [(_, ServerMsg::Layout(_)), (_, ServerMsg::View(_)), (_, ServerMsg::Lesson(_))]), "{out:?}");
 }
+
+/// Polish spec H5: a step with a `done` text, its task done, says so and
+/// waits for Next, so the player sees the result; a Next pressed early does
+/// not count.
+#[test]
+fn a_done_step_waits_for_next_after_its_task() {
+    let steps = json!([
+        {"say": "pause it", "done": "Paused: nothing moves.", "wait_for": {"clock": {"paused": true}}},
+        {"say": "end", "wait_for": next()}
+    ]);
+    let (mut rig, _) = Rig::new(&hollins(), "Hollins Cross", steps);
+    let v = rig.r.view();
+    assert_eq!((v.completed, v.needs_next, v.after.clone()), (false, false, None));
+    assert!(rig.send(ClientMsg::LessonNext).is_empty(), "too early");
+    let v = lessons(&rig.send(ClientMsg::Vote { proposal: Proposal::Pause }));
+    assert_eq!((v[0].index, v[0].completed, v[0].needs_next, v[0].after.as_deref()), (0, true, true, Some("Paused: nothing moves.")));
+    assert_eq!(rig.r.step(), 0, "it waits");
+    let v = lessons(&rig.send(ClientMsg::LessonNext));
+    assert_eq!((v[0].index, v[0].completed), (1, false));
+}
diff --git a/crates/game/tests/lessons.rs b/crates/game/tests/lessons.rs
index 4806b17..4622b5c 100644
--- a/crates/game/tests/lessons.rs
+++ b/crates/game/tests/lessons.rs
@@ -173,6 +173,11 @@ fn play(dir: &PathBuf) -> f64 {
         let since = p.g.sim().now_s();
         let mut ticks = 0u32;
         while p.r.step() == i {
+            // The task is done: the player looks, then presses Next (polish spec H5).
+            if p.r.view().completed {
+                p.send(ClientMsg::LessonNext);
+                continue;
+            }
             if trains {
                 p.drive();
             }
diff --git a/crates/protocol/tests/golden.rs b/crates/protocol/tests/golden.rs
index 92bc47d..a2e93f2 100644
--- a/crates/protocol/tests/golden.rs
+++ b/crates/protocol/tests/golden.rs
@@ -404,6 +404,8 @@ fn lesson_view() {
         needs_next: false,
         done: false,
         alert: Some(s("Restart the step.")),
+        completed: false,
+        after: None,
     };
     check_server(
         ServerMsg::Lesson(v.clone()),
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-game --test lesson a_done_step -p signalbox-client-ui --test lesson`
Expected: compile errors (no fields `completed`, `after`; the lesson file's `done` is an unknown field).

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index 38f7b27..5967da2 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -58,6 +58,8 @@ pub fn simplifier_columns(train_w: f32) -> [f32; 8] {
 pub fn table_width(cols: &[f32; 8]) -> f32 {
     cols.iter().sum::<f32>() + CELL_GAP * (cols.len() - 1) as f32
 }
+/// The lesson box's Next button is at least this wide (polish spec H4).
+const LESSON_NEXT_W: f32 = 72.0;
 /// The enquiry window opens this far right of and below where it was asked for.
 const ENQUIRY_OFFSET_PX: f32 = 16.0;
 /// The top bar's fixed widths (polish spec M5): the clock state (`paused`,
@@ -668,8 +670,11 @@ impl UiApp {
         });
     }
 
-    /// The lesson (tutorial spec §4): title, step, what to do, the alert,
-    /// and its buttons; on `done`, the way back to the lobby.
+    /// The lesson (tutorial spec §4; polish spec H4, H5): its title, then a
+    /// row of buttons that never moves — Next on the left (greyed while the
+    /// step waits for something else; Enter presses it), Restart step,
+    /// Restart lesson and Leave on the right — then the step, its text, a
+    /// done step's result and the alert. On `done`, the way back.
     fn lesson_box(&mut self, ui: &mut Ui) {
         let Some(v) = self.core.game().and_then(|g| g.lesson()).cloned() else { return };
         let mut act: Option<fn(&mut App)> = None;
@@ -685,25 +690,32 @@ impl UiApp {
                 }
             });
         } else {
+            ui.horizontal(|ui| {
+                let typing = ui.ctx().egui_wants_keyboard_input();
+                let enter = v.needs_next && !typing && ui.input(|i| i.key_pressed(Key::Enter));
+                if ui.add_enabled(v.needs_next, egui::Button::new("Next").min_size(vec2(LESSON_NEXT_W, 0.0))).clicked() || enter {
+                    act = Some(App::lesson_next);
+                }
+                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
+                    if ui.button("Leave").clicked() {
+                        act = Some(App::leave);
+                    }
+                    if ui.button("Restart lesson").clicked() {
+                        act = Some(App::lesson_restart);
+                    }
+                    if ui.button("Restart step").clicked() {
+                        act = Some(App::lesson_restart_step);
+                    }
+                });
+            });
             ui.label(format!("Step {} of {}", v.index + 1, v.count));
             ui.label(RichText::new(&v.say).size(14.0));
+            if v.completed {
+                ui.label(RichText::new(v.after.as_deref().unwrap_or("Done. Press Next.")).color(paint::GREEN).size(14.0));
+            }
             if let Some(a) = &v.alert {
                 ui.label(RichText::new(a).color(ALARM));
             }
-            ui.horizontal(|ui| {
-                if v.needs_next && ui.button("Next").clicked() {
-                    act = Some(App::lesson_next);
-                }
-                if ui.button("Restart step").clicked() {
-                    act = Some(App::lesson_restart_step);
-                }
-                if ui.button("Restart lesson").clicked() {
-                    act = Some(App::lesson_restart);
-                }
-                if ui.button("Leave").clicked() {
-                    act = Some(App::leave);
-                }
-            });
         }
         ui.separator();
         if let Some(f) = act {
diff --git a/crates/game/src/lesson/file.rs b/crates/game/src/lesson/file.rs
index dd473c6..2297287 100644
--- a/crates/game/src/lesson/file.rs
+++ b/crates/game/src/lesson/file.rs
@@ -34,6 +34,11 @@ pub struct Step {
     /// when `wait_for` alone does not say (`rejected`, say).
     #[serde(default)]
     pub solution: Vec<Move>,
+    /// Said once the step's task is done; the step then waits for Next, so
+    /// the player sees the result (polish spec H5). Without it the lesson
+    /// moves on at once.
+    #[serde(default)]
+    pub done: Option<String>,
 }
 
 /// What a step waits for.
diff --git a/crates/game/src/lesson/run.rs b/crates/game/src/lesson/run.rs
index ee4e579..a57eabf 100644
--- a/crates/game/src/lesson/run.rs
+++ b/crates/game/src/lesson/run.rs
@@ -28,6 +28,9 @@ pub const COLLISION_ALERT: &str = "Two trains collided. Press Restart step to tr
 struct Progress {
     next: bool,
     rejected: bool,
+    /// The task of a step with a `done` text is done: it now waits for
+    /// Next (polish spec H5).
+    completed: bool,
 }
 
 /// What has happened so far in the lesson, so that a step whose event came
@@ -118,9 +121,10 @@ impl Runner {
 
     pub fn view(&self) -> LessonView {
         let count = self.lesson.steps.len();
-        let (say, highlight, needs_next) = match self.lesson.steps.get(self.step) {
-            Some(s) => (s.say.clone(), s.highlight.clone(), s.wait_for.needs_next()),
-            None => (String::new(), vec![], false),
+        let completed = self.progress.completed;
+        let (say, highlight, needs_next, after) = match self.lesson.steps.get(self.step) {
+            Some(s) => (s.say.clone(), s.highlight.clone(), s.wait_for.needs_next() || completed, s.done.clone().filter(|_| completed)),
+            None => (String::new(), vec![], false, None),
         };
         LessonView {
             lesson: self.id.clone(),
@@ -132,6 +136,8 @@ impl Runner {
             needs_next,
             done: self.done(),
             alert: self.alert.clone(),
+            completed,
+            after,
         }
     }
 
@@ -333,15 +339,30 @@ impl Runner {
         if self.alert.is_some() {
             return vec![];
         }
-        let mut moved = false;
+        let mut changed = false;
         while let Some(step) = self.lesson.steps.get(self.step) {
-            if !self.met(g, &step.wait_for) {
-                break;
+            // A step with a `done` text waits for Next once its task is done
+            // (polish spec H5); any other moves on as soon as its condition holds.
+            if self.progress.completed {
+                if !self.progress.next {
+                    break;
+                }
+            } else {
+                if !self.met(g, &step.wait_for) {
+                    break;
+                }
+                if step.done.is_some() && !step.wait_for.needs_next() {
+                    self.progress.completed = true;
+                    // A Next pressed before the task was done does not count.
+                    self.progress.next = false;
+                    changed = true;
+                    break;
+                }
             }
             self.begin(g, self.step + 1);
-            moved = true;
+            changed = true;
         }
-        if moved { self.message(g) } else { vec![] }
+        if changed { self.message(g) } else { vec![] }
     }
 
     fn met(&self, g: &Game, c: &Condition) -> bool {
diff --git a/crates/protocol/src/lesson.rs b/crates/protocol/src/lesson.rs
index 8eb6131..6144122 100644
--- a/crates/protocol/src/lesson.rs
+++ b/crates/protocol/src/lesson.rs
@@ -50,4 +50,10 @@ pub struct LessonView {
     /// Said after a SPAD or a collision: the step can be restarted.
     #[serde(default, skip_serializing_if = "Option::is_none")]
     pub alert: Option<String>,
+    /// The step's task is done and it waits for Next, so the player sees
+    /// what happened (polish spec H5); `after` says what to look at.
+    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
+    pub completed: bool,
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub after: Option<String>,
 }
diff --git a/lessons/01-reading-the-panel/lesson.json b/lessons/01-reading-the-panel/lesson.json
index 226a381..397ad55 100644
--- a/lessons/01-reading-the-panel/lesson.json
+++ b/lessons/01-reading-the-panel/lesson.json
@@ -29,22 +29,23 @@
       "wait_for": {"continue": {}}
     },
     {
-      "say": "A train is coming. Its headcode 2S01 has appeared in the berth of S1, and the track under it has turned red: red track means a train is on it. The train will stop at S1, because S1 is red. Press Next.",
+      "say": "The lesson has set a route from S3 to S5, so S3 shows a proceed aspect. On a signal box panel a signal shows only red (stop) or green (go). Open Settings at the top and choose Real aspects to see what the driver sees: S3 turns yellow, meaning 'the next signal is red'. Switch back to Red/green if you like, then press Next.",
+      "highlight": [{"ui": "settings"}, {"signal": "3"}],
+      "do": [{"set_route": {"entrance": "3", "exit": {"kind": "signal", "name": "5"}}}],
+      "wait_for": {"continue": {}}
+    },
+    {
+      "say": "The lesson has cancelled S3's route. A train is coming. Its headcode 2S01 has appeared in the berth of S1, and the track under it has turned red: red track means a train is on it. The train will stop at S1, because S1 is red. Press Next.",
       "highlight": [{"berth": "B1"}],
-      "do": [{"spawn": {"headcode": "2S01", "entry": "W"}}],
+      "do": [{"cancel_route": {"entrance": "3"}}, {"spawn": {"headcode": "2S01", "entry": "W"}}],
       "wait_for": {"all": [{"berth": {"name": "B1", "headcode": "2S01"}}, {"continue": {}}]}
     },
     {
-      "say": "This time the lesson sets a route for you, from S1 to S3. A route is the path a train may take; it shows white. S1 now shows green, so the driver may go. Watch the train run through the station and stop at S3. Press Next.",
+      "say": "This time the lesson sets a route for you, from S1 to S3. A route is the path a train may take; it shows white. S1 now shows a proceed aspect, so the driver may go. Watch the train run through the station and stop at S3. Press Next.",
       "highlight": [{"signal": "1"}, {"section": "TB"}],
       "do": [{"set_route": {"entrance": "1", "exit": {"kind": "signal", "name": "3"}}}],
       "wait_for": {"all": [{"route_set": {"entrance": "1", "exit": {"kind": "signal", "name": "3"}}}, {"continue": {}}]}
     },
-    {
-      "say": "On a signal box panel a signal shows only red (stop) or green (go). Open Settings at the top and choose Real aspects to see what the driver sees: with yellow meaning 'the next signal is red'. Switch back to Red/green if you like, then press Next.",
-      "highlight": [{"ui": "settings"}],
-      "wait_for": {"continue": {}}
-    },
     {
       "say": "The lesson now clears the way out: S3 to S5, and S5 to the edge of your area. Watch 2S01 leave. Behind it the track turns grey again as it is freed.",
       "do": [
diff --git a/lessons/02-setting-routes/lesson.json b/lessons/02-setting-routes/lesson.json
index 5aad0cc..e2bce82 100644
--- a/lessons/02-setting-routes/lesson.json
+++ b/lessons/02-setting-routes/lesson.json
@@ -21,11 +21,13 @@
     },
     {
       "say": "Click H5 to set the route from H3 to H5. The points are set for platform 1, the route turns white, and H3 turns green. The short white piece past H5 is the overlap: spare track kept clear in case a train runs a little past a red signal. (If H3 is no longer chosen, click H3 first.)",
+      "done": "Done: the route is white and H3 shows a proceed aspect. Look, then press Next.",
       "highlight": [{"exit": {"kind": "signal", "name": "5"}}],
       "wait_for": {"route_set": {"entrance": "3", "exit": {"kind": "signal", "name": "5"}}}
     },
     {
       "say": "Now cancel the route. Right-click H3 and choose 'Cancel route H3 to H5'. The white track goes grey and H3 goes back to red.",
+      "done": "Done: the track is grey again and H3 is back to red. Press Next.",
       "highlight": [{"signal": "3"}],
       "wait_for": {"route_cancelled": {"entrance": "3"}}
     },
@@ -49,6 +51,7 @@
     },
     {
       "say": "Now set the route from H3 to H7, into platform 2. It uses the points as they lie.",
+      "done": "Done: the route into platform 2 is set over the reversed points. Press Next.",
       "highlight": [{"signal": "3"}, {"exit": {"kind": "signal", "name": "7"}}],
       "wait_for": {"route_set": {"entrance": "3", "exit": {"kind": "signal", "name": "7"}}}
     },
diff --git a/lessons/03-running-trains/lesson.json b/lessons/03-running-trains/lesson.json
index 5d6b2b2..032f7e1 100644
--- a/lessons/03-running-trains/lesson.json
+++ b/lessons/03-running-trains/lesson.json
@@ -36,6 +36,7 @@
     },
     {
       "say": "Watch the train. As it leaves each track circuit, the white route behind it turns grey: this is sectional release, which frees track for other routes as soon as the train has passed. The step ends when 2H05 stands at platform 2.",
+      "done": "2H05 stands at platform 2, and the route behind it has gone grey. Press Next.",
       "highlight": [{"platform": {"place": "HXC", "platform": "2"}}],
       "wait_for": {"train_at": {"headcode": "2H05", "place": "HXC", "platform": "2"}}
     },
diff --git a/lessons/04-junctions-and-handovers/lesson.json b/lessons/04-junctions-and-handovers/lesson.json
index 79e3633..b52062d 100644
--- a/lessons/04-junctions-and-handovers/lesson.json
+++ b/lessons/04-junctions-and-handovers/lesson.json
@@ -38,6 +38,7 @@
     },
     {
       "say": "Beside KA5 is a small blue circle marked A: the auto-working button. Click it. It fills in, and the route from KA5 now stays set after each train, so the trains behind get a clear signal without you setting it again. Auto-working suits plain line with no junction. (If the route has already gone, set KA5 to KB7 again first, then click the circle.)",
+      "done": "Done: the circle is filled, so auto-working is on. Press Next.",
       "highlight": [{"ui": "auto:5"}],
       "wait_for": {"auto_working": {"signal": "5", "on": true}}
     },
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-protocol -p signalbox-game -p signalbox-client-core -p signalbox-client-ui -p signalbox-server && scripts/cargo test -p signalbox-game --test lessons -- --nocapture`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Document**

`CLAUDE.md`, "Tutorials": "A step may carry a `done` text (polish spec H5): once its task is done it says so and
waits for Next; the CI play-through presses Next there. The lesson box's buttons sit above the text and never move;
Enter is Next (H4)."

- [ ] **Step 6: Commit**

```bash
git add crates/client-core crates/client-ui crates/client-ui/src/screens.rs crates/game crates/game/src/lesson/file.rs crates/game/src/lesson/run.rs crates/protocol crates/protocol/src/lesson.rs lessons/01-reading-the-panel/lesson.json lessons/02-setting-routes/lesson.json lessons/03-running-trains/lesson.json lessons/04-junctions-and-handovers/lesson.json CLAUDE.md
git commit -m "feat: lesson steps can wait to show their result; a lesson box that does not move; texts for both aspect modes"
```

---

### Task 24: Tutorial highlights that stand out and keep clear (UI review M16)

Spec §10 M16 (U22). A stronger pulse with a black underlay, points outlined along their legs, and a signal's ring
kept clear so the placer moves its number off it (the plan cache is keyed by the highlights too).

**Files:**
- Modify: `crates/client-ui/src/paint.rs`
- Modify: `crates/client-ui/src/screens.rs`
- Test: `crates/client-ui/tests/paint.rs`

**Interfaces:**
- Consumes: Task 1's `KeepClear.rounds`, Task 3's `PlacementKey`.
- Produces: `paint::{HIGHLIGHT_MIN_ALPHA = 0.6, HIGHLIGHT_UNDER_PX = 2.0}`; `PlacementKey` gains `Vec<Highlight>`.

- [ ] **Step 1: Write the failing tests**

Apply (written and run on the scratch copy; re-check the context on the base, keep the intent):

```diff
diff --git a/crates/client-ui/tests/paint.rs b/crates/client-ui/tests/paint.rs
index 8176e7c..368259b 100644
--- a/crates/client-ui/tests/paint.rs
+++ b/crates/client-ui/tests/paint.rs
@@ -725,7 +725,7 @@ fn a_lesson_highlight_outlines_what_it_names_and_pulses() {
         (Highlight::Signal(s("W1")), 1),
         (Highlight::Exit(ExitName::Signal(s("A"))), 1),
         (Highlight::Exit(ExitName::Node(s("E"))), 1),
-        (Highlight::Points(s("P")), 1),
+        (Highlight::Points(s("P")), 6),
         (Highlight::Berth(s("BA")), 1),
         (Highlight::Section(s("TW2")), 2),
         (Highlight::Section(s("TP")), 6),
@@ -744,9 +744,9 @@ fn a_lesson_highlight_outlines_what_it_names_and_pulses() {
     let d = with(&[Highlight::Signal(s("W1"))], 0.0);
     let Shape::Circle(c) = highlighted(&d)[0] else { panic!() };
     assert!(close(c.center, signal_disc(&r.cam, screen(), w1)), "round the lamp");
-    // 1 Hz between a third and full strength.
+    // 1 Hz between 60 % and full strength (polish spec M16).
     assert_eq!(highlight_colour(0.25).a(), 255);
-    assert_eq!(highlight_colour(0.75).a(), 89);
+    assert_eq!(highlight_colour(0.75).a(), 153);
     assert_eq!(highlight_colour(1.25), highlight_colour(0.25));
 }
 
@@ -852,3 +852,40 @@ fn signal_glyphs_grow_when_zoomed_in() {
     let n = d.texts.iter().find(|t| t.text == "TAW1").unwrap();
     assert_eq!(n.size, NUMBER_MAX_PX * 2.0);
 }
+
+/// Polish spec M16: a highlight is drawn over a black underlay, a points
+/// highlight outlines the legs (no ring takes in the signals beside them),
+/// and a signal's ring is kept clear so the placer moves its number off it.
+#[test]
+fn highlights_stand_out_and_keep_clear_of_labels() {
+    use client_ui::labels::{Role, plan};
+    let r = Rig::new(Some("West"));
+    let with = |h: &[Highlight]| {
+        let st = PaintState {
+            view: Some(&r.view),
+            selected: None,
+            exits: &[],
+            refused: None,
+            blocking: None,
+            time: 0.0,
+            aspects: AspectMode::RedGreen,
+            numbers: true,
+            names: &r.names,
+            highlight: h,
+        };
+        draw(&r.sc, &r.cam, screen(), &st)
+    };
+    let d = with(&[Highlight::Points("P".into())]);
+    assert!(highlighted(&d).iter().all(|s| matches!(s, Shape::LineSegment { .. })), "legs, no ring");
+    let under = d.shapes.iter().filter(|s| matches!(s, Shape::LineSegment { stroke, .. } if stroke.color == BG && stroke.width == HIGHLIGHT_W + HIGHLIGHT_UNDER_PX)).count();
+    assert_eq!(under, 6, "a black line under each");
+    let d = with(&[Highlight::Signal("W1".into())]);
+    let ring = d.keep.rounds.iter().find(|(c, rad)| close(*c, r.disc("W1")) && *rad > LAMP_R + HIGHLIGHT_GAP_PX).copied();
+    assert!(ring.is_some(), "the ring is kept clear");
+    let p = plan(&d, &mut |t| vec2(t.text.chars().count() as f32 * 6.0, 10.0));
+    let w1 = d.movable.iter().position(|m| m.role == Role::Number && d.texts[m.text].text == "TAW1").unwrap();
+    let (off, anchor) = p.spots[w1].expect("drawn");
+    let t = &d.texts[d.movable[w1].text];
+    let at = anchor.anchor_size(t.at + off, vec2(24.0, 10.0));
+    assert!(!client_ui::labels::touches_round(at, ring.unwrap()), "TAW1 moved off the ring: {at:?}");
+}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-ui --test paint highlight`
Expected: compile error (`HIGHLIGHT_UNDER_PX` not found); with it stubbed, alpha is 89, not 153, and the points highlight is a ring.

- [ ] **Step 3: Implement**

```diff
diff --git a/crates/client-ui/src/paint.rs b/crates/client-ui/src/paint.rs
index a7db69c..050f9be 100644
--- a/crates/client-ui/src/paint.rs
+++ b/crates/client-ui/src/paint.rs
@@ -38,6 +38,11 @@ pub const HIGHLIGHT: Color32 = Color32::from_rgb(0xFF, 0x8C, 0x1A);
 /// The highlight's outline: this wide, and this far round what it marks.
 pub const HIGHLIGHT_W: f32 = 2.5;
 pub const HIGHLIGHT_GAP_PX: f32 = 5.0;
+/// The pulse's weakest strength (polish spec M16).
+pub const HIGHLIGHT_MIN_ALPHA: f64 = 0.6;
+/// Each highlight stroke is drawn over a black one this much wider, so it
+/// stands out on ochre platforms and grey track alike (polish spec M16).
+pub const HIGHLIGHT_UNDER_PX: f32 = 2.0;
 
 /// Track width: this many pixels per layout unit, within the limits.
 pub const TRACK_UNITS: f32 = 9.0;
@@ -146,11 +151,12 @@ pub fn blink_on(time: f64) -> bool {
     (time * 4.0).floor().rem_euclid(2.0) == 0.0
 }
 
-/// A tutorial highlight's colour at `time`: a calm 1 Hz pulse between a
-/// third and full strength (tutorial spec §4: UI, not panel state).
+/// A tutorial highlight's colour at `time`: a calm 1 Hz pulse between 60 %
+/// and full strength (tutorial spec §4: UI, not panel state; polish spec
+/// M16: never a dim brown).
 pub fn highlight_colour(time: f64) -> Color32 {
     let k = 0.5 + 0.5 * (time * std::f64::consts::TAU).sin();
-    let a = (255.0 * (0.35 + 0.65 * k)).round().clamp(0.0, 255.0) as u8;
+    let a = (255.0 * (HIGHLIGHT_MIN_ALPHA + (1.0 - HIGHLIGHT_MIN_ALPHA) * k)).round().clamp(0.0, 255.0) as u8;
     Color32::from_rgba_unmultiplied(HIGHLIGHT.r(), HIGHLIGHT.g(), HIGHLIGHT.b(), a)
 }
 
@@ -614,11 +620,20 @@ fn highlight_shapes(d: &mut Drawing, scene: &Scene, cam: &Camera, screen: Rect,
     }
     let colour = highlight_colour(st.time);
     let stroke = Stroke::new(HIGHLIGHT_W, colour);
+    let under = Stroke::new(HIGHLIGHT_W + HIGHLIGHT_UNDER_PX, BG);
     let to = |p: Pos2| cam.to_screen(screen, p);
     let w = track_w(cam.scale);
-    let ring = |d: &mut Drawing, c: Pos2, r: f32| d.shapes.push(Shape::circle_stroke(c, r, stroke));
+    // A ring is kept clear of texts too: the placer moves a number off it
+    // (polish spec M16).
+    let ring = |d: &mut Drawing, c: Pos2, r: f32| {
+        d.shapes.push(Shape::circle_stroke(c, r, under));
+        d.shapes.push(Shape::circle_stroke(c, r, stroke));
+        d.keep.rounds.push((c, r + HIGHLIGHT_W));
+    };
     let boxed = |d: &mut Drawing, r: Rect| {
-        d.shapes.push(Shape::rect_stroke(r.expand(HIGHLIGHT_GAP_PX - 2.0), CornerRadius::same(2), stroke, StrokeKind::Outside));
+        let r = r.expand(HIGHLIGHT_GAP_PX - 2.0);
+        d.shapes.push(Shape::rect_stroke(r, CornerRadius::same(2), under, StrokeKind::Outside));
+        d.shapes.push(Shape::rect_stroke(r, CornerRadius::same(2), stroke, StrokeKind::Outside));
     };
     // Both sides of a bar, clear of it.
     let along = |d: &mut Drawing, a: Pos2, b: Pos2| {
@@ -628,6 +643,7 @@ fn highlight_shapes(d: &mut Drawing, scene: &Scene, cam: &Camera, screen: Rect,
         }
         let n = vec2(-v.y, v.x).normalized() * (w / 2.0 + HIGHLIGHT_GAP_PX);
         for side in [n, -n] {
+            d.shapes.push(Shape::line_segment([a + side, b + side], under));
             d.shapes.push(Shape::line_segment([a + side, b + side], stroke));
         }
     };
@@ -644,9 +660,13 @@ fn highlight_shapes(d: &mut Drawing, scene: &Scene, cam: &Camera, screen: Rect,
                     boxed(d, Rect::from_center_size(to(e.at), vec2(7.0, 7.0)));
                 }
             }
+            // Points: their legs outlined, not a ring that takes in the
+            // signals beside them (polish spec M16).
             Highlight::Points(p) => {
                 if let Some(m) = scene.points.iter().find(|m| m.name == *p) {
-                    ring(d, to(m.at), w + HIGHLIGHT_GAP_PX * 2.0);
+                    for leg in [m.toe, m.normal, m.reverse].into_iter().flatten() {
+                        along(d, to(m.at), to(leg));
+                    }
                 }
             }
             Highlight::Berth(b) => {
diff --git a/crates/client-ui/src/screens.rs b/crates/client-ui/src/screens.rs
index 5967da2..c383a40 100644
--- a/crates/client-ui/src/screens.rs
+++ b/crates/client-ui/src/screens.rs
@@ -152,7 +152,9 @@ pub struct UiApp {
     ticks_store: Option<Box<dyn SettingsStore>>,
 }
 
-type PlacementKey = (String, u64, u32, bool);
+/// (game, layout generation, scale bits, numbers on, the lesson's highlights:
+/// a highlight ring is kept clear, polish spec M16).
+type PlacementKey = (String, u64, u32, bool, Vec<Highlight>);
 
 impl UiApp {
     pub fn new(core: App) -> UiApp {
@@ -991,7 +993,7 @@ impl UiApp {
             highlight: &highlight,
         };
         let d = paint::draw(scene, &cam, rect, &st);
-        let key = (g.id.clone(), g.layout_gen(), cam.scale.to_bits(), self.settings.numbers);
+        let key = (g.id.clone(), g.layout_gen(), cam.scale.to_bits(), self.settings.numbers, highlight.clone());
         if self.placement.as_ref().is_none_or(|(k, p)| *k != key || p.spots.len() != d.movable.len()) {
             let plan = ui.ctx().fonts_mut(|f| {
                 labels::plan(&d, &mut |t| f.layout_no_wrap(t.text.clone(), paint::font(t), t.colour).size())
```

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p signalbox-client-core -p signalbox-client-ui`
Expected: PASS, no warnings (as on the scratch copy).

- [ ] **Step 5: Commit**

```bash
git add crates/client-ui crates/client-ui/src/paint.rs crates/client-ui/src/screens.rs
git commit -m "feat(client-ui): tutorial highlights stand out and keep clear of the labels"
```

---

### Task 25: Final verification

**Files:** none (fixes go back to the task that owns them).

- [ ] **Step 1: The CI gate's builds, warnings as errors**

Run (CLAUDE.md's way of passing environment into the container):

```bash
docker run --rm -u "$(id -u):$(id -g)" -v "$PWD:/w" -w /w -e CARGO_HOME=/w/.cargo-home -e RUSTFLAGS="-D warnings" \
  rust:1.98-slim-bookworm sh -c 'cargo build --workspace --all-targets --locked && cargo test --workspace --locked \
  && cargo build -p signalbox-bot --no-default-features --locked \
  && cargo test -p signalbox-server --features dev-auth --locked'
```

Expected: all green, no warnings (as on the scratch copy).

- [ ] **Step 2: The browser client and the browser check**

Run: `scripts/wasm-build && deploy/browser-check.sh --no-build`
Expected: the three `ok` lines. Look at `target/browser-check/webgl2-lobby.png` (the centred lobby, its first line,
"Signed in as check", Sign out) and `webgl2-game.png` (the hint line at the diagram's foot, + and − in its corner).

- [ ] **Step 3: The lessons and the numbers for the owner**

Run: `scripts/cargo test -p signalbox-game --test lessons -- --nocapture` (every lesson to the end; note each one's sim
seconds) and `scripts/cargo test --release -p signalbox-client-ui --test legibility -- --nocapture`.
Expected: PASS; copy the 1280x800 and 1920x1080 rows at zoom 1 (with their `all`/`read` column) and the plan times
into the branch report, with the browser-check screenshots.

---

## Controller section (after the branch's final review; not for subagents)

The owner has agreed to redeploy as the realism pass did.

1. **CI cache:** nothing to reseed (no new crates).
2. **Deploy:** as "Build and run" from the merged commit (with the owner's WTT PDF in `external/wtt/`, as since the drain-wtt plan), then `deploy/smoke.sh https://ra.tail3e0c1e.ts.net:50160 303`. The front passes `--current-layout` on resume. Roll back by retagging the previous `<rev>`.
3. **Old saves:** join an existing pre-realism Drain save in the lobby; its signals must read `WA…`/`WB…` and `docker logs signalbox` show `display data from layout drain`. Its timetable still ends at 06:43 (spec P9).
4. **Owner's look (morning):** the `legibility` table and the browser-check screenshots in the branch report; then in Chrome/Edge and Firefox on the tailnet, at 1024 and 1920 px wide: the lobby (first line, Signed in, layout descriptions, Signal list, a bad seed explained in place); create Liverpool Street choosing an area (you signal at once); box A at Fit (numbers clear of the next platform road); the top bar while a second player votes (nothing moves; Agree/Decline; the outcome logged); a refused route (the blocking route named and outlined); hover hints and the hand cursor; Hide panel; zoom with the wheel, + and −; Gretz box A at Fit (readable, round Gretz); points swinging; Release area (asks); Leave (the lobby shows the area free); lessons 1 and 2 (the Next row stays put, done steps wait, Real aspects shows yellow at S3, highlights). **Then the proposed decisions U1–U22 (spec §10.2) for the owner's OK.**
