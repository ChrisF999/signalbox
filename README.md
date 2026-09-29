# signalbox

A deterministic UK railway signalling simulation in Rust — a rewrite of
[TS2](https://github.com/ts2/ts2) aimed at multiplayer signal boxes in the browser.

Status: simulation core (plan 1 of 2). The TS2 converter comes next.

## Build and test

There is no Rust toolchain on the host; `scripts/cargo` runs cargo in Docker.

```bash
scripts/cargo test
scripts/cargo run -p sim-cli -- run crates/core/tests/fixtures/junction.json --robot --hours 1
```

## Layout

- `crates/core` — the simulation library (`signalbox-core`)
- `crates/sim-cli` — headless runner: `run` (optionally with the robot signaller, recording a command log) and `replay`
- `docs/superpowers/specs` — design; `docs/superpowers/plans` — implementation plans

## License

GPL-2.0-or-later, like TS2.
