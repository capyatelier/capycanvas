//! GPU connected regions with immutable packed coverage. Readback is a single
//! asynchronous history snapshot per request, never part of live brush work.
use super::*;
use layer_render::RegionRefinement;
use wgpu::util::DeviceExt;

pub(super) struct Flood {
    layout: wgpu::BindGroupLayout,
    pipelines: std::collections::HashMap<&'static str, Deferred<wgpu::ComputePipeline>>,
    parents: Option<wgpu::Buffer>,
    mask: wgpu::Buffer,
    capacity: u64,
    empty: wgpu::Buffer,
}
const STAGES: [&str; 11] = [
    "classify",
    "close_h",
    "close_v",
    "reopen_h",
    "reopen_v",
    "initialize",
    "merge",
    "component_mask",
    "expand_h",
    "expand_v",
    "pack",
];
fn stages(refinement: RegionRefinement) -> impl Iterator<Item = &'static str> {
    STAGES.into_iter().filter(move |entry| match *entry {
        "classify" | "close_h" | "close_v" | "reopen_h" | "reopen_v" => refinement.gap_closing != 0,
        "component_mask" => refinement.expansion != 0 || refinement.smoothing != 0.,
        "expand_h" | "expand_v" => refinement.expansion != 0,
        _ => true,
    })
}
pub(super) struct Region {
    pub coverage: wgpu::Buffer,
    pub bounds_offset: u64,
}
impl Flood {
    pub fn storage_bytes(&self) -> u64 {
        self.capacity + self.empty.size() + self.mask.size()
    }
    pub fn new(device: &PipelineDevice) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("connected region"),
            source: wgpu::ShaderSource::Wgsl(compose_wgsl(&[
                include_str!("flood.wgsl"),
                include_str!("region_refine.wgsl"),
                &include_str!("selection_clip.wgsl")
                    .replace("@group(1) @binding(1)", "@group(0) @binding(4)"),
            ])),
        });
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }];
        entries.extend((1..6).map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: if binding == 1 {
                    wgpu::BufferBindingType::Uniform
                } else {
                    wgpu::BufferBindingType::Storage {
                        read_only: binding == 4,
                    }
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("connected region"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("connected region"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipelines = STAGES
            .into_iter()
            .map(|entry| {
                let (device, layout, shader) =
                    (device.clone(), pipeline_layout.clone(), shader.clone());
                (
                    entry,
                    Deferred::new(move || {
                        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                            label: Some(entry),
                            layout: Some(&layout),
                            module: &shader,
                            entry_point: Some(entry),
                            compilation_options: Default::default(),
                            cache: None,
                        })
                    }),
                )
            })
            .collect();
        Self {
            layout,
            pipelines,
            parents: None,
            mask: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("empty region morphology"),
                size: 4,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            capacity: 0,
            empty: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("unlimited region"),
                size: 48,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
        }
    }
    pub fn pipelines(&self) -> impl Iterator<Item = &Deferred<wgpu::ComputePipeline>> {
        STAGES.into_iter().map(|entry| &self.pipelines[entry])
    }
    pub fn prepare(&self, compiler: &startup::Compiler, refinement: RegionRefinement) -> bool {
        let mut ready = true;
        for entry in stages(refinement) {
            let pipeline = &self.pipelines[entry];
            compiler.pipeline(pipeline, startup::BRUSH);
            ready &= pipeline.ready();
        }
        ready
    }
    #[allow(clippy::too_many_arguments)] // Explicit GPU resources and region operands.
    pub fn encode(
        &mut self,
        device: &PipelineDevice,
        encoder: &mut crate::submission::CommandEncoder,
        source: &wgpu::TextureView,
        extent: [u32; 2],
        seed: [u32; 2],
        tolerance: f32,
        selection: Option<&wgpu::Buffer>,
        refinement: RegionRefinement,
    ) -> Result<Region, GpuRasterError> {
        let [w, h] = extent;
        if w == 0
            || h == 0
            || seed[0] >= w
            || seed[1] >= h
            || !tolerance.is_finite()
            || !(0.0..=1.0).contains(&tolerance)
            || !refinement.is_valid()
        {
            return Err(GpuRasterError::InvalidExtent);
        }
        let limit = device.limits();
        if w > limit.max_texture_dimension_2d || h > limit.max_texture_dimension_2d {
            return Err(GpuRasterError::SizeOverflow);
        }
        let bytes = u64::from(w) * u64::from(h) * 4;
        let words = u64::from(w.div_ceil(8)) * u64::from(h);
        let boundaries =
            u64::from(w) * u64::from((h - 1) / 16) + u64::from(h) * u64::from((w - 1) / 16);
        if bytes > limit.max_storage_buffer_binding_size
            || bytes > limit.max_buffer_size
            || words.max(boundaries).div_ceil(64)
                > u64::from(limit.max_compute_workgroups_per_dimension).pow(2)
        {
            return Err(GpuRasterError::SizeOverflow);
        }
        if bytes > self.capacity {
            self.parents = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("flood reusable parents"),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }));
            self.capacity = bytes;
        }
        let mask_bytes = u64::from(w.div_ceil(32)) * u64::from(h) * 4;
        if refinement.needs_mask() && self.mask.size() < mask_bytes {
            self.mask = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("region reusable packed morphology"),
                size: mask_bytes,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
        }
        let region = Region {
            coverage: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("connected coverage"),
                size: (64 + words * 4).next_multiple_of(16),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            bounds_offset: 32 + words * 4,
        };
        let params = [
            w,
            h,
            seed[0],
            seed[1],
            tolerance.to_bits(),
            (refinement.gap_closing as f32).to_bits(),
            (refinement.expansion as f32).to_bits(),
            refinement.smoothing.to_bits(),
        ];
        let data: Vec<_> = params.into_iter().flat_map(u32::to_ne_bytes).collect();
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("flood request"),
            contents: &data,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(source),
        }];
        entries.extend(
            [
                &uniform,
                self.parents.as_ref().unwrap(),
                &region.coverage,
                selection.unwrap_or(&self.empty),
                &self.mask,
            ]
            .into_iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32 + 1,
                resource: b.as_entire_binding(),
            }),
        );
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("flood request"),
            layout: &self.layout,
            entries: &entries,
        });
        // All morphology is bit-packed. The parent allocation doubles as the
        // other ping-pong plane before/after (never during) component labeling.
        // Interactive hosts prepare these recipes on the startup compiler.
        // Headless callers compile only what they use. Disabled refinements
        // keep the original three dispatches and allocate no morphology mask.
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("connected region"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &group, &[]);
        for entry in stages(refinement) {
            pass.set_pipeline(&self.pipelines[entry]);
            if entry == "initialize" {
                pass.dispatch_workgroups(w.div_ceil(16), h.div_ceil(16), 1);
            } else {
                let invocations = match entry {
                    "merge" => boundaries,
                    "pack" => words,
                    _ => mask_bytes / 4,
                };
                dispatch_linear(
                    &mut pass,
                    invocations,
                    limit.max_compute_workgroups_per_dimension,
                );
            }
        }
        Ok(region)
    }
}

fn dispatch_linear(pass: &mut wgpu::ComputePass<'_>, invocations: u64, limit: u32) {
    let groups = invocations.div_ceil(64) as u32;
    pass.dispatch_workgroups(groups.min(limit), groups.div_ceil(limit).max(1), 1);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(r: &WgpuRasterizer, extent: [u32; 2], pixels: &[u8]) -> wgpu::TextureView {
        let texture = r.device.create_texture_with_data(
            &r.queue,
            &wgpu::TextureDescriptor {
                label: Some("flood fixture"),
                size: wgpu::Extent3d {
                    width: extent[0],
                    height: extent[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: COLOR_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            pixels,
        );
        texture.create_view(&Default::default())
    }
    fn read_region(r: &WgpuRasterizer, region: &Region) -> (Vec<u32>, [u32; 5]) {
        let size = region.bounds_offset;
        let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test flood output"),
            size: size + 32,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        encoder.copy_buffer_to_buffer(&region.coverage, 0, &buffer, 0, size + 32);
        let submitted = encoder.submit(&r.queue);
        let (tx, rx) = mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                tx.send(result).unwrap();
            });
        r.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submitted),
                timeout: Some(READBACK_TIMEOUT),
            })
            .unwrap();
        rx.recv().unwrap().unwrap();
        let view = buffer.slice(..).get_mapped_range().unwrap();
        let values: Vec<_> = view
            .chunks_exact(4)
            .map(|v| u32::from_ne_bytes(v.try_into().unwrap()))
            .collect();
        let bounds = values[size as usize / 4..][..5].try_into().unwrap();
        let packed = values[..size as usize / 4].to_vec();
        drop(view);
        buffer.unmap();
        (packed, bounds)
    }

    #[test]
    fn connected_region_matches_independent_flood_oracle() {
        let r = WgpuRasterizer::new_headless().unwrap();
        let mut flood = Flood::new(&r.device);
        for extent in [
            [1_u32, 1],
            [7, 5],
            [16, 16],
            [63, 47],
            [129, 129],
            [259, 17],
        ] {
            let [w, h] = extent;
            for pattern in 0..6 {
                let pixels: Vec<u8> = (0..w * h)
                    .flat_map(|i| {
                        let x = i % w;
                        let y = i / w;
                        let on = match pattern {
                            0 => true,
                            1 => (x + y) % 2 == 0,
                            2 => x % 9 < 5 && y % 7 < 5,
                            3 => y % 2 == 0 || x == if (y / 2) % 2 == 0 { w - 1 } else { 0 },
                            4 => (x * 13 + y * 97 + x * y * 7) % 101 > 35,
                            _ => !(x == w / 2 && y != h.saturating_sub(2)),
                        };
                        if on { [255; 4] } else { [0, 0, 0, 255] }
                    })
                    .collect();
                let source = source(&r, extent, &pixels);
                for seed in [[0, 0], [w / 2, h / 2], [w - 1, h - 1]] {
                    // Independent test-only BFS oracle. Production never uploads or
                    // downloads pixels to run a CPU flood-fill implementation.
                    let mut expected = vec![false; (w * h) as usize];
                    let start = (seed[1] * w + seed[0]) as usize;
                    let color = pixels[start * 4];
                    let mut queue = std::collections::VecDeque::from([start]);
                    expected[start] = true;
                    while let Some(p) = queue.pop_front() {
                        let x = p as u32 % w;
                        let y = p as u32 / w;
                        for [dx, dy] in [[-1, 0], [1, 0], [0, -1], [0, 1]] {
                            let nx = x as i32 + dx;
                            let ny = y as i32 + dy;
                            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                                continue;
                            }
                            let i = ny as usize * w as usize + nx as usize;
                            if !expected[i] && pixels[i * 4] == color {
                                expected[i] = true;
                                queue.push_back(i);
                            }
                        }
                    }
                    let mut encoder =
                        crate::submission::CommandEncoder::new(&r.device, &Default::default());
                    let region = flood
                        .encode(
                            &r.device,
                            &mut encoder,
                            &source,
                            extent,
                            seed,
                            0.,
                            None,
                            Default::default(),
                        )
                        .unwrap();
                    encoder.submit(&r.queue);
                    let (words, bounds) = read_region(&r, &region);
                    assert_eq!(&words[..8], &[0, 0, w, h, 0, 1, 0, 0]);
                    let mut expected_bounds = [w, h, 0, 0, 0];
                    for y in 0..h {
                        for x in 0..w {
                            let word = words[8 + (y * w.div_ceil(8) + x / 8) as usize];
                            let value = (word >> ((x % 8) * 4)) & 15;
                            let expected = expected[(y * w + x) as usize];
                            assert_eq!(
                                value,
                                if expected { 4 } else { 0 },
                                "{extent:?} pattern {pattern} seed {seed:?} pixel {x},{y}"
                            );
                            if expected {
                                expected_bounds[0] = expected_bounds[0].min(x);
                                expected_bounds[1] = expected_bounds[1].min(y);
                                expected_bounds[2] = expected_bounds[2].max(x + 1);
                                expected_bounds[3] = expected_bounds[3].max(y + 1);
                                expected_bounds[4] += 1;
                            }
                        }
                    }
                    assert_eq!(bounds, expected_bounds);
                }
            }
        }
    }

    #[test]
    fn refined_regions_match_independent_pixel_morphology() {
        fn sample(mask: &[bool], [w, h]: [u32; 2], x: i32, y: i32, extend: bool) -> bool {
            if !extend && (x < 0 || y < 0 || x >= w as i32 || y >= h as i32) {
                return false;
            }
            mask[(y.clamp(0, h as i32 - 1) as u32 * w + x.clamp(0, w as i32 - 1) as u32) as usize]
        }
        fn morphology(
            mask: &[bool],
            extent: [u32; 2],
            horizontal: bool,
            erode: bool,
            range: std::ops::RangeInclusive<i32>,
            extend: bool,
        ) -> Vec<bool> {
            (0..mask.len())
                .map(|i| {
                    let (x, y) = (i as i32 % extent[0] as i32, i as i32 / extent[0] as i32);
                    let mut values = range.clone().map(|d| {
                        sample(
                            mask,
                            extent,
                            x + if horizontal { d } else { 0 },
                            y + if horizontal { 0 } else { d },
                            extend,
                        )
                    });
                    if erode {
                        values.all(|v| v)
                    } else {
                        values.any(|v| v)
                    }
                })
                .collect()
        }
        fn component(mask: &[bool], [w, h]: [u32; 2], seed: [u32; 2]) -> Vec<bool> {
            let mut result = vec![false; mask.len()];
            let start = (seed[1] * w + seed[0]) as usize;
            if !mask[start] {
                return result;
            }
            result[start] = true;
            let mut queue = std::collections::VecDeque::from([start]);
            while let Some(i) = queue.pop_front() {
                let (x, y) = (i as i32 % w as i32, i as i32 / w as i32);
                for (xx, yy) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                    if xx < 0 || yy < 0 || xx >= w as i32 || yy >= h as i32 {
                        continue;
                    }
                    let j = (yy as u32 * w + xx as u32) as usize;
                    if mask[j] && !result[j] {
                        result[j] = true;
                        queue.push_back(j);
                    }
                }
            }
            result
        }
        let r = WgpuRasterizer::new_headless().unwrap();
        let mut flood = Flood::new(&r.device);
        for extent in [[1_u32, 17], [17, 9], [33, 19], [97, 81], [259, 35]] {
            let [w, h] = extent;
            let seed = [w / 2, h / 2];
            // A thin rectangular enclosure with a three-pixel opening, plus
            // an inner hole. Odd widths exercise every packed-word boundary.
            let eligible: Vec<_> = (0..w * h)
                .map(|i| {
                    let (x, y) = (i % w, i / w);
                    !(w > 20
                        && ((x == 5 || x == w - 6) && (5..h - 5).contains(&y)
                            || (y == 5 || y == h - 6)
                                && (5..w - 5).contains(&x)
                                && !(y == 5 && x.abs_diff(w / 2) <= 1)
                            || x.abs_diff(w / 3) < 2 && y.abs_diff(h / 2) < 2))
                })
                .collect();
            let pixels: Vec<_> = eligible
                .iter()
                .flat_map(|on| if *on { [255; 4] } else { [0, 0, 0, 255] })
                .collect();
            let source = source(&r, extent, &pixels);
            for (gap, expansion, smoothing) in [
                (0, 0, 0.),
                (1, 0, 0.),
                (3, 0, 0.),
                (4, 0, 0.),
                (32, 0, 0.),
                (0, 1, 0.),
                (0, -1, 0.),
                (0, 32, 0.),
                (0, -32, 0.),
                (0, 0, 1.),
                (0, 0, 0.25),
                (0, 0, 0.75),
                (3, 2, 1.),
                (4, -2, 1.),
            ] {
                let refine = RegionRefinement {
                    gap_closing: gap,
                    expansion,
                    smoothing,
                };
                let mut expected = eligible.clone();
                if gap != 0 {
                    for (horizontal, erode, low, high) in [
                        (true, true, -(gap as i32) / 2, (gap as i32 + 1) / 2),
                        (false, true, -(gap as i32) / 2, (gap as i32 + 1) / 2),
                        (true, false, -(gap as i32 + 1) / 2, gap as i32 / 2),
                        (false, false, -(gap as i32 + 1) / 2, gap as i32 / 2),
                    ] {
                        expected =
                            morphology(&expected, extent, horizontal, erode, low..=high, true);
                    }
                }
                expected = component(&expected, extent, seed);
                if expansion != 0 {
                    for horizontal in [true, false] {
                        expected = morphology(
                            &expected,
                            extent,
                            horizontal,
                            expansion < 0,
                            -expansion.abs()..=expansion.abs(),
                            false,
                        );
                    }
                }
                let mut encoder =
                    crate::submission::CommandEncoder::new(&r.device, &Default::default());
                let region = flood
                    .encode(
                        &r.device,
                        &mut encoder,
                        &source,
                        extent,
                        seed,
                        0.,
                        None,
                        refine,
                    )
                    .unwrap();
                encoder.submit(&r.queue);
                let (words, bounds) = read_region(&r, &region);
                let mut expected_bounds = [w, h, 0, 0, 0];
                for y in 0..h {
                    for x in 0..w {
                        let center = expected[(y * w + x) as usize];
                        let mut samples = u32::from(center) * 4;
                        if smoothing != 0. {
                            samples = 0;
                            for dy in [-1, 1] {
                                for dx in [-1, 1] {
                                    let neighbors = [(0, 0), (dx, 0), (0, dy), (dx, dy)]
                                        .into_iter()
                                        .filter(|(xx, yy)| {
                                            sample(
                                                &expected,
                                                extent,
                                                x as i32 + xx,
                                                y as i32 + yy,
                                                true,
                                            )
                                        })
                                        .count();
                                    samples += u32::from(neighbors > 2 || neighbors == 2 && center);
                                }
                            }
                            samples = if center {
                                samples.max(1)
                            } else {
                                samples.min(3)
                            };
                            samples = (f32::from(center) * 4. * (1. - smoothing)
                                + samples as f32 * smoothing)
                                .round_ties_even() as u32; // WGSL round(), including half-quarter ties.
                        }
                        let actual =
                            words[8 + (y * w.div_ceil(8) + x / 8) as usize] >> ((x % 8) * 4) & 15;
                        assert_eq!(actual, samples, "{extent:?} {refine:?} at {x},{y}");
                        if samples != 0 {
                            expected_bounds[0] = expected_bounds[0].min(x);
                            expected_bounds[1] = expected_bounds[1].min(y);
                            expected_bounds[2] = expected_bounds[2].max(x + 1);
                            expected_bounds[3] = expected_bounds[3].max(y + 1);
                            expected_bounds[4] += 1;
                        }
                    }
                }
                assert_eq!(bounds, expected_bounds);
                if w == 97 && (3..5).contains(&gap) && expansion == 0 {
                    assert_eq!(words[8], 0, "closed gap cannot leak into outside corner");
                    assert!(bounds[4] > 100, "must still fill the intended interior");
                }
            }
        }
    }

    #[test]
    fn tolerance_is_seed_relative_and_includes_transparency() {
        let r = WgpuRasterizer::new_headless().unwrap();
        let mut flood = Flood::new(&r.device);
        for pixels in [
            vec![0, 0, 0, 255, 2, 0, 0, 255, 4, 0, 0, 255, 2, 0, 0, 255],
            vec![255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 32, 0, 0, 0, 0],
        ] {
            let source = source(&r, [4, 1], &pixels);
            for (tolerance, expected, count) in [(0.1, 0x44, 2), (0.2, 0x4444, 4)] {
                let mut encoder =
                    crate::submission::CommandEncoder::new(&r.device, &Default::default());
                let region = flood
                    .encode(
                        &r.device,
                        &mut encoder,
                        &source,
                        [4, 1],
                        [0, 0],
                        tolerance,
                        None,
                        Default::default(),
                    )
                    .unwrap();
                encoder.submit(&r.queue);
                let (coverage, bounds) = read_region(&r, &region);
                assert_eq!(coverage[8], expected);
                assert_eq!(bounds, [0, 0, count, 1, count]);
            }
        }
    }

    #[test]
    fn queued_regions_keep_independent_results_while_reusing_scratch() {
        let r = WgpuRasterizer::new_headless().unwrap();
        let mut flood = Flood::new(&r.device);
        let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
        let mut regions = Vec::new();
        for (extent, refinement) in [
            ([65, 49], RegionRefinement::default()),
            (
                [19, 17],
                RegionRefinement {
                    gap_closing: 4,
                    expansion: 2,
                    smoothing: 1.,
                },
            ),
            (
                [65, 49],
                RegionRefinement {
                    gap_closing: 3,
                    expansion: 0,
                    smoothing: 0.75,
                },
            ),
            ([19, 17], RegionRefinement::default()),
        ] {
            let pixels = [255; 4].repeat((extent[0] * extent[1]) as usize);
            let source = source(&r, extent, &pixels);
            let region = flood
                .encode(
                    &r.device,
                    &mut encoder,
                    &source,
                    extent,
                    [0, 0],
                    0.,
                    None,
                    refinement,
                )
                .unwrap();
            assert_eq!(flood.capacity, 65 * 49 * 4);
            regions.push((region, extent));
        }
        encoder.submit(&r.queue);
        for (region, [w, h]) in regions {
            let (coverage, bounds) = read_region(&r, &region);
            assert_eq!(bounds, [0, 0, w, h, w * h]);
            assert_eq!(&coverage[..8], &[0, 0, w, h, 0, 1, 0, 0]);
            for y in 0..h {
                for x in 0..w {
                    assert_eq!(
                        (coverage[8 + (y * w.div_ceil(8) + x / 8) as usize] >> (x % 8 * 4)) & 15,
                        4
                    );
                }
            }
        }
    }

    #[test]
    fn invalid_region_requests_do_not_allocate_scratch() {
        let r = WgpuRasterizer::new_headless().unwrap();
        let source = source(&r, [1, 1], &[255; 4]);
        let mut flood = Flood::new(&r.device);
        for (extent, seed, tolerance) in [
            ([0, 1], [0, 0], 0.),
            ([1, 1], [1, 0], 0.),
            ([1, 1], [0, 1], 0.),
            ([1, 1], [0, 0], f32::NAN),
            ([1, 1], [0, 0], -0.1),
            ([1, 1], [0, 0], 1.1),
            ([u32::MAX, 1], [0, 0], 0.),
            ([u32::MAX, u32::MAX], [0, 0], 0.),
        ] {
            let mut encoder =
                crate::submission::CommandEncoder::new(&r.device, &Default::default());
            assert!(
                flood
                    .encode(
                        &r.device,
                        &mut encoder,
                        &source,
                        extent,
                        seed,
                        tolerance,
                        None,
                        Default::default(),
                    )
                    .is_err()
            );
            assert_eq!(flood.capacity, 0);
            assert!(flood.parents.is_none());
        }
        for refinement in [
            RegionRefinement {
                gap_closing: u32::MAX,
                ..Default::default()
            },
            RegionRefinement {
                expansion: i32::MIN,
                ..Default::default()
            },
            RegionRefinement {
                expansion: 33,
                ..Default::default()
            },
            RegionRefinement {
                smoothing: f32::NAN,
                ..Default::default()
            },
            RegionRefinement {
                smoothing: -0.1,
                ..Default::default()
            },
            RegionRefinement {
                smoothing: 1.1,
                ..Default::default()
            },
        ] {
            let mut encoder =
                crate::submission::CommandEncoder::new(&r.device, &Default::default());
            assert!(
                flood
                    .encode(
                        &r.device,
                        &mut encoder,
                        &source,
                        [1, 1],
                        [0, 0],
                        0.,
                        None,
                        refinement
                    )
                    .is_err()
            );
            assert_eq!(flood.capacity, 0);
            assert_eq!(flood.mask.size(), 4);
            assert!(flood.pipelines().all(|p| !p.ready()));
        }
    }

    #[test]
    #[ignore = "hardware GPU connected-region benchmark; release, serial"]
    fn connected_region_latency() {
        let r = WgpuRasterizer::new_headless().unwrap();
        let mut flood = Flood::new(&r.device);
        let extent = [2048, 1536];
        for (pattern, gap_closing, expansion, smoothing) in [
            ("solid", 0, 0, 0.),
            ("linework", 0, 0, 0.),
            ("maze", 0, 0, 0.),
            ("noise", 0, 0, 0.),
            ("linework", 4, 0, 0.),
            ("linework", 0, 4, 0.),
            ("linework", 0, 0, 1.),
            ("linework", 4, 2, 1.),
            ("linework", 32, 32, 1.),
            ("linework", 0, -32, 1.),
        ] {
            let refinement = RegionRefinement {
                gap_closing,
                expansion,
                smoothing,
            };
            let pixels: Vec<u8> = (0..extent[0] * extent[1])
                .flat_map(|i| {
                    let x = i % extent[0];
                    let y = i / extent[0];
                    let on = match pattern {
                        "solid" => true,
                        "linework" => x % 200 != 100 && y % 150 != 75,
                        "maze" => {
                            y % 2 == 0 || x == if (y / 2) % 2 == 0 { extent[0] - 1 } else { 0 }
                        }
                        _ => (x * 13 + y * 97 + x * y * 7) % 101 > 35,
                    };
                    if on { [255; 4] } else { [0, 0, 0, 255] }
                })
                .collect();
            let source = source(&r, extent, &pixels);
            let mut timing = telemetry::Telemetry::new(&r.device, &r.queue);
            timing.enabled = true;
            let mut completed = layer_render::TimingSamples::default();
            for i in 0..150 {
                let start = std::time::Instant::now();
                let mut encoder =
                    crate::submission::CommandEncoder::new(&r.device, &Default::default());
                timing.begin(&r.device, &mut encoder);
                let region = flood
                    .encode(
                        &r.device,
                        &mut encoder,
                        &source,
                        extent,
                        [0, 0],
                        0.,
                        None,
                        refinement,
                    )
                    .unwrap();
                timing.end(&mut encoder);
                let submitted = encoder.submit(&r.queue);
                timing.submitted();
                timing.cpu.push(start.elapsed().as_secs_f32() * 1000.);
                r.device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submitted),
                        timeout: Some(READBACK_TIMEOUT),
                    })
                    .unwrap();
                let elapsed = start.elapsed().as_secs_f32() * 1000.;
                completed.push(elapsed);
                if i == 0 {
                    let (_, bounds) = read_region(&r, &region);
                    eprintln!(
                        "{pattern} {refinement:?}: first completion {elapsed:.3}ms; bounds/count {bounds:?}"
                    );
                }
            }
            let stats = timing.snapshot();
            for (name, samples) in [
                ("CPU", &stats.cpu),
                ("GPU", &stats.gpu),
                ("Completed", &completed),
            ] {
                let mut values = samples.ordered();
                values.sort_by(f32::total_cmp);
                assert_eq!(values.len(), 120, "{name} timing samples unavailable");
                eprintln!(
                    "{pattern} {refinement:?} {name} median/p95/p99: {:.3}/{:.3}/{:.3}ms",
                    values[59], values[113], values[118]
                );
            }
        }
    }
}
