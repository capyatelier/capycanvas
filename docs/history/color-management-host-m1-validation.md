# Raster foundation host validation

This records the Web and Android follow-up to the GTK qualification in
[color-management-gtk-m1-validation.md](color-management-gtk-m1-validation.md).
The GTK and integration checkpoint was pushed to main as `c1ec69e` before these
ports. The old Web comparison is main `1756cfa`, before the raster merge.

## Web

The browser uses the shared encoded sRGB8 renderer, immutable raster revisions,
indexed project format and bounded history. GPU objects remain on their owning
browser event loop. GPU mappings are awaited asynchronously; a dedicated Wasm
worker hashes and compresses bounded tile blocks. A separate worker validates
project input, encodes project/PNG output, and writes recovery transactions.
Ordinary frames do not await those workers. A frame that actually restores
pending tiles is retained and retried without consuming another contact.

Project files are transferred to the validation worker without first copying the
archive into the input owner's Wasm memory. Immutable compressed tiles and source
assets cross independent Wasm memories in blocks, yielding after 4 MiB of copying.
The worker protocol is private: persisted files still use only the shared,
integrity-checked `Project::read`/`Project::write` format. Browser project limits
are 256 MiB source bytes, 512 MiB raw raster samples and 8192 tiles; the device's
texture limit also applies. These limits do not describe total process RSS.

Recovery uses strict IndexedDB transactions on the file worker and a Web Lock
per live tab. A closed tab's checkpoint can be recovered without racing a live
tab. Recovery preserves dirty-state protection and has no user file location.
GPU replacement retains the UiSession and history through `replace_renderer`;
`Suspend` discards unsubmitted contacts, and device loss exposes Restart Canvas.

Validation on Chrome 150 / NVIDIA driver 610.57.04, RTX PRO 6000 Blackwell:

- Release Wasm build; 45 core, 49 engine and 356 UI/session tests passed.
- 12 Web packaging tests passed, including dependency fingerprint rewriting.
  The production PWA build includes all 251 precached files and both raster workers.
- Real hardware WebGPU offscreen acceptance passed: paint and save, exact tile
  hashes after reopen, identical full-canvas PNGs, undo/redo, corrupt archive
  rejection without adoption, renderer replacement, and an abandoned tab's
  IndexedDB recovery after reload. Recovered pixels match and remain unsaved.
- Frame creation sampled 2304 moving frames and 15 contact completions per canvas,
  in three consecutive runs with five contacts each. The first contact warms the
  path and is excluded from moving-frame percentiles. New-backend runs include
  concurrent immutable recovery encoding and IndexedDB publication. Inputs are
  synthetic; timing includes the Wasm host frame/presentation encoding, not GPU
  completion, physical input latency or display presentation.
- On 2048 × 1536, old moving-frame p99 was 0.40/0.30/0.30 ms; new was
  0.50/0.30/0.30 ms. New maximum was 1.20 ms, contact-completion maximum 1.90 ms,
  and concurrent recovery writes took 40.0/12.1/12.2 ms. Steady-state p99 is
  unchanged at the browser clock's 0.1 ms resolution.
- On 6000 × 4000, new moving-frame p99 was 0.40/0.60/0.40 ms; maximum 1.90 ms,
  contact-completion maximum 2.60 ms, and recovery writes took 19.1/29.1/39.9 ms.
  The matching old p99 was 0.40/0.30/0.30 ms; the median increase is 0.10 ms,
  below the predeclared 0.20 ms noise allowance. These are sparse paint workloads; the GTK dense multi-layer qualification
  remains separate and is not a browser memory or mobile performance claim.

**Presentation limitation:** the strict editor screenshot test fails before
painting on both old main and this port in this environment. Chrome reports
`A valid external Instance reference no longer exists` and presents a black
headless canvas while hardware GPU exports remain correct. The explicit
`--offscreen-raster` test mode reports this known failure and tolerates only that
specific diagnostic; every other error fails. It does not qualify display
presentation. The ordinary editor/screenshot suite remains strict. A private
Wayland test also encountered the installed Chrome GTK 4 startup failure; GTK 3
avoids that launch crash but does not establish Vulkan presentation here.

Local evidence is in `artifacts/color-m1/web-raster-recovery.txt`,
`web-before-editor.txt`, `web-before-frame-times.json`, `web-frame-times.json`,
`web-24mp-frame-times.json` and the accompanying benchmark logs. Reproduce with:

```sh
bash apps/layer-web/build.sh
node apps/layer-web/package.test.mjs
LAYER_WEB_URL=http://127.0.0.1:4173 node apps/layer-web/test.mjs --headless --raster --offscreen-raster
LAYER_WEB_URL=http://127.0.0.1:4173 node apps/layer-web/test.mjs --headless --raster-bench --offscreen-raster
# Add --large-raster for a 6000 × 4000 canvas.
```
