# syntax=docker/dockerfile:1
# signalbox: the front (signalbox-server), the game process (signalbox-game)
# and the three converted TS2 layouts. Release build: no dev login.
# Build from the repository root:
#   docker build -f deploy/Dockerfile -t local/signalbox:$(git rev-parse --short HEAD) .

FROM rust:1.98-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p signalbox-server -p ts2-import --bins \
 && mkdir -p /out/bin /out/layouts \
 && cp target/release/signalbox-server target/release/signalbox-game /out/bin/ \
 && for n in liverpool-st drain gretz-armainvilliers; do \
      target/release/ts2-import "crates/ts2-import/tests/data/$n.json" -o "/out/layouts/$n.json" \
        --areas "layouts/$n.areas.json" || exit 1; \
    done

FROM debian:bookworm-slim
# TLS to Authentik is rustls with bundled webpki roots: no CA bundle, no OpenSSL.
RUN useradd --system --uid 10160 --user-group --home-dir /data --no-create-home --shell /usr/sbin/nologin signalbox \
 && install -d -o signalbox -g signalbox -m 0700 /data
COPY --from=build /out/bin/ /opt/signalbox/bin/
COPY --from=build /out/layouts/ /opt/signalbox/layouts/
USER signalbox
ENV SIGNALBOX_ADDR=0.0.0.0:9160 \
    SIGNALBOX_DATA=/data \
    SIGNALBOX_LAYOUTS=/opt/signalbox/layouts \
    SIGNALBOX_GAME_BIN=/opt/signalbox/bin/signalbox-game
EXPOSE 9160
VOLUME ["/data"]
ENTRYPOINT ["/opt/signalbox/bin/signalbox-server"]
