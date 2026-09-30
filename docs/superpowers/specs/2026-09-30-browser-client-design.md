# signalbox — browser client design (sub-project D1)

Date: 2026-09-30
Status: draft for review
License: GPL-2.0-or-later
Builds on: `2026-09-30-server-and-protocol-design.md` (sub-project C, C1 + C2)

## 1. Context and goal

D1 is the signaller's screen: a browser client for the C server, so friends can play
multiplayer signalling from any browser on the tailnet. D2 (later) is a native desktop
client built from the same Rust code.

### Decisions (owner, 2026-09-30)

| Topic | Decision |
|---|---|
| Style | Modern UK VDU (IECC / Westcad style), not an NX panel or a game map |
| Language | Rust compiled to WASM |
| Rendering | wgpu: WebGPU (Dawn in Chrome/Edge) with automatic WebGL2 fallback |
| Desktop | Yes, as D2, reusing D1's core and UI; designed for from day one |
| UI toolkit | egui (eframe) for everything: panels and the track diagram |
| D1 screens | Lobby + area picker; track diagram; alarms / event log; train list; clock, votes, players |

### Success criteria

1. From a browser on the tailnet, a `signalbox-users` member logs in with Authentik, creates or
   joins a game, claims an area, sets and cancels routes, swings points, interposes headcodes and
   votes on the clock — with a second player in a neighbouring area at the same time.
2. Works in Chrome/Edge (WebGPU) and Firefox (WebGL2 fallback).
3. The client never crashes on anything the server sends; losing the connection recovers by
   itself with one resync.
4. All client logic except drawing and the browser shell is tested natively in CI.

## 2. Architecture

| Crate | Responsibility | Targets |
|---|---|---|
| `client-core` | Connection state machine, lobby state, layout/view/delta handling (reuses `bot::Bot`), notice log, train list, selection and input logic (clicks → `PlayerCommand`), behind a `Transport` trait | native + wasm32; tested natively |
| `client-ui` | egui app: screen layout, lobby, alarms, train list, clock/votes/players, the track diagram (egui painter), hit-testing, pan/zoom | native + wasm32 |
| `client-web` | eframe web runner (WebGPU → WebGL2), WebSocket `Transport` via web-sys, reconnect | wasm32 only |
| `client-desktop` (D2) | eframe native runner, tungstenite `Transport`, native login | native (D2) |

Serving: the Docker build compiles `client-web` with the `wasm32-unknown-unknown` target and a
version-pinned `wasm-bindgen-cli`; the C2 front serves the static files, replacing its
placeholder page at `/`. The page opens `/ws` on the same origin with the existing Authentik
session cookie — no new login code in D1. The CI image and its offline cargo cache gain the
wasm32 target and `wasm-bindgen-cli`.

## 3. The screen

```
┌──────────────────────────────────────────────────────────────────────┐
│ <game> · <area> (you)   07:14:32  4×   [⏸][1×][2×][4×][8×]  vote…      │
│ Players: ann → Liverpool St, bob → Bethnal Green, robot → Hackney     │
├───────────────────────────────────────────────┬──────────────────────┤
│            track diagram (pan / zoom)         │ TRAINS               │
│                                               ├──────────────────────┤
│                                               │ ALARMS               │
└───────────────────────────────────────────────┴──────────────────────┘
```
The lobby is its own screen: games list, create (layout, seed, start), join, then area picker or
spectate.

### 3.1 Diagram (IECC style, near-black background)

- Track: grey free; white along a set route; dimmer white along its overlap; red occupied.
- Points: the lying leg drawn connected, the other with a gap; the gap flashes while moving.
- Signals: a small head with the real aspect (R, Y, YY, G) and a stub showing the direction;
  automatic signals carry an "A" marker, highlighted while auto-working is on.
- Berths: box with the headcode in yellow; dim outline when empty.
- Platforms and place labels from the TS2 geometry.
- Fringe at about half brightness and never clickable.
- Pan/zoom: fits the player's area on join; wheel zooms, drag pans, a "Fit" button.

### 3.2 Controls

- Route: click the entrance signal (highlighted), then an exit signal or exit marker → `set_route`.
  Only exits with a route from the chosen entrance light up. Clicking the entrance again or Esc
  clears the selection.
- Right-click signal: cancel route; auto-working on/off where allowed.
- Right-click points: swing normal / reverse.
- Right-click berth: interpose (small text box) or cancel.
- A rejected command flashes the entrance signal and adds an alarm with the reason.
- Hover shows the name and state of anything; spectators and fringe get hover only.
- Clock buttons propose a vote or agree with the open one; the open vote shows who has agreed and
  the time left.

## 4. Changes to C (protocol + game)

### 4.1 `Layout.geometry`

New field, filtered to the visible set (own area + fringe; everything for spectators):
`lines` (per segment: segment name, x1, y1, x2, y2), `points` (node, x, y), `signals` (signal, x, y,
berth_x, berth_y), `platforms` (place, platform, x1, y1, x2, y2), `labels` (text, x, y) inside the
visible bounding box. Source: the converted world's `layout` field (ts2-import already writes it).
A world without geometry gives `geometry: null`; the client shows "no diagram for this layout".

### 4.2 `View.trains`

`BTreeMap<headcode, TrainRow>`, diffed like the other view maps. A row per train that is on the
player's visible track, or whose next call is at a platform in the player's area, or whose entry
at a boundary into the player's area is due within 30 sim minutes (spectators: every train).
`TrainRow { next_place, next_platform, booked, late_s, state: due | approaching | in_area |
at_platform }`. Built from sim state only (spec C §4.3), so the C1/C2 consistency tests cover it.

## 5. Errors and connection

- Connection lost → red "Reconnecting…" banner, exponential backoff capped at 10 s; the full view
  on reconnect restores everything.
- `/ws` 401 (session expired) → navigate to `/auth/login`.
- No WebGPU → WebGL2; neither → a plain page explaining why.
- Rejected commands and `game_crashed` → alarms; `game_crashed` also returns to the lobby with a
  banner.
- A malformed server frame is logged and triggers a resync; nothing received can panic the client.

## 6. Testing

- `client-core` (native): click sequences → exact `PlayerCommand`s; valid-exit highlighting;
  right-click menus → commands; lobby state machine; reconnect → one resync.
- `client-ui` (headless): hit-testing (signal / berth / points under a point) and fit/zoom maths.
- `protocol`: golden JSON for `geometry` and `trains`; the C1 consistency soak extended to
  `trains`.
- End to end (native, CI): `client-core` logs in with dev auth against the real front, joins
  Liverpool Street, "clicks" a route and sees it set in its view.
- Browser (controller, after deploy, not CI): headless Chromium (Playwright image on ra) loads a
  dev-auth test build, sets a route by clicking, and screenshots the result.

## 7. Out of scope

Native desktop client and its login (D2); sounds; exact train positions on the diagram;
touch/mobile layout; editing layouts; chat; accessibility beyond keyboard Esc and hover text
(revisit in D2).

## 8. Amendments (D1 planning, 2026-09-30)

Recorded in full in the header of `docs/superpowers/plans/2026-09-30-d1-browser-client.md`. In short:
egui/eframe 0.36.2 on wgpu 30 (WebGPU first, WebGL2 fallback — egui-wgpu's default; WebGPU needs HTTPS, which the
tailnet URL has); wasm-bindgen pinned to 0.2.129 with the matching CLI in a repo-local `wasm-tools` build stage (the
stock Rust image has no wasm32 target); `rand` default features off in the workspace (getrandom does not build for the
browser) with only the server enabling them; `bot` gains a default `net` feature so the client reuses `Bot` without
tokio; geometry also carries points leg ends, signal facing and exit-node positions (all 22 Liverpool Street
buffer/boundary exits need them to be clickable); an expired session is detected by probing `GET /ws` for 401; on-screen
text avoids glyphs missing from egui's default fonts ("W1 to A", pinned by a test); the front serves the web client from
`SIGNALBOX_WEB`, keeping the placeholder page when it is absent; bundle-size work (wasm-opt, precompression) is deferred.
