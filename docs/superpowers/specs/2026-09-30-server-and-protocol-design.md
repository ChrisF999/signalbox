# signalbox — multiplayer server + protocol design (sub-project C)

Date: 2026-09-30
Status: draft for review
License: GPL-2.0-or-later
Builds on: `2026-09-29-core-and-converter-design.md` (sub-projects A + B, merged)

## 1. Context and goal

Sub-project C turns the deterministic sim core into a multiplayer game: several
players, each running one signal box (an *area*) on a shared layout, over
WebSockets, with the robot signaller playing every area nobody has claimed.
The browser client (D) is out of scope; C ships a bot client that exercises the
whole protocol, and D will reuse C's `protocol` crate.

### Decisions already made (owner, 2026-09-30)

| Topic | Decision |
|---|---|
| Transport | WebSocket |
| Updates | Deltas only; client may request a full resync, server may push one |
| Screens | One signal box per screen; one screen = one area; one player per area |
| Areas for TS2 layouts | Hand-made per-layout area file, applied by the converter |
| Unclaimed areas | Played by the robot signaller |
| Identity | Authentik login (native OIDC), gated by group `signalbox-users` |
| Game clock | Pause / speed change needs agreement of every player holding an area |
| Games | Lobby; several games at once; any member may start one |
| Architecture | Front server + one OS process per game |
| Saving | Autosave + resume; each game is its own SQLite database |
| Visibility | Own area in full + read-only fringe of neighbouring areas |
| Hosting | ra, tailnet only (`tailscale serve`), Docker |

### Assumptions (presented, not objected to)

- A disconnected player keeps their area for a 2-minute grace period, then the
  robot takes it over and it becomes claimable.
- The game refuses commands on signals, points or berths outside the sender's
  claimed area before they reach the sim.
- Scores stay per area (core `Scores::by_area`).
- The browser client is not part of C.

### Success criteria

1. Two bot players and the robot run the Liverpool Street layout, split into
   areas, through the real front and game processes, with no SPAD, collision or
   invariant violation, and every client's delta-built view equal to the
   server's full view throughout.
2. A game saved, killed and resumed reaches a state hash identical to an
   uninterrupted run with the same commands.
3. A player cannot operate anything outside their area; a spectator and the
   robot never vote.
4. A crashing game process does not affect the front or other games.
5. Nothing is reachable without an Authentik session in `signalbox-users`,
   except the login flow itself. The dev login used by tests is absent from the
   release build.

## 2. Architecture

```
browser/bot ⇄ WebSocket ⇄ front (server) ⇄ Unix socket (ipc) ⇄ game process ⇄ core Sim
```

### 2.1 Crates

| Crate | Kind | Responsibility |
|---|---|---|
| `core` | lib (exists) | Simulation; gains nothing railway-new except what §6 lists |
| `ts2-import` | lib + bin (exists) | Gains `--areas <file>` (§5) |
| `protocol` | lib | Client ⇄ server message types, `View`, `Delta`, view diff/apply. No I/O. |
| `ipc` | lib | Framed front ⇄ game messages over a Unix socket |
| `game` | lib + bin | `Game` (pure logic: claims, votes, views, robot filter, clock) + process shell (socket, timing, SQLite saves) |
| `server` | bin | Front: HTTP, OIDC, sessions, lobby, WebSocket termination, supervisor, relay |
| `bot` | bin | Headless client: login (dev auth in tests), join, claim, play with robot logic |

Rule: the front knows *who* a player is and nothing about railways; the game
knows everything about the railway and trusts the identity the front attaches.

### 2.2 Game lifecycle

- **Start:** a member picks layout, seed, start time in the lobby. The front
  creates the save database (§7) and spawns `game --save <db> --socket <path>`.
- **Resume:** the front spawns `game --save <db> --socket <path>`; the game
  sees an existing snapshot and restores (§7.3).
- **Empty:** when the last player leaves, the game pauses, saves, and exits
  after 10 minutes empty. It appears in the lobby as saved.
- **Stop:** on front shutdown (SIGTERM), the front sends `Shutdown` to every
  game; each saves and exits; the front waits up to 10 s, then kills.
- **Crash:** socket EOF or child exit without `Shutdown` → front sends a
  `game_crashed` notice to that game's clients, drops them to the lobby, and
  marks the game resumable from its last save. No automatic restart.

A game is identified by a random id (`g-` + 12 base32 chars), which is also its
save file name.

## 3. Protocol (client ⇄ front ⇄ game)

JSON text frames, tagged `{"type": ...}`. All types live in `protocol`.

### 3.1 Lobby (handled by the front)

Client → front: `list_games`, `list_layouts`,
`create_game {layout, seed?, start?}`, `join {game}`, `leave`.
Front → client: `games [{id, layout, state: running|saved|crashed, sim_time,
areas: [{name, holder?}], players}]`, `layouts [{name, areas}]`,
`joined {game, you}`, `error {code, message}`.

`create_game` resumes nothing; `join` on a `saved`/`crashed` game resumes it.

### 3.2 In-game, client → game

| Message | Meaning |
|---|---|
| `claim {area}` | Take a free area (fails `area_taken`) |
| `release` | Give your area back (robot takes over) |
| `command {cmd}` | Core `Command` JSON, e.g. `{"cmd":"set_route",...}` |
| `vote {proposal}` | Propose or agree: `pause`, `resume`, `speed {x}` with x ∈ {1,2,4,8} |
| `resync` | Ask for layout + full view |

### 3.3 In-game, game → client

| Message | Meaning |
|---|---|
| `layout {...}` | Static picture of your screen (§4.1). Sent on join, claim/release and every resync |
| `view {seq, ...}` | Full dynamic view (§4.2); new `seq` base |
| `delta {seq, ...}` | Changed fields since `seq - 1` (§4.3) |
| `notice {kind, ...}` | One-off events (§4.4) |

A client that sees `seq` jump sends `resync`.

### 3.4 Commands and area checks

The game maps each command to the area of its subject: `SetRoute`,
`CancelRoute`, `SetAutoWorking` → entrance signal's area; `SwingPoints` →
the points node's section's area; `Interpose`, `CancelBerth` → berth's area.
If it is not the sender's claimed area → `notice {kind: not_your_area}`; the
sim never sees it. Accepted commands are submitted to the sim and logged (§7).
Sim rejections (`CommandRejected`) go back to the sender as
`notice {kind: rejected, reason}`.

### 3.5 Clock votes

- Only players holding an area vote; spectators and the robot never do.
- A proposal applies as soon as every holder has agreed (so a lone holder's
  proposal is instant). It lapses after 30 s of real time.
- One open proposal at a time; a different proposal replaces it (resetting
  agreements). Claiming or releasing re-evaluates: a holder leaving can
  complete a vote.
- With no holders (spectators only), the clock keeps its current state; the
  empty-game rule (§2.2) still applies.
- Speed x means x sim ticks per 0.1 s of real time (max 80 ticks/s).

### 3.6 Identity, sessions, duplicates

The front attaches the player's Authentik username to every message it
forwards. The same user joining twice: the newest connection takes over the
player (and their area); the old one gets `notice {kind: replaced}` and is
closed.

## 4. Views

### 4.1 Layout (static)

For the player's *visible set* — their area plus fringe (§4.5), or, for a
spectator, the whole layout — the static subset of the world: sections,
segments with geometry-free topology, signals (name, area, aspects), points,
berths, platforms, routes from each visible signal (name, entrance, exit),
area names. Marks which elements are operable (own area) and which are fringe.

### 4.2 View (dynamic)

Per visible element:

- signals: aspect
- routes (entrance visible): state `setting|locked|cancelling`, auto-working
- points: position, moving, locked
- sections: occupied, held (path/overlap) or free
- berths: headcode or empty
- global: sim time, speed, paused, open vote (proposal, agreed, expires),
  area holders (player or robot), own area's score

Train positions are **not** sent; players see track occupancy and describer
headcodes, as on a real UK panel. (Exact positions may be added for D later.)

### 4.3 Deltas

- Built at most 5 times per real second, after the sim has stepped: the game
  computes each client's view, diffs it against the last view that client was
  sent, and sends only changed fields (`null` = cleared). No delta if nothing
  changed.
- Views are built from sim state, not events, so a client can never drift: a
  resync is simply the full view.
- A client's outbound queue is bounded (64 messages); on overflow the game
  drops the queue and sends one full `view`.
- Forced resync: on join, claim/release (visible set changes), resume.

### 4.4 Notices

Derived from core events, filtered to the recipient's area:
`rejected`, `not_your_area`, `spad`, `collision`, `late {train, platform,
late_s}`, `wrong_platform`, `handover {headcode, from_area}` (a berth in your
area filled by a step from another area's berth), plus `area_taken`,
`replaced`, `game_crashed`, `error`.

### 4.5 Fringe

For each boundary signal, the fringe is the sections of the neighbouring area
between the boundary and the first signal beyond it (walking away from your
area), plus the berths on them — in both directions. Fringe elements are
visible, never operable.

## 5. Areas for converted layouts

A per-layout JSON file, applied by `ts2-import --areas <file>`:

```json
{"schema": 1,
 "boundaries": ["LS121", "BG40", "BG41"],
 "areas": [{"name": "Liverpool Street", "seeds": ["LS1"]},
           {"name": "Bethnal Green", "seeds": ["BG40"]}]}
```

- Seeds are section, signal or berth names (signals and berths resolve to their
  section). Each area floods outward through the section graph from its seeds;
  the flood does not cross a boundary signal.
- A signal belongs to the area of the section on its approach side, so a box
  owns the signals trains approach it through.
- Hard errors (conversion fails, listing names): a section reached by no area,
  a section reached by two areas, an unknown name, a boundary name that is not
  a signal.
- No area file → one area, as today.
- Because boundaries are signals, a route lies in its entrance signal's area;
  only its overlap may extend into the next area.

C delivers one area file: Liverpool Street, 2–3 areas, boundaries agreed with
the owner during planning.

## 6. The game process

### 6.1 `Game` (pure, tested in-process)

```text
Game::new(world, seed) / Game::restore(world, snapshot, commands)
Game::handle(player, ClientMsg) -> Vec<(player, ServerMsg)>
Game::connect(player) / Game::disconnect(player)
Game::advance(real_dt) -> Vec<(player, ServerMsg)>   // steps sim per clock, runs robot, votes, grace timers
Game::flush() -> Vec<(player, ServerMsg)>            // deltas (called ≤ 5 Hz)
```

- Robot: each tick while not paused, `robot::commands(sim)` filtered to
  commands whose subject (§3.4 mapping) lies in an unclaimed area. Claiming
  stops the robot touching that area at once; routes it set stay set.
- Grace: a disconnected holder keeps the area for 120 s of real time, then it
  is released (robot takes over, others may claim).
- Handover detection: a `BerthChanged` filling a berth in area A in the same
  tick as a berth in area B (≠ A) emptied with the same headcode.

### 6.2 Core additions

Only what C needs, each small:

- An area lookup for every command subject (signal, points node, berth) — may
  live in `game` using existing `network` fields; move to core only if needed.
- `robot::commands` unchanged; filtering happens in `game`.
- `Sim::state_hash()` if not already exposed (sim-cli has one) for the resume
  test.

## 7. Saving (SQLite, one database per game)

### 7.1 File

`saves/<game-id>.sqlite`, WAL mode, `rusqlite` with bundled SQLite. Only the
game process writes it; the front opens it read-only to list `meta`.

```sql
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
  -- schema, layout, seed, created, last_played, areas (JSON), start
CREATE TABLE world (id INTEGER PRIMARY KEY CHECK (id = 1), json TEXT NOT NULL);
CREATE TABLE snapshots (tick INTEGER PRIMARY KEY, saved_at TEXT NOT NULL, state TEXT NOT NULL);
CREATE TABLE commands (seq INTEGER PRIMARY KEY, tick INTEGER NOT NULL,
                       player TEXT NOT NULL, area TEXT NOT NULL, command TEXT NOT NULL);
```

### 7.2 Writes

- `world` + `meta` once at creation; `meta.last_played` on each snapshot.
- Each accepted command is inserted as it is submitted (one small
  transaction; batched per tick if needed for speed).
- A snapshot every 60 s of real time while running, and on going empty,
  `Shutdown`, and clean exit. Keep the newest 3 snapshots.
- The world is copied into the save so re-converting a layout never breaks a
  saved game.

### 7.3 Resume

Load `world`, newest snapshot, `Sim::restore`, then replay `commands` with
`tick >= snapshot.tick` in `seq` order, stepping to the tick the log reaches.
The full command history is kept, so a game can also be replayed from tick 0
with the seed (future replay/spectate).

Claims are not saved: after resume every area is unclaimed (robot-run) until
players claim again. The clock resumes paused at 1×.

## 8. The front (`server`)

- **HTTP:** `/` (placeholder page until D), `/auth/login`, `/auth/callback`,
  `/auth/logout`, `/ws` (WebSocket upgrade).
- **OIDC:** authorisation code + PKCE against Authentik. The `groups` claim
  must contain `signalbox-users`; otherwise 403. Player name =
  `preferred_username`. Session = signed, HttpOnly, SameSite=Lax cookie
  (12 h); secret from the environment.
- **Authentik:** a new OIDC provider + application `signalbox` and group
  `signalbox-users`, added to `/opt/stack/apps/authentik/blueprints/` in the
  existing files' pattern. Membership is the owner's decision.
- **Supervisor:** spawns/tracks game children, one Unix socket each in a
  0700 directory; routes client messages by game; detects crashes (§2.2).
- **Limits:** > 20 client messages/s → close; malformed JSON → `error`, not a
  disconnect; max frame 64 KiB.
- **Dev auth:** cargo feature `dev-auth` adds `/auth/dev?user=` for tests and
  the bot. The release image is built without it; a test asserts the release
  binary rejects `/auth/dev`.

## 9. ipc (front ⇄ game)

Length-prefixed (u32 BE) JSON frames over a Unix stream socket.
Front → game: `Connect {player}`, `Disconnect {player}`,
`Client {player, msg}`, `Shutdown`.
Game → front: `ToPlayer {player, msg}`, `Status {sim_time, holders, players}`
(for the lobby, at most 1/s), `Saved {tick}`.

## 10. Deployment

One Docker image (both binaries + `game`), on ra; saves and layouts on a
mounted volume; published on the tailnet with `tailscale serve` (same pattern
as descry). Authentik redirect URI = the tailnet URL. New crates (`rusqlite`,
tokio/axum, OIDC, cookies) require reseeding the offline CI cargo cache before
the first CI run.

## 11. Error handling

- Game panic/exit → §2.2 crash path; the front never panics on game output
  (malformed ipc frame → treat the game as crashed).
- SQLite errors in the game: failing to write a command or snapshot is logged
  and surfaced to players as `error {code: save_failed}`; the game keeps
  running (a later snapshot may succeed).
- Resume failures (bad snapshot, world mismatch) → game exits non-zero; the
  front marks the game `crashed` with the error text in the lobby.
- No client input can panic the game or the front.

## 12. Testing

- `protocol`: serde round-trips; golden JSON examples for each message.
- `game` (in-process, no sockets):
  - delta consistency: over a multi-hour soak with robot + scripted players,
    `apply(layout+view, deltas…)` equals the freshly built full view at every
    flush, for area, fringe and spectator views;
  - area enforcement (every command kind), claim conflicts, release/grace;
  - votes: holder-only, unanimity, lapse, replacement, lone holder instant,
    holder leaving completes a vote;
  - robot never commands a claimed area;
  - handover notice fires on a boundary crossing;
  - save → drop → resume → continue gives the same state hash as an
    uninterrupted run (through a real SQLite file in a temp dir).
- `ts2-import --areas`: flood fill, signal ownership, every hard error.
- End to end (`server` + `game` + `bot`, dev auth): two bots + robot on
  Liverpool Street at 8× for a fixed sim span — no SPAD/collision/invariant,
  bot views consistent; crash a game process mid-run → other game unaffected,
  lobby shows `crashed`, resume works; unauthenticated `/ws` is refused.

## 13. Plans

Two implementation plans, each independently testable:

- **C1:** `protocol`, `game` (lib + bin with SQLite saves), area files in
  `ts2-import` + the Liverpool Street area file, bot playing in-process
  against `Game`.
- **C2:** `ipc`, `server` (lobby, supervisor, OIDC, sessions, limits,
  dev-auth), Authentik blueprint, Docker + `tailscale serve` on ra, CI cache
  reseed, end-to-end soak.

## 14. Out of scope

Browser client (D); exact train positions in views; public (non-tailnet)
hosting; replay/spectate UI; chat; per-player permissions beyond area
ownership; Darwin timetables (E); level-2 signalling.
