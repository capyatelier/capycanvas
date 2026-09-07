//! Stable, renderer-specific C ABI used by platform canvas hosts.
//!
//! Foreign callers submit validated, fixed-size event histories in batches.
//! The ABI never exposes Rust enums, references, strings, or collections. Each
//! handle has one mutable owner; platform callbacks may instead feed an SPSC
//! producer owned by a small host shim when input arrives on another thread.

use layer_core::{
    AssetId, DefaultBrushPreset, Document, LayerId, Point, StrokeTool,
    WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET, WATERCOLOR_TRANSPORT_LONG_NARROW_ASSET,
    WATERCOLOR_TRANSPORT_SHORT_BROAD_ASSET, WATERCOLOR_TRANSPORT_SHORT_NARROW_ASSET, default_brush,
};
use layer_engine::{
    CanvasEngine, EngineCapacity, InputProducer, InstantFeedbackConfig, PenEvent, PenPhase,
    PressureCurve, SampleFlags, ToolKind, ViewTransform, input_queue,
};
use layer_render::ViewState;
use layer_render_wgpu::{GpuRasterMetrics, WgpuRasterizer};
use std::{panic::AssertUnwindSafe, ptr, slice, str};

const MAX_CANVAS_EXTENT: u32 = 32_768;
const MAX_CAPACITY: u32 = 1_048_576;
const KNOWN_SAMPLE_FLAGS: u32 = 0x0f;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum LayerStatus {
    Ok = 0,
    NullPointer = 1,
    InvalidArgument = 2,
    QueueFull = 3,
    BufferTooSmall = 4,
    DocumentError = 5,
    RenderError = 6,
    Panic = 255,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct LayerCanvasConfig {
    pub document_width: u32,
    pub document_height: u32,
    pub surface_width: u32,
    pub surface_height: u32,
    pub input_capacity: u32,
    pub stroke_point_capacity: u32,
    pub dab_capacity: u32,
    pub batch_capacity: u32,
    pub background_rgba_linear: [f32; 4],
}

impl Default for LayerCanvasConfig {
    fn default() -> Self {
        Self {
            document_width: 4096,
            document_height: 4096,
            surface_width: 4096,
            surface_height: 4096,
            input_capacity: 16_384,
            stroke_point_capacity: 65_536,
            dab_capacity: 65_536,
            batch_capacity: 64,
            background_rgba_linear: [1.0, 1.0, 1.0, 1.0],
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct LayerPenEvent {
    pub device_id: u64,
    pub sequence: u64,
    pub timestamp_ns: u64,
    pub view_revision: u64,
    pub x_physical_px: f32,
    pub y_physical_px: f32,
    pub pressure: f32,
    pub tilt_x_radians: f32,
    pub tilt_y_radians: f32,
    pub twist_radians: f32,
    pub distance: f32,
    pub phase: u32,
    pub tool: u32,
    pub flags: u32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct LayerViewState {
    pub revision: u64,
    pub surface_width: u32,
    pub surface_height: u32,
    pub document_to_surface: [f32; 6],
    pub surface_to_document: [f32; 6],
    pub background_rgba_linear: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct LayerBrushSettings {
    pub preset: u32,
    pub diameter_document_px: f32,
    pub opacity: f32,
    pub color_rgba_linear: [f32; 4],
    pub streamline: f32,
    pub pressure_smoothing: f32,
    pub stabilization: f32,
    pub motion_filtering: f32,
    pub stabilization_expression: f32,
    /// `0` keeps the preset field; `1..=4` select long-narrow, long-broad,
    /// short-narrow, and short-broad conductance fields.
    pub transport_field: u32,
    pub transport_scale: f32,
    pub transport_rotation_radians: f32,
    pub transport_contrast: f32,
    pub transport_wet_flow: f32,
    pub transport_dry_flow: f32,
    pub transport_distance_px: f32,
    pub transport_water_load: f32,
}

impl Default for LayerBrushSettings {
    fn default() -> Self {
        Self {
            preset: DefaultBrushPreset::GPen as u32,
            diameter_document_px: 12.0,
            opacity: 1.0,
            color_rgba_linear: [0.02, 0.02, 0.018, 1.0],
            streamline: 0.0,
            pressure_smoothing: 0.0,
            stabilization: 0.0,
            motion_filtering: 0.0,
            stabilization_expression: 1.0,
            transport_field: 0,
            transport_scale: 1.0,
            transport_rotation_radians: 0.0,
            transport_contrast: 0.8,
            transport_wet_flow: 0.3,
            transport_dry_flow: 0.08,
            transport_distance_px: 24.0,
            transport_water_load: 0.9,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct LayerInstantFeedbackSettings {
    pub enabled: u8,
    pub use_platform_prediction: u8,
    pub use_engine_prediction: u8,
    pub reserved: u8,
    pub finalization_lag_micros: u32,
    pub prediction_horizon_micros: u32,
    pub max_prediction_distance_px: f32,
    pub tip_lock: f32,
    pub correction_easing: f32,
    pub minimum_prediction_speed_px_per_second: f32,
    pub corner_suppression: f32,
}

impl Default for LayerInstantFeedbackSettings {
    fn default() -> Self {
        let config = InstantFeedbackConfig::default();
        Self {
            enabled: u8::from(config.enabled),
            use_platform_prediction: u8::from(config.use_platform_prediction),
            use_engine_prediction: u8::from(config.use_engine_prediction),
            reserved: 0,
            finalization_lag_micros: config.finalization_lag_micros,
            prediction_horizon_micros: config.prediction_horizon_micros,
            max_prediction_distance_px: config.max_prediction_distance_px,
            tip_lock: config.tip_lock,
            correction_easing: config.correction_easing,
            minimum_prediction_speed_px_per_second: config.minimum_prediction_speed_px_per_second,
            corner_suppression: config.corner_suppression,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct LayerCanvasMetrics {
    pub input_events: u64,
    pub frames: u64,
    pub committed_strokes: u64,
    pub stale_transform_fallbacks: u64,
    pub raster_dabs: u64,
    pub raster_candidate_pixels: u64,
    pub composited_pixels: u64,
    pub paint_pages: u64,
    pub preview_pages: u64,
    pub destination_companion_pages: u64,
    pub coverage_pages: u64,
    pub material_pages: u64,
    pub paint_storage_bytes: u64,
    pub preview_storage_bytes: u64,
    pub destination_storage_bytes: u64,
    pub paint_state_storage_bytes: u64,
    pub composite_storage_bytes: u64,
    pub feedback_frames: u64,
    pub platform_prediction_frames: u64,
    pub engine_prediction_frames: u64,
    pub preview_dabs: u64,
    pub last_tip_gap_surface_px: f32,
    pub maximum_tip_gap_surface_px: f32,
    pub last_endpoint_correction_surface_px: f32,
    pub maximum_endpoint_correction_surface_px: f32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct LayerGpuInfo {
    pub backend: u32,
    pub device_type: u32,
    pub vendor_id: u32,
    pub device_id: u32,
    pub name_utf8: [u8; 256],
}

impl Default for LayerGpuInfo {
    fn default() -> Self {
        Self {
            backend: 0,
            device_type: 0,
            vendor_id: 0,
            device_id: 0,
            name_utf8: [0; 256],
        }
    }
}

pub struct LayerCanvas {
    producer: InputProducer<PenEvent>,
    engine: CanvasEngine<WgpuRasterizer>,
}

/// # Safety
/// `output` must be writable and properly aligned for one configuration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_config_default(
    output: *mut LayerCanvasConfig,
) -> LayerStatus {
    ffi_boundary(|| {
        let output = unsafe { output.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        *output = LayerCanvasConfig::default();
        Ok(())
    })
}

/// # Safety
/// `output` must be writable and properly aligned for one configuration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_instant_feedback_default(
    output: *mut LayerInstantFeedbackSettings,
) -> LayerStatus {
    ffi_boundary(|| {
        let output = unsafe { output.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        *output = LayerInstantFeedbackSettings::default();
        Ok(())
    })
}

/// # Safety
/// `config` and `output` must point to readable/writable aligned values for
/// the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_create(
    config: *const LayerCanvasConfig,
    output: *mut *mut LayerCanvas,
) -> LayerStatus {
    ffi_boundary(|| {
        let config = *unsafe { config.as_ref() }.ok_or(LayerStatus::NullPointer)?;
        let output = unsafe { output.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        *output = ptr::null_mut();
        validate_config(config)?;

        let (producer, consumer) = input_queue(config.input_capacity as usize);
        let view = ViewState {
            width_px: config.surface_width,
            height_px: config.surface_height,
            document_to_surface: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            background_rgba_linear: config.background_rgba_linear,
        };
        let engine = CanvasEngine::with_capacity(
            WgpuRasterizer::new().map_err(|_| LayerStatus::RenderError)?,
            Document::new("untitled", config.document_width, config.document_height),
            consumer,
            view,
            ViewTransform::IDENTITY,
            EngineCapacity {
                stroke_points: config.stroke_point_capacity as usize,
                dabs_per_frame: config.dab_capacity as usize,
                batches_per_frame: config.batch_capacity as usize,
            },
        )
        .map_err(|_| LayerStatus::RenderError)?;
        *output = Box::into_raw(Box::new(LayerCanvas { producer, engine }));
        Ok(())
    })
}

/// # Safety
/// `canvas` must be null or a live handle returned by `layer_canvas_create`,
/// and it must be destroyed exactly once with no overlapping call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_destroy(canvas: *mut LayerCanvas) {
    if !canvas.is_null() {
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| unsafe {
            drop(Box::from_raw(canvas));
        }));
    }
}

/// Submit a complete coalesced event-history batch in chronological order.
/// `accepted` is always written. `QueueFull` means the caller retains the
/// unaccepted suffix and should retry it before submitting newer samples.
///
/// # Safety
/// `canvas` must be live and uniquely accessed, `accepted` writable, and
/// `events` readable for `event_count` aligned records when the count is nonzero.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_submit_pen_events(
    canvas: *mut LayerCanvas,
    events: *const LayerPenEvent,
    event_count: usize,
    accepted: *mut usize,
) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        let accepted = unsafe { accepted.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        *accepted = 0;
        if event_count == 0 {
            return Ok(());
        }
        if events.is_null() {
            return Err(LayerStatus::NullPointer);
        }
        let events = unsafe { slice::from_raw_parts(events, event_count) };
        for event in events {
            let event = decode_event(*event)?;
            if canvas.producer.push(event).is_err() {
                return Err(LayerStatus::QueueFull);
            }
            *accepted += 1;
        }
        Ok(())
    })
}

/// # Safety
/// `canvas` must be a live, uniquely accessed canvas handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_draw_frame(canvas: *mut LayerCanvas) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        canvas
            .engine
            .render_frame()
            .map_err(|_| LayerStatus::RenderError)
    })
}

/// Advances time-driven brushes and draws one frame using a platform-provided
/// monotonic timestamp.
///
/// # Safety
/// `canvas` must be a live, uniquely accessed canvas handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_draw_frame_at(
    canvas: *mut LayerCanvas,
    timestamp_ns: u64,
) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        canvas
            .engine
            .render_frame_at(timestamp_ns)
            .map_err(|_| LayerStatus::RenderError)
    })
}

/// Draws a frame using separate observation and expected-presentation clocks.
/// Both timestamps use the same monotonic nanosecond timebase as pen events.
///
/// # Safety
/// `canvas` must be a live, uniquely accessed canvas handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_draw_frame_for(
    canvas: *mut LayerCanvas,
    timestamp_ns: u64,
    presentation_timestamp_ns: u64,
) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        if presentation_timestamp_ns < timestamp_ns {
            return Err(LayerStatus::InvalidArgument);
        }
        canvas
            .engine
            .render_frame_for(timestamp_ns, presentation_timestamp_ns)
            .map_err(|_| LayerStatus::RenderError)
    })
}

/// Settings are snapshotted at pen-down; changing them never alters a stroke
/// already in progress.
///
/// # Safety
/// `canvas` must be live and uniquely accessed and `settings` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_set_instant_feedback(
    canvas: *mut LayerCanvas,
    settings: *const LayerInstantFeedbackSettings,
) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        let settings = *unsafe { settings.as_ref() }.ok_or(LayerStatus::NullPointer)?;
        let config = decode_feedback(settings)?;
        canvas
            .engine
            .set_instant_feedback(config)
            .map_err(|_| LayerStatus::InvalidArgument)
    })
}

/// # Safety
/// `canvas` must be live and uniquely accessed; `view` must be readable and aligned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_set_view(
    canvas: *mut LayerCanvas,
    view: *const LayerViewState,
) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        let view = *unsafe { view.as_ref() }.ok_or(LayerStatus::NullPointer)?;
        if view.surface_width == 0
            || view.surface_height == 0
            || view.surface_width > MAX_CANVAS_EXTENT
            || view.surface_height > MAX_CANVAS_EXTENT
            || view
                .document_to_surface
                .iter()
                .chain(view.surface_to_document.iter())
                .chain(view.background_rgba_linear.iter())
                .any(|value| !value.is_finite())
        {
            return Err(LayerStatus::InvalidArgument);
        }
        canvas
            .engine
            .resize_surface(view.surface_width, view.surface_height)
            .map_err(|_| LayerStatus::RenderError)?;
        canvas.engine.set_view(
            ViewState {
                width_px: view.surface_width,
                height_px: view.surface_height,
                document_to_surface: view.document_to_surface,
                background_rgba_linear: view.background_rgba_linear,
            },
            ViewTransform {
                revision: view.revision,
                surface_to_document: view.surface_to_document,
            },
        );
        Ok(())
    })
}

/// # Safety
/// `canvas` must be live and uniquely accessed; `settings` must be readable and aligned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_set_brush(
    canvas: *mut LayerCanvas,
    settings: *const LayerBrushSettings,
) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        let settings = *unsafe { settings.as_ref() }.ok_or(LayerStatus::NullPointer)?;
        let preset = decode_preset(settings.preset)?;
        let mut brush = default_brush(preset);
        brush.diameter = settings.diameter_document_px;
        brush.opacity = settings.opacity;
        brush.color_rgba_linear = settings.color_rgba_linear;
        brush.stabilization.streamline = settings.streamline;
        brush.stabilization.pressure_smoothing = settings.pressure_smoothing;
        brush.stabilization.stabilization = settings.stabilization;
        brush.stabilization.motion_filtering = settings.motion_filtering;
        brush.stabilization.expression = settings.stabilization_expression;
        if settings.transport_field != 0 {
            let conductance = match settings.transport_field {
                1 => WATERCOLOR_TRANSPORT_LONG_NARROW_ASSET,
                2 => WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET,
                3 => WATERCOLOR_TRANSPORT_SHORT_NARROW_ASSET,
                4 => WATERCOLOR_TRANSPORT_SHORT_BROAD_ASSET,
                _ => return Err(LayerStatus::InvalidArgument),
            };
            let Some(transport) = brush.transport.as_mut() else {
                return Err(LayerStatus::InvalidArgument);
            };
            transport.conductance = AssetId::from(conductance);
            transport.scale = settings.transport_scale;
            transport.rotation_radians = settings.transport_rotation_radians;
            transport.contrast = settings.transport_contrast;
            transport.wet_flow = settings.transport_wet_flow;
            transport.dry_flow = settings.transport_dry_flow;
            transport.distance = settings.transport_distance_px;
            transport.water_load = settings.transport_water_load;
        }
        canvas
            .engine
            .set_brush(brush)
            .map_err(|_| LayerStatus::InvalidArgument)?;
        canvas
            .engine
            .set_tool(if preset == DefaultBrushPreset::Eraser {
                StrokeTool::Eraser
            } else {
                StrokeTool::Brush
            });
        Ok(())
    })
}

/// # Safety
/// `canvas` must be a live, uniquely accessed canvas handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_set_pressure_curve(
    canvas: *mut LayerCanvas,
    dead_zone: f32,
    gamma: f32,
    ceiling: f32,
) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        if [dead_zone, gamma, ceiling]
            .iter()
            .any(|value| !value.is_finite())
            || dead_zone < 0.0
            || ceiling > 1.0
            || dead_zone >= ceiling
            || gamma <= 0.0
        {
            return Err(LayerStatus::InvalidArgument);
        }
        canvas.engine.set_pressure_curve(PressureCurve {
            dead_zone,
            gamma,
            ceiling,
        });
        Ok(())
    })
}

/// # Safety
/// `canvas` must be live and unique, `output_layer_id` writable, and
/// `name_utf8` readable for `name_length` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_add_paint_layer(
    canvas: *mut LayerCanvas,
    name_utf8: *const u8,
    name_length: usize,
    front_to_back_index: usize,
    output_layer_id: *mut u64,
) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        let output = unsafe { output_layer_id.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        if name_length == 0 || name_utf8.is_null() {
            return Err(LayerStatus::InvalidArgument);
        }
        let bytes = unsafe { slice::from_raw_parts(name_utf8, name_length) };
        let name = str::from_utf8(bytes).map_err(|_| LayerStatus::InvalidArgument)?;
        let id = canvas
            .engine
            .create_paint_layer(name, front_to_back_index)
            .map_err(|_| LayerStatus::DocumentError)?;
        *output = id.0;
        Ok(())
    })
}

/// # Safety
/// `canvas` must be a live, uniquely accessed canvas handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_remove_layer(
    canvas: *mut LayerCanvas,
    layer_id: u64,
) -> LayerStatus {
    unsafe {
        document_call(canvas, |canvas| {
            canvas.engine.remove_layer(LayerId(layer_id))
        })
    }
}

/// # Safety
/// `canvas` must be a live, uniquely accessed canvas handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_move_layer(
    canvas: *mut LayerCanvas,
    layer_id: u64,
    front_to_back_index: usize,
) -> LayerStatus {
    unsafe {
        document_call(canvas, |canvas| {
            canvas
                .engine
                .move_layer(LayerId(layer_id), front_to_back_index)
        })
    }
}

/// # Safety
/// `canvas` must be a live, uniquely accessed canvas handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_set_active_layer(
    canvas: *mut LayerCanvas,
    layer_id: u64,
) -> LayerStatus {
    unsafe {
        document_call(canvas, |canvas| {
            canvas.engine.set_active_layer(LayerId(layer_id))
        })
    }
}

/// # Safety
/// `canvas` must be a live, uniquely accessed canvas handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_set_layer_opacity(
    canvas: *mut LayerCanvas,
    layer_id: u64,
    opacity: f32,
) -> LayerStatus {
    if !opacity.is_finite() {
        return LayerStatus::InvalidArgument;
    }
    unsafe {
        document_call(canvas, |canvas| {
            canvas.engine.set_layer_opacity(LayerId(layer_id), opacity)
        })
    }
}

/// # Safety
/// `canvas` must be a live, uniquely accessed canvas handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_set_layer_visibility(
    canvas: *mut LayerCanvas,
    layer_id: u64,
    visible: u8,
) -> LayerStatus {
    if visible > 1 {
        return LayerStatus::InvalidArgument;
    }
    unsafe {
        document_call(canvas, |canvas| {
            canvas
                .engine
                .set_layer_visibility(LayerId(layer_id), visible != 0)
        })
    }
}

/// # Safety
/// `canvas` must be live and unique; `changed` must be writable and aligned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_undo(
    canvas: *mut LayerCanvas,
    changed: *mut u8,
) -> LayerStatus {
    unsafe { history_call(canvas, changed, |engine| engine.undo()) }
}

/// # Safety
/// `canvas` must be live and unique; `changed` must be writable and aligned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_redo(
    canvas: *mut LayerCanvas,
    changed: *mut u8,
) -> LayerStatus {
    unsafe { history_call(canvas, changed, |engine| engine.redo()) }
}

/// # Safety
/// `canvas` must be live and unique; `destination` must be writable for
/// `destination_length` bytes without overlapping the canvas allocation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_copy_rgba8_srgb(
    canvas: *mut LayerCanvas,
    destination: *mut u8,
    destination_length: usize,
    stride: usize,
) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        let [width, height] = canvas.engine.backend().document_extent();
        let required = stride
            .checked_mul(height as usize)
            .ok_or(LayerStatus::InvalidArgument)?;
        if stride < width as usize * 4 || destination_length < required {
            return Err(LayerStatus::BufferTooSmall);
        }
        if required != 0 && destination.is_null() {
            return Err(LayerStatus::NullPointer);
        }
        let destination = unsafe { slice::from_raw_parts_mut(destination, destination_length) };
        canvas
            .engine
            .backend_mut()
            .copy_rgba8_srgb(destination, stride)
            .map_err(|_| LayerStatus::RenderError)
    })
}

/// Waits for the most recently submitted GPU frame. This exists for tests,
/// export, and completed-work benchmarks; display callbacks must not call it.
///
/// # Safety
/// `canvas` must be live and uniquely accessed for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_wait_idle(canvas: *mut LayerCanvas) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        canvas
            .engine
            .backend_mut()
            .wait_idle()
            .map_err(|_| LayerStatus::RenderError)
    })
}

/// # Safety
/// `canvas` must be live and shared; `output` must be writable and aligned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_get_gpu_info(
    canvas: *const LayerCanvas,
    output: *mut LayerGpuInfo,
) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_ref() }.ok_or(LayerStatus::NullPointer)?;
        let output = unsafe { output.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        let info = canvas.engine.backend().adapter_info();
        let mut name_utf8 = [0_u8; 256];
        let count = info.name.len().min(name_utf8.len() - 1);
        name_utf8[..count].copy_from_slice(&info.name.as_bytes()[..count]);
        *output = LayerGpuInfo {
            backend: info.backend,
            device_type: info.device_type,
            vendor_id: info.vendor_id,
            device_id: info.device_id,
            name_utf8,
        };
        Ok(())
    })
}

/// # Safety
/// `canvas` must be a live shared handle and `output` writable and aligned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn layer_canvas_get_metrics(
    canvas: *const LayerCanvas,
    output: *mut LayerCanvasMetrics,
) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_ref() }.ok_or(LayerStatus::NullPointer)?;
        let output = unsafe { output.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        let engine = canvas.engine.metrics();
        let raster = canvas.engine.backend().metrics();
        *output = merge_metrics(engine, raster);
        Ok(())
    })
}

fn merge_metrics(
    engine: layer_engine::EngineMetrics,
    raster: GpuRasterMetrics,
) -> LayerCanvasMetrics {
    LayerCanvasMetrics {
        input_events: engine.input_events,
        frames: engine.frames,
        committed_strokes: engine.committed_strokes,
        stale_transform_fallbacks: engine.stale_transform_fallbacks,
        raster_dabs: raster.dabs,
        raster_candidate_pixels: raster.raster_candidate_pixels,
        composited_pixels: raster.composited_pixels,
        paint_pages: raster.paint_pages,
        preview_pages: raster.preview_pages,
        destination_companion_pages: raster.destination_companion_pages,
        coverage_pages: raster.coverage_pages,
        material_pages: raster.material_pages,
        paint_storage_bytes: raster.paint_storage_bytes,
        preview_storage_bytes: raster.preview_storage_bytes,
        destination_storage_bytes: raster.destination_storage_bytes,
        paint_state_storage_bytes: raster.paint_state_storage_bytes,
        composite_storage_bytes: raster.composite_storage_bytes,
        feedback_frames: engine.feedback_frames,
        platform_prediction_frames: engine.platform_prediction_frames,
        engine_prediction_frames: engine.engine_prediction_frames,
        preview_dabs: engine.preview_dabs,
        last_tip_gap_surface_px: engine.last_tip_gap_surface_px,
        maximum_tip_gap_surface_px: engine.maximum_tip_gap_surface_px,
        last_endpoint_correction_surface_px: engine.last_endpoint_correction_surface_px,
        maximum_endpoint_correction_surface_px: engine.maximum_endpoint_correction_surface_px,
    }
}

fn decode_feedback(
    settings: LayerInstantFeedbackSettings,
) -> Result<InstantFeedbackConfig, LayerStatus> {
    if settings.enabled > 1
        || settings.use_platform_prediction > 1
        || settings.use_engine_prediction > 1
        || settings.reserved != 0
    {
        return Err(LayerStatus::InvalidArgument);
    }
    let config = InstantFeedbackConfig {
        enabled: settings.enabled != 0,
        use_platform_prediction: settings.use_platform_prediction != 0,
        use_engine_prediction: settings.use_engine_prediction != 0,
        finalization_lag_micros: settings.finalization_lag_micros,
        prediction_horizon_micros: settings.prediction_horizon_micros,
        max_prediction_distance_px: settings.max_prediction_distance_px,
        tip_lock: settings.tip_lock,
        correction_easing: settings.correction_easing,
        minimum_prediction_speed_px_per_second: settings.minimum_prediction_speed_px_per_second,
        corner_suppression: settings.corner_suppression,
    };
    config
        .validate()
        .map_err(|_| LayerStatus::InvalidArgument)?;
    Ok(config)
}

fn validate_config(config: LayerCanvasConfig) -> Result<(), LayerStatus> {
    let extents = [
        config.document_width,
        config.document_height,
        config.surface_width,
        config.surface_height,
    ];
    let capacities = [
        config.input_capacity,
        config.stroke_point_capacity,
        config.dab_capacity,
        config.batch_capacity,
    ];
    if extents
        .iter()
        .any(|value| *value == 0 || *value > MAX_CANVAS_EXTENT)
        || capacities
            .iter()
            .any(|value| *value < 2 || *value > MAX_CAPACITY)
        || config
            .background_rgba_linear
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
    {
        return Err(LayerStatus::InvalidArgument);
    }
    Ok(())
}

fn decode_event(event: LayerPenEvent) -> Result<PenEvent, LayerStatus> {
    let phase = match event.phase {
        0 => PenPhase::Hover,
        1 => PenPhase::Down,
        2 => PenPhase::Move,
        3 => PenPhase::Up,
        4 => PenPhase::Cancel,
        _ => return Err(LayerStatus::InvalidArgument),
    };
    let tool = match event.tool {
        0 => ToolKind::Unknown,
        1 => ToolKind::Pen,
        2 => ToolKind::Eraser,
        3 => ToolKind::Brush,
        4 => ToolKind::Pencil,
        5 => ToolKind::Airbrush,
        6 => ToolKind::Finger,
        7 => ToolKind::Mouse,
        _ => return Err(LayerStatus::InvalidArgument),
    };
    if event.flags & !KNOWN_SAMPLE_FLAGS != 0
        || [
            event.x_physical_px,
            event.y_physical_px,
            event.pressure,
            event.tilt_x_radians,
            event.tilt_y_radians,
            event.twist_radians,
            event.distance,
        ]
        .iter()
        .any(|value| !value.is_finite())
        || !(0.0..=1.0).contains(&event.pressure)
    {
        return Err(LayerStatus::InvalidArgument);
    }
    Ok(PenEvent {
        device_id: event.device_id,
        sequence: event.sequence,
        timestamp_ns: event.timestamp_ns,
        view_revision: event.view_revision,
        surface_position: Point {
            x: event.x_physical_px,
            y: event.y_physical_px,
        },
        pressure: event.pressure,
        tilt_radians: [event.tilt_x_radians, event.tilt_y_radians],
        twist_radians: event.twist_radians,
        distance: event.distance,
        phase,
        tool,
        flags: SampleFlags(event.flags as u16),
    })
}

fn decode_preset(value: u32) -> Result<DefaultBrushPreset, LayerStatus> {
    match value {
        1 => Ok(DefaultBrushPreset::GPen),
        2 => Ok(DefaultBrushPreset::Pencil),
        3 => Ok(DefaultBrushPreset::Eraser),
        4 => Ok(DefaultBrushPreset::Paintbrush),
        5 => Ok(DefaultBrushPreset::Airbrush),
        6 => Ok(DefaultBrushPreset::Chalk),
        7 => Ok(DefaultBrushPreset::Marker),
        8 => Ok(DefaultBrushPreset::Spray),
        9 => Ok(DefaultBrushPreset::DualTexture),
        10 => Ok(DefaultBrushPreset::Smudge),
        11 => Ok(DefaultBrushPreset::WetRound),
        12 => Ok(DefaultBrushPreset::LiquifyPush),
        13 => Ok(DefaultBrushPreset::LiquifyTwirl),
        14 => Ok(DefaultBrushPreset::MultiplyGlaze),
        15 => Ok(DefaultBrushPreset::TexturedFlat),
        16 => Ok(DefaultBrushPreset::DryScumble),
        17 => Ok(DefaultBrushPreset::PastelBlock),
        18 => Ok(DefaultBrushPreset::TransparentGlaze),
        19 => Ok(DefaultBrushPreset::OpaqueGouache),
        20 => Ok(DefaultBrushPreset::WatercolorWash),
        21 => Ok(DefaultBrushPreset::WetWatercolor),
        22 => Ok(DefaultBrushPreset::LoadedOil),
        23 => Ok(DefaultBrushPreset::PaletteKnife),
        24 => Ok(DefaultBrushPreset::NaturalBlender),
        _ => Err(LayerStatus::InvalidArgument),
    }
}

unsafe fn document_call(
    canvas: *mut LayerCanvas,
    operation: impl FnOnce(&mut LayerCanvas) -> Result<(), layer_core::DocumentError>,
) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        operation(canvas).map_err(|_| LayerStatus::DocumentError)
    })
}

unsafe fn history_call(
    canvas: *mut LayerCanvas,
    changed: *mut u8,
    operation: impl FnOnce(&mut CanvasEngine<WgpuRasterizer>) -> Result<bool, layer_core::DocumentError>,
) -> LayerStatus {
    ffi_boundary(|| {
        let canvas = unsafe { canvas.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        let changed = unsafe { changed.as_mut() }.ok_or(LayerStatus::NullPointer)?;
        *changed = operation(&mut canvas.engine).map_err(|_| LayerStatus::DocumentError)? as u8;
        Ok(())
    })
}

fn ffi_boundary(operation: impl FnOnce() -> Result<(), LayerStatus>) -> LayerStatus {
    match std::panic::catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(())) => LayerStatus::Ok,
        Ok(Err(status)) => status,
        Err(_) => LayerStatus::Panic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Canvas(*mut LayerCanvas);

    impl Canvas {
        fn new(width: u32, height: u32) -> Self {
            let config = LayerCanvasConfig {
                document_width: width,
                document_height: height,
                surface_width: width,
                surface_height: height,
                ..LayerCanvasConfig::default()
            };
            let mut canvas = ptr::null_mut();
            assert_eq!(
                unsafe { layer_canvas_create(&config, &mut canvas) },
                LayerStatus::Ok
            );
            Self(canvas)
        }
    }

    impl Drop for Canvas {
        fn drop(&mut self) {
            unsafe { layer_canvas_destroy(self.0) };
        }
    }

    fn event(sequence: u64, phase: u32, x: f32, y: f32) -> LayerPenEvent {
        LayerPenEvent {
            device_id: 1,
            sequence,
            timestamp_ns: sequence * 1_000_000,
            view_revision: 0,
            x_physical_px: x,
            y_physical_px: y,
            pressure: 0.8,
            phase,
            tool: 1,
            ..LayerPenEvent::default()
        }
    }

    #[test]
    fn batch_input_reaches_gpu_canvas_through_only_the_abi() {
        let canvas = Canvas::new(256, 256);
        let settings = LayerBrushSettings {
            preset: 1,
            diameter_document_px: 16.0,
            opacity: 0.75,
            color_rgba_linear: [0.0, 0.0, 0.0, 1.0],
            ..LayerBrushSettings::default()
        };
        assert_eq!(
            unsafe { layer_canvas_set_brush(canvas.0, &settings) },
            LayerStatus::Ok
        );
        let events = [
            event(1, 1, 20.0, 20.0),
            event(2, 2, 128.0, 128.0),
            event(3, 3, 220.0, 220.0),
        ];
        let mut accepted = 0;
        assert_eq!(
            unsafe {
                layer_canvas_submit_pen_events(
                    canvas.0,
                    events.as_ptr(),
                    events.len(),
                    &mut accepted,
                )
            },
            LayerStatus::Ok
        );
        assert_eq!(accepted, events.len());
        assert_eq!(
            unsafe { layer_canvas_draw_frame(canvas.0) },
            LayerStatus::Ok
        );
        let mut metrics = LayerCanvasMetrics::default();
        assert_eq!(
            unsafe { layer_canvas_get_metrics(canvas.0, &mut metrics) },
            LayerStatus::Ok
        );
        assert_eq!(metrics.input_events, 3);
        assert_eq!(metrics.committed_strokes, 1);
        assert!(metrics.raster_dabs > 3);
    }

    #[test]
    fn invalid_foreign_discriminants_are_rejected_before_rust_enums() {
        let canvas = Canvas::new(32, 32);
        let invalid = event(1, 99, 1.0, 1.0);
        let mut accepted = usize::MAX;
        assert_eq!(
            unsafe { layer_canvas_submit_pen_events(canvas.0, &invalid, 1, &mut accepted) },
            LayerStatus::InvalidArgument
        );
        assert_eq!(accepted, 0);
    }

    #[test]
    fn painter_preset_ids_are_part_of_the_validated_abi() {
        for preset in 15..=24 {
            assert!(decode_preset(preset).is_ok(), "preset {preset}");
        }
        assert_eq!(decode_preset(25), Err(LayerStatus::InvalidArgument));
    }

    #[test]
    fn feedback_configuration_is_validated_at_the_abi() {
        let mut settings = LayerInstantFeedbackSettings::default();
        assert!(decode_feedback(settings).is_ok());
        settings.reserved = 1;
        assert_eq!(decode_feedback(settings), Err(LayerStatus::InvalidArgument));
        settings = LayerInstantFeedbackSettings::default();
        settings.prediction_horizon_micros = 50_001;
        assert_eq!(decode_feedback(settings), Err(LayerStatus::InvalidArgument));
    }

    #[test]
    fn timed_frame_advances_stationary_airbrush() {
        let canvas = Canvas::new(128, 128);
        let settings = LayerBrushSettings {
            preset: 5,
            diameter_document_px: 40.0,
            opacity: 1.0,
            color_rgba_linear: [0.0, 0.1, 0.8, 1.0],
            ..LayerBrushSettings::default()
        };
        assert_eq!(
            unsafe { layer_canvas_set_brush(canvas.0, &settings) },
            LayerStatus::Ok
        );
        let mut down = event(1, 1, 64.0, 64.0);
        down.timestamp_ns = 1_000_000_000;
        let mut accepted = 0;
        assert_eq!(
            unsafe { layer_canvas_submit_pen_events(canvas.0, &down, 1, &mut accepted) },
            LayerStatus::Ok
        );
        assert_eq!(
            unsafe { layer_canvas_draw_frame_at(canvas.0, down.timestamp_ns) },
            LayerStatus::Ok
        );
        let mut before = LayerCanvasMetrics::default();
        assert_eq!(
            unsafe { layer_canvas_get_metrics(canvas.0, &mut before) },
            LayerStatus::Ok
        );
        assert_eq!(
            unsafe { layer_canvas_draw_frame_at(canvas.0, down.timestamp_ns + 100_000_000) },
            LayerStatus::Ok
        );
        let mut after = LayerCanvasMetrics::default();
        assert_eq!(
            unsafe { layer_canvas_get_metrics(canvas.0, &mut after) },
            LayerStatus::Ok
        );
        assert!(after.raster_dabs >= before.raster_dabs + 6);
    }

    #[test]
    fn layers_and_eraser_work_through_abi() {
        let canvas = Canvas::new(128, 128);
        let name = b"Color";
        let mut layer_id = 0;
        assert_eq!(
            unsafe {
                layer_canvas_add_paint_layer(canvas.0, name.as_ptr(), name.len(), 0, &mut layer_id)
            },
            LayerStatus::Ok
        );
        assert_eq!(
            unsafe { layer_canvas_set_active_layer(canvas.0, layer_id) },
            LayerStatus::Ok
        );
        assert_eq!(
            unsafe { layer_canvas_set_layer_opacity(canvas.0, layer_id, 0.5) },
            LayerStatus::Ok
        );
        assert_eq!(
            unsafe { layer_canvas_draw_frame(canvas.0) },
            LayerStatus::Ok
        );
    }

    #[test]
    fn explicit_readback_observes_gpu_ink() {
        let canvas = Canvas::new(512, 512);
        let settings = LayerBrushSettings {
            preset: 1,
            diameter_document_px: 20.0,
            opacity: 1.0,
            color_rgba_linear: [0.0, 0.0, 0.0, 1.0],
            ..LayerBrushSettings::default()
        };
        assert_eq!(
            unsafe { layer_canvas_set_brush(canvas.0, &settings) },
            LayerStatus::Ok
        );
        let down = event(1, 1, 100.0, 100.0);
        let mut accepted = 0;
        assert_eq!(
            unsafe { layer_canvas_submit_pen_events(canvas.0, &down, 1, &mut accepted) },
            LayerStatus::Ok
        );
        assert_eq!(
            unsafe { layer_canvas_draw_frame(canvas.0) },
            LayerStatus::Ok
        );

        let movement = event(2, 3, 112.0, 108.0);
        assert_eq!(
            unsafe { layer_canvas_submit_pen_events(canvas.0, &movement, 1, &mut accepted) },
            LayerStatus::Ok
        );
        assert_eq!(
            unsafe { layer_canvas_draw_frame(canvas.0) },
            LayerStatus::Ok
        );
        assert_eq!(unsafe { layer_canvas_wait_idle(canvas.0) }, LayerStatus::Ok);
        let mut pixels = vec![0_u8; 512 * 512 * 4];
        assert_eq!(
            unsafe {
                layer_canvas_copy_rgba8_srgb(canvas.0, pixels.as_mut_ptr(), pixels.len(), 512 * 4)
            },
            LayerStatus::Ok
        );
        let center = (104 * 512 + 106) * 4;
        assert!(pixels[center] < 128, "stroke pixel should be dark");
    }

    #[test]
    fn predicted_eraser_previews_against_gpu_layer_pixels() {
        let canvas = Canvas::new(256, 256);
        let paint = LayerBrushSettings {
            preset: 1,
            diameter_document_px: 64.0,
            opacity: 1.0,
            color_rgba_linear: [0.0, 0.0, 0.0, 1.0],
            ..LayerBrushSettings::default()
        };
        assert_eq!(
            unsafe { layer_canvas_set_brush(canvas.0, &paint) },
            LayerStatus::Ok
        );

        let painted = [event(1, 1, 64.0, 128.0), event(2, 3, 192.0, 128.0)];
        let mut accepted = 0;
        assert_eq!(
            unsafe {
                layer_canvas_submit_pen_events(
                    canvas.0,
                    painted.as_ptr(),
                    painted.len(),
                    &mut accepted,
                )
            },
            LayerStatus::Ok
        );
        assert_eq!(
            unsafe { layer_canvas_draw_frame(canvas.0) },
            LayerStatus::Ok
        );

        let eraser = LayerBrushSettings {
            preset: 3,
            diameter_document_px: 48.0,
            opacity: 1.0,
            color_rgba_linear: [0.0, 0.0, 0.0, 1.0],
            ..LayerBrushSettings::default()
        };
        assert_eq!(
            unsafe { layer_canvas_set_brush(canvas.0, &eraser) },
            LayerStatus::Ok
        );
        let mut down = event(3, 1, 32.0, 128.0);
        down.tool = 2;
        let mut predicted = event(4, 2, 128.0, 128.0);
        predicted.tool = 2;
        predicted.flags = SampleFlags::PREDICTED.0 as u32;
        let preview = [down, predicted];
        assert_eq!(
            unsafe {
                layer_canvas_submit_pen_events(
                    canvas.0,
                    preview.as_ptr(),
                    preview.len(),
                    &mut accepted,
                )
            },
            LayerStatus::Ok
        );
        assert_eq!(
            unsafe { layer_canvas_draw_frame(canvas.0) },
            LayerStatus::Ok
        );

        let mut metrics = LayerCanvasMetrics::default();
        assert_eq!(
            unsafe { layer_canvas_get_metrics(canvas.0, &mut metrics) },
            LayerStatus::Ok
        );
        assert_eq!(metrics.platform_prediction_frames, 1);
        assert!(metrics.maximum_tip_gap_surface_px < 0.001);

        let mut pixels = vec![0_u8; 256 * 256 * 4];
        assert_eq!(
            unsafe {
                layer_canvas_copy_rgba8_srgb(canvas.0, pixels.as_mut_ptr(), pixels.len(), 256 * 4)
            },
            LayerStatus::Ok
        );
        let predicted_center = (128 * 256 + 128) * 4;
        assert!(
            pixels[predicted_center] > 240,
            "predicted eraser should reveal the white background"
        );
    }

    #[test]
    fn predicted_dry_paint_draws_directly_without_preview_pages() {
        let canvas = Canvas::new(128, 128);
        let paint = LayerBrushSettings {
            preset: 1,
            diameter_document_px: 18.0,
            opacity: 1.0,
            color_rgba_linear: [0.0, 0.0, 0.0, 1.0],
            streamline: 0.7,
            ..LayerBrushSettings::default()
        };
        assert_eq!(
            unsafe { layer_canvas_set_brush(canvas.0, &paint) },
            LayerStatus::Ok
        );
        let down = event(1, 1, 20.0, 64.0);
        let mut predicted = event(2, 2, 100.0, 64.0);
        predicted.flags = SampleFlags::PREDICTED.0 as u32;
        let samples = [down, predicted];
        let mut accepted = 0;
        assert_eq!(
            unsafe {
                layer_canvas_submit_pen_events(
                    canvas.0,
                    samples.as_ptr(),
                    samples.len(),
                    &mut accepted,
                )
            },
            LayerStatus::Ok
        );
        assert_eq!(
            unsafe { layer_canvas_draw_frame_for(canvas.0, 1_000_000, 2_000_000) },
            LayerStatus::Ok
        );
        let mut metrics = LayerCanvasMetrics::default();
        assert_eq!(
            unsafe { layer_canvas_get_metrics(canvas.0, &mut metrics) },
            LayerStatus::Ok
        );
        assert_eq!(metrics.preview_pages, 0);
        assert_eq!(metrics.platform_prediction_frames, 1);

        let mut pixels = vec![0_u8; 128 * 128 * 4];
        assert_eq!(
            unsafe {
                layer_canvas_copy_rgba8_srgb(canvas.0, pixels.as_mut_ptr(), pixels.len(), 128 * 4)
            },
            LayerStatus::Ok
        );
        let predicted_center = (64 * 128 + 100) * 4;
        assert!(pixels[predicted_center] < 64);

        let cancel = event(3, 4, 20.0, 64.0);
        assert_eq!(
            unsafe { layer_canvas_submit_pen_events(canvas.0, &cancel, 1, &mut accepted) },
            LayerStatus::Ok
        );
        assert_eq!(
            unsafe { layer_canvas_draw_frame(canvas.0) },
            LayerStatus::Ok
        );
        assert_eq!(
            unsafe {
                layer_canvas_copy_rgba8_srgb(canvas.0, pixels.as_mut_ptr(), pixels.len(), 128 * 4)
            },
            LayerStatus::Ok
        );
        assert!(pixels[predicted_center] > 240);
    }

    #[test]
    fn every_destination_brush_predicts_on_a_private_gpu_branch() {
        for preset in [10_u32, 11, 12, 13, 14] {
            let canvas = Canvas::new(128, 128);
            let base = LayerBrushSettings {
                preset: 1,
                diameter_document_px: 44.0,
                opacity: 1.0,
                color_rgba_linear: [0.7, 0.02, 0.01, 1.0],
                ..LayerBrushSettings::default()
            };
            assert_eq!(
                unsafe { layer_canvas_set_brush(canvas.0, &base) },
                LayerStatus::Ok
            );
            let base_samples = [
                event(1, 1, 24.0, 64.0),
                event(2, 3, 104.0, 64.0),
                event(3, 1, 24.0, 118.0),
                event(4, 3, 104.0, 118.0),
            ];
            let mut accepted = 0;
            assert_eq!(
                unsafe {
                    layer_canvas_submit_pen_events(
                        canvas.0,
                        base_samples.as_ptr(),
                        base_samples.len(),
                        &mut accepted,
                    )
                },
                LayerStatus::Ok
            );
            assert_eq!(
                unsafe { layer_canvas_draw_frame(canvas.0) },
                LayerStatus::Ok
            );
            let mut baseline = vec![0_u8; 128 * 128 * 4];
            assert_eq!(
                unsafe {
                    layer_canvas_copy_rgba8_srgb(
                        canvas.0,
                        baseline.as_mut_ptr(),
                        baseline.len(),
                        128 * 4,
                    )
                },
                LayerStatus::Ok
            );

            let advanced = LayerBrushSettings {
                preset,
                diameter_document_px: 68.0,
                opacity: 1.0,
                color_rgba_linear: [0.01, 0.08, 0.8, 1.0],
                streamline: 0.62,
                ..LayerBrushSettings::default()
            };
            assert_eq!(
                unsafe { layer_canvas_set_brush(canvas.0, &advanced) },
                LayerStatus::Ok
            );
            let down = event(5, 1, 32.0, 64.0);
            let mut predicted = event(6, 2, 96.0, 64.0);
            predicted.flags = SampleFlags::PREDICTED.0 as u32;
            let prediction = [down, predicted];
            assert_eq!(
                unsafe {
                    layer_canvas_submit_pen_events(
                        canvas.0,
                        prediction.as_ptr(),
                        prediction.len(),
                        &mut accepted,
                    )
                },
                LayerStatus::Ok
            );
            assert_eq!(
                unsafe { layer_canvas_draw_frame_for(canvas.0, 5_000_000, 6_000_000) },
                LayerStatus::Ok
            );
            let mut speculative = vec![0_u8; baseline.len()];
            assert_eq!(
                unsafe {
                    layer_canvas_copy_rgba8_srgb(
                        canvas.0,
                        speculative.as_mut_ptr(),
                        speculative.len(),
                        128 * 4,
                    )
                },
                LayerStatus::Ok
            );
            let changed = baseline
                .iter()
                .zip(&speculative)
                .filter(|(before, after)| before != after)
                .count();
            assert!(
                changed > 32,
                "preset {preset} produced no visible prediction"
            );
            let outside_preview = (118 * 128 + 64) * 4;
            assert_eq!(
                &speculative[outside_preview..outside_preview + 4],
                &baseline[outside_preview..outside_preview + 4],
                "preset {preset} hid committed pixels outside preview damage"
            );
            let mut metrics = LayerCanvasMetrics::default();
            assert_eq!(
                unsafe { layer_canvas_get_metrics(canvas.0, &mut metrics) },
                LayerStatus::Ok
            );
            assert_eq!(metrics.platform_prediction_frames, 1);
            assert!(metrics.preview_pages > 0);
            assert!(metrics.destination_companion_pages > 0);

            let cancel = event(7, 4, 32.0, 64.0);
            assert_eq!(
                unsafe { layer_canvas_submit_pen_events(canvas.0, &cancel, 1, &mut accepted) },
                LayerStatus::Ok
            );
            assert_eq!(
                unsafe { layer_canvas_draw_frame(canvas.0) },
                LayerStatus::Ok
            );
            let mut restored = vec![0_u8; baseline.len()];
            assert_eq!(
                unsafe {
                    layer_canvas_copy_rgba8_srgb(
                        canvas.0,
                        restored.as_mut_ptr(),
                        restored.len(),
                        128 * 4,
                    )
                },
                LayerStatus::Ok
            );
            assert_eq!(restored, baseline, "preset {preset} leaked preview state");
        }
    }

    #[test]
    fn explicit_export_is_gpu_encoded_straight_srgb() {
        let config = LayerCanvasConfig {
            document_width: 64,
            document_height: 64,
            surface_width: 64,
            surface_height: 64,
            background_rgba_linear: [0.0, 0.0, 0.0, 0.0],
            ..LayerCanvasConfig::default()
        };
        let mut raw = ptr::null_mut();
        assert_eq!(
            unsafe { layer_canvas_create(&config, &mut raw) },
            LayerStatus::Ok
        );
        let canvas = Canvas(raw);
        let paint = LayerBrushSettings {
            preset: 1,
            diameter_document_px: 32.0,
            opacity: 0.5,
            color_rgba_linear: [0.25, 0.0, 0.0, 1.0],
            ..LayerBrushSettings::default()
        };
        assert_eq!(
            unsafe { layer_canvas_set_brush(canvas.0, &paint) },
            LayerStatus::Ok
        );
        let samples = [event(1, 1, 32.0, 32.0), event(2, 3, 32.0, 32.0)];
        let mut accepted = 0;
        assert_eq!(
            unsafe {
                layer_canvas_submit_pen_events(
                    canvas.0,
                    samples.as_ptr(),
                    samples.len(),
                    &mut accepted,
                )
            },
            LayerStatus::Ok
        );
        assert_eq!(
            unsafe { layer_canvas_draw_frame(canvas.0) },
            LayerStatus::Ok
        );
        let mut pixels = vec![0_u8; 64 * 64 * 4];
        assert_eq!(
            unsafe {
                layer_canvas_copy_rgba8_srgb(canvas.0, pixels.as_mut_ptr(), pixels.len(), 64 * 4)
            },
            LayerStatus::Ok
        );
        let center = (32 * 64 + 32) * 4;
        assert!(
            (130..=145).contains(&pixels[center]),
            "linear 0.25 should be GPU-encoded near sRGB 137"
        );
        assert!(
            (120..=136).contains(&pixels[center + 3]),
            "half-opacity paint should retain its alpha"
        );
    }
}
