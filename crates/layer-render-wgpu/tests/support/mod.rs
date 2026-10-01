//! The engine and renderer harness the integration tests share: a headless
//! native renderer, composite captures and pen strokes.
#![allow(dead_code)]
use layer_core::*;
use layer_engine::{
    CanvasEngine, InputProducer, PenEvent, PenPhase, SampleFlags, ToolKind, ViewTransform,
    input_queue,
};
use layer_render::ViewState;
use layer_render_wgpu::WgpuRasterizer;

pub type Engine = CanvasEngine<WgpuRasterizer>;
pub const SIZE: [u32; 2] = [384, 256];

pub struct Image {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}
impl Image {
    pub fn crop(&self, origin: [u32; 2], size: [u32; 2]) -> Image {
        let mut rgba = Vec::with_capacity((size[0] * size[1] * 4) as usize);
        for y in origin[1]..origin[1] + size[1] {
            let start = ((y * self.size[0] + origin[0]) * 4) as usize;
            rgba.extend_from_slice(&self.rgba[start..start + size[0] as usize * 4]);
        }
        Image { size, rgba }
    }
    pub fn assert_eq(&self, other: &Image, what: &str) {
        assert_eq!(self.size, other.size, "{what}: size");
        let first = self.rgba.iter().zip(&other.rgba).position(|(a, b)| a != b);
        assert_eq!(first.map(|i| [(i / 4) as u32 % self.size[0], (i / 4) as u32 / self.size[0]]), None, "{what}: first differing pixel");
    }
    /// Every channel within `tolerance` codes of `other`; the color of pixels
    /// transparent in both is ignored.
    pub fn assert_near(&self, other: &Image, tolerance: u8, what: &str) {
        assert_eq!(self.size, other.size, "{what}: size");
        let (pixel, difference) = self
            .rgba
            .chunks_exact(4)
            .zip(other.rgba.chunks_exact(4))
            .map(|(a, b)| {
                let channels = if a[3] == 0 && b[3] == 0 { 3..4 } else { 0..4 };
                channels.map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0)
            })
            .enumerate()
            .max_by_key(|(_, d)| *d)
            .unwrap_or_default();
        let at = [pixel as u32 % self.size[0], pixel as u32 / self.size[0]];
        assert!(difference <= tolerance, "{what}: {difference} codes apart at {at:?}");
    }
}

pub fn engine(document: Document) -> (Engine, InputProducer<PenEvent>) {
    let gpu = WgpuRasterizer::new_native_headless(document.color).expect("physical GPU required");
    let (producer, consumer) = input_queue(64);
    let view = ViewState {
        width_px: 1024,
        height_px: 768,
        document_to_surface: Affine::IDENTITY.0,
        background_rgba_linear: [0.; 4],
    };
    let mut engine = CanvasEngine::new(gpu, document, consumer, view, ViewTransform::IDENTITY).unwrap();
    engine.render_frame_at(0).unwrap();
    (engine, producer)
}

pub fn image(engine: &mut Engine, time: u64) -> Image {
    engine.render_frame_at(time).unwrap();
    while engine.has_pending_document_edits() {
        std::thread::yield_now();
        engine.render_frame_at(time).unwrap();
    }
    let size = [engine.document().width, engine.document().height];
    let rgba = engine.backend_mut().readback_srgb_rgba8().unwrap();
    Image { size, rgba }
}

pub fn draw(engine: &mut Engine, input: &mut InputProducer<PenEvent>, color: [f32; 4], from: Point, to: Point, start: u64) {
    let mut brush = default_brush(DefaultBrushPreset::GPen);
    brush.diameter = 40.;
    brush.color_rgba_linear = color;
    engine.set_brush(brush).unwrap();
    draw_with_brush(engine, input, from, to, start);
}

/// A straight pen stroke with the engine's current brush.
pub fn draw_with_brush(engine: &mut Engine, input: &mut InputProducer<PenEvent>, from: Point, to: Point, start: u64) {
    for i in 0..10u64 {
        let t = i as f32 / 9.;
        let timestamp_ns = start + i * 8_000_000;
        input
            .push(PenEvent {
                device_id: 1,
                sequence: i,
                timestamp_ns,
                view_revision: 0,
                surface_position: Point { x: from.x + (to.x - from.x) * t, y: from.y + (to.y - from.y) * t },
                pressure: 0.8,
                tilt_radians: [0., 0.],
                twist_radians: 0.,
                distance: 0.,
                phase: match i {
                    0 => PenPhase::Down,
                    9 => PenPhase::Up,
                    _ => PenPhase::Move,
                },
                tool: ToolKind::Pen,
                flags: SampleFlags::PRIMARY,
            })
            .unwrap();
        engine.render_frame_at(timestamp_ns).unwrap();
        while engine.has_pending_input() {
            std::thread::yield_now();
            engine.render_frame_at(timestamp_ns).unwrap();
        }
    }
}

