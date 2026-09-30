#!/usr/bin/env bash
# CI test gate: build and test the whole workspace, warnings as errors.
# Runs natively on the CI runner (offline, against its seeded cargo cache);
# locally use `scripts/cargo test` instead.
set -euo pipefail
export RUSTFLAGS="${RUSTFLAGS:-} -D warnings"
cargo --version
cargo build --workspace --all-targets --locked --offline
cargo test --workspace --locked --offline
# The browser client's view of the bot: Bot and Greedy without tokio.
cargo build -p signalbox-bot --no-default-features --locked --offline
# The dev-login build of the front (tests and bots only; the release image
# never has it) and the tests that need it.
cargo build -p signalbox-server --features dev-auth --all-targets --locked --offline
cargo test -p signalbox-server --features dev-auth --locked --offline
# The browser client for wasm32, where the runner has the target and
# wasm-bindgen-cli. The controller's runner image sets SIGNALBOX_REQUIRE_WASM=1
# once it has them; until then a runner without them skips, loudly.
# Capture the list first: `grep -q` in a pipe can exit before rustup has
# written everything, and under pipefail the SIGPIPE would read as "missing".
installed_targets="$(rustup target list --installed 2>/dev/null || true)"
if grep -qx wasm32-unknown-unknown <<<"$installed_targets" && command -v wasm-bindgen >/dev/null; then
  scripts/build-web.sh target/web-dist --offline
elif [ "${SIGNALBOX_REQUIRE_WASM:-0}" = 1 ]; then
  echo "ci: SIGNALBOX_REQUIRE_WASM=1 but the wasm32 target or wasm-bindgen-cli is missing" >&2
  exit 1
else
  echo "ci: SKIP the web client build: no wasm32 target or wasm-bindgen-cli on this runner"
fi
