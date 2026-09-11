#ifndef CAPY_APPLE_H
#define CAPY_APPLE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

typedef struct CapyApple CapyApple;
/* All session calls run on one serial engine/render owner. UIKit/AppKit owns
   the retained CAMetalLayer and must keep it alive until detach completes. */
CapyApple *capy_apple_create(uint32_t platform); /* 0 iPadOS, 1 macOS */
void capy_apple_destroy(CapyApple *app);
const char *capy_apple_error(const CapyApple *app); /* borrowed until next call */
void capy_apple_string_free(char *text);
typedef struct CapyProjectTask CapyProjectTask;
/* Capture/context and adopt/saved run on the editor owner. read/write/free run
   on the file worker. Jobs own immutable data, never an editor pointer. */
CapyProjectTask *capy_apple_project_task(CapyApple *app, uint32_t opening);
int32_t capy_apple_project_ready(CapyApple *app); /* 0 ready, 1 preparing filters, -1 interaction/error */
int32_t capy_project_matches(const CapyProjectTask *task, uint64_t epoch, uint64_t revision);
int32_t capy_project_write(const CapyProjectTask *task, int32_t fd);
int32_t capy_apple_export_task(CapyApple *app, uint32_t id, uint64_t now, CapyProjectTask **output);
int32_t capy_project_new(const CapyProjectTask *task, uint32_t width, uint32_t height);
int32_t capy_project_read(const CapyProjectTask *task, int32_t fd); /* -1: new */
int32_t capy_apple_project_adopt(CapyApple *app, const CapyProjectTask *task, const char *title, const char *uri);
int32_t capy_apple_project_saved(CapyApple *app, const CapyProjectTask *task, const char *title, const char *uri);
int32_t capy_apple_document_complete(CapyApple *app, uint32_t id, uint32_t succeeded);
int32_t capy_apple_document_close(CapyApple *app, uint32_t id, uint32_t decision);
void capy_project_cancel(const CapyProjectTask *task);
int32_t capy_project_begin_commit(const CapyProjectTask *task);
char *capy_project_error(const CapyProjectTask *task); /* owned, NULL on success */
void capy_project_free(CapyProjectTask *task);
/* Stateless numeric policy; safe on the UI thread. Owned JSON result contains
   either the shared numeric response or {"error": ...}. */
char *capy_apple_numeric(const char *json);
/* Stateless shared wheel hit test in local logical coordinates. Space: 0 HSV,
   1 HLS. Result: 0 miss/invalid, 1 hue ring, 2 field. No session/GPU access. */
uint32_t capy_apple_color_hit(float x, float y, float size, uint32_t space);
/* request: 0 action, 1 UI input, 2 query, 3 changed snapshot, 4 numeric control.
   Returned JSON is owned; release using capy_apple_string_free. NULL is either
   no changed snapshot or failure (consult capy_apple_error). */
char *capy_apple_request(CapyApple *app, uint32_t request, const char *json);
/* Nonblocking owner poll. The owned atlas is independent of the editor; decode
   and free on a worker. All info pointers are borrowed until previews_free.
   Pixels are straight sRGB RGBA8, with equal-height rows in filters JSON order. */
typedef struct CapyFilterPreviews CapyFilterPreviews;
typedef struct {
    uint64_t request;
    uint32_t width, height, stride;
    const uint8_t *pixels;
    size_t count;
    const char *filters;
} CapyFilterPreviewInfo;
CapyFilterPreviews *capy_apple_take_filter_previews(CapyApple *app);
void capy_filter_previews_read(const CapyFilterPreviews *previews, CapyFilterPreviewInfo *output);
void capy_filter_previews_free(CapyFilterPreviews *previews);
int32_t capy_apple_attach(CapyApple *app, void *metal_layer,
                         uint32_t width, uint32_t height, float scale,
                         const char *cache_directory);
int32_t capy_apple_finish_startup_cache(CapyApple *app);
/* Straight-alpha RGBA8 sRGB, tightly packed top-to-bottom rows. */
int32_t capy_apple_import_layer(CapyApple *app, const char *name, uint32_t width,
                               uint32_t height, const uint8_t *rgba, size_t count);
int32_t capy_apple_resize(CapyApple *app, uint32_t width, uint32_t height, float scale);
int32_t capy_apple_detach(CapyApple *app);
/* Nine doubles per record: x/y physical pixels, pressure, tilt x/y radians,
   twist radians, distance, monotonic nanoseconds, phase (0 hover..4 cancel).
   tool: 0 pen, 1 mouse, 2 eraser, 3 touch; button: 0 primary, 1 pan, 2 other. */
int32_t capy_apple_pointer(CapyApple *app, uint64_t id, uint32_t tool, uint32_t button,
                          const double *records, size_t count, uint32_t predicted,
                          uint64_t view_revision);
/* Anchors are physical canvas pixels; wheel deltas are logical points.
   Magnification is a multiplicative factor; rotation is in radians. */
int32_t capy_apple_scroll(CapyApple *app, float x, float y, float dx, float dy,
                          float scale, uint32_t zoom, uint32_t horizontal);
int32_t capy_apple_gesture(CapyApple *app, float x, float y, float scale, float rotation);
/* Returns 1 if more frames are needed, 0 when idle, -1 on error. Optional costs
   receives 5 nanosecond durations: paint, acquire, viewport, present, poll. */
int32_t capy_apple_frame(CapyApple *app, uint64_t now_ns, uint64_t presentation_ns,
                        uint64_t *costs);
uint64_t capy_apple_camera_revision(const CapyApple *app);
/* Optional GPU queue span (includes submission gaps, not GPU busy time or
   presentation latency). No timestamp submissions when disabled (default).
   Sample status: 1 valid, 2 map/read failure, 3 invalid timestamps.
   Support: 0 not initialized, 1 available, 2 unavailable. */
typedef struct { uint64_t frame, elapsed_ns, status; } CapyGpuFrameSample;
typedef struct { uint64_t support, requested, skipped, invalid, pending; } CapyGpuFrameTimingStats;
int32_t capy_apple_gpu_timing(CapyApple *app, uint32_t enabled);
/* Nonblocking poll and bounded drain: returns count or -1. Capacity <= 256;
   samples may be NULL only when capacity is zero; stats must be writable. */
int32_t capy_apple_take_gpu_timing(CapyApple *app, CapyGpuFrameSample *samples,
                                size_t capacity, CapyGpuFrameTimingStats *stats);
#ifdef __cplusplus
}
#endif
#endif
