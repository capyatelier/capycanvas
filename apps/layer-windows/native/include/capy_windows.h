#pragma once
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif
typedef struct CapyHost CapyHost;
typedef struct CapyPreview CapyPreview;
/* Render-owner-only request/poll, nonblocking GPU readback. CPU-only packet
   transfers to a worker and is freed once; buffers live until that free. */
__declspec(dllimport) CapyPreview* capy_filter_previews(CapyHost*, const char* json);
__declspec(dllimport) CapyPreview* capy_layer_thumbnails(CapyHost*, const char* json);
__declspec(dllimport) CapyPreview* capy_layer_menu(CapyHost*, const char* json);
/* Read-only workspace geometry/menu queries, on the render owner. Metadata
   contains result (any JSON type) and error (null or string); no pixel payload. */
__declspec(dllimport) CapyPreview* capy_workspace_query(CapyHost*, const char* json);
__declspec(dllimport) const char* capy_preview_metadata(const CapyPreview*);
__declspec(dllimport) const uint8_t* capy_preview_bytes(const CapyPreview*, size_t* length);
__declspec(dllimport) void capy_preview_free(CapyPreview*);
typedef struct CapyPointer {
    uint64_t id, timestamp_ns, sequence, view_revision;
    float x, y, pressure, tilt_x, tilt_y, twist, distance;
    uint32_t phase; /* hover=0, down=1, move=2, up=3, cancel=4 */
    uint32_t tool;  /* pen=0, mouse=1, eraser=2, touch=3 */
    uint32_t button; /* primary=0, pan=1, other=2 */
    uint32_t flags; /* shared predicted/primary/barrel/inverted bits; 0x40 indirect pen */
} CapyPointer;

/* Create on XAML thread with Microsoft ISwapChainPanelNative interface pointer.
   Then transfer exclusive ownership to the canvas thread. */
__declspec(dllimport) CapyHost* capy_create(void* panel, uint32_t width, uint32_t height, float scale);
/* Prepare on the render worker, then park it for the first UI-thread capy_resize.
   Every subsequent resize also requires exclusive ownership on the UI thread. */
__declspec(dllimport) int32_t capy_prepare_gpu(CapyHost*);
__declspec(dllimport) int32_t capy_test_display(CapyHost*, bool hdr);
__declspec(dllimport) int32_t capy_set_window(CapyHost*, void* hwnd);
/* Device loss is observed by the render owner. A poisoned host cannot recover. */
__declspec(dllimport) bool capy_device_lost(const CapyHost*);
/* CPU retirement after input admission stops; keep document services alive. */
__declspec(dllimport) int32_t capy_suspend_renderer(CapyHost*);
/* UI-thread replacement: worker parked, no acquired image, old swap chain detached. */
__declspec(dllimport) int32_t capy_reset_surface(CapyHost*, void* panel);
/* Isolated CAPY_SMOKE_TEST only; removes the process's D3D12 device, never the adapter. */
__declspec(dllimport) int32_t capy_test_device_loss(CapyHost*);
/* Start/load on the render owner before queued user actions. Wake runs on the
   storage thread and must only signal owned synchronization state. */
__declspec(dllimport) int32_t capy_start_services(CapyHost*, void* context, void (*wake)(void*));
/* Returns 1 when canvas work is pending; 0 permits an idle service-only tick. */
__declspec(dllimport) int32_t capy_poll_services(CapyHost*);
__declspec(dllimport) int32_t capy_workspace_action(CapyHost*, const char* json);
/* Cold runtime package transport on the render owner. JSON: directory (optional),
   mode (add/replace/merge), library (default false; true preserves embedded project
   programs). Directory omitted reloads installed/overridden resources.
   1=busy/rejected, -1=fatal; progress/error is in windows_filter_load snapshots. */
__declspec(dllimport) int32_t capy_load_filter_directory(CapyHost*, const char* json);
/* Logical native overview slots; call only on the canvas owner. */
__declspec(dllimport) int32_t capy_overviews(CapyHost*, const char* json);
__declspec(dllimport) int32_t capy_glass(CapyHost*, const char* json);
/* Pure shared image bounds for the native cutout; output has four floats. */
__declspec(dllimport) bool capy_navigator_image(float width, float height, uint32_t document_width, uint32_t document_height, float* output);
__declspec(dllimport) float capy_navigator_aspect(uint32_t document_width, uint32_t document_height);
/* Stateless shared color presentation: projection 0 square, 1 triangle, 2 circle.
   Hit result: 0 none, 1 hue, 2 field. Free returned strings with capy_string_free.
   Field output is exactly side*side*4 writable RGBA8 bytes, side in 1..=2048. */
__declspec(dllimport) uint32_t capy_color_hit(float x, float y, float size, uint32_t projection);
__declspec(dllimport) CapyPreview* capy_document_preview(CapyHost* host, char const* json);
__declspec(dllimport) char* capy_export_draft(char const* json);
__declspec(dllimport) bool capy_color_raster(uint32_t side, float hue, uint32_t projection, uint32_t space, bool guide, uint8_t* output, size_t length);
__declspec(dllimport) char* capy_color_ui(char const* json);
__declspec(dllimport) char* capy_color_layout(float size);
__declspec(dllimport) char* capy_color_hue_stops(uint32_t projection);
__declspec(dllimport) bool capy_color_mapped_field(uint32_t side, char const* input, uint8_t* output, size_t length);
__declspec(dllimport) bool capy_color_field(uint32_t side, float hue, uint32_t projection, uint8_t* output, size_t length);
/* Flush/join on the render owner before destroying the callback context.
   Cleanup is required even after a renderer failure. */
__declspec(dllimport) int32_t capy_finish_services(CapyHost*);
__declspec(dllimport) void capy_destroy(CapyHost*);
/* Process exit only, after every canvas host has been destroyed. */
__declspec(dllimport) int32_t capy_finish_process();
/* Read-only DXGI diagnostics: six counters; 0=valid, HRESULT otherwise. */
__declspec(dllimport) int32_t capy_presentation_stats(const CapyHost*, uint64_t* values);
__declspec(dllimport) const char* capy_error(void);
__declspec(dllimport) int32_t capy_resize(CapyHost*, uint32_t width, uint32_t height, float scale);
__declspec(dllimport) int32_t capy_pointer(CapyHost*, const CapyPointer*, size_t count);
/* 1=action rejected (capy_error explains); the host remains usable. -1=fatal. */
__declspec(dllimport) int32_t capy_action(CapyHost*, const char* json);
/* Typed document dialog responses, queued to the same render owner as actions.
   capy_finish_services cancels outstanding document work and joins its callback. */
__declspec(dllimport) int32_t capy_document_action(CapyHost*, const char* json);
__declspec(dllimport) int32_t capy_native_prediction(CapyHost* host, bool available);
int32_t capy_input(CapyHost*, const char* json);
/* Physical anchor, logical wheel deltas, current composition density. */
__declspec(dllimport) int32_t capy_scroll(CapyHost*, float x, float y, float dx, float dy, float density, bool zoom, bool horizontal);
/* 0=refresh, 1=motion, 2=contact, 3=leave. Returns handled/dismiss bits. */
__declspec(dllimport) int32_t capy_chrome(CapyHost*, uint32_t kind, float x, float y, bool canvas, bool popup_open, bool touch);
__declspec(dllimport) int32_t capy_suspend(CapyHost*);
__declspec(dllimport) uint64_t capy_view_revision(const CapyHost*);
/* Acquisition: 0=wait, 1=image, 2=UI-thread reconfiguration, 3=commands only while a drawing renderer is unavailable. Can wait for DXGI. Drain newly arrived input after this call. */
__declspec(dllimport) int32_t capy_acquire(CapyHost*);
__declspec(dllimport) int32_t capy_frame(CapyHost*, uint64_t now_ns, uint64_t presentation_ns);
/* Returned UTF-8 owned by Rust; release with capy_string_free. Null=no change. */
__declspec(dllimport) char* capy_snapshot(CapyHost*);
__declspec(dllimport) char* capy_query(CapyHost*, const char* json);
/* Local-only DXGI identity for correlating a presentation probe with ETW. */
__declspec(dllimport) char* capy_surface_info(CapyHost*);
/* Pure numeric policy; does not touch the render owner's host. */
__declspec(dllimport) bool capy_document_tabs_compact(float width, size_t count);
__declspec(dllimport) char* capy_document_tab_slide(const char* json);
__declspec(dllimport) char* capy_proof_dial(const char* json);
__declspec(dllimport) bool capy_proof_texture(uint32_t edge, uint8_t* output, size_t length);
__declspec(dllimport) char* capy_number(const char* json);
__declspec(dllimport) char* capy_toolbar_ui(const char* json);
__declspec(dllimport) void capy_string_free(char*);
#ifdef __cplusplus
}
#endif
