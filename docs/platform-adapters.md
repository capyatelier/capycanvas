# Platform adapters

The adapters share `layer-ui` state/actions, canvas semantics, and conformance
tests, not widgets, windowing, or graphics implementation.
Linux and web hosts are implemented. Other rows describe the intended native
mapping, not completed clients. Physical-device input validation is listed in
the [UI human-test checklist](ui-implementation.md#run).

| Platform | UI/input path | Canvas backend | Notes |
| --- | --- | --- | --- |
| Linux Wayland | GTK4 GestureStylus/GDK tablet events | `layer-render-wgpu` → Vulkan mailbox swapchain → app-owned subsurface | One GPU worker/device/queue; full-window viewport and cursor below transparent native UI; shader-rounded corners; independent 120 Hz pacing |
| Windows | WinUI + Win32 `WM_POINTER` history | `layer-render-wgpu` → D3D12 | Reverse history into chronological order; shared predictor supplies future samples |
| macOS | SwiftUI/AppKit tablet events | `layer-render-wgpu` → Metal | Pressure/tilt/rotation are native; shared prediction uses the display target |
| iPadOS | SwiftUI/UIKit + coalesced/predicted Pencil touches | `layer-render-wgpu` → Metal | Submit precise coalesced touches, then UIKit predictions, through the batched path |
| Android | Compose + `MotionEvent` history through JNI | `layer-render-wgpu` → Vulkan | Record every event in AndroidX MotionEventPredictor and submit its frame-time result |
| Web | DOM/Wasm toolkit + Pointer Events | `layer-render-wgpu` → WebGPU | Use raw/coalesced events plus browser predictions when available |

The platform callback must:

1. retrieve all platform-provided historical/coalesced samples immediately;
2. convert timestamps to monotonic nanoseconds;
3. convert coordinates to physical surface pixels;
4. normalize pressure and angular axes without smoothing them;
5. tag predicted samples and the current view revision;
6. append any platform-predicted records with `LAYER_SAMPLE_PREDICTED`;
7. push into engine input ingress and request a display callback. Non-web
   adapters may place the SPSC producer and consumer on separate owners; a
   single-threaded Wasm adapter uses the same ingress on one event loop.

The platform UI adapter must also:

1. render changed `layer-ui` regions with toolkit widgets;
2. convert widget events to typed `UiAction`s rather than mutating engine state;
3. keep focus, hover, animation, window geometry, and native container allocation
   in the frontend; Rust supplies ordered dock bands, splits, and tabs;
4. present Rust-owned modal state using the appropriate toolkit sheet, dialog,
   popover, or navigation container;
5. complete durable platform requests for file pickers, sharing, and
   permissions;
6. keep generated bindings and general UI callbacks out of pen and frame
   submission paths.

For WebAssembly, these are logical adapter responsibilities rather than thread
requirements. `layer-ui` runs synchronously on the browser event loop; a Web
Worker is optional. The adapter must not require shared memory, blocking locks,
a filesystem, or OS handles.

Smoothing and brush semantics remain shared so the same brush feels comparable
across devices. Device-specific pressure calibration is represented by a
`PressureCurve`, not hidden inside a render backend.

The display callback calls `layer_canvas_draw_frame_for` with separate current
and expected-presentation timestamps when the platform exposes both. Prediction
controls are common, runtime-tunable interaction settings so device labs can
compare native, shared, reduced-horizon, tip-lock-only, and fully disabled
behavior without changing brush files.

The platform renderer must:

1. select a hardware GPU compatible with the platform surface and fail painting
   cleanly when none is available;
2. consume one `FramePacket` per display callback without waiting for the GPU;
3. keep real-input paint in persistent renderer-owned GPU textures;
4. render predicted batches into replaceable GPU-only preview state;
5. recover surface loss and report unrecoverable device errors;
6. keep readback outside the pen-to-present path.

Adapters never select a software rasterizer or maintain a host-memory canvas.
The Linux host currently requires Wayland, premultiplied-alpha surface support
and Vulkan mailbox presentation; it does not implement an X11 canvas path.
GTK retains ownership of input, its connection and parent surface. The worker
owns only its child and its own Wayland event queue. Integer buffer scaling is
supported; fractional desktop scales currently use GTK's integer buffer scale
and compositor resampling, rather than a fractional-resolution swapchain.

Primary API references:

- [Wayland tablet-v2 protocol](https://wayland.app/protocols/wayland-protocols/480)
- [Windows `GetPointerPenInfoHistory`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getpointerpeninfohistory)
- [Windows `POINTER_PEN_INFO`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-pointer_pen_info)
- [Apple high-fidelity coalesced touches](https://developer.apple.com/documentation/uikit/getting-high-fidelity-input-with-coalesced-touches)
- [Apple predicted touches](https://developer.apple.com/documentation/uikit/uievent/predictedtouches%28for%3A%29)
- [Android `MotionEvent`](https://developer.android.com/reference/android/view/MotionEvent.html)
- [AndroidX `MotionEventPredictor`](https://developer.android.com/reference/androidx/input/motionprediction/MotionEventPredictor)
- [W3C Pointer Events Level 3](https://www.w3.org/TR/pointerevents3/)
- [wgpu WebAssembly backends](https://docs.rs/wgpu/latest/wasm32-unknown-unknown/wgpu/struct.Backends.html)
- [Shared UI and platform adapter design](shared-ui.md)
