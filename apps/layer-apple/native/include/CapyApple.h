#ifndef CAPY_APPLE_H
#define CAPY_APPLE_H
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#ifdef __cplusplus
extern "C" {
#endif

typedef uint64_t CapyApple;
typedef uint64_t CapyText;
typedef uint64_t CapyBytes;
/* Typed generation handles reject stale and wrong-kind values. Zero means no result.
   Every CapyText result is owned and requires capy_apple_string_free. Copies retain
   the result and accept ranges up to 8 MiB. Caller pointers remain valid and
   exclusive during calls. All session calls run on one fixed render thread. UIKit/AppKit owns
   the retained CAMetalLayer and must keep it alive until detach completes. */
typedef uint64_t CapyAppleLaunch;
CapyAppleLaunch capy_apple_prepare_launch(const char *json, CapyText *error);
CapyApple capy_apple_launch(uint32_t platform, CapyAppleLaunch prepared, CapyText *bootstrap, CapyText *error);
void capy_apple_launch_free(CapyAppleLaunch prepared);
typedef uint64_t CapyLanguageTask;
CapyLanguageTask capy_apple_language_request(CapyApple app, const char *preferred_languages);
void capy_language_prepare(CapyLanguageTask task);
bool capy_apple_language_prepared(CapyApple app, CapyLanguageTask task);
int32_t capy_apple_language_publish(CapyApple app, bool input_busy);
void capy_language_free(CapyLanguageTask task);
void capy_apple_destroy(CapyApple app);
CapyText capy_apple_error(CapyApple app); /* owned text; free with capy_apple_string_free */
void capy_apple_string_free(CapyText text);
size_t capy_apple_string_length(CapyText text);
bool capy_apple_string_copy(CapyText text, size_t offset, uint8_t *output, size_t count);
/* {"directory": path} names the stores within one private folder; {"platform":
   {config, data, state, cache, temp}} resolves the installation once per process
   and sets its temporary folder. Returns owned JSON paths or {"error": text}. */
CapyText capy_apple_storage(const char *request_json);
typedef uint64_t CapyDocumentTask;
typedef uint64_t CapySessionTask;
typedef uint64_t CapySessionDestination;
typedef CapySessionDestination (*CapySessionDestinationObserver)(const char *uri, void *context);
CapySessionTask capy_apple_session_open(CapyApple app, const char *sessions, const char *scene, bool adopt, bool retry);
CapySessionTask capy_apple_session_capture(CapyApple app, uint64_t exclusion, bool clean_exit);
int32_t capy_session_work(CapySessionTask task, CapySessionDestinationObserver observe, void *context);
CapySessionDestination capy_session_destination_read(int32_t fd);
void capy_session_destination_free(CapySessionDestination fingerprint);
int32_t capy_apple_session_adopt(CapyApple app, CapySessionTask task);
int32_t capy_session_restore_finished(CapySessionTask task);
CapyText capy_session_error(CapySessionTask task);
CapyText capy_session_failures(CapySessionTask task);
bool capy_session_committed(CapySessionTask task);
void capy_session_free(CapySessionTask task);
CapyDocumentTask capy_apple_document_switch(CapyApple app, uint64_t id, bool closing);
int32_t capy_apple_document_prepare_switch(CapyApple app, uint64_t now);
CapyDocumentTask capy_apple_document_storage(CapyApple app);
int32_t capy_document_prepare(CapyDocumentTask task);
int32_t capy_apple_document_close_commit(CapyApple app, CapyDocumentTask task, bool accept);
int32_t capy_apple_document_close_validate(CapyApple app, CapyDocumentTask task);
int32_t capy_apple_document_resume(CapyApple app, CapyDocumentTask task);
void capy_document_free(CapyDocumentTask task);
typedef uint64_t CapyPaperTask;
/* Capture/apply on serial owner; work/free on file worker; error after work. Request0 lists the library. */
CapyPaperTask capy_apple_paper_task(CapyApple app, uint32_t request, uint64_t epoch, const char *directory);
CapyPaperTask capy_apple_paper_import(CapyApple app, const char *directory); /* library only; the drawing is unchanged */
void capy_paper_work(CapyPaperTask task, int32_t fd, const char *name);
void capy_paper_failed(CapyPaperTask task, const char *message);
CapyText capy_paper_error(CapyPaperTask task);
int32_t capy_apple_paper_apply(CapyApple app, CapyPaperTask task);
void capy_paper_cancel(CapyPaperTask task);
void capy_paper_free(CapyPaperTask task);
typedef uint64_t CapyPaperPreviews;
/* Owner captures immutable grayscale leases; read/decode/free away from UI/input. */
CapyPaperPreviews capy_apple_paper_previews(CapyApple app, const char *ids);
size_t capy_paper_previews_count(CapyPaperPreviews previews);
uint32_t capy_paper_previews_read(CapyPaperPreviews previews, size_t index, size_t *count);
bool capy_paper_previews_copy(CapyPaperPreviews previews, size_t index, size_t offset, uint8_t *output, size_t count);
void capy_paper_previews_free(CapyPaperPreviews previews);
typedef uint64_t CapyProjectTask;
int32_t capy_apple_project_prepare_adopt(CapyApple app, CapyProjectTask task, uint64_t now);
/* Capture/context and adopt/saved run on the editor owner. read/write/free run
   on the file worker. Jobs own immutable data, never an editor pointer. */
/* kind: 0 save, 1 open, 3 Place/Paste, 4 color/history, 5 properties, 6 source, 7 histogram. */
CapyProjectTask capy_apple_project_task(CapyApple app, uint32_t kind, const char *placement);
int32_t capy_apple_project_ready(CapyApple app); /* 0 ready, 1 preparing filters, -1 interaction/error */
int32_t capy_project_matches(CapyProjectTask task, uint64_t epoch, uint64_t revision);
int32_t capy_project_destination_matches(CapyProjectTask task, int32_t descriptor, const char *uri);
int32_t capy_project_write(CapyProjectTask task, int32_t fd);
int32_t capy_project_export_preview(CapyProjectTask task, int32_t fd);
int32_t capy_apple_export_task(CapyApple app, uint32_t id, uint64_t now, CapyProjectTask *output);
int32_t capy_project_export_options(CapyProjectTask task, const char *recipe_json); /* worker */
CapyText capy_project_clip_progress(CapyProjectTask task); /* zero unless large */
int32_t capy_project_clip_run(CapyProjectTask task, const char *nonce); /* worker */
int32_t capy_project_clip_png(CapyProjectTask task, size_t *count); /* worker */
bool capy_project_clip_png_copy(CapyProjectTask task, size_t offset, uint8_t *output, size_t count);
void capy_project_clip_published(CapyProjectTask task);
CapyText capy_apple_clip_nonce(CapyApple app);
/* Proof preparation borrows no editor. Build and profile copying run on the
   worker; check/apply/failed run on the owner. Request 0 restores viewing only. */
CapyProjectTask capy_apple_proof_task(CapyApple app, uint32_t request, const char *recipe_json);
int32_t capy_project_proof_build(CapyProjectTask task);
int32_t capy_project_proof_preservation(CapyProjectTask task, size_t *count);
bool capy_project_proof_preservation_copy(CapyProjectTask task, size_t offset, uint8_t *output, size_t count);
int32_t capy_apple_proof_check(CapyApple app, CapyProjectTask task);
int32_t capy_apple_proof_apply(CapyApple app, CapyProjectTask task, bool preserved);
int32_t capy_apple_proof_failed(CapyApple app, CapyProjectTask task, const char *message);
int32_t capy_project_new(CapyProjectTask task); /* worker; the order New drawing's Create left */
int32_t capy_project_read(CapyProjectTask task, int32_t fd, const char *name);
int32_t capy_project_finish_images(CapyProjectTask task);
int32_t capy_project_read_bytes(CapyProjectTask task, const uint8_t *bytes, size_t count, const char *name);
CapyText capy_photo_formats(void);
size_t capy_clipboard_image_bytes(void);
bool capy_drawing_path(const char *path);
CapyText capy_apple_file_drop(const char *json);
CapyText capy_project_profile(CapyProjectTask task); /* owned JSON interpretation or null */
int32_t capy_project_assume_profile(CapyProjectTask task, const char *profile_json);
int32_t capy_project_edit_work(CapyProjectTask task, const char *choice_json, bool copy);
int32_t capy_apple_project_candidate(CapyApple app, CapyProjectTask task); /* owner after edit_work */
int32_t capy_project_compare(CapyProjectTask task); /* worker after candidate */
CapyText capy_profile_library(const char *request_json, const uint8_t *bytes, size_t count); /* worker; bounded ICC, owned JSON */
CapyText capy_export_draft(const char *recipe_json, const char *action_json); /* worker; owned shared draft JSON */
CapyText capy_palette_file(const char *request_json, const uint8_t *bytes, size_t count, int32_t output_fd); /* worker; owned metadata JSON, export bytes to output_fd */
int32_t capy_apple_glass_regions(CapyApple app, const char *json); /* owner; logical regions [x,y,w,h,tl,tr,br,bl,shape,strength], arcs [cx,cy,radius,band,start,sweep,strength], color_bubbles [cx,cy,wheel_radius,arc_radius,cap,end_radians,strength], connections */
CapyBytes capy_apple_stroke_recording_data(CapyApple app); /* owner; raw capture copy or NULL with error */
void capy_bytes_free(CapyBytes bytes);
size_t capy_bytes_length(CapyBytes bytes);
bool capy_bytes_copy(CapyBytes bytes, size_t offset, uint8_t *output, size_t count);
CapyText capy_stroke_recording_write(const uint8_t *bytes, size_t count, int32_t output_fd); /* worker; NULL or owned error JSON */
CapyText capy_export_presets(int32_t input_fd, int32_t output_fd, const char *request_json, const char *color_json); /* worker; host atomically publishes changed output */
CapyText capy_project_details(CapyProjectTask task); /* owned JSON; worker only */
typedef struct { uint32_t width, height; size_t count; } CapyProjectPreview;
/* Worker-only metadata and copied straight Display P3 RGBA8. */
int32_t capy_project_preview_at(CapyProjectTask task, uint32_t index, CapyProjectPreview *output);
bool capy_project_preview_copy(CapyProjectTask task, uint32_t index, size_t offset, uint8_t *output, size_t count);
CapyText capy_project_package_summary(CapyProjectTask task);
/* Worker-only metadata and copied bounded straight sRGB RGBA8. */
int32_t capy_project_package_preview(CapyProjectTask task, CapyProjectPreview *output);
bool capy_project_package_preview_copy(CapyProjectTask task, size_t offset, uint8_t *output, size_t count);
int32_t capy_apple_project_adopt(CapyApple app, CapyProjectTask task, const char *title, const char *uri);
int32_t capy_apple_prepare_recovery(CapyApple app, uint64_t now); /* 0 capturable, 1 preparing, -1 error; not durable */
int32_t capy_apple_project_saved(CapyApple app, CapyProjectTask task, const char *title, const char *uri);
int32_t capy_apple_document_complete(CapyApple app, uint32_t id, uint32_t succeeded);
int32_t capy_apple_document_close(CapyApple app, uint32_t id, uint32_t decision);
void capy_project_cancel(CapyProjectTask task);
int32_t capy_project_begin_commit(CapyProjectTask task);
CapyText capy_project_error(CapyProjectTask task); /* owned, NULL on success */
void capy_project_free(CapyProjectTask task);
/* Stateless presentation requests contain {"language": tag, "request": payload}.
   The tag selects a previously prepared immutable context.
   Numeric policy is safe on the UI thread. Owned JSON result contains
   either the shared numeric response or {"error": ...}. */
CapyText capy_apple_numeric(const char *json);
bool capy_apple_control_takes_key(const char *key, bool command, bool alt, bool keyboard_focus);
CapyText capy_apple_native_caption(const char *json);
CapyText capy_apple_numeric_labels(const char *json);
/* Stateless shared tagged-color forms, previews in the requested display space and gradient
   samples. Owned JSON result; same error and lifetime rules as numeric. */
CapyText capy_apple_color_ui(const char *json);
CapyText capy_apple_toolbar_ui(const char *json);
CapyText capy_apple_bubble_ui(const char *json);
CapyText capy_apple_drawer_frame(const char *json);
CapyText capy_apple_settings_search_key(const char *json);
/* Stateless shared wheel hit test in local logical coordinates. Shape: 0 circle,
   1 square, 2 triangle. Result: 0 miss/invalid, 1 hue ring, 2 field. */
uint32_t capy_apple_color_hit(float x, float y, float size, uint32_t shape);
// operation: 0 = generate and map; 1 = generate base; 2 = map retained base in place.
bool capy_apple_hdr_field(uint32_t side, const char *request, float *pixels, size_t count, uint32_t operation);
uint32_t capy_apple_parameter_hit(float x, float y, float size, bool hdr);
bool capy_apple_proof_texture(uint32_t edge, uint8_t *bytes, size_t count);
/* Stateless cached field or hue guide: square, straight Display P3 RGBA8.
   Caller owns side*side*4 writable bytes. Returns 1 on success, 0 invalid.
   No session access or retained pointers; safe on the UI thread. */
int32_t capy_apple_color_field(uint32_t side, float hue, uint32_t shape, const char *rgb_space, bool guide, uint8_t *rgba, size_t count);
/* request: 0 action, 1 UI input, 2 query, 6 workspace control or capture, 7 layout-aware incremental update.
   Workspace start/input/tick replies {view,wake,refresh}; SQLite stays on its worker.
   Returned JSON is owned; release using capy_apple_string_free. NULL is either
   no changed snapshot or failure (consult capy_apple_error). */
CapyText capy_apple_request(CapyApple app, uint32_t request, const char *json);
/* Nonblocking owner poll. The owned atlas is independent of the editor; decode
   and free on a worker. All info pointers are borrowed until previews_free.
   Pixels are straight Display P3 RGBA8, with equal-height rows in filters JSON order. */
typedef uint64_t CapyFilterPreviews;
typedef struct {
    uint64_t request;
    uint32_t width, height, stride;
    size_t count;
    CapyText filters;
} CapyFilterPreviewInfo;
/* Transfers the shared query's completed atlas; cancellation is owned by Rust. */
CapyFilterPreviews capy_apple_take_filter_previews(CapyApple app);
bool capy_filter_previews_read(CapyFilterPreviews previews, CapyFilterPreviewInfo *output);
bool capy_filter_previews_copy(CapyFilterPreviews previews, size_t offset, uint8_t *output, size_t count);
void capy_filter_previews_free(CapyFilterPreviews previews);
/* Owner poll after a publication; NULL when no plot changed. plots JSON is
   {plots:{histogram|waveform|tonal_histogram:{plot,extent?}},colors}; plot holds
   [channel, normalized bins] and colors index sRGB channels. Waveform pixels are
   premultiplied sRGB RGBA8 rows of extent[0] width. Borrowed until scopes_free. */
typedef uint64_t CapyScopes;
typedef struct {
    CapyText plots;
    size_t count;
} CapyScopeInfo;
CapyScopes capy_apple_take_scopes(CapyApple app);
bool capy_scopes_read(CapyScopes scopes, CapyScopeInfo *output);
bool capy_scopes_copy(CapyScopes scopes, size_t offset, uint8_t *output, size_t count);
void capy_scopes_free(CapyScopes scopes);
/* Logical editor bounds/clip/order records. Owner-only, maximum 32 slots.
   The live overview is drawn in the existing Metal canvas presentation pass. */
int32_t capy_apple_navigator_placements(CapyApple app, const char *json);
/* Stateless [Camera, documentExtent, viewport] -> shared geometry JSON. */
CapyText capy_apple_navigator_geometry(const char *json);
int32_t capy_apple_attach(CapyApple app, void *metal_layer,
                         uint32_t width, uint32_t height, float scale);
int32_t capy_apple_finish_startup_catalog(CapyApple app);
int32_t capy_apple_resize(CapyApple app, uint32_t width, uint32_t height, float scale);
int32_t capy_apple_redraw(CapyApple app);
int32_t capy_apple_detach(CapyApple app);
typedef struct CapyFrameRateObserver CapyFrameRateObserver;
/* Owner-only acquisition. The returned Arc remains valid through session retirement.
   Presentation calls may run on any thread while retained; release exactly once. */
const CapyFrameRateObserver *capy_apple_frame_rate_observer(CapyApple app);
void capy_apple_frame_rate_presented(const CapyFrameRateObserver *observer,
                                    uint64_t presented_ns, uint64_t observed_ns);
void capy_apple_frame_rate_observer_release(const CapyFrameRateObserver *observer);
int32_t capy_apple_suspend_renderer(CapyApple app);
int32_t capy_apple_poll_renderer(CapyApple app); /* 0 available, 1 suspended, -1 error */
int32_t capy_apple_test_gpu_fault(CapyApple app, uint32_t validation); /* Debug builds only */
/* Nine doubles per record: x/y physical pixels, pressure, tilt x/y radians,
   twist radians, distance, monotonic nanoseconds, phase (0 hover..4 cancel).
   tool: 0 pen, 1 mouse, 2 eraser, 3 touch; button: 0 primary, 1 pan, 2 other. */
enum { CAPY_APPLE_INDIRECT_POINTER = 0x40, CAPY_APPLE_BARREL_TWIST = 0x80 };
int32_t capy_apple_pointer(CapyApple app, uint64_t id, uint32_t tool, uint32_t button,
                          const double *records, size_t count, uint32_t predicted,
                          uint64_t view_revision, uint32_t device_flags);
/* Estimate metadata is two uint64_t values per nine-double sample: an opaque
   contact-local token (zero for untracked samples) and expecting updates (0/1).
   Corrections replace previously admitted points, including after pen-up, and
   must carry the original contact and view revision. They never route UI input. */
int32_t capy_apple_pointer_updates(CapyApple app, uint64_t id, uint32_t tool, uint32_t button,
                                  const double *records, size_t count, const uint64_t *updates,
                                  uint32_t correction, uint64_t view_revision, uint32_t device_flags);
/* Anchors are physical canvas pixels; wheel deltas are logical points.
   Magnification is a multiplicative factor; rotation is in radians. */
int32_t capy_apple_scroll(CapyApple app, float x, float y, float dx, float dy,
                          float scale, uint32_t zoom, uint32_t horizontal);
int32_t capy_apple_gesture(CapyApple app, float x, float y, float scale, float rotation, uint32_t began);
/* Returns 1 if more frames are needed, 0 when idle, -1 on error. Optional costs
   receives 5 nanosecond durations: paint, acquire, viewport, present, poll. */
int32_t capy_apple_frame(CapyApple app, uint64_t now_ns, uint64_t presentation_ns,
                        uint64_t *costs);
/* Last consumed real Down/Move timestamp; serial owner only, zero for NULL. */
uint64_t capy_apple_consumed_paint_ns(CapyApple app);
uint64_t capy_apple_submitted_canvas_updates(CapyApple app);
uint64_t capy_apple_camera_revision(CapyApple app);
void capy_apple_shader_input(CapyApple app);
uint32_t capy_apple_canvas_bar_hold(CapyApple app);
/* Optional GPU queue span (includes submission gaps, not GPU busy time or
   presentation latency). No timestamp submissions when disabled (default).
   Sample status: 1 valid, 2 map/read failure, 3 invalid timestamps.
   Support: 0 not initialized, 1 available, 2 unavailable. */
/* Raw timestamp endpoints need native clock calibration before CPU comparison. */
typedef struct { uint64_t frame, elapsed_ns, status, start_tick, end_tick; } CapyGpuFrameSample;
typedef struct { uint64_t support, requested, skipped, invalid, pending; } CapyGpuFrameTimingStats;
int32_t capy_apple_gpu_timing(CapyApple app, uint32_t enabled);
/* Nonblocking poll and bounded drain: returns count or -1. Capacity <= 256;
   samples may be NULL only when capacity is zero; stats must be writable. */
int32_t capy_apple_take_gpu_timing(CapyApple app, CapyGpuFrameSample *samples,
                                size_t capacity, CapyGpuFrameTimingStats *stats);
#ifdef __cplusplus
}
#endif
#endif
