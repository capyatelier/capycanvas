#ifndef LAYER_H
#define LAYER_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct LayerCanvas LayerCanvas;
typedef int32_t LayerStatus;

enum {
  LAYER_STATUS_OK = 0,
  LAYER_STATUS_NULL_POINTER = 1,
  LAYER_STATUS_INVALID_ARGUMENT = 2,
  LAYER_STATUS_QUEUE_FULL = 3,
  LAYER_STATUS_BUFFER_TOO_SMALL = 4,
  LAYER_STATUS_DOCUMENT_ERROR = 5,
  LAYER_STATUS_RENDER_ERROR = 6,
  LAYER_STATUS_PANIC = 255,
};

enum {
  LAYER_PEN_HOVER = 0,
  LAYER_PEN_DOWN = 1,
  LAYER_PEN_MOVE = 2,
  LAYER_PEN_UP = 3,
  LAYER_PEN_CANCEL = 4,
};

enum {
  LAYER_TOOL_UNKNOWN = 0,
  LAYER_TOOL_PEN = 1,
  LAYER_TOOL_ERASER = 2,
  LAYER_TOOL_BRUSH = 3,
  LAYER_TOOL_PENCIL = 4,
  LAYER_TOOL_AIRBRUSH = 5,
  LAYER_TOOL_FINGER = 6,
  LAYER_TOOL_MOUSE = 7,
};

enum {
  LAYER_SAMPLE_PREDICTED = 1u << 0,
  LAYER_SAMPLE_PRIMARY = 1u << 1,
  LAYER_SAMPLE_BARREL_BUTTON = 1u << 2,
  LAYER_SAMPLE_INVERTED = 1u << 3,
};

enum {
  LAYER_BRUSH_G_PEN = 1,
  LAYER_BRUSH_PENCIL = 2,
  LAYER_BRUSH_ERASER = 3,
  LAYER_BRUSH_PAINTBRUSH = 4,
  LAYER_BRUSH_AIRBRUSH = 5,
  LAYER_BRUSH_CHALK = 6,
  LAYER_BRUSH_MARKER = 7,
  LAYER_BRUSH_SPRAY = 8,
  LAYER_BRUSH_DUAL_TEXTURE = 9,
  LAYER_BRUSH_SMUDGE = 10,
  LAYER_BRUSH_WET_ROUND = 11,
  LAYER_BRUSH_LIQUIFY_PUSH = 12,
  LAYER_BRUSH_LIQUIFY_TWIRL = 13,
  LAYER_BRUSH_MULTIPLY_GLAZE = 14,
  LAYER_BRUSH_TEXTURED_FLAT = 15,
  LAYER_BRUSH_DRY_SCUMBLE = 16,
  LAYER_BRUSH_PASTEL_BLOCK = 17,
  LAYER_BRUSH_TRANSPARENT_GLAZE = 18,
  LAYER_BRUSH_OPAQUE_GOUACHE = 19,
  LAYER_BRUSH_WATERCOLOR_WASH = 20,
  LAYER_BRUSH_WET_WATERCOLOR = 21,
  LAYER_BRUSH_LOADED_OIL = 22,
  LAYER_BRUSH_PALETTE_KNIFE = 23,
  LAYER_BRUSH_NATURAL_BLENDER = 24,
};

typedef struct LayerCanvasConfig {
  uint32_t document_width;
  uint32_t document_height;
  uint32_t surface_width;
  uint32_t surface_height;
  uint32_t input_capacity;
  uint32_t stroke_point_capacity;
  uint32_t dab_capacity;
  uint32_t batch_capacity;
  float background_rgba_linear[4];
} LayerCanvasConfig;

typedef struct LayerPenEvent {
  uint64_t device_id;
  uint64_t sequence;
  uint64_t timestamp_ns;
  uint64_t view_revision;
  float x_physical_px;
  float y_physical_px;
  float pressure;
  float tilt_x_radians;
  float tilt_y_radians;
  float twist_radians;
  float distance;
  uint32_t phase;
  uint32_t tool;
  uint32_t flags;
} LayerPenEvent;

typedef struct LayerViewState {
  uint64_t revision;
  uint32_t surface_width;
  uint32_t surface_height;
  float document_to_surface[6];
  float surface_to_document[6];
  float background_rgba_linear[4];
} LayerViewState;

typedef struct LayerBrushSettings {
  uint32_t preset;
  float diameter_document_px;
  float opacity;
  float color_rgba_linear[4];
  float streamline;
  float pressure_smoothing;
  float stabilization;
  float motion_filtering;
  float stabilization_expression;
  /* 0 keeps the preset; 1..4 select long-narrow, long-broad,
     short-narrow, and short-broad conductance. */
  uint32_t transport_field;
  float transport_scale;
  float transport_rotation_radians;
  float transport_contrast;
  /* Existing-wet and initially-dry pigment exchange rates, independently. */
  float transport_wet_flow;
  float transport_dry_flow;
  float transport_distance_px;
  float transport_water_load;
} LayerBrushSettings;

typedef struct LayerInstantFeedbackSettings {
  uint8_t enabled;
  uint8_t use_platform_prediction;
  uint8_t use_engine_prediction;
  uint8_t reserved;
  uint32_t finalization_lag_micros;
  uint32_t prediction_horizon_micros;
  float max_prediction_distance_px;
  float tip_lock;
  float correction_easing;
  float minimum_prediction_speed_px_per_second;
  float corner_suppression;
} LayerInstantFeedbackSettings;

typedef struct LayerCanvasMetrics {
  uint64_t input_events;
  uint64_t frames;
  uint64_t committed_strokes;
  uint64_t stale_transform_fallbacks;
  uint64_t raster_dabs;
  uint64_t raster_candidate_pixels;
  uint64_t composited_pixels;
  uint64_t paint_pages;
  uint64_t preview_pages;
  uint64_t destination_companion_pages;
  uint64_t coverage_pages;
  uint64_t material_pages;
  uint64_t paint_storage_bytes;
  uint64_t preview_storage_bytes;
  uint64_t destination_storage_bytes;
  uint64_t paint_state_storage_bytes;
  uint64_t composite_storage_bytes;
  uint64_t feedback_frames;
  uint64_t platform_prediction_frames;
  uint64_t engine_prediction_frames;
  uint64_t preview_dabs;
  float last_tip_gap_surface_px;
  float maximum_tip_gap_surface_px;
  float last_endpoint_correction_surface_px;
  float maximum_endpoint_correction_surface_px;
} LayerCanvasMetrics;

typedef struct LayerGpuInfo {
  uint32_t backend;
  uint32_t device_type;
  uint32_t vendor_id;
  uint32_t device_id;
  uint8_t name_utf8[256];
} LayerGpuInfo;

LayerStatus layer_canvas_config_default(LayerCanvasConfig *output);
LayerStatus layer_canvas_instant_feedback_default(
    LayerInstantFeedbackSettings *output);
/* Blocking headless/diagnostic constructor, not an interactive window host.
 * Native frontends use layer-host and the staged WgpuRasterizer lifecycle. */
LayerStatus layer_canvas_create(const LayerCanvasConfig *config, LayerCanvas **output);
void layer_canvas_destroy(LayerCanvas *canvas);
LayerStatus layer_canvas_submit_pen_events(LayerCanvas *canvas,
                                           const LayerPenEvent *events,
                                           size_t event_count,
                                           size_t *accepted);
LayerStatus layer_canvas_draw_frame(LayerCanvas *canvas);
/* Preferred display callback entry point; timestamp_ns is monotonic. */
LayerStatus layer_canvas_draw_frame_at(LayerCanvas *canvas,
                                       uint64_t timestamp_ns);
LayerStatus layer_canvas_draw_frame_for(LayerCanvas *canvas,
                                        uint64_t timestamp_ns,
                                        uint64_t presentation_timestamp_ns);
LayerStatus layer_canvas_set_instant_feedback(
    LayerCanvas *canvas, const LayerInstantFeedbackSettings *settings);
LayerStatus layer_canvas_set_view(LayerCanvas *canvas, const LayerViewState *view);
LayerStatus layer_canvas_set_brush(LayerCanvas *canvas,
                                   const LayerBrushSettings *settings);
LayerStatus layer_canvas_set_pressure_curve(LayerCanvas *canvas, float dead_zone,
                                            float gamma, float ceiling);
LayerStatus layer_canvas_add_paint_layer(LayerCanvas *canvas,
                                         const uint8_t *name_utf8,
                                         size_t name_length,
                                         size_t front_to_back_index,
                                         uint64_t *output_layer_id);
LayerStatus layer_canvas_remove_layer(LayerCanvas *canvas, uint64_t layer_id);
LayerStatus layer_canvas_move_layer(LayerCanvas *canvas, uint64_t layer_id,
                                    size_t front_to_back_index);
LayerStatus layer_canvas_set_active_layer(LayerCanvas *canvas, uint64_t layer_id);
LayerStatus layer_canvas_set_layer_opacity(LayerCanvas *canvas, uint64_t layer_id,
                                           float opacity);
LayerStatus layer_canvas_set_layer_visibility(LayerCanvas *canvas,
                                              uint64_t layer_id,
                                              uint8_t visible);
LayerStatus layer_canvas_undo(LayerCanvas *canvas, uint8_t *changed);
LayerStatus layer_canvas_redo(LayerCanvas *canvas, uint8_t *changed);

/* Caller-owned straight sRGB RGBA8 export buffer. */
LayerStatus layer_canvas_copy_rgba8_srgb(LayerCanvas *canvas,
                                         uint8_t *destination,
                                         size_t destination_length,
                                         size_t stride);
/* Benchmark/export synchronization only; never call from a display callback. */
LayerStatus layer_canvas_wait_idle(LayerCanvas *canvas);
LayerStatus layer_canvas_get_gpu_info(const LayerCanvas *canvas,
                                      LayerGpuInfo *output);
LayerStatus layer_canvas_get_metrics(const LayerCanvas *canvas,
                                     LayerCanvasMetrics *output);

#ifdef __cplusplus
}
#endif

#endif
