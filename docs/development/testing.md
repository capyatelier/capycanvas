# Testing and performance

[Developer guide](README.md)

Different checks answer different questions. Shared tests exercise document and
editor rules. Host tests exercise native controls and transport. GPU tests check
pixels and rendering cost. Physical-device tests establish whether real pen and
display behavior matches the intended interaction.

## Shared tests

Run focused tests without installing every platform SDK:

```bash
cargo test --locked -p layer-core -p layer-engine -p layer-ui
```

These are useful for edit history, brush placement, workspace behavior and
preference validation. For changes to the rendering contract or shared native
integration, also run the relevant tests in `layer-render`, `layer-render-wgpu`
and `layer-host`. Renderer tests that create a device need a working GPU backend.

A workspace-wide test can be useful in a fully configured environment, but it
also includes platform crates. Start with the affected packages rather than
assuming a Linux machine can build and validate every native client.

## Linux and web interaction

For reorder gestures, follow the required
[drag convention and validation matrix](../ui/drag-and-reorder.md#required-validation-when-implementing).
Test mouse, touch, and pen separately. In particular, assert that tile movement
before a hold does **not** reorder, pen/touch row movement before a hold scrolls,
and handles/title/tab bars drag without waiting. A test that succeeds after a
hold does not establish that the hold is required. Current gaps and relevant
suites are listed in the [drag inventory](../ui/drag-inventory.md).

Build the [Linux client](linux.md) before running its ignored interactive tests.
Run GTK tests individually, with one test thread per process:

```bash
GDK_BACKEND=wayland GSK_RENDERER=vulkan G_DEBUG=fatal-criticals \
  cargo test --locked --release -p layer-linux native_workspace_controls_docking_and_ink \
  -- --ignored --test-threads=1
```

[`apps/layer-linux/src/tests.rs`](../../apps/layer-linux/src/tests.rs) contains
focused document, layer, tool, workspace and settings cases. Native test windows
need a real Wayland session and GPU; they are not headless model tests.

For the web launcher and packager:

```bash
node --test apps/layer-web/run.test.mjs apps/layer-web/package.test.mjs
```

The [web packaging reference](web-packaging.md#preview-and-test) lists real-browser
checks, including GPU startup, PWA behavior and workspace interactions. Its browser
harness has additional Chrome and local environment requirements. Package tests
and live GPU checks are separate validations.

## Native ports

Use the platform guides for [Android](android.md), [Apple](apple.md) and
[Windows](windows.md). Their build tools and acceptance records include device
and UI tests that cannot be replaced by running shared Rust tests on another OS.

For input changes, test a real stylus: pressure, tilt, hover, cancellation and
fast curves. Check touch navigation and resuming after backgrounding or surface
recreation. Injected pointer records are useful for deterministic regressions,
but do not validate the OS driver or hardware delivery path.

## Performance

Keep three measurements separate:

- CPU submission measures how long the engine and renderer take to enqueue work.
- GPU completion measures when the submitted work has finished executing.
- Presentation latency measures when input becomes visible on the display.

A short submission time does not establish a high frame rate. Record the device,
driver, build profile, document, brush and display setup when comparing results.
Pipeline startup and steady-state drawing should also be measured separately.

```bash
cargo run --locked --release -p layer-engine --example hot_path
cargo run --locked --release -p layer-bench -- --scenario painter --repeats 3
```

The first command measures the engine hot path. The GPU harness measures completed
workloads and produces explicit-export review images; it needs a hardware GPU.
[GPU benchmark workloads](gpu-raster-benchmarks.md) describes its scenarios and
options. The [optimization log](../history/optimization-log.md) records earlier
measurements, not performance guarantees for an arbitrary device.

Brush previews can be regenerated with:

```bash
cargo run --locked --release -p layer-bench --bin gpu-bench -- --brush-previews
```

Review both live drawing and replay for brush changes, and both light and dark
UI themes for preview changes. Keep generated diagnostics in ignored `artifacts/`
locations; bundled runtime brush previews are intentionally tracked assets.
