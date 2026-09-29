# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

signalbox is a deterministic UK railway signalling simulation in Rust (a rewrite of
the TS2 game), headed for multiplayer signal boxes in the browser. The design is in
`docs/superpowers/specs/2026-09-29-core-and-converter-design.md`; the task-by-task
implementation plans it was built from are in `docs/superpowers/plans/`.

## Commands

Use `scripts/cargo` in place of `cargo` where no local toolchain is installed (it
runs the official Rust image in Docker with the repo mounted at `/w`; it does **not**
forward environment variables, and the image has no clippy or rustfmt).

```bash
scripts/cargo test                                            # everything
scripts/cargo test -p signalbox-core --test release           # one test file
scripts/cargo test -p signalbox-core --test release cancel_keeps_section_under_train   # one test
scripts/cargo test --release -p ts2-import --test soak -- --ignored   # slow Liverpool St soak
scripts/ci/test.sh                                            # the CI gate (native cargo, offline, -D warnings)

scripts/cargo run -p sim-cli -- run crates/core/tests/fixtures/junction.json --robot --hours 1 --record /w/target/log.json
scripts/cargo run -p sim-cli -- replay crates/core/tests/fixtures/junction.json /w/target/log.json
scripts/cargo run -p ts2-import -- crates/ts2-import/tests/data/drain.json -o /w/target/drain.json
```

Paths passed through `scripts/cargo` resolve inside the container (`/w` = repo root).

CI (`.forgejo/workflows/ci.yml`) runs on a self-hosted Forgejo runner that builds
**offline** with warnings as errors; adding a crate dependency means the runner's
cargo cache must be reseeded by the maintainer. The GitHub repo is a mirror without CI.

Converter warning snapshots live in `crates/ts2-import/tests/expected/`. To re-record
after an intended change, run the `convert` test with `UPDATE_EXPECTED=1` set inside
the container (e.g. `docker run ... -e UPDATE_EXPECTED=1 rust:1.98-slim-bookworm cargo test -p ts2-import --test convert`), then review the diff.

## Architecture

Workspace crates: `crates/core` (library `signalbox-core`), `crates/sim-cli`
(headless run/replay), `crates/ts2-import` (TS2 → signalbox converter, lib + CLI).

### World vs state
- `world::World` is static: loaded from a JSON `WorldFile` (`world/file.rs`, names
  everywhere), resolved to typed arena ids (`ids.rs`) and validated in
  `world/load.rs`. Invalid worlds are rejected with `LoadError`, never at runtime.
- `sim::SimState` is everything dynamic and is serde-serialisable; `Sim` owns a
  `World`, a `SimState`, the RNG and a derived `Occupancy` rebuilt every tick.
  `snapshot`/`restore`/`replay` depend on all mutable state living in `SimState`.

### The tick (`Sim::step`, 100 ms of sim time)
Order matters and tests depend on it: apply queued commands (logged with their tick)
→ points movement → timetable entries → `Interlocking::update` (locking, sectional
release, overlap/cancel timers) → `refresh_aspects` → move trains (driver model,
signal passes/SPADs, calls, describer, exits) → rebuild occupancy → detect
collisions → scoring. Interlocking and the driver therefore see start-of-tick
occupancy and aspects.

### Determinism rules (replay must be bit-exact)
No `HashMap`/`HashSet` in sim or converter logic (use `BTreeMap`/`BTreeSet`/`Vec`),
no wall clock (sim time = `start_s + tick * TICK_S`), one seeded `ChaCha8Rng`
drawn in a fixed order, everything processed in index order. `serde_json` has
`float_roundtrip` on so snapshots survive JSON.

### Interlocking model (`interlocking.rs`)
- Each track section has at most one owner: `Owner::Path(route)` or
  `Owner::Overlap(route)`. Setting a route claims its path and overlap sections;
  a route may take over the overlap of the route it continues (whose exit is its
  entrance). Points are locked implicitly by their section being owned.
- Per-route `progress` per path section (`Untouched → Occupied → Released`) drives
  sectional release in running order; out-of-sequence occupation releases nothing.
- A signal shows proceed only if `proceed()` holds (route locked, not cancelled,
  path untouched and clear, overlap held and clear, points detected). Aspects are
  computed recursively from the exit signal (`aspect::cleared_aspect`).
- Cancel = approach locking with route holding: sections under or ahead of a train
  on the route stay locked.

### Routes are validated against the track
`Network::trace_route` (in `network.rs`) walks from an entrance signal with the
route's points positions and reports the real path, exit and overlap. The loader
requires a route's declared path/exit/overlap/points to match it exactly, so routes
are always **signal to signal** (exit = the first same-direction signal) and every
signal used by a route must stand on a section boundary. Trailing points must be
listed too (at the leg the route runs over). The converter reuses the same tracer.

### Trains and the robot
`trains.rs` keeps each train as a deque of `(segment, dir)` from tail to head plus
`head_m`; `advance` returns every swept stretch so signals and platforms passed in
one tick are all seen (half-open `(from, to]`). `driver.rs` reads signals within
sighting distance and expects the rest from the last aspect passed (a new train
expects red). `robot.rs` is a deterministic auto-signaller; `robot::soak` runs a
world under it and is the integration oracle (no SPADs, collisions, invariant
violations or stuck trains).

### TS2 converter (`crates/ts2-import`)
Pipeline in `lib.rs::convert`: parse TS2 JSON (`ts2.rs`) → `graph::build` (TS2's
item chain → nodes/segments/sections; points get 1 m legs, touching zero-length
items get 1 m spacers, sections break at signals and points, flat crossings share a
section) → `timetable::build` → load the world without routes → `routes::build`
(walk each TS2 route, split at signals via `trace_route`, merge, generate routes
for signals TS2 never started one from, overlaps ≤ 180 m) → `routes::finish` (drop
any route the loader rejects, with a warning). Anything the target can't express is
a `report::Report` warning; the output always passes `World::from_file`. Generated
names are derived from TS2 ids (`L<tiId>`, `N<tiId>`, `P<tiId><p|n|r>`, `T1..`), so
output is byte-identical for the same input.

### Tests
- Core fixtures: `crates/core/tests/fixtures/{plain_line,terminus,junction}.json`.
  `tests/common/mod.rs` has `load_with(name, |json| ...)` for mutating a fixture
  per test, `Rig` for interlocking-only tests, and `run_until`/`count` for sim tests.
- Converter data: the real ts2-data layouts (GPL-2.0) plus a hand-made `mini.json`
  in `crates/ts2-import/tests/data/`. Facts about the TS2 format that the converter
  relies on are asserted by tests against these files.
