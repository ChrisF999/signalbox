#!/usr/bin/env bash
# Real-browser check of the web client's renderers (polish spec section 6):
# builds the client and a dev-login front from this checkout, runs the front
# on 127.0.0.1 in the stock Rust image, then headless Chromium from the
# Playwright image three ways (WebGL2 fallback, WebGPU, neither; see
# browser-check.py). Prints one ok/FAIL line per case and exits 1 on any
# failure; screenshots and consoles land in target/browser-check/.
# Needs Docker and the Playwright image (pip fetches the matching playwright).
# usage: deploy/browser-check.sh [--no-build]
#   --no-build: use the existing target/web-dist and target/debug binaries
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
port="${SIGNALBOX_CHECK_PORT:-19162}"
image="${SIGNALBOX_PLAYWRIGHT_IMAGE:-mcr.microsoft.com/playwright/python:v1.55.0-noble}"
out="$root/target/browser-check"
name="sbx-browser-check-$$"
if [[ "${1:-}" != "--no-build" ]]; then
  scripts/wasm-build
  scripts/cargo build -q -p signalbox-server --features dev-auth --bins
  scripts/cargo build -q -p ts2-import --bins
fi
for f in target/web-dist/index.html target/debug/signalbox-server target/debug/signalbox-game target/debug/ts2-import; do
  [[ -e $f ]] || { echo "browser-check: $f is missing (run without --no-build)" >&2; exit 2; }
done
rm -rf "$out"
mkdir -p "$out/layouts" "$out/data"
scripts/cargo run -q -p ts2-import -- crates/ts2-import/tests/data/drain.json -o /w/target/browser-check/layouts/drain.json \
  --areas /w/layouts/drain.areas.json --lines /w/layouts/drain.lines.json 2>/dev/null
cleanup() {
  docker stop "$name" >/dev/null 2>&1 || true
  # The browser's files are root-owned: give them back.
  docker run --rm -v "$out:/out" alpine chown -R "$(id -u):$(id -g)" /out >/dev/null 2>&1 || true
}
trap cleanup EXIT
docker run --rm -d --name "$name" --network host -u "$(id -u):$(id -g)" -v "$root:/w" -w /w \
  -e SIGNALBOX_ADDR="127.0.0.1:$port" -e SIGNALBOX_DATA=/w/target/browser-check/data \
  -e SIGNALBOX_LAYOUTS=/w/target/browser-check/layouts -e SIGNALBOX_WEB=/w/target/web-dist \
  -e SIGNALBOX_GAME_BIN=/w/target/debug/signalbox-game -e SIGNALBOX_SESSION_KEY="$(openssl rand -hex 64)" \
  rust:1.98-slim-bookworm target/debug/signalbox-server >/dev/null
for _ in $(seq 100); do
  curl -s -o /dev/null "http://127.0.0.1:$port/auth/logout" && break
  sleep 0.1
done
docker run --rm --network host -v "$root/deploy/browser-check.py:/check.py:ro" -v "$out:/out" "$image" \
  sh -c "pip install -q playwright==1.55.0 >/dev/null 2>&1 && python3 /check.py http://127.0.0.1:$port /out"
