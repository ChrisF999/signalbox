# signalbox multiplayer C1 — game library, protocol, areas and saves — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build everything a multiplayer signalbox game needs *in process*: area files for converted layouts, the wire protocol, a pure `Game` (claims, area checks, the robot for unclaimed areas, clock votes, per-player views and deltas, notices), SQLite saves with resume, and a bot client — proven by a Liverpool Street soak with two bots, the robot and a spectator.

**Architecture:** `ts2-import --areas` floods each area over the section graph from hand-picked seeds, stopping at boundary signals, and rewrites the converted world's areas. A new `protocol` crate holds every message, the `Layout`/`View` types and the view diff. A new `game` library wraps a core `Sim`: it maps each command to its subject's area, filters the robot to unclaimed areas, builds each player's view (own area + fringe, or everything for a spectator) and diffs it against what that player was last sent. Saves are one SQLite file per game (world, meta, newest 3 snapshots, full command log). A `bot` library applies deltas and asks for a resync on a gap; its tests drive a real `Game`.

**Tech Stack:** Rust 1.98 (edition 2024) via `scripts/cargo` (Docker); serde, serde_json (`float_roundtrip`), thiserror; new: `rusqlite` 0.40 with `bundled` SQLite. No tokio in C1.

**Spec:** `docs/superpowers/specs/2026-09-30-server-and-protocol-design.md` (sub-project C). This plan is C1 of §13, refined by the amendments below. Earlier plans for conventions: `docs/superpowers/plans/2026-09-29-ts2-import.md`.

### Spec amendments (controller decisions for C1, 2026-09-30, plus this plan's own)

1. **Scope.** C1 = `protocol`; `game` as a **library only** (Game, views, areas, votes, SQLite saves); `ts2-import --areas` + area files for Liverpool Street, Drain and Gretz; `Sim::state_hash` in core; an in-process `bot` library and the acceptance soak. The `game` *binary*, `ipc`, the front, auth, Docker and the CI cache reseed move to C2. The bounded outbound queue (§4.3) lives in C2's relay; C1 provides `Game::resync(player)` for it.
2. **Names on the wire.** Clients send `PlayerCommand` (mirrors core `Command`, with string names); the game resolves names to ids; an unknown name answers `notice {kind: rejected, reason: unknown_id}`. `Layout` and `View` are keyed by names. Names are opaque strings (they may hold `,`, `#`, `&`, spaces).
3. **`protocol` depends on `signalbox-core`** for `Aspect`, `PointsPos`, `Dir` and `Rejection` rather than mirroring them: their serde forms are already snake_case wire forms, one definition cannot drift, and core is pure Rust with no I/O (it will compile for the browser client in D).
4. **Areas file (§5).** Every signal in a converted world stands at a segment end; its node is the segment's `from` node if `offset_m <= length_m / 2`, else `to`. The flood never crosses a node holding a boundary signal; sections are adjacent when their segments share a node. Seeds are section, signal or berth names (tried in that order). A signal's area = its own segment's section's area (in converted worlds that is the approach side, so spec §5's rule holds). Applied after `convert()` on the `WorldFile`, then re-validated with `World::from_file`. Hard errors list names sorted. No areas file = one area, as before. Drain and Gretz files ship too (owner request).
5. **Consequence of 4, correcting spec §5's last bullet:** a route from a *boundary* signal runs into the next area (the signal belongs to the box in rear). Area checks use the entrance signal's area, as §3.4 says.
6. **Fringe (§4.5), refining the brief.** For area A: walk the *track* away from A the way a train could run — from each A segment through its end nodes into non-A segments, through points only toe↔leg (never leg↔leg), not re-entering A — and stop after a segment whose far node holds any signal (a signal stands at the nearer end of its segment, as in amendment 4). The fringe is every section walked. The brief's undirected section flood turns back through points (normal leg → reverse leg), which no train can do; on Liverpool Street it gave Bethnal Green 193 of the 239 foreign sections, while the directed walk gives 16 / 37 / 33 fringe sections for Liverpool Street / Bethnal Green / Hackney & Bow, and every boundary signal is seen by exactly the two boxes it separates (checked with a prototype of both rules; pinned in Task 4). Visible = A ∪ fringe; spectators see everything. Signals are visible when their segment's section is; points when their points section is; berths when their anchor section is (signal's section, or a boundary berth's boundary segment's section); routes when their entrance is.
7. **Game API.** `Game::new(world, meta)` — the seed travels in `GameMeta` (it is saved there), so the brief's separate `seed` argument is folded in. `Out = (String, ServerMsg)`. New games start **running at 1x**; resumed games start **paused at 1x**. Speed x = x ticks per 0.1 s of real time. The robot runs once per `robot::ROBOT_EVERY_TICKS` (10) ticks — the cadence `robot::soak` proved safe — rather than every tick; the constant becomes `pub`. Robot commands are logged as player `"robot"`, which is therefore a reserved player name.
8. **Duplicate connections** (`replaced`, §3.6) are the front's job (C2); `Game::connect` on a connected player just resyncs it.
9. **Commands are saved as core `Command` JSON** (ids). The world is copied into the save, so ids stay valid.
10. **Resume refinement (§7.3).** Commands logged at the snapshot's tick that were already queued when the snapshot was taken are inside the snapshot; replay skips the first `snapshot.queue.len()` of them. If the log's last tick holds robot commands, the robot is not run again at that tick.
11. **Small rules the spec leaves open:** claiming while holding another area moves you (the old area goes to the robot); interposed headcodes are 1–10 ASCII letters/digits (`error {code: bad_headcode}` otherwise); `late` notices only when `late_s >= 60`, and carry `place` as well as `platform`; `advance` treats non-finite or negative time as 0 and runs at most 800 ticks per call. Area notices (`spad`, `collision`, `late`, `wrong_platform`, `handover`) go to the connected holder of the area concerned, never to spectators; `rejected` goes to the sender. The saved game's `created`, `last_played` and `saved_at` are Unix seconds as text; `meta.start` is the world's start time. Choosing a start time in the lobby (`create_game {start?}`) means editing the world's `options.start_time` before `Game::create` — that is C2's job.
12. **Soak cadence.** The in-process soaks advance 0.125 s of real time per call at 8x (10 ticks = exactly one `ROBOT_EVERY_TICKS` period) instead of the brief's 0.1 s, and flush every second call (4 Hz). Bots then decide on exactly the state the game's robot sees at the same tick, so bots + robot together issue precisely what one robot call would (a single `robot::commands` result filtered by area); at 0.1 s the robot would set half of a train's route chain between bot decisions and the train could stall for good. Checked with a prototype run: first handovers into Hackney & Bow at tick 7742 and into Liverpool Street at tick 8934.
13. **Save API shape.** `SaveDb::create(path, &meta, &world_json)` as the brief says; `SaveDb::open(path)` then `db.load()` in place of a free `load(path)`, because the resumed game keeps writing through the same handle. `Game::create(path, world_json, meta)` makes a saved game (writing a tick-0 snapshot) and `Game::resume(path)` restores one.

## Global Constraints

- License: GPL-2.0-or-later on every new crate (`license.workspace = true`).
- Every cargo command runs through `scripts/cargo` from the repo root; paths inside the container are under `/w`.
- CI (`scripts/ci/test.sh`) builds with `-D warnings`, `--locked`, `--offline`: no unused imports, variables or dead code; commit `Cargo.lock` with every dependency change.
- Determinism: `BTreeMap`/`BTreeSet`/`Vec` only in `game`, `protocol`, `ts2-import`; players are always visited in name order; no wall clock in `Game` (only `game::save` reads the system time, for `created`/`last_played`/`saved_at`, which the sim never reads).
- New dependencies: only `rusqlite = { version = "0.40", features = ["bundled"] }` (workspace). No tokio.
- Crate names: `signalbox-protocol` (lib `protocol`), `signalbox-game` (lib `game`), `signalbox-bot` (lib `bot`).
- Wire JSON: messages tagged `"type"`, notices `"kind"`, commands `"cmd"`, proposals `"kind"`, exits `{"kind": "signal"|"node", "name": ...}`; all snake_case.
- Numbers: grace 120 s real; vote lapse 30 s real; speeds {1, 2, 4, 8}; snapshot every 60 s real while running; keep the newest 3 snapshots; the caller flushes at most 5 times per real second.
- No client input may panic the game; every error is a notice.

## Review Focus

1. **Names that look like syntax** (`39,1V1`, `512#113`, `Hackney & Bow`) — they must work as opaque strings in area files, commands and error listings. Pinned in Task 2 (`names_are_opaque_strings`) and Task 6 (`names_with_commas_and_hashes_resolve`).
2. **Resuming right after a command or a robot tick** — a snapshot taken with commands queued, or a log ending in robot commands, must not apply anything twice. Pinned in Task 9 (`resume_skips_commands_already_queued_in_the_snapshot`, `resuming_after_robot_commands_does_not_run_the_robot_twice`, `save_now_right_after_a_command_resumes_exactly`).
3. **A client that misses a delta** — it must ask for exactly one resync and recover from the next full view. Pinned in Task 10 (`a_gap_asks_for_one_resync_and_the_next_view_recovers`) and Task 8 (`resync_restarts_the_delta_base`).
4. **Real time that is NaN, negative or huge** (a stalled caller) — no panic, no runaway stepping. Pinned in Task 8 (`advance_survives_bad_real_time`).
5. **A holder who disconnects during a vote** — they keep blocking it through the grace period, and their expiry completes it. Pinned in Task 8 (`grace_expiry_completes_a_vote`).

---

## File Structure

```
Cargo.toml                                   members + workspace dep rusqlite (Task 9)
layouts/liverpool-st.areas.json              (Task 2)
layouts/drain.areas.json                     (Task 2)
layouts/gretz-armainvilliers.areas.json      (Task 2)
crates/core/src/sim.rs                       (Task 1) Sim::state_hash
crates/core/src/robot.rs                     (Task 1) pub ROBOT_EVERY_TICKS
crates/core/tests/snapshot.rs                (Task 1)
crates/sim-cli/src/main.rs                   (Task 1) uses Sim::state_hash
crates/ts2-import/src/areas.rs               (Task 2) AreasFile, parse, apply, AreasError
crates/ts2-import/src/lib.rs                 (Task 2) pub mod areas
crates/ts2-import/src/main.rs                (Task 2) --areas
crates/ts2-import/tests/areas.rs             (Task 2)
crates/ts2-import/tests/cli.rs               (Task 2)
crates/protocol/Cargo.toml                   (Task 3)
crates/protocol/src/lib.rs                   re-exports, error codes
crates/protocol/src/msg.rs                   ClientMsg, PlayerCommand, ExitName, Proposal, ServerMsg, Notice
crates/protocol/src/view.rs                  Layout (+ parts), View (+ parts), Delta
crates/protocol/src/diff.rs                  diff, View::apply, SeqGap
crates/protocol/tests/{golden,diff}.rs
crates/game/Cargo.toml                       (Task 4; deps grow in Task 9)
crates/game/src/lib.rs                       module list (grows per task)
crates/game/src/areas.rs                     (Task 4) AreaMap, Visibility, fringe
crates/game/src/layout.rs                    (Task 4) build_layout, exit_name
crates/game/src/view.rs                      (Task 5) Shared, build_view
crates/game/src/names.rs                     (Task 6) resolve, to_player_command
crates/game/src/notices.rs                   (Task 6) area_notices (incl. handover)
crates/game/src/clock.rs                     (Task 7) GameClock, votes
crates/game/src/game.rs                      (Task 8) Game, GameMeta, GameStats, Out; save hooks (Task 9)
crates/game/src/save.rs                      (Task 9) SaveDb, Saved, Logged, resume_sim
crates/game/tests/common/mod.rs              helpers (grows per task)
crates/game/tests/fixtures/twobox.json       (Task 4) two-area test world
crates/game/tests/{areas,layout,view,names,notices,clock,game,save}.rs
crates/bot/Cargo.toml, src/lib.rs            (Task 10) Bot
crates/bot/tests/{bot,soak}.rs               (Task 10)
CLAUDE.md                                    (Task 10) new crates, commands, architecture notes
```

---

### Task 1: Core — `Sim::state_hash` and a public robot cadence

**Files:**
- Modify: `crates/core/src/sim.rs` (add `state_hash` after `snapshot`)
- Modify: `crates/core/src/robot.rs:24` (`const ROBOT_EVERY_TICKS` → `pub const`)
- Modify: `crates/sim-cli/src/main.rs` (use `sim.state_hash()`, delete local `state_hash`/`fnv1a`)
- Test: `crates/core/tests/snapshot.rs`

**Interfaces:**
- Produces: `Sim::state_hash(&self) -> u64` (FNV-1a over `serde_json::to_string(&self.snapshot())`, the exact algorithm sim-cli used); `signalbox_core::robot::ROBOT_EVERY_TICKS: u64 = 10`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/core/tests/snapshot.rs`:
```rust
#[test]
fn state_hash_is_fnv1a_of_the_snapshot_json() {
    let mut sim = busy_terminus(3);
    sim.run_for(60.0);
    let json = serde_json::to_string(&sim.snapshot()).unwrap();
    let want = json.bytes().fold(0xcbf29ce484222325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
    assert_eq!(sim.state_hash(), want);
}

#[test]
fn state_hash_follows_the_state() {
    let mut a = busy_terminus(3);
    let mut b = busy_terminus(3);
    a.run_for(100.0);
    b.run_for(100.0);
    assert_eq!(a.state_hash(), b.state_hash());
    b.step();
    assert_ne!(a.state_hash(), b.state_hash());
}

#[test]
fn robot_cadence_is_public() {
    assert_eq!(signalbox_core::robot::ROBOT_EVERY_TICKS, 10);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-core --test snapshot`
Expected: compile error — no method `state_hash` on `Sim`; `ROBOT_EVERY_TICKS` is private.

- [ ] **Step 3: Implement**

In `crates/core/src/sim.rs`, directly after `pub fn snapshot(&self) -> SimState { ... }`:
```rust
    /// FNV-1a hash of the full serialised state (`snapshot()` as JSON), for
    /// comparing runs: equal hashes mean the same state.
    pub fn state_hash(&self) -> u64 {
        let state = serde_json::to_string(&self.snapshot()).expect("state serialises");
        state.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3))
    }
```

In `crates/core/src/robot.rs` change
```rust
/// The robot looks at the railway once per this many ticks.
const ROBOT_EVERY_TICKS: u64 = 10;
```
to
```rust
/// The robot looks at the railway once per this many ticks.
pub const ROBOT_EVERY_TICKS: u64 = 10;
```

In `crates/sim-cli/src/main.rs`: replace both `state_hash(&sim)` calls with `sim.state_hash()`, and delete the two functions at the end of the file (`fn state_hash(sim: &Sim) -> u64 { ... }` and `fn fnv1a(bytes: &[u8]) -> u64 { ... }` with their doc comments).

- [ ] **Step 4: Run the tests**

Run: `scripts/cargo test -p signalbox-core --test snapshot && scripts/cargo test -p sim-cli`
Expected: PASS (sim-cli's `run_and_replay_print_the_same_state_hash` still passes — same algorithm).

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/sim.rs crates/core/src/robot.rs crates/core/tests/snapshot.rs crates/sim-cli/src/main.rs
git commit -m "feat(core): Sim::state_hash and a public robot cadence

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Areas files in `ts2-import` (flood fill, CLI, three shipped layouts)

**Files:**
- Create: `crates/ts2-import/src/areas.rs`
- Modify: `crates/ts2-import/src/lib.rs` (add `pub mod areas;`)
- Modify: `crates/ts2-import/src/main.rs` (whole file below)
- Create: `layouts/liverpool-st.areas.json`, `layouts/drain.areas.json`, `layouts/gretz-armainvilliers.areas.json`
- Test: `crates/ts2-import/tests/areas.rs`, `crates/ts2-import/tests/cli.rs`

**Interfaces:**
- Consumes: `signalbox_core::world::file::{WorldFile, AreaFile, SegmentFile, SignalFile, BerthFile}`, `World::from_file`, `LoadError`.
- Produces (module `ts2_import::areas`):
  - `pub const AREAS_SCHEMA: u32 = 1;`
  - `pub struct AreasFile { pub schema: u32, pub boundaries: Vec<String>, pub areas: Vec<AreaSpec> }` and `pub struct AreaSpec { pub name: String, pub seeds: Vec<String> }` (both `Debug, Clone, PartialEq, Deserialize`, `deny_unknown_fields`)
  - `pub struct AreaCount { pub name: String, pub sections: usize, pub signals: usize }` (`Debug, Clone, PartialEq, Eq`)
  - `pub enum AreasError { Parse(String), Schema(u32), NoAreas, DuplicateAreas(Vec<String>), NoSeeds(Vec<String>), UnknownNames(Vec<String>), NotSignals(Vec<String>), NotAtSegmentEnd(Vec<String>), DoublyReached(Vec<String>), Unreached(Vec<String>), Invalid(LoadError) }` (`Debug, Clone, PartialEq, thiserror::Error`; every list sorted; names shown in backticks)
  - `pub fn parse(json: &str) -> Result<AreasFile, AreasError>`
  - `pub fn apply(world: &mut WorldFile, spec: &AreasFile) -> Result<Vec<AreaCount>, AreasError>` — all-or-nothing: on error `world` is unchanged. Counts are in file order.
  - Files `layouts/<name>.areas.json` (repo root) for `liverpool-st`, `drain`, `gretz-armainvilliers`.

Checks run in this order, first failure wins: empty area list, duplicate area names, areas without seeds, unknown names (seeds and boundaries together), boundaries that exist but are not signals, boundary signals not at a segment end, doubly reached sections, unreached sections, the rewritten world failing to load.

- [ ] **Step 1: Write the failing tests**

`crates/ts2-import/tests/areas.rs`:
```rust
//! Area files: flood fill from seeds, boundary signals, hard errors, and the shipped layouts.

use signalbox_core::world::World;
use signalbox_core::world::file::WorldFile;
use ts2_import::areas::{self, AreaCount, AreaSpec, AreasError, AreasFile};

const PLAIN_LINE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/fixtures/plain_line.json");

fn plain_line() -> WorldFile {
    serde_json::from_str(&std::fs::read_to_string(PLAIN_LINE).unwrap()).unwrap()
}

fn spec(boundaries: &[&str], areas: &[(&str, Vec<&str>)]) -> AreasFile {
    AreasFile {
        schema: 1,
        boundaries: boundaries.iter().map(|s| s.to_string()).collect(),
        areas: areas
            .iter()
            .map(|(n, seeds)| AreaSpec { name: n.to_string(), seeds: seeds.iter().map(|s| s.to_string()).collect() })
            .collect(),
    }
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn section_area(w: &WorldFile, s: &str) -> String {
    w.sections.iter().find(|x| x.name == s).unwrap_or_else(|| panic!("no section {s}")).area.clone()
}

fn signal_area(w: &WorldFile, s: &str) -> String {
    w.signals.iter().find(|x| x.name == s).unwrap_or_else(|| panic!("no signal {s}")).area.clone()
}

/// plain_line: W -a- J1 -b- J2 -c- E, sections TA TB TC, S1 at the end of a (J1),
/// S2 at the end of b (J2).
#[test]
fn splits_plain_line_at_a_boundary_signal() {
    let mut w = plain_line();
    let counts = areas::apply(&mut w, &spec(&["S2"], &[("West", vec!["TA"]), ("East", vec!["TC"])])).unwrap();
    assert_eq!(
        counts,
        vec![
            AreaCount { name: "West".into(), sections: 2, signals: 2 },
            AreaCount { name: "East".into(), sections: 1, signals: 0 },
        ]
    );
    assert_eq!(w.areas.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["West", "East"]);
    assert_eq!([section_area(&w, "TA"), section_area(&w, "TB"), section_area(&w, "TC")], ["West", "West", "East"]);
    assert_eq!([signal_area(&w, "S1"), signal_area(&w, "S2")], ["West", "West"]);
    World::from_file(w).unwrap();
}

#[test]
fn signal_and_berth_seeds_resolve_to_sections() {
    for west in ["S1", "BW", "B2", "TB"] {
        let mut w = plain_line();
        areas::apply(&mut w, &spec(&["S2"], &[("West", vec![west]), ("East", vec!["TC"])]))
            .unwrap_or_else(|e| panic!("seed {west}: {e}"));
        assert_eq!(section_area(&w, "TA"), "West", "seed {west}");
    }
}

#[test]
fn unreached_sections_are_listed() {
    let mut w = plain_line();
    let e = areas::apply(&mut w, &spec(&["S1", "S2"], &[("West", vec!["TA"]), ("East", vec!["TC"])])).unwrap_err();
    assert_eq!(e, AreasError::Unreached(names(&["TB"])));
}

#[test]
fn doubly_reached_sections_are_listed() {
    let mut w = plain_line();
    let e = areas::apply(&mut w, &spec(&[], &[("West", vec!["TA"]), ("East", vec!["TC"])])).unwrap_err();
    assert_eq!(e, AreasError::DoublyReached(names(&["TA", "TB", "TC"])));
}

#[test]
fn unknown_names_are_listed_sorted() {
    let mut w = plain_line();
    let e = areas::apply(&mut w, &spec(&["ZZ"], &[("West", vec!["TA", "NOPE"]), ("East", vec!["TC"])])).unwrap_err();
    assert_eq!(e, AreasError::UnknownNames(names(&["NOPE", "ZZ"])));
}

#[test]
fn boundaries_must_be_signals() {
    let mut w = plain_line();
    let e = areas::apply(&mut w, &spec(&["TB", "B1"], &[("West", vec!["TA"]), ("East", vec!["TC"])])).unwrap_err();
    assert_eq!(e, AreasError::NotSignals(names(&["B1", "TB"])));
}

#[test]
fn boundary_signals_must_stand_at_a_segment_end() {
    let mut w = plain_line();
    w.signals[1].offset_m = 500.0;
    let e = areas::apply(&mut w, &spec(&["S2"], &[("West", vec!["TA"]), ("East", vec!["TC"])])).unwrap_err();
    assert_eq!(e, AreasError::NotAtSegmentEnd(names(&["S2"])));
}

#[test]
fn the_area_list_is_checked_first() {
    let mut w = plain_line();
    assert_eq!(areas::apply(&mut w, &spec(&["S2"], &[])), Err(AreasError::NoAreas));
    let dup = spec(&["S2"], &[("West", vec!["TA"]), ("West", vec!["TC"])]);
    assert_eq!(areas::apply(&mut w, &dup), Err(AreasError::DuplicateAreas(names(&["West"]))));
    let seedless = spec(&["S2"], &[("West", vec!["TA"]), ("East", vec![])]);
    assert_eq!(areas::apply(&mut w, &seedless), Err(AreasError::NoSeeds(names(&["East"]))));
}

#[test]
fn failed_apply_leaves_the_world_untouched() {
    let mut w = plain_line();
    let before = serde_json::to_string(&w).unwrap();
    assert!(areas::apply(&mut w, &spec(&["S1", "S2"], &[("West", vec!["TA"]), ("East", vec!["TC"])])).is_err());
    assert_eq!(serde_json::to_string(&w).unwrap(), before);
}

#[test]
fn names_are_opaque_strings() {
    let text = std::fs::read_to_string(PLAIN_LINE)
        .unwrap()
        .replace("\"S2\"", "\"39,1V1\"")
        .replace("\"TC\"", "\"T#3 & more\"");
    let mut w: WorldFile = serde_json::from_str(&text).unwrap();
    areas::apply(&mut w, &spec(&["39,1V1"], &[("Hackney & Bow", vec!["TA"]), ("East, far", vec!["T#3 & more"])]))
        .unwrap();
    assert_eq!(section_area(&w, "T#3 & more"), "East, far");
    assert_eq!(signal_area(&w, "39,1V1"), "Hackney & Bow");
    let e = areas::apply(&mut w, &spec(&["39,1V2"], &[("A", vec!["TA"])])).unwrap_err();
    assert_eq!(e, AreasError::UnknownNames(names(&["39,1V2"])));
    assert!(e.to_string().contains("`39,1V2`"), "{e}");
}

#[test]
fn parse_checks_schema_and_fields() {
    assert_eq!(areas::parse(r#"{"schema": 2, "areas": []}"#), Err(AreasError::Schema(2)));
    assert!(matches!(areas::parse(r#"{"schema": 1, "areas": [], "extra": 1}"#), Err(AreasError::Parse(_))));
    assert!(matches!(areas::parse("not json"), Err(AreasError::Parse(_))));
    let ok = areas::parse(r#"{"schema": 1, "boundaries": ["S2"], "areas": [{"name": "West", "seeds": ["TA"]}]}"#).unwrap();
    assert_eq!(ok, spec(&["S2"], &[("West", vec!["TA"])]));
}

fn check_shipped(name: &str, want: &[(&str, usize, usize)]) -> WorldFile {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/tests/data/{name}.json")).unwrap();
    let text = std::fs::read_to_string(format!("{dir}/../../layouts/{name}.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    let counts = areas::apply(&mut w, &areas::parse(&text).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
    let want: Vec<AreaCount> =
        want.iter().map(|&(n, sections, signals)| AreaCount { name: n.into(), sections, signals }).collect();
    assert_eq!(counts, want, "{name}");
    World::from_file(w.clone()).unwrap();
    w
}

#[test]
fn liverpool_street_has_three_boxes() {
    let w = check_shipped(
        "liverpool-st",
        &[("Liverpool Street", 185, 33), ("Bethnal Green", 77, 19), ("Hackney & Bow", 54, 23)],
    );
    for s in ["61", "63", "65"] {
        assert_eq!(signal_area(&w, s), "Liverpool Street", "{s}");
    }
    for s in ["64", "66", "68", "91", "93", "95"] {
        assert_eq!(signal_area(&w, s), "Bethnal Green", "{s}");
    }
    for s in ["90", "92", "94"] {
        assert_eq!(signal_area(&w, s), "Hackney & Bow", "{s}");
    }
}

#[test]
fn drain_has_two_boxes() {
    let w = check_shipped("drain", &[("Bank", 11, 4), ("Waterloo", 24, 11)]);
    for s in ["72", "73", "82", "83"] {
        assert_eq!(signal_area(&w, s), "Bank", "{s}");
    }
}

#[test]
fn gretz_has_three_boxes() {
    check_shipped(
        "gretz-armainvilliers",
        &[("Gretz", 123, 47), ("Tournan & Marles", 68, 26), ("Mortcerf & Coulommiers", 36, 22)],
    );
}
```

`crates/ts2-import/tests/cli.rs`:
```rust
//! The converter CLI's `--areas` flag.

use std::path::PathBuf;
use std::process::{Command, Output};

use signalbox_core::world::file::WorldFile;

const DRAIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/drain.json");
const DRAIN_AREAS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../layouts/drain.areas.json");

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ts2-import")).args(args).output().expect("ts2-import runs")
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("signalbox-ts2-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn areas_flag_splits_the_world() {
    let dir = temp_dir("ok");
    let out = dir.join("drain.json");
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--areas", DRAIN_AREAS]);
    assert!(o.status.success(), "{}", stderr(&o));
    let w: WorldFile = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(w.areas.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Bank", "Waterloo"]);
    assert!(stderr(&o).contains("area Bank: 11 sections, 4 signals"), "{}", stderr(&o));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_areas_file_fails_with_the_names_and_writes_nothing() {
    let dir = temp_dir("bad");
    let bad = dir.join("bad.areas.json");
    std::fs::write(&bad, r#"{"schema": 1, "boundaries": ["nope"], "areas": [{"name": "A", "seeds": ["72"]}]}"#).unwrap();
    let out = dir.join("drain.json");
    let o = cli(&[DRAIN, "-o", out.to_str().unwrap(), "--areas", bad.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1), "{}", stderr(&o));
    assert!(stderr(&o).contains("`nope`"), "{}", stderr(&o));
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn areas_flag_needs_a_value() {
    let o = cli(&[DRAIN, "-o", "/tmp/unused.json", "--areas"]);
    assert_eq!(o.status.code(), Some(2));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `scripts/cargo test -p ts2-import --test areas --test cli`
Expected: compile error — unresolved module `ts2_import::areas`.

- [ ] **Step 3: Write the area files**

`layouts/liverpool-st.areas.json`:
```json
{
  "schema": 1,
  "boundaries": ["61", "64", "63", "66", "65", "68", "91", "90", "93", "92", "95", "94"],
  "areas": [
    {"name": "Liverpool Street", "seeds": ["9"]},
    {"name": "Bethnal Green", "seeds": ["72"]},
    {"name": "Hackney & Bow", "seeds": ["121"]}
  ]
}
```

`layouts/drain.areas.json`:
```json
{
  "schema": 1,
  "boundaries": ["73", "84"],
  "areas": [
    {"name": "Bank", "seeds": ["72"]},
    {"name": "Waterloo", "seeds": ["75"]}
  ]
}
```

`layouts/gretz-armainvilliers.areas.json`:
```json
{
  "schema": 1,
  "boundaries": ["39,1V1", "39,1V2", "52,1"],
  "areas": [
    {"name": "Gretz", "seeds": ["3618"]},
    {"name": "Tournan & Marles", "seeds": ["504"]},
    {"name": "Mortcerf & Coulommiers", "seeds": ["725"]}
  ]
}
```

- [ ] **Step 4: Implement `areas.rs`**

`crates/ts2-import/src/areas.rs`:
```rust
//! Signalling areas for converted worlds, from a hand-made per-layout file
//! (spec §5). Each area floods the section graph from its seeds; the flood
//! never crosses a node where a boundary signal stands. Names are opaque.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use signalbox_core::world::file::{AreaFile, WorldFile};
use signalbox_core::world::{LoadError, World};

pub const AREAS_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AreasFile {
    pub schema: u32,
    #[serde(default)]
    pub boundaries: Vec<String>,
    pub areas: Vec<AreaSpec>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AreaSpec {
    pub name: String,
    pub seeds: Vec<String>,
}

/// What one area ended up with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaCount {
    pub name: String,
    pub sections: usize,
    pub signals: usize,
}

/// Names quoted, so names containing commas stay readable.
fn list(names: &[String]) -> String {
    names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AreasError {
    #[error("not an areas file: {0}")]
    Parse(String),
    #[error("unsupported areas schema {0} (expected {expected})", expected = AREAS_SCHEMA)]
    Schema(u32),
    #[error("no areas listed")]
    NoAreas,
    #[error("duplicate area names: {}", list(.0))]
    DuplicateAreas(Vec<String>),
    #[error("areas without seeds: {}", list(.0))]
    NoSeeds(Vec<String>),
    #[error("unknown names: {}", list(.0))]
    UnknownNames(Vec<String>),
    #[error("boundaries that are not signals: {}", list(.0))]
    NotSignals(Vec<String>),
    #[error("boundary signals not at a segment end: {}", list(.0))]
    NotAtSegmentEnd(Vec<String>),
    #[error("sections reached by more than one area: {}", list(.0))]
    DoublyReached(Vec<String>),
    #[error("sections reached by no area: {}", list(.0))]
    Unreached(Vec<String>),
    #[error("the world with areas does not load: {0}")]
    Invalid(LoadError),
}

pub fn parse(json: &str) -> Result<AreasFile, AreasError> {
    let f: AreasFile = serde_json::from_str(json).map_err(|e| AreasError::Parse(e.to_string()))?;
    if f.schema != AREAS_SCHEMA {
        return Err(AreasError::Schema(f.schema));
    }
    Ok(f)
}

/// Rewrite the world's areas, each section's area and each signal's area
/// (the area of the section its segment belongs to). All or nothing.
pub fn apply(world: &mut WorldFile, spec: &AreasFile) -> Result<Vec<AreaCount>, AreasError> {
    let owner = assign(world, spec)?;
    let mut out = world.clone();
    out.areas = spec.areas.iter().map(|a| AreaFile { name: a.name.clone() }).collect();
    let mut counts: Vec<AreaCount> =
        spec.areas.iter().map(|a| AreaCount { name: a.name.clone(), sections: 0, signals: 0 }).collect();
    for s in &mut out.sections {
        let i = owner[&s.name];
        s.area = spec.areas[i].name.clone();
        counts[i].sections += 1;
    }
    let seg_section: BTreeMap<&str, &str> =
        world.segments.iter().map(|g| (g.name.as_str(), g.section.as_str())).collect();
    for s in &mut out.signals {
        let sec = seg_section.get(s.segment.as_str()).ok_or_else(|| {
            AreasError::Invalid(LoadError::UnknownRef { kind: "segment", name: s.segment.clone(), from: s.name.clone() })
        })?;
        let i = owner[*sec];
        s.area = spec.areas[i].name.clone();
        counts[i].signals += 1;
    }
    World::from_file(out.clone()).map_err(AreasError::Invalid)?;
    *world = out;
    Ok(counts)
}

/// Section name → index of the area that owns it.
fn assign(world: &WorldFile, spec: &AreasFile) -> Result<BTreeMap<String, usize>, AreasError> {
    if spec.areas.is_empty() {
        return Err(AreasError::NoAreas);
    }
    let mut seen = BTreeSet::new();
    let dup: BTreeSet<String> =
        spec.areas.iter().filter(|a| !seen.insert(a.name.as_str())).map(|a| a.name.clone()).collect();
    if !dup.is_empty() {
        return Err(AreasError::DuplicateAreas(dup.into_iter().collect()));
    }
    let seedless: BTreeSet<String> =
        spec.areas.iter().filter(|a| a.seeds.is_empty()).map(|a| a.name.clone()).collect();
    if !seedless.is_empty() {
        return Err(AreasError::NoSeeds(seedless.into_iter().collect()));
    }

    let sections: BTreeSet<&str> = world.sections.iter().map(|s| s.name.as_str()).collect();
    let segs: BTreeMap<&str, (&str, &str, f64, &str)> = world
        .segments
        .iter()
        .map(|g| (g.name.as_str(), (g.from.as_str(), g.to.as_str(), g.length_m, g.section.as_str())))
        .collect();
    let signals: BTreeMap<&str, (&str, f64)> =
        world.signals.iter().map(|s| (s.name.as_str(), (s.segment.as_str(), s.offset_m))).collect();
    let berths: BTreeMap<&str, (Option<&str>, Option<&str>)> = world
        .berths
        .iter()
        .map(|b| (b.name.as_str(), (b.signal.as_deref(), b.boundary.as_deref())))
        .collect();
    let mut node_secs: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for g in &world.segments {
        node_secs.entry(g.from.as_str()).or_default().insert(g.section.as_str());
        node_secs.entry(g.to.as_str()).or_default().insert(g.section.as_str());
    }
    let signal_section = |s: &str| signals.get(s).and_then(|(seg, _)| segs.get(seg)).map(|g| g.3);
    let resolve = |name: &str| {
        if let Some(s) = sections.get(name) {
            return Some(*s);
        }
        if signals.contains_key(name) {
            return signal_section(name);
        }
        match berths.get(name)? {
            (Some(sig), _) => signal_section(*sig),
            (None, Some(node)) => node_secs.get(*node)?.iter().next().copied(),
            (None, None) => None,
        }
    };

    let mut unknown: BTreeSet<String> = BTreeSet::new();
    let mut not_signals: BTreeSet<String> = BTreeSet::new();
    for b in &spec.boundaries {
        if signals.contains_key(b.as_str()) {
            continue;
        }
        if resolve(b.as_str()).is_some() {
            not_signals.insert(b.clone());
        } else {
            unknown.insert(b.clone());
        }
    }
    let mut seeds: Vec<Vec<&str>> = Vec::new();
    for a in &spec.areas {
        let mut v = Vec::new();
        for s in &a.seeds {
            match resolve(s.as_str()) {
                Some(sec) => v.push(sec),
                None => {
                    unknown.insert(s.clone());
                }
            }
        }
        seeds.push(v);
    }
    if !unknown.is_empty() {
        return Err(AreasError::UnknownNames(unknown.into_iter().collect()));
    }
    if !not_signals.is_empty() {
        return Err(AreasError::NotSignals(not_signals.into_iter().collect()));
    }

    let mut blocked: BTreeSet<&str> = BTreeSet::new();
    let mut off_end: BTreeSet<String> = BTreeSet::new();
    for b in &spec.boundaries {
        let (seg, offset) = signals[b.as_str()];
        let Some(&(from, to, len, _)) = segs.get(seg) else {
            return Err(AreasError::Invalid(LoadError::UnknownRef {
                kind: "segment",
                name: seg.to_string(),
                from: b.clone(),
            }));
        };
        if offset.abs() > 1e-6 && (offset - len).abs() > 1e-6 {
            off_end.insert(b.clone());
        }
        blocked.insert(if offset <= len / 2.0 { from } else { to });
    }
    if !off_end.is_empty() {
        return Err(AreasError::NotAtSegmentEnd(off_end.into_iter().collect()));
    }

    let mut adj: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (n, secs) in &node_secs {
        if blocked.contains(n) {
            continue;
        }
        for &a in secs {
            for &b in secs {
                if a != b {
                    adj.entry(a).or_default().insert(b);
                }
            }
        }
    }
    let mut owners: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
    for (i, start) in seeds.iter().enumerate() {
        let mut reached: BTreeSet<&str> = start.iter().copied().collect();
        let mut stack: Vec<&str> = reached.iter().copied().collect();
        while let Some(c) = stack.pop() {
            for &d in adj.get(c).into_iter().flatten() {
                if reached.insert(d) {
                    stack.push(d);
                }
            }
        }
        for c in reached {
            owners.entry(c).or_default().insert(i);
        }
    }
    let double: BTreeSet<String> = world
        .sections
        .iter()
        .filter(|s| owners.get(s.name.as_str()).is_some_and(|o| o.len() > 1))
        .map(|s| s.name.clone())
        .collect();
    if !double.is_empty() {
        return Err(AreasError::DoublyReached(double.into_iter().collect()));
    }
    let unreached: BTreeSet<String> =
        world.sections.iter().filter(|s| !owners.contains_key(s.name.as_str())).map(|s| s.name.clone()).collect();
    if !unreached.is_empty() {
        return Err(AreasError::Unreached(unreached.into_iter().collect()));
    }
    Ok(owners
        .into_iter()
        .map(|(s, o)| (s.to_string(), *o.iter().next().expect("reached sections have an owner")))
        .collect())
}
```

In `crates/ts2-import/src/lib.rs` add `pub mod areas;` above `pub mod graph;`.

- [ ] **Step 5: Add `--areas` to the CLI**

Replace `crates/ts2-import/src/main.rs` with:
```rust
//! Command-line converter: TS2 simulation → signalbox world.

use std::process::ExitCode;

use ts2_import::{areas, convert, report};

const USAGE: &str = "usage: ts2-import <input.json> -o <world.json> [--strict] [--areas <areas.json>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut input, mut output, mut strict, mut areas_path) = (None, None, false, None);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                match args.get(i) {
                    Some(o) => output = Some(o.clone()),
                    None => return usage(),
                }
            }
            "--areas" => {
                i += 1;
                match args.get(i) {
                    Some(a) => areas_path = Some(a.clone()),
                    None => return usage(),
                }
            }
            "--strict" => strict = true,
            a if input.is_none() && !a.starts_with('-') => input = Some(a.to_string()),
            _ => return usage(),
        }
        i += 1;
    }
    let (Some(input), Some(output)) = (input, output) else { return usage() };
    let spec = match &areas_path {
        Some(p) => match std::fs::read_to_string(p).map_err(|e| e.to_string()).and_then(|t| areas::parse(&t).map_err(|e| e.to_string())) {
            Ok(s) => Some((p.clone(), s)),
            Err(e) => {
                eprintln!("{p}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let text = match std::fs::read_to_string(&input) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{input}: {e}");
            return ExitCode::FAILURE;
        }
    };
    match convert(&text) {
        Ok(mut c) => {
            let mut counts = Vec::new();
            if let Some((p, s)) = &spec {
                match areas::apply(&mut c.world, s) {
                    Ok(v) => counts = v,
                    Err(e) => {
                        eprintln!("{p}: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            let json = serde_json::to_string_pretty(&c.world).expect("world serialises");
            if let Err(e) = std::fs::write(&output, json) {
                eprintln!("{output}: {e}");
                return ExitCode::FAILURE;
            }
            eprint!("{}", c.report.render());
            eprintln!(
                "wrote {output}: {} sections, {} signals, {} routes, {} services, {} entries",
                c.world.sections.len(),
                c.world.signals.len(),
                c.world.routes.len(),
                c.world.services.len(),
                c.world.entries.len()
            );
            for a in &counts {
                eprintln!("area {}: {} sections, {} signals", a.name, a.sections, a.signals);
            }
            if strict && c.report.count(report::ROUTE_DROPPED) > 0 { ExitCode::FAILURE } else { ExitCode::SUCCESS }
        }
        Err(e) => {
            eprintln!("{input}: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}
```

- [ ] **Step 6: Run the tests**

Run: `scripts/cargo test -p ts2-import`
Expected: PASS, including the unchanged `convert`, `soak` (non-ignored) and the new `areas` and `cli` tests. If a shipped-layout count differs from the brief's prototype numbers, stop and report — do not edit the expected counts.

- [ ] **Step 7: Commit**

```bash
git add crates/ts2-import/src/areas.rs crates/ts2-import/src/lib.rs crates/ts2-import/src/main.rs \
  crates/ts2-import/tests/areas.rs crates/ts2-import/tests/cli.rs layouts/
git commit -m "feat(ts2-import): --areas splits converted worlds into signal boxes

Area files for Liverpool Street, Drain and Gretz.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---
### Task 3: `protocol` crate — messages, `Layout`, `View`, `Delta`, diff/apply, golden JSON

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `crates/protocol/Cargo.toml`
- Create: `crates/protocol/src/lib.rs`, `src/msg.rs`, `src/view.rs`, `src/diff.rs`
- Test: `crates/protocol/tests/golden.rs`, `crates/protocol/tests/diff.rs`

**Interfaces:**
- Consumes: `signalbox_core::{aspect::Aspect, events::Rejection, network::{Dir, PointsPos}}` (re-exported by `protocol`, amendment 3).
- Produces (crate `signalbox-protocol`, lib `protocol`; everything re-exported at the crate root):
  - `enum ClientMsg { Claim { area: String }, Release, Command { cmd: PlayerCommand }, Vote { proposal: Proposal }, Resync }` — tag `"type"`.
  - `enum PlayerCommand { SetRoute { entrance: String, exit: ExitName }, CancelRoute { entrance: String }, SetAutoWorking { entrance: String, on: bool }, SwingPoints { points: String, to: PointsPos }, Interpose { berth: String, headcode: String }, CancelBerth { berth: String } }` — tag `"cmd"`; `Clone, Debug, PartialEq, Eq`.
  - `enum ExitName { Signal(String), Node(String) }` — `{"kind": ..., "name": ...}`.
  - `enum Proposal { Pause, Resume, Speed { x: u8 } }` — tag `"kind"`; `Clone, Copy, Debug, PartialEq, Eq`.
  - `enum ServerMsg { Layout(Layout), View(View), Delta(Delta), Notice(Notice) }` — tag `"type"`, payload fields inline.
  - `enum Notice { Rejected { cmd: PlayerCommand, reason: Rejection }, NotYourArea { area: String }, Spad { signal: String, train: String }, Collision { section: String }, Late { train: String, place: String, platform: String, late_s: i64 }, WrongPlatform { train: String, place: String, platform: String, expected: String }, Handover { headcode: String, from_area: String }, AreaTaken { area: String, holder: String }, Replaced, GameCrashed, Error { code: String, message: String } }` — tag `"kind"`.
  - `mod codes` — `BAD_HEADCODE, BAD_SPEED, NOT_A_HOLDER, NOT_HOLDING, UNKNOWN_AREA, RESERVED_NAME, SAVE_FAILED: &str`.
  - `struct Layout { title, you: String, area: Option<String>, areas: Vec<String>, sections: Vec<SectionInfo>, segments: Vec<SegmentInfo>, signals: Vec<SignalInfo>, points: Vec<PointsInfo>, berths: Vec<BerthInfo>, platforms: Vec<PlatformInfo>, routes: Vec<RouteInfo> }` with
    `SectionInfo { name, area: String, fringe: bool }`, `SegmentInfo { name, from, to: String, length_m: f64, section: String }`, `SignalInfo { name, area, segment: String, offset_m: f64, direction: Dir, aspects: u8, operable: bool }`, `PointsInfo { name, section, area: String, operable: bool }`, `BerthInfo { name: String, signal: Option<String>, boundary: Option<String>, area: String, operable: bool }`, `PlatformInfo { place, platform, segment: String, from_m: f64, to_m: f64 }`, `RouteInfo { name, entrance: String, exit: ExitName, automatic: bool, operable: bool }`.
  - `struct View { seq: u64, sim_time: f64, speed: u8, paused: bool, vote: Option<VoteView>, holders: BTreeMap<String, String>, score: Option<i64>, signals: BTreeMap<String, Aspect>, routes: BTreeMap<String, RouteView>, points: BTreeMap<String, PointsView>, sections: BTreeMap<String, SectionView>, berths: BTreeMap<String, String> }` with `VoteView { proposal: Proposal, agreed: Vec<String>, expires_in_s: u32 }`, `RouteView { state: RouteState, auto_working: bool }`, `enum RouteState { Setting, Locked, Cancelling }`, `PointsView { position: PointsPos, moving: bool, locked: bool }`, `SectionView { occupied: bool, held: Held }`, `enum Held { Free, Path, Overlap }`. `routes` lists only non-idle routes and `berths` only filled berths; the other maps list every visible element.
  - `struct Delta { seq: u64, sim_time: Option<f64>, speed: Option<u8>, paused: Option<bool>, vote: Option<Option<VoteView>>, score: Option<Option<i64>>, holders: BTreeMap<String, String>, signals: BTreeMap<String, Aspect>, routes: BTreeMap<String, Option<RouteView>>, points: BTreeMap<String, PointsView>, sections: BTreeMap<String, SectionView>, berths: BTreeMap<String, Option<String>> }` (`Default`; `None`/empty = unchanged; inner `None` serialises as `null` = cleared) and `Delta::is_empty(&self) -> bool`.
  - `fn diff(old: &View, new: &View) -> Option<Delta>` (numbered `new.seq`; `None` when nothing changed; both views must cover the same elements), `View::apply(&mut self, d: &Delta) -> Result<(), SeqGap>` (refuses unless `d.seq == self.seq + 1`, leaving the view untouched), `struct SeqGap { have: u64, got: u64 }`.

- [ ] **Step 1: Add the crate skeleton**

In the root `Cargo.toml` change the members line to:
```toml
members = ["crates/core", "crates/sim-cli", "crates/ts2-import", "crates/protocol"]
```

`crates/protocol/Cargo.toml`:
```toml
[package]
name = "signalbox-protocol"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "protocol"
path = "src/lib.rs"

[dependencies]
signalbox-core = { path = "../core" }
serde.workspace = true
thiserror.workspace = true

[dev-dependencies]
serde_json.workspace = true
```

- [ ] **Step 2: Write the failing tests**

`crates/protocol/tests/golden.rs`:
```rust
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
```

`crates/protocol/tests/diff.rs`:
```rust
//! Deltas rebuild views exactly (spec §4.3).

use std::collections::BTreeMap;

use protocol::*;

fn s(x: &str) -> String {
    x.to_string()
}

fn base() -> View {
    View {
        seq: 1,
        sim_time: 25200.0,
        speed: 1,
        paused: false,
        vote: Some(VoteView { proposal: Proposal::Pause, agreed: vec![s("alice")], expires_in_s: 12 }),
        holders: BTreeMap::from([(s("East"), s("robot")), (s("West"), s("alice"))]),
        score: Some(0),
        signals: BTreeMap::from([(s("A"), Aspect::Red), (s("W1"), Aspect::Red)]),
        routes: BTreeMap::from([(s("W1-A"), RouteView { state: RouteState::Setting, auto_working: false })]),
        points: BTreeMap::from([(s("P"), PointsView { position: PointsPos::Normal, moving: false, locked: false })]),
        sections: BTreeMap::from([
            (s("TW1"), SectionView { occupied: false, held: Held::Free }),
            (s("TW2"), SectionView { occupied: false, held: Held::Path }),
        ]),
        berths: BTreeMap::from([(s("BW1"), s("1E01"))]),
    }
}

fn changed() -> View {
    let mut v = base();
    v.seq = 2;
    v.sim_time = 25201.6;
    v.speed = 8;
    v.paused = true;
    v.vote = None;
    v.score = Some(5);
    v.holders.insert(s("East"), s("bob"));
    v.signals.insert(s("W1"), Aspect::Yellow);
    v.routes.insert(s("W1-A"), RouteView { state: RouteState::Locked, auto_working: true });
    v.routes.insert(s("A-E"), RouteView { state: RouteState::Cancelling, auto_working: false });
    v.points.insert(s("P"), PointsView { position: PointsPos::Reverse, moving: true, locked: true });
    v.sections.insert(s("TW1"), SectionView { occupied: true, held: Held::Overlap });
    v.berths.remove("BW1");
    v.berths.insert(s("BA"), s("1E01"));
    v
}

#[test]
fn identical_views_give_no_delta() {
    let mut next = base();
    next.seq = 2;
    assert_eq!(diff(&base(), &next), None);
}

#[test]
fn applying_the_diff_rebuilds_the_new_view() {
    let (old, new) = (base(), changed());
    let d = diff(&old, &new).expect("views differ");
    let mut v = old.clone();
    v.apply(&d).unwrap();
    assert_eq!(v, new);
    let wire = serde_json::to_string(&ServerMsg::Delta(d)).unwrap();
    let ServerMsg::Delta(back) = serde_json::from_str::<ServerMsg>(&wire).unwrap() else { panic!("{wire}") };
    let mut v = old;
    v.apply(&back).unwrap();
    assert_eq!(v, new);
}

#[test]
fn a_delta_carries_only_what_changed() {
    let mut new = base();
    new.seq = 2;
    new.signals.insert(s("A"), Aspect::Green);
    new.berths.remove("BW1");
    let d = diff(&base(), &new).unwrap();
    assert_eq!(
        d,
        Delta {
            seq: 2,
            signals: BTreeMap::from([(s("A"), Aspect::Green)]),
            berths: BTreeMap::from([(s("BW1"), None)]),
            ..Delta::default()
        }
    );
    assert!(!d.is_empty());
    assert!(Delta { seq: 4, ..Delta::default() }.is_empty());
}

#[test]
fn cleared_vote_and_score_travel_as_null() {
    let mut new = base();
    new.seq = 2;
    new.vote = None;
    new.score = None;
    let d = diff(&base(), &new).unwrap();
    assert_eq!((d.vote.clone(), d.score), (Some(None), Some(None)));
    let json = serde_json::to_value(&d).unwrap();
    assert!(json["vote"].is_null() && json["score"].is_null(), "{json}");
    let mut v = base();
    v.apply(&d).unwrap();
    assert_eq!(v, new);
}

#[test]
fn a_gap_is_refused_and_leaves_the_view_alone() {
    let mut new = changed();
    new.seq = 3;
    let d = diff(&base(), &new).unwrap();
    let mut v = base();
    assert_eq!(v.apply(&d), Err(SeqGap { have: 1, got: 3 }));
    assert_eq!(v, base());
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-protocol`
Expected: compile errors — the crate has no `src/lib.rs` yet.

- [ ] **Step 4: Implement**

`crates/protocol/src/lib.rs`:
```rust
//! Messages between signalbox clients and a game, and the per-player views
//! they carry (spec §3–§4). No I/O: callers move these as JSON text. Core's
//! small enums are reused as they are; their serde forms are the wire forms.

pub mod diff;
pub mod msg;
pub mod view;

pub use diff::{SeqGap, diff};
pub use msg::{ClientMsg, ExitName, Notice, PlayerCommand, Proposal, ServerMsg};
pub use signalbox_core::aspect::Aspect;
pub use signalbox_core::events::Rejection;
pub use signalbox_core::network::{Dir, PointsPos};
pub use view::*;

/// Codes carried by `Notice::Error`.
pub mod codes {
    pub const BAD_HEADCODE: &str = "bad_headcode";
    pub const BAD_SPEED: &str = "bad_speed";
    pub const NOT_A_HOLDER: &str = "not_a_holder";
    pub const NOT_HOLDING: &str = "not_holding";
    pub const UNKNOWN_AREA: &str = "unknown_area";
    pub const RESERVED_NAME: &str = "reserved_name";
    pub const SAVE_FAILED: &str = "save_failed";
}
```

`crates/protocol/src/msg.rs`:
```rust
//! Client ⇄ game messages. Everything is named, never numbered.

use serde::{Deserialize, Serialize};
use signalbox_core::events::Rejection;
use signalbox_core::network::PointsPos;

use crate::view::{Delta, Layout, View};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Take a free area.
    Claim { area: String },
    /// Give your area back to the robot.
    Release,
    Command { cmd: PlayerCommand },
    /// Propose, or agree to, a clock change.
    Vote { proposal: Proposal },
    /// Ask for the layout and a full view.
    Resync,
}

/// Core `Command` with names in place of ids.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum PlayerCommand {
    SetRoute { entrance: String, exit: ExitName },
    CancelRoute { entrance: String },
    SetAutoWorking { entrance: String, on: bool },
    SwingPoints { points: String, to: PointsPos },
    Interpose { berth: String, headcode: String },
    CancelBerth { berth: String },
}

/// Where a route ends: a signal, or a buffer stop / boundary node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum ExitName {
    Signal(String),
    Node(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Proposal {
    Pause,
    Resume,
    /// `x` sim ticks per 0.1 s of real time; 1, 2, 4 or 8.
    Speed { x: u8 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Layout(Layout),
    View(View),
    Delta(Delta),
    Notice(Notice),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Notice {
    /// The command was refused (unknown name, or by the interlocking).
    Rejected { cmd: PlayerCommand, reason: Rejection },
    /// The command's subject lies in `area`, which is not yours.
    NotYourArea { area: String },
    Spad { signal: String, train: String },
    Collision { section: String },
    Late { train: String, place: String, platform: String, late_s: i64 },
    WrongPlatform { train: String, place: String, platform: String, expected: String },
    /// A berth in your area was filled by a step from `from_area`'s berth.
    Handover { headcode: String, from_area: String },
    AreaTaken { area: String, holder: String },
    Replaced,
    GameCrashed,
    Error { code: String, message: String },
}
```

`crates/protocol/src/view.rs`:
```rust
//! What a player sees: the static `Layout` of their screen and the dynamic
//! `View`, keyed by element name, plus the `Delta` between two views.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use signalbox_core::aspect::Aspect;
use signalbox_core::network::{Dir, PointsPos};

use crate::msg::{ExitName, Proposal};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub title: String,
    /// The player this layout was built for.
    pub you: String,
    /// The area they hold; `None` for a spectator.
    pub area: Option<String>,
    /// Every area of the layout, in world order.
    pub areas: Vec<String>,
    pub sections: Vec<SectionInfo>,
    pub segments: Vec<SegmentInfo>,
    pub signals: Vec<SignalInfo>,
    pub points: Vec<PointsInfo>,
    pub berths: Vec<BerthInfo>,
    pub platforms: Vec<PlatformInfo>,
    pub routes: Vec<RouteInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionInfo {
    pub name: String,
    pub area: String,
    /// Visible but in a neighbouring area.
    pub fringe: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SegmentInfo {
    pub name: String,
    pub from: String,
    pub to: String,
    pub length_m: f64,
    pub section: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SignalInfo {
    pub name: String,
    pub area: String,
    pub segment: String,
    pub offset_m: f64,
    pub direction: Dir,
    pub aspects: u8,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointsInfo {
    pub name: String,
    pub section: String,
    pub area: String,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BerthInfo {
    pub name: String,
    pub signal: Option<String>,
    pub boundary: Option<String>,
    pub area: String,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlatformInfo {
    pub place: String,
    pub platform: String,
    pub segment: String,
    pub from_m: f64,
    pub to_m: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteInfo {
    pub name: String,
    pub entrance: String,
    pub exit: ExitName,
    pub automatic: bool,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct View {
    pub seq: u64,
    /// Seconds since midnight.
    pub sim_time: f64,
    pub speed: u8,
    pub paused: bool,
    pub vote: Option<VoteView>,
    /// Area → holding player, or `"robot"`.
    pub holders: BTreeMap<String, String>,
    /// Your area's penalty points; `None` for a spectator.
    pub score: Option<i64>,
    pub signals: BTreeMap<String, Aspect>,
    /// Only routes that are not idle.
    pub routes: BTreeMap<String, RouteView>,
    pub points: BTreeMap<String, PointsView>,
    pub sections: BTreeMap<String, SectionView>,
    /// Only berths holding a headcode.
    pub berths: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoteView {
    pub proposal: Proposal,
    pub agreed: Vec<String>,
    /// Whole seconds of real time before it lapses (rounded up).
    pub expires_in_s: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteState {
    Setting,
    Locked,
    Cancelling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteView {
    pub state: RouteState,
    pub auto_working: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PointsView {
    /// Where the points lie, or are moving to.
    pub position: PointsPos,
    pub moving: bool,
    /// Their section is held by a route.
    pub locked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Held {
    Free,
    Path,
    Overlap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionView {
    pub occupied: bool,
    pub held: Held,
}

/// Changes since view `seq - 1`. Absent = unchanged; `null` = cleared.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    pub seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sim_time: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "double_option")]
    pub vote: Option<Option<VoteView>>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "double_option")]
    pub score: Option<Option<i64>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub holders: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub signals: BTreeMap<String, Aspect>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub routes: BTreeMap<String, Option<RouteView>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub points: BTreeMap<String, PointsView>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sections: BTreeMap<String, SectionView>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub berths: BTreeMap<String, Option<String>>,
}

impl Delta {
    /// Nothing but the sequence number.
    pub fn is_empty(&self) -> bool {
        *self == Delta { seq: self.seq, ..Delta::default() }
    }
}

/// `Some(None)` ⇄ `null`, `Some(Some(v))` ⇄ `v`; an absent field is `None`.
mod double_option {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<T: Serialize, S: Serializer>(v: &Option<Option<T>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(inner) => inner.serialize(s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(d).map(Some)
    }
}
```

`crates/protocol/src/diff.rs`:
```rust
//! View deltas: what changed between two views of the same elements.

use std::collections::BTreeMap;

use crate::view::{Delta, View};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("delta {got} does not follow view {have}")]
pub struct SeqGap {
    pub have: u64,
    pub got: u64,
}

/// The changes from `old` to `new`, numbered `new.seq`, or `None` when
/// nothing but the number differs. Both views must cover the same elements
/// (the game sends a full view whenever a player's visible set changes).
pub fn diff(old: &View, new: &View) -> Option<Delta> {
    let mut d = Delta { seq: new.seq, ..Delta::default() };
    if old.sim_time != new.sim_time {
        d.sim_time = Some(new.sim_time);
    }
    if old.speed != new.speed {
        d.speed = Some(new.speed);
    }
    if old.paused != new.paused {
        d.paused = Some(new.paused);
    }
    if old.vote != new.vote {
        d.vote = Some(new.vote.clone());
    }
    if old.score != new.score {
        d.score = Some(new.score);
    }
    d.holders = changed(&old.holders, &new.holders);
    d.signals = changed(&old.signals, &new.signals);
    d.points = changed(&old.points, &new.points);
    d.sections = changed(&old.sections, &new.sections);
    d.routes = sparse(&old.routes, &new.routes);
    d.berths = sparse(&old.berths, &new.berths);
    (!d.is_empty()).then_some(d)
}

/// Entries of `new` that are missing from `old` or differ.
fn changed<V: Clone + PartialEq>(old: &BTreeMap<String, V>, new: &BTreeMap<String, V>) -> BTreeMap<String, V> {
    new.iter().filter(|(k, v)| old.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Like `changed`, plus `None` for entries that went away.
fn sparse<V: Clone + PartialEq>(old: &BTreeMap<String, V>, new: &BTreeMap<String, V>) -> BTreeMap<String, Option<V>> {
    let mut out: BTreeMap<String, Option<V>> = changed(old, new).into_iter().map(|(k, v)| (k, Some(v))).collect();
    for k in old.keys() {
        if !new.contains_key(k) {
            out.insert(k.clone(), None);
        }
    }
    out
}

impl View {
    /// Apply the delta that follows this view. A delta out of sequence is
    /// refused and the view is left as it was.
    pub fn apply(&mut self, d: &Delta) -> Result<(), SeqGap> {
        if d.seq != self.seq + 1 {
            return Err(SeqGap { have: self.seq, got: d.seq });
        }
        self.seq = d.seq;
        if let Some(t) = d.sim_time {
            self.sim_time = t;
        }
        if let Some(x) = d.speed {
            self.speed = x;
        }
        if let Some(p) = d.paused {
            self.paused = p;
        }
        if let Some(v) = &d.vote {
            self.vote = v.clone();
        }
        if let Some(s) = d.score {
            self.score = s;
        }
        self.holders.extend(d.holders.iter().map(|(k, v)| (k.clone(), v.clone())));
        self.signals.extend(d.signals.iter().map(|(k, v)| (k.clone(), *v)));
        self.points.extend(d.points.iter().map(|(k, v)| (k.clone(), *v)));
        self.sections.extend(d.sections.iter().map(|(k, v)| (k.clone(), *v)));
        for (k, v) in &d.routes {
            match v {
                Some(r) => {
                    self.routes.insert(k.clone(), *r);
                }
                None => {
                    self.routes.remove(k);
                }
            }
        }
        for (k, v) in &d.berths {
            match v {
                Some(h) => {
                    self.berths.insert(k.clone(), h.clone());
                }
                None => {
                    self.berths.remove(k);
                }
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `scripts/cargo test -p signalbox-protocol`
Expected: PASS (both files). If a golden JSON differs, fix the type (serde attributes), never the expected JSON — the JSON is the wire contract.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/protocol
git commit -m "feat(protocol): wire messages, views and deltas

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: `game` crate — area map, visibility with fringe, and the `Layout`

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `crates/game/Cargo.toml`, `crates/game/src/lib.rs`
- Create: `crates/game/src/areas.rs`, `crates/game/src/layout.rs`
- Create: `crates/game/tests/fixtures/twobox.json`, `crates/game/tests/common/mod.rs`
- Test: `crates/game/tests/areas.rs`, `crates/game/tests/layout.rs`

**Interfaces:**
- Consumes: `protocol::{Layout, SectionInfo, SegmentInfo, SignalInfo, PointsInfo, BerthInfo, PlatformInfo, RouteInfo, ExitName}` (Task 3); `ts2_import::{convert, areas::{parse, apply}}` (Task 2, tests only).
- Produces (crate `signalbox-game`, lib `game`):
  - `game::areas::AreaMap { signal: Vec<AreaId>, berth_section: Vec<SectionId>, berth: Vec<AreaId>, points: Vec<Option<AreaId>> }` with `AreaMap::new(w: &World) -> AreaMap` and `AreaMap::subject(&self, cmd: &Command) -> Option<AreaId>` (§3.4 mapping; `None` for `SwingPoints` on a node that is not points, or an id out of range).
  - `game::areas::Visibility { area: Option<AreaId>, sections: Vec<SectionId>, fringe: BTreeSet<SectionId>, signals: Vec<SignalId>, points: Vec<NodeId>, berths: Vec<BerthId>, routes: Vec<RouteId> }` (all lists in index order) with `Visibility::spectator(w: &World, map: &AreaMap) -> Visibility`, `Visibility::of_area(w: &World, map: &AreaMap, a: AreaId) -> Visibility`, `Visibility::operable(&self, a: AreaId) -> bool`.
  - `game::areas::fringe(w: &World, a: AreaId) -> BTreeSet<SectionId>` (amendment 6), `game::areas::signal_node(net: &Network, s: SignalId) -> NodeId`, `game::areas::signal_section(net: &Network, s: SignalId) -> SectionId`.
  - `game::layout::build_layout(w: &World, map: &AreaMap, vis: &Visibility, you: &str) -> Layout`, `game::layout::exit_name(w: &World, e: Exit) -> ExitName`.
  - Test fixture `twobox.json`: areas West (TW1, TW2; signals W1, A, W2) and East (TP with points P, TE, TN; signals C, D). `A` (at J1) is the West→East boundary signal; `C`/`D` face West. Routes W1-A, A-E, A-N, C-W2, D-W2, W2-W. Timetable: 1E01 W 07:00 → EST, 2W03 E 07:10 → W, 1N02 W 07:14 → NST, 2W04 N 07:20 → W.
  - Test helpers (`tests/common/mod.rs`): `TWOBOX`, `twobox_json() -> String`, `twobox() -> World`, `area(w: &World, name: &str) -> AreaId`, `map<V: Clone>(pairs: &[(&str, V)]) -> BTreeMap<String, V>`, `liverpool_json() -> String`.

- [ ] **Step 1: Add the crate skeleton and fixture**

Root `Cargo.toml` members:
```toml
members = ["crates/core", "crates/sim-cli", "crates/ts2-import", "crates/protocol", "crates/game"]
```

`crates/game/Cargo.toml`:
```toml
[package]
name = "signalbox-game"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "game"
path = "src/lib.rs"

[dependencies]
signalbox-core = { path = "../core" }
signalbox-protocol = { path = "../protocol" }

[dev-dependencies]
serde_json.workspace = true
ts2-import = { path = "../ts2-import" }
```

`crates/game/src/lib.rs`:
```rust
//! The multiplayer game (spec §6): a core `Sim` shared by players who each
//! run one signalling area, with the robot playing the rest. Pure logic; the
//! only I/O is the SQLite save in `save`.

pub mod areas;
pub mod layout;
```

`crates/game/tests/fixtures/twobox.json`:
```json
{
  "schema": 1,
  "title": "Two boxes",
  "areas": [{"name": "West"}, {"name": "East"}],
  "sections": [
    {"name": "TW1", "area": "West"},
    {"name": "TW2", "area": "West"},
    {"name": "TP", "area": "East"},
    {"name": "TE", "area": "East"},
    {"name": "TN", "area": "East"}
  ],
  "nodes": [
    {"name": "W", "kind": "boundary"},
    {"name": "J0", "kind": "joint"},
    {"name": "J1", "kind": "joint"},
    {"name": "P", "kind": "points", "toe": "pa", "normal": "pe", "reverse": "pn"},
    {"name": "J2", "kind": "joint"},
    {"name": "J3", "kind": "joint"},
    {"name": "E", "kind": "boundary"},
    {"name": "N", "kind": "boundary"}
  ],
  "segments": [
    {"name": "w1", "from": "W", "to": "J0", "length_m": 1000, "line_speed_kmh": 100, "section": "TW1"},
    {"name": "w2", "from": "J0", "to": "J1", "length_m": 1000, "line_speed_kmh": 100, "section": "TW2"},
    {"name": "pa", "from": "J1", "to": "P", "length_m": 40, "line_speed_kmh": 50, "section": "TP"},
    {"name": "pe", "from": "P", "to": "J2", "length_m": 40, "line_speed_kmh": 50, "section": "TP"},
    {"name": "pn", "from": "P", "to": "J3", "length_m": 40, "line_speed_kmh": 50, "section": "TP"},
    {"name": "e", "from": "J2", "to": "E", "length_m": 1500, "line_speed_kmh": 100, "section": "TE"},
    {"name": "n", "from": "J3", "to": "N", "length_m": 1500, "line_speed_kmh": 80, "section": "TN"}
  ],
  "signals": [
    {"name": "W1", "area": "West", "segment": "w1", "offset_m": 1000, "direction": "up", "aspects": 3},
    {"name": "A", "area": "West", "segment": "w2", "offset_m": 1000, "direction": "up", "aspects": 3},
    {"name": "W2", "area": "West", "segment": "w2", "offset_m": 0, "direction": "down", "aspects": 3},
    {"name": "C", "area": "East", "segment": "e", "offset_m": 0, "direction": "down", "aspects": 3},
    {"name": "D", "area": "East", "segment": "n", "offset_m": 0, "direction": "down", "aspects": 3}
  ],
  "berths": [
    {"name": "BW", "boundary": "W"},
    {"name": "BE", "boundary": "E"},
    {"name": "BN", "boundary": "N"},
    {"name": "BW1", "signal": "W1"},
    {"name": "BA", "signal": "A"},
    {"name": "BW2", "signal": "W2"},
    {"name": "BC", "signal": "C"},
    {"name": "BD", "signal": "D"}
  ],
  "platforms": [
    {"place": "EST", "platform": "1", "segment": "e", "from_m": 700, "to_m": 900},
    {"place": "NST", "platform": "1", "segment": "n", "from_m": 700, "to_m": 900}
  ],
  "routes": [
    {"entrance": "W1", "exit": {"kind": "signal", "name": "A"}, "path": ["TW2"]},
    {"entrance": "A", "exit": {"kind": "node", "name": "E"}, "path": ["TP", "TE"], "points": [{"points": "P", "position": "normal"}]},
    {"entrance": "A", "exit": {"kind": "node", "name": "N"}, "path": ["TP", "TN"], "points": [{"points": "P", "position": "reverse"}]},
    {"entrance": "C", "exit": {"kind": "signal", "name": "W2"}, "path": ["TP", "TW2"], "points": [{"points": "P", "position": "normal"}]},
    {"entrance": "D", "exit": {"kind": "signal", "name": "W2"}, "path": ["TP", "TW2"], "points": [{"points": "P", "position": "reverse"}]},
    {"entrance": "W2", "exit": {"kind": "node", "name": "W"}, "path": ["TW1"]}
  ],
  "train_types": [
    {"code": "EMU", "max_speed_kmh": 120, "accel": 0.8, "service_brake": 0.7, "emergency_brake": 1.2, "length_m": 100}
  ],
  "services": [
    {"headcode": "1E01", "train_type": "EMU", "calls": [{"place": "EST", "platform": "1", "arr": "07:04", "dep": "07:05"}]},
    {"headcode": "2W03", "train_type": "EMU", "calls": []},
    {"headcode": "1N02", "train_type": "EMU", "calls": [{"place": "NST", "platform": "1", "arr": "07:18", "dep": "07:19"}]},
    {"headcode": "2W04", "train_type": "EMU", "calls": []}
  ],
  "entries": [
    {"service": "1E01", "boundary": "W", "time": "07:00"},
    {"service": "2W03", "boundary": "E", "time": "07:10"},
    {"service": "1N02", "boundary": "W", "time": "07:14"},
    {"service": "2W04", "boundary": "N", "time": "07:20"}
  ],
  "options": {"start_time": "07:00"}
}
```

The routes are exactly what `Network::trace_route` finds (no overlaps: each route's overlap length is 0), so the file loads; Step 4's first test proves it.

`crates/game/tests/common/mod.rs`:
```rust
#![allow(dead_code)]

use std::collections::BTreeMap;

use signalbox_core::ids::AreaId;
use signalbox_core::world::World;

pub const TWOBOX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/twobox.json");

pub fn twobox_json() -> String {
    std::fs::read_to_string(TWOBOX).unwrap()
}

pub fn twobox() -> World {
    World::from_json(&twobox_json()).unwrap()
}

pub fn area(w: &World, name: &str) -> AreaId {
    w.net.area(name).unwrap_or_else(|| panic!("no area {name}"))
}

pub fn map<V: Clone>(pairs: &[(&str, V)]) -> BTreeMap<String, V> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

/// Liverpool Street converted from TS2 and split by its shipped area file.
pub fn liverpool_json() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/liverpool-st.json")).unwrap();
    let text = std::fs::read_to_string(format!("{dir}/../../layouts/liverpool-st.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&text).unwrap()).unwrap();
    serde_json::to_string(&w).unwrap()
}
```

- [ ] **Step 2: Write the failing tests**

`crates/game/tests/areas.rs`:
```rust
//! Command subjects, the fringe and what each player sees.

mod common;

use std::collections::BTreeSet;

use common::*;
use game::areas::{AreaMap, Visibility, fringe, signal_node};
use signalbox_core::events::Command;
use signalbox_core::ids::*;
use signalbox_core::network::PointsPos;
use signalbox_core::routes::Exit;
use signalbox_core::world::World;

fn names<T: Copy>(ids: &[T], name: impl Fn(T) -> String) -> Vec<String> {
    ids.iter().map(|&i| name(i)).collect()
}

#[test]
fn the_fixture_loads_with_two_areas() {
    let w = twobox();
    assert_eq!(w.net.areas.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["West", "East"]);
    assert_eq!(w.routes.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["W1-A", "A-E", "A-N", "C-W2", "D-W2", "W2-W"]);
}

#[test]
fn every_command_maps_to_its_subjects_area() {
    let w = twobox();
    let m = AreaMap::new(&w);
    let net = &w.net;
    let (west, east) = (area(&w, "West"), area(&w, "East"));
    let sig = |x: &str| net.signal(x).unwrap();
    let node = |x: &str| net.node(x).unwrap();
    let berth = |x: &str| net.berth(x).unwrap();
    assert_eq!(m.subject(&Command::SetRoute { entrance: sig("A"), exit: Exit::Node(node("E")) }), Some(west));
    assert_eq!(m.subject(&Command::CancelRoute { entrance: sig("C") }), Some(east));
    assert_eq!(m.subject(&Command::SetAutoWorking { entrance: sig("W1"), on: true }), Some(west));
    assert_eq!(m.subject(&Command::SwingPoints { points: node("P"), to: PointsPos::Reverse }), Some(east));
    assert_eq!(m.subject(&Command::SwingPoints { points: node("J1"), to: PointsPos::Reverse }), None);
    assert_eq!(m.subject(&Command::Interpose { berth: berth("BW"), headcode: "1A01".into() }), Some(west));
    assert_eq!(m.subject(&Command::CancelBerth { berth: berth("BE") }), Some(east));
    assert_eq!(m.subject(&Command::CancelBerth { berth: berth("BA") }), Some(west));
    assert_eq!(m.subject(&Command::CancelRoute { entrance: SignalId(99) }), None);
}

#[test]
fn signals_stand_at_the_nearer_end_of_their_segment() {
    let w = twobox();
    let net = &w.net;
    let at = |s: &str| net.nodes[signal_node(net, net.signal(s).unwrap()).idx()].name.clone();
    assert_eq!([at("W1"), at("A"), at("W2"), at("C"), at("D")], ["J0", "J1", "J0", "J2", "J3"]);
}

#[test]
fn the_fringe_runs_to_the_first_signal_beyond_the_boundary() {
    let w = twobox();
    let names = |set: BTreeSet<SectionId>| set.into_iter().map(|s| w.net.sections[s.idx()].name.clone()).collect::<Vec<_>>();
    // West sees the junction up to C and D; East sees back to W1/W2's node.
    assert_eq!(names(fringe(&w, area(&w, "West"))), ["TP"]);
    assert_eq!(names(fringe(&w, area(&w, "East"))), ["TW2"]);
}

#[test]
fn each_player_sees_their_area_and_its_fringe() {
    let w = twobox();
    let m = AreaMap::new(&w);
    let net = &w.net;
    let sec = |s: SectionId| net.sections[s.idx()].name.clone();
    let sig = |s: SignalId| net.signals[s.idx()].name.clone();
    let node = |n: NodeId| net.nodes[n.idx()].name.clone();
    let berth = |b: BerthId| net.berths[b.idx()].name.clone();
    let route = |r: RouteId| w.routes[r.idx()].name.clone();

    let west = Visibility::of_area(&w, &m, area(&w, "West"));
    assert_eq!(west.area, Some(area(&w, "West")));
    assert_eq!(names(&west.sections, sec), ["TW1", "TW2", "TP"]);
    assert_eq!(west.fringe.iter().copied().map(sec).collect::<Vec<_>>(), ["TP"]);
    assert_eq!(names(&west.signals, sig), ["W1", "A", "W2"]);
    assert_eq!(names(&west.points, node), ["P"]);
    assert_eq!(names(&west.berths, berth), ["BW", "BW1", "BA", "BW2"]);
    assert_eq!(names(&west.routes, route), ["W1-A", "A-E", "A-N", "W2-W"]);
    assert!(west.operable(area(&w, "West")) && !west.operable(area(&w, "East")));

    let east = Visibility::of_area(&w, &m, area(&w, "East"));
    assert_eq!(names(&east.sections, sec), ["TW2", "TP", "TE", "TN"]);
    assert_eq!(names(&east.signals, sig), ["A", "W2", "C", "D"]);
    assert_eq!(names(&east.berths, berth), ["BE", "BN", "BA", "BW2", "BC", "BD"]);
    assert_eq!(names(&east.routes, route), ["A-E", "A-N", "C-W2", "D-W2", "W2-W"]);

    let all = Visibility::spectator(&w, &m);
    assert_eq!(all.area, None);
    assert!(all.fringe.is_empty());
    assert_eq!((all.sections.len(), all.signals.len(), all.points.len(), all.berths.len(), all.routes.len()), (5, 5, 1, 8, 6));
    assert!(!all.operable(area(&w, "West")));
}

#[test]
fn liverpool_street_fringes() {
    let w = World::from_json(&liverpool_json()).unwrap();
    let m = AreaMap::new(&w);
    let sizes: Vec<(String, usize)> =
        w.net.areas.iter().enumerate().map(|(i, a)| (a.name.clone(), fringe(&w, AreaId::from_idx(i)).len())).collect();
    assert_eq!(
        sizes,
        [("Liverpool Street".to_string(), 16), ("Bethnal Green".to_string(), 37), ("Hackney & Bow".to_string(), 33)]
    );
    let seen_by = |s: &str| -> Vec<String> {
        let id = w.net.signal(s).unwrap();
        (0..w.net.areas.len())
            .filter(|&i| Visibility::of_area(&w, &m, AreaId::from_idx(i)).signals.contains(&id))
            .map(|i| w.net.areas[i].name.clone())
            .collect()
    };
    for s in ["61", "63", "65", "64", "66", "68"] {
        assert_eq!(seen_by(s), ["Liverpool Street", "Bethnal Green"], "{s}");
    }
    for s in ["91", "93", "95", "90", "92", "94"] {
        assert_eq!(seen_by(s), ["Bethnal Green", "Hackney & Bow"], "{s}");
    }
}
```

`crates/game/tests/layout.rs`:
```rust
//! The static layout each player is sent.

mod common;

use common::*;
use game::areas::{AreaMap, Visibility};
use game::layout::{build_layout, exit_name};
use protocol::{ExitName, Layout};
use signalbox_core::routes::Exit;

fn layout_for(area_name: Option<&str>) -> Layout {
    let w = twobox();
    let m = AreaMap::new(&w);
    let vis = match area_name {
        Some(a) => Visibility::of_area(&w, &m, area(&w, a)),
        None => Visibility::spectator(&w, &m),
    };
    build_layout(&w, &m, &vis, "alice")
}

#[test]
fn an_area_layout_marks_operable_and_fringe_elements() {
    let l = layout_for(Some("West"));
    assert_eq!((l.title.as_str(), l.you.as_str(), l.area.as_deref()), ("Two boxes", "alice", Some("West")));
    assert_eq!(l.areas, ["West", "East"]);
    let sections: Vec<(&str, &str, bool)> = l.sections.iter().map(|s| (s.name.as_str(), s.area.as_str(), s.fringe)).collect();
    assert_eq!(sections, [("TW1", "West", false), ("TW2", "West", false), ("TP", "East", true)]);
    let segments: Vec<&str> = l.segments.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(segments, ["w1", "w2", "pa", "pe", "pn"]);
    let signals: Vec<(&str, bool)> = l.signals.iter().map(|s| (s.name.as_str(), s.operable)).collect();
    assert_eq!(signals, [("W1", true), ("A", true), ("W2", true)]);
    let points: Vec<(&str, &str, bool)> = l.points.iter().map(|p| (p.name.as_str(), p.area.as_str(), p.operable)).collect();
    assert_eq!(points, [("P", "East", false)]);
    let berths: Vec<(&str, bool)> = l.berths.iter().map(|b| (b.name.as_str(), b.operable)).collect();
    assert_eq!(berths, [("BW", true), ("BW1", true), ("BA", true), ("BW2", true)]);
    assert_eq!(l.berths[0].boundary.as_deref(), Some("W"));
    assert_eq!(l.berths[2].signal.as_deref(), Some("A"));
    assert!(l.platforms.is_empty());
    let routes: Vec<(&str, bool)> = l.routes.iter().map(|r| (r.name.as_str(), r.operable)).collect();
    assert_eq!(routes, [("W1-A", true), ("A-E", true), ("A-N", true), ("W2-W", true)]);
    assert_eq!(l.routes[1].exit, ExitName::Node("E".into()));
    assert_eq!(l.routes[0].exit, ExitName::Signal("A".into()));
}

#[test]
fn the_neighbour_sees_the_boundary_signal_but_cannot_work_it() {
    let l = layout_for(Some("East"));
    let a = l.signals.iter().find(|s| s.name == "A").unwrap();
    assert!(!a.operable);
    assert_eq!(a.area, "West");
    let routes: Vec<(&str, bool)> = l.routes.iter().map(|r| (r.name.as_str(), r.operable)).collect();
    assert_eq!(routes, [("A-E", false), ("A-N", false), ("C-W2", true), ("D-W2", true), ("W2-W", false)]);
    assert_eq!(l.platforms.iter().map(|p| p.place.as_str()).collect::<Vec<_>>(), ["EST", "NST"]);
    assert!(l.points[0].operable);
}

#[test]
fn a_spectator_sees_everything_and_works_nothing() {
    let l = layout_for(None);
    assert_eq!(l.area, None);
    assert_eq!((l.sections.len(), l.segments.len(), l.signals.len(), l.berths.len(), l.routes.len()), (5, 7, 5, 8, 6));
    assert!(l.sections.iter().all(|s| !s.fringe));
    assert!(l.signals.iter().all(|s| !s.operable) && l.points.iter().all(|p| !p.operable));
    assert!(l.berths.iter().all(|b| !b.operable) && l.routes.iter().all(|r| !r.operable));
}

#[test]
fn exits_are_named() {
    let w = twobox();
    assert_eq!(exit_name(&w, Exit::Signal(w.net.signal("W2").unwrap())), ExitName::Signal("W2".into()));
    assert_eq!(exit_name(&w, Exit::Node(w.net.node("N").unwrap())), ExitName::Node("N".into()));
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-game --test areas --test layout`
Expected: compile errors — `game::areas` and `game::layout` are empty modules / missing files.

- [ ] **Step 4: Implement `areas.rs`**

`crates/game/src/areas.rs`:
```rust
//! Which area each command subject belongs to (spec §3.4), and what each
//! player sees: their area plus the fringe of their neighbours (§4.5).

use std::collections::BTreeSet;

use signalbox_core::events::Command;
use signalbox_core::ids::*;
use signalbox_core::network::{Network, NodeKind};
use signalbox_core::world::World;

/// The areas of command subjects, resolved once per world.
#[derive(Clone, Debug)]
pub struct AreaMap {
    /// Each signal's area (routes belong to their entrance signal's).
    pub signal: Vec<AreaId>,
    /// The section each berth hangs on: its signal's section, or for a
    /// boundary berth the section at the boundary.
    pub berth_section: Vec<SectionId>,
    /// Each berth's area: its signal's, or its boundary section's.
    pub berth: Vec<AreaId>,
    /// Each node's points area (the points section's), `None` for other nodes.
    pub points: Vec<Option<AreaId>>,
}

/// The section a signal's segment belongs to.
pub fn signal_section(net: &Network, s: SignalId) -> SectionId {
    net.segments[net.signals[s.idx()].at.segment.idx()].section
}

/// The node a signal stands at: the nearer end of its segment.
pub fn signal_node(net: &Network, s: SignalId) -> NodeId {
    let sig = &net.signals[s.idx()];
    let seg = &net.segments[sig.at.segment.idx()];
    if sig.at.offset_m <= seg.length_m / 2.0 { seg.a } else { seg.b }
}

impl AreaMap {
    pub fn new(w: &World) -> AreaMap {
        let net = &w.net;
        let signal: Vec<AreaId> = net.signals.iter().map(|s| s.area).collect();
        let mut berth_section = Vec::with_capacity(net.berths.len());
        let mut berth = Vec::with_capacity(net.berths.len());
        for b in &net.berths {
            let (sec, area) = match (b.signal, b.boundary) {
                (Some(s), _) => (signal_section(net, s), signal[s.idx()]),
                (None, Some(n)) => {
                    let sec = net.segments[net.nodes[n.idx()].segments[0].idx()].section;
                    (sec, net.sections[sec.idx()].area)
                }
                (None, None) => unreachable!("the loader gives every berth a signal or a boundary"),
            };
            berth_section.push(sec);
            berth.push(area);
        }
        let points = (0..net.nodes.len())
            .map(|n| net.points_section(NodeId::from_idx(n)).map(|sec| net.sections[sec.idx()].area))
            .collect();
        AreaMap { signal, berth_section, berth, points }
    }

    /// The area of the thing a command acts on.
    pub fn subject(&self, cmd: &Command) -> Option<AreaId> {
        match cmd {
            Command::SetRoute { entrance, .. }
            | Command::CancelRoute { entrance }
            | Command::SetAutoWorking { entrance, .. } => self.signal.get(entrance.idx()).copied(),
            Command::SwingPoints { points, .. } => self.points.get(points.idx()).copied().flatten(),
            Command::Interpose { berth, .. } | Command::CancelBerth { berth } => self.berth.get(berth.idx()).copied(),
        }
    }
}

/// What one player sees. Lists are in index order.
#[derive(Clone, Debug, PartialEq)]
pub struct Visibility {
    /// The area they hold; `None` for a spectator.
    pub area: Option<AreaId>,
    pub sections: Vec<SectionId>,
    /// Visible sections outside `area` (empty for a spectator).
    pub fringe: BTreeSet<SectionId>,
    pub signals: Vec<SignalId>,
    pub points: Vec<NodeId>,
    pub berths: Vec<BerthId>,
    pub routes: Vec<RouteId>,
}

impl Visibility {
    pub fn spectator(w: &World, map: &AreaMap) -> Visibility {
        Self::from_sections(w, map, None, vec![true; w.net.sections.len()], BTreeSet::new())
    }

    pub fn of_area(w: &World, map: &AreaMap, a: AreaId) -> Visibility {
        let fringe = fringe(w, a);
        let visible = (0..w.net.sections.len())
            .map(|i| w.net.sections[i].area == a || fringe.contains(&SectionId::from_idx(i)))
            .collect();
        Self::from_sections(w, map, Some(a), visible, fringe)
    }

    /// Whether this player may work things in area `a`.
    pub fn operable(&self, a: AreaId) -> bool {
        self.area == Some(a)
    }

    fn from_sections(w: &World, map: &AreaMap, area: Option<AreaId>, visible: Vec<bool>, fringe: BTreeSet<SectionId>) -> Visibility {
        let net = &w.net;
        let sections = (0..net.sections.len()).filter(|&i| visible[i]).map(SectionId::from_idx).collect();
        let signals = (0..net.signals.len())
            .map(SignalId::from_idx)
            .filter(|&s| visible[signal_section(net, s).idx()])
            .collect();
        let points = (0..net.nodes.len())
            .map(NodeId::from_idx)
            .filter(|&n| net.points_section(n).is_some_and(|sec| visible[sec.idx()]))
            .collect();
        let berths = (0..net.berths.len())
            .map(BerthId::from_idx)
            .filter(|&b| visible[map.berth_section[b.idx()].idx()])
            .collect();
        let routes = (0..w.routes.len())
            .map(RouteId::from_idx)
            .filter(|&r| visible[signal_section(net, w.routes[r.idx()].entrance).idx()])
            .collect();
        Visibility { area, sections, fringe, signals, points, berths, routes }
    }
}

/// Segments a train arriving at node `n` on segment `came` can run on to:
/// through points only from the toe to a leg or from a leg to the toe.
fn onward(net: &Network, n: NodeId, came: SegmentId) -> Vec<SegmentId> {
    let node = &net.nodes[n.idx()];
    match node.kind {
        NodeKind::Points { toe, normal, reverse, .. } => {
            if came == toe {
                vec![normal, reverse]
            } else if came == normal || came == reverse {
                vec![toe]
            } else {
                vec![]
            }
        }
        _ => node.segments.iter().copied().filter(|&s| s != came).collect(),
    }
}

/// Sections of other areas a player of `a` sees (amendment 6): walk the
/// track away from `a` as a train could, and stop after a segment whose far
/// node holds any signal.
pub fn fringe(w: &World, a: AreaId) -> BTreeSet<SectionId> {
    let net = &w.net;
    let signal_nodes: BTreeSet<NodeId> =
        (0..net.signals.len()).map(|s| signal_node(net, SignalId::from_idx(s))).collect();
    let area_of = |g: SegmentId| net.sections[net.segments[g.idx()].section.idx()].area;
    // (segment entered, node it was entered from)
    let mut stack: Vec<(SegmentId, NodeId)> = Vec::new();
    for i in 0..net.segments.len() {
        let g = SegmentId::from_idx(i);
        if area_of(g) != a {
            continue;
        }
        for n in [net.segments[i].a, net.segments[i].b] {
            for h in onward(net, n, g) {
                if area_of(h) != a {
                    stack.push((h, n));
                }
            }
        }
    }
    let mut seen: BTreeSet<(SegmentId, NodeId)> = BTreeSet::new();
    let mut out = BTreeSet::new();
    while let Some((h, n)) = stack.pop() {
        if !seen.insert((h, n)) {
            continue;
        }
        let seg = &net.segments[h.idx()];
        out.insert(seg.section);
        let far = if seg.a == n { seg.b } else { seg.a };
        if signal_nodes.contains(&far) {
            continue;
        }
        for k in onward(net, far, h) {
            if area_of(k) != a {
                stack.push((k, far));
            }
        }
    }
    out
}
```

- [ ] **Step 5: Implement `layout.rs`**

`crates/game/src/layout.rs`:
```rust
//! The static layout a player is sent (spec §4.1).

use std::collections::BTreeSet;

use protocol::{BerthInfo, ExitName, Layout, PlatformInfo, PointsInfo, RouteInfo, SectionInfo, SegmentInfo, SignalInfo};
use signalbox_core::ids::*;
use signalbox_core::routes::Exit;
use signalbox_core::world::World;

use crate::areas::{AreaMap, Visibility};

pub fn exit_name(w: &World, e: Exit) -> ExitName {
    match e {
        Exit::Signal(s) => ExitName::Signal(w.net.signals[s.idx()].name.clone()),
        Exit::Node(n) => ExitName::Node(w.net.nodes[n.idx()].name.clone()),
    }
}

pub fn build_layout(w: &World, map: &AreaMap, vis: &Visibility, you: &str) -> Layout {
    let net = &w.net;
    let area_name = |a: AreaId| net.areas[a.idx()].name.clone();
    let section_name = |s: SectionId| net.sections[s.idx()].name.clone();
    let visible: BTreeSet<SectionId> = vis.sections.iter().copied().collect();
    Layout {
        title: w.title.clone(),
        you: you.to_string(),
        area: vis.area.map(area_name),
        areas: net.areas.iter().map(|a| a.name.clone()).collect(),
        sections: vis
            .sections
            .iter()
            .map(|&s| SectionInfo {
                name: section_name(s),
                area: area_name(net.sections[s.idx()].area),
                fringe: vis.fringe.contains(&s),
            })
            .collect(),
        segments: net
            .segments
            .iter()
            .filter(|g| visible.contains(&g.section))
            .map(|g| SegmentInfo {
                name: g.name.clone(),
                from: net.nodes[g.a.idx()].name.clone(),
                to: net.nodes[g.b.idx()].name.clone(),
                length_m: g.length_m,
                section: section_name(g.section),
            })
            .collect(),
        signals: vis
            .signals
            .iter()
            .map(|&s| {
                let sig = &net.signals[s.idx()];
                SignalInfo {
                    name: sig.name.clone(),
                    area: area_name(map.signal[s.idx()]),
                    segment: net.segments[sig.at.segment.idx()].name.clone(),
                    offset_m: sig.at.offset_m,
                    direction: sig.at.dir,
                    aspects: sig.aspects,
                    operable: vis.operable(map.signal[s.idx()]),
                }
            })
            .collect(),
        points: vis
            .points
            .iter()
            .map(|&n| {
                let sec = net.points_section(n).expect("visible points are points");
                let a = net.sections[sec.idx()].area;
                PointsInfo {
                    name: net.nodes[n.idx()].name.clone(),
                    section: section_name(sec),
                    area: area_name(a),
                    operable: vis.operable(a),
                }
            })
            .collect(),
        berths: vis
            .berths
            .iter()
            .map(|&b| {
                let berth = &net.berths[b.idx()];
                BerthInfo {
                    name: berth.name.clone(),
                    signal: berth.signal.map(|s| net.signals[s.idx()].name.clone()),
                    boundary: berth.boundary.map(|n| net.nodes[n.idx()].name.clone()),
                    area: area_name(map.berth[b.idx()]),
                    operable: vis.operable(map.berth[b.idx()]),
                }
            })
            .collect(),
        platforms: net
            .platforms
            .iter()
            .filter(|p| visible.contains(&net.segments[p.segment.idx()].section))
            .map(|p| PlatformInfo {
                place: p.place.clone(),
                platform: p.platform.clone(),
                segment: net.segments[p.segment.idx()].name.clone(),
                from_m: p.from_m,
                to_m: p.to_m,
            })
            .collect(),
        routes: vis
            .routes
            .iter()
            .map(|&r| {
                let def = &w.routes[r.idx()];
                RouteInfo {
                    name: def.name.clone(),
                    entrance: net.signals[def.entrance.idx()].name.clone(),
                    exit: exit_name(w, def.exit),
                    automatic: def.automatic,
                    operable: vis.operable(map.signal[def.entrance.idx()]),
                }
            })
            .collect(),
    }
}
```

- [ ] **Step 6: Run the tests**

Run: `scripts/cargo test -p signalbox-game --test areas --test layout`
Expected: PASS. If `liverpool_street_fringes` shows other sizes than 16 / 37 / 33, stop and report (the numbers come from a prototype of the same rule); do not change the expected numbers.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/game
git commit -m "feat(game): area map, per-player visibility with fringe, layouts

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Views built from sim state

**Files:**
- Create: `crates/game/src/view.rs`
- Modify: `crates/game/src/lib.rs` (add `pub mod view;`)
- Test: `crates/game/tests/view.rs`

**Interfaces:**
- Consumes: `Visibility` (Task 4); `protocol::{View, VoteView, RouteView, RouteState, PointsView, SectionView, Held}` (Task 3); `Sim::{world, interlocking, points, occupancy, describer, scores, aspect}`, `interlocking::{Owner, RouteState}`, `points::PointsState`.
- Produces:
  - `game::view::Shared { sim_time: f64, speed: u8, paused: bool, vote: Option<VoteView>, holders: BTreeMap<String, String> }` (`Clone, Debug, PartialEq`) — the parts every player's view has in common, built once per flush.
  - `game::view::build_view(sim: &Sim, vis: &Visibility, shared: &Shared, seq: u64) -> View` — pure function of sim state (spec §4.3: views come from state, never from events). Route state: idle → absent; `cancel.is_some()` → `cancelling`; else `setting`/`locked`. Points: `position` is the detected position or the one they are moving to; `locked` = their section is held. Sections: `held` from the interlocking owner. Berths: only filled ones. `score`: the area's `Scores::by_area` entry, `None` for spectators.

- [ ] **Step 1: Write the failing tests**

`crates/game/tests/view.rs`:
```rust
//! Views are built from the sim's state for each player's visible set.

mod common;

use std::collections::BTreeMap;

use common::*;
use game::areas::{AreaMap, Visibility};
use game::view::{Shared, build_view};
use protocol::{Aspect, Held, PointsPos, PointsView, RouteState, RouteView, SectionView, View};
use signalbox_core::events::Command;
use signalbox_core::routes::Exit;
use signalbox_core::sim::Sim;

struct Rig {
    sim: Sim,
    west: Visibility,
    east: Visibility,
    all: Visibility,
}

fn rig() -> Rig {
    let w = twobox();
    let m = AreaMap::new(&w);
    let west = Visibility::of_area(&w, &m, area(&w, "West"));
    let east = Visibility::of_area(&w, &m, area(&w, "East"));
    let all = Visibility::spectator(&w, &m);
    Rig { sim: Sim::new(w, 1), west, east, all }
}

impl Rig {
    fn view(&self, vis: &Visibility) -> View {
        let shared = Shared {
            sim_time: self.sim.now_s(),
            speed: 2,
            paused: false,
            vote: None,
            holders: map(&[("East", "robot".to_string()), ("West", "alice".to_string())]),
        };
        build_view(&self.sim, vis, &shared, 4)
    }
}

#[test]
fn a_fresh_railway() {
    let r = rig();
    let v = r.view(&r.west);
    assert_eq!((v.seq, v.sim_time, v.speed, v.paused, v.vote.clone()), (4, 7.0 * 3600.0, 2, false, None));
    assert_eq!(v.holders["West"], "alice");
    assert_eq!(v.score, Some(0));
    assert_eq!(v.signals, map(&[("A", Aspect::Red), ("W1", Aspect::Red), ("W2", Aspect::Red)]));
    assert_eq!(v.points, map(&[("P", PointsView { position: PointsPos::Normal, moving: false, locked: false })]));
    let free = SectionView { occupied: false, held: Held::Free };
    assert_eq!(v.sections, map(&[("TP", free), ("TW1", free), ("TW2", free)]));
    assert!(v.routes.is_empty() && v.berths.is_empty());
    let all = r.view(&r.all);
    assert_eq!(all.score, None);
    assert_eq!((all.signals.len(), all.sections.len()), (5, 5));
}

#[test]
fn routes_points_and_holding_show_in_every_view_that_sees_them() {
    let mut r = rig();
    let w = r.sim.world().clone();
    let a = w.net.signal("A").unwrap();
    r.sim.submit(Command::SetRoute { entrance: a, exit: Exit::Node(w.net.node("N").unwrap()) });
    r.sim.step();
    let v = r.view(&r.west);
    assert_eq!(v.routes, map(&[("A-N", RouteView { state: RouteState::Setting, auto_working: false })]));
    assert_eq!(v.points["P"], PointsView { position: PointsPos::Reverse, moving: true, locked: true });
    assert_eq!(v.sections["TP"], SectionView { occupied: false, held: Held::Path });
    let e = r.view(&r.east);
    assert_eq!(e.routes, v.routes);
    assert_eq!(e.sections["TN"].held, Held::Path);

    r.sim.run_for(6.0);
    let v = r.view(&r.west);
    assert_eq!(v.routes["A-N"].state, RouteState::Locked);
    assert_eq!(v.points["P"], PointsView { position: PointsPos::Reverse, moving: false, locked: true });
    assert_eq!(v.signals["A"], Aspect::Green);

    // A cancelled route still under approach locking shows as cancelling.
    let mut st = r.sim.snapshot();
    let an = w.route_by_name("A-N").unwrap();
    st.il.routes[an.idx()].cancel = Some(120.0);
    r.sim = Sim::restore(w, st).unwrap();
    assert_eq!(r.view(&r.east).routes["A-N"].state, RouteState::Cancelling);
}

#[test]
fn occupancy_and_berths() {
    let mut r = rig();
    r.sim.run_for(1.0);
    let v = r.view(&r.west);
    assert!(v.sections["TW1"].occupied);
    assert_eq!(v.berths, map(&[("BW1", "1E01".to_string())]));
    assert!(r.view(&r.east).berths.is_empty(), "BW1 is not visible from East");
    assert_eq!(r.view(&r.all).berths, v.berths);
    assert_eq!(r.view(&r.all).sections["TW1"], SectionView { occupied: true, held: Held::Free });
}

#[test]
fn views_are_a_function_of_state() {
    let mut r = rig();
    r.sim.run_for(30.0);
    let restored = Sim::restore(r.sim.world().clone(), r.sim.snapshot()).unwrap();
    let a = r.view(&r.all);
    r.sim = restored;
    assert_eq!(r.view(&r.all), a);
    let empty: BTreeMap<String, String> = BTreeMap::new();
    assert_ne!(a.berths, empty);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-game --test view`
Expected: compile error — unresolved `game::view`.

- [ ] **Step 3: Implement**

Add `pub mod view;` to `crates/game/src/lib.rs` (after `pub mod layout;`).

`crates/game/src/view.rs`:
```rust
//! A player's dynamic view, built from sim state (spec §4.2). Never from
//! events, so a client that applies every delta can never drift.

use std::collections::BTreeMap;

use protocol::{Held, PointsPos, PointsView, RouteState, RouteView, SectionView, View, VoteView};
use signalbox_core::interlocking::{Owner, RouteState as IlState};
use signalbox_core::points::PointsState;
use signalbox_core::sim::Sim;

use crate::areas::Visibility;

/// The parts of a view every player shares, computed once per flush.
#[derive(Clone, Debug, PartialEq)]
pub struct Shared {
    pub sim_time: f64,
    pub speed: u8,
    pub paused: bool,
    pub vote: Option<VoteView>,
    /// Area → holder, or `"robot"`.
    pub holders: BTreeMap<String, String>,
}

pub fn build_view(sim: &Sim, vis: &Visibility, shared: &Shared, seq: u64) -> View {
    let w = sim.world();
    let net = &w.net;
    let il = sim.interlocking();
    let signals = vis.signals.iter().map(|&s| (net.signals[s.idx()].name.clone(), sim.aspect(s))).collect();
    let routes = vis
        .routes
        .iter()
        .filter_map(|&r| {
            let st = &il.routes[r.idx()];
            let state = match (st.state, st.cancel) {
                (IlState::Idle, _) => return None,
                (_, Some(_)) => RouteState::Cancelling,
                (IlState::Setting, None) => RouteState::Setting,
                (IlState::Locked, None) => RouteState::Locked,
            };
            Some((w.routes[r.idx()].name.clone(), RouteView { state, auto_working: st.auto_working }))
        })
        .collect();
    let points = vis
        .points
        .iter()
        .map(|&n| {
            let sec = net.points_section(n).expect("visible points are points");
            let (position, moving) = match sim.points().state(n) {
                Some(PointsState::Set(p)) => (p, false),
                Some(PointsState::Moving { to, .. }) => (to, true),
                None => (PointsPos::Normal, false),
            };
            let locked = il.owner[sec.idx()].is_some();
            (net.nodes[n.idx()].name.clone(), PointsView { position, moving, locked })
        })
        .collect();
    let sections = vis
        .sections
        .iter()
        .map(|&s| {
            let held = match il.owner[s.idx()] {
                None => Held::Free,
                Some(Owner::Path(_)) => Held::Path,
                Some(Owner::Overlap(_)) => Held::Overlap,
            };
            (net.sections[s.idx()].name.clone(), SectionView { occupied: sim.occupancy().occupied(s), held })
        })
        .collect();
    let berths = vis
        .berths
        .iter()
        .filter_map(|&b| sim.describer().get(b).map(|h| (net.berths[b.idx()].name.clone(), h.to_string())))
        .collect();
    View {
        seq,
        sim_time: shared.sim_time,
        speed: shared.speed,
        paused: shared.paused,
        vote: shared.vote.clone(),
        holders: shared.holders.clone(),
        score: vis.area.map(|a| sim.scores().by_area[a.idx()]),
        signals,
        routes,
        points,
        sections,
        berths,
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `scripts/cargo test -p signalbox-game --test view`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game/src/lib.rs crates/game/src/view.rs crates/game/tests/view.rs
git commit -m "feat(game): per-player views built from sim state

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Names ⇄ ids, and notices from events (including handover)

**Files:**
- Create: `crates/game/src/names.rs`, `crates/game/src/notices.rs`
- Modify: `crates/game/src/lib.rs` (add `pub mod names;` and `pub mod notices;`)
- Test: `crates/game/tests/names.rs`, `crates/game/tests/notices.rs`

**Interfaces:**
- Consumes: `AreaMap` (Task 4), `exit_name` (Task 4), `protocol::{PlayerCommand, ExitName, Notice}` (Task 3); `Network::{signal, node, berth}` name lookups; `Event` from core.
- Produces:
  - `game::names::resolve(w: &World, cmd: &PlayerCommand) -> Option<Command>` — `None` when any name is unknown (a signal name where a berth is expected is unknown too).
  - `game::names::to_player_command(w: &World, cmd: &Command) -> PlayerCommand` — the inverse, for valid ids (robot commands, accepted commands, sim rejections).
  - `game::names::valid_headcode(h: &str) -> bool` — 1 to 10 ASCII letters or digits.
  - `game::notices::LATE_NOTICE_S: i64 = 60`.
  - `game::notices::area_notices(sim: &Sim, map: &AreaMap, before: &[Option<String>], events: &[Event]) -> Vec<(AreaId, Notice)>` — for one tick's events, in event order: `spad` (signal's area), `collision` (section's area), `late` (`TrainArrived`/`TrainPassed` with `late_s >= 60`, platform's area), `wrong_platform` (platform's area), `handover` (a `BerthChanged` filling a berth in area A with H while another `BerthChanged` of the same tick empties a berth of area B ≠ A that held H in `before`, the describer's berths at the start of the tick). Trains are named by headcode, or `train <id>` if the train is gone.

- [ ] **Step 1: Write the failing tests**

`crates/game/tests/names.rs`:
```rust
//! Commands travel by name; names are opaque.

mod common;

use common::*;
use game::areas::AreaMap;
use game::names::{resolve, to_player_command, valid_headcode};
use protocol::{ExitName, PlayerCommand, PointsPos};
use signalbox_core::events::Command;
use signalbox_core::routes::Exit;
use signalbox_core::world::World;

fn s(x: &str) -> String {
    x.to_string()
}

fn every_kind() -> Vec<PlayerCommand> {
    vec![
        PlayerCommand::SetRoute { entrance: s("W1"), exit: ExitName::Signal(s("A")) },
        PlayerCommand::SetRoute { entrance: s("A"), exit: ExitName::Node(s("E")) },
        PlayerCommand::CancelRoute { entrance: s("C") },
        PlayerCommand::SetAutoWorking { entrance: s("C"), on: true },
        PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse },
        PlayerCommand::Interpose { berth: s("BA"), headcode: s("1A01") },
        PlayerCommand::CancelBerth { berth: s("BC") },
    ]
}

#[test]
fn names_resolve_to_ids_and_back() {
    let w = twobox();
    let net = &w.net;
    assert_eq!(
        resolve(&w, &every_kind()[1]),
        Some(Command::SetRoute { entrance: net.signal("A").unwrap(), exit: Exit::Node(net.node("E").unwrap()) })
    );
    for pc in every_kind() {
        let c = resolve(&w, &pc).unwrap_or_else(|| panic!("{pc:?}"));
        assert_eq!(to_player_command(&w, &c), pc);
    }
}

#[test]
fn unknown_names_resolve_to_nothing() {
    let w = twobox();
    for pc in [
        PlayerCommand::SetRoute { entrance: s("Z9"), exit: ExitName::Signal(s("A")) },
        PlayerCommand::SetRoute { entrance: s("W1"), exit: ExitName::Signal(s("Z9")) },
        PlayerCommand::SetRoute { entrance: s("A"), exit: ExitName::Node(s("A")) },
        PlayerCommand::CancelRoute { entrance: s("BA") },
        PlayerCommand::SetAutoWorking { entrance: s(""), on: false },
        PlayerCommand::SwingPoints { points: s("TP"), to: PointsPos::Normal },
        PlayerCommand::Interpose { berth: s("A"), headcode: s("1A01") },
        PlayerCommand::CancelBerth { berth: s("bc") },
    ] {
        assert_eq!(resolve(&w, &pc), None, "{pc:?}");
    }
}

#[test]
fn names_with_commas_and_hashes_resolve() {
    let text = twobox_json()
        .replace("\"C\"", "\"39,1V1\"")
        .replace("\"BC\"", "\"512#113\"")
        .replace("\"East\"", "\"Hackney & Bow\"");
    let w = World::from_json(&text).unwrap();
    let pc = PlayerCommand::SetRoute { entrance: s("39,1V1"), exit: ExitName::Signal(s("W2")) };
    let c = resolve(&w, &pc).unwrap();
    assert_eq!(to_player_command(&w, &c), pc);
    let m = AreaMap::new(&w);
    assert_eq!(w.net.areas[m.subject(&c).unwrap().idx()].name, "Hackney & Bow");
    let berth = PlayerCommand::CancelBerth { berth: s("512#113") };
    assert_eq!(to_player_command(&w, &resolve(&w, &berth).unwrap()), berth);
    assert!(w.route_by_name("39,1V1-W2").is_some());
}

#[test]
fn headcodes_are_short_and_plain() {
    for ok in ["1A01", "2W03", "X", "ABCDEFGHIJ"] {
        assert!(valid_headcode(ok), "{ok}");
    }
    for bad in ["", "1A 01", "ABCDEFGHIJK", "1A-01", "<b>", "é1"] {
        assert!(!valid_headcode(bad), "{bad}");
    }
}
```

`crates/game/tests/notices.rs`:
```rust
//! Notices derived from one tick's events.

mod common;

use common::*;
use game::areas::AreaMap;
use game::notices::area_notices;
use protocol::Notice;
use signalbox_core::events::Event;
use signalbox_core::ids::*;
use signalbox_core::sim::Sim;

fn setup() -> (Sim, AreaMap) {
    let w = twobox();
    let m = AreaMap::new(&w);
    (Sim::new(w, 1), m)
}

fn berth(sim: &Sim, name: &str) -> BerthId {
    sim.world().net.berth(name).unwrap()
}

/// Describer contents at the start of a tick.
fn berths_with(sim: &Sim, filled: &[(&str, &str)]) -> Vec<Option<String>> {
    let mut v = vec![None; sim.world().net.berths.len()];
    for (b, h) in filled {
        v[berth(sim, b).idx()] = Some(h.to_string());
    }
    v
}

fn changed(sim: &Sim, name: &str, headcode: Option<&str>) -> Event {
    Event::BerthChanged { berth: berth(sim, name), headcode: headcode.map(str::to_string) }
}

#[test]
fn a_step_across_the_boundary_is_a_handover() {
    let (sim, m) = setup();
    let before = berths_with(&sim, &[("BC", "2W03")]);
    let ev = vec![changed(&sim, "BC", None), changed(&sim, "BW2", Some("2W03"))];
    let west = area(sim.world(), "West");
    assert_eq!(
        area_notices(&sim, &m, &before, &ev),
        vec![(west, Notice::Handover { headcode: "2W03".into(), from_area: "East".into() })]
    );
}

#[test]
fn steps_inside_an_area_and_interposing_are_not_handovers() {
    let (sim, m) = setup();
    let inside = vec![changed(&sim, "BW1", None), changed(&sim, "BA", Some("1E01"))];
    assert!(area_notices(&sim, &m, &berths_with(&sim, &[("BW1", "1E01")]), &inside).is_empty());
    let interposed = vec![changed(&sim, "BW2", Some("2W03"))];
    assert!(area_notices(&sim, &m, &berths_with(&sim, &[]), &interposed).is_empty());
    let other_train = vec![changed(&sim, "BC", None), changed(&sim, "BW2", Some("2W03"))];
    assert!(area_notices(&sim, &m, &berths_with(&sim, &[("BC", "1A01")]), &other_train).is_empty());
}

#[test]
fn incidents_go_to_the_area_they_happen_in() {
    let (sim, m) = setup();
    let net = &sim.world().net;
    let (west, east) = (area(sim.world(), "West"), area(sim.world(), "East"));
    let ev = vec![
        Event::SignalPassedAtDanger { signal: net.signal("A").unwrap(), train: TrainId(9) },
        Event::Collision { train: TrainId(9), other: TrainId(8), section: net.section("TP").unwrap() },
        Event::TrainArrived { train: TrainId(9), platform: PlatformId(0), late_s: 59 },
        Event::TrainArrived { train: TrainId(9), platform: PlatformId(0), late_s: 60 },
        Event::TrainPassed { train: TrainId(9), platform: PlatformId(1), late_s: 300 },
        Event::WrongPlatform { train: TrainId(9), platform: PlatformId(0), expected: "2".into() },
    ];
    let train = || "train 9".to_string();
    assert_eq!(
        area_notices(&sim, &m, &berths_with(&sim, &[]), &ev),
        vec![
            (west, Notice::Spad { signal: "A".into(), train: train() }),
            (east, Notice::Collision { section: "TP".into() }),
            (east, Notice::Late { train: train(), place: "EST".into(), platform: "1".into(), late_s: 60 }),
            (east, Notice::Late { train: train(), place: "NST".into(), platform: "1".into(), late_s: 300 }),
            (east, Notice::WrongPlatform { train: train(), place: "EST".into(), platform: "1".into(), expected: "2".into() }),
        ]
    );
}

#[test]
fn trains_are_named_by_headcode() {
    let (mut sim, m) = setup();
    sim.run_for(1.0);
    let w1 = sim.world().net.signal("W1").unwrap();
    let west = area(sim.world(), "West");
    let ev = vec![Event::SignalPassedAtDanger { signal: w1, train: TrainId(0) }];
    assert_eq!(
        area_notices(&sim, &m, &berths_with(&sim, &[]), &ev),
        vec![(west, Notice::Spad { signal: "W1".into(), train: "1E01".into() })]
    );
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-game --test names --test notices`
Expected: compile errors — unresolved `game::names`, `game::notices`.

- [ ] **Step 3: Implement**

Add to `crates/game/src/lib.rs` (after `pub mod layout;`):
```rust
pub mod names;
pub mod notices;
```

`crates/game/src/names.rs`:
```rust
//! Player commands carry names; the sim wants ids.

use protocol::{ExitName, PlayerCommand};
use signalbox_core::events::Command;
use signalbox_core::routes::Exit;
use signalbox_core::world::World;

use crate::layout::exit_name;

/// The core command a named command means; `None` if a name is unknown.
pub fn resolve(w: &World, cmd: &PlayerCommand) -> Option<Command> {
    let net = &w.net;
    Some(match cmd {
        PlayerCommand::SetRoute { entrance, exit } => Command::SetRoute {
            entrance: net.signal(entrance)?,
            exit: match exit {
                ExitName::Signal(s) => Exit::Signal(net.signal(s)?),
                ExitName::Node(n) => Exit::Node(net.node(n)?),
            },
        },
        PlayerCommand::CancelRoute { entrance } => Command::CancelRoute { entrance: net.signal(entrance)? },
        PlayerCommand::SetAutoWorking { entrance, on } => {
            Command::SetAutoWorking { entrance: net.signal(entrance)?, on: *on }
        }
        PlayerCommand::SwingPoints { points, to } => Command::SwingPoints { points: net.node(points)?, to: *to },
        PlayerCommand::Interpose { berth, headcode } => {
            Command::Interpose { berth: net.berth(berth)?, headcode: headcode.clone() }
        }
        PlayerCommand::CancelBerth { berth } => Command::CancelBerth { berth: net.berth(berth)? },
    })
}

/// The named form of a command with valid ids.
pub fn to_player_command(w: &World, cmd: &Command) -> PlayerCommand {
    let net = &w.net;
    let signal = |s: signalbox_core::ids::SignalId| net.signals[s.idx()].name.clone();
    let berth = |b: signalbox_core::ids::BerthId| net.berths[b.idx()].name.clone();
    match cmd {
        Command::SetRoute { entrance, exit } => {
            PlayerCommand::SetRoute { entrance: signal(*entrance), exit: exit_name(w, *exit) }
        }
        Command::CancelRoute { entrance } => PlayerCommand::CancelRoute { entrance: signal(*entrance) },
        Command::SetAutoWorking { entrance, on } => PlayerCommand::SetAutoWorking { entrance: signal(*entrance), on: *on },
        Command::SwingPoints { points, to } => {
            PlayerCommand::SwingPoints { points: net.nodes[points.idx()].name.clone(), to: *to }
        }
        Command::Interpose { berth: b, headcode } => PlayerCommand::Interpose { berth: berth(*b), headcode: headcode.clone() },
        Command::CancelBerth { berth: b } => PlayerCommand::CancelBerth { berth: berth(*b) },
    }
}

/// 1 to 10 ASCII letters or digits.
pub fn valid_headcode(h: &str) -> bool {
    (1..=10).contains(&h.len()) && h.bytes().all(|b| b.is_ascii_alphanumeric())
}
```

`crates/game/src/notices.rs`:
```rust
//! One-off notices derived from a tick's events (spec §4.4), each tagged
//! with the area it concerns; the game sends them to that area's holder.

use protocol::Notice;
use signalbox_core::events::Event;
use signalbox_core::ids::*;
use signalbox_core::sim::Sim;

use crate::areas::AreaMap;

/// Trains at least this late are reported.
pub const LATE_NOTICE_S: i64 = 60;

/// Notices for one tick's `events`. `before` is the describer's berths at the
/// start of that tick (to see which headcode an emptied berth held).
pub fn area_notices(sim: &Sim, map: &AreaMap, before: &[Option<String>], events: &[Event]) -> Vec<(AreaId, Notice)> {
    let net = &sim.world().net;
    let train_name = |t: TrainId| {
        sim.trains().iter().find(|x| x.id == t).map_or_else(|| format!("train {}", t.0), |x| x.headcode.clone())
    };
    let platform = |p: PlatformId| {
        let pl = &net.platforms[p.idx()];
        let area = net.sections[net.segments[pl.segment.idx()].section.idx()].area;
        (area, pl.place.clone(), pl.platform.clone())
    };
    let mut out = Vec::new();
    for e in events {
        match e {
            Event::SignalPassedAtDanger { signal, train } => out.push((
                map.signal[signal.idx()],
                Notice::Spad { signal: net.signals[signal.idx()].name.clone(), train: train_name(*train) },
            )),
            Event::Collision { section, .. } => out.push((
                net.sections[section.idx()].area,
                Notice::Collision { section: net.sections[section.idx()].name.clone() },
            )),
            Event::TrainArrived { train, platform: p, late_s } | Event::TrainPassed { train, platform: p, late_s }
                if *late_s >= LATE_NOTICE_S =>
            {
                let (area, place, platform) = platform(*p);
                out.push((area, Notice::Late { train: train_name(*train), place, platform, late_s: *late_s }));
            }
            Event::WrongPlatform { train, platform: p, expected } => {
                let (area, place, platform) = platform(*p);
                out.push((
                    area,
                    Notice::WrongPlatform { train: train_name(*train), place, platform, expected: expected.clone() },
                ));
            }
            Event::BerthChanged { berth, headcode: Some(h) } => {
                let to = map.berth[berth.idx()];
                let from = events.iter().find_map(|x| match x {
                    Event::BerthChanged { berth: b, headcode: None }
                        if before.get(b.idx()).and_then(|o| o.as_deref()) == Some(h.as_str())
                            && map.berth[b.idx()] != to =>
                    {
                        Some(map.berth[b.idx()])
                    }
                    _ => None,
                });
                if let Some(from) = from {
                    out.push((to, Notice::Handover { headcode: h.clone(), from_area: net.areas[from.idx()].name.clone() }));
                }
            }
            _ => {}
        }
    }
    out
}
```

- [ ] **Step 4: Run the tests**

Run: `scripts/cargo test -p signalbox-game --test names --test notices`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game/src/lib.rs crates/game/src/names.rs crates/game/src/notices.rs \
  crates/game/tests/names.rs crates/game/tests/notices.rs
git commit -m "feat(game): named commands, and notices with boundary handovers

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Game clock and votes

**Files:**
- Create: `crates/game/src/clock.rs`
- Modify: `crates/game/src/lib.rs` (add `pub mod clock;`)
- Test: `crates/game/tests/clock.rs`

**Interfaces:**
- Consumes: `protocol::{Proposal, VoteView}` (Task 3).
- Produces (module `game::clock`):
  - `pub const SPEEDS: [u8; 4] = [1, 2, 4, 8]; pub const VOTE_LAPSE_S: f64 = 30.0; pub const TICKS_PER_REAL_S: f64 = 10.0;`
  - `pub struct OpenVote { pub proposal: Proposal, pub agreed: BTreeSet<String>, pub left_s: f64 }`
  - `pub enum VoteError { NotAHolder, BadSpeed }` (`Clone, Copy, Debug, PartialEq, Eq`)
  - `pub struct GameClock { pub paused: bool, pub speed: u8, pub vote: Option<OpenVote>, carry: f64 }` with
    `GameClock::new(paused: bool) -> GameClock` (speed 1),
    `ticks_for(&mut self, real_dt: f64) -> u64` (speed × 10 ticks per real second, fractions carried; 0 and carry cleared while paused; callers pass finite, non-negative time),
    `vote(&mut self, voter: &str, proposal: Proposal, holders: &BTreeSet<String>) -> Result<Option<Proposal>, VoteError>` (returns the proposal if it applied),
    `settle(&mut self, holders: &BTreeSet<String>) -> Option<Proposal>` (after claims, releases and grace expiry; no holders drops the vote),
    `lapse(&mut self, real_dt: f64)`, `vote_view(&self) -> Option<VoteView>`.

- [ ] **Step 1: Write the failing tests**

`crates/game/tests/clock.rs`:
```rust
//! Clock speed and votes (spec §3.5).

use std::collections::BTreeSet;

use game::clock::{GameClock, SPEEDS, VOTE_LAPSE_S, VoteError};
use protocol::Proposal;

fn holders(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn ticks_follow_the_speed_and_carry_fractions() {
    let mut c = GameClock::new(false);
    assert_eq!((c.paused, c.speed), (false, 1));
    assert_eq!(c.ticks_for(0.1), 1);
    assert_eq!(c.ticks_for(0.05), 0);
    assert_eq!(c.ticks_for(0.05), 1);
    c.speed = 8;
    assert_eq!(c.ticks_for(0.1), 8);
    assert_eq!(c.ticks_for(0.125), 10);
    let total: u64 = (0..1000).map(|_| c.ticks_for(0.1)).sum();
    assert_eq!(total, 8000);
    c.paused = true;
    assert_eq!(c.ticks_for(5.0), 0);
}

#[test]
fn a_lone_holders_proposal_applies_at_once() {
    let h = holders(&["alice"]);
    let mut c = GameClock::new(false);
    assert_eq!(c.vote("alice", Proposal::Pause, &h), Ok(Some(Proposal::Pause)));
    assert!(c.paused && c.vote.is_none());
    assert_eq!(c.vote("alice", Proposal::Speed { x: 4 }, &h), Ok(Some(Proposal::Speed { x: 4 })));
    assert_eq!(c.vote("alice", Proposal::Resume, &h), Ok(Some(Proposal::Resume)));
    assert_eq!((c.paused, c.speed), (false, 4));
}

#[test]
fn every_holder_must_agree() {
    let h = holders(&["alice", "bob"]);
    let mut c = GameClock::new(false);
    assert_eq!(c.vote("alice", Proposal::Speed { x: 4 }, &h), Ok(None));
    assert_eq!(c.speed, 1);
    let v = c.vote_view().unwrap();
    assert_eq!((v.proposal, v.agreed, v.expires_in_s), (Proposal::Speed { x: 4 }, vec!["alice".to_string()], 30));
    assert_eq!(c.vote("alice", Proposal::Speed { x: 4 }, &h), Ok(None), "agreeing twice changes nothing");
    assert_eq!(c.vote("bob", Proposal::Speed { x: 4 }, &h), Ok(Some(Proposal::Speed { x: 4 })));
    assert_eq!((c.speed, c.vote.is_none()), (4, true));
}

#[test]
fn only_holders_vote_and_only_listed_speeds() {
    let h = holders(&["alice"]);
    let mut c = GameClock::new(false);
    assert_eq!(c.vote("sam", Proposal::Pause, &h), Err(VoteError::NotAHolder));
    assert_eq!(c.vote("robot", Proposal::Pause, &h), Err(VoteError::NotAHolder));
    for x in [0, 3, 16, 255] {
        assert_eq!(c.vote("alice", Proposal::Speed { x }, &h), Err(VoteError::BadSpeed), "{x}");
    }
    assert_eq!(SPEEDS, [1, 2, 4, 8]);
    assert!(!c.paused && c.vote.is_none());
}

#[test]
fn a_different_proposal_replaces_the_open_one() {
    let h = holders(&["alice", "bob"]);
    let mut c = GameClock::new(false);
    c.vote("alice", Proposal::Pause, &h).unwrap();
    c.lapse(10.0);
    assert_eq!(c.vote("bob", Proposal::Speed { x: 2 }, &h), Ok(None));
    let v = c.vote_view().unwrap();
    assert_eq!((v.proposal, v.agreed, v.expires_in_s), (Proposal::Speed { x: 2 }, vec!["bob".to_string()], 30));
    assert_eq!(c.vote("alice", Proposal::Speed { x: 2 }, &h), Ok(Some(Proposal::Speed { x: 2 })));
    assert!(!c.paused);
}

#[test]
fn votes_lapse_after_thirty_seconds_of_real_time() {
    let h = holders(&["alice", "bob"]);
    let mut c = GameClock::new(false);
    c.vote("alice", Proposal::Pause, &h).unwrap();
    c.lapse(20.0);
    assert_eq!(c.vote_view().unwrap().expires_in_s, 10);
    c.lapse(VOTE_LAPSE_S - 20.0);
    assert!(c.vote.is_none());
    assert_eq!(c.vote("bob", Proposal::Pause, &h), Ok(None), "a lapsed vote starts again");
}

#[test]
fn a_holder_leaving_can_complete_a_vote() {
    let mut c = GameClock::new(false);
    c.vote("alice", Proposal::Pause, &holders(&["alice", "bob"])).unwrap();
    assert_eq!(c.settle(&holders(&["alice", "bob"])), None);
    assert_eq!(c.settle(&holders(&["alice"])), Some(Proposal::Pause));
    assert!(c.paused);
}

#[test]
fn with_no_holders_the_open_vote_is_dropped_and_the_clock_stays() {
    let mut c = GameClock::new(false);
    c.vote("alice", Proposal::Pause, &holders(&["alice", "bob"])).unwrap();
    assert_eq!(c.settle(&BTreeSet::new()), None);
    assert!(c.vote.is_none() && !c.paused);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-game --test clock`
Expected: compile error — unresolved `game::clock`.

- [ ] **Step 3: Implement**

Add `pub mod clock;` to `crates/game/src/lib.rs` (first in the list, keeping it alphabetical: `areas`, `clock`, `layout`, `names`, `notices`, `view`).

`crates/game/src/clock.rs`:
```rust
//! The game clock: pause and speed change only when every holder agrees
//! (spec §3.5). Real time is whatever the caller says it is.

use std::collections::BTreeSet;

use protocol::{Proposal, VoteView};

pub const SPEEDS: [u8; 4] = [1, 2, 4, 8];
/// A proposal lapses after this much real time.
pub const VOTE_LAPSE_S: f64 = 30.0;
/// Sim ticks per real second at 1x (one tick per 0.1 s).
pub const TICKS_PER_REAL_S: f64 = 10.0;

#[derive(Clone, Debug, PartialEq)]
pub struct OpenVote {
    pub proposal: Proposal,
    pub agreed: BTreeSet<String>,
    /// Real seconds before it lapses.
    pub left_s: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoteError {
    /// Only players holding an area vote.
    NotAHolder,
    /// Speeds are 1, 2, 4 or 8.
    BadSpeed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GameClock {
    pub paused: bool,
    pub speed: u8,
    /// At most one open proposal.
    pub vote: Option<OpenVote>,
    /// Fraction of a tick owed from earlier calls.
    carry: f64,
}

impl GameClock {
    pub fn new(paused: bool) -> GameClock {
        GameClock { paused, speed: 1, vote: None, carry: 0.0 }
    }

    /// Whole ticks to run for `real_dt` seconds of real time.
    pub fn ticks_for(&mut self, real_dt: f64) -> u64 {
        if self.paused {
            self.carry = 0.0;
            return 0;
        }
        self.carry += real_dt * TICKS_PER_REAL_S * f64::from(self.speed);
        // A hair of slack so 0.1 s steps are never a float's width short.
        let n = (self.carry + 1e-9).floor();
        self.carry = (self.carry - n).max(0.0);
        n as u64
    }

    /// `voter` proposes, or agrees to, `proposal`. Returns it if it applied.
    pub fn vote(&mut self, voter: &str, proposal: Proposal, holders: &BTreeSet<String>) -> Result<Option<Proposal>, VoteError> {
        if !holders.contains(voter) {
            return Err(VoteError::NotAHolder);
        }
        if let Proposal::Speed { x } = proposal {
            if !SPEEDS.contains(&x) {
                return Err(VoteError::BadSpeed);
            }
        }
        let same = self.vote.as_ref().is_some_and(|v| v.proposal == proposal);
        if same {
            if let Some(v) = self.vote.as_mut() {
                v.agreed.insert(voter.to_string());
            }
        } else {
            self.vote = Some(OpenVote { proposal, agreed: BTreeSet::from([voter.to_string()]), left_s: VOTE_LAPSE_S });
        }
        Ok(self.settle(holders))
    }

    /// Apply the open proposal if every holder has agreed. With no holders
    /// at all nobody can agree, so the proposal is dropped.
    pub fn settle(&mut self, holders: &BTreeSet<String>) -> Option<Proposal> {
        let v = self.vote.as_ref()?;
        if holders.is_empty() {
            self.vote = None;
            return None;
        }
        if !holders.iter().all(|h| v.agreed.contains(h)) {
            return None;
        }
        let p = v.proposal;
        self.vote = None;
        match p {
            Proposal::Pause => self.paused = true,
            Proposal::Resume => self.paused = false,
            Proposal::Speed { x } => self.speed = x,
        }
        Some(p)
    }

    /// Let `real_dt` seconds pass for the open proposal.
    pub fn lapse(&mut self, real_dt: f64) {
        let lapsed = match self.vote.as_mut() {
            Some(v) => {
                v.left_s -= real_dt;
                v.left_s <= 0.0
            }
            None => false,
        };
        if lapsed {
            self.vote = None;
        }
    }

    pub fn vote_view(&self) -> Option<VoteView> {
        self.vote.as_ref().map(|v| VoteView {
            proposal: v.proposal,
            agreed: v.agreed.iter().cloned().collect(),
            expires_in_s: v.left_s.max(0.0).ceil() as u32,
        })
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `scripts/cargo test -p signalbox-game --test clock`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/game/src/lib.rs crates/game/src/clock.rs crates/game/tests/clock.rs
git commit -m "feat(game): clock with unanimous holder votes

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: `Game` — players, claims, area checks, the robot, grace, notices, flush and resync

**Files:**
- Create: `crates/game/src/game.rs`
- Modify: `crates/game/src/lib.rs` (add `pub mod game;` and the re-exports)
- Modify: `crates/game/tests/common/mod.rs` (append game helpers)
- Test: `crates/game/tests/game.rs`

**Interfaces:**
- Consumes: `AreaMap`, `Visibility` (Task 4), `build_layout` (Task 4), `build_view`, `Shared` (Task 5), `resolve`, `to_player_command`, `valid_headcode`, `area_notices` (Task 6), `GameClock`, `VoteError` (Task 7), `protocol::diff` (Task 3), `robot::{commands, ROBOT_EVERY_TICKS}` (Task 1).
- Produces (module `game::game`, re-exported from `game`):
  - `pub const ROBOT: &str = "robot"; pub const GRACE_S: f64 = 120.0; pub const MAX_TICKS_PER_ADVANCE: u64 = 800;`
  - `pub type Out = (String, ServerMsg);`
  - `pub struct GameMeta { pub layout: String, pub seed: u64 }` (`Clone, Debug, PartialEq, Eq`)
  - `pub struct GameStats { pub spads, pub collisions, pub invariant_violations, pub player_commands, pub robot_commands, pub sim_rejections: usize }` (`Clone, Debug, Default, PartialEq, Eq`)
  - `impl Game`: `new(world: World, meta: GameMeta) -> Game` (running at 1x), `connect(&mut self, player: &str) -> Vec<Out>`, `disconnect(&mut self, player: &str)`, `handle(&mut self, player: &str, msg: ClientMsg) -> Vec<Out>`, `advance(&mut self, real_dt: f64) -> Vec<Out>`, `flush(&mut self) -> Vec<Out>`, `resync(&mut self, player: &str) -> Vec<Out>`, `sim(&self) -> &Sim`, `meta(&self) -> &GameMeta`, `stats(&self) -> &GameStats`, `clock(&self) -> &GameClock`, `holder(&self, area: &str) -> Option<&str>`, `area_of(&self, player: &str) -> Option<&str>`, `view_of(&self, player: &str) -> Option<View>` (the full view that player should hold now, numbered like the last one sent), `layout_of(&self, player: &str) -> Option<Layout>`.
  - Private hooks Task 9 builds on: `fn from_sim(sim: Sim, meta: GameMeta, paused: bool) -> Game`, `fn submit(&mut self, player: &str, cmd: Command) -> Vec<Out>` (every accepted and robot command goes through it), the field `robot_ran_at: Option<u64>`.
  - Test helpers appended to `tests/common/mod.rs`: `meta() -> GameMeta`, `game() -> Game`, `join(g: &mut Game, player: &str, area: Option<&str>) -> Vec<Out>`, `send(g: &mut Game, player: &str, msg: ClientMsg) -> Vec<Out>`, `command(g: &mut Game, player: &str, cmd: PlayerCommand) -> Vec<Out>`, `set_route(entrance: &str, exit: ExitName) -> PlayerCommand`, `notices(outs: &[Out], player: &str) -> Vec<Notice>`, `error_codes(outs: &[Out], player: &str) -> Vec<String>`, `run(g: &mut Game, real_s: f64, dt: f64) -> Vec<Out>`, `run_to_tick(g: &mut Game, tick: u64) -> Vec<Out>`, `Client { layout: Option<Layout>, view: Option<View> }` with `Client::take(&mut self, outs: &[Out], me: &str)`, `play_as_robot(g: &mut Game, player: &str) -> Vec<Out>`.

Rules implemented here (spec §3–§6 with amendments 7, 8, 11): `"robot"` cannot connect; a new player is a spectator; `connect` on a known player (reconnecting in grace, or already connected) marks them connected and resyncs. `disconnect` drops a spectator at once; a holder keeps the area for `GRACE_S` of real time (still counted as a holder for votes), then it goes to the robot. Messages from players who are not connected are ignored. Commands: unknown name → `rejected {unknown_id}`; bad headcode → `error {bad_headcode}`; `SwingPoints` on a non-points node → `rejected {not_points}`; subject outside the sender's area → `not_your_area {area}`; otherwise submitted. Sim rejections go back to their sender (matched in queue order). The robot runs at ticks that are multiples of `ROBOT_EVERY_TICKS`, only for commands whose subject area has no holder. `advance` does, in order: sanitise `real_dt`, lapse the vote, expire grace (then settle the vote), step `min(ticks, 800)` ticks. `flush` diffs each connected player's fresh view against the last one sent.

- [ ] **Step 1: Add the test helpers**

Append to `crates/game/tests/common/mod.rs`:
```rust
use game::areas::AreaMap;
use game::names::to_player_command;
use game::{Game, GameMeta, Out};
use protocol::{ClientMsg, ExitName, Layout, Notice, PlayerCommand, ServerMsg, View};
use signalbox_core::robot;

pub fn meta() -> GameMeta {
    GameMeta { layout: "twobox".into(), seed: 1 }
}

pub fn game() -> Game {
    Game::new(twobox(), meta())
}

/// Connect `player` and, if given, claim `area`; everything sent back.
pub fn join(g: &mut Game, player: &str, area: Option<&str>) -> Vec<Out> {
    let mut out = g.connect(player);
    if let Some(a) = area {
        out.extend(g.handle(player, ClientMsg::Claim { area: a.to_string() }));
    }
    out
}

pub fn send(g: &mut Game, player: &str, msg: ClientMsg) -> Vec<Out> {
    g.handle(player, msg)
}

pub fn command(g: &mut Game, player: &str, cmd: PlayerCommand) -> Vec<Out> {
    g.handle(player, ClientMsg::Command { cmd })
}

pub fn set_route(entrance: &str, exit: ExitName) -> PlayerCommand {
    PlayerCommand::SetRoute { entrance: entrance.to_string(), exit }
}

pub fn notices(outs: &[Out], player: &str) -> Vec<Notice> {
    outs.iter()
        .filter(|(p, _)| p == player)
        .filter_map(|(_, m)| match m {
            ServerMsg::Notice(n) => Some(n.clone()),
            _ => None,
        })
        .collect()
}

pub fn error_codes(outs: &[Out], player: &str) -> Vec<String> {
    notices(outs, player)
        .into_iter()
        .filter_map(|n| match n {
            Notice::Error { code, .. } => Some(code),
            _ => None,
        })
        .collect()
}

/// Advance `real_s` seconds of real time in steps of `dt`.
pub fn run(g: &mut Game, real_s: f64, dt: f64) -> Vec<Out> {
    let mut out = Vec::new();
    for _ in 0..(real_s / dt).round() as u64 {
        out.extend(g.advance(dt));
    }
    out
}

/// At 1x, advance one tick at a time until the sim reaches `tick`.
pub fn run_to_tick(g: &mut Game, tick: u64) -> Vec<Out> {
    assert_eq!(g.clock().speed, 1, "run_to_tick steps one tick per 0.1 s");
    assert!(!g.clock().paused, "run_to_tick on a paused game would never return");
    let mut out = Vec::new();
    while g.sim().tick() < tick {
        out.extend(g.advance(0.1));
    }
    out
}

/// A minimal client: the layout and view it has been sent, deltas applied.
#[derive(Debug, Default)]
pub struct Client {
    pub layout: Option<Layout>,
    pub view: Option<View>,
}

impl Client {
    pub fn take(&mut self, outs: &[Out], me: &str) {
        for (p, m) in outs {
            if p != me {
                continue;
            }
            match m {
                ServerMsg::Layout(l) => self.layout = Some(l.clone()),
                ServerMsg::View(v) => self.view = Some(v.clone()),
                ServerMsg::Delta(d) => self.view.as_mut().expect("a view before any delta").apply(d).expect("deltas in order"),
                ServerMsg::Notice(_) => {}
            }
        }
    }
}

/// Play `player`'s area as the robot would, sending its commands by name.
pub fn play_as_robot(g: &mut Game, player: &str) -> Vec<Out> {
    let w = g.sim().world();
    let Some(a) = g.area_of(player).and_then(|name| w.net.area(name)) else { return vec![] };
    let map = AreaMap::new(w);
    let cmds: Vec<PlayerCommand> = robot::commands(g.sim())
        .iter()
        .filter(|c| map.subject(c) == Some(a))
        .map(|c| to_player_command(w, c))
        .collect();
    cmds.into_iter().flat_map(|cmd| g.handle(player, ClientMsg::Command { cmd })).collect()
}
```

- [ ] **Step 2: Write the failing tests**

`crates/game/tests/game.rs`:
```rust
//! The game in process: claims, area checks, the robot, votes, grace,
//! notices, and views that clients rebuild from deltas.

mod common;

use std::collections::BTreeMap;

use common::*;
use game::areas::AreaMap;
use game::game::{GRACE_S, MAX_TICKS_PER_ADVANCE};
use game::{Out, ROBOT};
use protocol::*;

fn s(x: &str) -> String {
    x.to_string()
}

fn claim(g: &mut game::Game, player: &str, area: &str) -> Vec<Out> {
    g.handle(player, ClientMsg::Claim { area: s(area) })
}

#[test]
fn connecting_sends_the_whole_layout_and_a_first_view() {
    let mut g = game();
    let out = g.connect("alice");
    assert_eq!(out.len(), 2, "{out:?}");
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert_eq!((l.you.as_str(), l.area.as_deref(), l.sections.len(), l.signals.len()), ("alice", None, 5, 5));
    assert!(l.signals.iter().all(|x| !x.operable));
    let ServerMsg::View(v) = &out[1].1 else { panic!("{out:?}") };
    assert_eq!(v.seq, 1);
    assert_eq!(v.holders, map(&[("East", s(ROBOT)), ("West", s(ROBOT))]));
    assert_eq!((v.speed, v.paused, v.score), (1, false, None));
}

#[test]
fn robot_is_a_reserved_name() {
    let mut g = game();
    let out = g.connect(ROBOT);
    assert_eq!(error_codes(&out, ROBOT), [codes::RESERVED_NAME]);
    assert!(g.handle(ROBOT, ClientMsg::Claim { area: s("West") }).is_empty());
    assert_eq!(g.holder("West"), None);
}

#[test]
fn reconnecting_while_connected_just_resyncs() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    let out = g.connect("alice");
    let ServerMsg::View(v) = &out[1].1 else { panic!("{out:?}") };
    assert_eq!(v.seq, 3);
    assert_eq!(g.holder("West"), Some("alice"));
}

#[test]
fn claiming_an_area_resyncs_with_its_layout_and_fringe() {
    let mut g = game();
    g.connect("alice");
    let out = claim(&mut g, "alice", "West");
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert_eq!(l.area.as_deref(), Some("West"));
    let sections: Vec<(&str, bool)> = l.sections.iter().map(|x| (x.name.as_str(), x.fringe)).collect();
    assert_eq!(sections, [("TW1", false), ("TW2", false), ("TP", true)]);
    let ServerMsg::View(v) = &out[1].1 else { panic!("{out:?}") };
    assert_eq!((v.seq, v.score), (2, Some(0)));
    assert_eq!(v.holders["West"], "alice");
    assert_eq!((g.holder("West"), g.area_of("alice")), (Some("alice"), Some("West")));
}

#[test]
fn a_held_area_cannot_be_claimed_and_unknown_areas_are_errors() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    g.connect("bob");
    let out = claim(&mut g, "bob", "West");
    assert_eq!(notices(&out, "bob"), vec![Notice::AreaTaken { area: s("West"), holder: s("alice") }]);
    assert_eq!(error_codes(&claim(&mut g, "bob", "North"), "bob"), [codes::UNKNOWN_AREA]);
    assert_eq!(g.holder("West"), Some("alice"));
    assert_eq!(g.area_of("bob"), None);
}

#[test]
fn claiming_another_area_moves_you() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    let out = claim(&mut g, "alice", "East");
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert_eq!(l.area.as_deref(), Some("East"));
    assert_eq!((g.holder("West"), g.holder("East")), (None, Some("alice")));
}

#[test]
fn releasing_gives_the_area_back_to_the_robot() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    let out = send(&mut g, "alice", ClientMsg::Release);
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert_eq!(l.area, None);
    assert_eq!(g.holder("West"), None);
    assert_eq!(error_codes(&send(&mut g, "alice", ClientMsg::Release), "alice"), [codes::NOT_HOLDING]);
}

#[test]
fn commands_outside_your_area_never_reach_the_sim() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "sam", None);
    let east = vec![
        set_route("C", ExitName::Signal(s("W2"))),
        PlayerCommand::CancelRoute { entrance: s("C") },
        PlayerCommand::SetAutoWorking { entrance: s("D"), on: true },
        PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse },
        PlayerCommand::Interpose { berth: s("BC"), headcode: s("1A01") },
        PlayerCommand::CancelBerth { berth: s("BE") },
    ];
    for cmd in east {
        for p in ["alice", "sam"] {
            let out = command(&mut g, p, cmd.clone());
            assert_eq!(notices(&out, p), vec![Notice::NotYourArea { area: s("East") }], "{p} {cmd:?}");
        }
    }
    g.advance(1.0);
    assert!(g.sim().log().is_empty(), "{:?}", g.sim().log());

    let west = vec![
        set_route("W1", ExitName::Signal(s("A"))),
        PlayerCommand::SetAutoWorking { entrance: s("W1"), on: true },
        PlayerCommand::CancelRoute { entrance: s("W1") },
        PlayerCommand::Interpose { berth: s("BA"), headcode: s("1A01") },
        PlayerCommand::CancelBerth { berth: s("BA") },
    ];
    for cmd in west {
        assert!(command(&mut g, "alice", cmd).is_empty());
    }
    let out = g.advance(0.1);
    assert_eq!(g.sim().log().len(), 5);
    assert!(notices(&out, "alice").is_empty(), "{out:?}");
    assert_eq!(g.stats().player_commands, 5);

    join(&mut g, "bob", Some("East"));
    assert!(command(&mut g, "bob", PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse }).is_empty());
    g.advance(0.1);
    assert_eq!(g.sim().log().len(), 6);
}

#[test]
fn unknown_names_and_non_points_are_rejected_before_the_sim() {
    let mut g = game();
    join(&mut g, "alice", Some("East"));
    let bad = PlayerCommand::CancelRoute { entrance: s("Z9") };
    let out = command(&mut g, "alice", bad.clone());
    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: bad, reason: Rejection::UnknownId }]);
    let joint = PlayerCommand::SwingPoints { points: s("J2"), to: PointsPos::Reverse };
    let out = command(&mut g, "alice", joint.clone());
    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: joint, reason: Rejection::NotPoints }]);
    g.advance(0.1);
    assert!(g.sim().log().is_empty());
}

#[test]
fn sim_rejections_go_back_to_the_sender() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "bob", Some("East"));
    let cancel = PlayerCommand::CancelRoute { entrance: s("W1") };
    assert!(command(&mut g, "alice", cancel.clone()).is_empty());
    let out = g.advance(0.1);
    assert_eq!(notices(&out, "alice"), vec![Notice::Rejected { cmd: cancel, reason: Rejection::RouteNotSet }]);
    assert!(notices(&out, "bob").is_empty());
    assert_eq!(g.stats().sim_rejections, 1);
}

#[test]
fn bad_headcodes_are_refused() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    for h in ["", "1A 01", "ABCDEFGHIJK"] {
        let out = command(&mut g, "alice", PlayerCommand::Interpose { berth: s("BA"), headcode: s(h) });
        assert_eq!(error_codes(&out, "alice"), [codes::BAD_HEADCODE], "{h:?}");
    }
    g.advance(0.1);
    assert!(g.sim().log().is_empty());
}

#[test]
fn the_robot_never_commands_a_claimed_area() {
    let mut g = game();
    join(&mut g, "alice", Some("East"));
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
    run(&mut g, 20.0 * 60.0 / 8.0, 0.1);
    let map = AreaMap::new(g.sim().world());
    let east = g.sim().world().net.area("East").unwrap();
    assert!(g.stats().robot_commands > 0, "{:?}", g.stats());
    assert!(g.sim().log().iter().all(|(_, c)| map.subject(c) != Some(east)), "{:?}", g.sim().log());
}

#[test]
fn claiming_stops_the_robot_at_once_and_its_routes_stay_set() {
    let mut g = game();
    join(&mut g, "alice", None);
    run_to_tick(&mut g, 11);
    let before = g.sim().log().len();
    assert!(before >= 2, "the robot routes 1E01 at tick 10: {:?}", g.sim().log());
    let out = claim(&mut g, "alice", "West");
    let ServerMsg::View(v) = &out[1].1 else { panic!("{out:?}") };
    assert!(v.routes.contains_key("W1-A") && v.routes.contains_key("A-E"), "{:?}", v.routes);
    run_to_tick(&mut g, 15 * 600);
    let map = AreaMap::new(g.sim().world());
    let west = g.sim().world().net.area("West").unwrap();
    assert!(g.sim().log()[before..].iter().all(|(_, c)| map.subject(c) != Some(west)), "{:?}", g.sim().log());
}

#[test]
fn a_train_crossing_into_your_area_is_handed_over() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    command(&mut g, "alice", set_route("W1", ExitName::Signal(s("A"))));
    command(&mut g, "alice", set_route("A", ExitName::Node(s("E"))));
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
    let out = run(&mut g, 16.0 * 60.0 / 8.0, 0.1);
    let handover = Notice::Handover { headcode: s("2W03"), from_area: s("East") };
    assert!(notices(&out, "alice").contains(&handover), "{:?}", notices(&out, "alice"));
}

#[test]
fn the_clock_follows_votes() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    g.advance(1.0);
    assert_eq!(g.sim().tick(), 10);
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Speed { x: 4 } });
    g.advance(1.0);
    assert_eq!(g.sim().tick(), 50);
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Pause });
    g.advance(10.0);
    assert_eq!(g.sim().tick(), 50);
    assert!(g.view_of("alice").unwrap().paused);
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Resume });
    g.advance(0.1);
    assert_eq!(g.sim().tick(), 54);
    let out = send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Speed { x: 3 } });
    assert_eq!(error_codes(&out, "alice"), [codes::BAD_SPEED]);
    join(&mut g, "sam", None);
    let out = send(&mut g, "sam", ClientMsg::Vote { proposal: Proposal::Pause });
    assert_eq!(error_codes(&out, "sam"), [codes::NOT_A_HOLDER]);
}

#[test]
fn advance_survives_bad_real_time() {
    let mut g = game();
    for dt in [f64::NAN, -1.0, f64::NEG_INFINITY, f64::INFINITY, 0.0, -0.0] {
        assert!(g.advance(dt).is_empty(), "{dt}");
        assert_eq!(g.sim().tick(), 0, "{dt}");
    }
    g.advance(1e9);
    assert_eq!(g.sim().tick(), MAX_TICKS_PER_ADVANCE);
    g.advance(0.1);
    assert_eq!(g.sim().tick(), MAX_TICKS_PER_ADVANCE + 1);
}

#[test]
fn grace_keeps_the_area_then_releases_it() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "bob", None);
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Pause });
    g.disconnect("alice");
    assert!(g.handle("alice", ClientMsg::Release).is_empty(), "a disconnected player is not heard");
    g.advance(GRACE_S - 1.0);
    assert_eq!(g.holder("West"), Some("alice"));
    assert_eq!(notices(&claim(&mut g, "bob", "West"), "bob"), vec![Notice::AreaTaken { area: s("West"), holder: s("alice") }]);
    let out = g.connect("alice");
    let ServerMsg::Layout(l) = &out[0].1 else { panic!("{out:?}") };
    assert_eq!(l.area.as_deref(), Some("West"));
    g.disconnect("alice");
    g.advance(GRACE_S - 1.0);
    assert_eq!(g.holder("West"), Some("alice"), "reconnecting restarted the grace period");
    g.advance(1.0);
    assert_eq!(g.holder("West"), None);
    assert_eq!(g.view_of("bob").unwrap().holders["West"], ROBOT);
    claim(&mut g, "bob", "West");
    assert_eq!(g.holder("West"), Some("bob"));
}

#[test]
fn a_spectator_who_leaves_is_forgotten() {
    let mut g = game();
    join(&mut g, "sam", None);
    g.disconnect("sam");
    assert_eq!(g.view_of("sam"), None);
    assert!(g.flush().is_empty());
}

#[test]
fn grace_expiry_completes_a_vote() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "bob", Some("East"));
    g.disconnect("bob");
    g.advance(100.0);
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Pause });
    assert!(!g.clock().paused, "bob still holds East and has not agreed");
    g.advance(10.0);
    assert!(!g.clock().paused && g.clock().vote.is_some());
    g.advance(10.0);
    assert_eq!(g.holder("East"), None);
    assert!(g.clock().paused, "bob's grace ran out, leaving alice as the only holder");
}

#[test]
fn resync_restarts_the_delta_base() {
    let mut g = game();
    let mut c = Client::default();
    c.take(&g.connect("alice"), "alice");
    g.advance(1.0);
    let out = g.flush();
    assert!(matches!(&out[..], [(_, ServerMsg::Delta(d))] if d.seq == 2), "{out:?}");
    c.take(&out, "alice");
    let out = send(&mut g, "alice", ClientMsg::Resync);
    assert!(matches!(&out[..], [(_, ServerMsg::Layout(_)), (_, ServerMsg::View(v))] if v.seq == 3), "{out:?}");
    c.take(&out, "alice");
    g.advance(1.0);
    let out = g.flush();
    assert!(matches!(&out[..], [(_, ServerMsg::Delta(d))] if d.seq == 4), "{out:?}");
    c.take(&out, "alice");
    assert_eq!(c.view, g.view_of("alice"));
}

#[test]
fn flush_sends_nothing_when_nothing_changed() {
    let mut g = game();
    join(&mut g, "alice", Some("West"));
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Pause });
    assert_eq!(g.flush().len(), 1, "the pause itself");
    g.advance(5.0);
    assert!(g.flush().is_empty());
}

fn deliver(clients: &mut BTreeMap<&'static str, Client>, out: &[Out]) {
    for (p, c) in clients.iter_mut() {
        c.take(out, p);
    }
}

/// Spec §12: area, fringe and spectator views rebuilt from deltas equal the
/// server's full view at every flush, through claims and releases.
#[test]
fn every_client_rebuilds_the_servers_view_from_deltas() {
    let mut g = game();
    let mut clients: BTreeMap<&'static str, Client> = BTreeMap::new();
    for p in ["alice", "bob", "carol"] {
        clients.insert(p, Client::default());
    }
    for (p, a) in [("alice", Some("West")), ("bob", Some("East")), ("carol", None)] {
        let out = join(&mut g, p, a);
        deliver(&mut clients, &out);
    }
    for p in ["alice", "bob"] {
        let out = send(&mut g, p, ClientMsg::Vote { proposal: Proposal::Speed { x: 8 } });
        deliver(&mut clients, &out);
    }
    assert_eq!(g.clock().speed, 8);
    // 0.125 s at 8x is one robot period (10 ticks): 2400 periods = 40 sim minutes.
    for period in 0..2400u64 {
        if period == 1200 {
            let out = send(&mut g, "bob", ClientMsg::Release);
            deliver(&mut clients, &out);
        }
        if period == 1500 {
            let out = claim(&mut g, "bob", "East");
            deliver(&mut clients, &out);
        }
        for p in ["alice", "bob"] {
            let out = play_as_robot(&mut g, p);
            deliver(&mut clients, &out);
        }
        let out = g.advance(0.125);
        deliver(&mut clients, &out);
        if period % 2 == 1 {
            let out = g.flush();
            deliver(&mut clients, &out);
            for (p, c) in &clients {
                assert_eq!(c.view, g.view_of(p), "{p} at tick {}", g.sim().tick());
                assert_eq!(c.layout, g.layout_of(p), "{p}");
            }
        }
    }
    let st = g.stats();
    assert!(st.player_commands > 0, "{st:?}");
    assert_eq!((st.spads, st.collisions, st.invariant_violations), (0, 0, 0), "{st:?}");
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-game --test game`
Expected: compile errors — unresolved `game::Game`, `game::game`.

- [ ] **Step 4: Implement**

Replace `crates/game/src/lib.rs` with:
```rust
//! The multiplayer game (spec §6): a core `Sim` shared by players who each
//! run one signalling area, with the robot playing the rest. Pure logic; the
//! only I/O is the SQLite save in `save`.

pub mod areas;
pub mod clock;
pub mod game;
pub mod layout;
pub mod names;
pub mod notices;
pub mod view;

pub use game::{GRACE_S, Game, GameMeta, GameStats, MAX_TICKS_PER_ADVANCE, Out, ROBOT};
```

`crates/game/src/game.rs`:
```rust
//! The game: players, claims, area checks, the robot for unclaimed areas,
//! the clock, notices and per-player views (spec §6.1). No I/O.

use std::collections::{BTreeMap, BTreeSet};

use protocol::{ClientMsg, Layout, Notice, PlayerCommand, Proposal, Rejection, ServerMsg, View, codes};
use signalbox_core::events::{Command, Event};
use signalbox_core::ids::AreaId;
use signalbox_core::robot;
use signalbox_core::sim::Sim;
use signalbox_core::world::World;

use crate::areas::{AreaMap, Visibility};
use crate::clock::{GameClock, VoteError};
use crate::layout::build_layout;
use crate::names::{resolve, to_player_command, valid_headcode};
use crate::notices::area_notices;
use crate::view::{Shared, build_view};

/// The robot's player name, reserved: it holds every unclaimed area.
pub const ROBOT: &str = "robot";
/// Real seconds a disconnected holder keeps their area.
pub const GRACE_S: f64 = 120.0;
/// A stalled caller never makes one `advance` run away.
pub const MAX_TICKS_PER_ADVANCE: u64 = 800;

/// A message for one player.
pub type Out = (String, ServerMsg);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameMeta {
    /// Layout name, for the lobby.
    pub layout: String,
    pub seed: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameStats {
    pub spads: usize,
    pub collisions: usize,
    pub invariant_violations: usize,
    /// Commands from players that reached the sim.
    pub player_commands: usize,
    /// Commands from the robot that reached the sim.
    pub robot_commands: usize,
    /// Commands the sim refused.
    pub sim_rejections: usize,
}

struct Player {
    area: Option<AreaId>,
    connected: bool,
    /// Real seconds since a holder disconnected.
    gone_s: f64,
    vis: Visibility,
    /// The last view sent (the base of the next delta).
    last: Option<View>,
}

pub struct Game {
    sim: Sim,
    meta: GameMeta,
    map: AreaMap,
    spectator: Visibility,
    /// Visibility of each area's holder, by area.
    by_area: Vec<Visibility>,
    /// Holder of each area, by area; `None` = the robot.
    holders: Vec<Option<String>>,
    players: BTreeMap<String, Player>,
    clock: GameClock,
    /// Commands queued for the next tick, with who sent them.
    queued: Vec<(String, Command)>,
    /// The robot already ran at this tick (a resumed log ended with its commands).
    robot_ran_at: Option<u64>,
    stats: GameStats,
}

fn error(player: &str, code: &str, message: &str) -> Out {
    (player.to_string(), ServerMsg::Notice(Notice::Error { code: code.to_string(), message: message.to_string() }))
}

fn notice(player: &str, n: Notice) -> Out {
    (player.to_string(), ServerMsg::Notice(n))
}

impl Game {
    /// A new game, running at 1x.
    pub fn new(world: World, meta: GameMeta) -> Game {
        let sim = Sim::new(world, meta.seed);
        Game::from_sim(sim, meta, false)
    }

    fn from_sim(sim: Sim, meta: GameMeta, paused: bool) -> Game {
        let w = sim.world();
        let map = AreaMap::new(w);
        let spectator = Visibility::spectator(w, &map);
        let by_area = (0..w.net.areas.len()).map(|a| Visibility::of_area(w, &map, AreaId::from_idx(a))).collect();
        let holders = vec![None; w.net.areas.len()];
        Game {
            sim,
            meta,
            map,
            spectator,
            by_area,
            holders,
            players: BTreeMap::new(),
            clock: GameClock::new(paused),
            queued: Vec::new(),
            robot_ran_at: None,
            stats: GameStats::default(),
        }
    }

    pub fn sim(&self) -> &Sim {
        &self.sim
    }

    pub fn meta(&self) -> &GameMeta {
        &self.meta
    }

    pub fn stats(&self) -> &GameStats {
        &self.stats
    }

    pub fn clock(&self) -> &GameClock {
        &self.clock
    }

    /// Who holds `area` (`None` for the robot or an unknown area).
    pub fn holder(&self, area: &str) -> Option<&str> {
        let a = self.sim.world().net.area(area)?;
        self.holders[a.idx()].as_deref()
    }

    /// The area `player` holds.
    pub fn area_of(&self, player: &str) -> Option<&str> {
        let a = self.players.get(player)?.area?;
        Some(self.sim.world().net.areas[a.idx()].name.as_str())
    }

    /// The full view `player` should hold now, numbered like the last one sent.
    pub fn view_of(&self, player: &str) -> Option<View> {
        let p = self.players.get(player)?;
        let seq = p.last.as_ref()?.seq;
        Some(build_view(&self.sim, &p.vis, &self.shared(), seq))
    }

    pub fn layout_of(&self, player: &str) -> Option<Layout> {
        let p = self.players.get(player)?;
        Some(build_layout(self.sim.world(), &self.map, &p.vis, player))
    }

    pub fn connect(&mut self, player: &str) -> Vec<Out> {
        if player == ROBOT {
            return vec![error(player, codes::RESERVED_NAME, "`robot` is a reserved name")];
        }
        let spectator = self.spectator.clone();
        let p = self.players.entry(player.to_string()).or_insert_with(|| Player {
            area: None,
            connected: true,
            gone_s: 0.0,
            vis: spectator,
            last: None,
        });
        p.connected = true;
        p.gone_s = 0.0;
        self.resync(player)
    }

    /// A spectator is forgotten; a holder keeps their area for `GRACE_S`.
    pub fn disconnect(&mut self, player: &str) {
        let Some(p) = self.players.get_mut(player) else { return };
        if p.area.is_none() {
            self.players.remove(player);
            return;
        }
        p.connected = false;
        p.gone_s = 0.0;
        p.last = None;
    }

    pub fn handle(&mut self, player: &str, msg: ClientMsg) -> Vec<Out> {
        if !self.players.get(player).is_some_and(|p| p.connected) {
            return vec![];
        }
        match msg {
            ClientMsg::Claim { area } => self.claim(player, &area),
            ClientMsg::Release => self.release(player),
            ClientMsg::Command { cmd } => self.command(player, cmd),
            ClientMsg::Vote { proposal } => self.vote(player, proposal),
            ClientMsg::Resync => self.resync(player),
        }
    }

    /// The layout and a full view; the view becomes the next delta's base.
    pub fn resync(&mut self, player: &str) -> Vec<Out> {
        let shared = self.shared();
        let Some(p) = self.players.get_mut(player) else { return vec![] };
        if !p.connected {
            return vec![];
        }
        let seq = p.last.as_ref().map_or(1, |v| v.seq + 1);
        let layout = build_layout(self.sim.world(), &self.map, &p.vis, player);
        let view = build_view(&self.sim, &p.vis, &shared, seq);
        p.last = Some(view.clone());
        vec![(player.to_string(), ServerMsg::Layout(layout)), (player.to_string(), ServerMsg::View(view))]
    }

    /// Run the clock for `real_dt` seconds of real time.
    pub fn advance(&mut self, real_dt: f64) -> Vec<Out> {
        let dt = if real_dt.is_finite() && real_dt > 0.0 { real_dt } else { 0.0 };
        let mut out = Vec::new();
        self.clock.lapse(dt);
        self.expire_grace(dt);
        let n = self.clock.ticks_for(dt).min(MAX_TICKS_PER_ADVANCE);
        for _ in 0..n {
            out.extend(self.tick());
        }
        out
    }

    /// Deltas for every connected player whose view changed.
    pub fn flush(&mut self) -> Vec<Out> {
        let shared = self.shared();
        let mut out = Vec::new();
        for (name, p) in self.players.iter_mut() {
            if !p.connected {
                continue;
            }
            let Some(last) = &p.last else { continue };
            let view = build_view(&self.sim, &p.vis, &shared, last.seq + 1);
            if let Some(d) = protocol::diff(last, &view) {
                out.push((name.clone(), ServerMsg::Delta(d)));
                p.last = Some(view);
            }
        }
        out
    }

    fn shared(&self) -> Shared {
        let net = &self.sim.world().net;
        Shared {
            sim_time: self.sim.now_s(),
            speed: self.clock.speed,
            paused: self.clock.paused,
            vote: self.clock.vote_view(),
            holders: net
                .areas
                .iter()
                .zip(&self.holders)
                .map(|(a, h)| (a.name.clone(), h.clone().unwrap_or_else(|| ROBOT.to_string())))
                .collect(),
        }
    }

    fn area_name(&self, a: AreaId) -> String {
        self.sim.world().net.areas[a.idx()].name.clone()
    }

    fn holder_set(&self) -> BTreeSet<String> {
        self.holders.iter().flatten().cloned().collect()
    }

    fn settle_vote(&mut self) {
        let holders = self.holder_set();
        self.clock.settle(&holders);
    }

    fn claim(&mut self, player: &str, area: &str) -> Vec<Out> {
        let Some(a) = self.sim.world().net.area(area) else {
            return vec![error(player, codes::UNKNOWN_AREA, &format!("no area `{area}`"))];
        };
        if let Some(h) = self.holders[a.idx()].clone() {
            if h == player {
                return self.resync(player);
            }
            return vec![notice(player, Notice::AreaTaken { area: area.to_string(), holder: h })];
        }
        if let Some(old) = self.players[player].area {
            self.holders[old.idx()] = None;
        }
        self.holders[a.idx()] = Some(player.to_string());
        let vis = self.by_area[a.idx()].clone();
        let p = self.players.get_mut(player).expect("connected players exist");
        p.area = Some(a);
        p.vis = vis;
        self.settle_vote();
        self.resync(player)
    }

    fn release(&mut self, player: &str) -> Vec<Out> {
        let Some(a) = self.players[player].area else {
            return vec![error(player, codes::NOT_HOLDING, "you hold no area")];
        };
        self.holders[a.idx()] = None;
        let vis = self.spectator.clone();
        let p = self.players.get_mut(player).expect("connected players exist");
        p.area = None;
        p.vis = vis;
        self.settle_vote();
        self.resync(player)
    }

    fn command(&mut self, player: &str, cmd: PlayerCommand) -> Vec<Out> {
        let reject = |reason| vec![notice(player, Notice::Rejected { cmd: cmd.clone(), reason })];
        let Some(core) = resolve(self.sim.world(), &cmd) else { return reject(Rejection::UnknownId) };
        if let PlayerCommand::Interpose { headcode, .. } = &cmd {
            if !valid_headcode(headcode) {
                return vec![error(player, codes::BAD_HEADCODE, "a headcode is 1 to 10 letters or digits")];
            }
        }
        let Some(area) = self.map.subject(&core) else { return reject(Rejection::NotPoints) };
        if self.players[player].area != Some(area) {
            return vec![notice(player, Notice::NotYourArea { area: self.area_name(area) })];
        }
        self.stats.player_commands += 1;
        self.submit(player, core)
    }

    fn vote(&mut self, player: &str, proposal: Proposal) -> Vec<Out> {
        let holders = self.holder_set();
        match self.clock.vote(player, proposal, &holders) {
            Ok(_) => vec![],
            Err(VoteError::NotAHolder) => vec![error(player, codes::NOT_A_HOLDER, "only players holding an area vote")],
            Err(VoteError::BadSpeed) => vec![error(player, codes::BAD_SPEED, "speed must be 1, 2, 4 or 8")],
        }
    }

    /// Queue a command for the next tick; `player` is `ROBOT` for the robot.
    fn submit(&mut self, player: &str, cmd: Command) -> Vec<Out> {
        self.queued.push((player.to_string(), cmd.clone()));
        self.sim.submit(cmd);
        Vec::new()
    }

    fn expire_grace(&mut self, dt: f64) {
        let mut expired = Vec::new();
        for (name, p) in self.players.iter_mut() {
            if !p.connected {
                p.gone_s += dt;
                if p.gone_s >= GRACE_S {
                    expired.push(name.clone());
                }
            }
        }
        if expired.is_empty() {
            return;
        }
        for name in &expired {
            if let Some(a) = self.players.remove(name).and_then(|p| p.area) {
                self.holders[a.idx()] = None;
            }
        }
        self.settle_vote();
    }

    /// The robot's commands for areas nobody holds.
    fn run_robot(&mut self) -> Vec<Out> {
        let mut out = Vec::new();
        for cmd in robot::commands(&self.sim) {
            let Some(area) = self.map.subject(&cmd) else { continue };
            if self.holders[area.idx()].is_none() {
                self.stats.robot_commands += 1;
                out.extend(self.submit(ROBOT, cmd));
            }
        }
        out
    }

    fn tick(&mut self) -> Vec<Out> {
        let mut out = Vec::new();
        let t = self.sim.tick();
        if t % robot::ROBOT_EVERY_TICKS == 0 && self.robot_ran_at != Some(t) {
            out.extend(self.run_robot());
        }
        let before = self.sim.describer().berths.clone();
        let queued = std::mem::take(&mut self.queued);
        let events = self.sim.step();
        let mut cursor = 0;
        for e in &events {
            match e {
                Event::SignalPassedAtDanger { .. } => self.stats.spads += 1,
                Event::Collision { .. } => self.stats.collisions += 1,
                Event::InvariantViolated { .. } => self.stats.invariant_violations += 1,
                Event::CommandRejected { cmd, reason } => {
                    self.stats.sim_rejections += 1;
                    if let Some(i) = (cursor..queued.len()).find(|&i| queued[i].1 == *cmd) {
                        cursor = i + 1;
                        let who = &queued[i].0;
                        if self.players.get(who).is_some_and(|p| p.connected) {
                            let named = to_player_command(self.sim.world(), cmd);
                            out.push(notice(who, Notice::Rejected { cmd: named, reason: *reason }));
                        }
                    }
                }
                _ => {}
            }
        }
        for (area, n) in area_notices(&self.sim, &self.map, &before, &events) {
            if let Some(h) = &self.holders[area.idx()] {
                if self.players.get(h).is_some_and(|p| p.connected) {
                    out.push(notice(h, n));
                }
            }
        }
        out
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `scripts/cargo test -p signalbox-game`
Expected: PASS (all game test files). `every_client_rebuilds_the_servers_view_from_deltas` and the 15-minute tests run a few thousand ticks each; they take seconds in a debug build.

- [ ] **Step 6: Commit**

```bash
git add crates/game/src/lib.rs crates/game/src/game.rs crates/game/tests/common/mod.rs crates/game/tests/game.rs
git commit -m "feat(game): players, claims, area checks, robot, grace, votes and deltas

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: SQLite saves, autosave and resume

**Files:**
- Modify: `Cargo.toml` (workspace dependency `rusqlite`)
- Modify: `crates/game/Cargo.toml` (dependencies)
- Create: `crates/game/src/save.rs`
- Modify: `crates/game/src/lib.rs` (add `pub mod save;`, re-exports)
- Modify: `crates/game/src/game.rs` (save handle, `create`, `resume`, `save_now`, logging in `submit`, autosave in `advance`)
- Test: `crates/game/tests/save.rs`

**Interfaces:**
- Consumes: `Game`, `GameMeta`, `ROBOT`, `Game::from_sim`, `Game::submit`, field `robot_ran_at` (Task 8); `Sim::{restore, snapshot, submit, step, tick}`, `SimState` (already `Serialize + Deserialize + PartialEq`), `WorldFile` (for the `areas` and `start` meta rows).
- Produces:
  - `game::save::SAVE_SCHEMA: u32 = 1`, `game::save::KEEP_SNAPSHOTS: i64 = 3`.
  - `game::save::SaveError { Sql(rusqlite::Error), Bad(String) }` (`Debug, thiserror::Error`).
  - `game::save::Logged { seq: i64, tick: u64, player: String, area: String, command: Command }`, `game::save::Saved { meta: GameMeta, world_json: String, snapshot: SimState, commands_after: Vec<Logged> }` (both `Clone, Debug, PartialEq`).
  - `SaveDb::create(path: &Path, meta: &GameMeta, world_json: &str) -> Result<SaveDb, SaveError>` (refuses an existing file; WAL; schema exactly spec §7.1; meta rows `schema, layout, seed, created, last_played, areas, start`), `SaveDb::open(path: &Path) -> Result<SaveDb, SaveError>`, `append_command(&self, tick: u64, player: &str, area: &str, cmd: &Command) -> Result<(), SaveError>`, `write_snapshot(&self, state: &SimState) -> Result<(), SaveError>` (replaces a snapshot of the same tick, keeps the newest 3, updates `last_played`), `load(&self) -> Result<Saved, SaveError>` (newest snapshot; commands with `tick >= snapshot.tick` in `seq` order).
  - `game::save::resume_sim(world: World, snapshot: SimState, commands: &[Logged]) -> Result<(Sim, bool), String>` — restore, skip the first `snapshot.queue.len()` commands logged at the snapshot's tick (amendment 10), replay the rest (submit each tick's commands, step), stop at the last logged tick with its commands queued; the flag says the robot's commands are among them.
  - `game::GameError { World(LoadError), Save(SaveError), Resume(String) }`; `game::game::SNAPSHOT_EVERY_S: f64 = 60.0`; `Game::create(path: &Path, world_json: &str, meta: GameMeta) -> Result<Game, GameError>`, `Game::resume(path: &Path) -> Result<Game, GameError>` (paused, 1x, no claims), `Game::save_now(&mut self) -> Vec<Out>`.
  - Failed writes become `error {code: save_failed}` to every connected player; the game keeps running (spec §11).

- [ ] **Step 1: Add the dependency**

In the root `Cargo.toml`, add to `[workspace.dependencies]`:
```toml
rusqlite = { version = "0.40", features = ["bundled"] }
```

In `crates/game/Cargo.toml`, make `[dependencies]`:
```toml
[dependencies]
signalbox-core = { path = "../core" }
signalbox-protocol = { path = "../protocol" }
rusqlite.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```

`bundled` compiles SQLite from C source with the image's `cc`; the first build takes a minute and needs the network (`scripts/cargo` has it). If 0.40 does not resolve, use the newest published `rusqlite` and record the version in the commit message. `Cargo.lock` changes; commit it.

- [ ] **Step 2: Write the failing tests**

`crates/game/tests/save.rs`:
```rust
//! SQLite saves (spec §7): a saved, dropped and resumed game continues
//! exactly as an uninterrupted one.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use game::names::resolve;
use game::save::KEEP_SNAPSHOTS;
use game::{Game, GameError, ROBOT};
use protocol::*;
use rusqlite::Connection;
use signalbox_core::events::Command;

fn s(x: &str) -> String {
    x.to_string()
}

fn temp_save(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("signalbox-game-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.sqlite"));
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
    path
}

fn open(path: &Path) -> Connection {
    Connection::open(path).unwrap()
}

fn snapshot_ticks(path: &Path) -> Vec<i64> {
    let c = open(path);
    let mut st = c.prepare("SELECT tick FROM snapshots ORDER BY tick").unwrap();
    let ticks: Vec<i64> = st.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
    ticks
}

fn resume_err(path: &Path) -> GameError {
    match Game::resume(path) {
        Ok(_) => panic!("resumed a broken save"),
        Err(e) => e,
    }
}

/// Alice claims West again and resumes the clock.
fn rejoin(g: &mut Game) {
    join(g, "alice", Some("West"));
    send(g, "alice", ClientMsg::Vote { proposal: Proposal::Resume });
    assert!(!g.clock().paused);
}

/// Resume the clock of a game nobody should hold: claim, vote, release.
fn unpause(g: &mut Game) {
    join(g, "ops", Some("West"));
    send(g, "ops", ClientMsg::Vote { proposal: Proposal::Resume });
    send(g, "ops", ClientMsg::Release);
    assert!(!g.clock().paused);
}

#[test]
fn create_writes_meta_world_and_a_first_snapshot() {
    let path = temp_save("create");
    let g = Game::create(&path, &twobox_json(), meta()).unwrap();
    assert_eq!(g.meta(), &meta());
    drop(g);
    let c = open(&path);
    let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
    assert_eq!(mode, "wal");
    let mut st = c.prepare("SELECT key, value FROM meta ORDER BY key").unwrap();
    let rows: Vec<(String, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();
    let keys: Vec<&str> = rows.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["areas", "created", "last_played", "layout", "schema", "seed", "start"]);
    let get = |k: &str| rows.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap();
    assert_eq!(
        (get("schema"), get("layout"), get("seed"), get("areas"), get("start")),
        (s("1"), s("twobox"), s("1"), s(r#"["West","East"]"#), s("07:00"))
    );
    assert!(get("created").parse::<u64>().is_ok(), "{}", get("created"));
    let world: String = c.query_row("SELECT json FROM world WHERE id = 1", [], |r| r.get(0)).unwrap();
    assert_eq!(world, twobox_json());
    drop(st);
    drop(c);
    assert_eq!(snapshot_ticks(&path), [0]);
}

#[test]
fn create_refuses_an_existing_file() {
    let path = temp_save("exists");
    std::fs::write(&path, "").unwrap();
    assert!(matches!(Game::create(&path, &twobox_json(), meta()), Err(GameError::Save(_))));
}

#[test]
fn commands_are_logged_with_player_area_and_tick() {
    let path = temp_save("log");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    join(&mut g, "alice", Some("East"));
    command(&mut g, "alice", PlayerCommand::SwingPoints { points: s("P"), to: PointsPos::Reverse });
    run_to_tick(&mut g, 11);
    drop(g);
    let c = open(&path);
    let mut st = c.prepare("SELECT tick, player, area, command FROM commands ORDER BY seq").unwrap();
    let rows: Vec<(i64, String, String, String)> =
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!((rows[0].0, rows[0].1.as_str(), rows[0].2.as_str()), (0, "alice", "East"));
    let first: Command = serde_json::from_str(&rows[0].3).unwrap();
    assert!(matches!(first, Command::SwingPoints { .. }), "{first:?}");
    let robot: Vec<&(i64, String, String, String)> = rows.iter().filter(|r| r.1 == ROBOT).collect();
    assert!(!robot.is_empty(), "the robot routes 1E01 at tick 10: {rows:?}");
    assert!(robot.iter().all(|r| r.0 == 10 && r.2 == "West"), "{robot:?}");
}

#[test]
fn keeps_the_newest_three_snapshots() {
    let path = temp_save("three");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    for t in [5, 10, 15, 20] {
        run_to_tick(&mut g, t);
        assert!(g.save_now().is_empty());
    }
    drop(g);
    assert_eq!(KEEP_SNAPSHOTS, 3);
    assert_eq!(snapshot_ticks(&path), [10, 15, 20]);
}

#[test]
fn autosaves_every_minute_of_running_time() {
    let path = temp_save("auto");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    run(&mut g, 61.0, 0.1);
    assert_eq!(snapshot_ticks(&path).len(), 2);
    join(&mut g, "alice", Some("West"));
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Pause });
    run(&mut g, 120.0, 1.0);
    assert_eq!(snapshot_ticks(&path).len(), 2, "no snapshots while paused");
}

#[test]
fn resumed_games_start_paused_at_1x_with_no_claims() {
    let path = temp_save("fresh");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    join(&mut g, "alice", Some("West"));
    send(&mut g, "alice", ClientMsg::Vote { proposal: Proposal::Speed { x: 4 } });
    run(&mut g, 2.0, 0.1);
    drop(g);
    let r = Game::resume(&path).unwrap();
    assert!(r.clock().paused);
    assert_eq!(r.clock().speed, 1);
    assert_eq!(r.holder("West"), None);
    assert_eq!(r.meta(), &meta());
}

fn opening(g: &mut Game) {
    join(g, "alice", Some("West"));
    run_to_tick(g, 20);
    command(g, "alice", set_route("W1", ExitName::Signal(s("A"))));
    command(g, "alice", set_route("A", ExitName::Node(s("E"))));
    run_to_tick(g, 300);
}

fn middle(g: &mut Game) {
    command(g, "alice", PlayerCommand::Interpose { berth: s("BW2"), headcode: s("9Z99") });
    run_to_tick(g, 450);
    command(g, "alice", PlayerCommand::Interpose { berth: s("BW1"), headcode: s("8Z88") });
    run_to_tick(g, 555);
}

/// Spec §1 success criterion 2 and §12, in process.
#[test]
fn save_resume_continue_matches_an_uninterrupted_run() {
    let path = temp_save("resume");
    let mut reference = game();
    opening(&mut reference);
    middle(&mut reference);
    run_to_tick(&mut reference, 9000);

    let mut saved = Game::create(&path, &twobox_json(), meta()).unwrap();
    opening(&mut saved);
    assert!(saved.save_now().is_empty());
    middle(&mut saved);
    drop(saved);

    let mut resumed = Game::resume(&path).unwrap();
    assert_eq!(resumed.sim().tick(), 450, "positioned at the last logged tick");
    rejoin(&mut resumed);
    run_to_tick(&mut resumed, 9000);
    assert_eq!(resumed.sim().state_hash(), reference.sim().state_hash());
}

#[test]
fn save_now_right_after_a_command_resumes_exactly() {
    let path = temp_save("exact");
    let mut saved = Game::create(&path, &twobox_json(), meta()).unwrap();
    join(&mut saved, "alice", Some("West"));
    run_to_tick(&mut saved, 40);
    command(&mut saved, "alice", set_route("W1", ExitName::Signal(s("A"))));
    assert!(saved.save_now().is_empty());
    let want = saved.sim().snapshot();
    assert_eq!(want.queue.len(), 1);
    drop(saved);
    let resumed = Game::resume(&path).unwrap();
    assert_eq!(resumed.sim().snapshot(), want);
}

#[test]
fn resume_skips_commands_already_queued_in_the_snapshot() {
    let path = temp_save("queued");
    let w1a = set_route("W1", ExitName::Signal(s("A")));
    let ae = set_route("A", ExitName::Node(s("E")));
    let mut reference = game();
    join(&mut reference, "alice", Some("West"));
    run_to_tick(&mut reference, 5);
    command(&mut reference, "alice", w1a.clone());
    command(&mut reference, "alice", ae.clone());
    run_to_tick(&mut reference, 2000);

    let mut saved = Game::create(&path, &twobox_json(), meta()).unwrap();
    join(&mut saved, "alice", Some("West"));
    run_to_tick(&mut saved, 5);
    command(&mut saved, "alice", w1a.clone());
    assert!(saved.save_now().is_empty());
    command(&mut saved, "alice", ae.clone());
    drop(saved);

    let mut resumed = Game::resume(&path).unwrap();
    let w = resumed.sim().world().clone();
    assert_eq!(resumed.sim().tick(), 5);
    assert_eq!(resumed.sim().snapshot().queue, vec![resolve(&w, &w1a).unwrap(), resolve(&w, &ae).unwrap()]);
    rejoin(&mut resumed);
    run_to_tick(&mut resumed, 2000);
    assert_eq!(resumed.sim().state_hash(), reference.sim().state_hash());
}

#[test]
fn resuming_after_robot_commands_does_not_run_the_robot_twice() {
    let path = temp_save("robot");
    let mut reference = game();
    run_to_tick(&mut reference, 3000);

    let mut saved = Game::create(&path, &twobox_json(), meta()).unwrap();
    run_to_tick(&mut saved, 11);
    let robot = saved.stats().robot_commands;
    assert!(robot > 0, "the robot routes 1E01 at tick 10");
    drop(saved);

    let mut resumed = Game::resume(&path).unwrap();
    assert_eq!(resumed.sim().tick(), 10, "the robot's tick, with its commands queued");
    assert_eq!(resumed.sim().snapshot().queue.len(), robot);
    unpause(&mut resumed);
    run_to_tick(&mut resumed, 3000);
    assert_eq!(resumed.stats().robot_commands, reference.stats().robot_commands - robot);
    assert_eq!(resumed.sim().state_hash(), reference.sim().state_hash());
}

#[test]
fn broken_saves_fail_to_resume() {
    assert!(matches!(resume_err(&temp_save("missing")), GameError::Save(_)));

    let path = temp_save("corrupt");
    drop(Game::create(&path, &twobox_json(), meta()).unwrap());
    open(&path).execute("UPDATE snapshots SET state = '{}'", []).unwrap();
    assert!(matches!(resume_err(&path), GameError::Save(_)));

    let path = temp_save("mismatch");
    drop(Game::create(&path, &twobox_json(), meta()).unwrap());
    let other = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/fixtures/plain_line.json")).unwrap();
    open(&path).execute("UPDATE world SET json = ?1", [other]).unwrap();
    assert!(matches!(resume_err(&path), GameError::Resume(_)));
}

#[test]
fn save_failures_become_error_notices_and_the_game_goes_on() {
    let path = temp_save("fail");
    let mut g = Game::create(&path, &twobox_json(), meta()).unwrap();
    join(&mut g, "alice", Some("West"));
    join(&mut g, "sam", None);
    open(&path).execute("DROP TABLE commands", []).unwrap();
    let out = command(&mut g, "alice", set_route("W1", ExitName::Signal(s("A"))));
    assert_eq!(error_codes(&out, "alice"), [codes::SAVE_FAILED]);
    assert_eq!(error_codes(&out, "sam"), [codes::SAVE_FAILED]);
    g.advance(0.1);
    assert_eq!(g.sim().log().len(), 1, "the command still ran");
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-game --test save`
Expected: compile errors — unresolved `game::save`, `Game::create`, `Game::resume`, `GameError`.

- [ ] **Step 4: Implement `save.rs`**

`crates/game/src/save.rs`:
```rust
//! One SQLite database per game (spec §7): the world, meta, the newest
//! snapshots and every command ever submitted. Only this module does I/O,
//! and only it reads the wall clock (for timestamps the sim never sees).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use signalbox_core::events::Command;
use signalbox_core::sim::{Sim, SimState};
use signalbox_core::world::World;
use signalbox_core::world::file::WorldFile;

use crate::game::{GameMeta, ROBOT};

pub const SAVE_SCHEMA: u32 = 1;
/// Snapshots kept; older ones are deleted.
pub const KEEP_SNAPSHOTS: i64 = 3;

const SCHEMA_SQL: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE world (id INTEGER PRIMARY KEY CHECK (id = 1), json TEXT NOT NULL);
CREATE TABLE snapshots (tick INTEGER PRIMARY KEY, saved_at TEXT NOT NULL, state TEXT NOT NULL);
CREATE TABLE commands (seq INTEGER PRIMARY KEY, tick INTEGER NOT NULL,
                       player TEXT NOT NULL, area TEXT NOT NULL, command TEXT NOT NULL);
";

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("sqlite: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("{0}")]
    Bad(String),
}

/// One row of the command log.
#[derive(Clone, Debug, PartialEq)]
pub struct Logged {
    pub seq: i64,
    pub tick: u64,
    pub player: String,
    pub area: String,
    pub command: Command,
}

/// What a resume needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub meta: GameMeta,
    pub world_json: String,
    /// The newest snapshot.
    pub snapshot: SimState,
    /// Commands logged at or after the snapshot's tick, in `seq` order.
    pub commands_after: Vec<Logged>,
}

pub struct SaveDb {
    conn: Connection,
}

fn now_text() -> String {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0).to_string()
}

fn wal(conn: &Connection) -> Result<(), SaveError> {
    let _mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))?;
    Ok(())
}

impl SaveDb {
    /// A new save file; `path` must not exist yet.
    pub fn create(path: &Path, meta: &GameMeta, world_json: &str) -> Result<SaveDb, SaveError> {
        if path.exists() {
            return Err(SaveError::Bad(format!("{} already exists", path.display())));
        }
        let file: WorldFile = serde_json::from_str(world_json).map_err(|e| SaveError::Bad(format!("world: {e}")))?;
        let conn = Connection::open(path)?;
        wal(&conn)?;
        let areas: Vec<&str> = file.areas.iter().map(|a| a.name.as_str()).collect();
        let areas = serde_json::to_string(&areas).expect("names serialise");
        let now = now_text();
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(SCHEMA_SQL)?;
        for (key, value) in [
            ("schema", SAVE_SCHEMA.to_string()),
            ("layout", meta.layout.clone()),
            ("seed", meta.seed.to_string()),
            ("created", now.clone()),
            ("last_played", now),
            ("areas", areas),
            ("start", file.options.start_time.clone()),
        ] {
            tx.execute("INSERT INTO meta (key, value) VALUES (?1, ?2)", params![key, value])?;
        }
        tx.execute("INSERT INTO world (id, json) VALUES (1, ?1)", params![world_json])?;
        tx.commit()?;
        Ok(SaveDb { conn })
    }

    /// An existing save file.
    pub fn open(path: &Path) -> Result<SaveDb, SaveError> {
        if !path.exists() {
            return Err(SaveError::Bad(format!("no save file {}", path.display())));
        }
        let conn = Connection::open(path)?;
        wal(&conn)?;
        Ok(SaveDb { conn })
    }

    pub fn append_command(&self, tick: u64, player: &str, area: &str, cmd: &Command) -> Result<(), SaveError> {
        let json = serde_json::to_string(cmd).expect("commands serialise");
        self.conn.execute(
            "INSERT INTO commands (tick, player, area, command) VALUES (?1, ?2, ?3, ?4)",
            params![tick as i64, player, area, json],
        )?;
        Ok(())
    }

    pub fn write_snapshot(&self, state: &SimState) -> Result<(), SaveError> {
        let json = serde_json::to_string(state).expect("state serialises");
        let now = now_text();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT OR REPLACE INTO snapshots (tick, saved_at, state) VALUES (?1, ?2, ?3)",
            params![state.tick as i64, now, json],
        )?;
        tx.execute(
            "DELETE FROM snapshots WHERE tick NOT IN (SELECT tick FROM snapshots ORDER BY tick DESC LIMIT ?1)",
            params![KEEP_SNAPSHOTS],
        )?;
        tx.execute("UPDATE meta SET value = ?1 WHERE key = 'last_played'", params![now])?;
        tx.commit()?;
        Ok(())
    }

    pub fn load(&self) -> Result<Saved, SaveError> {
        let meta_value = |key: &str| -> Result<String, SaveError> {
            self.conn
                .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| r.get(0))
                .optional()?
                .ok_or_else(|| SaveError::Bad(format!("meta `{key}` is missing")))
        };
        let schema = meta_value("schema")?;
        if schema != SAVE_SCHEMA.to_string() {
            return Err(SaveError::Bad(format!("unsupported save schema {schema}")));
        }
        let seed = meta_value("seed")?.parse::<u64>().map_err(|e| SaveError::Bad(format!("meta seed: {e}")))?;
        let meta = GameMeta { layout: meta_value("layout")?, seed };
        let world_json: String = self.conn.query_row("SELECT json FROM world WHERE id = 1", [], |r| r.get(0))?;
        let state: Option<String> = self
            .conn
            .query_row("SELECT state FROM snapshots ORDER BY tick DESC LIMIT 1", [], |r| r.get(0))
            .optional()?;
        let state = state.ok_or_else(|| SaveError::Bad("no snapshot".into()))?;
        let snapshot: SimState = serde_json::from_str(&state).map_err(|e| SaveError::Bad(format!("snapshot: {e}")))?;
        let mut st = self.conn.prepare("SELECT seq, tick, player, area, command FROM commands WHERE tick >= ?1 ORDER BY seq")?;
        let rows = st.query_map(params![snapshot.tick as i64], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
        })?;
        let mut commands_after = Vec::new();
        for row in rows {
            let (seq, tick, player, area, json) = row?;
            let command: Command = serde_json::from_str(&json).map_err(|e| SaveError::Bad(format!("command {seq}: {e}")))?;
            commands_after.push(Logged { seq, tick: tick as u64, player, area, command });
        }
        Ok(Saved { meta, world_json, snapshot, commands_after })
    }
}

/// Rebuild a saved game's sim (spec §7.3, amendment 10): restore the
/// snapshot, skip the commands logged at its tick that it already holds in
/// its queue, then replay the rest tick by tick, stopping at the last logged
/// tick with that tick's commands queued. Returns the sim and whether the
/// robot's commands are among those queued (so it must not run again there).
pub fn resume_sim(world: World, snapshot: SimState, commands: &[Logged]) -> Result<(Sim, bool), String> {
    let start = snapshot.tick;
    let mut skip = snapshot.queue.len();
    let mut sim = Sim::restore(world, snapshot)?;
    let mut cmds: Vec<&Logged> = Vec::new();
    for c in commands.iter().filter(|c| c.tick >= start) {
        if c.tick == start && skip > 0 {
            skip -= 1;
            continue;
        }
        cmds.push(c);
    }
    if cmds.windows(2).any(|p| p[1].tick < p[0].tick) {
        return Err("the command log goes back in time".into());
    }
    let Some(last) = cmds.last().map(|c| c.tick) else { return Ok((sim, false)) };
    let mut i = 0;
    loop {
        while i < cmds.len() && cmds[i].tick == sim.tick() {
            sim.submit(cmds[i].command.clone());
            i += 1;
        }
        if sim.tick() >= last {
            break;
        }
        sim.step();
    }
    let robot_ran = cmds.iter().any(|c| c.tick == last && c.player == ROBOT);
    Ok((sim, robot_ran))
}
```

- [ ] **Step 5: Wire saves into `Game`**

In `crates/game/src/lib.rs` add `pub mod save;` after `pub mod notices;`, and change the re-export to:
```rust
pub use game::{GRACE_S, Game, GameError, GameMeta, GameStats, MAX_TICKS_PER_ADVANCE, Out, ROBOT, SNAPSHOT_EVERY_S};
```

In `crates/game/src/game.rs`:

1. Imports — replace
```rust
use std::collections::{BTreeMap, BTreeSet};
```
with
```rust
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
```
replace
```rust
use signalbox_core::world::World;
```
with
```rust
use signalbox_core::world::{LoadError, World};
```
and after `use crate::notices::area_notices;` add
```rust
use crate::save::{SaveDb, SaveError, resume_sim};
```

2. After `pub const MAX_TICKS_PER_ADVANCE: u64 = 800;` add:
```rust
/// Real seconds of running between autosave snapshots.
pub const SNAPSHOT_EVERY_S: f64 = 60.0;

#[derive(Debug, thiserror::Error)]
pub enum GameError {
    #[error("world: {0}")]
    World(#[from] LoadError),
    #[error("save: {0}")]
    Save(#[from] SaveError),
    #[error("resume: {0}")]
    Resume(String),
}
```

3. In `struct Game`, replace
```rust
    robot_ran_at: Option<u64>,
    stats: GameStats,
}
```
with
```rust
    robot_ran_at: Option<u64>,
    stats: GameStats,
    save: Option<SaveDb>,
    /// Real seconds of running since the last snapshot.
    since_snapshot_s: f64,
}
```
and in `from_sim` replace
```rust
            robot_ran_at: None,
            stats: GameStats::default(),
        }
```
with
```rust
            robot_ran_at: None,
            stats: GameStats::default(),
            save: None,
            since_snapshot_s: 0.0,
        }
```

4. After `pub fn new(...) -> Game { ... }` add:
```rust
    /// A new game saved at `path`, which must not exist: world and meta are
    /// written, then a first snapshot at tick 0. Runs at 1x.
    pub fn create(path: &Path, world_json: &str, meta: GameMeta) -> Result<Game, GameError> {
        let world = World::from_json(world_json)?;
        let db = SaveDb::create(path, &meta, world_json)?;
        let mut g = Game::new(world, meta);
        db.write_snapshot(&g.sim.snapshot())?;
        g.save = Some(db);
        Ok(g)
    }

    /// Resume the game saved at `path`: paused at 1x, every area unclaimed.
    pub fn resume(path: &Path) -> Result<Game, GameError> {
        let db = SaveDb::open(path)?;
        let saved = db.load()?;
        let world = World::from_json(&saved.world_json)?;
        let (sim, robot_ran) = resume_sim(world, saved.snapshot, &saved.commands_after).map_err(GameError::Resume)?;
        let mut g = Game::from_sim(sim, saved.meta, true);
        if robot_ran {
            g.robot_ran_at = Some(g.sim.tick());
        }
        g.save = Some(db);
        Ok(g)
    }

    /// Snapshot now and restart the autosave timer. A failure goes to every
    /// connected player as `save_failed`; the game carries on.
    pub fn save_now(&mut self) -> Vec<Out> {
        self.since_snapshot_s = 0.0;
        let Some(db) = &self.save else { return vec![] };
        match db.write_snapshot(&self.sim.snapshot()) {
            Ok(()) => vec![],
            Err(e) => self.save_failed(&e.to_string()),
        }
    }

    fn save_failed(&self, why: &str) -> Vec<Out> {
        self.players
            .iter()
            .filter(|(_, p)| p.connected)
            .map(|(name, _)| error(name, codes::SAVE_FAILED, why))
            .collect()
    }
```

5. Replace the whole `submit` function with:
```rust
    /// Queue a command for the next tick, logging it to the save first;
    /// `player` is `ROBOT` for the robot.
    fn submit(&mut self, player: &str, cmd: Command) -> Vec<Out> {
        let mut out = Vec::new();
        if let Some(db) = &self.save {
            let area = self.map.subject(&cmd).map(|a| self.area_name(a)).unwrap_or_default();
            if let Err(e) = db.append_command(self.sim.tick(), player, &area, &cmd) {
                out = self.save_failed(&e.to_string());
            }
        }
        self.queued.push((player.to_string(), cmd.clone()));
        self.sim.submit(cmd);
        out
    }
```

6. In `advance`, replace
```rust
        for _ in 0..n {
            out.extend(self.tick());
        }
        out
    }
```
with
```rust
        for _ in 0..n {
            out.extend(self.tick());
        }
        if self.save.is_some() && !self.clock.paused {
            self.since_snapshot_s += dt;
            if self.since_snapshot_s >= SNAPSHOT_EVERY_S {
                out.extend(self.save_now());
            }
        }
        out
    }
```

- [ ] **Step 6: Run the tests**

Run: `scripts/cargo test -p signalbox-game`
Expected: PASS (every game test file, including `save`).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/game
git commit -m "feat(game): SQLite saves with autosave and exact resume

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: `bot` crate and the Liverpool Street acceptance soak

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `crates/bot/Cargo.toml`, `crates/bot/src/lib.rs`
- Test: `crates/bot/tests/bot.rs`, `crates/bot/tests/soak.rs`
- Modify: `CLAUDE.md` (crates, commands, architecture)

**Interfaces:**
- Consumes: `protocol::{ClientMsg, ServerMsg, Layout, View, Notice, SeqGap}` (Task 3); in tests `Game`, `GameMeta`, `Out`, `AreaMap`, `to_player_command`, `Game::{create, resume, save_now, view_of, layout_of, holder, stats, clock}` (Tasks 4–9), `robot::{commands, ROBOT_EVERY_TICKS}`, `ts2_import::{convert, areas}`.
- Produces (crate `signalbox-bot`, lib `bot`):
  - `pub struct Bot` (`Clone, Debug, Default`) with `Bot::new() -> Bot`, `receive(&mut self, msg: ServerMsg) -> Option<ClientMsg>` (stores layouts and views; applies deltas; on a sequence gap or a delta before any view, answers `ClientMsg::Resync` once and ignores deltas until the next full view), `layout(&self) -> Option<&Layout>`, `view(&self) -> Option<&View>`, `area(&self) -> Option<&str>`, `resyncs(&self) -> usize`, `take_notices(&mut self) -> Vec<Notice>`.
  - The acceptance soak (spec §1 criteria 1–3 in process; amendment 12 for its cadence).

- [ ] **Step 1: Add the crate skeleton**

Root `Cargo.toml` members:
```toml
members = ["crates/core", "crates/sim-cli", "crates/ts2-import", "crates/protocol", "crates/game", "crates/bot"]
```

`crates/bot/Cargo.toml`:
```toml
[package]
name = "signalbox-bot"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
name = "bot"
path = "src/lib.rs"

[dependencies]
signalbox-protocol = { path = "../protocol" }

[dev-dependencies]
signalbox-core = { path = "../core" }
signalbox-game = { path = "../game" }
ts2-import = { path = "../ts2-import" }
serde_json.workspace = true
```

- [ ] **Step 2: Write the failing bot tests**

`crates/bot/tests/bot.rs`:
```rust
//! The bot keeps its view in step with the game and recovers from a gap.

use std::collections::BTreeMap;

use bot::Bot;
use game::{Game, GameMeta};
use protocol::*;
use signalbox_core::world::World;

fn view(seq: u64) -> View {
    View {
        seq,
        sim_time: 25200.0,
        speed: 1,
        paused: false,
        vote: None,
        holders: BTreeMap::new(),
        score: None,
        signals: BTreeMap::from([("A".to_string(), Aspect::Red)]),
        routes: BTreeMap::new(),
        points: BTreeMap::new(),
        sections: BTreeMap::new(),
        berths: BTreeMap::new(),
    }
}

fn delta(seq: u64, aspect: Aspect) -> ServerMsg {
    ServerMsg::Delta(Delta { seq, signals: BTreeMap::from([("A".to_string(), aspect)]), ..Delta::default() })
}

#[test]
fn applies_views_and_deltas_in_order() {
    let mut b = Bot::new();
    assert_eq!(b.receive(ServerMsg::View(view(1))), None);
    assert_eq!(b.receive(delta(2, Aspect::Green)), None);
    let v = b.view().unwrap();
    assert_eq!((v.seq, v.signals["A"]), (2, Aspect::Green));
    assert_eq!(b.resyncs(), 0);
}

#[test]
fn a_gap_asks_for_one_resync_and_the_next_view_recovers() {
    let mut b = Bot::new();
    b.receive(ServerMsg::View(view(1)));
    assert_eq!(b.receive(delta(3, Aspect::Green)), Some(ClientMsg::Resync));
    assert_eq!(b.receive(delta(4, Aspect::Yellow)), None, "one resync is enough");
    assert_eq!(b.view().unwrap().seq, 1, "the stale view is left alone");
    assert_eq!(b.receive(ServerMsg::View(view(5))), None);
    assert_eq!(b.receive(delta(6, Aspect::Yellow)), None);
    let v = b.view().unwrap();
    assert_eq!((v.seq, v.signals["A"]), (6, Aspect::Yellow));
    assert_eq!(b.resyncs(), 1);
}

#[test]
fn a_delta_before_any_view_asks_for_a_resync() {
    let mut b = Bot::new();
    assert_eq!(b.receive(delta(1, Aspect::Red)), Some(ClientMsg::Resync));
    assert!(b.view().is_none());
    assert_eq!(b.receive(delta(2, Aspect::Red)), None);
}

#[test]
fn notices_are_kept_until_taken() {
    let mut b = Bot::new();
    assert_eq!(b.receive(ServerMsg::Notice(Notice::Replaced)), None);
    assert_eq!(b.take_notices(), vec![Notice::Replaced]);
    assert!(b.take_notices().is_empty());
}

#[test]
fn recovers_from_a_dropped_delta_against_a_real_game() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../game/tests/fixtures/twobox.json")).unwrap();
    let mut g = Game::new(World::from_json(&text).unwrap(), GameMeta { layout: "twobox".into(), seed: 1 });
    let mut b = Bot::new();
    for (_, m) in g.connect("alice") {
        assert_eq!(b.receive(m), None);
    }
    for (_, m) in g.handle("alice", ClientMsg::Claim { area: "West".into() }) {
        assert_eq!(b.receive(m), None);
    }
    assert_eq!(b.area(), Some("West"));
    g.advance(1.0);
    let _lost = g.flush();
    g.advance(1.0);
    let mut replies = Vec::new();
    for (_, m) in g.flush() {
        replies.extend(b.receive(m));
    }
    assert_eq!(replies, vec![ClientMsg::Resync]);
    for r in replies {
        for (_, m) in g.handle("alice", r) {
            assert_eq!(b.receive(m), None);
        }
    }
    assert_eq!(b.view(), g.view_of("alice").as_ref());
    assert_eq!(b.layout(), g.layout_of("alice").as_ref());
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `scripts/cargo test -p signalbox-bot --test bot`
Expected: compile error — `crates/bot/src/lib.rs` missing.

- [ ] **Step 4: Implement the bot**

`crates/bot/src/lib.rs`:
```rust
//! A headless signalbox client (spec §2.1): keeps the layout and view a game
//! sends, applies deltas, and asks for one resync when a delta goes missing.
//! In C1 it talks to an in-process `Game`; C2 puts a WebSocket in between.

use protocol::{ClientMsg, Layout, Notice, ServerMsg, View};

#[derive(Clone, Debug, Default)]
pub struct Bot {
    layout: Option<Layout>,
    view: Option<View>,
    /// A resync was asked for; deltas are ignored until the next full view.
    awaiting_resync: bool,
    notices: Vec<Notice>,
    resyncs: usize,
}

impl Bot {
    pub fn new() -> Bot {
        Bot::default()
    }

    /// Take one message from the game; returns what to send back, if anything.
    pub fn receive(&mut self, msg: ServerMsg) -> Option<ClientMsg> {
        match msg {
            ServerMsg::Layout(l) => {
                self.layout = Some(l);
                None
            }
            ServerMsg::View(v) => {
                self.view = Some(v);
                self.awaiting_resync = false;
                None
            }
            ServerMsg::Delta(d) => {
                if self.awaiting_resync {
                    return None;
                }
                let applied = match self.view.as_mut() {
                    Some(v) => v.apply(&d).is_ok(),
                    None => false,
                };
                if applied {
                    return None;
                }
                self.awaiting_resync = true;
                self.resyncs += 1;
                Some(ClientMsg::Resync)
            }
            ServerMsg::Notice(n) => {
                self.notices.push(n);
                None
            }
        }
    }

    pub fn layout(&self) -> Option<&Layout> {
        self.layout.as_ref()
    }

    pub fn view(&self) -> Option<&View> {
        self.view.as_ref()
    }

    /// The area this bot holds, from its layout.
    pub fn area(&self) -> Option<&str> {
        self.layout.as_ref()?.area.as_deref()
    }

    /// How many resyncs this bot has asked for.
    pub fn resyncs(&self) -> usize {
        self.resyncs
    }

    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }
}
```

- [ ] **Step 5: Run the bot tests**

Run: `scripts/cargo test -p signalbox-bot --test bot`
Expected: PASS.

- [ ] **Step 6: Write the acceptance soak**

`crates/bot/tests/soak.rs`:
```rust
//! Two bots and the robot play Liverpool Street through `Game`, with a
//! spectator watching (spec §1 success criteria 1–3, in process; C2 repeats
//! it through the real front and game processes).
//!
//! Honest label: bot decisions come from the robot signaller's logic reading
//! the game's sim (`robot::commands(game.sim())`), filtered to the bot's area
//! and sent by name through `Game::handle`, so area checks, command logging
//! and notices are exercised. A strategy that reads only the bot's own view
//! is future work.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bot::Bot;
use game::areas::AreaMap;
use game::names::to_player_command;
use game::{Game, GameMeta, Out};
use protocol::{ClientMsg, Notice, PlayerCommand, Proposal, ServerMsg};
use signalbox_core::robot::{self, ROBOT_EVERY_TICKS};
use signalbox_core::world::World;

const SPEED: u8 = 8;
/// At 8x, 0.125 s of real time is exactly one robot period (10 ticks), so the
/// bots decide on the very state the game's robot sees (amendment 12).
const DT: f64 = 0.125;

fn liverpool_json() -> String {
    let dir = env!("CARGO_MANIFEST_DIR");
    let ts2 = std::fs::read_to_string(format!("{dir}/../ts2-import/tests/data/liverpool-st.json")).unwrap();
    let text = std::fs::read_to_string(format!("{dir}/../../layouts/liverpool-st.areas.json")).unwrap();
    let mut w = ts2_import::convert(&ts2).unwrap().world;
    ts2_import::areas::apply(&mut w, &ts2_import::areas::parse(&text).unwrap()).unwrap();
    serde_json::to_string(&w).unwrap()
}

fn meta() -> GameMeta {
    GameMeta { layout: "liverpool-st".into(), seed: 7 }
}

fn temp_save(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("signalbox-bot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.sqlite"));
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
    path
}

/// A game and its clients, wired together in process.
struct Table {
    game: Game,
    map: AreaMap,
    bots: BTreeMap<String, Bot>,
    periods: u64,
    handovers: usize,
    not_your_area: usize,
    /// Skip the bots' first decision: a resumed game already holds it queued.
    skip_decide: bool,
}

impl Table {
    fn new(game: Game) -> Table {
        let map = AreaMap::new(game.sim().world());
        Table { game, map, bots: BTreeMap::new(), periods: 0, handovers: 0, not_your_area: 0, skip_decide: false }
    }

    /// Hand the game's messages to the bots, and the bots' answers back.
    fn deliver(&mut self, mut outs: Vec<Out>) {
        while !outs.is_empty() {
            let mut replies: Vec<(String, ClientMsg)> = Vec::new();
            for (name, msg) in outs {
                match &msg {
                    ServerMsg::Notice(Notice::Handover { .. }) => self.handovers += 1,
                    ServerMsg::Notice(Notice::NotYourArea { .. }) => self.not_your_area += 1,
                    _ => {}
                }
                if let Some(bot) = self.bots.get_mut(&name) {
                    replies.extend(bot.receive(msg).map(|r| (name.clone(), r)));
                }
            }
            outs = replies.into_iter().flat_map(|(name, m)| self.game.handle(&name, m)).collect();
        }
    }

    fn join(&mut self, name: &str, area: Option<&str>) {
        self.bots.insert(name.to_string(), Bot::new());
        let outs = self.game.connect(name);
        self.deliver(outs);
        if let Some(a) = area {
            self.send(name, ClientMsg::Claim { area: a.to_string() });
        }
    }

    fn send(&mut self, name: &str, msg: ClientMsg) {
        let outs = self.game.handle(name, msg);
        self.deliver(outs);
    }

    /// Every bot holding an area votes for `proposal`.
    fn vote_all(&mut self, proposal: Proposal) {
        let holders: Vec<String> = self.bots.iter().filter(|(_, b)| b.area().is_some()).map(|(n, _)| n.clone()).collect();
        for h in holders {
            self.send(&h, ClientMsg::Vote { proposal });
        }
    }

    /// Each holding bot sends the robot's commands for its own area, by name.
    fn decide(&mut self) {
        let world = self.game.sim().world();
        let cmds = robot::commands(self.game.sim());
        let mut sends: Vec<(String, PlayerCommand)> = Vec::new();
        for (name, bot) in &self.bots {
            let Some(area) = bot.area().and_then(|a| world.net.area(a)) else { continue };
            for c in cmds.iter().filter(|c| self.map.subject(c) == Some(area)) {
                sends.push((name.clone(), to_player_command(world, c)));
            }
        }
        for (name, cmd) in sends {
            self.send(&name, ClientMsg::Command { cmd });
        }
    }

    /// One robot period; every second one flushes (4 Hz) and checks every
    /// client's delta-built view and layout against a fresh full one.
    fn period(&mut self) {
        assert_eq!(self.game.sim().tick() % ROBOT_EVERY_TICKS, 0, "periods start on robot ticks");
        if !std::mem::take(&mut self.skip_decide) {
            self.decide();
        }
        let outs = self.game.advance(DT);
        self.deliver(outs);
        self.periods += 1;
        if self.periods % 2 == 0 {
            let outs = self.game.flush();
            self.deliver(outs);
            for (name, bot) in &self.bots {
                assert_eq!(bot.view(), self.game.view_of(name).as_ref(), "{name}'s view at tick {}", self.game.sim().tick());
                assert_eq!(bot.layout(), self.game.layout_of(name).as_ref(), "{name}'s layout");
            }
        }
    }

    fn run_to(&mut self, tick: u64) {
        while self.game.sim().tick() < tick {
            self.period();
        }
    }
}

/// Two bots hold Liverpool Street and Hackney & Bow, a spectator watches,
/// and the robot keeps Bethnal Green.
fn seat(game: Game) -> Table {
    let mut t = Table::new(game);
    t.join("ann", Some("Liverpool Street"));
    t.join("hal", Some("Hackney & Bow"));
    t.join("sam", None);
    t
}

fn play_liverpool(minutes: u64) {
    let mut t = seat(Game::new(World::from_json(&liverpool_json()).unwrap(), meta()));
    t.vote_all(Proposal::Speed { x: SPEED });
    assert_eq!(t.game.clock().speed, SPEED);
    t.run_to(minutes * 600);
    let s = t.game.stats().clone();
    assert_eq!((s.spads, s.collisions, s.invariant_violations), (0, 0, 0), "{s:?}");
    assert!(s.player_commands > 0 && s.robot_commands > 0, "{s:?}");
    assert_eq!(t.not_your_area, 0, "bots only work their own areas");
    assert!(t.handovers >= 1, "no handover notice in {minutes} minutes");
    assert_eq!(t.game.holder("Bethnal Green"), None);
    assert!(t.bots.values().all(|b| b.resyncs() == 0), "in process, no delta is ever lost");
}

/// The first handovers into the bots' areas come at about 13 and 15 sim
/// minutes (prototype run), so 20 minutes sees at least one.
#[test]
fn liverpool_street_twenty_minutes_with_two_bots() {
    play_liverpool(20);
}

/// Three sim-hours at 8x. Slow in debug builds: run with
/// `scripts/cargo test --release -p signalbox-bot --test soak -- --ignored`.
#[test]
#[ignore]
fn liverpool_street_three_hours_with_two_bots() {
    play_liverpool(180);
}

/// Spec §1 success criterion 2 on the real layout: save at 2 minutes, drop
/// at 4, resume, and at 6 minutes the state matches an uninterrupted run.
#[test]
fn liverpool_street_save_and_resume_match_an_uninterrupted_run() {
    let json = liverpool_json();
    let mut reference = seat(Game::new(World::from_json(&json).unwrap(), meta()));
    reference.vote_all(Proposal::Speed { x: SPEED });
    reference.run_to(3600);

    let path = temp_save("liverpool");
    let mut saved = seat(Game::create(&path, &json, meta()).unwrap());
    saved.vote_all(Proposal::Speed { x: SPEED });
    saved.run_to(1200);
    assert!(saved.game.save_now().is_empty());
    saved.run_to(2400);
    drop(saved);

    let game = Game::resume(&path).unwrap();
    let at = game.sim().tick();
    assert!((1200..=2400).contains(&at), "resumed at {at}");
    let queued = !game.sim().snapshot().queue.is_empty();
    let mut resumed = seat(game);
    resumed.skip_decide = queued;
    resumed.vote_all(Proposal::Resume);
    resumed.vote_all(Proposal::Speed { x: SPEED });
    assert!(!resumed.game.clock().paused);
    resumed.run_to(3600);
    assert_eq!(resumed.game.sim().tick(), reference.game.sim().tick());
    assert_eq!(resumed.game.sim().state_hash(), reference.game.sim().state_hash());
}
```

Why skipping the bots' first decision after a resume is right: a resumed game stands at its last logged tick `L` with that tick's commands queued. If anything is queued, the bots' decision at `L` (if they had one) is already in it; if they had none, deciding again on the same state gives nothing anyway. The game's own robot does the same through `robot_ran_at`.

- [ ] **Step 7: Run the soaks**

Run: `scripts/cargo test -p signalbox-bot`
Expected: PASS (`bot` and the two non-ignored soak tests).

Run: `scripts/cargo test --release -p signalbox-bot --test soak -- --ignored`
Expected: PASS.

If a soak fails, use superpowers:systematic-debugging. A view mismatch is a bug in `diff`/`apply` or in `build_view`/`Visibility` (reproduce it on `twobox` in `crates/game/tests/game.rs` first). A SPAD or collision means the bots and robot did not issue what one robot call would: check that every period starts on a multiple of `ROBOT_EVERY_TICKS` and that `AreaMap::subject` gives every robot command exactly one area. Never loosen an assertion, shorten the run, or change seeds or layout data to make it pass; stop and report if a failure needs a design decision.

- [ ] **Step 8: Update CLAUDE.md**

In `CLAUDE.md`, in the `## Commands` code block, after the `scripts/cargo test --release -p ts2-import --test soak -- --ignored` line add:
```bash
scripts/cargo test -p signalbox-game                          # game library (twobox fixture, saves in a temp dir)
scripts/cargo test --release -p signalbox-bot --test soak -- --ignored   # 3 h Liverpool St: two bots + robot
```
and after the `scripts/cargo run -p ts2-import -- crates/ts2-import/tests/data/drain.json -o /w/target/drain.json` line add:
```bash
scripts/cargo run -p ts2-import -- crates/ts2-import/tests/data/liverpool-st.json -o /w/target/lst.json --areas /w/layouts/liverpool-st.areas.json
```

Replace the paragraph
```
Workspace crates: `crates/core` (library `signalbox-core`), `crates/sim-cli`
(headless run/replay), `crates/ts2-import` (TS2 → signalbox converter, lib + CLI).
```
with
```
Workspace crates: `crates/core` (library `signalbox-core`), `crates/sim-cli`
(headless run/replay), `crates/ts2-import` (TS2 → signalbox converter, lib + CLI),
`crates/protocol` (`signalbox-protocol`: wire messages, views, deltas),
`crates/game` (`signalbox-game`: the multiplayer game library + SQLite saves),
`crates/bot` (`signalbox-bot`: headless client). The multiplayer design is
`docs/superpowers/specs/2026-09-30-server-and-protocol-design.md`.
```

At the end of the `## Architecture` section (before `### Tests`), add:
```markdown
### Multiplayer (`protocol`, `game`, `bot`)
- Areas for converted layouts come from `layouts/<name>.areas.json`, applied by
  `ts2-import --areas`: each area floods the section graph from its seeds and
  stops at nodes holding boundary signals (`ts2_import::areas`).
- `game::Game` is pure: `connect`/`handle`/`advance(real_dt)`/`flush` return
  `(player, ServerMsg)` pairs. It maps every command to its subject's area
  (`game::areas::AreaMap`), refuses commands outside the sender's area, and runs
  `robot::commands` every `ROBOT_EVERY_TICKS` for areas nobody holds.
- Views are built from sim state per player (`game::view::build_view`) — own area
  plus a fringe walked along the track to the first signal — and sent as deltas
  (`protocol::diff`, `View::apply`); a client that sees a `seq` gap resyncs.
- Wire commands carry names (`protocol::PlayerCommand`); the save logs core
  `Command`s with ids, since the world is copied into each save.
- Resume (`game::save::resume_sim`) restores the newest snapshot, skips commands
  already in its queue, and replays the log up to the last logged tick, leaving
  that tick's commands queued (and the robot marked as run if it logged there).
```

- [ ] **Step 9: Run everything and commit**

Run: `scripts/cargo test`
Expected: PASS across the workspace.

```bash
git add Cargo.toml Cargo.lock crates/bot CLAUDE.md
git commit -m "feat(bot): headless client and the Liverpool Street two-bot soak

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

## Spec coverage (self-review)

| Spec | Where |
|---|---|
| §1 criterion 1 (two bots + robot on Liverpool St, no SPAD/collision/invariant, delta views equal full views) | Task 10 (in process; through real processes in C2) |
| §1 criterion 2 (save, kill, resume → same state hash) | Task 9 (`save_resume_continue_matches_an_uninterrupted_run` and the three Review Focus tests), Task 10 (Liverpool) |
| §1 criterion 3 (nothing outside your area; spectators and robot never vote) | Task 8 (`commands_outside_your_area_never_reach_the_sim`, `the_clock_follows_votes`), Task 7 (`only_holders_vote_and_only_listed_speeds`), Task 8 (`robot_is_a_reserved_name`) |
| §1 criteria 4–5 (crash isolation, Authentik) | C2 |
| §2.1 crates `protocol`, `game` (lib), `bot` | Tasks 3, 4–9, 10 (`game` binary, `ipc`, `server` → C2) |
| §3.2–3.3 in-game messages, `seq` gap → resync | Task 3 (types, golden JSON), Task 8 (resync), Task 10 (bot) |
| §3.4 area checks and rejections | Tasks 4 (`AreaMap::subject`), 8 |
| §3.5 votes | Task 7, Task 8 (grace expiry, claims/releases settle) |
| §3.6 identity / duplicates | Front's job (C2); `Game::connect` resyncs a known player (amendment 8) |
| §4.1 layout, §4.2 view, §4.3 deltas, §4.5 fringe | Tasks 3, 4, 5, 8 (`every_client_rebuilds_the_servers_view_from_deltas`); outbound queue bound → C2 relay |
| §4.4 notices (`rejected`, `not_your_area`, `spad`, `collision`, `late`, `wrong_platform`, `handover`, `area_taken`, `error`) | Tasks 6, 8; `replaced`/`game_crashed` are defined (Task 3) for C2 to send |
| §5 area files, hard errors, Liverpool St (+ Drain, Gretz) | Task 2 |
| §6.1 `Game` API, robot filter, grace, handover | Task 8 |
| §6.2 core additions (`state_hash`; area lookup in `game`) | Tasks 1, 4 |
| §7.1 schema, WAL, bundled SQLite | Task 9 |
| §7.2 writes (world+meta once, each command, snapshot per 60 s and on demand, newest 3) | Task 9 (snapshots on going empty / `Shutdown` / exit are the C2 binary calling `save_now`) |
| §7.3 resume (snapshot + replay, paused at 1x, no claims) | Task 9 |
| §11 save errors → `save_failed`, resume failures → error | Task 9 |
| §12 protocol round-trips, game tests, `--areas` tests | Tasks 2, 3, 4–9 |

Deliberately deferred to C2 (amendment 1): the `game` binary and its socket loop, `Status`/`Saved` ipc messages, snapshots on going empty / `Shutdown` / clean exit (the binary calls `Game::save_now`), the 10-minute empty-game exit, the 64-message outbound queue (the relay calls `Game::resync`), duplicate-connection `replaced`, lobby start time (`create_game {start?}`), crash handling, auth and deployment. Not in the `Layout` yet: the world's diagram geometry (`WorldFile::layout`), which the browser client (D) will need.
