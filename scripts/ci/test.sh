#!/usr/bin/env bash
# CI test gate: build and test the whole workspace, warnings as errors.
# Runs natively on the CI runner (offline, against its seeded cargo cache);
# locally use `scripts/cargo test` instead.
set -euo pipefail
export RUSTFLAGS="${RUSTFLAGS:-} -D warnings"
cargo --version
cargo build --workspace --all-targets --locked --offline
cargo test --workspace --locked --offline
