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
3. Copies a curated list of runtime files and renders 180/192/512 px PNG app
   icons from the shared capybara SVG, preserving its separate branding terms.
4. Puts matching JS, Wasm, CSS and artwork under one content-addressed asset
   directory. Relative module/asset URLs require no deployment-specific base
   path and keep a release's JS/Wasm pair together.
5. Includes project/branding licenses, original notices for the Wasm dependency
   graph (including build-time dependencies), and the installed Rust toolchain's
   complete copyright notice. Generic missing-license placeholders fail the
   package build; the `profiling` notice is retrieved from its recorded upstream
   revision and checked against a pinned checksum.
6. Generates a relative-scope manifest, content-versioned service worker,
   integrity-checked precache list and `.nojekyll` marker.

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

# Inject unavailable API/adapter/device and delayed startup; verify UI + retry.
node apps/layer-web/test.mjs --package --gpu-startup
```

The browser test uses a fresh temporary Chrome profile on Wayland and serves
the same package at `/` and `/nested/capy/`. It checks manifest installability,
cold offline Wasm startup with HTTP cache disabled, real GPU ink, all brush
previews in both themes, integrity-mismatched update recovery, waiting updates that preserve
the live session, activation after leaving the old page, and isolation between
installations on different paths. The test never installs an OS app, injects OS
pointer events, touches an existing browser profile or deploys anything.
Packaging/unit checks do not need a GPU; the browser test does. OS-specific
installation UI, mobile browsers and real offline storage eviction still require
device testing.

## Runtime and updates

Serve over HTTPS (or localhost for testing), with JavaScript module MIME types
and `application/wasm` for `.wasm`. WebGPU support and a suitable hardware adapter
are required for drawing; packaging cannot enable unsupported browser/GPU features.
The Rust UI session starts before GPU initialization. Without a GPU, menus,
panels and preferences still work, and the canvas area shows theme-matched help.
Missing secure context, missing WebGPU, adapter failure and device failure have
distinct guidance, with copyable Chrome/Edge settings addresses and collapsible
technical details. Experimental flags are explicitly cautioned, not enabled by
the app. Retry attaches a GPU to the existing session without resetting it.
No paint input is queued and no render loop runs before attachment; this is
not a CPU renderer or an invisible drawing mode. GPU initialization is a
separate async object so it never borrows the Rust UI session across `await`.
The current Wasm client does not use shared-memory threads, so it does not need
cross-origin-isolation headers. A later shared-memory implementation would need
a separate hosting review.

After the first successful online install, the worker precaches the whole
package. Runtime assets are served from that complete version; failed or
integrity-mismatched installs retain the previous working version. New workers
wait for the old app's tabs/windows to close: no `skipWaiting`, forced reload or
mid-drawing JS/Wasm replacement. Cache names include the exact installation
scope; cleanup never deletes a neighboring app's cache. Unknown URLs, APIs,
non-GET requests and user data are not cached by this worker.

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
