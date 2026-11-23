use super::*;

struct Plane {
    side: u32,
    weights: Vec<f32>,
    values: Vec<[f32; 4]>,
    initial: Vec<[f32; 4]>,
}

fn plane(side: u32, windows: &[[u32; 4]]) -> Plane {
    let area = (side * side) as usize;
    let count = windows.len() * area;
    let mut weights = vec![-1.0f32; count];
    let mut values = vec![[0.0f32; 4]; count];
    let mut initial = vec![[0.0f32; 4]; count];
    for (page, &[x0, y0, x1, y1]) in windows.iter().enumerate() {
        for y in 0..side { for x in 0..side {
            let at = page * area + (y * side + x) as usize;
            if x >= x0 && x < x1 && y >= y0 && y < y1 {
                weights[at] = match (x + y * 3) % 11 { 0 => 1., 1..=3 => 0.3, _ => 0. };
            }
            values[at] = std::array::from_fn(|c| ((x * 13 + y * 7 + c as u32 * 19) % 97) as f32 / 97. - 0.5);
            initial[at] = std::array::from_fn(|c| ((x * 3 + y * 11 + c as u32 * 7) % 83) as f32 / 83. - 0.5);
        } }
    }
    Plane { side, weights, values, initial }
}

fn relax_on_gpu(r: &WgpuRasterizer, k: &Pipelines, pages: &[[u32; 2]], windows: &[[u32; 4]], plane: &Plane, parities: &[u32]) -> Vec<[f32; 4]> {
    let span = [0, 1].map(|axis| pages.iter().map(|p| p[axis]).max().unwrap() + 1);
    let pyramid = Pyramid::scaled(pages, [0, 0], span, PAGE_SIZE / plane.side);
    let mut counts = vec![0u32];
    for &[x0, y0, x1, y1] in windows {
        counts.push(counts.last().unwrap() + (x1.div_ceil(BLOCK) - x0 / BLOCK) * (y1.div_ceil(BLOCK) - y0 / BLOCK));
    }
    let blocks = *counts.last().unwrap();
    let mut layout = pyramid.words();
    let windows_at = layout.len() as u32;
    layout.extend(windows.iter().flatten());
    layout.extend(std::iter::repeat_n(0, pages.len() + 1));
    layout.extend(counts);
    let mut slots = Slots { stride: u64::from(r.device.limits().min_uniform_buffer_offset_alignment), bytes: Vec::new() };
    let offsets = [0, 1].map(|parity| (0..blocks).step_by(37).map(|start| {
        (slots.push(Params { parity, candidate: start, pages: pages.len() as u32, windows: windows_at, ..Default::default() }), (blocks - start).min(37))
    }).collect::<Vec<_>>());
    let buffer = |label, contents: &[u8], usage| r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label), contents, usage,
    });
    let floats = |v: &[[f32; 4]]| v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect::<Vec<_>>();
    let params = buffer("relax params", &slots.bytes, wgpu::BufferUsages::UNIFORM);
    let layout = buffer("relax layout", &layout.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<_>>(), wgpu::BufferUsages::STORAGE);
    let values = buffer("relax values", &floats(&plane.values), wgpu::BufferUsages::STORAGE);
    let membrane = buffer("relax membrane", &floats(&plane.initial), wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
    let words = buffer("relax weights", &plane.weights.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<_>>(), wgpu::BufferUsages::STORAGE);
    let group = bindings::group(&r.device, "relax planes", &k.planes, [
        wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &params, offset: 0, size: NonZeroU64::new(PARAMS_BYTES) }),
        layout.as_entire_binding(), values.as_entire_binding(), membrane.as_entire_binding(), words.as_entire_binding(),
    ]);
    let readback = r.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("relax readback"), size: membrane.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false,
    });
    let mut encoder = r.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&k.relax);
        for &parity in parities { for &(offset, groups) in &offsets[parity as usize] {
            pass.set_bind_group(0, &group, &[offset]);
            pass.dispatch_workgroups(groups, 1, 1);
        } }
    }
    encoder.copy_buffer_to_buffer(&membrane, 0, &readback, 0, membrane.size());
    r.queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    r.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None }).unwrap();
    let bytes = readback.slice(..).get_mapped_range().unwrap();
    bytes.as_chunks::<16>().0.iter().map(|cell| std::array::from_fn(|c| f32::from_le_bytes(cell[4 * c..4 * c + 4].try_into().unwrap()))).collect()
}

fn relax_pipelines() -> (WgpuRasterizer, Pipelines) {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let k = Pipelines::new(&r.device);
    pollster::block_on(r.compilation.prepare_all([&k.relax])).unwrap();
    (r, k)
}

#[test]
fn relaxation_matches_sparse_block_reference() {
    let (r, k) = relax_pipelines();
    let pages = [[0, 0], [1, 0], [0, 1], [3, 3]];
    let sparse = |side: u32| [[0, 0, side, side], [0, 7, side - 9, side - 13], [11, 0, side - 3, side], [7, 7, 8, 8]];
    for (scale, windows) in [(1, sparse(256)), (4, sparse(64)), (32, [[0, 0, 8, 8], [1, 0, 8, 7], [0, 2, 6, 8], [3, 3, 5, 5]])] {
        let side = PAGE_SIZE / scale;
        let plane = plane(side, &windows);
        let area = (side * side) as usize;
        let mut expected = plane.initial.clone();
        let index = |x: i32, y: i32| -> Option<usize> {
            if x < 0 || y < 0 { return None; }
            let (x, y) = (x as u32, y as u32);
            let page = pages.iter().position(|p| *p == [x / side, y / side])?;
            let at = page * area + ((y % side) * side + x % side) as usize;
            (plane.weights[at] >= 0.).then_some(at)
        };
        let per_page = side.div_ceil(BLOCK);
        for _ in 0..SWEEPS / BLOCK_SWEEPS { for parity in 0..2 {
            for (page, &[x0, y0, x1, y1]) in windows.iter().enumerate() {
                for by in y0 / BLOCK..y1.div_ceil(BLOCK) { for bx in x0 / BLOCK..x1.div_ceil(BLOCK) {
                    if (pages[page][0] * per_page + bx + pages[page][1] * per_page + by) % 2 != parity { continue; }
                    for sweep in 0..2 * BLOCK_SWEEPS {
                        for ly in by * BLOCK..((by + 1) * BLOCK).min(side) { for lx in bx * BLOCK..((bx + 1) * BLOCK).min(side) {
                            let [x, y] = [pages[page][0] * side + lx, pages[page][1] * side + ly];
                            if (x + y) % 2 != sweep % 2 { continue; }
                            let Some(at) = index(x as i32, y as i32) else { continue; };
                            let w = plane.weights[at];
                            if w >= 1. { continue; }
                            let mut sum = [0.0f32; 4];
                            let mut n = 0.;
                            for [dx, dy] in [[-1, 0], [1, 0], [0, -1], [0, 1]] {
                                if let Some(i) = index(x as i32 + dx, y as i32 + dy) {
                                    for c in 0..4 { sum[c] += expected[i][c]; }
                                    n += 1.;
                                }
                            }
                            for c in 0..4 {
                                let average = if n > 0. { sum[c] / n } else { expected[at][c] };
                                expected[at][c] = w * plane.values[at][c] + (1. - w) * average;
                            }
                        } }
                    }
                } }
            }
        } }
        let parities: Vec<u32> = (0..SWEEPS / BLOCK_SWEEPS).flat_map(|_| [0, 1]).collect();
        let actual = relax_on_gpu(&r, &k, &pages, &windows, &plane, &parities);
        for (i, (actual, expected)) in actual.iter().flatten().zip(expected.iter().flatten()).enumerate() {
            assert!((actual - expected).abs() < 0.000002, "scale {scale}, cell {}, channel {}: {actual} != {expected}", i / 4, i % 4);
        }
    }
}

#[test]
fn adjacent_reduced_windows_relax_in_opposite_colours() {
    let (r, k) = relax_pipelines();
    let windows = [[0, 0, 8, 8]; 2];
    let plane = plane(8, &windows);
    for pages in [[[0, 0], [1, 0]], [[0, 0], [0, 1]]] {
        let relaxed = relax_on_gpu(&r, &k, &pages, &windows, &plane, &[0]);
        let changed: Vec<bool> = relaxed.chunks(64).zip(plane.initial.chunks(64)).map(|(after, before)| after != before).collect();
        assert_eq!(changed, [true, false], "pages {pages:?}: one colour relaxes only the first of two windows that share an edge");
    }
}
