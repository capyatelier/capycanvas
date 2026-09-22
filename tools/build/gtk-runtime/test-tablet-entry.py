#!/usr/bin/env python3
"""Exercise the actual patched GTK Wayland callbacks with recorded entry sequences.

Only protocol/GDK delivery is stubbed; the event queue and proximity/motion
callbacks are extracted unchanged from the supplied GTK source directory.
Requires a C compiler, pkg-config, and GLib development headers.
"""
from pathlib import Path
import subprocess, tempfile, sys
root=Path(sys.argv[1])
src=(root/'gdk/wayland/gdkseat-wayland.c').read_text()
def function(name, src=src):
    a=src.index('\n'+name+' (')+1
    a=src.rfind('static ',0,a)
    b=src.index('\n}',a)+2
    return src[a:b]
names=['gdk_wayland_tablet_add_frame_event','gdk_wayland_tablet_flush_frame_events','tablet_tool_handle_proximity_in','tablet_tool_handle_proximity_out','tablet_tool_handle_motion']
header=r'''
#include <glib.h>
#include <stdint.h>
#include <assert.h>
#include <stdio.h>
#define GDK_SEAT_DEBUG(...) ((void)0)
#define GDK_IS_SURFACE(x) ((x)!=NULL)
#define g_object_ref(x) (x)
#define g_object_unref(x) ((void)0)
#define g_clear_object(p) (*(p)=NULL)
#define g_set_object(p,x) (*(p)=(x))
#define GDK_WAYLAND_SEAT(x) (x)
#define GDK_WAYLAND_DEVICE(x) (x)
#define GDK_CROSSING_NORMAL 0
#define GDK_BUTTON_PRIMARY 1
#define GDK_BUTTON1_MASK (1<<8)
#define GDK_BUTTON2_MASK (1<<9)
#define GDK_BUTTON3_MASK (1<<10)
#define GDK_BUTTON4_MASK (1<<11)
#define GDK_BUTTON5_MASK (1<<12)
typedef enum {GDK_PROXIMITY_IN,GDK_PROXIMITY_OUT,GDK_MOTION_NOTIFY,GDK_BUTTON_PRESS,GDK_BUTTON_RELEASE,GDK_ENTER_NOTIFY,GDK_LEAVE_NOTIFY} GdkEventType;
typedef struct { GdkEventType event_type; uint32_t button; } TabletEvent;
typedef struct {} GdkSurface;
typedef struct { GdkEventType type; } GdkEvent;
typedef struct { void *focus,*cursor; unsigned enter_serial,cursor_shape,button_modifiers; int has_cursor_surface,cursor_is_default; double surface_x,surface_y; } GdkWaylandPointerData;
typedef struct _Tablet GdkWaylandTabletData;
typedef struct {GdkWaylandTabletData *current_tablet; void *tool,*seat;} GdkWaylandTabletToolData;
struct _Tablet {void *logical_device,*stylus_device,*seat; GdkWaylandPointerData pointer_info; GList *events; gboolean proximity_enter_pending, cursor_update_inhibited; GdkWaylandTabletToolData *current_tool;};
typedef GdkWaylandTabletData GdkDevice;
typedef GdkWaylandTabletData GdkWaylandDevice;
typedef struct {int arrow;} GdkCursor;
typedef struct {GdkDevice *logical_touch;GdkCursor *grab_cursor;} GdkWaylandSeat;
static GdkWaylandSeat seat;
static GdkCursor arrow={1}, hidden={0};
static int cursor_updates, arrow_updates;
static GdkWaylandSeat *gdk_device_get_seat(GdkDevice *d) {return &seat;}
static GdkWaylandPointerData *gdk_wayland_device_get_pointer(GdkDevice *d) {return &d->pointer_info;}
static GdkWaylandTabletData *gdk_wayland_seat_find_tablet(GdkWaylandSeat *s,GdkDevice *d) {return d;}
static gboolean gdk_cursor_equal(GdkCursor *a,GdkCursor *b) {return a==b;}
static GdkCursor *gdk_cursor_new_from_name(const char *name,void *fallback) {return &arrow;}
static void gdk_wayland_device_update_surface_cursor(GdkDevice *d) {
  cursor_updates++;
  arrow_updates+=((GdkCursor *)d->pointer_info.cursor)->arrow;
}
static void gdk_wayland_device_set_surface_cursor(GdkDevice *device,GdkSurface *surface,GdkCursor *cursor);
struct zwp_tablet_tool_v2 {};
struct zwp_tablet_v2 {GdkWaylandTabletData *data;};
struct wl_surface {GdkSurface *data;};
typedef int32_t wl_fixed_t;
static double wl_fixed_to_double(wl_fixed_t x) {return x/256.;}
static void *zwp_tablet_v2_get_user_data(struct zwp_tablet_v2 *v) {return v->data;}
static void *wl_surface_get_user_data(struct wl_surface *v) {return v->data;}
static void gdk_device_update_tool(void *d,void *t) {}
static void gdk_wayland_device_tablet_clone_tool_axes(void *d,void *t) {}
static void gdk_wayland_mimic_device_axes(void *d,void *s) {}
static unsigned gdk_wayland_device_get_modifiers(void *d) {return 0;}
static void *gdk_seat_get_display(void *s) {return s;}
static double *tablet_copy_axes(void *t) {return NULL;}
static GdkEvent *make_event(GdkEventType type) {GdkEvent *e=g_new0(GdkEvent,1);e->type=type;return e;}
static GdkEvent *gdk_proximity_event_new(GdkEventType t,void *s,void *d,void *tool,unsigned time) {return make_event(t);}
static GdkEvent *gdk_button_event_new(GdkEventType t,void *s,void *d,void *tool,unsigned time,unsigned state,unsigned b,double x,double y,double *axes) {return make_event(t);}
static GdkEvent *gdk_motion_event_new(void *s,void *d,void *tool,unsigned time,unsigned state,double x,double y,double *axes) {return make_event(GDK_MOTION_NOTIFY);}
static GdkEventType delivered[64];
static int count, enters, leaves;
static double entry_x,entry_y;
static void _gdk_wayland_display_deliver_event(void *d,GdkEvent *e) {delivered[count++]=e->type;g_free(e);}
static void emulate_crossing(void *s,void *child,void *d,GdkEventType t,int mode,unsigned time) {
  GdkWaylandTabletData *tablet=d;
  delivered[count++]=t;
  if(t==GDK_ENTER_NOTIFY) {
    enters++;entry_x=tablet->pointer_info.surface_x;entry_y=tablet->pointer_info.surface_y;
    // Surface entry tries its stale/default cursor before GTK picks a widget.
    gdk_wayland_device_set_surface_cursor(tablet,s,&arrow);
  }
  else leaves++;
}
'''
main=r'''
int main(void) {
  GdkWaylandTabletData tablet={0};
  GdkWaylandTabletToolData tool={0};
  GdkSurface surface;
  struct wl_surface ws={&surface};
  struct zwp_tablet_v2 wt={&tablet};
  tablet.logical_device=&tablet;
  // Separate proximity-only frame; stale coordinates are over window chrome.
  tablet_tool_handle_proximity_in(&tool,NULL,1,&wt,&ws);
  gdk_wayland_tablet_flush_frame_events(&tablet,10);
  assert(enters==0 && count==1 && delivered[0]==GDK_PROXIMITY_IN);
  tablet_tool_handle_motion(&tool,NULL,800*256,500*256);
  gdk_wayland_tablet_flush_frame_events(&tablet,20);
  assert(enters==1 && entry_x==800 && entry_y==500);
  assert(cursor_updates==0 && arrow_updates==0);
  // GTK picks the canvas and installs its hidden native cursor.
  gdk_wayland_device_set_surface_cursor(&tablet,&surface,&hidden);
  assert(cursor_updates==1 && arrow_updates==0);
  assert(delivered[1]==GDK_ENTER_NOTIFY && delivered[2]==GDK_MOTION_NOTIFY);
  tablet_tool_handle_motion(&tool,NULL,850*256,550*256);
  gdk_wayland_tablet_flush_frame_events(&tablet,25);
  assert(enters==1);
  tablet_tool_handle_proximity_out(&tool,NULL);
  gdk_wayland_tablet_flush_frame_events(&tablet,30);
  assert(leaves==1 && tool.current_tablet==NULL);
  // Entry and position in one frame still deliver exactly one crossing first.
  count=enters=leaves=0;
  tablet_tool_handle_proximity_in(&tool,NULL,2,&wt,&ws);
  tablet_tool_handle_motion(&tool,NULL,100*256,200*256);
  gdk_wayland_tablet_flush_frame_events(&tablet,40);
  assert(count==3 && delivered[0]==GDK_PROXIMITY_IN && delivered[1]==GDK_ENTER_NOTIFY && delivered[2]==GDK_MOTION_NOTIFY);
  assert(enters==1 && entry_x==100 && entry_y==200);
  assert(cursor_updates==1 && arrow_updates==0);
  // Picking a normal control must still show its arrow immediately.
  gdk_wayland_device_set_surface_cursor(&tablet,&surface,NULL);
  assert(cursor_updates==2 && arrow_updates==1);
  tablet_tool_handle_proximity_out(&tool,NULL);
  gdk_wayland_tablet_flush_frame_events(&tablet,45);
  // A tool that leaves before motion never entered a widget.
  count=enters=leaves=0;
  tablet_tool_handle_proximity_in(&tool,NULL,3,&wt,&ws);
  gdk_wayland_tablet_flush_frame_events(&tablet,50);
  tablet_tool_handle_proximity_out(&tool,NULL);
  gdk_wayland_tablet_flush_frame_events(&tablet,55);
  assert(enters==0 && leaves==0 && count==2 && tool.current_tablet==NULL);
  puts("PASS: entry ordering, stale cursor suppression, canvas/control cursors, repeat motion, and leave before motion");
}
'''
with tempfile.TemporaryDirectory(prefix='capy-tablet-entry-') as d:
    p=Path(d)/'test.c';p.write_text(header+function('gdk_wayland_device_set_surface_cursor', (root/'gdk/wayland/gdkdevice-wayland.c').read_text())+'\n'.join(function(n) for n in names)+main)
    flags=subprocess.check_output(['pkg-config','--cflags','--libs','glib-2.0'],text=True).split()
    subprocess.run(['cc','-std=c11','-Wall','-Wno-unused-parameter',str(p),'-o',d+'/test',*flags],check=True)
    subprocess.run([d+'/test'],check=True)
