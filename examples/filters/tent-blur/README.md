# Runtime filter example

`manifest.json` and the two WGSL files define an original, separable triangular
blur. No Rust kernel, coefficient generator or filter ID registration is needed.
The same metadata declares the Radius control, sampling halo and preview preset.

`prepare.wgsl` produces 17 normalized weight records only when Radius changes.
Both image passes in `tent.wgsl` share that persistent table. This deliberately
uses a different kernel from the application's Gaussian family.

The GPU test `runtime_manifest_loads_a_new_filter_and_its_preparation` reads these
files at runtime, renders the new program and checks that radius zero is identity.
Core loading uses `EffectPackage::parse(manifest)?.resolve(read_module)`; hosts
provide `read_module`, and catalog add/replace is staged without mutating the
published catalog. Renderer validation must precede publication.

The application-level entry point is `UiSession::load_effect_package`; it owns
GPU validation and atomic publication. With an already-built GTK executable:

```sh
CAPY_FILTERS_DIR=examples/filters/tent-blur CAPY_FILTERS_MODE=add ./target/debug/layer-linux
```

On web, serve this directory and call
`await layerApp.loadFilters('/tent-blur/manifest.json', 'add')`. Edit the WGSL or
manifest, call with `'replace'`, and inspect `layerApp.state().filter_load` for
completion. No application rebuild is needed. Android's matching host entry is
`CanvasHost.loadFilters(manifest, modules, mode)`. These are programmatic loading
interfaces, not a shader-editor UI.

For an already-built Windows app, from the repository root:

```powershell
$env:CAPY_FILTERS_DIR=(Resolve-Path examples/filters/tent-blur).Path
$env:CAPY_FILTERS_MODE='add'
& ./artifacts/windows/Debug/CapyCanvas.exe
```

The Windows render-owner API `capy_load_filter_directory` supports live reload
through the ordered native command queue. See the
[Windows host commands](../../../apps/layer-windows/README.md#runtime-filter-packages)
and [runtime filters](../../../docs/reference/runtime-filters.md).

These files are covered by the repository's MIT OR Apache-2.0 source license.
