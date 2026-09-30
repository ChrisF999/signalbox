# signalbox — native desktop client design (sub-project D2)

Date: 2026-09-30
Status: draft for review
License: GPL-2.0-or-later
Builds on: `2026-09-30-browser-client-design.md` (D1) and `2026-09-30-server-and-protocol-design.md` (C)

## 1. Context and goal

D2 is a native signalbox app for Linux and Windows. It shows exactly the D1 screens (same
`client_ui::UiApp` over the same `client_core::App`), logs in without the browser cookie, and keeps
itself up to date with signed releases.

### Decisions (owner, 2026-09-30)

| Topic | Decision |
|---|---|
| Platforms | Linux x86_64, Windows x86_64 (no macOS) |
| Login | Browser sign-in with a loopback redirect, brokered by the front (primary); paste a desktop token from the website (fallback); no device-code flow |
| Updates | Self-updating app |
| Signing | CI builds; signing on ra with an Ed25519 key from the vault; the key never reaches the CI runner |
| Order | D2 after D1; its plan is written once D1's client crates are merged |

### Success criteria

1. A `signalbox-users` member downloads the app from `/download`, signs in through the browser on
   Linux and on Windows, and plays exactly as in the browser.
2. When the browser sign-in cannot complete, the paste-token path works.
3. A new release installs itself from the lobby after its manifest signature and file hash are checked;
   a tampered manifest or binary, a wrong key, or an older version is refused and leaves the current
   app running.
4. Desktop tokens survive server restarts, expire, and can be listed and revoked by their owner.

## 2. The app (`client-desktop`, native only)

- eframe native runner with wgpu (Vulkan or GL on Linux, DX12 on Windows), running `client_ui::UiApp`.
- A WebSocket `Transport` on `tungstenite` in a background thread, feeding `client-core` through the
  D1 `Transport` trait. No tokio in the app.
- Settings (server URL defaulting to `https://ra.tail3e0c1e.ts.net:50160`, window size, zoom) in
  `~/.config/signalbox/` on Linux and `%APPDATA%\signalbox\` on Windows.

## 3. Login

### 3.1 Browser sign-in (primary)

1. The app listens on `127.0.0.1:<random port>`, makes a PKCE verifier/challenge (S256) and a random
   `state`, and opens `/auth/desktop/start?port=P&challenge=C&state=S` in the default browser.
2. The front validates the parameters (port 1024–65535, challenge and state shape), stores them in a
   short-lived signed cookie, and runs the normal C2 Authentik login including the `signalbox-users`
   and `robot` checks.
3. On success the front creates a one-time code (256-bit, valid 60 s, bound to the challenge and user)
   and redirects to `http://127.0.0.1:P/callback?code=X&state=S`. The browser tab shows "You can close
   this tab".
4. The app checks `state`, then POSTs `{code, verifier}` to `/auth/desktop/exchange`; the front checks
   the code is unused and unexpired and that `SHA256(verifier)` matches the challenge, and answers with a
   desktop token. The code is burned whether or not the exchange succeeds.
5. The listener gives up after 5 minutes.

### 3.2 Paste a token (fallback)

A logged-in page `/desktop` offers "Create desktop token" (with a label); the token is shown once for
copying. The app has a "Paste a token" field.

### 3.3 Desktop tokens (server)

- 256-bit random values; only their SHA-256 is stored, in `/data/tokens.sqlite` (survives restarts).
- Row: user, label, created, last used, expires (30 days, extended on use).
- `/desktop` lists the user's tokens and revokes them.
- `/ws` accepts `Authorization: Bearer <token>` as well as the session cookie; revoked, expired, unknown
  or `robot` tokens get 401.
- Group membership is checked when a token is issued, not on every connection: removing someone from
  `signalbox-users` takes effect when their tokens expire or are revoked.

### 3.4 Storage on the client

The token is kept in the OS keyring (Windows Credential Manager; Secret Service on Linux). Without a
keyring it falls back to a file readable only by the user, with a warning in the app.

## 4. Updates

### 4.1 Versions and manifest

- The app knows its version and the protocol version it speaks. The front serves
  `/download/manifest.json` and `/download/manifest.json.sig`: latest version, minimum protocol, and per
  platform (`linux-x86_64`, `windows-x86_64`) the file name, size and SHA-256.
- Server protocol newer than the app understands → the lobby shows "Update required"; a newer version
  with a compatible protocol → "Update available". Updates run only from the lobby, never mid-game.

### 4.2 Self-update

1. Download manifest and signature; verify the Ed25519 signature with the public key compiled into the
   app.
2. Refuse a manifest whose version is not newer than the running one (no downgrade or replay).
3. Download the binary; verify size and SHA-256 against the signed manifest.
4. Replace the running executable (Windows: the self-replace technique, since a running exe cannot be
   overwritten; Linux: rename over it), then restart.
5. Any failure leaves the current version in place and shows the reason.

### 4.3 Signing and publishing (controller, owner-gated)

- An Ed25519 key pair made once: private key at `/srv/vault/creds/signalbox/release.key`; public key
  committed as `crates/client-desktop/release.pub` and compiled into the app.
- CI builds the Linux binary natively (Debian bookworm, glibc 2.36) and the Windows `.exe` by
  cross-compiling for `x86_64-pc-windows-gnu` (mingw) in Docker; binaries are CI artifacts.
- `scripts/release-desktop.sh` on ra fetches the artifacts for a tagged commit, writes and signs the
  manifest with the vault key, and copies manifest, signature and binaries to `/data/downloads`, which
  the front serves. The owner runs or approves every release.
- `/download` (behind login) links the latest builds and explains the first install, including the
  Windows "unknown publisher" warning (no code-signing certificate).
- The CI runner image gains the Windows target and mingw (owner-gated infra, like D1's wasm toolchain).

## 5. Testing

- `client-desktop` (native): manifest parsing; signature good / tampered / wrong key; downgrade refused;
  hash mismatch refused; loopback listener (state mismatch, timeout, single use); keyring fallback.
- Server: desktop login endpoints end to end with dev auth standing in for Authentik; code single use,
  expiry and PKCE mismatch; token hashing, expiry, extension and revocation; `/ws` with a bearer token,
  and 401 for revoked, expired, unknown and `robot` tokens.
- End to end (CI): the desktop transport, headless, logs in with the paste-token flow against the real
  front and sets a route on Liverpool Street.
- Manual after a release: install on Linux and on ptah (Windows), sign in through the browser, update
  from one version to the next, and check a tampered manifest is refused.

## 6. Out of scope

macOS; Windows code signing; updating mid-game or in the background; store listings; device-code login;
per-connection group re-checks against Authentik.
