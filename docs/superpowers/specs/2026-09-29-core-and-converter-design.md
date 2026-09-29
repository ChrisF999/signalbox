# signalbox — sim core + TS2 converter design

Date: 2026-09-29
Status: draft for review
Working name: `signalbox` (rename is cheap until the first release)
License: GPL-2.0-or-later (same as TS2, whose format and behaviour we reimplement)

## 1. Context and goal

signalbox is a Rust railway signalling simulation, started as a port of
[TS2](https://github.com/ts2/ts2) (Python/PyQt5 client) and its Go simulation
server [ts2-sim-server](https://github.com/ts2/ts2-sim-server). Both are
effectively abandoned (server master last changed 2021).

The long-term product is **multiplayer UK signalling**: several players each run
a signal box on one shared network, handing trains to each other, playable in a
browser. Real timetables from Darwin data are a later goal.

That is split into sub-projects:

| # | Sub-project | This spec? |
|---|---|---|
| A | Sim core (Rust library): network, interlocking, trains, timetables, describer | **yes** |
| B | TS2 converter: import ts2-data `.ts2`/JSON simulations | **yes** |
| C | Server + new multiplayer protocol (areas, handovers, permissions) | no |
| D | Browser client (signaller's panel) | no |
| E | Darwin timetable import | no |

Order: A + B, then C + D (first playable multiplayer), then E.

### Decisions already made

- **Port and improve**, not a behaviour-identical port. The Go server is a
  reference, not an oracle.
- **Free to break** TS2's wire protocol and file format. Existing TS2
  simulations stay usable through the converter (B).
- **UK signalling, level 1** in this spec (§4). Calling-on, shunt signals,
  permissive working, approach release and flashing yellows come next and the
  model must leave room for them. AWS/TPWS and ARS are out of scope.
- **Train movement**: TS2-style (per-type acceleration/braking, line speeds),
  with a per-segment gradient field that defaults to 0 so realistic physics can
  be added without a redesign.
- **Approach A**: a new UK-shaped model in plain Rust data (typed arena IDs),
  fixed-tick, command-in / event-out, deterministic. Not Bevy ECS; not a port of
  TS2's item graph.

### Success criteria

1. Converted **Drain** (UK/drain.json) and **Liverpool Street**
   (UK/liverpool-st.json) from ts2-data load, validate, and run for several
   sim-hours under the robot signaller with **no SPADs, no collisions, every
   train accounted for** (exited, stabled or formed onward).
2. **Gretz-Armainvilliers** (France) converts and validates; it may run with
   warnings since its signalling is not UK.
3. A recorded session replays to a bit-identical final state.
4. Every command rejection carries a specific reason.

## 2. Workspace layout

```
signalbox/
  Cargo.toml            # workspace
  crates/
    core/               # library: the simulation (no I/O, no networking)
    ts2-import/         # library + CLI: TS2 JSON -> signalbox world JSON
    sim-cli/            # headless runner: run, soak, record, replay
  tests/data/           # vendored ts2-data sims used by tests (with their LICENSE)
  docs/
```

`core` modules, one job each:

| Module | Job |
|---|---|
| `ids` | Typed arena IDs and stable string names |
| `network` | Static topology: nodes, segments, points, sections, signals, berths, platforms, areas |
| `layout` | Diagram coordinates only; never read by sim logic |
| `interlocking` | Routes, route/approach/overlap locking, signal controls, aspects |
| `movement` | Train kinematics and the driver model |
| `timetable` | Services, entries, dwell, service-end actions |
| `describer` | Train describer berths, stepping, interpose/cancel |
| `scoring` | Event-driven penalties |
| `sim` | `Sim`: tick loop, command queue, event log, RNG, snapshot/restore |
| `world` | Serde file format, schema version, load-time validation |

## 3. Network model (static layout)

- **Segment**: an edge between two nodes. Fields: `length_m`, `line_speed`
  (m/s), `gradient` (per mille, default 0, unused by movement in v1 except as a
  zero term), `section: SectionId`.
- **Node**: a joint between segments. Kinds:
  - `Joint` (plain or insulated — insulated if its segments are in different
    sections),
  - `BufferStop`,
  - `Boundary` (fringe entry/exit point where trains appear or leave),
  - `Points { toe, normal, reverse }`.
- **Points state**: `Normal | Reverse | Moving { to, remaining }`. Swing time is
  per-points (default 5 s). Detection is true only when not `Moving`. Locking is
  owned by `interlocking`; `network` stores the state only.
- **Track section**: a named set of segments; the unit of occupancy (track
  circuit). Occupied iff any train overlaps any of its segments. Reserved field
  `failed: bool` (unused in v1).
- **Position**: `(SegmentId, offset_m, Direction)` where offset is from the
  segment's start node and direction is `Up | Down` relative to segment
  orientation.
- **Train footprint**: head position + length; occupied segments are derived by
  walking back from the head through current points positions.
- **Signal**: position + facing direction, UK-style ID (e.g. `LS123`),
  `kind: Main { aspects: 2 | 3 | 4 }` (enum leaves room for `Shunt`,
  `CallingOn`, etc.), `sighting_m` (default 200 m).
- **Berth**: train describer berth. One in rear of each main signal, plus fringe
  berths at `Boundary` entry nodes.
- **Platform**: a stretch `(segment, from_m, to_m)` with `place` (TIPLOC-style
  code) and `platform` label. Timetable calls refer to `(place, platform)`.
- **Area**: every signal, points and section has an `AreaId` (signal box). Unused
  by `core` logic; it is the hook for multiplayer ownership in sub-project C.
- **Layout**: a separate table mapping elements to diagram geometry (x/y
  polylines, labels). Filled by the converter from TS2 coordinates.

Internally all references are typed arena indices (`SegmentId(u32)` etc.). The
file format uses stable string names, resolved to IDs at load.

## 4. Interlocking

### 4.1 Routes (data)

A route has: entrance signal; exit (signal, buffer stop, or boundary); the
ordered list of path sections; required points positions; **one overlap**
(sections beyond the exit signal plus any points positions within it; empty for
buffer stop / boundary exits). Swinging/alternative overlaps are out of scope.

### 4.2 Setting a route (entrance–exit)

`SetRoute { entrance, exit }` is rejected with a reason if:

- `NoSuchRoute` — no route defined between them;
- `ConflictingRoute` — another locked route (or its overlap) uses any of the same
  sections, or needs any shared points in the other position;
- `PointsLocked` — required points are locked in the wrong position by
  something else;
- `PointsOccupied` — required points that must move are in an occupied section.

On acceptance: points that need to move start swinging. Once every required
points (route and overlap) is detected in position, the route becomes
**Locked**: its sections are locked in the route direction and its points are
locked.

### 4.3 Signal controls and aspects

The entrance signal may show a proceed aspect only if its route is Locked, all
points are detected, and every section in the path **and overlap** is clear.

The signal returns to danger (**train-operated replacement**) when a train
occupies the first section beyond it.

Aspect of a cleared signal, from the exit signal's current aspect:

| Exit shows | 4-aspect | 3-aspect | 2-aspect |
|---|---|---|---|
| Red, or exit is a buffer stop | Y | Y | G* |
| Y | YY | G | G |
| YY or G | G | G | G |

\*2-aspect signals show only R/G; a 2-aspect signal before a red is expected to
have a separate distant, which is out of scope, so it shows G when cleared.
Exits to a `Boundary` treat the boundary as showing G.

### 4.4 Release

- **Sectional route release**: a path section releases once a train has
  occupied it and then cleared it, **in sequence** (the section in rear must have
  been occupied first). A section clearing out of sequence releases nothing.
- **Overlap release**: when the train is stationary in rear of the exit signal
  for the overlap timer (default 60 s), or when the train occupies the first
  section of the next route beyond the exit signal.
- **Auto-working (fleeting)**: a per-route flag; a route with it on does not
  release after a train passes and the signal re-clears when conditions allow.
  Routes marked `automatic` in the world are permanently locked auto signals.

### 4.5 Cancelling

`CancelRoute { entrance }` puts the entrance signal to danger immediately.

- If no train is **approaching** (no train occupying the approach sections
  within the signal's sighting distance in rear, or on the route itself), the
  route releases immediately.
- Otherwise **approach locking** holds it for a timed release (default 120 s,
  configurable per world), then releases the parts not occupied.

### 4.6 Points

`SwingPoints { points, to }` is accepted only if the points are not locked by any
route and their section is clear.

### 4.7 SPAD

When a train's head passes a signal at danger: emit
`SignalPassedAtDanger { signal, train }` and apply emergency braking to that
train. No TPWS/AWS modelling.

## 5. Trains, timetables, describer

### 5.1 Train types

`max_speed`, `accel`, `service_brake`, `emergency_brake`, `length_m`, and
reserved `mass_t` (unused in v1).

### 5.2 Movement (per tick)

A driver model picks a target speed as the minimum of:

- the lowest `line_speed` under the whole train (it cannot accelerate until its
  tail clears a lower limit);
- a service-braking curve to the nearest known restriction ahead: a red signal
  within sighting (stop a margin in rear of it), a signal showing Y within
  sighting (plan to stop at the *next* signal), a platform stop mark if the
  train calls there, a lower line speed ahead, or a buffer stop.

Acceleration is `accel` toward target, braking at `service_brake`, or
`emergency_brake` after a SPAD. The acceleration formula includes a gradient
term `-g · gradient/1000`, which is 0 in v1 data.

### 5.3 Services and entries

- **Service**: headcode (e.g. `1A23`), planned train type, ordered calls
  `(place, platform, arr, dep, stop|pass)`, and an end action.
- **Entry**: a service's train appears at a `Boundary` node at its scheduled time
  plus a delay drawn from the world's delay distribution. Its headcode is placed
  in that boundary's fringe berth. If the entry segment is occupied, entry waits.
- **Dwell**: at a stopping call, departure is at the later of the scheduled
  departure and arrival + minimum dwell (drawn from a distribution), and only
  with a proceed aspect ahead.
- **Wrong platform**: if the train stops at the right place but a different
  platform, it still calls; a `WrongPlatform` event is emitted.
- **End action**: `Exit` (leaves at a boundary), `Form { next_service }` (reverse
  and take the new headcode), or `Stable`.

### 5.4 Randomness

A single ChaCha RNG seeded at `Sim::new` supplies every random draw, in a fixed
order, so runs are reproducible.

### 5.5 Train describer

A headcode steps from the berth in rear of a signal to the next berth when the
train passes that signal. Commands `Interpose { berth, headcode }` and
`CancelBerth { berth }` let the signaller fix the describer by hand.

### 5.6 Scoring

`scoring` consumes events only. Penalties for: lateness at calls, wrong platform,
SPAD, collision. It keeps a total per `AreaId` so multiplayer can score each box.

## 6. Simulation API and determinism

```rust
let world = World::load(json)?;          // validated
let mut sim = Sim::new(world, seed);
sim.submit(Command::SetRoute { entrance, exit });   // applied at next tick
let events: Vec<Event> = sim.step();     // advance TICK = 100 ms sim time
let view = sim.signal(id);               // read-only queries
let snap = sim.snapshot();               // serde-serialisable
let sim2 = Sim::restore(world, snap)?;
```

- Commands are validated and applied at the start of the next tick, in
  submission order. Rejections come back as `Event::CommandRejected { cmd,
  reason }`.
- Time acceleration, pausing and wall-clock pacing belong to the future server;
  `core` only steps.
- Determinism rules: no `HashMap`/`HashSet` iteration in sim logic (use `Vec`,
  `BTreeMap`, or `IndexMap`); no wall clock; one seeded RNG; stable ordering of
  trains, routes and events.
- **Save** = world + snapshot + command log (with tick numbers). Replay =
  `Sim::new` with the same seed + resubmitting the log.

## 7. World file format

JSON via serde, top-level `{ "schema": 1, ... }`. Sections: `areas`, `sections`,
`nodes`, `segments`, `signals`, `berths`, `platforms`, `routes`, `train_types`,
`services`, `entries`, `options` (timers, delay distributions, penalties),
`layout`. References are by string name.

Load-time validation returns typed errors (not panics), at least:
dangling references; a segment in zero or two sections; a route whose path is not
contiguous or crosses points not listed in its positions; signals not on a
segment; berths without a signal or boundary; duplicate names; an unsupported
schema version.

## 8. TS2 converter (`ts2-import`)

Input: a TS2 simulation JSON (as in ts2-data). Output: a signalbox world JSON
plus a warnings report. The TS2 format facts this section relies on were
checked against all three ts2-data files (format notes kept with the Plan 2
workspace). Amended 2026-09-29 after that survey; owner decisions marked (O),
controller rulings marked (R).

### 8.1 Graph

- TS2 is an item chain: items link through `previousTiId`/`nextTiId`
  (`reverseTiId` for points), and item orientation is not consistent, so the
  converter resolves direction by walking (arriving from an item's previous
  end you leave by its next end).
- Every `LineItem`/`InvisibleLinkItem` becomes one segment
  (`length_m = realLength`, speed from `maxSpeed` m/s, where 0 inherits the
  place's speed if the line has a `placeCode`, else `defaultMaxSpeed`).
- Signals and points are zero-length items. Where two of them (or one of them
  and an `EndItem`) touch with no line in between, the converter inserts a
  1 m segment so every node has real segments.
- `PointsItem` → `Points` node: toe = previous end, normal = next end,
  reverse = reverse end.
- `EndItem` → `Boundary` or `BufferStop` (R): an end is a **Boundary** if any
  train enters there, or if no platform line (a line with a `placeCode`) lies
  within 400 m in rear of it; otherwise it is a **BufferStop** (terminal
  platforms, sidings). The classification of every end is listed in the report.
- Orphan fragments not connected to anything a route or train uses are
  dropped with a warning.

### 8.2 Signals

- A `SignalItem` protects trains running from its previous item to its next
  item, and stands at the end of the segment it is entered from. The `reverse`
  flag is drawing-only and ignored.
- `BUFFER` signals are not signals: they are dropped, and routes that end at
  one get the buffer stop / boundary beyond it as their exit node.
- Aspects: `UK_3_ASPECTS*` → 3, `UK_4_ASPECTS*` → 4; anything else (the French
  `FR_*` types) → 3, with one warning per type name. Aspect condition rules and
  `customProperties` are dropped (one warning per type).
- `sighting_m` = `options.defaultSignalVisibility`.
- Names: the TS2 `name`, or `name#tiId` where a name repeats.

### 8.3 Sections

- A section boundary at every signal, and on each side of every points (points
  get their own section, including any 1 m legs).
- Flat crossings (`conflictTiId` pairs) (R): both crossing lines go into one
  shared section, so a train on either line occupies the crossing and a route
  over one conflicts with a route over the other. One warning per crossing
  (the shared section is longer than a real diamond).
- `pairedTiId` (crossover pairs) is ignored: routes already list both points.

### 8.4 Routes

- Each TS2 route's path is found by walking from its begin signal using its
  `directions` (0 = normal, 1 = reverse).
- **Split at signals (O):** a TS2 route that passes through intermediate
  signals becomes one signalbox route per signal-to-signal stretch, each
  carrying the points positions on its own stretch, as on a real UK
  entrance–exit panel. Identical stretches from different TS2 routes are merged;
  if two TS2 routes need different points positions on the same stretch, both
  are kept as separate routes (same entrance and exit with different points is
  not allowed, so this is reported and the second is dropped).
- `initialState` 2 → `automatic` on every stretch; `initialState` 1 (one-off
  pre-set) → a normal route, with a warning.
- A main signal that is still the entrance of no route and whose track ahead
  reaches the next signal or an end without passing facing points gets a
  generated **automatic** route to it (plain-line automatic signals). Any other
  signal with no route is reported.
- Overlaps are generated: the sections beyond the exit signal up to about 180 m,
  extended to the next section boundary. Beyond facing points the overlap uses
  the positions the TS2 route set there, if it passed them; otherwise the overlap
  stops before those points (with a warning). Exits to a buffer stop or
  boundary have no overlap.
- Every generated route is checked with the core route tracer before output;
  a route that fails is dropped with a warning (never silently).

### 8.5 Places, timetable, trains

- Platforms are the lines carrying `placeCode` + `trackCode` (platform label =
  `trackCode`), not TS2's `PlatformItem`s, which are drawing only and go to the
  layout. A (place, track) spread over two lines gives two platform entries with
  the same label.
- Train types map field for field (m/s → km/h); `elements` is ignored.
- Service lines → calls (`mustStop` → stop, `""` times → none; a pass line with
  only one time keeps it as `dep`). Calls at a (place, track) with no line are
  dropped with a warning.
- `postActions`: `SET_SERVICE X` + `REVERSE` → `Form(X)`; `REVERSE` alone →
  `Stable`; none → `Exit`; `SET_SERVICE` alone → `Form(X)` with a warning (the
  core always reverses on forming). A Form/Stable service with no stopping call
  becomes `Exit` with a warning.
- Trains → entries. A train whose head starts next to an `EndItem` enters at that
  boundary. **Mid-network starts (O):** any other train starts at its TS2 head
  position (segment, offset, direction) using the new core entry kind below;
  a train starting in a platform of its first call simply arrives there at once.
  `appearTime` + `initialDelay` → entry time (entries before the start time are
  due immediately).
- Options: `start_time = currentTime`; TS2 delay generators (integer, or bands of
  `[lo, hi, %]`) become the `[min lo, max hi]` range, clamped at 0, with a
  warning when bands are merged; `latePenalty`/`wrongPlatformPenalty` carry over;
  `timeFactor`, `trackCircuitBased`, `warningSpeed`, `wrongDestinationPenalty`,
  scores and tokens are ignored (listed once in the report).

### 8.6 Core change: start positions (O)

`EntryFile` gains an optional `at: { segment, offset_m, direction }`, exclusive
with `boundary`. Such a train appears with its head at that position; the rest of
the train is laid behind it along the track (any part that would run off the
network is treated like a train still entering). It is placed only once every
section under it is free and unowned, and its headcode is interposed in the berth
of the first signal ahead.

### 8.7 Output and CLI

- Layout: line polylines, points glyphs, signals and berth positions, platform
  rectangles, place names and text labels, all in TS2 scene coordinates.
- `ts2-import input.json -o world.json` writes the world and prints the warnings
  report (grouped counts, then details); `--strict` exits non-zero if any
  route was dropped.
- The world always passes `World::from_json` validation; a converter bug that
  produces an invalid world is a failed conversion, not a warning.

## 9. Error handling

- Invalid worlds are rejected at load with typed errors (§7).
- Commands never panic; they are rejected with a reason.
- A train entering a section occupied by another train emits `Collision`. It is
  always a test failure (permissive working will later allow some cases).
- Internal invariants (e.g. a locked route's points are still in position) are
  `debug_assert!`ed; in release builds a violation emits `InvariantViolated`
  instead of crashing.

## 10. Testing

Built test-first.

- **Unit tests** per module: aspect sequences for 2/3/4-aspect; each route
  rejection reason; sectional release order (including out-of-sequence clears);
  approach-locking timing; overlap release; auto-working; braking curves stop
  short of reds; describer stepping.
- **Hand-built worlds** as fixtures: plain line with auto signals, a double
  junction, a two-platform terminus.
- **Converter tests** on Drain, Liverpool Street and Gretz-Armainvilliers:
  converts, validates, warning counts recorded as snapshots.
- **Robot signaller soak** (`sim-cli soak`): for each train, set the next
  route along its timetabled path when possible. Drain and Liverpool Street run
  several sim-hours with no SPAD, no collision, and all trains accounted for.
- **Replay test**: run with a recorded command log, replay, compare final
  snapshots byte for byte.

## 11. Out of scope for this spec

Networking and the multiplayer protocol (C), any UI (D), Darwin (E), calling-on
and shunt signals, permissive working, approach release, flashing yellows,
swinging overlaps, AWS/TPWS, ARS, realistic traction physics, failures
(track circuit / points / signal).
