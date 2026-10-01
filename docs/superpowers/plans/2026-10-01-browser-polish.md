# Browser Polish (D1.2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A diagram legible at every zoom (no text over text, numbers off the track), a Drain timetable that runs until 14:00, old Drain saves showing today's `W…` names, the simplifier opening at "now", and a real-browser check of the WebGL2 fallback.

**Architecture:** `paint::draw` stays pure and now records which texts may move (role + other spots) and what must be kept clear; a new pure `labels` module plans placement greedily by priority and `UiApp` caches the plan per zoom. The converter gains a per-layout `--repeat` file (a pure `WorldFile` transform). Resume optionally takes the current layout file and swaps in its display-only `layout` JSON when the saved network matches. A shell + Python script drives Playwright Chromium against a throwaway dev-login front.

**Tech Stack:** Rust 1.98 (Docker `scripts/cargo`), egui 0.36 (headless tests), serde_json, rusqlite, tokio; Playwright 1.55 Chromium (Docker image on ra).

**Spec:** `docs/superpowers/specs/2026-10-01-browser-polish-design.md`

**Base:** `main` **after the `tutorial` branch merges** (this plan was written against `5eb82f0`). Before Task 1, re-check every file and line reference below against the merged code — the tutorial edits `crates/server/src/process.rs`, `crates/server/src/supervisor.rs` and `crates/game/src/game.rs`, which Tasks 6 and 7 touch, and may add lesson highlights to `crates/client-ui/src/paint.rs`. Every Rust and script block in this plan was compiled and its tests run (and `deploy/browser-check.sh` run end to end) on a scratch copy of `5eb82f0`; the `deploy/Dockerfile` loop was not built (the controller's deploy exercises it). Where the merged code differs, keep the intent and the tests.

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
- Drain repeat (spec P8): `every` 00:10:00, `until` 14:00:00, `headcode_step` 2 → 192 services, last BW96/WB96.
- Infra (deploying, the CI runner, `/opt/stack`, `tailscale serve`) is controller-only, in the final Controller section. Subagents may run `scripts/cargo`, `scripts/wasm-build` and `deploy/browser-check.sh` (it starts and removes its own throwaway container), and must not touch any other container, image, volume or network.

## Review Focus

1. **Nonsense geometry reaching the placer** — coordinates of ±1e9 or NaN from a bad layout, a text that measures NaN: `plan` must return within a second, place nothing at a non-finite offset, and hide what it cannot measure rather than draw it somewhere odd. Pinned in Task 1 (`nonsense_geometry_stays_cheap_and_finite`).
2. **A save whose layout the front no longer lists** (renamed or removed from the image) or whose layout file is unreadable: it must still resume, with its own display data, exactly as today. Pinned in Task 7 (`a_save_of_a_layout_no_longer_listed_still_resumes`) and Task 6 (`a_different_network_or_a_bad_file_keeps_the_saves_own`).
3. **A repeat file that adds nothing** (an `until` before the next repeat): the converted timetable must come out byte-identical, ends included. Pinned in Task 4 (`an_until_before_the_next_repeat_changes_nothing`).
4. **A stale placement plan** applied to a different drawing (numbers toggled, a claim changing the layout, a tutorial pushing extra texts): nothing may be moved by another drawing's offsets. Pinned in Task 1 (`apply_drops_hidden_texts_and_points_the_rest_at_their_new_index`: a plan of the wrong length changes nothing) and Task 3 (the cache key holds game, layout generation, scale and the numbers setting).
5. **Train movement re-placing labels** (flicker): placement must be identical with and without a headcode in a berth and after a pan. Pinned in Task 2 (`a_plan_depends_on_neither_pan_nor_trains`).

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
| `crates/ts2-import/src/repeat.rs` (new), `src/lib.rs`, `src/main.rs` | 4 | The repeat rule and the `--repeat` flag |
| `crates/ts2-import/tests/repeat.rs` (new), `tests/cli.rs`, `tests/soak.rs` | 4 | Rule, errors, Drain, CLI, soak |
| `layouts/drain.repeat.json` (new), `deploy/Dockerfile` | 4 | Drain's pattern; the image converts with it |
| `crates/client-core/src/simplifier.rs`, `tests/simplifier.rs` | 5 | `last_time`, `now_line` |
| `crates/game/src/save.rs`, `src/game.rs`, `src/lib.rs`, `tests/refresh.rs` (new) | 6 | `refresh_display`, `Game::resume_with_layout`, `Refresh` |
| `crates/server/src/process.rs`, `src/supervisor.rs`, `tests/process.rs`, `tests/supervisor.rs` | 7 | `--current-layout`; the front passes it on resume |
| `deploy/browser-check.sh`, `deploy/browser-check.py` (new), `deploy/README.md` | 8 | The real-browser renderer check |
| `CLAUDE.md` | 3, 4, 6, 7, 8 | One paragraph per change, in the task that makes it |

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

### Task 4: Repeat timetables (Drain until 14:00)

**Files:**
- Create: `crates/ts2-import/src/repeat.rs`, `layouts/drain.repeat.json`, `crates/ts2-import/tests/repeat.rs`
- Modify: `crates/ts2-import/src/lib.rs`, `crates/ts2-import/src/main.rs`, `crates/ts2-import/tests/cli.rs`, `crates/ts2-import/tests/soak.rs`, `deploy/Dockerfile`, `deploy/README.md`, `CLAUDE.md`

**Interfaces:**
- Consumes: `signalbox_core::world::file::{WorldFile, ServiceFile, CallFile, EndFile}`, `signalbox_core::time::{parse_hms, fmt_hms}`.
- Produces: `ts2_import::repeat::{RepeatSpec { schema: u32, every: String, until: String, headcode_step: u32 }, RepeatError, parse(&str) -> Result<RepeatSpec, RepeatError>, apply(&mut WorldFile, &RepeatSpec) -> Result<(), RepeatError>}`; CLI flag `--repeat <repeat.json>` (applied after `--areas` and `--lines`). Task 5 uses `layouts/drain.repeat.json`.

- [ ] **Step 1: Write the failing tests**

Create `crates/ts2-import/tests/repeat.rs`:

```rust
//! Repeating a timetable (polish spec §4): the rule on a small shuttle, its
//! hard errors, and Drain's shipped file.

use signalbox_core::world::World;
use signalbox_core::world::file::{EndFile, WorldFile};
use ts2_import::repeat::{RepeatError, RepeatSpec, apply, parse};

const DRAIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/drain.json");
const DRAIN_REPEAT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../layouts/drain.repeat.json");

fn spec(every: &str, until: &str, step: u32) -> RepeatSpec {
    RepeatSpec { schema: 1, every: every.into(), until: until.into(), headcode_step: step }
}

/// A one-train shuttle X ⇄ Y every 20 minutes, written out for two rounds:
/// A01 06:00 X→Y forms B01 06:10 Y→X forms A03 06:20 forms B03 06:30, stables.
fn shuttle() -> WorldFile {
    let svc = |h: &str, from: &str, to: &str, dep: &str, arr: &str, end: &str| {
        format!(
            r#"{{"headcode": "{h}", "train_type": "T", "calls": [
                {{"place": "{from}", "platform": "1", "dep": "{dep}"}},
                {{"place": "{to}", "platform": "1", "arr": "{arr}"}}], "end": {end}}}"#
        )
    };
    let form = |s: &str| format!(r#"{{"kind": "form", "service": "{s}"}}"#);
    let services = [
        svc("A01", "X", "Y", "06:00:00", "06:05:00", &form("B01")),
        svc("B01", "Y", "X", "06:10:00", "06:15:00", &form("A03")),
        svc("A03", "X", "Y", "06:20:00", "06:25:00", &form("B03")),
        svc("B03", "Y", "X", "06:30:00", "06:35:00", r#"{"kind": "stable"}"#),
    ]
    .join(",");
    serde_json::from_str(&format!(
        r#"{{"schema": 1, "areas": [], "sections": [], "nodes": [], "segments": [],
            "services": [{services}], "entries": [{{"service": "A01", "boundary": "X", "time": "05:59:00"}}]}}"#
    ))
    .unwrap()
}

fn heads(w: &WorldFile) -> Vec<&str> {
    w.services.iter().map(|s| s.headcode.as_str()).collect()
}

fn end_of<'a>(w: &'a WorldFile, h: &str) -> &'a EndFile {
    &w.services.iter().find(|s| s.headcode == h).unwrap().end
}

fn forms(w: &WorldFile, h: &str) -> Option<String> {
    match end_of(w, h) {
        EndFile::Form { service } => Some(service.clone()),
        _ => None,
    }
}

#[test]
fn the_file_is_checked() {
    let ok = r#"{"schema": 1, "every": "00:10:00", "until": "14:00:00", "headcode_step": 2}"#;
    assert_eq!(parse(ok).unwrap(), spec("00:10:00", "14:00:00", 2));
    assert!(matches!(parse(r#"{"schema": 1, "every": "00:10:00", "until": "14:00", "headcode_step": 2, "x": 1}"#), Err(RepeatError::Parse(_))));
    assert_eq!(parse(r#"{"schema": 2, "every": "00:10:00", "until": "14:00", "headcode_step": 2}"#), Err(RepeatError::Schema(2)));
    for bad in [
        r#"{"schema": 1, "every": "00:00:30", "until": "14:00", "headcode_step": 2}"#,
        r#"{"schema": 1, "every": "13:00:00", "until": "14:00", "headcode_step": 2}"#,
        r#"{"schema": 1, "every": "00:10:00", "until": "24:00:00", "headcode_step": 2}"#,
        r#"{"schema": 1, "every": "00:10:00", "until": "14:00", "headcode_step": 0}"#,
        r#"{"schema": 1, "every": "ten", "until": "14:00", "headcode_step": 2}"#,
    ] {
        assert_eq!(parse(bad), Err(RepeatError::Bad), "{bad}");
    }
}

#[test]
fn a_shuttle_carries_on_until_the_last_repeat_then_stables() {
    let mut w = shuttle();
    apply(&mut w, &spec("00:20:00", "07:00:00", 2)).unwrap();
    // The converted services first, then the repeats by round, then root order.
    assert_eq!(heads(&w), ["A01", "B01", "A03", "B03", "A05", "B05", "A07"]);
    let a05 = w.services.iter().find(|s| s.headcode == "A05").unwrap();
    assert_eq!((a05.calls[0].dep.as_deref(), a05.calls[1].arr.as_deref()), (Some("06:40:00"), Some("06:45:00")));
    assert_eq!(a05.calls[0].place, "X");
    assert_eq!(forms(&w, "B03").as_deref(), Some("A05"), "the old last service now works on");
    assert_eq!(forms(&w, "A05").as_deref(), Some("B05"));
    assert_eq!(forms(&w, "B05").as_deref(), Some("A07"));
    assert!(matches!(end_of(&w, "A07"), EndFile::Stable), "07:10 is past `until`: it stables as before");
    assert_eq!(forms(&w, "A01").as_deref(), Some("B01"), "converted workings stay");
}

/// Review focus 3: an `until` that adds nothing leaves the timetable as it was.
#[test]
fn an_until_before_the_next_repeat_changes_nothing() {
    let mut w = shuttle();
    let before = serde_json::to_string(&w).unwrap();
    apply(&mut w, &spec("00:20:00", "06:30:00", 2)).unwrap();
    assert_eq!(serde_json::to_string(&w).unwrap(), before);
}

#[test]
fn the_same_input_gives_the_same_output() {
    let run = || {
        let mut w = shuttle();
        apply(&mut w, &spec("00:20:00", "09:00:00", 2)).unwrap();
        serde_json::to_string(&w).unwrap()
    };
    assert_eq!(run(), run());
}

#[test]
fn headcodes_that_do_not_follow_the_step_or_outgrow_it_are_errors() {
    let mut w = shuttle();
    assert_eq!(
        apply(&mut w, &spec("00:20:00", "07:00:00", 4)),
        Err(RepeatError::Step("A03".into(), "A01".into(), "A05".into()))
    );
    let mut w = shuttle();
    for s in &mut w.services {
        s.headcode = s.headcode.replace("01", "X").replace("03", "Y");
    }
    for s in &mut w.services {
        if let EndFile::Form { service } = &mut s.end {
            *service = service.replace("01", "X").replace("03", "Y");
        }
    }
    w.entries[0].service = "AX".into();
    assert_eq!(apply(&mut w, &spec("00:20:00", "07:00:00", 2)), Err(RepeatError::NoNumber("AX".into())));
    let mut w = shuttle();
    assert!(matches!(apply(&mut w, &spec("00:20:00", "23:00:00", 2)), Err(RepeatError::Headcode(_, h, _)) if h == "A101"));
}

#[test]
fn a_repeat_may_not_take_another_services_headcode_run_past_midnight_or_form_twice() {
    let mut w = shuttle();
    let mut other = w.services[0].clone();
    other.headcode = "A05".into();
    other.calls[0].place = "Z".into();
    other.end = EndFile::Stable;
    w.services.push(other);
    assert!(matches!(apply(&mut w, &spec("00:20:00", "07:00:00", 2)), Err(RepeatError::Headcode(_, h, _)) if h == "A05"));
    let mut w = shuttle();
    for s in &mut w.services {
        for c in &mut s.calls {
            for t in [&mut c.arr, &mut c.dep].into_iter().flatten() {
                let h: u32 = t[..2].parse().unwrap();
                *t = format!("{:02}{}", h + 17, &t[2..]); // 23:00 to 23:35
            }
        }
    }
    // A runs take 25 minutes: the 23:40 repeat would arrive at 00:05.
    w.services[0].calls[1].arr = Some("23:25:00".into());
    w.services[2].calls[1].arr = Some("23:45:00".into());
    assert_eq!(apply(&mut w, &spec("00:20:00", "23:50:00", 2)), Err(RepeatError::Midnight("A01".into())));
    let mut w = shuttle();
    w.services[3].end = EndFile::Form { service: "A03".into() };
    assert_eq!(apply(&mut w, &spec("01:00:00", "06:00:00", 2)), Err(RepeatError::Formed("A03".into())));
}

/// Drain (spec P8): every 10 minutes until 14:00, three trains all day.
#[test]
fn drain_runs_until_two_in_the_afternoon() {
    let mut w = ts2_import::convert(&std::fs::read_to_string(DRAIN).unwrap()).unwrap().world;
    let before = w.services.len();
    apply(&mut w, &parse(&std::fs::read_to_string(DRAIN_REPEAT).unwrap()).unwrap()).unwrap();
    assert_eq!((before, w.services.len()), (16, 192));
    let h = heads(&w);
    assert!(h.contains(&"BW96") && h.contains(&"WB96") && !h.contains(&"BW98"), "{h:?}");
    let ends: Vec<&str> = w.services.iter().filter(|s| !matches!(s.end, EndFile::Form { .. })).map(|s| s.headcode.as_str()).collect();
    assert_eq!(ends.len(), 3, "three trains stable at the end of the day: {ends:?}");
    assert_eq!(forms(&w, "BW08").as_deref(), Some("WB09"));
    assert_eq!(forms(&w, "WB07").as_deref(), Some("BW09"));
    assert_eq!(forms(&w, "WB08").as_deref(), Some("BW10"));
    World::from_file(w).expect("the repeated world loads");
}
```

In `crates/ts2-import/tests/cli.rs`: the first doc line becomes ``//! The converter CLI's `--areas`, `--lines` and `--repeat` flags.``, add after `DRAIN_LINES`:

```rust
const DRAIN_REPEAT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../layouts/drain.repeat.json");
```

and append:

```rust
#[test]
fn repeat_flag_repeats_the_timetable() {
    let dir = temp_dir("repeat");
    let out = dir.join("drain.json");
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--areas", DRAIN_AREAS, "--lines", DRAIN_LINES, "--repeat", DRAIN_REPEAT]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stderr(&o).contains("192 services"), "{}", stderr(&o));
    let w: WorldFile = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(w.services.len(), 192);
    let bad = dir.join("bad.repeat.json");
    std::fs::write(&bad, r#"{"schema": 1, "every": "00:10:00", "until": "23:00:00", "headcode_step": 2}"#).unwrap();
    let out2 = dir.join("drain2.json");
    let o = cli(&[DRAIN, "-o", out2.to_str().unwrap(), "--repeat", bad.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("longer than the original"), "{}", stderr(&o));
    assert!(!out2.exists());
    let o = cli(&[DRAIN, "-o", out2.to_str().unwrap(), "--repeat"]);
    assert_eq!(o.status.code(), Some(2), "--repeat needs a value");
    let _ = std::fs::remove_dir_all(&dir);
}
```

In `crates/ts2-import/tests/soak.rs`, before the doc comment of `liverpool_street_runs_three_hours` (`/// Three sim-hours from 05:00:15.`), add:

```rust
/// Drain with its repeat file (polish spec §4): three sim hours from 06:00,
/// the three trains still shuttling on repeated headcodes.
#[test]
fn drain_repeats_through_the_morning() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let mut w = ts2_import::convert(&std::fs::read_to_string(format!("{dir}/tests/data/drain.json")).unwrap()).unwrap().world;
    let spec = ts2_import::repeat::parse(&std::fs::read_to_string(format!("{dir}/../../layouts/drain.repeat.json")).unwrap()).unwrap();
    ts2_import::repeat::apply(&mut w, &spec).unwrap();
    let mut sim = Sim::new(World::from_file(w).unwrap(), 7);
    let r = soak(&mut sim, 3.0 * 3600.0);
    assert_safe("drain repeated", &r);
    assert_eq!(r.still_running.len(), 3, "{r:?}");
    let number = |h: &str| h[2..].parse::<u32>().unwrap();
    assert!(r.still_running.iter().all(|h| number(h) > 8), "repeated headcodes by 09:00: {r:?}");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p ts2-import --test repeat --test cli --test soak`
Expected: compile errors (`could not find repeat in ts2_import`).

- [ ] **Step 3: Implement**

Create `crates/ts2-import/src/repeat.rs`:

```rust
//! Repeating a converted timetable (polish spec §4), from an optional
//! hand-made per-layout file: every service that the timetable already
//! repeats `every` minutes is carried on until `until`, with its headcode's
//! number going up by `headcode_step` each time, and services that ended
//! the day without working on now form the next repeat's, as the earlier
//! ones did. Deterministic: same world and file, byte-identical result.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use signalbox_core::time::{fmt_hms, parse_hms};
use signalbox_core::world::file::{CallFile, EndFile, ServiceFile, WorldFile};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepeatSpec {
    pub schema: u32,
    /// "HH:MM:SS": how far apart repeats are.
    pub every: String,
    /// "HH:MM:SS": no repeat starts later than this.
    pub until: String,
    /// Added to the headcode's trailing number at each repeat.
    pub headcode_step: u32,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RepeatError {
    #[error("not a repeat file: {0}")]
    Parse(String),
    #[error("unsupported repeat file schema {0}")]
    Schema(u32),
    #[error("`every` must be 00:01:00 to 12:00:00, `until` before 24:00:00 and `headcode_step` at least 1")]
    Bad,
    #[error("headcode `{0}` has no trailing number to renumber")]
    NoNumber(String),
    #[error("`{0}` repeats `{1}` but its headcode should then be `{2}`")]
    Step(String, String, String),
    #[error("repeating `{0}` would need headcode `{1}`, which {2}")]
    Headcode(String, String, &'static str),
    #[error("repeating `{0}` would run past 23:59:59")]
    Midnight(String),
    #[error("`{0}` would be formed by more than one service, or both formed and entered")]
    Formed(String),
}

pub fn parse(json: &str) -> Result<RepeatSpec, RepeatError> {
    let s: RepeatSpec = serde_json::from_str(json).map_err(|e| RepeatError::Parse(e.to_string()))?;
    if s.schema != 1 {
        return Err(RepeatError::Schema(s.schema));
    }
    let (every, until) = (parse_hms(&s.every), parse_hms(&s.until));
    match (every, until) {
        (Some(e), Some(u)) if (60..=12 * 3600).contains(&e) && u < 24 * 3600 && s.headcode_step >= 1 => Ok(s),
        _ => Err(RepeatError::Bad),
    }
}

/// A call's times in seconds (`None` where it has none); unreadable times
/// never reach here (the converter only writes good ones).
fn times(s: &ServiceFile) -> Vec<(Option<u32>, Option<u32>)> {
    let t = |x: &Option<String>| x.as_deref().and_then(parse_hms);
    s.calls.iter().map(|c| (t(&c.arr), t(&c.dep))).collect()
}

/// What must match for one service to repeat another: type and calls.
fn shape(s: &ServiceFile) -> (String, Vec<(String, Option<String>, bool)>) {
    (s.train_type.clone(), s.calls.iter().map(|c| (c.place.clone(), c.platform.clone(), c.stop)).collect())
}

fn shifted(ts: &[(Option<u32>, Option<u32>)], by: i64) -> Option<Vec<(Option<u32>, Option<u32>)>> {
    let f = |t: Option<u32>| -> Option<Option<u32>> {
        match t {
            None => Some(None),
            Some(v) => u32::try_from(i64::from(v) + by).ok().map(Some),
        }
    };
    ts.iter().map(|&(a, d)| Some((f(a)?, f(d)?))).collect()
}

fn first_time(s: &ServiceFile) -> Option<u32> {
    times(s).into_iter().find_map(|(a, d)| a.or(d))
}

/// `head` with `n` added to its trailing number, at the same width.
fn renumber(head: &str, n: u64) -> Result<String, RepeatError> {
    let digits = head.bytes().rev().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return Err(RepeatError::NoNumber(head.to_string()));
    }
    let (stem, num) = head.split_at(head.len() - digits);
    let value: u64 = num.parse().map_err(|_| RepeatError::NoNumber(head.to_string()))?;
    let out = format!("{stem}{:0digits$}", value + n);
    if out.len() != head.len() {
        return Err(RepeatError::Headcode(head.to_string(), out, "is longer than the original"));
    }
    Ok(out)
}

/// Repeat the world's timetable as `spec` says (spec §4.2).
pub fn apply(w: &mut WorldFile, spec: &RepeatSpec) -> Result<(), RepeatError> {
    let every = parse_hms(&spec.every).ok_or(RepeatError::Bad)?;
    let until = parse_hms(&spec.until).ok_or(RepeatError::Bad)?;
    let step = u64::from(spec.headcode_step);
    let base = std::mem::take(&mut w.services);
    let by_name: BTreeMap<&str, usize> = base.iter().enumerate().map(|(i, s)| (s.headcode.as_str(), i)).collect();
    let index: BTreeMap<_, usize> = base.iter().enumerate().map(|(i, s)| ((shape(s), times(s)), i)).collect();
    // The service each one repeats (`every` earlier), if any.
    let pred: Vec<Option<usize>> = base
        .iter()
        .map(|s| shifted(&times(s), -i64::from(every)).and_then(|t| index.get(&(shape(s), t)).copied()))
        .collect();
    // Root and repeat number of each service; every root's members by number.
    let mut root = vec![(0, 0u64); base.len()];
    let mut members: BTreeMap<usize, BTreeMap<u64, String>> = BTreeMap::new();
    for i in 0..base.len() {
        let (mut r, mut j) = (i, 0u64);
        while let Some(p) = pred[r] {
            r = p;
            j += 1;
        }
        let want = renumber(&base[r].headcode, j * step)?;
        if want != base[i].headcode {
            return Err(RepeatError::Step(base[i].headcode.clone(), base[r].headcode.clone(), want));
        }
        root[i] = (r, j);
        members.entry(r).or_default().insert(j, base[i].headcode.clone());
    }
    let roots: Vec<usize> = (0..base.len()).filter(|&i| pred[i].is_none()).collect();
    // The copies, by repeat number and then root order.
    let mut taken: BTreeSet<String> = base.iter().map(|s| s.headcode.clone()).collect();
    let mut copies: Vec<(usize, u64, ServiceFile)> = Vec::new();
    for &r in &roots {
        let last = *members[&r].keys().next_back().expect("a root is its own member");
        let Some(start) = first_time(&base[r]) else { continue };
        let mut j = last + 1;
        while u64::from(start) + j * u64::from(every) <= u64::from(until) {
            let head = renumber(&base[r].headcode, j * step)?;
            if !taken.insert(head.clone()) {
                return Err(RepeatError::Headcode(base[r].headcode.clone(), head, "another service has"));
            }
            let by = i64::try_from(j * u64::from(every)).map_err(|_| RepeatError::Midnight(base[r].headcode.clone()))?;
            let ts = shifted(&times(&base[r]), by).ok_or_else(|| RepeatError::Midnight(base[r].headcode.clone()))?;
            if ts.iter().any(|&(a, d)| a.max(d).is_some_and(|t| t >= 24 * 3600)) {
                return Err(RepeatError::Midnight(base[r].headcode.clone()));
            }
            let fmt = |t: Option<u32>| t.map(|v| fmt_hms(f64::from(v)));
            let calls = base[r].calls.iter().zip(&ts).map(|(c, &(a, d))| CallFile { arr: fmt(a), dep: fmt(d), ..c.clone() }).collect();
            copies.push((r, j, ServiceFile { headcode: head.clone(), calls, ..base[r].clone() }));
            members.get_mut(&r).expect("root").insert(j, head);
            j += 1;
        }
    }
    copies.sort_by_key(|(r, j, _)| (*j, roots.iter().position(|x| x == r)));
    // The steady end of member j of root r (spec §4.2 rule 3).
    let steady = |r: usize, j: u64| -> EndFile {
        let base_members: Vec<(u64, usize)> =
            members[&r].iter().filter_map(|(&i, h)| by_name.get(h.as_str()).map(|&b| (i, b))).collect();
        if let Some(&(i, b)) = base_members.iter().rev().find(|&&(i, b)| i <= j && matches!(base[b].end, EndFile::Form { .. })) {
            if let EndFile::Form { service } = &base[b].end {
                let (q, l) = root[by_name[service.as_str()]];
                if let Some(t) = members[&q].get(&(l + j - i)) {
                    return EndFile::Form { service: t.clone() };
                }
            }
        }
        // Past the last repeat: end as the root's last converted member does.
        match base_members.last().map(|&(_, b)| &base[b].end) {
            Some(EndFile::Form { .. }) | None => EndFile::Stable,
            Some(e) => e.clone(),
        }
    };
    let mut out: Vec<ServiceFile> = Vec::with_capacity(base.len() + copies.len());
    for (i, s) in base.iter().enumerate() {
        let end = if matches!(s.end, EndFile::Form { .. }) { s.end.clone() } else { steady(root[i].0, root[i].1) };
        out.push(ServiceFile { end, ..s.clone() });
    }
    for (r, j, s) in copies {
        out.push(ServiceFile { end: steady(r, j), ..s });
    }
    let mut formed: BTreeSet<&str> = w.entries.iter().map(|e| e.service.as_str()).collect();
    for s in &out {
        if let EndFile::Form { service } = &s.end {
            if !formed.insert(service.as_str()) {
                return Err(RepeatError::Formed(service.clone()));
            }
        }
    }
    w.services = out;
    Ok(())
}
```

Note: copies are built with `..base[r].clone()` / `..c.clone()`, so fields the tutorial adds to `ServiceFile` or `CallFile` are carried over unchanged.

In `crates/ts2-import/src/lib.rs`, after `pub mod lines;` add `pub mod repeat;`.

Create `layouts/drain.repeat.json`:

```json
{
  "schema": 1,
  "every": "00:10:00",
  "until": "14:00:00",
  "headcode_step": 2
}
```

`crates/ts2-import/src/main.rs`:
- `use ts2_import::{areas, convert, lines, repeat, report};`
- `USAGE` ends `[--lines <lines.json>] [--repeat <repeat.json>]";`
- after the `let (mut input, …) = …;` line: `let mut repeat_path = None;`
- a match arm before `"--strict"`:

```rust
            "--repeat" => {
                i += 1;
                match args.get(i) {
                    Some(r) => repeat_path = Some(r.clone()),
                    None => return usage(),
                }
            }
```

- before `let text = match std::fs::read_to_string(&input) {`:

```rust
    let repeat_spec = match &repeat_path {
        Some(p) => match std::fs::read_to_string(p).map_err(|e| e.to_string()).and_then(|t| repeat::parse(&t).map_err(|e| e.to_string())) {
            Ok(r) => Some((p.clone(), r)),
            Err(e) => {
                eprintln!("{p}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
```

- before `let json = serde_json::to_string_pretty(&c.world)…`:

```rust
            if let Some((p, r)) = &repeat_spec {
                if let Err(e) = repeat::apply(&mut c.world, r) {
                    eprintln!("{p}: {e}");
                    return ExitCode::FAILURE;
                }
            }
```

`deploy/Dockerfile`, the conversion loop in the `build` stage becomes:

```dockerfile
 && for n in liverpool-st drain gretz-armainvilliers; do \
      rep=""; if [ -f "layouts/$n.repeat.json" ]; then rep="--repeat layouts/$n.repeat.json"; fi; \
      target/release/ts2-import "crates/ts2-import/tests/data/$n.json" -o "/out/layouts/$n.json" \
        --areas "layouts/$n.areas.json" --lines "layouts/$n.lines.json" $rep || exit 1; \
    done
```

and the header comment's "(with their areas, box prefixes and line names)" becomes "(with their areas, box prefixes, line names and, for Drain, a repeated timetable)".

- [ ] **Step 4: Run the tests to see them pass**

Run: `scripts/cargo test -p ts2-import`
Expected: PASS (repeat 7, cli 6, soak 3 + 1 ignored, convert snapshots unchanged — `convert` itself is untouched).

- [ ] **Step 5: Document**

`deploy/README.md`, the `Dockerfile` row: "… line names from `layouts/` and Drain's repeated timetable (`layouts/drain.repeat.json`) (no dev login)". `CLAUDE.md`:
- Commands block, after the Liverpool Street `ts2-import` line:
  `scripts/cargo run -p ts2-import -- crates/ts2-import/tests/data/drain.json -o /w/target/drain.json --areas /w/layouts/drain.areas.json --lines /w/layouts/drain.lines.json --repeat /w/layouts/drain.repeat.json`
- "Multiplayer", after the lines-file sentence: "An optional `layouts/<name>.repeat.json` (`ts2-import --repeat`, `ts2_import::repeat`, polish spec §4) carries on every service the timetable already repeats `every` minutes until `until`, renumbering headcodes by `headcode_step` at the same width, and turns the day's last workings into forms of the next repeat; only Drain has one (to 14:00). Old saves keep the timetable they were created with."

- [ ] **Step 6: Commit**

```bash
git add crates/ts2-import layouts/drain.repeat.json deploy/Dockerfile deploy/README.md CLAUDE.md
git commit -m "feat(ts2-import): repeat a layout's timetable from a per-layout file; Drain runs until 14:00"
```

---

### Task 5: The simplifier opens at "now"

**Files:**
- Modify: `crates/client-core/src/simplifier.rs`, `crates/client-ui/src/screens.rs`
- Test: `crates/client-core/tests/simplifier.rs`, `crates/client-ui/tests/screens.rs`

**Interfaces:**
- Consumes: `layouts/drain.repeat.json` and `ts2_import::repeat` (Task 4) in the screen test.
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
/// Polish spec §7: with Drain repeated, the simplifier opens at 07:00's
/// trains, not at 06:00's.
#[test]
fn the_simplifier_opens_at_now() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let read = |p: String| std::fs::read_to_string(p).unwrap();
    let mut w = ts2_import::convert(&read(format!("{dir}/../ts2-import/tests/data/drain.json"))).unwrap().world;
    ts2_import::repeat::apply(&mut w, &ts2_import::repeat::parse(&read(format!("{dir}/../../layouts/drain.repeat.json"))).unwrap()).unwrap();
    let mut r = Rig::in_game(signalbox_core::world::World::from_file(w).unwrap(), None);
    r.game.handle("ann", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
    for _ in 0..450 {
        r.game.advance(1.0); // 06:00 to 07:00 at 8x
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
    assert!(!side.iter().any(|t| t == "BW01"), "06:00 is long gone: {side:?}");
    assert!(side.iter().any(|t| t == "BW13"), "07:00's trains: {side:?}");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `scripts/cargo test -p signalbox-client-core --test simplifier` → compile error (`now_line` not found).
Run: `scripts/cargo test -p signalbox-client-ui --test screens the_simplifier_opens_at_now` → FAIL: `06:00 is long gone: [… "BW01" …]`.

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
- `UiApp` gains, after `placement`:

```rust
    /// The simplifier line to scroll to once, set when its lines are built.
    simplifier_scroll: Option<usize>,
```

  and `simplifier_scroll: None,` in `UiApp::new`.
- in `side`, the SIMPLIFIER tab:

```rust
            if ui.selectable_value(&mut self.side_tab, SideTab::Simplifier, RichText::new("SIMPLIFIER").strong()).clicked() {
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
- Consumes: `scripts/wasm-build`, the dev-auth front, Task 4's `layouts/drain.repeat.json` (the check converts Drain with it), the client's console line `signalbox: drawing with <backend>` (already in `crates/client-web/src/lib.rs`).
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
  --areas /w/layouts/drain.areas.json --lines /w/layouts/drain.lines.json --repeat /w/layouts/drain.repeat.json 2>/dev/null
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

---

## Controller section (after the branch's final review; not for subagents)

The owner has agreed to redeploy as the realism pass did.

1. **CI cache:** nothing to reseed (no new crates).
2. **Deploy:** as `deploy/README.md` "Build and run" from the merged commit, then `deploy/smoke.sh https://ra.tail3e0c1e.ts.net:50160 303`. The image now carries Drain's repeated timetable and the front passes `--current-layout` on resume. Roll back by retagging the previous `<rev>`.
3. **Old saves:** join an existing pre-realism Drain save in the lobby; its signals must read `WA…`/`WB…` and `docker logs signalbox` show `display data from layout drain`. Its timetable still ends at 06:43 (spec P9).
4. **Owner's look (morning):** the `legibility` table and the browser-check screenshots in the branch report; then in Chrome/Edge and Firefox on the tailnet: Liverpool Street box A at Fit (numbers clear of the next platform road), the spectator view of Gretz (no pile-ups; ○A appears one zoom step in), a new Drain game past 06:43 (trains keep running; the simplifier opens at now).
