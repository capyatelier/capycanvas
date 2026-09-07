# GTK + Wayland canvas subsurface: feasibility proof

Paths under `artifacts/` refer to ignored local outputs, not files shipped in
this repository. See [publication notes](publication.md#publication-checks).

Implementation follow-up: this is the original research record. The app now uses
the full-window worker-owned variant with shader-rounded corners, not an inset
rectangle. See [current architecture](ui-implementation.md#milestones-and-acceptance)
and `artifacts/benchmarks/gtk-wayland.md`.

Research only, 2026-09-07. Inspected GTK 4.22.4 and libadwaita 1.9.3 source in temporary clones, and the project's installed wgpu 30.0.1 source. No application implementation or runtime experiment was performed for this assessment.

## Verdict

**Technically feasible, not an invented API combination.** An application-owned Wayland child surface can host a wgpu/Vulkan swapchain beneath native GTK controls. This bypasses GTK's canvas texture imports without modifying GTK.

**Not a drop-in GTK widget, and not proof that latency spikes disappear.** It requires explicit transparency, surface lifecycle/geometry integration, and a presentation scheduling change. Merely replacing the current GTK callback's handoff with `get_current_texture()` would introduce another possible blocking point.

## Constructive proof

| Required property | Evidence and implication |
| --- | --- |
| Create a child without taking over GTK's window | GDK publicly exposes its borrowed Wayland display and surface. Create a **new** `wl_surface` on that connection and assign it the subsurface role with GTK's surface as parent. GTK itself uses precisely this parent/child construction. Do not present into GTK's own surface. [GDK getters](https://docs.gtk.org/gdk4-wayland/method.WaylandSurface.get_wl_surface.html), [GTK implementation](https://github.com/GNOME/gtk/blob/4.22.4/gdk/wayland/gdksubsurface-wayland.c#L1016). |
| Present through wgpu, without winit or OpenGL | `SurfaceTargetUnsafe::RawHandle` accepts Wayland handles; wgpu's Vulkan backend passes them directly to `vkCreateWaylandSurfaceKHR`. Vulkan requires valid surface/display objects, not an `xdg_toplevel` role, and mandates a separate event queue for its objects on the shared connection. [wgpu implementation](https://github.com/gfx-rs/wgpu/blob/40f4a34ebaf56f9a046231f54125ad046239d3f3/wgpu-hal/src/vulkan/instance.rs#L438), [Vulkan Wayland contract](https://docs.vulkan.org/spec/latest/chapters/VK_KHR_surface/wsi.html#_wayland_platform). |
| Retain native overlapping controls | Wayland supports placing the child below its parent. The parent must contain real transparent pixels over the canvas. Libadwaita's default window fill is CSS, not mandatory compositor content: override that window's background, keep intervening containers transparent, and paint only the perimeter and controls. GTK derives opacity from its rendered scene. [Libadwaita background](https://github.com/GNOME/libadwaita/blob/1.9.3/src/stylesheet/_common.scss#L25), [GTK opacity derivation](https://github.com/GNOME/gtk/blob/4.22.4/gsk/gpu/gskgpurenderer.c#L432). |
| Preserve GTK input and rounded window edges | Give the child an empty input region; keep the GTK canvas widget input-targetable. GTK uses the same empty-region technique for its own subsurfaces. Keep the child rectangle inside the rounded window outline: a child does **not** inherit its parent's clip. [GTK input setup](https://github.com/GNOME/gtk/blob/4.22.4/gdk/wayland/gdksubsurface-wayland.c#L1032), [Wayland subsurface rules](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_subsurface). |
| Remove the recurring GTK import path | wgpu creates the swapchain and obtains its images during configuration; subsequent frames acquire/present those images. Our existing `ViewportPresenter::encode` already accepts a target texture view. No canvas `GdkTexture` is needed, so GTK's per-new-texture Wayland-buffer creation/response wait is absent. This does **not** guarantee how NVIDIA's WSI or the compositor caches imports internally. [wgpu swapchain code](https://github.com/gfx-rs/wgpu/blob/40f4a34ebaf56f9a046231f54125ad046239d3f3/wgpu-hal/src/vulkan/swapchain/native.rs#L182), [GTK path being bypassed](https://github.com/GNOME/gtk/blob/4.22.4/gdk/wayland/gdksubsurface-wayland.c#L133), [our presenter](../crates/layer-render-wgpu/src/present.rs). |

## Conditions that cannot be skipped

- **Transparency must be explicit.** `GraphicsOffload` currently punches a transparent hole using internal GSK handling. An external child will not get that automatically. Drawing a transparent rectangle over the existing opaque background does not erase it. The parent Vulkan swapchain must support transparency; GTK has an opaque fallback on unsupported hardware. [GSK clearing](https://github.com/GNOME/gtk/blob/4.22.4/gsk/gpu/gskgpunodeprocessor.c#L4290), [alpha selection](https://github.com/GNOME/gtk/blob/4.22.4/gdk/gdkvulkancontext.c#L449).
- **Keep blocking presentation off the input thread.** wgpu 30.0.1 uses a 1,000 ms acquisition timeout; its Vulkan path can wait for a submission fence and an available image. That is a potential wait, not a normal frame cost. A render/presentation owner thread needs bounded communication and must not leave GTK waiting on its locks. [Timeout](https://github.com/gfx-rs/wgpu/blob/40f4a34ebaf56f9a046231f54125ad046239d3f3/wgpu-core/src/present.rs#L33), [acquisition](https://github.com/gfx-rs/wgpu/blob/40f4a34ebaf56f9a046231f54125ad046239d3f3/wgpu-hal/src/vulkan/swapchain/native.rs#L418).
- **Manage geometry and lifetime.** Select a surface-compatible adapter from the same wgpu instance used by the engine. Account for native shadow offsets and fractional scaling. Desynchronized child commits permit independent steady-state updates, but mapping/position/stacking changes still need a parent commit. Request a GTK frame and use `force_next_commit`; that function alone only sets a flag. Stop presentation and destroy the swapchain/child before GTK destroys its native surface. [GTK commit code](https://github.com/GNOME/gtk/blob/4.22.4/gdk/wayland/gdksurface-wayland.c#L432).
- **Own only our surface.** Let Vulkan manage its buffer queue, synchronization and presentation commits. Do not also attach buffers manually, adopt GTK's private `GdkSubsurface`, or intercept GTK's protocol traffic. GTK does not expose an ordinary native-child-widget API. [GTK maintainer guidance](https://discourse.gnome.org/t/need-to-create-a-child-native-window/22934).

## What remains unproven

No API-level blocker was found for the inset-canvas design. Adoption still needs a hardware test of visible controls/cursor, modal dimming, fractional-scale resize, minimize/restore and close, followed by presentation p95/p99 measurements. GTK-only widget snapshots would omit the external canvas; compositor capture or explicit capture composition would be necessary. Backdrop effects sampling canvas pixels would also need separate treatment.

**Decision: valid candidate for a focused prototype; not yet a demonstrated 120 Hz fix.** It removes one known class of GTK work by construction, not all possible driver, compositor or input-thread stalls.
