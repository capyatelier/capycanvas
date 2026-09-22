# Vulkan rendering with native Windows presentation — 2026-09-22

**Status: deferred investigation; implementation has not started.** The user
requested that this analysis be preserved for future work. Windows continues to
use D3D12 and DXGI; this record does not authorize or implement a backend switch.

The investigation inspected and measured source commit
0d00689c4fb5787cdb44ffc7b2e89cedfc0bf9bf. This record was added after updating to
0591b22c; measurements were not repeated on that later source. Related records:
[Windows brush qualification](../development/windows-features-20260922.md) and
[Windows pen latency](../development/windows-pen-latency-20260920.md).

## Assessment

There is a concrete candidate for combining Vulkan's measured rendering/backend
advantage with native Windows UI composition: render into Windows-owned shared
FP16 textures and present those allocations through the Windows 11 Composition
Swapchain API (IPresentationManager). Resource import, presentation-buffer
registration, and one direction of GPU fence signaling succeeded on the test
machine. Full image rendering, WinUI attachment, and displayed-frame latency
through this combined path remain untested.

The design can avoid an additional application copy between Vulkan rendering and
Windows presentation. It does not guarantee that DWM will avoid composition, that
our window will qualify for direct scanout, or that it will beat current immediate
DXGI presentation. Adoption should depend on latency as well as throughput.

## Rendering evidence and limits

Test system: Intel Core i7-1255U, Intel Iris Xe integrated GPU, 16 GiB RAM,
driver 32.0.101.6737, Windows 11 Home build 26200, AC power. The native Intel
driver reports Vulkan 1.4.309. Microsoft Dozen (Vulkan-on-D3D12) is also
enumerated; future adapter selection must distinguish them.

Follow-up G-Pen measurements used a Release build, blank 9504 × 6336 document,
1000 px brush, 1600 × 1000 fitted FP16 viewport, 480 timestamped samples on the
same ellipse, pressure 1, zero tilt, and 16 ms shared prediction. Three repeats
followed a 128-sample warmup. Rates include the final GPU drain and exclude
initialization, readback and history verification.

| Configuration | D3D12 completed generations/s | Vulkan completed generations/s |
| --- | ---: | ---: |
| Default cache admission, 2 samples/generation, drain every generation | 78.55 | 110.13 |
| Default cache admission, 1 sample/generation, drain every generation | 87.83 | 116.95 |
| Default cache admission, 2 samples/generation, drain every 2 generations | 99.49 | 120.05 |
| Default cache admission, 1 sample/generation, drain every 2 generations | 115.10 | 129.76 |
| Full display cache, 1 sample/generation, drain every 2 generations | **112.89** | **173.85** |

The last row is a 54% Vulkan throughput advantage on the same machine. Explicitly
allowing 1536 MiB selected the full display pyramid on both backends; retained
composition storage was 1,286,940,672 bytes. Source/upload admission ceilings still
differed, so this blank-source test does not establish memory-policy equivalence
for imported photographs. The experimental allowance is not a production proposal.

Separate instrumented full-cache runs used two samples/generation and a drain
after each generation:

| Metric | D3D12 | Vulkan |
| --- | ---: | ---: |
| Mean CPU submission | 4.95 ms | 1.39 ms |
| Mean CPU composition phase | 2.365 ms | 0.179 ms |
| Recent GPU brush/composition span | 4.880 ms | 4.063 ms |

GPU spans exclude the final viewport pass and average the final 120 samples of
each repeat. These results expose substantial CPU/backend overhead differences;
they do not isolate shader execution speed. The CPU composition timer measures
canvas preparation/encoding, not DWM composition or finished-buffer-to-screen time.

All 14 configurations × 3 strokes passed exact native tile-digest Undo/Redo.
Compared backend captures differed by at most 1/255 per RGB channel at seven pixel
locations out of 1.6 million; alpha was identical. These renderer checks do not
validate the proposed shared-buffer presenter.

Local evidence remains under ignored artifacts/windows/gpen-investigation:
report.md, measurements.json, retained gpen_probe.rs, CSVs, logs and PNGs. Those
files are not included in a fresh clone. The temporary probe was derived from
[brush_frames.rs](../../crates/layer-render-wgpu/examples/brush_frames.rs).
The final comparison used PROBE_SAMPLES=1, PROBE_FENCE_EVERY=2,
PROBE_DISPLAY_MIB=1536 and PROBE_TELEMETRY=0, with 480 generations and 3 repeats.
A future maintained diagnostic should capture this configuration; the existing
example must not be assumed to accept these probe-specific variables.

The user's MovinkPad Pro 14 result of 150–200 generations/s was not rerun.
Android's live-input/presentation workload differs. Neither that comparison nor
the offscreen 173.85 result establishes native WinUI latency, physical
input-to-photon latency, or a hardware ranking between devices.

## Hardware interoperability probes

Temporary in-memory Python/ctypes probes called installed Vulkan and Windows APIs.
They did not modify application code or render/present images.

| Probe on this machine | Observed result |
| --- | --- |
| Native Vulkan external Win32 memory/semaphore extensions and timeline semaphores | Supported |
| Optimal-tiled RGBA16F import using D3D11_TEXTURE | Importable; dedicated allocation required |
| D3D11-owned 1600 × 1000 RGBA16F texture imported and bound to a Vulkan image | Passed; allocation 13,107,200 bytes |
| Same import with SHARED_DISPLAYABLE enabled | Passed |
| Register the same allocation with IPresentationManager::AddBufferFromResource while importing it into Vulkan | Passed for ordinary and displayable FP16 textures |
| Vulkan GPU submission signaling an imported Windows shared fence | Passed; D3D11 observed completed value 1 |
| D3D11 displayable-surface capability | Supported; shared resource tier 3 |
| Presentation factory and manager creation | Passed; presentation support reported true |
| IsPresentationSupportedWithIndependentFlip | True; actual application presentation mode remains untested |
| D3D12_RESOURCE external image-format queries | VK_ERROR_FORMAT_NOT_SUPPORTED for the tested combinations |

Reproduction parameters for future probes:

- Image queries: optimal 2D images, sampled plus color-attachment plus transfer
  source/destination usage; RGBA16F, RGBA8 UNORM, BGRA8 UNORM and RGBA32F.
  D3D11-texture queries reported importable/dedicated-only, not exportable.
  D3D12-resource queries failed for these combinations; this does not rule out
  every form of D3D12 interoperability.
- Actual imports: D3D11 R16G16B16A16_FLOAT, one mip/slice/sample, default usage,
  render-target and shader-resource bindings, SHARED | SHARED_NTHANDLE, optionally
  SHARED_DISPLAYABLE. Export an NT handle and use a dedicated Vulkan allocation.
- Presentation device: BGRA support, single-threaded access, and disabled internal
  threading optimizations, following Microsoft's composition-swapchain example.
- Fence: create a shared D3D11 fence and import it into a Vulkan timeline
  semaphore with D3D12_FENCE handle type, which also covers D3D11 fences. Submit
  an empty GPU operation signaling value 1, wait for diagnostic completion, and
  observe ID3D11Fence::GetCompletedValue. This proves that signaling direction,
  not image visibility, bidirectional reuse, or display retirement. The CPU wait
  is diagnostic only. See the
  [Khronos semaphore handle definitions](https://docs.vulkan.org/refpages/latest/refpages/source/VkExternalSemaphoreHandleTypeFlagBits.html).

## Proposed presentation path

~~~mermaid
flowchart TD
    A[Vulkan brush and document composition] --> B[Vulkan viewport and color conversion]
    B --> C[Windows-owned shared FP16 texture]
    C --> D[GPU fence handoff]
    D --> E[Windows composition presentation manager]
    E --> F[WinUI SwapChainPanel]
    F --> G[DWM composition or direct hardware scanout]
~~~

Use a small pool of D3D11-owned shared displayable textures, imported once per
size/device generation. Keep document tiles, display pyramids, filters and brush
work private to Vulkan. Only the final viewport crosses the API boundary:
1600 × 1000 FP16 contains 12.8 MB of pixel data, rather than the full 61 MP
document. Preserve final color conversion, cursor and selection behavior.
Buffer-pool size must not become permission to queue stale frames.

IPresentationManager registers application-created textures and presents them
through a composition surface handle. FP16 is supported; displayable allocations
permit consideration for hardware presentation. The same allocation can
therefore be Vulkan output and a Windows presentation buffer. This architectural
inference is supported by the probes, not an end-to-end displayed-frame result.
See Microsoft's [programming guide](https://learn.microsoft.com/en-us/windows/win32/comp_swapchain/comp-swapchain)
and [buffer-creation examples](https://learn.microsoft.com/en-us/windows/win32/comp_swapchain/comp-swapchain-examples).

WinUI exposes
[ISwapChainPanelNative2::SetSwapChainHandle](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/win32/microsoft.ui.xaml.media.dxinterop/nf-microsoft-ui-xaml-media-dxinterop-iswapchainpanelnative2-setswapchainhandle).
Microsoft's [SwapChainElement implementation](https://github.com/microsoft/microsoft-ui-xaml/blob/main/dxaml/xcp/core/core/elements/SwapChainElement.cpp)
routes that handle to CreateCompositionSurfaceForHandle. This supports preserving
the panel and its independent input source. Attachment occurs on the UI thread;
ordinary presents should not require reattachment or a XAML render tick. Runtime
validation with our presentation manager and installed WinUI version remains
necessary. Do not substitute Windows.UI.Composition's ICompositorInterop for
WinUI's Microsoft.UI.Composition interface: their methods differ.

The project already targets Windows 11. Keep capability checks and a fallback
for unsupported drivers/configurations.

## Boundaries for a future prototype

- **Separate renderer and presenter.** The Windows
  [host](../../apps/layer-windows/native/src/host.rs) currently selects D3D12 and
  a wgpu SwapChainPanel surface.
  [ViewportPresenter::encode](../../crates/layer-render-wgpu/src/present.rs)
  accepts a caller-owned target and encoder, providing the output seam.
- **Reuse wgpu interoperability.** Vendored
  [Vulkan device code](../../vendor/wgpu-hal/src/vulkan/device.rs) exposes
  texture_from_d3d11_shared_handle, and the
  [queue](../../vendor/wgpu-hal/src/vulkan/mod.rs) exposes external semaphore
  hooks. At the inspected source, normal device construction enables external
  memory but not VK_KHR_external_semaphore_win32. A controlled raw-device
  bootstrap through HAL APIs or a small extension-selection change is needed.
  No brush shader rewrite is implied.
- **Implement both synchronization directions.** Acquire/release external image
  ownership and the required layouts while keeping wgpu tracking consistent.
  Signal on the final output submission, then order Windows presentation after
  that signal with a GPU wait. Returning buffers to Vulkan must respect actual
  presentation lifetime; do not rely on Direct3D's implicit protection against
  writes to displayed buffers. Validate cleanup under cancellation/device loss.
  [Khronos synchronization](https://docs.vulkan.org/spec/latest/chapters/synchronization.html)
  distinguishes external queue ownership from semaphore ordering.
- **Preserve scheduling and input.** Maintain independent input/render ownership,
  bounded presentation depth and backpressure before draining fresh input.
  Avoid CPU readback and per-frame CPU completion waits. Android's retained
  front-buffer behavior is not automatically transferable to Windows.
- **Match hardware and memory policy.** Match adapter LUIDs and native driver
  identity, avoiding Dozen/software selection. Extend
  [Windows memory admission](../../crates/layer-render-wgpu/src/display_memory.rs)
  to the supported VK_EXT_memory_budget path, accounting for shared allocations
  and retaining bounded fallback. Do not hard-code the benchmark allowance or
  duplicate the document across devices.
- **Preserve display and lifecycle behavior.** Retain
  [display/color handling](../../apps/layer-windows/native/src/display.rs),
  FP16 scRGB, DPI/resize, overlays, Navigator, multiwindow ownership and recovery.
  Resource sharing must preserve existing native input and reconstruction rules.

## Latency questions and adoption experiment

The current host already uses flip-discard buffers, maximum frame latency one,
a frame-latency waitable object, immediate presentation with tearing when
supported, and FIFO fallback. Its ordinary frame loop does not use the offscreen
benchmark's completion wait after every generation.

Sharing need not add a frame or application pixel copy, but synchronization and
scheduling have costs. Displayable texture layout may affect the viewport pass.
Independent Flip/hardware overlays depend on window geometry, scaling, format and
overlapping content. Capability support does not prove our canvas qualifies.
Microsoft's [flip-model guidance](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/for-best-performance--use-dxgi-flip-model)
describes these conditions, tearing-related latency advantages, and observing
actual presentation mode.

It remains unestablished whether IPresentationManager matches the lowest latency
of our current immediate DXGI path with tearing. Removing a copy is insufficient
evidence. The test display runs at approximately 60 Hz: missing a presentation
opportunity costs about 16.7 ms. Earlier native measurements are software
presentation-observation bounds with incomplete present matching, not precise
GPU-completion-to-display or physical photon times.

When work resumes, compare:

| Configuration | Additional application copy | Role |
| --- | --- | --- |
| Current D3D12 + DXGI/WinUI | None | Existing latency baseline |
| Vulkan + shared texture + native DXGI presenter | One viewport copy | Retain established DXGI presentation controls |
| Vulkan + shared displayable buffers + IPresentationManager | None | Candidate for combined benefits |

First prove pixel correctness and synchronization in the real panel. Then match
document, samples, zoom, prediction, cache admission and queue limits across
configurations. Count completed generations separately from displayed updates.
Record input-to-submit, GPU completion, presentation handoff, displayed-frame
timing and composition mode. Use calibrated GPU timestamps and ETW/PresentMon or
presentation statistics without per-frame completion waits. Report p50/p95/p99,
dropped/unmatched presents and sustained memory use. Physical input-to-photon
claims require camera or photodiode measurement, including scanout position and
panel response.

Validate normal/overlapping UI, menus, cursor/selection, SDR/HDR, fractional DPI,
resize, minimize/restore, multiwindow, device loss and pen/touch/mouse input.
Repeat on additional GPU vendors before changing the default. Retain the current
presenter until correctness and a latency benefit are demonstrated.

**Decision: preserve this candidate and verified low-level evidence; defer the
prototype and backend switch.**
