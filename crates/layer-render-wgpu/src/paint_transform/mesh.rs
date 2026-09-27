//! Warp mesh geometry for the transform pass: the tessellated patches with a
//! skirt beyond their edges, their quads binned by destination page so region
//! jobs can bound the source each region samples, and their triangles grouped
//! by the positions window they reach.
use super::*;
use crate::submission::ColorPass;
use std::ops::Range;

/// Destination tolerance of the tessellated surface, in pixels.
const TOLERANCE: f32 = 0.5;
/// Destination pages per side of a positions window.
pub(crate) const WINDOW_PAGES: u32 = 4;
/// Most cells one binning of the destination may hold.
const BIN_CELLS: u64 = 1 << 16;

pub(crate) struct MeshGeometry {
    /// Destination x, y and source x, y of each vertex, row by row with one
    /// more vertex per row than `quads[0]`.
    pub vertices: Vec<[f32; 4]>,
    quads: [u32; 2],
    /// Destination bounds of each quad, as min x, min y, max x, max y.
    destination: Vec<[f32; 4]>,
    pages: Bins,
    windows: Option<Bins>,
    /// Triangles of every window in turn, or of the whole mesh when the
    /// windows are too many to bin.
    indices: Vec<u32>,
}

impl MeshGeometry {
    /// The mesh within TOLERANCE destination pixels, or within `display`
    /// pixels for a display level drawn whole, which neither bounds regions
    /// nor bins windows.
    pub fn new(mesh: &layer_core::MeshMap, display: Option<f32>) -> Self {
        let (vertices, quads) = Self::surface(mesh, display.map_or(TOLERANCE, |t| t.max(TOLERANCE)));
        let width = quads[0] + 1;
        let rows = if display.is_some() { 0 } else { quads[1] };
        let mut destination = Vec::with_capacity(quads[0] * rows);
        for j in 0..rows {
            for i in 0..quads[0] {
                let a = j * width + i;
                destination.push([a, a + 1, a + width, a + width + 1].iter().fold(
                    [
                        f32::INFINITY,
                        f32::INFINITY,
                        f32::NEG_INFINITY,
                        f32::NEG_INFINITY,
                    ],
                    |b, v| {
                        let [x, y, ..] = vertices[*v];
                        [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)]
                    },
                ));
            }
        }
        let pages = (PAGE_SIZE.trailing_zeros()..u32::BITS)
            .find_map(|shift| Bins::new(&destination, 0., shift))
            .unwrap_or_default();
        let window_shift = (WINDOW_PAGES * PAGE_SIZE).trailing_zeros();
        let windows = display.is_none().then(|| Bins::new(&destination, 1., window_shift)).flatten();
        let quads = quads.map(|n| n as u32);
        let mut geometry = Self {
            vertices,
            quads,
            destination,
            pages,
            windows,
            indices: Vec::new(),
        };
        geometry.indices = match &geometry.windows {
            Some(windows) => windows
                .items
                .iter()
                .flat_map(|q| geometry.quad(*q).into_iter().flatten())
                .collect(),
            None => geometry.triangles().flatten().collect(),
        };
        geometry
    }

    /// Vertices of the tessellated surface with its skirt, and its quads per
    /// row and column.
    fn surface(mesh: &layer_core::MeshMap, tolerance: f32) -> (Vec<[f32; 4]>, [usize; 2]) {
        let tessellation = mesh.tessellate(tolerance);
        let [columns, rows] = tessellation.grid.map(|n| n as usize);
        // A ring of vertices extrapolated beyond each edge carries source
        // positions just outside the source, so the filter softens the edge.
        let width = columns + 3;
        let height = rows + 3;
        let mut vertices = vec![[0f32; 4]; width * height];
        for (j, (positions, sources)) in tessellation
            .positions
            .chunks_exact(columns + 1)
            .zip(tessellation.sources.chunks_exact(columns + 1))
            .enumerate()
        {
            let row = &mut vertices[(j + 1) * width + 1..][..=columns];
            for ((vertex, p), s) in row.iter_mut().zip(positions).zip(sources) {
                *vertex = [p.x, p.y, s.x, s.y];
            }
        }
        let extrude = |edge: [f32; 4], inner: [f32; 4]| {
            let d: [f32; 4] = std::array::from_fn(|n| edge[n] - inner[n]);
            let [dest, source] = [d[0].hypot(d[1]), d[2].hypot(d[3])];
            let reach = layer_core::MeshMap::SKIRT;
            let k = (reach / dest.max(1e-6))
                .max(reach / source.max(1e-6))
                .min(1e3);
            std::array::from_fn(|n| edge[n] + d[n] * k)
        };
        for j in 1..height - 1 {
            vertices[j * width] = extrude(vertices[j * width + 1], vertices[j * width + 2]);
            vertices[j * width + width - 1] = extrude(
                vertices[j * width + width - 2],
                vertices[j * width + width - 3],
            );
        }
        for i in 0..width {
            vertices[i] = extrude(vertices[width + i], vertices[2 * width + i]);
            vertices[(height - 1) * width + i] = extrude(
                vertices[(height - 2) * width + i],
                vertices[(height - 3) * width + i],
            );
        }
        (vertices, [width - 1, height - 1])
    }

    fn quad(&self, quad: u32) -> [[u32; 3]; 2] {
        let width = self.quads[0] + 1;
        let a = quad / self.quads[0] * width + quad % self.quads[0];
        let [b, c, d] = [a + 1, a + width, a + width + 1];
        [[a, b, d], [a, d, c]]
    }

    /// Every triangle in drawing order: later triangles cover earlier ones.
    pub fn triangles(&self) -> impl Iterator<Item = [u32; 3]> + '_ {
        (0..self.quads[0] * self.quads[1]).flat_map(|q| self.quad(q))
    }

    /// Index range of the triangles reaching the window whose first page is
    /// `page`, or its one-pixel border.
    fn window(&self, page: [u32; 2]) -> Range<u32> {
        let Some(windows) = &self.windows else {
            return 0..self.indices.len() as u32;
        };
        let items = windows.cell(page.map(|c| c / WINDOW_PAGES));
        items.start * 6..items.end * 6
    }

    /// Source bounds of the triangles' parts within a destination pixel of
    /// the region, or None when no triangle reaches it. Positions interpolate
    /// linearly across each triangle, so its clipped corners bound them.
    pub fn footprint(&self, region: PixelRect) -> Option<[f64; 4]> {
        let near = [
            region.min_x() as f32 - 1.,
            region.min_y() as f32 - 1.,
            region.max_x() as f32 + 1.,
            region.max_y() as f32 + 1.,
        ];
        let mut found = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for quad in self.pages.reaching(near) {
            let dest = self.destination[quad as usize];
            if dest[0] > near[2] || dest[2] < near[0] || dest[1] > near[3] || dest[3] < near[1] {
                continue;
            }
            for triangle in self.quad(quad) {
                let (corners, count) = clip(triangle.map(|v| self.vertices[v as usize]), near);
                for v in &corners[..count] {
                    found = [
                        found[0].min(v[2]),
                        found[1].min(v[3]),
                        found[2].max(v[2]),
                        found[3].max(v[3]),
                    ];
                }
            }
        }
        (found[0] <= found[2]).then(|| found.map(f64::from))
    }
}

/// The part of a triangle of destination x, y and source x, y vertices within
/// `rect` in destination space, with sources interpolated along cut edges. A
/// triangle clipped by four edges has at most seven corners.
fn clip(triangle: [[f32; 4]; 3], rect: [f32; 4]) -> ([[f32; 4]; 7], usize) {
    let mut polygon = [[0f32; 4]; 7];
    polygon[..3].copy_from_slice(&triangle);
    let mut count = 3;
    for (axis, edge, sign) in [
        (0, rect[0], 1.),
        (1, rect[1], 1.),
        (0, rect[2], -1.),
        (1, rect[3], -1.),
    ] {
        let inside = |v: &[f32; 4]| (v[axis] - edge) * sign >= 0.;
        let mut clipped = [[0f32; 4]; 7];
        let mut kept = 0;
        for n in 0..count {
            let [previous, current] = [polygon[(n + count - 1) % count], polygon[n]];
            if inside(&current) != inside(&previous) && kept < 7 {
                let t = (edge - previous[axis]) / (current[axis] - previous[axis]);
                clipped[kept] =
                    std::array::from_fn(|k| previous[k] + (current[k] - previous[k]) * t);
                kept += 1;
            }
            if inside(&current) && kept < 7 {
                clipped[kept] = current;
                kept += 1;
            }
        }
        (polygon, count) = (clipped, kept);
        if count == 0 {
            break;
        }
    }
    (polygon, count)
}

/// Items binned by the square destination cells, `1 << shift` pixels wide,
/// that their bounds reach; `items[starts[c]..starts[c + 1]]` lie in cell `c`,
/// counted row by row from `first`.
#[derive(Default)]
struct Bins {
    shift: u32,
    first: [u32; 2],
    size: [u32; 2],
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl Bins {
    /// Bins for `bounds` widened by `margin`, or None when they reach more
    /// than BIN_CELLS cells.
    fn new(bounds: &[[f32; 4]], margin: f32, shift: u32) -> Option<Self> {
        let cells = |b: &[f32; 4]| {
            let b = [b[0] - margin, b[1] - margin, b[2] + margin, b[3] + margin];
            (b.iter().all(|v| v.is_finite()) && b[2] >= 0. && b[3] >= 0.)
                .then(|| b.map(|v| (v.max(0.) as u32) >> shift))
        };
        let mut first = [u32::MAX; 2];
        let mut last = [0; 2];
        for [x0, y0, x1, y1] in bounds.iter().filter_map(cells) {
            first = [first[0].min(x0), first[1].min(y0)];
            last = [last[0].max(x1), last[1].max(y1)];
        }
        if first[0] > last[0] {
            return Some(Self {
                shift,
                starts: vec![0],
                ..Default::default()
            });
        }
        let size = [last[0] - first[0] + 1, last[1] - first[1] + 1];
        if u64::from(size[0]) * u64::from(size[1]) > BIN_CELLS {
            return None;
        }
        let index = |x: u32, y: u32| ((y - first[1]) * size[0] + x - first[0]) as usize;
        let mut starts = vec![0u32; (size[0] * size[1]) as usize + 1];
        for [x0, y0, x1, y1] in bounds.iter().filter_map(cells) {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    starts[index(x, y) + 1] += 1;
                }
            }
        }
        for c in 1..starts.len() {
            starts[c] += starts[c - 1];
        }
        let mut next = starts.clone();
        let mut items = vec![0; *starts.last().unwrap() as usize];
        for (item, reach) in bounds.iter().map(cells).enumerate() {
            let Some([x0, y0, x1, y1]) = reach else {
                continue;
            };
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let slot = &mut next[index(x, y)];
                    items[*slot as usize] = item as u32;
                    *slot += 1;
                }
            }
        }
        Some(Self {
            shift,
            first,
            size,
            starts,
            items,
        })
    }

    /// Positions in `items` of the items in cell `c`.
    fn cell(&self, c: [u32; 2]) -> Range<u32> {
        let [x, y] = [
            c[0].wrapping_sub(self.first[0]),
            c[1].wrapping_sub(self.first[1]),
        ];
        if x >= self.size[0] || y >= self.size[1] {
            return 0..0;
        }
        let index = (y * self.size[0] + x) as usize;
        self.starts[index]..self.starts[index + 1]
    }

    /// Items in the cells `rect` reaches; an item in several cells repeats.
    fn reaching(&self, rect: [f32; 4]) -> impl Iterator<Item = u32> + '_ {
        let [x0, y0, x1, y1] = rect.map(|v| (v.max(0.) as u32) >> self.shift);
        let end = [self.first[0] + self.size[0], self.first[1] + self.size[1]];
        let columns = x0.max(self.first[0])..x1.saturating_add(1).min(end[0]);
        (y0.max(self.first[1])..y1.saturating_add(1).min(end[1])).flat_map(move |y| {
            columns.clone().flat_map(move |x| {
                let items = self.cell([x, y]);
                self.items[items.start as usize..items.end as usize]
                    .iter()
                    .copied()
            })
        })
    }
}

/// Window origin and size, and the rows mapping destination pixels into it
/// with the scale of the source positions.
const WINDOW_BYTES: u64 = 48;

/// Rasterizes a mesh's source positions for one window of destination pages,
/// with a one-pixel border so the transform pass can difference neighbors.
pub(crate) struct Positions {
    pub pipeline: Deferred<wgpu::RenderPipeline>,
    layout: wgpu::BindGroupLayout,
    window: Option<(wgpu::Buffer, wgpu::BindGroup)>,
    target: Option<(wgpu::Texture, wgpu::TextureView)>,
    vertices: Option<wgpu::Buffer>,
    indices: Option<wgpu::Buffer>,
    uploaded: Option<Arc<MeshGeometry>>,
    bytes: Vec<u8>,
}
impl Positions {
    pub fn new(device: &PipelineDevice) -> Self {
        let layout = crate::bindings::layout(device, "mesh position window", &[crate::bindings::buffer(
            0,
            wgpu::ShaderStages::VERTEX,
            wgpu::BufferBindingType::Uniform,
            false,
            wgpu::BufferSize::new(WINDOW_BYTES),
        )]);
        let (compile_device, parameters) = (device.clone(), layout.clone());
        let pipeline = Deferred::pipeline(move |mode| {
            let device = &compile_device;
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("mesh source positions"),
                source: wgpu::ShaderSource::Wgsl(include_str!("../mesh_positions.wgsl").into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("mesh source positions"),
                bind_group_layouts: &[Some(&parameters)],
                immediate_size: 0,
            });
            mode.render(
                device,
                &wgpu::RenderPipelineDescriptor {
                    label: Some("mesh source positions"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vertex_main"),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: 16,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                        })],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fragment_main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: pixel_transform::POSITIONS_FORMAT,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                },
            )
        });
        Self {
            pipeline,
            layout,
            window: None,
            target: None,
            vertices: None,
            indices: None,
            uploaded: None,
            bytes: Vec::new(),
        }
    }
    pub fn fork(&self) -> Self {
        Self {
            pipeline: self.pipeline.clone(),
            layout: self.layout.clone(),
            window: None,
            target: None,
            vertices: None,
            indices: None,
            uploaded: None,
            bytes: Vec::new(),
        }
    }
    /// Positions drawn with the transforms' pipeline.
    pub fn sharing(transforms: &PaintTransforms) -> Self {
        transforms.0[0].positions.fork()
    }
    pub fn storage_bytes(&self) -> u64 {
        self.target.as_ref().map_or(0, |(t, _)| texture_bytes(t))
            + self.vertices.as_ref().map_or(0, wgpu::Buffer::size)
            + self.indices.as_ref().map_or(0, wgpu::Buffer::size)
            + self.window.as_ref().map_or(0, |(b, _)| b.size())
    }
    /// A positions texture for windows of at least `size` destination pixels.
    pub fn view(&mut self, device: &wgpu::Device, size: [u32; 2]) -> wgpu::TextureView {
        let held = self.target.as_ref().map_or([0; 2], |(t, _)| [t.width(), t.height()]);
        let extent = [0, 1].map(|axis| held[axis].max(size[axis] + 2));
        if extent != held {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("mesh source positions"),
                size: wgpu::Extent3d {
                    width: extent[0],
                    height: extent[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: pixel_transform::POSITIONS_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.target = Some((texture, view));
        }
        self.target.as_ref().unwrap().1.clone()
    }
    /// Upload the geometry drawn by later windows, unless it is already there.
    pub fn upload(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        geometry: &Arc<MeshGeometry>,
    ) -> Result<(), GpuRasterError> {
        if self
            .uploaded
            .as_ref()
            .is_some_and(|g| Arc::ptr_eq(g, geometry))
        {
            return Ok(());
        }
        self.uploaded = None;
        for (buffer, (floats, integers), usage, label) in [
            (
                &mut self.vertices,
                (geometry.vertices.as_flattened(), &[][..]),
                wgpu::BufferUsages::VERTEX,
                "mesh vertices",
            ),
            (
                &mut self.indices,
                (&[][..], &geometry.indices[..]),
                wgpu::BufferUsages::INDEX,
                "mesh triangles",
            ),
        ] {
            let bytes = &mut self.bytes;
            bytes.clear();
            bytes.extend(floats.iter().flat_map(|v| v.to_le_bytes()));
            bytes.extend(integers.iter().flat_map(|v| v.to_le_bytes()));
            if bytes.is_empty() {
                continue;
            }
            if buffer
                .as_ref()
                .is_none_or(|b| b.size() < bytes.len() as u64)
            {
                *buffer = Some(r.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: (bytes.len() as u64).next_power_of_two().max(256),
                    usage: usage | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
            }
            r.uploads.write(encoder, buffer.as_ref().unwrap(), bytes)?;
        }
        self.uploaded = Some(geometry.clone());
        Ok(())
    }
    /// Rasterize the uploaded mesh for the window whose first page is `page`.
    pub fn draw(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        page: [u32; 2],
    ) -> Result<(), GpuRasterError> {
        let triangles = self.uploaded.as_ref().map_or(0..0, |g| g.window(page));
        let origin = page.map(|v| (v * PAGE_SIZE) as f32 - 1.);
        self.draw_window(r, encoder, triangles, origin, layer_core::Affine::IDENTITY, 1.)
    }
    /// Rasterize the whole uploaded mesh at display texels, whose first is
    /// `origin` beyond the border: `to_texels` maps its destination pixels to
    /// them and `source_scale` its source pixels to the texels stored.
    pub fn draw_display(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        origin: [f32; 2],
        to_texels: layer_core::Affine,
        source_scale: f32,
    ) -> Result<(), GpuRasterError> {
        let triangles = 0..self.uploaded.as_ref().map_or(0, |g| g.indices.len() as u32);
        self.draw_window(r, encoder, triangles, origin, to_texels, source_scale)
    }
    fn draw_window(
        &mut self,
        r: &mut WgpuRasterizer,
        encoder: &mut crate::submission::CommandEncoder,
        triangles: Range<u32>,
        origin: [f32; 2],
        to_window: layer_core::Affine,
        source_scale: f32,
    ) -> Result<(), GpuRasterError> {
        let (texture, view) = self.target.as_ref().unwrap();
        let [a, b, c, d, e, f] = to_window.0;
        let size = [texture.width() as f32, texture.height() as f32];
        let mut window = [0u8; WINDOW_BYTES as usize];
        for (dst, value) in window
            .chunks_exact_mut(4)
            .zip([origin[0], origin[1], size[0], size[1], a, c, e, source_scale, b, d, f, 0.])
        {
            dst.copy_from_slice(&value.to_le_bytes());
        }
        let (buffer, binding) = &*self.window.get_or_insert_with(|| {
            let buffer = r.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mesh position window"),
                size: WINDOW_BYTES,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let binding = crate::bindings::group(&r.device, "mesh position window", &self.layout, [buffer.as_entire_binding()]);
            (buffer, binding)
        });
        r.uploads.write(encoder, buffer, &window)?;
        let uncovered = wgpu::Color { r: pixel_transform::UNCOVERED, g: pixel_transform::UNCOVERED, b: 0., a: 0. };
        let mut pass = encoder.color_pass("mesh source positions", view, wgpu::LoadOp::Clear(uncovered));
        if !triangles.is_empty() {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, binding, &[]);
            pass.set_vertex_buffer(0, self.vertices.as_ref().unwrap().slice(..));
            pass.set_index_buffer(
                self.indices.as_ref().unwrap().slice(..),
                wgpu::IndexFormat::Uint32,
            );
            pass.draw_indexed(triangles, 0, 0..1);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{Affine, MeshMap, Point, Rect};

    fn warped(offset: f32) -> MeshGeometry {
        let bounds = Rect {
            min: Point {
                x: 100. + offset,
                y: 50. + offset,
            },
            max: Point {
                x: 2900. + offset,
                y: 2100. + offset,
            },
        };
        let center = Point {
            x: 1500. + offset,
            y: 1075. + offset,
        };
        let mesh = MeshMap::from_affine(
            bounds,
            [4, 4],
            Affine::around(center, [1.1, 0.9], 0.3, Point::default()),
        )
        .unwrap()
        .move_node(6, Point { x: 700., y: -350. })
        .unwrap()
        .move_tangent(
            12,
            0,
            Point {
                x: 2200. + offset,
                y: 1500. + offset,
            },
        )
        .unwrap();
        MeshGeometry::new(&mesh, None)
    }

    fn bounds(g: &MeshGeometry, triangle: [u32; 3]) -> [f32; 4] {
        triangle.iter().fold(
            [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ],
            |b, v| {
                let v = g.vertices[*v as usize];
                [
                    b[0].min(v[0]),
                    b[1].min(v[1]),
                    b[2].max(v[0]),
                    b[3].max(v[1]),
                ]
            },
        )
    }

    #[test]
    fn each_window_draws_the_triangles_reaching_it_in_drawing_order() {
        let g = warped(0.);
        assert!(g.windows.is_some());
        let order: std::collections::HashMap<[u32; 3], usize> =
            g.triangles().enumerate().map(|(n, t)| (t, n)).collect();
        let span = (WINDOW_PAGES * PAGE_SIZE) as f32;
        let mut drawn_somewhere = std::collections::HashSet::new();
        for wy in 0..4 {
            for wx in 0..5 {
                let range = g.window([wx * WINDOW_PAGES, wy * WINDOW_PAGES]);
                let drawn: Vec<[u32; 3]> = g.indices[range.start as usize..range.end as usize]
                    .chunks_exact(3)
                    .map(|t| [t[0], t[1], t[2]])
                    .collect();
                let positions: Vec<usize> = drawn.iter().map(|t| order[t]).collect();
                assert!(
                    positions.windows(2).all(|p| p[0] < p[1]),
                    "window {wx},{wy} keeps drawing order"
                );
                let window = [
                    wx as f32 * span - 1.,
                    wy as f32 * span - 1.,
                    (wx + 1) as f32 * span + 1.,
                    (wy + 1) as f32 * span + 1.,
                ];
                for triangle in g.triangles() {
                    let b = bounds(&g, triangle);
                    if b[0] < window[2] && b[2] > window[0] && b[1] < window[3] && b[3] > window[1]
                    {
                        assert!(
                            drawn.contains(&triangle),
                            "window {wx},{wy} draws {triangle:?}"
                        );
                    }
                }
                drawn_somewhere.extend(drawn);
            }
        }
        assert!(g.triangles().all(|t| {
            let b = bounds(&g, t);
            drawn_somewhere.contains(&t) || b[2] <= -1. || b[3] <= -1.
        }));
    }

    #[test]
    fn footprints_bound_the_source_sampled_in_the_region() {
        let g = warped(0.);
        for (x, y, w, h) in [
            (0, 0, 256, 256),
            (512, 768, 512, 256),
            (1800, 1900, 37, 300),
            (1111, 777, 1, 1),
            (5000, 5000, 64, 64),
        ] {
            let region = PixelRect::new(x, y, x + w, y + h);
            let footprint = g.footprint(region);
            let mut sampled = 0;
            for j in 0..=16 {
                for i in 0..=16 {
                    let p = [
                        x as f32 - 1. + (w + 2) as f32 * i as f32 / 16.,
                        y as f32 - 1. + (h + 2) as f32 * j as f32 / 16.,
                    ];
                    for [a, b, c] in g.triangles().map(|t| t.map(|v| g.vertices[v as usize])) {
                        let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
                        if area.abs() < 1e-3 {
                            continue;
                        }
                        let u =
                            ((p[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (p[1] - a[1])) / area;
                        let v =
                            ((b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1])) / area;
                        if u < 0. || v < 0. || u + v > 1. {
                            continue;
                        }
                        let source = [0, 1].map(|k| {
                            a[k + 2] + (b[k + 2] - a[k + 2]) * u + (c[k + 2] - a[k + 2]) * v
                        });
                        let f = footprint.expect("a sampled region has a footprint");
                        assert!(
                            f[0] - 0.05 <= source[0] as f64
                                && source[0] as f64 <= f[2] + 0.05
                                && f[1] - 0.05 <= source[1] as f64
                                && source[1] as f64 <= f[3] + 0.05,
                            "{source:?} sampled at {p:?} lies in {f:?}"
                        );
                        sampled += 1;
                    }
                }
            }
            assert_eq!(footprint.is_some(), sampled > 0, "region {x},{y}");
        }
    }

    #[test]
    fn a_pixel_on_a_patch_seam_reads_only_its_neighborhood() {
        let bounds = Rect {
            min: Point { x: 0., y: 0. },
            max: Point { x: 2048., y: 1536. },
        };
        let mesh = MeshMap::identity(bounds, [3, 3]).unwrap();
        let g = MeshGeometry::new(&mesh, None);
        let f = g.footprint(PixelRect::new(681, 0, 682, 1)).unwrap();
        for (actual, expected) in f.into_iter().zip([680., -1., 683., 2.]) {
            assert!((actual - expected).abs() < 0.01, "{f:?}");
        }
    }

    #[test]
    fn positions_grow_and_keep_their_texture_for_smaller_windows() {
        let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut positions = Positions::new(&r.device);
        let first = positions.view(&r.device, [300, 200]);
        for size in [[120, 90], [300, 200], [298, 3], [1, 200]] {
            assert!(positions.view(&r.device, size) == first, "{size:?} reuses the texture");
        }
        let grown = positions.view(&r.device, [310, 150]);
        let texture = &positions.target.as_ref().unwrap().0;
        assert_eq!([texture.width(), texture.height()], [312, 202]);
        assert!(grown != first && positions.view(&r.device, [310, 200]) == grown);
    }

    #[test]
    fn meshes_beyond_the_binnable_windows_draw_whole() {
        let bounds = Rect {
            min: Point { x: 100., y: 50. },
            max: Point { x: 2900., y: 2100. },
        };
        let far = Point { x: 3.0e8, y: 3.0e8 };
        let mesh = MeshMap::identity(bounds, [3, 3])
            .unwrap()
            .move_node(15, far)
            .unwrap();
        let g = MeshGeometry::new(&mesh, None);
        assert!(g.windows.is_none());
        assert_eq!(g.window([0, 0]), 0..g.indices.len() as u32);
        assert_eq!(g.indices.len(), g.triangles().count() * 3);
        assert!(g.footprint(PixelRect::new(0, 0, 256, 256)).is_some());
    }
}
