# signalbox

A deterministic railway signalling simulation in Rust, modelled on UK practice.
It started as a rewrite of [TS2](https://github.com/ts2/ts2), the abandoned
train-signalling game, and is aimed at multiplayer signal boxes played in the
browser: several players each running a box on one shared network, handing
trains to each other.

**Status:** simulation core and TS2 converter. Real TS2 layouts
([ts2-data](https://github.com/ts2/ts2-data)) convert and run. There is no user interface yet — you drive the
simulation from code or the headless `sim-cli`.

## What it models

- **Track:** segments, points, buffer stops and boundaries, grouped into track
  sections (track circuits).
- **Interlocking:** entrance–exit route setting with conflict checks, overlaps,
  route locking with sectional release, approach locking on cancel, auto-working
  and automatic signals, and points that cannot move under a train or a locked
  route.
- **Signals:** 2-, 3- and 4-aspect colour lights (red, yellow, double yellow,
  green) derived from the signal ahead; signals replaced to red by the passing
  train; SPAD detection.
- **Trains:** per-type acceleration and braking, line speeds, a driver who reads
  signals within sighting distance and brakes to stop short of reds and at
  platforms.
- **Timetables:** services with calls, dwell times, late running, wrong-platform
  detection, and trains that form their next service or stable at the end.
- **Train describer:** headcodes stepping from berth to berth, with manual
  interpose and cancel.
- **Determinism:** fixed 100 ms ticks, commands in and events out, one seeded
  RNG. A session is its seed plus a command log, and replays exactly; the full
  state can be snapshotted and restored.

A robot signaller sets routes for every train, which is how the test suite soaks
whole layouts for hours of simulated time and checks there are no SPADs or
collisions.

## Getting started

You need Rust 1.85 or newer (edition 2024); it is developed on 1.98.

```bash
cargo test
cargo run -p sim-cli -- run crates/core/tests/fixtures/junction.json --robot --hours 1
```

`run` prints a report (trains entered, exited, SPADs, collisions, penalty
points). Add `--record session.json` to save the command log, then replay it:

```bash
cargo run -p sim-cli -- replay crates/core/tests/fixtures/junction.json session.json
```

Both commands print a hash of the final state, so you can check a replay matches.

Convert a TS2 simulation and run it:

```bash
cargo run -p ts2-import -- crates/ts2-import/tests/data/drain.json -o target/drain.json
cargo run -p sim-cli -- run target/drain.json --robot --hours 2
```

No local Rust? `scripts/cargo` runs cargo in the official Rust Docker image
(set `SIGNALBOX_RUST_IMAGE` to use another), e.g. `scripts/cargo test`.

## Worlds

A world is a JSON file describing the track, signals, routes, platforms, train
types, services and when trains appear. The three small layouts in
`crates/core/tests/fixtures/` (a plain line, a terminus and a junction) are the
easiest place to see the format; the full definition is in
`crates/core/src/world/file.rs`, and every world is validated on load.

## Repository layout

- `crates/core` — the simulation library (`signalbox-core`)
- `crates/sim-cli` — headless runner: `run` (optionally with the robot
  signaller, recording a command log) and `replay`
- `crates/ts2-import` — TS2 → signalbox converter (library and CLI); vendored
  ts2-data (GPL-2.0) under `tests/data`
- `docs/superpowers/specs` — the design; `docs/superpowers/plans` — the
  step-by-step implementation plans it was built from

## Roadmap

1. A server and network protocol for several players, each owning a signal box.
2. A browser client: the signaller's panel.
3. Real timetables from UK open rail data.
4. More UK signalling: calling-on and shunt signals, permissive working,
   approach release, flashing yellows.

## Contributing

The primary repository and CI live on the author's self-hosted Forgejo; the
GitHub repository is a public mirror without CI. Issues and pull requests on
GitHub are welcome. Before sending changes, run `cargo test` — CI builds the
workspace with warnings as errors.

## License

GPL-2.0-or-later, the same as TS2.
