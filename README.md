# signalbox

A deterministic railway signalling simulation in Rust, modelled on UK practice.
It started as a rewrite of [TS2](https://github.com/ts2/ts2), the abandoned
train-signalling game, and is aimed at multiplayer signal boxes played in the
browser: several players each running a box on one shared network, handing
trains to each other.

**Status:** playable. A server runs several games at once, each in its own
process with its own SQLite save; players sign in and work signal boxes from
the browser, and a robot signaller runs every box nobody holds. Three real
layouts ship (Liverpool Street, the Waterloo & City "Drain" and
Gretz-Armainvilliers), plus four interactive tutorial lessons.

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

A robot signaller sets routes for every train it is responsible for, planning
each train's whole journey so it keeps booked lines and platforms. It plays
every area nobody holds, and the test suite uses it to soak whole layouts for
hours of simulated time, checking there are no SPADs, collisions, stuck trains
or wrong platforms, and that lateness stays within bounds.

## Playing

- **Lobby:** create a game on a layout (optionally choosing a seed, a start time
  and the area you want to signal), join a running one, resume a saved one, or
  start a tutorial. A game created with a start later than its timetable's is
  first run up to that time by the robot ("Preparing…"), so trains are where
  they should be. Saved and crashed games can be deleted by their creator or
  an admin.
- **The panel:** a UK IECC/Westcad-style VDU — joint gaps between track circuits,
  white routes and overlaps, red occupation, discs on hooked posts with box and
  workstation prefixes (`LA121`), cyan headcodes in the train describer,
  ○A auto-working buttons, named lines with direction arrows, and a fringe drawn
  hollow. Signals show red/green as on a real panel, or real aspects as a
  setting.
- **Working:** click an entrance then an exit to set a route; cancel, swing
  points, interpose or cancel headcodes from menus. Refused commands say why.
  The simplifier lists your area's working timetable; a headcode enquiry shows a
  train's booked calls and lateness.
- **Multiplayer:** one area per player, spectators welcome, trains handed over
  between areas; the clock (pause, 1–8×) changes only when every voter agrees.
  Games autosave and resume exactly where they were.
- **Tutorials:** four lessons (reading the panel; setting and cancelling routes;
  running trains; junctions, auto-working and handovers), played in a private
  game with highlights on the panel. Every lesson is played through in CI.

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

Run the server and the browser client locally (the `dev-auth` feature adds a
`/auth/dev?user=<name>` sign-in for testing; never use it in production):

```bash
scripts/wasm-build                                   # the browser client into target/web-dist/
cargo run -p signalbox-server --features dev-auth    # see deploy/README.md for its environment
```

Deploying for real (Docker, OpenID Connect sign-in, secrets, the optional
Waterloo & City timetable) is described in `deploy/README.md`.

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
- `crates/game` — a game: the sim plus areas, players, visibility, votes,
  saves (SQLite), late-start seeding and the tutorial lesson runner
- `crates/protocol` — the messages between browser, front and game
- `crates/server` — `signalbox-server` (the front: web, sign-in, lobby, one
  process per game) and `signalbox-game` (a game process)
- `crates/ipc` — framing between the front and game processes
- `crates/client-core`, `crates/client-ui`, `crates/client-web` — the client:
  state, the egui panel, and the WebAssembly/WebGPU (WebGL2 fallback) shell
- `crates/bot` — scripted players used by soaks and end-to-end tests
- `crates/sim-cli` — headless runner: `run` (optionally with the robot
  signaller, recording a command log) and `replay`
- `crates/ts2-import` — TS2 → signalbox converter (library and CLI), including
  a reader for the Waterloo & City working timetable; vendored ts2-data
  (GPL-2.0) under `tests/data`
- `layouts/` — per-layout area, prefix and line-name files; `lessons/` — the
  tutorial lessons; `deploy/` — the Docker image, compose file and checks
- `docs/superpowers/specs` — the design; `docs/superpowers/plans` — the
  step-by-step implementation plans it was built from

## Roadmap

Next: a polish pass on the browser panel (labels that never overlap, clearer
lobby and controls), chat and shared notes, then **Signals v2** — Train Ready
To Start, signal-post telephones with the Rule Book's instructions, faults and
failures, reminders and emergency replacement, ARS and sound — and real UK
areas imported from OpenStreetMap. A native desktop client comes later.
Further designs are written for box-to-box communication, shunt and
calling-on routes, train operations, real timetables and scenarios, replays,
accessibility, NX panels and level crossings.

## Contributing

The primary repository and CI live on the author's self-hosted Forgejo; the
GitHub repository is a public mirror without CI. Issues and pull requests on
GitHub are welcome. Before sending changes, run `cargo test` — CI builds the
workspace with warnings as errors.

## License

GPL-2.0-or-later, the same as TS2.
