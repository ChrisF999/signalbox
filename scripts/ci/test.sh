#!/usr/bin/env bash
# CI test gate: build and test the whole workspace, warnings as errors.
# Runs natively on the CI runner (offline, against its seeded cargo cache);
# locally use `scripts/cargo test` instead.
set -euo pipefail
export RUSTFLAGS="${RUSTFLAGS:-} -D warnings"
cargo --version
cargo build --workspace --all-targets --locked --offline
cargo test --workspace --locked --offline
# The dev-login build of the front (tests and bots only; the release image
# never has it) and the tests that need it.
cargo build -p signalbox-server --features dev-auth --all-targets --locked --offline
cargo test -p signalbox-server --features dev-auth --locked --offline
