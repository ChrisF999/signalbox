# syntax=docker/dockerfile:1
# signalbox: the front (signalbox-server), the game process (signalbox-game),
# the browser client, the three converted TS2 layouts (with their areas, box
# prefixes and line names; Drain with the real Waterloo & City timetable when
# external/wtt/ holds the owner's WTT) and the tutorial lessons. Release
# build: no dev login.
# Build from the repository root:
#   docker build -f deploy/Dockerfile -t local/signalbox:$(git rev-parse --short HEAD) .

# The browser client's toolchain: the wasm32 target and wasm-bindgen-cli at
# the version Cargo.lock pins (scripts/build-web.sh refuses a mismatch).
# scripts/wasm-build builds this stage alone as local/signalbox-wasm-tools:<version>.
FROM rust:1.98-slim-bookworm AS wasm-tools
ARG WASM_BINDGEN_VERSION=0.2.129
RUN rustup target add wasm32-unknown-unknown \
 && cargo install wasm-bindgen-cli --version "=${WASM_BINDGEN_VERSION}" --locked \
 && rm -rf /usr/local/cargo/registry

FROM wasm-tools AS web
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    scripts/build-web.sh /out/web

# The owner's copy of the Waterloo & City line's Working Timetable, if
# external/wtt/ holds one (git-ignored: TfL's document and anything made from
# it never go into the repository; see external/wtt/README.md). ts2-import
# --wtt reads its words and their positions. Without it Drain keeps its TS2
# timetable.
FROM debian:bookworm-slim AS wtt
RUN apt-get update \
 && apt-get install -y --no-install-recommends poppler-utils \
 && rm -rf /var/lib/apt/lists/*
COPY external/wtt/ /wtt/
RUN set -eu; mkdir -p /out; set -- /wtt/*.pdf; \
    if [ "$#" -gt 1 ]; then echo "external/wtt: more than one PDF" >&2; exit 1; fi; \
    if [ -f "$1" ]; then sha256sum "$1"; pdftotext -bbox "$1" /out/wtt.bbox.html; \
    else echo "external/wtt: no WTT; Drain keeps its TS2 timetable"; fi

FROM rust:1.98-slim-bookworm AS build
WORKDIR /src
COPY . .
COPY --from=wtt /out/ /wtt/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p signalbox-server -p ts2-import --bins \
 && mkdir -p /out/bin /out/layouts \
 && cp target/release/signalbox-server target/release/signalbox-game /out/bin/ \
 && for n in liverpool-st drain gretz-armainvilliers; do \
      wtt=""; if [ "$n" = drain ] && [ -f /wtt/wtt.bbox.html ]; then wtt="--wtt /wtt/wtt.bbox.html"; fi; \
      target/release/ts2-import "crates/ts2-import/tests/data/$n.json" -o "/out/layouts/$n.json" \
        --areas "layouts/$n.areas.json" --lines "layouts/$n.lines.json" $wtt || exit 1; \
    done \
 && cp -r lessons /out/lessons \
 && chmod -R a+rX /out/lessons

FROM debian:bookworm-slim
# TLS to Authentik is rustls with bundled webpki roots: no CA bundle, no OpenSSL.
RUN useradd --system --uid 10160 --user-group --home-dir /data --no-create-home --shell /usr/sbin/nologin signalbox \
 && install -d -o signalbox -g signalbox -m 0700 /data
COPY --from=build /out/bin/ /opt/signalbox/bin/
COPY --from=build /out/layouts/ /opt/signalbox/layouts/
COPY --from=web /out/web/ /opt/signalbox/web/
# Checked-out files may be private to their owner: the build stage made them readable.
COPY --from=build /out/lessons/ /opt/signalbox/lessons/
USER signalbox
ENV SIGNALBOX_ADDR=0.0.0.0:9160 \
    SIGNALBOX_DATA=/data \
    SIGNALBOX_LAYOUTS=/opt/signalbox/layouts \
    SIGNALBOX_LESSONS=/opt/signalbox/lessons \
    SIGNALBOX_GAME_BIN=/opt/signalbox/bin/signalbox-game \
    SIGNALBOX_WEB=/opt/signalbox/web
EXPOSE 9160
VOLUME ["/data"]
ENTRYPOINT ["/opt/signalbox/bin/signalbox-server"]
