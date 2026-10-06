# Static web / PWA packaging

[Developer guide](README.md) · [Web development](web.md)

The web client needs only static hosting: no Node or Rust server and no bundler.
The package adds installation metadata and an offline service worker, and works
unchanged at a domain root or a subpath such as `/capycanvas/`. The
[release workflow](releasing.md) attaches the package to each release as a ZIP.
`editor.capycanvas.art` runs the latest `main` instead: the **Deploy editor**
workflow in [capycanvas-release](https://github.com/capyatelier/capycanvas-release)
builds a chosen commit, `main` by default, with that repository's license and
package checks and publishes it.

## Build

Use Rust with the Wasm target, Node.js 22 or newer, Bash, and these tools:

```bash
rustup target add wasm32-unknown-unknown
rustup component add rust-docs
cargo install wasm-bindgen-cli --version 0.2.128 --locked
cargo install cargo-about --version 0.9.2 --features cli --locked
cargo install resvg --version 0.48.1 --locked

node apps/layer-web/package.mjs
```

Run from the repository root. `wasm-bindgen` must match `Cargo.lock`. Tools are
found on `PATH` or in Cargo's bin directory; `LAYER_WASM_BINDGEN`,
`LAYER_CARGO_ABOUT` and `LAYER_RESVG` override them. No GPU is needed. Dependency
downloads and license harvesting can need the network.

The build:

1. Compiles only `layer-web` with `--locked` and the `web-release` profile
   (ThinLTO), through the same `build.sh` as the development launcher. Debug
   data is off and source prefixes are remapped so panic strings carry no
   build-machine paths.
2. Copies every runtime module, the stylesheet and artwork, and renders the
   favicon and 180/192/512 px installation icons from the shared capybara SVG.
   The Apple 180 px icon is opaque and full-bleed because iPadOS applies its own
   corner mask.
3. Fingerprints every runtime asset with the first 20 hex digits of its
   SHA-256 (`assets/app.<sha>.js`, `assets/style.<sha>.css`, and so on).
   Dependencies are renamed first, so a rewritten file's name covers its final
   bytes and dependency URLs. Identical rebuilds keep identical URLs.
4. Includes project and branding licenses, original notices for the Wasm
   dependency graph and the Rust toolchain's copyright notice. Placeholder
   licenses fail the build. Web and GTK share `tools/build/about.toml`.
5. Writes a relative-scope manifest, a service worker whose cache version is
   derived from the whole package, an integrity-checked precache list and a
   `.nojekyll` marker.

The source HTML supplies the editor’s description, social preview, site name and
canonical URL at `https://editor.capycanvas.art/`. Packaging preserves this
metadata; the marketing site at `https://capycanvas.art/` has its own canonical.

The packager follows the module graph as the browser does: relative static,
side-effect and dynamic imports, and `new URL(…, import.meta.url)` workers and
Wasm. Adding a module, worker or import needs no packager change. The build
fails, rather than shipping a page that cannot start, when a module references
a missing file, modules import each other in a cycle (content hashes cannot name
each other), `index.html` references an unpublished file, or an `index.html`
insertion point changes. Paths computed at runtime cannot be followed: look them
up through `app.js`'s `asset()`, whose map covers every published file except
modules and the stylesheet.

Output is `dist/capycanvas/`; it and the intermediates in `target/` and
`apps/layer-web/pkg/` are ignored. A rebuild replaces only a directory carrying
the `.capy-package` marker, and a failed build keeps the last good package. The
toolchain notice accounts for most of the uncompressed size; it holds notices,
not toolchain source or binaries.

## Preview and test

Unit fixtures use a small synthetic module graph; an independent resolver also
checks the real runtime modules and worker URLs.

```bash
# Serve the package, not the source tree. This does not rebuild it.
python3 -m http.server 4174 --bind 127.0.0.1 --directory dist/capycanvas

# Launcher and packager unit tests: hashing, cache boundaries, failures.
node --test apps/layer-web/run.test.mjs apps/layer-web/package.test.mjs

# Real Chrome, WebGPU and service workers; each starts its own test server.
node apps/layer-web/test.mjs --package
node apps/layer-web/test.mjs --package --gpu-startup
node apps/layer-web/test.mjs --package --gpu-compatibility
node apps/layer-web/test.mjs --package --workspace
node apps/layer-web/test.mjs --package --customization
node apps/layer-web/test.mjs --package --smoke
```

`--package` serves the build at `/` and `/nested/capy/` in a fresh Chrome
profile. It checks installability, the fullscreen button, cold offline start
with the HTTP cache disabled, GPU ink and brush previews in both themes, touch
taps without sticky hover, recovery from an integrity mismatch, and isolation
between installations on different paths. Its update fixture changes real JS
and CSS and checks that one ordinary refresh runs the new code while a second
open editor keeps its state and can still fetch old assets offline.
`--gpu-startup` injects missing API, adapter and device failures and delayed
startup; `--gpu-compatibility` injects browser exceptions escaping Wasm startup.
The tests never install an OS app, touch an existing browser profile or deploy.
OS installation UI, mobile browsers and real storage eviction need device testing.

## Runtime requirements

Serve over HTTPS (or localhost), with JavaScript module MIME types and
`application/wasm` for `.wasm`. Drawing needs WebGPU on a hardware adapter; the
package cannot enable it. The client uses no shared-memory threads, so it needs
no cross-origin isolation headers.

The Rust UI session starts before the GPU. Without a GPU, menus, panels and
preferences still work and the canvas area shows platform-specific help. There
is no CPU renderer and no queued painting before the GPU attaches. Startup
checks the secure context, the WebGPU API, real adapter and device requests and
renderer validation, and reports each failure separately. It never blocks a
browser by name; browser and platform hints only choose which help to show.

Troubleshooting a user's GPU report:

- On Linux Chromium, relaunching with `--use-angle=vulkan` often enables WebGPU;
  `chrome://gpu` should then show **Display Type: ANGLE_VULKAN**.
- `chrome://gpu` showing OpenGL enabled and Vulkan disabled does not rule out
  hardware WebGPU, which can run on Vulkan while the compositor uses OpenGL.
  Check the WebGPU status and whether the app starts.
- There is no WebGL2 fallback: the material brush reads a storage buffer in its
  fragment shader, which WebGL2 lacks. WebGPU compatibility mode is also
  unusable: wgpu does not request it, and the brush blends its color attachments
  differently, which that mode forbids.

## Hosting and updates

- Serve only fingerprinted `assets/**` with
  `Cache-Control: public, max-age=31536000, immutable`. Keep `index.html`,
  `sw.js`, `manifest.webmanifest`, `apple-touch-icon.png` and the license pages
  at stable URLs with revalidation (`Cache-Control: no-cache` where the host
  allows). Never apply immutable caching to the whole site.
- Publish the complete package together, including `.nojekyll`, preferably
  atomically. During a non-atomic rollout, keep the previous hashed assets so
  HTML already in flight can load. Do not hardcode a `CNAME` or edit the precache
  list after building.
- `apple-touch-icon.png` is a root alias for Apple's conventional lookup. An
  already-installed Home Screen icon may need to be removed and added again.

After the first online visit, the worker precaches the whole package; this also
applies to ordinary tabs, not only installed PWAs. Navigations to `/` and
`index.html` fetch HTML with `cache: "no-cache"`. A network error, a non-HTML or
error response, or a five-second timeout falls back to the active worker's
complete package, and network HTML never replaces that fallback. Hashed assets
come from the current or retained caches by exact URL, otherwise from the
network, so new HTML can load before its worker finishes installing. Only a
successful, integrity-checked precache publishes a new offline release.

New workers call `skipWaiting()` and claim clients but never reload an open
editor; its code and drawing stay until the user refreshes. Old caches stay
available to open tabs and are deleted on a cold navigation with no other
clients in scope. Registration uses `updateViaCache: "none"`. The worker never
caches unknown URLs, non-GET requests or user data, and never clears the
separate storage that holds recovery checkpoints and preferences. Offline
availability is not drawing recovery. The app asks the browser to keep its
storage when it first checkpoints a drawing that was never saved to a file, and
keeps the exit warning while the browser has not agreed; see
[restart snapshots](web.md#build-and-run). Storage the browser has not agreed to
keep can still be cleared, so saved project files remain the durable copy.

`./apps/layer-web/run.sh` registers no worker. Use different origins, such as
ports 4173 and 4174, for development and package previews, so an installed
worker never serves an old package over development files.

## References

- [wasm-bindgen: deployment without a bundler](https://wasm-bindgen.github.io/wasm-bindgen/reference/deployment.html)
- [PWA installation requirements](https://developer.mozilla.org/en-US/docs/Web/Progressive_web_apps/Guides/Making_PWAs_installable)
- [Service-worker lifecycle](https://web.dev/articles/service-worker-lifecycle)
- [HTTP cache busting](https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/Caching#cache_busting)
- [Chrome: WebGPU troubleshooting](https://developer.chrome.com/docs/web-platform/webgpu/troubleshooting-tips)
- [WebGPU compatibility mode](https://github.com/gpuweb/gpuweb/blob/main/proposals/compatibility-mode.md)
