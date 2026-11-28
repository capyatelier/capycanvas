use super::*;
use crate::WgpuRasterizer;

fn f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mantissa = bits & 0x7f_ffff;
    if exponent <= 0 {
        if exponent < -10 { return sign; }
        return sign | (((mantissa | 0x80_0000) >> (14 - exponent)) as u16);
    }
    if exponent >= 31 { return sign | 0x7c00; }
    sign | ((exponent as u16) << 10) | ((mantissa >> 13) as u16)
}

fn f32_of(half: u16) -> f32 {
    let sign = if half & 0x8000 != 0 { -1. } else { 1. };
    let exponent = i32::from((half >> 10) & 0x1f);
    let mantissa = f32::from(half & 0x3ff);
    sign * match exponent {
        0 => mantissa * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1. + mantissa / 1024.) * 2f32.powi(exponent - 15),
    }
}

struct Harness { r: WgpuRasterizer, paper: wgpu::Texture }

impl Harness {
    fn new(flat: bool) -> Self {
        let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        pollster::block_on(pipelines(&r.device).prepare(&r.device)).unwrap();
        let size = 64u32;
        let bytes: Vec<u8> = (0..size * size).map(|i| {
            if flat { return 128; }
            let (x, y) = ((i % size) as f32, (i / size) as f32);
            let v = 0.5 + 0.22 * (x * 0.37 + y * 0.11).sin() * (y * 0.29 - x * 0.07).cos() + 0.12 * ((x * 1.7).sin() * (y * 1.3).cos());
            (v.clamp(0., 1.) * 255.).round() as u8
        }).collect();
        let paper = r.device.create_texture(&wgpu::TextureDescriptor {
            label: None, size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
            dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[],
        });
        r.queue.write_texture(paper.as_image_copy(), &bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size), rows_per_image: Some(size) },
            wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 });
        Self { r, paper }
    }
    fn run(&self, record: impl FnOnce(&mut CommandEncoder)) {
        let mut encoder = CommandEncoder::new(&self.r.device, &Default::default());
        record(&mut encoder);
        encoder.submit(&self.r.queue);
        crate::test_support::complete_device(&self.r.device);
    }
    fn layer(&self, tiles: &[[u32; 2]]) -> WetLayer {
        let mut layer = WetLayer::new(&self.r.device);
        self.add(&mut layer, tiles);
        layer
    }
    fn add(&self, layer: &mut WetLayer, tiles: &[[u32; 2]]) {
        let view = self.paper.create_view(&Default::default());
        self.run(|encoder| {
            for &tile in tiles { layer.allocate(encoder, tile); }
            layer.prepare_fresh(encoder, &view, PaperMapping { grain: [64., 64., 0., 0.], axes: [1., 0., 0., 1.] });
        });
    }
    fn read(&self, texture: &wgpu::Texture, origin: [u32; 2], size: [u32; 2]) -> Vec<u8> {
        let texel = texture.format().block_copy_size(None).unwrap();
        let row = (size[0] * texel).div_ceil(256) * 256;
        let buffer = self.r.device.create_buffer(&wgpu::BufferDescriptor { label: None, size: u64::from(row * size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        self.run(|encoder| encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d { x: origin[0], y: origin[1], z: 0 }, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(size[1]) } },
            wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 }));
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        crate::test_support::complete_device(&self.r.device);
        let mapped = buffer.slice(..).get_mapped_range().unwrap();
        (0..size[1] as usize).flat_map(|y| mapped[y * row as usize..y * row as usize + (size[0] * texel) as usize].to_vec()).collect()
    }
    fn write(&self, texture: &wgpu::Texture, origin: [u32; 2], size: [u32; 2], bytes: &[u8]) {
        let texel = texture.format().block_copy_size(None).unwrap();
        self.r.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d { x: origin[0], y: origin[1], z: 0 }, aspect: wgpu::TextureAspect::All },
            bytes, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size[0] * texel), rows_per_image: Some(size[1]) },
            wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 });
    }
    fn tile_field(&self, layer: &WetLayer, field: usize, tile: [u32; 2]) -> Vec<u8> {
        self.read(layer.fields()[field], slot_origin(layer.slot(tile).unwrap()), [SLOT_CELLS; 2])
    }
    fn tile_rgba(&self, layer: &WetLayer, field: usize, tile: [u32; 2]) -> Vec<[f32; 4]> {
        self.tile_field(layer, field, tile).chunks(8).map(|t| std::array::from_fn(|i| f32_of(u16::from_le_bytes([t[i * 2], t[i * 2 + 1]])))).collect()
    }
    fn set_tile(&self, layer: &WetLayer, tile: [u32; 2], cell: impl Fn([u32; 2]) -> ([f32; 4], [f32; 4], [f32; 4], u32)) {
        let origin = slot_origin(layer.slot(tile).unwrap());
        let mut fields: [Vec<u8>; 4] = Default::default();
        for y in 0..SLOT_CELLS {
            for x in 0..SLOT_CELLS {
                let (state, pops, deposit, word) = cell([tile[0] * SLOT_CELLS + x, tile[1] * SLOT_CELLS + y]);
                for (field, values) in [state, pops, deposit].iter().enumerate() {
                    fields[field].extend(values.iter().flat_map(|v| f16(*v).to_le_bytes()));
                }
                fields[3].extend(word.to_le_bytes());
            }
        }
        for (texture, bytes) in layer.fields().into_iter().zip(&fields) { self.write(texture, origin, [SLOT_CELLS; 2], bytes); }
    }
}

fn wash(center: [f32; 2], radius: f32) -> Dab {
    Dab { center, radius, water: 0.55, absorbance: [0.9, 0.5, 0.2], pickup: 1., flow: 0.6 }
}

fn stroke(from: [f32; 2], to: [f32; 2], radius: f32) -> Vec<Dab> {
    (0..=24).map(|i| {
        let t = i as f32 / 24.;
        wash([from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t], radius)
    }).collect()
}

#[test]
fn shuffled_slots_compute_the_same_cells_as_ordered_slots() {
    let h = Harness::new(false);
    let tiles = [[0, 0], [1, 0], [2, 0], [0, 1], [1, 1], [2, 1]];
    let mut ordered = h.layer(&tiles);
    let mut shuffled = h.layer(&[[7, 7], [8, 8], [2, 1], [0, 1], [1, 1], [9, 9], [2, 0], [1, 0], [0, 0]]);
    shuffled.release([7, 7]);
    shuffled.release([9, 9]);
    shuffled.release([8, 8]);
    assert_ne!(ordered.slot([0, 0]), shuffled.slot([0, 0]));
    let dabs = stroke([40., 250.], [700., 270.], 70.);
    for layer in [&mut ordered, &mut shuffled] {
        h.run(|encoder| {
            layer.inject(encoder, &dabs[..12]);
            for _ in 0..30 { layer.step(encoder, Rate::Sixty, Look::default(), true); }
        });
    }
    h.add(&mut shuffled, &(0..30).map(|i| [20 + i, 20]).collect::<Vec<_>>());
    assert!(shuffled.slot([49, 20]).unwrap() >= ATLAS_SLOTS);
    for layer in [&mut ordered, &mut shuffled] {
        h.run(|encoder| {
            layer.inject(encoder, &dabs[12..]);
            for _ in 0..30 { layer.step(encoder, Rate::Sixty, Look::default(), true); }
        });
    }
    for tile in tiles {
        for field in 0..4 {
            assert_eq!(h.tile_field(&ordered, field, tile), h.tile_field(&shuffled, field, tile), "tile {tile:?} field {field}");
        }
    }
}

#[test]
fn a_dab_on_a_four_tile_corner_spreads_symmetrically() {
    let h = Harness::new(true);
    let tiles = [[0, 0], [1, 0], [0, 1], [1, 1]];
    let mut layer = h.layer(&tiles);
    h.run(|encoder| {
        layer.inject(encoder, &[Dab { center: [256., 256.], radius: 90., water: 0.6, absorbance: [0.8, 0.8, 0.8], pickup: 1., flow: 0.6 }]);
        for _ in 0..45 { layer.step(encoder, Rate::Sixty, Look::default(), true); }
    });
    let mut grid = vec![[0f32; 4]; 128 * 128];
    for tile in tiles {
        for (i, value) in h.tile_rgba(&layer, 0, tile).into_iter().enumerate() {
            let x = tile[0] as usize * 64 + i % 64;
            let y = tile[1] as usize * 64 + i / 64;
            grid[y * 128 + x] = value;
        }
    }
    let worst = (0..128 * 128).map(|i| {
        let (x, y) = (i % 128, i / 128);
        let a = grid[i];
        [grid[y * 128 + 127 - x], grid[(127 - y) * 128 + x], grid[x * 128 + y]].iter()
            .flat_map(|b| (0..4).map(move |c| (a[c] - b[c]).abs()))
            .fold(0f32, f32::max)
    }).fold(0f32, f32::max);
    let peak = grid.iter().map(|v| v[0]).fold(0f32, f32::max);
    assert!(peak > 0.1, "{peak}");
    assert!(worst <= 2e-3 * peak.max(1.), "asymmetry {worst}");
}

#[test]
fn a_closed_domain_conserves_water_and_pigment() {
    let h = Harness::new(false);
    let tiles = [[0, 0], [1, 0], [0, 1], [1, 1]];
    let mut layer = h.layer(&tiles);
    h.run(|encoder| layer.inject(encoder, &stroke([60., 60.], [450., 420.], 60.)));
    let totals = |layer: &WetLayer| tiles.iter().flat_map(|&t| h.tile_rgba(layer, 0, t).into_iter().zip(h.tile_rgba(layer, 2, t))).fold([0f64; 4], |mut sum, (state, deposit)| {
        sum[0] += f64::from(state[0]);
        for c in 1..4 { sum[c] += f64::from(state[c] + deposit[c - 1]); }
        sum
    });
    let before = totals(&layer);
    h.run(|encoder| for _ in 0..600 { layer.step(encoder, Rate::Sixty, Look::default(), false); });
    let after = totals(&layer);
    for c in 0..4 {
        assert!(before[c] > 100.);
        assert!(((after[c] - before[c]) / before[c]).abs() < 5e-4, "channel {c}: {before:?} -> {after:?}");
    }
}

#[test]
fn injection_rescales_populations_with_the_water() {
    let h = Harness::new(true);
    let mut layer = h.layer(&[[0, 0]]);
    let centre = |layer: &WetLayer| (h.tile_rgba(layer, 0, [0, 0])[32 * 64 + 32], h.tile_rgba(layer, 1, [0, 0])[32 * 64 + 32]);
    h.run(|encoder| layer.inject(encoder, &[Dab { center: [130., 130.], radius: 60., water: 0.6, absorbance: [0.5; 3], pickup: 1., flow: 1. }]));
    let (state, pops) = centre(&layer);
    assert!((state[0] - 0.6).abs() < 2e-3, "{state:?}");
    for p in pops { assert!((p - state[0] / 6.).abs() < 1e-3, "{pops:?}"); }
    h.run(|encoder| layer.inject(encoder, &[Dab { center: [130., 130.], radius: 60., water: 0.45, absorbance: [0.5; 3], pickup: 1., flow: 1. }]));
    let (state, pops) = centre(&layer);
    assert!((state[0] - 0.45).abs() < 2e-3, "{state:?}");
    let rest = state[0] - pops.iter().sum::<f32>();
    assert!((rest - state[0] / 3.).abs() < 1e-3, "{state:?} {pops:?}");
}

#[test]
fn a_dab_that_covers_no_pixel_leaves_no_water_pigment_or_extent() {
    let h = Harness::new(true);
    let mut layer = h.layer(&[[0, 0]]);
    h.run(|encoder| layer.inject(encoder, &[Dab { center: [100., 100.], radius: 0.3, water: 0.6, absorbance: [3.; 3], pickup: 1., flow: 1. }]));
    assert!(h.tile_field(&layer, 0, [0, 0]).iter().all(|&b| b == 0));
    assert!(h.tile_field(&layer, 3, [0, 0]).iter().all(|&b| b == 0));
}

fn front(h: &Harness, layer: &WetLayer, tiles: &[[u32; 2]]) -> f32 {
    let mut columns = vec![(0f32, 0u32); tiles.len() * 64];
    for (i, &tile) in tiles.iter().enumerate() {
        for (j, v) in h.tile_rgba(layer, 0, tile).into_iter().enumerate() {
            let column = &mut columns[i * 64 + j % 64];
            column.0 += v[1] / v[0].max(1e-5);
            column.1 += 1;
        }
    }
    let beyond = columns.iter().rposition(|&(sum, n)| sum / n as f32 > 0.1).unwrap_or(0);
    (beyond as f32 + 0.5) * 4. - 160.
}

#[test]
fn every_rate_moves_the_dye_front_alike() {
    let h = Harness::new(false);
    let tiles = [[0, 0], [1, 0], [2, 0]];
    let mut fronts = Vec::new();
    for rate in [Rate::Sixty, Rate::Thirty, Rate::FortyFive, Rate::Ninety, Rate::OneTwenty] {
        let mut layer = h.layer(&tiles);
        for &tile in &tiles {
            h.set_tile(&layer, tile, |[x, _]| {
                let w = if x < 40 { 0.75 } else { 0.45 };
                let dye = if x < 40 { w } else { 0. };
                ([w, dye, dye, dye], [w / 6.; 4], [0.; 4], u32::MAX)
            });
        }
        let mut seen = Vec::new();
        let mut done = 0;
        for seconds in [0.25f32, 0.5, 1., 2.] {
            let target = (seconds * rate.per_second() as f32).round() as u32;
            h.run(|encoder| for _ in done..target { layer.step(encoder, rate, Look::default(), false); });
            done = target;
            seen.push(front(&h, &layer, &tiles));
        }
        fronts.push((rate, seen));
    }
    let reference = fronts[0].1.clone();
    assert!(reference[3] > reference[0], "{fronts:?}");
    for (rate, seen) in &fronts {
        for (a, b) in seen.iter().zip(&reference) {
            assert!((a - b).abs() <= 4., "{rate:?}: {seen:?} vs {reference:?}");
        }
    }
}

fn paint(h: &Harness, deposit: [f32; 3], base: [f32; 4]) -> [f32; 4] {
    let mut layer = h.layer(&[[0, 0]]);
    h.set_tile(&layer, [0, 0], |_| ([0.; 4], [0.; 4], [deposit[0], deposit[1], deposit[2], 0.], u32::MAX));
    let texture = |usage| h.r.device.create_texture(&wgpu::TextureDescriptor {
        label: None, size: wgpu::Extent3d { width: TILE_PIXELS, height: TILE_PIXELS, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1,
        dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba32Float, usage, view_formats: &[],
    });
    let base_texture = texture(wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
    let out = texture(wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC);
    let bytes: Vec<u8> = (0..TILE_PIXELS * TILE_PIXELS).flat_map(|_| base.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>()).collect();
    h.write(&base_texture, [0, 0], [TILE_PIXELS; 2], &bytes);
    h.run(|encoder| layer.bake(encoder, &[[0, 0]], &base_texture.create_view(&Default::default()), &out.create_view(&Default::default())));
    let pixel = h.read(&out, [100, 100], [1, 1]);
    std::array::from_fn(|i| f32::from_le_bytes(pixel[i * 4..i * 4 + 4].try_into().unwrap()))
}

#[test]
fn clear_water_leaves_the_layer_unchanged_and_a_glaze_multiplies() {
    let h = Harness::new(true);
    let base = [0.2, 0.3, 0.45, 0.6];
    assert_eq!(paint(&h, [0.; 3], base), base);
    let glazed = paint(&h, [0.5, 0., 1.], [0.2, 0.4, 0.8, 1.]);
    let expected = [0.2 * (-0.5f32).exp(), 0.4, 0.8 * (-1f32).exp(), 1.];
    for c in 0..4 { assert!((glazed[c] - expected[c]).abs() < 2e-3, "{glazed:?} vs {expected:?}"); }
    let empty = paint(&h, [0.5, 0., 1.], [0.; 4]);
    let t = [(-0.5f32).exp(), 1., (-1f32).exp()];
    let expected = [t[0] - t[2], t[1] - t[2], 0., 1. - t[2]];
    for c in 0..4 { assert!((empty[c] - expected[c]).abs() < 2e-3, "{empty:?} vs {expected:?}"); }
}
