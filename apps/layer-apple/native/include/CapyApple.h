#ifndef CAPY_APPLE_H
#define CAPY_APPLE_H
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
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
typedef struct CapyWorkspaceLibrary CapyWorkspaceLibrary;
/* A separate serial workspace owner, never the UI or input/render queue.
   SQLite I/O is shared across windows by the Rust storage worker.
   Replies are owned JSON envelopes {value,status} or {error,status}.
   Flush/close explicitly before destroy; destroy alone cannot confirm saving. */
CapyWorkspaceLibrary *capy_workspace_library_create(uint32_t platform, const char *directory, const char *resume_key);
char *capy_workspace_library_request(CapyWorkspaceLibrary *library, const char *json);
void capy_workspace_library_destroy(CapyWorkspaceLibrary *library);
typedef struct CapyProjectTask CapyProjectTask;
/* Capture/context and adopt/saved run on the editor owner. read/write/free run
   on the file worker. Jobs own immutable data, never an editor pointer. */
/* kind: 0 save, 1 open, 2 recovery, 3 Place/Paste, 4 color/history, 5 properties, 6 source, 7 histogram. */
CapyProjectTask *capy_apple_project_task(CapyApple *app, uint32_t kind, const char *placement);
int32_t capy_apple_project_ready(CapyApple *app); /* 0 ready, 1 preparing filters, -1 interaction/error */
int32_t capy_project_matches(const CapyProjectTask *task, uint64_t epoch, uint64_t revision);
int32_t capy_project_write(const CapyProjectTask *task, int32_t fd);
int32_t capy_apple_export_task(CapyApple *app, uint32_t id, uint64_t now, CapyProjectTask **output);
int32_t capy_project_export_options(const CapyProjectTask *task, const char *recipe_json); /* worker */
int32_t capy_project_new(const CapyProjectTask *task, const char *options_json);
int32_t capy_project_read(const CapyProjectTask *task, int32_t fd, const char *name); /* -1: new */
int32_t capy_project_read_bytes(const CapyProjectTask *task, const uint8_t *bytes, size_t count, const char *name);
char *capy_photo_formats(void);
char *capy_project_profile(const CapyProjectTask *task); /* owned JSON interpretation or null */
int32_t capy_project_assume_profile(const CapyProjectTask *task, const char *profile_json);
int32_t capy_project_edit_work(const CapyProjectTask *task, const char *choice_json, bool copy);
int32_t capy_apple_project_candidate(CapyApple *app, const CapyProjectTask *task); /* owner after edit_work */
int32_t capy_project_compare(const CapyProjectTask *task); /* worker after candidate */
char *capy_profile_library(const char *request_json, const uint8_t *bytes, size_t count); /* worker; bounded ICC, owned JSON */
char *capy_export_draft(const char *recipe_json, const char *action_json); /* worker; owned shared draft JSON */
char *capy_export_presets(int32_t input_fd, int32_t output_fd, const char *request_json, const char *color_json); /* worker; host atomically publishes changed output */
char *capy_project_details(const CapyProjectTask *task); /* owned JSON; worker only */
typedef struct { uint32_t width, height; const uint8_t *pixels; size_t count; } CapyProjectPreview;
/* Worker only; borrowed straight Display P3 RGBA8 until the next mutation/free. */
int32_t capy_project_preview(const CapyProjectTask *task, bool after, CapyProjectPreview *output);
int32_t capy_apple_project_adopt(CapyApple *app, const CapyProjectTask *task, const char *title, const char *uri);
int32_t capy_apple_project_recover(CapyApple *app, const CapyProjectTask *task);
int32_t capy_apple_prepare_recovery(CapyApple *app, uint64_t now); /* 0 capturable, 1 preparing, -1 error; not durable */
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
/* Stateless shared tagged-color forms, previews in the requested display space and gradient
   samples. Owned JSON result; same error and lifetime rules as numeric. */
char *capy_apple_color_ui(const char *json);
/* Stateless shared wheel hit test in local logical coordinates. Shape: 0 circle,
   1 square, 2 triangle. Result: 0 miss/invalid, 1 hue ring, 2 field. */
uint32_t capy_apple_color_hit(float x, float y, float size, uint32_t shape);
/* Shared logical panel layout. Owned JSON, NULL for invalid input.
   Cache by size; release with capy_apple_string_free. */
char *capy_apple_color_layout(float size);
/* Stateless cached field or hue guide: square, straight Display P3 RGBA8.
   Caller owns side*side*4 writable bytes. Returns 1 on success, 0 invalid.
   No session access or retained pointers; safe on the UI thread. */
int32_t capy_apple_color_field(uint32_t side, float hue, uint32_t shape, const char *rgb_space, bool guide, uint8_t *rgba, size_t count);
/* request: 0 action, 1 UI input, 2 query, 3 compatibility snapshot, 4 numeric control,
 * 5 incremental update (full models or workspace/camera presentation),
 * 6 workspace session capture/transition/adoption (no database I/O),
 * 7 layout-aware incremental update (retains controls across live resizing).
   Returned JSON is owned; release using capy_apple_string_free. NULL is either
   no changed snapshot or failure (consult capy_apple_error). */
char *capy_apple_request(CapyApple *app, uint32_t request, const char *json);
/* Nonblocking owner poll. The owned atlas is independent of the editor; decode
   and free on a worker. All info pointers are borrowed until previews_free.
   Pixels are straight Display P3 RGBA8, with equal-height rows in filters JSON order. */
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
/* Logical editor bounds/clip/order records. Owner-only, maximum 32 slots.
   The live overview is drawn in the existing Metal canvas presentation pass. */
int32_t capy_apple_navigator_placements(CapyApple *app, const char *json);
/* Stateless [Camera, documentExtent, viewport] -> shared geometry JSON. */
char *capy_apple_navigator_geometry(const char *json);
int32_t capy_apple_attach(CapyApple *app, void *metal_layer,
                         uint32_t width, uint32_t height, float scale,
                         const char *cache_directory);
int32_t capy_apple_finish_startup_cache(CapyApple *app);
int32_t capy_apple_resize(CapyApple *app, uint32_t width, uint32_t height, float scale);
int32_t capy_apple_redraw(CapyApple *app);
int32_t capy_apple_detach(CapyApple *app);
int32_t capy_apple_suspend_renderer(CapyApple *app);
int32_t capy_apple_poll_renderer(CapyApple *app); /* 0 available, 1 suspended, -1 error */
int32_t capy_apple_test_gpu_fault(CapyApple *app, uint32_t validation); /* Debug builds only */
/* Nine doubles per record: x/y physical pixels, pressure, tilt x/y radians,
   twist radians, distance, monotonic nanoseconds, phase (0 hover..4 cancel).
   tool: 0 pen, 1 mouse, 2 eraser, 3 touch; button: 0 primary, 1 pan, 2 other. */
int32_t capy_apple_pointer(CapyApple *app, uint64_t id, uint32_t tool, uint32_t button,
                          const double *records, size_t count, uint32_t predicted,
                          uint64_t view_revision);
/* Estimate metadata is two uint64_t values per nine-double sample: an opaque
   contact-local token (zero for untracked samples) and expecting updates (0/1).
   Corrections replace previously admitted points, including after pen-up, and
   must carry the original contact and view revision. They never route UI input. */
int32_t capy_apple_pointer_updates(CapyApple *app, uint64_t id, uint32_t tool, uint32_t button,
                                  const double *records, size_t count, const uint64_t *updates,
                                  uint32_t correction, uint64_t view_revision);
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
/* Raw timestamp endpoints need native clock calibration before CPU comparison. */
typedef struct { uint64_t frame, elapsed_ns, status, start_tick, end_tick; } CapyGpuFrameSample;
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
