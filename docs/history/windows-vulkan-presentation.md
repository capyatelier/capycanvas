# Vulkan rendering with native Windows presentation

[Design history](README.md)

A deferred design candidate, kept for future work. Windows renders with D3D12 and
presents through DXGI; nothing here is implemented.

## Motivation

On the Intel Iris Xe development laptop, the shared renderer drew the 1000 px
G-Pen offscreen workload on the 61 MP canvas markedly faster through Vulkan than
through D3D12, with identical Undo/Redo and at most 1/255 difference per channel
between backends. Most of the gap was CPU overhead: submission and composition
encoding cost several times more on D3D12, while GPU spans were close. These
offscreen rates say nothing about displayed frames or input latency.

## Candidate

Render with Vulkan into Windows-owned shared FP16 textures and present those
allocations through the Windows 11 Composition Swapchain API
(`IPresentationManager`), attached to the existing WinUI `SwapChainPanel`. This
avoids an application copy between Vulkan output and Windows presentation. It
does not guarantee that DWM avoids composition, that the window qualifies for
direct scanout, or that it beats the current immediate DXGI presentation.
Adoption depends on latency as well as throughput.

## Interoperability probes

Temporary probes called the installed Vulkan and Windows APIs on the development
laptop. They did not render or present images.

| Probe | Result |
| --- | --- |
| Native Vulkan external Win32 memory and semaphore extensions, timeline semaphores | Supported |
| Optimal-tiled RGBA16F import from `D3D11_TEXTURE` | Importable; dedicated allocation required |
| D3D11-owned 1600 × 1000 RGBA16F texture imported and bound to a Vulkan image, with and without `SHARED_DISPLAYABLE` | Passed |
| Same allocation registered with `IPresentationManager::AddBufferFromResource` while imported into Vulkan | Passed |
| Vulkan submission signaling an imported Windows shared fence | Passed; D3D11 observed the value |
| D3D11 displayable surfaces, presentation factory and manager | Supported; shared resource tier 3 |
| `IsPresentationSupportedWithIndependentFlip` | True; the application's actual mode is untested |
| `D3D12_RESOURCE` external image-format queries | `VK_ERROR_FORMAT_NOT_SUPPORTED` for the tested formats |

The driver also enumerates Microsoft's Dozen (Vulkan on D3D12); adapter selection
must pick the native driver.

Probe parameters: optimal 2D images with sampled, color-attachment and transfer
usage in RGBA16F, RGBA8, BGRA8 and RGBA32F. Imports used D3D11
`R16G16B16A16_FLOAT` textures with one mip, slice and sample, render-target and
shader-resource bindings, `SHARED | SHARED_NTHANDLE` and optionally
`SHARED_DISPLAYABLE`, exported as an NT handle into a dedicated Vulkan
allocation. The presentation device used BGRA support, single-threaded access and
disabled internal threading optimizations, as in Microsoft's composition-swapchain
example. The fence probe imported a shared D3D11 fence as a Vulkan timeline
semaphore with the `D3D12_FENCE` handle type and signaled it from an empty
submission. That proves one signaling direction only, not image visibility,
reuse or display retirement. See the
[Khronos semaphore handle types](https://docs.vulkan.org/refpages/latest/refpages/source/VkExternalSemaphoreHandleTypeFlagBits.html).

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
size and device generation. Document tiles, display pyramids, filters and brush
work stay private to Vulkan; only the final viewport crosses the API boundary.
Keep final color conversion, cursor and selection behavior. The buffer pool must
not become permission to queue stale frames.

`IPresentationManager` registers application-created textures and presents them
through a composition surface handle, so one allocation can be both Vulkan output
and a presentation buffer. See Microsoft's
[programming guide](https://learn.microsoft.com/en-us/windows/win32/comp_swapchain/comp-swapchain)
and [buffer-creation examples](https://learn.microsoft.com/en-us/windows/win32/comp_swapchain/comp-swapchain-examples).
WinUI's [`ISwapChainPanelNative2::SetSwapChainHandle`](https://learn.microsoft.com/en-us/windows/windows-app-sdk/api/win32/microsoft.ui.xaml.media.dxinterop/nf-microsoft-ui-xaml-media-dxinterop-iswapchainpanelnative2-setswapchainhandle)
routes the handle to `CreateCompositionSurfaceForHandle`
([SwapChainElement](https://github.com/microsoft/microsoft-ui-xaml/blob/main/dxaml/xcp/core/core/elements/SwapChainElement.cpp)),
which keeps the panel and its independent input source. Attachment happens on the
UI thread; ordinary presents should need neither reattachment nor a XAML render
tick. Do not substitute `Windows.UI.Composition`'s `ICompositorInterop` for WinUI's
`Microsoft.UI.Composition` interface: their methods differ. Keep capability checks
and a fallback for unsupported drivers.

## Boundaries for a prototype

- **Separate renderer and presenter.** The Windows
  [host](../../apps/layer-windows/native/src/host.rs) selects D3D12 and a wgpu
  `SwapChainPanel` surface.
  [`ViewportPresenter::encode`](../../crates/layer-render-wgpu/src/present.rs)
  accepts a caller-owned target and encoder, which is the output seam.
- **Reuse wgpu interoperability.** Vendored
  [Vulkan device code](../../vendor/wgpu-hal/src/vulkan/device.rs) exposes
  `texture_from_d3d11_shared_handle`, and the
  [queue](../../vendor/wgpu-hal/src/vulkan/mod.rs) exposes external semaphore
  hooks. Normal device construction enables external memory but not
  `VK_KHR_external_semaphore_win32`, so a raw-device bootstrap through HAL APIs
  or a small extension-selection change is needed. No brush shader rewrite is
  implied.
- **Synchronize both directions.** Acquire and release external image ownership
  and layouts while keeping wgpu's tracking consistent. Signal on the final output
  submission and order Windows presentation after it with a GPU wait. Return
  buffers to Vulkan only after their actual presentation lifetime; do not rely on
  Direct3D's implicit protection of displayed buffers. Validate cleanup under
  cancellation and device loss. [Khronos synchronization](https://docs.vulkan.org/spec/latest/chapters/synchronization.html)
  distinguishes queue ownership from semaphore ordering.
- **Preserve scheduling and input.** Keep independent input and render ownership,
  bounded presentation depth and backpressure before draining fresh input. No CPU
  readback or per-frame CPU completion waits. Android's retained front buffer does
  not transfer automatically to Windows.
- **Match hardware and memory policy.** Match adapter LUIDs and the native driver,
  avoiding Dozen and software adapters. Extend
  [display memory admission](../../crates/layer-render-wgpu/src/display_memory.rs)
  to `VK_EXT_memory_budget`, accounting for shared allocations and keeping the
  bounded fallback. Do not duplicate the document across devices.
- **Preserve display and lifecycle behavior.** Keep
  [display and color handling](../../apps/layer-windows/native/src/display.rs),
  FP16 scRGB, DPI and resize, overlays, Navigator, multiple windows and device
  recovery.

## Adoption experiment

The current host uses flip-discard buffers, maximum frame latency one, the
frame-latency waitable object, immediate presentation with tearing when supported
and FIFO otherwise. Sharing need not add a frame or a copy, but synchronization
and scheduling have costs, displayable layouts may slow the viewport pass, and
Independent Flip depends on window geometry, scaling, format and overlapping
content. Microsoft's [flip-model guidance](https://learn.microsoft.com/en-us/windows/win32/direct3ddxgi/for-best-performance--use-dxgi-flip-model)
describes these conditions and how to observe the actual presentation mode.
Whether `IPresentationManager` matches the latency of immediate DXGI presentation
is unknown; removing a copy is not evidence.

Compare three configurations:

| Configuration | Extra application copy | Role |
| --- | --- | --- |
| D3D12 + DXGI/WinUI | None | Current latency baseline |
| Vulkan + shared texture + native DXGI presenter | One viewport copy | Keeps the established DXGI presentation controls |
| Vulkan + shared displayable buffers + `IPresentationManager` | None | Candidate combining both benefits |

First prove pixel correctness and synchronization in the real panel. Then match
document, samples, zoom, prediction, cache admission and queue limits across
configurations, and count completed generations separately from displayed updates.
Record input-to-submit, GPU completion, presentation handoff, displayed-frame
timing and composition mode, using calibrated GPU timestamps and ETW/PresentMon or
presentation statistics without per-frame completion waits. Physical
input-to-photon claims need a camera or photodiode. Validate overlapping UI,
menus, cursor and selection, SDR and HDR, fractional DPI, resize,
minimize/restore, multiple windows, device loss and all input devices, on more
than one GPU vendor, before changing the default. Keep the current presenter
until correctness and a latency benefit are shown.
