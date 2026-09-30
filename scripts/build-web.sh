#!/usr/bin/env bash
# Build the browser client (crates/client-web) into OUT (default
# target/web): index.html, app/signalbox_web.js, app/signalbox_web_bg.wasm.
# Needs the wasm32-unknown-unknown target and wasm-bindgen-cli at the
# version Cargo.lock pins: the wasm-tools stage of deploy/Dockerfile has
# both (locally, run this through scripts/wasm-build). Arguments after OUT
# go to cargo (CI passes --offline).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd -P)"
out="$(realpath -m "${1:-$root/target/web}")"
shift || true
# OUT is emptied below: never /, the checkout or anything above it.
stem="${out%/}" # "" for /
case "$root/" in
  "$stem"/*)
    echo "build-web: refusing to use $out as OUT: it would delete the repository at $root" >&2
    exit 2
    ;;
esac
cd "$root"
want="$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"$/\1/p;q}' Cargo.lock)"
have="$(wasm-bindgen --version | awk '{print $2}')"
if [ "$want" != "$have" ]; then
  echo "build-web: wasm-bindgen-cli is $have but Cargo.lock has wasm-bindgen $want" >&2
  exit 1
fi
cargo build -p signalbox-client-web --target wasm32-unknown-unknown --profile web --locked "$@"
rm -rf "$out"
mkdir -p "$out/app"
wasm-bindgen --target web --no-typescript --out-dir "$out/app" --out-name signalbox_web \
  "${CARGO_TARGET_DIR:-$root/target}/wasm32-unknown-unknown/web/signalbox_web.wasm"
install -m 0644 crates/client-web/index.html "$out/index.html"
# The release image's front runs as its own user: everything world-readable.
chmod -R a+rX "$out"
ls -l "$out" "$out/app"
