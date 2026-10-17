use super::tiles::{self, Frame, LevelGeometry, capsule, capsule_distance};
use super::{Batch, Kernel, Layer, Pool};
use layer_core::authored::vector::*;
use layer_core::color::RgbColor;
use std::sync::Arc;

const SIZE: u32 = 256;

fn paint(rgba: [f32; 4]) -> Paint { Paint::color(RgbColor::new(Default::default(), rgba).unwrap()) }
fn smooth(points: &[[f64; 2]], closed: bool) -> Subpath {
    let n = points.len();
    let anchors: Vec<_> = (0..n).map(|i| {
        let previous = points[if i == 0 { if closed { n - 1 } else { 0 } } else { i - 1 }];
        let next = points[if i + 1 == n { if closed { 0 } else { n - 1 } } else { i + 1 }];
        let tangent = [(next[0] - previous[0]) / 6., (next[1] - previous[1]) / 6.];
        Anchor { point: points[i], in_handle: [-tangent[0], -tangent[1]], out_handle: tangent, kind: AnchorKind::SMOOTH }
    }).collect();
    Subpath::new(&anchors, closed)
}
fn path(subpaths: Vec<Subpath>, entries: Vec<PaintEntry>, rule: FillRule) -> PathItem {
    let mut item = PathItem::new(ItemId(1), Arc::new(PathGeometry::new(subpaths).unwrap()), Arc::new(Style::new(entries)));
    item.fill_rule = rule;
    item
}
fn frame(level: u32) -> Frame { let scale = 1. / f64::from(1 << level); Frame { scale, origin: [0.; 2], clip: [-4, -4, 64, 64] } }
fn round_stroke(width: f64, profile: Option<WidthProfile>) -> PaintEntry {
    let mut stroke = Stroke::new(paint([0.1, 0.2, 0.9, 1.]), width);
    stroke.cap = Cap::Round; stroke.join = Join::Round; stroke.profile = profile.map(Arc::new);
    PaintEntry::Stroke(stroke)
}

fn scenes() -> Vec<(&'static str, PathItem)> {
    let star: Vec<[f64; 2]> = (0..10).map(|i| { let a = i as f64 * std::f64::consts::PI / 5.; let r = if i % 2 == 0 { 100. } else { 38. }; [128. + r * a.sin(), 130. - r * a.cos()] }).collect();
    let corners = |points: &[[f64; 2]], closed| Subpath::new(&points.iter().map(|p| Anchor::corner(*p)).collect::<Vec<_>>(), closed);
    let fill = |rgba| PaintEntry::Fill(Fill { paint: paint(rgba), unknown: None });
    let blob = smooth(&[[40., 60.], [200., 30.], [230., 190.], [120., 240.], [20., 170.]], true);
    let hole = smooth(&[[90., 110.], [150., 100.], [160., 160.], [100., 170.]], true);
    vec![
        ("nonzero star", path(vec![corners(&star, true)], vec![fill([0.8, 0.3, 0.1, 1.])], FillRule::NonZero)),
        ("even-odd pentagram", path(vec![corners(&(0..5).map(|i| star[(i * 4) % 10]).collect::<Vec<_>>(), true)], vec![fill([0.2, 0.6, 0.3, 0.8])], FillRule::EvenOdd)),
        ("curved compound", path(vec![blob.clone(), hole], vec![fill([0.3, 0.3, 0.7, 1.])], FillRule::EvenOdd)),
        ("hairline and wide strokes", path(vec![smooth(&[[10., 20.], [120., 60.], [240., 30.]], false), smooth(&[[20., 200.], [128., 140.], [236., 210.]], false)],
            vec![round_stroke(0.4, None)], FillRule::NonZero)),
        ("tapered ink", path(vec![smooth(&[[30., 40.], [90., 200.], [170., 60.], [230., 220.]], false)],
            vec![round_stroke(14., Some(WidthProfile::new(vec![WidthKnot { segment: 0, t: 0., left: 0., right: 0. }, WidthKnot { segment: 1, t: 0.5, left: 1., right: 1. }, WidthKnot { segment: 2, t: 1., left: 0.05, right: 0.05 }]).unwrap()))], FillRule::NonZero)),
        ("thick round stroke over fill", path(vec![blob], vec![fill([0.9, 0.9, 0.2, 1.]), round_stroke(40., None)], FillRule::NonZero)),
        ("miter outline", path(vec![corners(&[[30., 200.], [128., 40.], [226., 200.]], false)], vec![PaintEntry::Stroke(Stroke::new(paint([0., 0., 0., 1.]), 12.))], FillRule::NonZero)),
    ]
}

fn colors(item: &PathItem) -> Vec<[f32; 4]> {
    item.style.entries.iter().map(|e| {
        let rgba = match e { PaintEntry::Fill(f) => f.paint.rgb(), PaintEntry::Stroke(s) => s.paint.rgb(), _ => None }.unwrap().rgba;
        [rgba[0] * rgba[3], rgba[1] * rgba[3], rgba[2] * rgba[3], rgba[3]]
    }).collect()
}

fn emulate(batch: &Batch, pool: &[u32]) -> Vec<[f64; 4]> {
    let mut out = vec![[0.; 4]; (SIZE * SIZE) as usize];
    for tile in &batch.tiles {
        for py in 0..16 { for px in 0..16 {
            let mut color = [0f64; 4];
            let mut stroke = 0f64;
            for command in &batch.commands[tile[2] as usize..tile[3] as usize] {
                let [header, start, backdrop, palette] = *command;
                let (kind, count) = (header & 15, header >> 8);
                let coverage = match kind {
                    0 => 1.,
                    3 => {
                        for i in 0..count {
                            let c = capsule(&pool[(start + i * 3) as usize..]);
                            let thin = (c[5] + c[6]).min(1.);
                            let at = [(i64::from(tile[0] + px) + batch.origin[0]) as f64 + 0.5, (i64::from(tile[1] + py) + batch.origin[1]) as f64 + 0.5];
                            stroke = stroke.max((0.5 - capsule_distance(at, &c)).clamp(0., 1.) * thin);
                        }
                        if header & tiles::CONTINUE != 0 { continue; }
                        std::mem::take(&mut stroke)
                    }
                    _ => {
                        let mut acc = f64::from(backdrop as i32);
                        for i in 0..count {
                            let packed = [pool[(start + i * 2) as usize], pool[(start + i * 2 + 1) as usize]];
                            let p0 = [f64::from(packed[0] & 0xffff) / 2048., f64::from(packed[0] >> 16) / 2048.];
                            let p1 = [f64::from(packed[1] & 0xffff) / 2048., f64::from(packed[1] >> 16) / 2048.];
                            let d = [p1[0] - p0[0], p1[1] - p0[1]];
                            let y = p0[1] - py as f64;
                            let (y0, y1) = (y.clamp(0., 1.), (y + d[1]).clamp(0., 1.));
                            let dy = y0 - y1;
                            if dy != 0. {
                                let (t0, t1) = ((y0 - y) / d[1], (y1 - y) / d[1]);
                                let start = p0[0] - px as f64;
                                let (x0, x1) = (start + t0 * d[0], start + t1 * d[0]);
                                let (xmin, xmax) = (x0.min(x1).min(1.) - 1e-9, x0.max(x1));
                                let b = xmax.min(1.); let c = b.max(0.); let dd = xmin.max(0.);
                                acc += (b + 0.5 * (dd * dd - c * c) - xmin) / (xmax - xmin) * dy;
                            }
                            let edge = if packed[0] & 0xffff == 0 { p0[1] } else if packed[1] & 0xffff == 0 { p1[1] } else { 1e9 };
                            acc += d[0].signum() * (py as f64 - edge + 1.).clamp(0., 1.) * f64::from(u8::from(d[0] != 0.));
                        }
                        if kind == 2 { (acc - 2. * (0.5 * acc).round()).abs() } else { acc.abs().min(1.) }
                    }
                };
                let source = batch.colors[palette as usize].map(|v| f64::from(v) * coverage);
                for k in 0..4 { color[k] = source[k] + color[k] * (1. - source[3]); }
            }
            let (x, y) = (tile[0] + px, tile[1] + py);
            if (0..SIZE as i32).contains(&x) && (0..SIZE as i32).contains(&y) { out[(y as u32 * SIZE + x as u32) as usize] = color; }
        }}
    }
    out
}

fn fine(item: &PathItem, level: u32) -> (Vec<Vec<[f64; 2]>>, Vec<Vec<([f64; 2], f64)>>) {
    let scale = 1. / f64::from(1 << level);
    let map = |p: [f64; 2]| [p[0] * scale, p[1] * scale];
    let mut rings = Vec::new();
    let mut lines = Vec::new();
    let mut base = 0u32;
    for subpath in item.geometry.subpaths() {
        let mut ring = vec![map(subpath.anchor(0).point)];
        let mut line = vec![(map(subpath.anchor(0).point), 0u32, 0.)];
        for (i, segment) in subpath.segments().enumerate() {
            tiles::flatten(&segment, map, 0.002, |p, t| { ring.push(p); line.push((p, base + i as u32, t)); });
        }
        base += subpath.segment_count() as u32;
        rings.push(ring);
        let stroke = item.style.strokes().next();
        lines.push(line.into_iter().map(|(p, s, t)| (p, stroke.map_or(0., |k| k.width * 0.5 * scale * k.profile.as_ref().map_or(1., |w| w.at(s, t)[0])))).collect());
    }
    (rings, lines)
}
fn winding(rings: &[Vec<[f64; 2]>], p: [f64; 2]) -> i32 {
    let mut w = 0;
    for ring in rings { for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        if (a[1] <= p[1]) != (b[1] <= p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if x < p[0] { w += if b[1] < a[1] { 1 } else { -1 }; }
        }
    }}
    w
}
fn inside_stroke(lines: &[Vec<([f64; 2], f64)>], p: [f64; 2]) -> bool {
    lines.iter().any(|line| line.windows(2).any(|w| {
        let (a, b) = (w[0].0, w[1].0);
        let d = [b[0] - a[0], b[1] - a[1]];
        let l2 = d[0] * d[0] + d[1] * d[1];
        let t = if l2 > 0. { (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / l2).clamp(0., 1.) } else { 0. };
        let q = [a[0] + d[0] * t - p[0], a[1] + d[1] * t - p[1]];
        (q[0] * q[0] + q[1] * q[1]).sqrt() <= w[0].1 + (w[1].1 - w[0].1) * t
    }))
}
fn reference(item: &PathItem, level: u32) -> Vec<[f64; 4]> {
    let (rings, lines) = fine(item, level);
    let colors = colors(item);
    let n = 8;
    (0..SIZE * SIZE).map(|i| {
        let (x, y) = (f64::from(i % SIZE), f64::from(i / SIZE));
        let mut color = [0f64; 4];
        for (entry, paint) in item.style.entries.iter().enumerate() {
            let mut hits = 0;
            for sy in 0..n { for sx in 0..n {
                let p = [x + (sx as f64 + 0.5) / n as f64, y + (sy as f64 + 0.5) / n as f64];
                let inside = match paint {
                    PaintEntry::Fill(_) => { let w = winding(&rings, p); if item.fill_rule == FillRule::EvenOdd { w % 2 != 0 } else { w != 0 } }
                    _ => inside_stroke(&lines, p),
                };
                hits += usize::from(inside);
            }}
            let coverage = hits as f64 / (n * n) as f64;
            let source = colors[entry].map(|v| f64::from(v) * coverage);
            for k in 0..4 { color[k] = source[k] + color[k] * (1. - source[3]); }
        }
        color
    }).collect()
}
fn batch(item: &PathItem, level: u32) -> (LevelGeometry, Batch) {
    let geometry = tiles::derive(item, &frame(level));
    let colors = colors(item);
    let mut batch = Batch::default();
    batch.build(&[Layer { geometry: &geometry, base: 0, colors: &colors }], [0, 0], [SIZE; 2]);
    (geometry, batch)
}
fn compare(a: &[[f64; 4]], b: &[[f64; 4]]) -> (f64, f64) {
    let errors: Vec<f64> = a.iter().zip(b).map(|(a, b)| (a[3] - b[3]).abs()).collect();
    (errors.iter().copied().fold(0., f64::max), errors.iter().sum::<f64>() / errors.len() as f64)
}

#[test]
fn binned_coverage_matches_the_exact_reference() {
    for level in [0, 1] {
        for (name, item) in scenes() {
            if name == "miter outline" { continue; }
            let (geometry, batch) = batch(&item, level);
            let emulated = emulate(&batch, &geometry.words);
            let expected = reference(&item, level);
            let (max, mean) = compare(&emulated, &expected);
            let mut worst: Vec<(f64, u32)> = emulated.iter().zip(&expected).enumerate().map(|(i, (a, b))| ((a[3] - b[3]).abs(), i as u32)).collect();
            worst.sort_by(|a, b| b.0.total_cmp(&a.0));
            eprintln!("{name} level {level} worst: {:?}", worst[..4].iter().map(|(e, i)| (format!("{e:.3}"), i % SIZE, i / SIZE, format!("{:.3}/{:.3}", emulated[*i as usize][3], expected[*i as usize][3]))).collect::<Vec<_>>());
            eprintln!("{name} level {level}: max {max:.4} mean {mean:.5} tiles {} commands {}", batch.tile_count(), batch.command_count());
            assert!(mean < 0.004 && max < 0.35, "{name} level {level}: max {max} mean {mean}");
        }
    }
}

#[test]
fn outlines_close_and_stay_inside_their_stroke() {
    let (name, item) = scenes().pop().unwrap();
    let (geometry, batch) = batch(&item, 0);
    let emulated = emulate(&batch, &geometry.words);
    let (_, lines) = fine(&item, 0);
    let covered = emulated.iter().filter(|c| c[3] > 0.99).count();
    let outside = emulated.iter().enumerate().filter(|(i, c)| c[3] > 0.01 && !inside_stroke(&lines, [f64::from(*i as u32 % SIZE) + 0.5, f64::from(*i as u32 / SIZE) + 0.5]) && {
        let (x, y) = (f64::from(*i as u32 % SIZE) + 0.5, f64::from(*i as u32 / SIZE) + 0.5);
        ![[-1., 0.], [1., 0.], [0., -1.], [0., 1.]].iter().any(|o: &[f64; 2]| inside_stroke(&lines, [x + o[0], y + o[1]]))
    }).count();
    eprintln!("{name}: covered {covered} stray {outside}");
    assert!(covered > 3000);
    assert!(outside < 400, "{name}: {outside} pixels outside a round stroke of the same width");
}

#[test]
fn gpu_kernel_matches_its_emulation() {
    let r = crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let kernel = Kernel::new(&r.device);
    let mut uploads = crate::Uploads::new(&r.device, 1 << 20);
    let mut pool = Pool::new(64 << 20);
    let mut scratch = Default::default();
    for (name, item) in scenes() {
        let geometry = Arc::new(tiles::derive(&item, &frame(0)));
        let base = pool.allocate(&geometry).unwrap();
        let colors = colors(&item);
        let mut batch = Batch::default();
        batch.build(&[Layer { geometry: &geometry, base, colors: &colors }], [0, 0], [SIZE; 2]);
        let output = crate::test_support::page_texture(&r, wgpu::TextureFormat::Rgba32Float);
        let view = output.create_view(&Default::default());
        let mut encoder = r.device.create_command_encoder(&Default::default());
        let buffer = pool.flush(&r.device, &mut encoder, &mut uploads).unwrap();
        kernel.encode(&r.device, &mut encoder, &mut uploads, &mut scratch, &batch, &buffer, None, &r.empty_view, &view).unwrap();
        uploads.finish(&encoder);
        r.queue.submit([encoder.finish()]);
        let gpu = crate::test_support::float_pixels(&r, &output);
        let mut words = vec![0u32; base as usize];
        words.extend_from_slice(&geometry.words);
        let expected = emulate(&batch, &words);
        let max = gpu.iter().zip(&expected).flat_map(|(a, b)| (0..4).map(move |k| (f64::from(a[k]) - b[k]).abs())).fold(0., f64::max);
        eprintln!("{name}: gpu vs emulation max {max:.6}");
        assert!(max < 2e-3, "{name}: {max}");
    }
}
