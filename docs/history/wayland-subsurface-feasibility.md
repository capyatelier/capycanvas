# GTK + Wayland canvas subsurface

> Design record: why the Linux canvas presents into its own Wayland subsurface.
> Start with the [current technical guides](../README.md).

GTK imports every new canvas texture as a Wayland buffer and waits for the
compositor's response, which put canvas-sized work on the GTK frame path. The
Linux host instead gives the canvas an application-owned Wayland child surface
beneath the native GTK controls, presented by wgpu/Vulkan from a render worker.
GTK is not modified. The research inspected GTK 4.22.4, libadwaita 1.9.3 and
wgpu 30.0.1.

## Why it works

| Property | How |
| --- | --- |
| A child without taking over GTK's window | GDK exposes its Wayland display and surface. Create a **new** `wl_surface` on that connection and give it the subsurface role with GTK's surface as parent, as GTK itself does ([GDK getters](https://docs.gtk.org/gdk4-wayland/method.WaylandSurface.get_wl_surface.html), [GTK implementation](https://github.com/GNOME/gtk/blob/4.22.4/gdk/wayland/gdksubsurface-wayland.c#L1016)). Never present into GTK's own surface. |
| wgpu presentation without winit or OpenGL | `SurfaceTargetUnsafe::RawHandle` accepts Wayland handles and the Vulkan backend passes them to `vkCreateWaylandSurfaceKHR`. Vulkan needs a valid surface and display, not an `xdg_toplevel` role, and a separate event queue for its objects on the shared connection ([Vulkan Wayland contract](https://docs.vulkan.org/spec/latest/chapters/VK_KHR_surface/wsi.html#_wayland_platform)). |
| Native controls over the canvas | The child sits below its parent, so the parent must contain real transparent pixels over the canvas. Override libadwaita's CSS window background, keep intervening containers transparent, and paint only controls ([libadwaita background](https://github.com/GNOME/libadwaita/blob/1.9.3/src/stylesheet/_common.scss#L25), [GTK opacity](https://github.com/GNOME/gtk/blob/4.22.4/gsk/gpu/gskgpurenderer.c#L432)). |
| GTK keeps input | The child has an empty input region; the GTK canvas widget stays input-targetable, as with GTK's own subsurfaces ([GTK input setup](https://github.com/GNOME/gtk/blob/4.22.4/gdk/wayland/gdksubsurface-wayland.c#L1032)). |
| No per-frame import | wgpu acquires and presents swapchain images; the shared presenter already renders into a target view, so no canvas `GdkTexture` exists ([GTK path bypassed](https://github.com/GNOME/gtk/blob/4.22.4/gdk/wayland/gdksubsurface-wayland.c#L133)). Driver and compositor import caching remain outside the app's control. |

## Constraints the implementation must keep

- **Transparency is explicit.** `GraphicsOffload` punches its hole with internal
  GSK handling that an external child does not get. Drawing a transparent
  rectangle over an opaque background does not erase it, and GTK falls back to an
  opaque swapchain on hardware without alpha support
  ([GSK clearing](https://github.com/GNOME/gtk/blob/4.22.4/gsk/gpu/gskgpunodeprocessor.c#L4290),
  [alpha selection](https://github.com/GNOME/gtk/blob/4.22.4/gdk/gdkvulkancontext.c#L449)).
- **Blocking presentation stays off the input thread.** wgpu's acquire can wait
  up to 1,000 ms for a fence and a free image. The render worker owns
  presentation and talks to GTK through bounded communication; GTK never waits
  on its locks.
- **Geometry and lifetime.** Use a surface-compatible adapter from the engine's
  own wgpu instance. Account for shadow offsets and fractional scaling.
  Desynchronized child commits allow independent frames, but mapping, position
  and stacking changes need a parent commit: request a GTK frame as well as
  `force_next_commit`, which only sets a flag. Stop presenting and destroy the
  swapchain and child before GTK destroys its surface
  ([GTK commit code](https://github.com/GNOME/gtk/blob/4.22.4/gdk/wayland/gdksurface-wayland.c#L432)).
- **Clipping.** A child does not inherit its parent's clip, so rounded window
  corners are drawn by the canvas shader rather than by GTK.
- **Own only this surface.** Let Vulkan manage buffers, synchronization and
  commits. Do not attach buffers manually, adopt GTK's private `GdkSubsurface` or
  intercept GTK's protocol traffic; GTK has no public native-child-widget API
  ([maintainer guidance](https://discourse.gnome.org/t/need-to-create-a-child-native-window/22934)).
- **Captures.** GTK widget snapshots omit the external canvas; use compositor
  capture or compose the canvas into the capture explicitly. Backdrop effects
  that sample canvas pixels need their own path.

The subsurface removes one class of GTK work by construction. It does not rule
out driver, compositor or input-thread stalls, which still need presentation
measurements.
