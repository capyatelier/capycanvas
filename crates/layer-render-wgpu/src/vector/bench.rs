use super::tiles::{self, Frame, LevelGeometry};
use super::{Batch, Kernel, Layer, Pool};
use layer_core::authored::vector::*;
use layer_core::color::RgbColor;
use std::{sync::Arc, time::Instant};

struct Random(u64);
impl Random {
    fn next(&mut self) -> f64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; (self.0 >> 11) as f64 / (1u64 << 53) as f64 }
    fn range(&mut self, a: f64, b: f64) -> f64 { a + (b - a) * self.next() }
}
fn paint(random: &mut Random, alpha: f32) -> Paint {
    Paint::color(RgbColor::new(Default::default(), [random.next() as f32, random.next() as f32, random.next() as f32, alpha]).unwrap())
}
fn smooth(points: &[[f64; 2]], closed: bool) -> Subpath {
    let n = points.len();
    let anchors: Vec<_> = (0..n).map(|i| {
        let previous = points[if i == 0 { if closed { n - 1 } else { 0 } } else { i - 1 }];
        let next = points[if i + 1 == n { if closed { 0 } else { n - 1 } } else { i + 1 }];
        let t = [(next[0] - previous[0]) / 6., (next[1] - previous[1]) / 6.];
        Anchor { point: points[i], in_handle: [-t[0], -t[1]], out_handle: t, kind: AnchorKind::SMOOTH }
    }).collect();
    Subpath::new(&anchors, closed)
}
fn round(width: f64, paint: Paint, profile: bool) -> PaintEntry {
    let mut stroke = Stroke::new(paint, width);
    stroke.cap = Cap::Round; stroke.join = Join::Round;
    if profile { stroke.profile = Some(Arc::new(WidthProfile::new(vec![WidthKnot { segment: 0, t: 0., left: 0.1, right: 0.1 }, WidthKnot { segment: 0, t: 0.6, left: 1., right: 1. }, WidthKnot { segment: 1, t: 1., left: 0.05, right: 0.05 }]).unwrap())); }
    PaintEntry::Stroke(stroke)
}
pub(crate) fn scene_a(extent: [f64; 2]) -> Vec<PathItem> {
    let mut random = Random(0x2545_f491_4f6c_dd1d);
    (0..5000).map(|i| {
        let center = [random.range(0., extent[0]), random.range(0., extent[1])];
        let radius = random.range(20., 260.);
        let points: Vec<[f64; 2]> = (0..8).map(|k| { let a = k as f64 * std::f64::consts::TAU / 8.; let r = radius * random.range(0.6, 1.); [center[0] + r * a.cos(), center[1] + r * a.sin()] }).collect();
        let entries = if i % 3 == 0 { vec![PaintEntry::Fill(Fill { paint: paint(&mut random, 1.), unknown: None }), round(random.range(2., 8.), paint(&mut random, 1.), false)] }
            else if i % 3 == 1 { vec![PaintEntry::Fill(Fill { paint: paint(&mut random, 0.7), unknown: None })] }
            else { vec![round(random.range(3., 12.), paint(&mut random, 1.), false)] };
        PathItem::new(ItemId(i + 1), Arc::new(PathGeometry::new(vec![smooth(&points, i % 3 != 2)]).unwrap()), Arc::new(Style::new(entries)))
    }).collect()
}
pub(crate) fn scene_b(extent: [f64; 2], count: u64) -> Vec<PathItem> {
    let mut random = Random(0x9e37_79b9_7f4a_7c15);
    (0..count).map(|i| {
        let start = [random.range(0., extent[0]), random.range(0., extent[1])];
        let angle = random.range(0.4, 0.9);
        let length = random.range(40., 220.);
        let bend = random.range(-20., 20.);
        let points: Vec<[f64; 2]> = (0..4).map(|k| { let t = k as f64 / 3.; [start[0] + angle.cos() * length * t - angle.sin() * bend * (t * (1. - t)) * 4., start[1] + angle.sin() * length * t + angle.cos() * bend * (t * (1. - t)) * 4.] }).collect();
        let style = Style::new(vec![round(random.range(2., 6.), paint(&mut random, 1.), true)]);
        PathItem::new(ItemId(i + 1), Arc::new(PathGeometry::new(vec![smooth(&points, false)]).unwrap()), Arc::new(style))
    }).collect()
}
fn colors(item: &PathItem) -> Vec<[f32; 4]> {
    item.style.entries.iter().map(|e| {
        let c = match e { PaintEntry::Fill(f) => f.paint.rgb(), PaintEntry::Stroke(s) => s.paint.rgb(), _ => None }.unwrap().rgba;
        [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
    }).collect()
}
fn median(mut values: Vec<f64>) -> f64 { values.sort_by(f64::total_cmp); values[values.len() / 2] }

struct Timer { query: wgpu::QuerySet, marker: crate::frame_timing::TimestampMarker, resolved: wgpu::Buffer, readback: wgpu::Buffer, period: f64 }
impl Timer {
    fn new(r: &crate::WgpuRasterizer) -> Option<Self> {
        let device: &wgpu::Device = &r.device;
        device.features().contains(wgpu::Features::TIMESTAMP_QUERY).then(|| Self {
            query: device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("vector bench"), ty: wgpu::QueryType::Timestamp, count: 2 }),
            marker: crate::frame_timing::TimestampMarker::new(device, Default::default()),
            resolved: device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 16, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false }),
            readback: device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 16, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false }),
            period: f64::from(r.queue.get_timestamp_period()),
        })
    }
}
fn run(r: &crate::WgpuRasterizer, kernel: &Kernel, uploads: &mut crate::Uploads, timer: &Option<Timer>, batch: &Batch, pool: &wgpu::Buffer, view: &wgpu::TextureView) -> (f64, f64) {
    let mut scratch = Default::default();
    let start = Instant::now();
    let mut encoder = r.device.create_command_encoder(&Default::default());
    if let Some(t) = timer { t.marker.write(&mut encoder, &t.query, 0); }
    kernel.encode(&r.device, &mut encoder, uploads, &mut scratch, batch, pool, None, &r.empty_view, view).unwrap();
    if let Some(t) = timer {
        t.marker.write(&mut encoder, &t.query, 1);
        encoder.resolve_query_set(&t.query, 0..2, &t.resolved, 0);
        encoder.copy_buffer_to_buffer(&t.resolved, 0, &t.readback, 0, 16);
    }
    uploads.finish(&encoder);
    let submission = r.queue.submit([encoder.finish()]);
    r.device.poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(std::time::Duration::from_secs(60)) }).unwrap();
    let wall = start.elapsed().as_secs_f64() * 1e3;
    let gpu = timer.as_ref().map_or(f64::NAN, |t| {
        t.readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let ticks: Vec<u64> = t.readback.slice(..).get_mapped_range().unwrap().chunks_exact(8).map(|b| u64::from_le_bytes(b.try_into().unwrap())).collect();
        t.readback.unmap();
        ticks[1].saturating_sub(ticks[0]) as f64 * t.period / 1e6
    });
    (wall, gpu)
}

#[test]
#[ignore = "vector kernel hardware benchmark"]
fn vector_kernel_scenes() {
    let r = crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let kernel = Kernel::new(&r.device);
    let mut uploads = crate::Uploads::new(&r.device, 8 << 20);
    let timer = Timer::new(&r);
    let extent = [4000., 3000.];
    let level = 1u32;
    let size = [2000u32, 1500u32];
    let texture = r.device.create_texture(&wgpu::TextureDescriptor { label: Some("vector bench"), size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] });
    let view = texture.create_view(&Default::default());
    let frame = Frame { scale: 0.5, origin: [0.; 2], clip: [0, 0, size[0].div_ceil(16) as i32, size[1].div_ceil(16) as i32] };
    let device_name = r.adapter.get_info().name;
    eprintln!("vector kernel bench on {device_name}, level {level}, window {}x{}", size[0], size[1]);
    for (name, items) in [("A: 5,000 shapes", scene_a(extent)), ("B: 50,000 hatching strokes", scene_b(extent, 50_000)), ("B10: 10,000 hatching strokes", scene_b(extent, 10_000))] {
        let start = Instant::now();
        let geometry: Vec<Arc<LevelGeometry>> = items.iter().map(|item| Arc::new(tiles::derive(item, &frame))).collect();
        let derive = start.elapsed().as_secs_f64() * 1e3;
        let mut pool = Pool::new(512 << 20);
        let bases: Vec<u32> = geometry.iter().map(|g| pool.allocate(g).unwrap()).collect();
        let start = Instant::now();
        let mut encoder = r.device.create_command_encoder(&Default::default());
        let buffer = pool.flush(&r.device, &mut encoder, &mut uploads).unwrap();
        uploads.finish(&encoder);
        r.queue.submit([encoder.finish()]);
        let upload = start.elapsed().as_secs_f64() * 1e3;
        let palette: Vec<Vec<[f32; 4]>> = items.iter().map(colors).collect();
        let layers: Vec<Layer> = geometry.iter().zip(&bases).zip(&palette).rev().map(|((geometry, base), colors)| Layer { geometry, base: *base, colors }).collect();
        let mut batch = Batch::default();
        let start = Instant::now();
        batch.build(&layers, [0, 0], size);
        let build = start.elapsed().as_secs_f64() * 1e3;
        let derived: usize = geometry.iter().map(|g| g.bytes()).sum();
        for _ in 0..3 { run(&r, &kernel, &mut uploads, &timer, &batch, &buffer, &view); }
        let samples: Vec<(f64, f64)> = (0..9).map(|_| run(&r, &kernel, &mut uploads, &timer, &batch, &buffer, &view)).collect();
        eprintln!("{name}: derive {derive:.1} ms, pool upload {upload:.1} ms ({:.1} MB), build {build:.1} ms, derived {:.1} MB, frame upload {:.1} MB, tiles {}, commands {}; full redraw wall {:.2} ms gpu {:.2} ms",
            f64::from(pool.used_words()) * 4. / 1e6, derived as f64 / 1e6, batch.bytes() as f64 / 1e6, batch.tile_count(), batch.command_count(),
            median(samples.iter().map(|s| s.0).collect()), median(samples.iter().map(|s| s.1).collect()));
        let region = [1414u32, 1414u32];
        let mut window = Batch::default();
        let start = Instant::now();
        window.build(&layers, [292, 43], region);
        let build = start.elapsed().as_secs_f64() * 1e3;
        let samples: Vec<(f64, f64)> = (0..9).map(|_| run(&r, &kernel, &mut uploads, &timer, &window, &buffer, &view)).collect();
        eprintln!("{name}: 2 MP window build {build:.1} ms, upload {:.1} MB, commands {}, wall {:.2} ms gpu {:.2} ms",
            window.bytes() as f64 / 1e6, window.command_count(), median(samples.iter().map(|s| s.0).collect()), median(samples.iter().map(|s| s.1).collect()));
    }
}
