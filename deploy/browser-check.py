"""Real-browser check of signalbox's renderers (polish spec section 6).

usage: python3 browser-check.py BASE OUTDIR

Runs headless Chromium three ways against a dev-login front at BASE and
prints one ok/FAIL line per case; exits 1 if any case fails. Screenshots and
each case's browser console go to OUTDIR.

  webgl2  no flags: Chromium has the WebGPU API but no adapter, so the client
          must fall back to WebGL2 (asserted first, so the case cannot pass
          vacuously); the lobby and a Drain game must be drawn.
  webgpu  --enable-unsafe-webgpu: a software WebGPU adapter; the client must
          draw with it (headless WebGPU canvases screenshot blank, so no
          pixels are checked).
  none    --disable-webgl: neither renderer; the page must explain why.
"""

import json
import struct
import sys
import zlib

from playwright.sync_api import sync_playwright

BASE, OUT = sys.argv[1], sys.argv[2]
TRACK_GREY = (0x7D, 0x7D, 0x7D)
HEADCODE_CYAN = (0x39, 0xE0, 0xFF)
NO_ADAPTER = "async () => !(navigator.gpu && await navigator.gpu.requestAdapter())"


def pixels(png):
    """(width, height, rows of (r, g, b)) of an 8-bit, non-interlaced RGB or
    RGBA PNG, as Chromium's screenshots are."""
    assert png[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, data, width, height, channels = 8, b"", 0, 0, 0
    while pos < len(png):
        (length,) = struct.unpack(">I", png[pos : pos + 4])
        kind, body = png[pos + 4 : pos + 8], png[pos + 8 : pos + 8 + length]
        if kind == b"IHDR":
            width, height, depth, colour, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and colour in (2, 6) and interlace == 0, "unexpected PNG format"
            channels = 3 if colour == 2 else 4
        elif kind == b"IDAT":
            data += body
        pos += 12 + length
    raw, stride, rows, prev = zlib.decompress(data), width * channels, [], bytearray(width * channels)
    for y in range(height):
        start = y * (stride + 1)
        kind, line = raw[start], bytearray(raw[start + 1 : start + 1 + stride])
        for i in range(stride):
            a = line[i - channels] if i >= channels else 0
            b, c = prev[i], prev[i - channels] if i >= channels else 0
            if kind == 1:
                line[i] = (line[i] + a) & 0xFF
            elif kind == 2:
                line[i] = (line[i] + b) & 0xFF
            elif kind == 3:
                line[i] = (line[i] + (a + b) // 2) & 0xFF
            elif kind == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 0xFF
        rows.append([tuple(line[x * channels : x * channels + 3]) for x in range(width)])
        prev = line
    return width, height, rows


def count(rows, colour, tolerance=2):
    return sum(1 for row in rows for p in row if all(abs(p[k] - colour[k]) <= tolerance for k in range(3)))


def not_blank(rows):
    """Share of pixels that differ from the most common colour."""
    seen = {}
    for row in rows:
        for p in row:
            seen[p] = seen.get(p, 0) + 1
    total = sum(seen.values())
    return 1.0 - max(seen.values()) / total


def run_case(p, name, flags):
    """Returns a list of failure reasons (empty: ok) and writes OUT/name-*.png
    and OUT/name-console.txt."""
    fails, logs, sockets, wasm = [], [], [], []
    browser = p.chromium.launch(args=flags)
    page = browser.new_page(viewport={"width": 1280, "height": 800})
    page.on("console", lambda m: logs.append(f"{m.type}: {m.text}"))
    page.on("response", lambda r: wasm.append(r.header_value("content-encoding")) if r.url.endswith(".wasm") else None)
    page.on("pageerror", lambda e: logs.append(f"pageerror: {e}"))
    # Every frame passes through untouched; the page's socket is kept so the
    # check can send a lobby message as the client would.
    page.route_web_socket("**/ws", lambda ws: sockets.append(ws.connect_to_server()))
    page.goto(f"{BASE}/auth/dev?user=check")
    no_adapter = page.evaluate(NO_ADAPTER)
    page.wait_for_timeout(4000)
    page.screenshot(path=f"{OUT}/{name}-lobby.png")
    if name == "webgl2":
        if not no_adapter:
            fails.append("this Chromium has a WebGPU adapter: the WebGL2 fallback was not exercised")
        if not any("signalbox: drawing with Gl" in line for line in logs):
            fails.append("the client did not say it draws with Gl")
        _, _, rows = pixels(page.screenshot())
        if not_blank(rows) < 0.01:
            fails.append("the lobby is blank")
        if not sockets:
            fails.append("the client opened no socket")
        else:
            sockets[-1].send(json.dumps({"type": "create_game", "layout": "drain", "seed": 1}))
            page.wait_for_timeout(6000)
            shot = page.screenshot(path=f"{OUT}/{name}-game.png")
            _, _, rows = pixels(shot)
            grey, cyan = count(rows, TRACK_GREY), count(rows, HEADCODE_CYAN)
            if grey < 2000:
                fails.append(f"only {grey} track-grey pixels in the game")
            if cyan < 20:
                fails.append(f"only {cyan} headcode-cyan pixels in the game")
    elif name == "webgpu":
        if no_adapter:
            fails.append("no WebGPU adapter even with --enable-unsafe-webgpu")
        if not any("signalbox: drawing with BrowserWebGpu" in line for line in logs):
            fails.append("the client did not say it draws with BrowserWebGpu")
    else:
        shown = page.evaluate(
            "() => { const f = document.getElementById('fallback'), g = document.getElementById('fallback_gpu');"
            " return [!!f && !f.hidden, !!g && !g.hidden, document.body.innerText]; }"
        )
        if not (shown[0] and shown[1] and "could not start" in shown[2]):
            fails.append(f"no renderer explanation shown: {shown[2][:200]!r}")
    # The front serves a .br or .gz copy when the browser accepts one: the
    # wasm that ran must have been the compressed copy.
    if name != "none" and not any(enc in ("br", "gzip") for enc in wasm):
        fails.append(f"the wasm was not served compressed (content-encoding: {wasm})")
    for line in logs:
        if "panicked" in line or line.startswith("pageerror"):
            fails.append(f"console: {line[:300]}")
    with open(f"{OUT}/{name}-console.txt", "w") as f:
        f.write("\n".join(logs) + "\n")
    browser.close()
    return fails


CASES = [
    ("webgl2", ["--enable-unsafe-swiftshader"]),
    ("webgpu", ["--enable-unsafe-webgpu"]),
    ("none", ["--disable-webgl"]),
]

failed = False
with sync_playwright() as p:
    for name, flags in CASES:
        fails = run_case(p, name, flags)
        print(f"{'ok  ' if not fails else 'FAIL'} {name} {' '.join(flags)}")
        for why in fails:
            print(f"     {why}")
        failed |= bool(fails)
sys.exit(1 if failed else 0)
