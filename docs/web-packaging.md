# Static web / PWA packaging

The web client needs only static hosting, not a Node/Rust server or a bundler.
The package adds installation metadata and an offline service worker. It works
unchanged at a domain root or a repository subpath, such as `/capycanvas/`.
There is no deploy workflow: a separate hosting repository can publish the
finished directory later.

## Build

Use Rust with the Wasm target, Node.js 22 or newer, Bash, and these build tools:

```bash
rustup target add wasm32-unknown-unknown
rustup component add rust-docs
cargo install wasm-bindgen-cli --version 0.2.128 --locked
cargo install cargo-about --version 0.9.2 --features cli --locked
cargo install resvg --version 0.48.1 --locked

node apps/layer-web/package.mjs
```

Run from the repository root. `wasm-bindgen` must match the version in
`Cargo.lock`. The tools are found on `PATH` or in Cargo's bin directory; explicit
paths can be provided through `LAYER_WASM_BINDGEN`, `LAYER_CARGO_ABOUT` and
`LAYER_RESVG`. No GPU is required to build the package or generate its app icons.
Dependency download and license harvesting can require network access.

The build:

1. Compiles only `layer-web` and its dependencies with `--locked` and the
   `web-release` profile; GTK and native rendering dependencies are not built.
2. Uses the same `build.sh`/`wasm-bindgen --target web` step as the development
   launcher. Runtime speed optimizations stay unchanged. Debug data is disabled
   and source prefixes are remapped so panic strings do not expose build-machine
   paths. The generated runtime is scanned before packaging.
3. Copies a curated list of runtime files and renders a 32 px favicon and
   180/192/512 px PNG installation icons from the shared capybara SVG, all with
   the same rounded corners, preserving its separate branding terms. Manifest
   icons use `purpose: any`, since the artwork already has its own shape.
4. Fingerprints every runtime asset with the first 20 hex digits of its own
   SHA-256: `assets/app.<sha>.js`, `assets/style.<sha>.css`, and similarly for
   imported JS, Wasm, SVGs and PNGs. Dependencies are renamed first; rewritten
   JS/CSS is then hashed, so its filename covers its final bytes and dependency
   URLs, including the CSS checkbox mask. A build-time artwork map handles
   dynamic icon/brush-preview lookups.
   HTML and manifest references use the new names; no unversioned runtime
   copies remain. Relative URLs work at any hosting subpath. Identical rebuilds
   retain identical URLs; unrelated assets retain their hashes.
5. Includes project/branding licenses, original notices for the Wasm dependency
   graph (including build-time dependencies), and the installed Rust toolchain's
   complete copyright notice. Generic missing-license placeholders fail the
   package build; the `profiling` notice is retrieved from its recorded upstream
   revision and checked against a pinned checksum.
6. Generates a relative-scope manifest, content-versioned service worker,
   integrity-checked precache list and `.nojekyll` marker. The worker/cache
   version is automatically SHA-256-derived from the complete package and
   worker template on every build; there is no manual version to forget.

Output is **`dist/capycanvas/`**. It is ignored, as are intermediate files in
`target/` and development bindings in `apps/layer-web/pkg/`. No generated files
are committed, and nothing writes into a hosting repository. A successful
rebuild replaces only this generated output; an existing directory without the
`.capy-package` ownership marker is left alone. Failed builds preserve the last
successful package. Keep the marker for local rebuilds; it is not a runtime asset.

The full toolchain copyright document covers components beyond those linked
into Wasm. It intentionally preserves upstream notices rather than making
fragile assumptions about which ones can be discarded, and accounts for most
of the package's uncompressed size. It contains notices, not vendored toolchain
source or binaries.

## Preview and test

```bash
# Preview the package, not the source tree. This does not rebuild it.
python3 -m http.server 4174 --bind 127.0.0.1 --directory dist/capycanvas

# Unit tests: launcher, packaging, cache boundaries and failure handling.
node --test apps/layer-web/run.test.mjs apps/layer-web/package.test.mjs

# Actual Chrome + WebGPU + service workers; starts its own local test server.
node apps/layer-web/test.mjs --package

# Inject unavailable API/adapter/device and delayed startup; verify UI + reload.
node apps/layer-web/test.mjs --package --gpu-startup
```

The browser test uses a fresh temporary Chrome profile on Wayland and serves
the same package at `/` and `/nested/capy/`. It checks manifest installability,
covers the web-only fullscreen button beside Settings (real entry/exit, an
external exit event, unavailable/denied requests, dialogs and GPU drawing after
resizing), and captures both fullscreen themes. It also checks
cold offline Wasm startup with HTTP cache disabled, real GPU ink, all brush
previews in both themes, nonselectable canvas/cursor artwork, real touch taps
without sticky hover and switching back to mouse/pen hover, integrity-mismatched
update recovery, waiting updates that preserve the live session, activation after
leaving the old page, and isolation between
installations on different paths. The test never installs an OS app, injects OS
pointer events, touches an existing browser profile or deploys anything.
Packaging/unit checks do not need a GPU; the browser test does. OS-specific
installation UI, mobile browsers and real offline storage eviction still require
device testing.

The update fixture changes actual JS and CSS, including their SHA filenames,
HTML references and service-worker version. It serves assets with one-year
immutable caching, checks that the old app stays intact while an update waits,
and verifies the new JS executes and CSS applies after activation, including
another cold offline start. Unit tests cover hash reproducibility, dependency
invalidation, artwork/Wasm changes and worker-only updates.

## Runtime and updates

Serve over HTTPS (or localhost for testing), with JavaScript module MIME types
and `application/wasm` for `.wasm`. WebGPU support and a suitable hardware adapter
are required for drawing; packaging cannot enable unsupported browser/GPU features.
The Rust UI session starts before GPU initialization. Without a GPU, menus,
panels and preferences still work, and the canvas area shows theme-matched help.
Startup checks the secure context, WebGPU API, real adapter/device requests and
renderer validation. It does not allowlist browser names, OS versions or GPU
vendors. Rust reports adapter, device and renderer failures separately; a failed
device or shader is not described as a missing adapter. No additional GPU probe
or device is created for detection.
Browser/platform hints select help only, including desktop-mode iPads and
Android client hints. Desktop Chromium gets the compact settings → acceleration
→ restart → reload checklist (Edge uses its own internal addresses). Only Linux
desktop Chromium gets the two unnumbered experimental-flag paragraphs and the
alternative Vulkan-driver guidance, followed by an indented instruction to relaunch
with the `--use-angle=vulkan` command-line option, styled like the copyable
browser addresses and separated from its introduction by the same 8 px gap as
other instruction rows. Its 28 px line height matches rows with Copy buttons.
The next paragraph identifies the GPU debug page. Android
gets Chrome/system-update guidance and iOS/iPadOS gets Safari/system-update guidance.
Other desktop browsers, including Firefox and Safari, get the requested compact
supported-browser list when startup fails, with bold platform labels in this order:
iPadOS, Android, Windows, macOS, Linux (Wayland). Missing WebGPU keeps the subtitle
“WebGPU is not available in this browser.” A failed adapter request uses
“Your browser could not find a GPU adapter.” Insecure access, device failures and
renderer failures also keep their distinct explanations.
The list reflects the upstream availability below, including Firefox on Windows
and Apple Silicon Macs; it does not block a working browser by name. Mobile users
do not get desktop graphics-acceleration switches or Linux flags.
For the Linux workaround, the graphics-report instruction checks **Display Type:
ANGLE_VULKAN**, not **Vulkan: Enabled**. Other desktop Chromium platforms retain
the **WebGPU: Hardware accelerated** instruction. Windows normally uses D3D12
and Apple platforms use Metal; Chromium can also run WebGPU over Vulkan while
its compositor stays on OpenGL. ANGLE Vulkan is troubleshooting guidance, not an
app requirement or a replacement for the actual adapter/device startup checks.
Every displayed internal address has a Copy button.
The help container shares the panels’ background, text color, corners and shadow
in both themes, with roomier padding and vertical centering in the canvas area.
All copyable addresses have the same left indent, and explanatory text uses the
same color as the instructions. Extra space below the subtitle and numbered steps
separates the sections. The default instructions fit without scrolling at 1280×720
and 900×760. Shorter, narrow windows scroll vertically without clipping the last
instruction or Copy button. Browser flags are never enabled by the app. Missing WebGPU and insecure
connections get appropriate explanations; users see only their platform’s guide.
There are no retry or technical-details controls. Reload the page after fixing
browser settings; technical errors are logged to the browser console only.
The document advertises the active light/dark color scheme and matching browser
theme color. A static Dark Reader lock preserves the app’s themes and artwork
colors without adding an extension dependency.
No paint input is queued and no render loop runs before attachment; this is
not a CPU renderer or an invisible drawing mode. GPU initialization is a
separate async object so it never borrows the Rust UI session across `await`.
The current Wasm client does not use shared-memory threads, so it does not need
cross-origin-isolation headers. A later shared-memory implementation would need
a separate hosting review.

### Browser expectations (reviewed 2026-09-07)

These are upstream WebGPU availability expectations, **not a claim that Capy
Canvas has been tested on every device**. Use current stable browsers and system
updates; old GPUs, driver blocklists, managed policies and memory limits can
still prevent startup. Safari/Firefox/mobile GPU rendering needs real-device
release testing. Our automated help tests simulate those browser identities in
Chrome; they do not emulate their GPU implementations.
The unavailable-WebGPU panel was also checked in actual headless Firefox on
Linux, with a disposable profile, in both light and dark themes.

| Platform | Expected browser availability |
| --- | --- |
| Windows x86/x64 | Chrome/Edge with supported D3D12 hardware; Firefox 141+. Chromium Windows ARM64 is still listed behind a flag. |
| macOS | Chrome/Edge; Safari 26+; Firefox 147+ on supported Apple Silicon Macs (Intel Firefox is still listed as Nightly-only). |
| iOS / iPadOS | Safari on iOS/iPadOS 26+. Other browser brands and embedded webviews must expose working WebGPU; desktop Chrome support does not imply Chrome for iOS support. |
| Android | Current Chrome on supported GPUs, chiefly Android 12+ ARM/Qualcomm/Intel. Imagination support starts with Android 16; other GPU families vary. Firefox Android remains experimental. |
| ChromeOS | Current Chrome on supported Vulkan hardware. |
| Linux | Chrome 144+ rollout for Intel Gen12+, expanding in 147 to NVIDIA on Wayland with sufficiently recent drivers. Other configurations may still need flags; Firefox stable is not yet listed as enabled. |

Sources: [GPUWeb implementation status](https://github.com/gpuweb/gpuweb/wiki/Implementation-Status),
[Safari 26 WebGPU](https://webkit.org/blog/17333/webkit-features-in-safari-26-0/#webgpu),
[Firefox 147 release notes](https://www.firefox.com/en-US/firefox/147.0/releasenotes/),
[Chrome Linux rollout](https://developer.chrome.com/blog/new-in-webgpu-144#webgpu_on_linux),
[NVIDIA expansion](https://developer.chrome.com/blog/new-in-webgpu-147-148#webgpu_on_linux_nvidia),
[Chrome troubleshooting](https://developer.chrome.com/docs/web-platform/webgpu/troubleshooting-tips).

### OpenGL versus WebGPU

`chrome://gpu` showing OpenGL enabled and Vulkan disabled does not exclude
hardware WebGPU: the Linux rollout deliberately separates WebGPU's Vulkan
backend from Chromium's OpenGL compositor. Check the WebGPU status and the
actual app startup, not the compositor's backend name.

wgpu's optional browser WebGL2 backend is not a drop-in replacement here: the
material brush reads a storage buffer in its fragment shader, which WebGL2 does
not provide. [Chrome 146 compatibility mode](https://developer.chrome.com/blog/new-in-webgpu-146)
is a different route: WebGPU over OpenGL ES 3.1, initially on Android, with
restricted capabilities. Our wgpu 30 adapter request does not expose the
`featureLevel: compatibility` option, and the brush's multiple color attachments
use different blending states, which the [compatibility subset disallows](https://github.com/gpuweb/gpuweb/blob/main/proposals/compatibility-mode.md#2-color-blending-state-may-not-differ-between-color-attachments-in-a-gpufragmentstate).
Supporting that subset would require renderer and integration work; it is not
enabled by this browser-help fix.

### Offline updates

Only fingerprinted `assets/**` files are suitable for
`Cache-Control: public, max-age=31536000, immutable`. Keep `index.html`, `sw.js`,
`manifest.webmanifest` and the license pages at stable URLs with revalidation
(`Cache-Control: no-cache`, where the host permits header configuration).
The package cannot set a hosting provider's response headers. Never apply a
blanket immutable policy to the whole site. Publish a complete package together;
retain previous hashed assets during a non-atomic rollout so an HTML response
already in flight can still load its dependencies. See
[HTTP cache busting](https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/Caching#cache_busting).

After the first successful online install, the worker precaches the whole
package. Runtime assets are served from that complete version; failed or
integrity-mismatched installs retain the previous working version. New workers
wait for the old app's tabs/windows to close: no `skipWaiting`, forced reload or
mid-drawing JS/Wasm replacement. Cache names include the exact installation
scope; cleanup never deletes a neighboring app's cache. Unknown URLs, APIs,
non-GET requests and user data are not cached by this worker.
Registration uses `updateViaCache: "none"` and precaching uses reload requests
with content integrity, so the worker update does not trust stale HTTP cache
entries. Filename hashes do not force an open drawing to reload. See the
[service-worker lifecycle](https://web.dev/articles/service-worker-lifecycle).

**Offline app availability is not document autosave.** The current drawing is
still in memory; reloading or closing the app discards it. Applied preferences
retain their existing localStorage persistence. Browser storage can be evicted,
so offline availability is not guaranteed permanent storage for artwork.

The development launcher remains `./apps/layer-web/run.sh`. It deliberately
does not register a worker or cache development files. Use different origins
(for example ports 4173 and 4174) for development and packaged previews to avoid
an already-installed worker serving an old package over development files.

When publishing later, copy the complete package together, including hidden
`.nojekyll`, to the hosting repository. Do not hardcode a `CNAME` or edit the
precache contents after building. Prefer atomic publication; integrity checking
protects installed clients from partial updates but cannot make an incomplete
first-time deployment usable.

## References

- [wasm-bindgen: deployment without a bundler](https://wasm-bindgen.github.io/wasm-bindgen/reference/deployment.html)
- [PWA installation requirements](https://developer.mozilla.org/en-US/docs/Web/Progressive_web_apps/Guides/Making_PWAs_installable)
- [Service-worker lifecycle](https://developer.chrome.com/docs/workbox/service-worker-lifecycle)
- [rustc source-path remapping](https://doc.rust-lang.org/rustc/command-line-arguments.html#--remap-path-prefix-remap-source-names-in-output)
- [cargo-about license harvesting](https://embarkstudios.github.io/cargo-about/cli/generate/index.html)
- [Chrome: WebGPU troubleshooting](https://developer.chrome.com/docs/web-platform/webgpu/troubleshooting-tips)
- [Darkly GPU help](https://github.com/darkly-art/darkly/blob/dev/frontend/src/ui/GpuErrorPage.svelte): UX reference only for actionable settings/diagnostics; no code, text or assets imported.
- [Dark Reader site opt-out](https://github.com/darkreader/darkreader/blob/main/CONTRIBUTING.md#disabling-dark-reader-on-your-site)
