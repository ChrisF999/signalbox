#!/usr/bin/env bash
# Build the browser client (crates/client-web) into OUT (default
# target/web-dist; not target/web, which is cargo's own directory for the
# `web` profile): index.html, app/signalbox_web.js, app/signalbox_web_bg.wasm,
# and beside each a brotli (.br, -q 11) and a gzip (.gz, -9 -n) copy that
# the front serves to browsers that accept them. The wasm has no name or
# producers section (panics lose function names; 8.3 -> 2.1 MB brotli).
# Needs the wasm32-unknown-unknown target, wasm-bindgen-cli at the version
# Cargo.lock pins, brotli and gzip: the wasm-tools stage of deploy/Dockerfile
# has them all (locally, run this through scripts/wasm-build). Arguments after OUT
# go to cargo (CI passes --offline).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd -P)"
out="$(realpath -m "${1:-$root/target/web-dist}")"
shift || true
# OUT is emptied below: never /, the checkout or anything above it.
stem="${out%/}" # "" for /
case "$root/" in
  "$stem"/*)
    echo "build-web: refusing to use $out as OUT: it would delete the repository at $root" >&2
    exit 2
    ;;
esac
# ... nor cargo's target directory or the profile directories in it (its
# build cache: emptying it makes every build start from scratch).
tdir="$(realpath -m "${CARGO_TARGET_DIR:-$root/target}")"
case "$out/" in
  "$tdir/" | "$tdir"/debug/* | "$tdir"/release/* | "$tdir"/web/* | "$tdir"/wasm32-unknown-unknown/*)
    echo "build-web: refusing to use $out as OUT: it is cargo's build directory (use e.g. $tdir/web-dist)" >&2
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
for tool in brotli gzip; do
  if ! command -v "$tool" >/dev/null; then
    echo "build-web: $tool is missing; the wasm-tools stage of deploy/Dockerfile installs it" >&2
    echo "build-web: (an older local tools image lacks brotli: docker image rm local/signalbox-wasm-tools:$want and run scripts/wasm-build again)" >&2
    exit 1
  fi
done
cargo build -p signalbox-client-web --target wasm32-unknown-unknown --profile web --locked "$@"
rm -rf "$out"
mkdir -p "$out/app"
wasm-bindgen --target web --no-typescript --remove-name-section --remove-producers-section --out-dir "$out/app" --out-name signalbox_web \
  "${CARGO_TARGET_DIR:-$root/target}/wasm32-unknown-unknown/web/signalbox_web.wasm"
install -m 0644 crates/client-web/index.html "$out/index.html"
# Precompressed copies; the same input always gives the same bytes (gzip -n
# leaves out the name and time).
for f in "$out/index.html" "$out"/app/*; do
  brotli -q 11 -f -o "$f.br" "$f"
  gzip -9 -n -c "$f" >"$f.gz"
done
# The release image's front runs as its own user: everything world-readable.
chmod -R a+rX "$out"
ls -l "$out" "$out/app"
