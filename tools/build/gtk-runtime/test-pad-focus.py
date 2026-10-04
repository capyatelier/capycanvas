#!/usr/bin/env python3
"""Run GTK's pad callbacks against independent focus and lifetime assertions."""
from pathlib import Path
import re
import subprocess
import sys
import tempfile

root = Path(sys.argv[1])
seat_source = (root / "gdk/wayland/gdkseat-wayland.c").read_text()
surface_source = (root / "gdk/wayland/gdksurface-wayland.c").read_text()


def function(source, name):
    match = re.search(r"^(?:static )?\w+(?: \*)?\n" + name + r" \(", source, re.M)
    return source[match.start():source.index("\n}", match.end()) + 2] if match else ""


header = r'''
#include <glib-object.h>
#include <stdint.h>
#include <assert.h>
#include <stdio.h>
#define GDK_SEAT_DEBUG(...) ((void)0)
#define GDK_WAYLAND_SEAT(s) (s)
#define GDK_SURFACE_DESTROYED(s) (g_object_get_data(G_OBJECT(s), "destroyed") != NULL)
#define GDK_TYPE_WAYLAND_DEVICE_PAD G_TYPE_OBJECT
#define GDK_SOURCE_TABLET_PAD 1
#define ZWP_TABLET_PAD_V2_BUTTON_STATE_PRESSED 1
enum { GDK_PAD_BUTTON_PRESS, GDK_PAD_BUTTON_RELEASE };
typedef GObject GdkSurface;
typedef GObject GdkDevice;
typedef int32_t wl_fixed_t;
typedef struct _Pad GdkWaylandTabletPadData;
typedef struct { GList *tablet_pads; GdkSurface *keyboard_focus; GdkDevice *logical_keyboard; } GdkWaylandSeat;
typedef GdkWaylandSeat GdkSeat;
typedef struct { const char *name; uint32_t vid, pid; GList *pads; } GdkWaylandTabletData;
struct wl_proxy { const void *listener; void *data; };
struct wl_surface { const void *listener; GdkSurface *data; };
struct zwp_tablet_v2 { const void *listener; GdkWaylandTabletData *data; };
struct zwp_tablet_pad_v2 {};
struct zwp_tablet_pad_group_v2 {};
struct zwp_tablet_pad_ring_v2 {};
struct zwp_tablet_pad_strip_v2 {};
struct zwp_tablet_pad_dial_v2 {};
static int tablet_listener, surface_listener, foreign_listener;
typedef struct {
  GdkWaylandTabletPadData *pad;
  uint32_t mode_switch_serial, current_mode;
  GList *buttons;
  struct { uint32_t source; gboolean is_stop; double value; } axis_tmp_info;
} GdkWaylandTabletPadGroupData;
struct _Pad {
  GdkSeat *seat;
  GdkDevice *device;
  GdkWaylandTabletData *current_tablet;
  GdkSurface *focus;
  GList *mode_groups, *rings, *strips, *dials;
};
typedef struct { GdkSurface *surface; GdkDevice *device; uint32_t mode; } GdkEvent;
static GdkSurface *expected_surface;
static uint32_t expected_mode;
static int delivered, added, removed;
static const void *wl_proxy_get_listener(struct wl_proxy *proxy) { return proxy->listener; }
static void *wl_surface_get_user_data(struct wl_surface *surface) { return surface->data; }
static void *zwp_tablet_v2_get_user_data(struct zwp_tablet_v2 *tablet) { return tablet->data; }
static void *gdk_seat_get_display(GdkSeat *seat) { return seat; }
static void _gdk_device_set_associated_device(GdkDevice *device, GdkDevice *other) {}
static void gdk_seat_device_added(GdkSeat *seat, GdkDevice *device) { added++; }
static void gdk_seat_device_removed(GdkSeat *seat, GdkDevice *device) { removed++; }
#define GDK_SEAT(s) (s)
static GdkDevice *new_object(void) { return g_object_new(G_TYPE_OBJECT, NULL); }
#define g_object_new(...) new_object()
static GdkEvent *make_event(GdkSurface *surface, GdkDevice *device, uint32_t mode) {
  assert(surface == expected_surface);
  assert(device != NULL);
  assert(mode == expected_mode);
  GdkEvent *event = g_new0(GdkEvent, 1);
  event->surface = g_object_ref(surface);
  event->device = g_object_ref(device);
  event->mode = mode;
  return event;
}
static GdkEvent *gdk_pad_event_new_group_mode(GdkSurface *s, GdkDevice *d, uint32_t t, uint32_t g, uint32_t m) { return make_event(s,d,m); }
static GdkEvent *gdk_pad_event_new_ring(GdkSurface *s, GdkDevice *d, uint32_t t, uint32_t g, uint32_t i, uint32_t m, double v) { return make_event(s,d,m); }
#define gdk_pad_event_new_strip gdk_pad_event_new_ring
#define gdk_pad_event_new_dial gdk_pad_event_new_ring
static GdkEvent *gdk_pad_event_new_button(int type, GdkSurface *s, GdkDevice *d, uint32_t t, uint32_t g, uint32_t b, uint32_t m) { return make_event(s,d,m); }
static void _gdk_wayland_display_deliver_event(void *display, GdkEvent *event) {
  delivered++;
  g_object_unref(event->surface);
  g_object_unref(event->device);
  g_free(event);
}
'''

names = ["tablet_pad_has_focus", "tablet_pad_lookup_button_group", "tablet_pad_handle_enter",
         "tablet_pad_handle_leave", "tablet_pad_group_handle_mode", "tablet_pad_handle_button",
         "tablet_pad_ring_handle_frame", "tablet_pad_strip_handle_frame", "tablet_pad_dial_handle_frame",
         "gdk_wayland_seat_clear_pad_focus", "_gdk_wayland_seat_remove_tablet_pad"]
callbacks = [function(surface_source, "gdk_wayland_surface_from_wl_surface")]
callbacks += [function(seat_source, name) for name in names]
if not callbacks[-2]:
    callbacks[-2] = "static void gdk_wayland_seat_clear_pad_focus(GdkWaylandSeat *s, GdkSurface *w) {}"

main = r'''
static void controls(GdkWaylandTabletPadGroupData *group, uint32_t mode) {
  expected_mode = mode;
  tablet_pad_group_handle_mode(group, NULL, 1, 900 + mode, mode);
  tablet_pad_handle_button(group->pad, NULL, 2, 0, 1);
  tablet_pad_handle_button(group->pad, NULL, 3, 0, 0);
  tablet_pad_ring_handle_frame(group, NULL, 4);
  tablet_pad_strip_handle_frame(group, NULL, 5);
  tablet_pad_dial_handle_frame(group, NULL, 6);
}
int main(void) {
  GdkSurface *a = new_object(), *b = new_object();
  GdkWaylandSeat seat = {0};
  GdkWaylandTabletData tablet = {.name="First"}, second = {.name="Second", .vid=1};
  struct zwp_tablet_v2 protocol_tablet = {&tablet_listener, &tablet};
  struct zwp_tablet_v2 protocol_second = {&tablet_listener, &second};
  struct zwp_tablet_v2 foreign_tablet = {&foreign_listener, (void *) 1};
  struct wl_surface wa = {&surface_listener, a}, wb = {&surface_listener, b};
  struct wl_surface foreign = {&foreign_listener, (void *) 1};
  GdkWaylandTabletPadData *pad = g_new0(GdkWaylandTabletPadData, 1);
  GdkWaylandTabletPadGroupData group = {.pad=pad};
  pad->seat = &seat;
  pad->mode_groups = g_list_append(NULL, &group);
  group.buttons = g_list_append(NULL, GUINT_TO_POINTER(0));
  seat.tablet_pads = g_list_append(NULL, pad);
  expected_surface = a;
  tablet_pad_handle_enter(pad, NULL, 0, &protocol_tablet, NULL);
  controls(&group, 1);
  assert(delivered == 0 && group.current_mode == 1 && group.mode_switch_serial == 901);
  tablet_pad_handle_leave(pad, NULL, 0, NULL);
  int devices_before_focus = added;
  tablet_pad_handle_enter(pad, NULL, 1, &protocol_tablet, &wa);
  controls(&group, 0);
  assert(delivered == 6 && added == devices_before_focus + 1 && removed == devices_before_focus && a->ref_count == 2);
  seat.keyboard_focus = b;
  controls(&group, 1);
  assert(delivered == 12);
  seat.keyboard_focus = NULL;
  controls(&group, 0);
  assert(delivered == 18);
  tablet_pad_handle_leave(pad, NULL, 2, &wa);
  controls(&group, 1);
  assert(delivered == 18 && group.current_mode == 1 && group.mode_switch_serial == 901);
  assert(a->ref_count == 1 && tablet.pads == NULL);
  expected_surface = b;
  tablet_pad_handle_enter(pad, NULL, 3, &protocol_second, &wb);
  controls(&group, 1);
  assert(delivered == 24 && added == devices_before_focus + 2 && removed == devices_before_focus + 1 && b->ref_count == 2);
  g_object_set_data(G_OBJECT(b), "destroyed", GINT_TO_POINTER(1));
  controls(&group, 0);
  assert(delivered == 24);
  gdk_wayland_seat_clear_pad_focus(&seat, b);
  assert(b->ref_count == 1 && pad->focus == NULL);
  tablet_pad_handle_enter(pad, NULL, 4, &protocol_tablet, &foreign);
  controls(&group, 1);
  tablet_pad_handle_enter(pad, NULL, 5, &protocol_tablet, NULL);
  controls(&group, 0);
  tablet_pad_handle_enter(pad, NULL, 5, &foreign_tablet, &wa);
  controls(&group, 0);
  assert(delivered == 24);
  assert(second.pads == NULL && g_list_length(tablet.pads) == 1);
  expected_surface = a;
  tablet_pad_handle_enter(pad, NULL, 6, &protocol_tablet, &wa);
  assert(g_list_length(tablet.pads) == 1);
  controls(&group, 0);
  assert(delivered == 30);
  GdkDevice *device = pad->device;
  pad->device = NULL;
  controls(&group, 0);
  assert(delivered == 30);
  pad->device = device;
  g_list_free(pad->mode_groups);
  g_list_free(group.buttons);
  g_list_free(tablet.pads);
  _gdk_wayland_seat_remove_tablet_pad(&seat, pad);
  assert(seat.tablet_pads == NULL && a->ref_count == 1 && b->ref_count == 1);
  g_object_unref(a);
  g_object_unref(b);
  puts("PASS: all pad producers, destroyed surface before entry, independent focus, absent device, modes, foreign surfaces, reassociation and reference lifetime");
  return 0;
}
'''

with tempfile.TemporaryDirectory(prefix="gtk-pad-focus-") as directory:
    source = Path(directory) / "pad.c"
    binary = Path(directory) / "pad"
    source.write_text(header + "\n".join(callbacks) + main)
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "gobject-2.0"], text=True).split()
    subprocess.run(["cc", "-Werror=implicit-function-declaration", str(source), "-o", str(binary), *flags], check=True)
    subprocess.run([str(binary)], check=True)
