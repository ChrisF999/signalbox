# Deploying signalbox on ra

One container runs the front (`signalbox-server`), which starts one
`signalbox-game` process per live game. It listens on `127.0.0.1:9160` only;
`tailscale serve` publishes it on the tailnet as
`https://ra.tail3e0c1e.ts.net:50160`. Login is Authentik OIDC, restricted to the
group `signalbox-users` (by an Authentik binding and again by the front, which
checks the `groups` claim). Nothing is on the public edge.

| File | What |
|---|---|
| `Dockerfile` | release image: both binaries, the browser client (`/opt/signalbox/web`, built by the `wasm-tools` and `web` stages) and the converted layouts `liverpool-st`, `drain`, `gretz-armainvilliers` with their areas, box prefixes and line names from `layouts/`, and Drain with the real Waterloo & City timetable when `external/wtt/` holds the owner's WTT (its `wtt` stage runs `pdftotext -bbox`; without one, Drain's TS2 timetable) (no dev login) |
| `Dockerfile.dockerignore` | keeps `target/`, `.cargo-home/`, `.git/` out of the build context (`external/wtt/` stays in, for the `wtt` stage) |
| `docker-compose.yml` | the service; copied to `/opt/stack/apps/signalbox/` |
| `authentik/signalbox-oidc-blueprint.yaml.example` | OAuth2 provider + application; rendered with the real secret into the vault |
| `authentik/signalbox-access.yaml.example` | the `signalbox-users` group and its binding, for the stack's blueprints |
| `smoke.sh` | checks a running front from outside |
| `browser-check.sh`, `browser-check.py` | real-browser check of the web client's renderers against a throwaway dev-login front built from the checkout |

## Configuration

All from the environment (see `crates/server/src/config.rs`); a missing or bad
value stops the front with exit code 2 and one line saying what is wrong.

| Variable | Where | Value |
|---|---|---|
| `SIGNALBOX_ADDR` | image | `0.0.0.0:9160` |
| `SIGNALBOX_DATA` | image | `/data` (volume: `saves/`, `sockets/`) |
| `SIGNALBOX_LAYOUTS` | image | `/opt/signalbox/layouts` |
| `SIGNALBOX_LESSONS` | image | `/opt/signalbox/lessons` (tutorial lessons, `<id>/lesson.json` + `world.json`; read and checked once at start, a broken one is left out with one log line; missing = no tutorials) |
| `SIGNALBOX_GAME_BIN` | image | `/opt/signalbox/bin/signalbox-game` |
| `SIGNALBOX_WEB` | image | `/opt/signalbox/web` (the browser client: `index.html`, `app/`, each with `.br` and `.gz` copies served by `Accept-Encoding`; read once at start) |
| `SIGNALBOX_ADMINS` | compose | `skye` (comma-separated usernames that may delete any saved or crashed game; a game's creator may always delete their own; default nobody) |
| `SIGNALBOX_PUBLIC_URL` | compose | `https://ra.tail3e0c1e.ts.net:50160` (redirect URI = this + `/auth/callback`) |
| `OIDC_ISSUER` | compose | `https://auth.skyes.lgbt/application/o/signalbox/` |
| `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET` | vault `oidc.env` | must match the provider blueprint |
| `SIGNALBOX_SESSION_KEY` | vault `oidc.env` | hex, at least 64 bytes; signs the cookies |

Sessions live in memory: restarting the front logs everyone out (games are saved
and resume on the next join).

The container's root filesystem is read-only (`read_only: true` in the
compose file); only the `/data` volume is writable. That is enough for the
saves: the bundled SQLite is built with `SQLITE_TEMP_STORE=2`, so its temporary
tables and indices stay in memory, and its journal files sit next to each save
in `/data/saves`.

## Secrets

`/srv/vault/creds/signalbox/` (root, 0700) on the LUKS vault holds `oidc.env`
and `oidc-blueprint.yaml` (root, 0600). Compose reads `oidc.env` at `up` time,
so the vault must be mounted for `up`; nothing is bind-mounted from the vault,
so Docker's autostart at boot does not need it.

To create them (new client secret and session key):

```bash
sudo bash -c 'set -euo pipefail; umask 077
  d=/srv/vault/creds/signalbox; install -d -m 0700 "$d"
  test ! -e "$d/oidc.env" || { echo "oidc.env exists; not overwriting"; exit 1; }
  id=$(openssl rand -hex 20); secret=$(openssl rand -hex 32); key=$(openssl rand -hex 64)
  printf "OIDC_CLIENT_ID=%s\nOIDC_CLIENT_SECRET=%s\nSIGNALBOX_SESSION_KEY=%s\n" "$id" "$secret" "$key" > "$d/oidc.env"
  t=$(cat /home/skye-fi/projects/signalbox/deploy/authentik/signalbox-oidc-blueprint.yaml.example)
  t=${t//"<OIDC_CLIENT_ID>"/$id}; t=${t//"<OIDC_CLIENT_SECRET>"/$secret}
  printf "%s\n" "$t" > "$d/oidc-blueprint.yaml"'
```

(Pure bash substitution, so the secret never appears in a process list. It
refuses to overwrite an existing `oidc.env`: a new client secret would no longer
match the provider Authentik already has.)

## Authentik

Follow `/opt/stack/apps/authentik/blueprints/README.md`. In order:

1. The provider and application, from the vault:

   ```bash
   dir=$(sudo mktemp -d)
   sudo install -m 0644 /srv/vault/creds/signalbox/oidc-blueprint.yaml "$dir/signalbox-oidc.yaml"
   sudo docker cp "$dir/signalbox-oidc.yaml" authentik-server:/blueprints/signalbox-oidc.yaml
   sudo rm -r "$dir"
   docker exec authentik-server ak apply_blueprint signalbox-oidc.yaml
   docker exec -u 0 authentik-server rm /blueprints/signalbox-oidc.yaml
   ```

2. The group: append the group entry of `authentik/signalbox-access.yaml.example`
   to `10-access-groups.yaml` (members are the owner's decision) and apply it as
   that README shows.
3. The binding: append the binding entry to `40-access-bindings.yaml` and apply
   it. Without a binding Authentik admits every authenticated user to the app.
4. Add `signalbox` to the README's list of vault-held OIDC blueprints.

## Build and run

Before building the release image: `deploy/browser-check.sh` from the checkout being deployed; all three cases must say `ok`. It needs Docker and `mcr.microsoft.com/playwright/python:v1.55.0-noble`; it cannot run in Forge CI (the runner has no Docker and no internet).

```bash
cd /home/skye-fi/projects/signalbox            # at the commit to deploy
rev=$(git rev-parse --short HEAD)
docker build -f deploy/Dockerfile -t local/signalbox:$rev -t local/signalbox:current .
sudo install -d -o root -g docker -m 2755 /opt/stack/apps/signalbox
sudo install -o root -g docker -m 0644 deploy/docker-compose.yml /opt/stack/apps/signalbox/docker-compose.yml
cd /opt/stack/apps/signalbox && sudo docker compose up -d
docker logs signalbox            # "signalbox-server: listening on 0.0.0.0:9160"
sudo -n tailscale serve --bg --https=50160 http://127.0.0.1:9160
/home/skye-fi/projects/signalbox/deploy/smoke.sh https://ra.tail3e0c1e.ts.net:50160 303
```

Then open `https://ra.tail3e0c1e.ts.net:50160/` in a browser: after the
Authentik login the signalbox lobby loads (WebGPU in Chrome/Edge, WebGL2 in
Firefox).

## The Waterloo & City timetable (optional)

Before `docker build`, copy the owner's WTT PDF into `external/wtt/` of the
checkout being built (the only PDF there; `external/wtt/README.md`). The
build log shows its sha256 and the converter's summary (`Wednesday, 585
trips of 7 trains; …`, `574 services, 5 entries from 05:40:00; …`); a WTT
that fails the checks fails the build. Without a PDF the log says `Drain
keeps its TS2 timetable`. An image built with it holds a timetable made
from TfL's document: it stays on ra and is never pushed to a public
registry. Old Drain saves keep the timetable they were created with.

## What `smoke.sh` expects

`/` answers 303 to `/auth/login`; `/ws` and the web client's files under
`/app/` 401 without a session; `/auth/dev` 404
(the release image has no dev login); `/auth/logout` 200; `/auth/login` 303 to
Authentik's authorize endpoint (503 if Authentik is unreachable). The display
name `robot` is refused. Game processes run in their own process group, and on
SIGTERM the front stops accepting connections before it shuts the games down.

## Update and roll back

Build a new `local/signalbox:<rev>`, retag it `current`, and
`sudo docker compose up -d` in `/opt/stack/apps/signalbox`. The front saves
every game on SIGTERM (up to 10 s; compose allows 20). To roll back, retag the
previous `<rev>` as `current` and `up -d` again. Saves are forward-compatible
only within save schema 2; a newer schema refuses old saves with
`unsupported save schema N`.

## Local check of an image (no Authentik)

```bash
docker build -f deploy/Dockerfile -t local/signalbox:test .
docker run --rm -d --name signalbox-test -p 127.0.0.1:19160:9160 \
  -e SIGNALBOX_SESSION_KEY=$(openssl rand -hex 64) -e OIDC_ISSUER=http://127.0.0.1:9/ \
  -e OIDC_CLIENT_ID=x -e OIDC_CLIENT_SECRET=y -e SIGNALBOX_PUBLIC_URL=http://127.0.0.1:19160 \
  local/signalbox:test
deploy/smoke.sh http://127.0.0.1:19160 503
docker exec signalbox-test ls /opt/signalbox/layouts /opt/signalbox/web /opt/signalbox/web/app
docker stop signalbox-test
```
