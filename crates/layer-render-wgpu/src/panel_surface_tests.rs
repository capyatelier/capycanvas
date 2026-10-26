use super::{PanelSurface, PanelSurfaceGroup};
use crate::{ViewportPresenter, WgpuRasterizer};
use crate::test_support::{packet, view};
use layer_core::authored::{Artwork, SceneIndex, SceneView};
use layer_render::{CanvasRenderer, FramePacket, ViewState};
use std::sync::Arc;

#[path = "panel_geometry_differential.rs"]
mod vector_geometry;

const SIZE: [u32; 2] = [256, 224];
const PLANE: f32 = 96.;
const WHITE: [f32; 4] = [1.; 4];

fn camera(size: [u32; 2]) -> ViewState {
    ViewState { document_to_surface: [1., 0., 0., 1., -4096., -4096.], ..view(size) }
}

fn renderer() -> WgpuRasterizer {
    let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    assert_ne!(renderer.adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    let mut artwork = Artwork::new([8; 2]).unwrap();
    crate::test_support::add_paint(&mut artwork, "Panel background", [8; 2]);
    let index = Arc::new(SceneIndex::build(&artwork).unwrap());
    renderer.submit(FramePacket {
        view: camera(SIZE), reset_layers: true,
        ..packet(SceneView::new(&artwork, &index), [8; 2])
    }).unwrap();
    renderer
}

fn group(bounds: [f32; 4]) -> PanelSurfaceGroup {
    PanelSurfaceGroup { bounds, radii: [18.; 4], tab_bar: [0.; 4], active_tab: [0.; 4],
        tab_clip: [0.; 4], color: WHITE, strip_color: WHITE }
}

fn surface(gap: f32) -> PanelSurface {
    PanelSurface { sidebar: [54., 8., 36., 208.], sidebar_radii: [18.; 4],
        plane: PLANE, bridge_width: 6., direction: 1., scale: 1., color: WHITE,
        groups: vec![group([PLANE, 24., 144., gap - 24.]), group([PLANE, gap + 6., 144., 194. - gap])],
        anchors: vec![[54., 80., 36., 48.]] }
}

fn tabbed_receiver_column() -> PanelSurface {
    let mut surface = surface(104.);
    surface.color = [0.6, 0.6, 0.6, 1.];
    surface.groups = vec![group([PLANE, 24., 144., 48.]), group([PLANE, 78., 144., 48.]),
        group([PLANE, 132., 144., 68.])];
    surface.anchors = vec![[54., 24., 36., 36.], [54., 164., 36., 36.]];
    for group in &mut surface.groups {
        group.color = surface.color;
        group.strip_color = [0., 0., 1., 1.];
        group.tab_bar = [group.bounds[0], group.bounds[1], group.bounds[2], 30.];
    }
    surface
}

fn mirrored(mut surface: PanelSurface, vertical: bool) -> PanelSurface {
    let reflect = |bounds: &mut [f32; 4]| {
        let axis = usize::from(vertical);
        bounds[axis] = SIZE[axis] as f32 - bounds[axis] - bounds[axis + 2];
    };
    let radii = |r: [f32; 4]| if vertical { [r[3], r[2], r[1], r[0]] } else { [r[1], r[0], r[3], r[2]] };
    reflect(&mut surface.sidebar);
    surface.sidebar_radii = radii(surface.sidebar_radii);
    for anchor in &mut surface.anchors { reflect(anchor); }
    for group in &mut surface.groups {
        reflect(&mut group.bounds);
        group.radii = radii(group.radii);
        reflect(&mut group.tab_bar);
        reflect(&mut group.active_tab);
        reflect(&mut group.tab_clip);
    }
    if vertical { surface.groups.reverse(); }
    else { surface.plane = SIZE[0] as f32 - surface.plane; surface.direction = -surface.direction; }
    surface
}

fn pixels(renderer: &WgpuRasterizer, presenter: &mut ViewportPresenter, surface: &PanelSurface,
    scale: u32, format: wgpu::TextureFormat) -> Vec<u8> {
    let size = SIZE.map(|v| v * scale);
    let texture = crate::create_target(renderer.device(), size, format, "panel surface regression").0;
    presenter.set_panel_surfaces(renderer, &[surface.clone().scaled(scale as f32)]);
    presenter.present(renderer, &texture.create_view(&Default::default()), camera(size), [0., 0., 0., 1.]).unwrap();
    let bytes = crate::layer_tests::page_bytes(renderer, &texture);
    if format == wgpu::TextureFormat::Rgba8Unorm
        && let Some(directory) = std::env::var_os("CAPY_PANEL_SURFACE_CAPTURE_DIR")
    {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let thread = std::thread::current();
        let test = thread.name().unwrap_or("panel").replace(':', "_");
        let name = format!("{test}-gap{}-anchor{}-side{}-scale{scale}.png",
            surface.groups[0].bounds[1] + surface.groups[0].bounds[3],
            surface.anchors.first().map_or(0., |a| a[1]), surface.direction);
        let mut png = png::Encoder::new(std::fs::File::create(directory.join(name)).unwrap(), size[0], size[1]);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header().unwrap().write_image_data(&bytes).unwrap();
    }
    bytes
}

fn red(bytes: &[u8], scale: u32, x: f32, y: f32) -> u8 {
    let x = (x * scale as f32).floor() as usize;
    let y = (y * scale as f32).floor() as usize;
    bytes[(y * (SIZE[0] * scale) as usize + x) * 4]
}

fn rgb(bytes: &[u8], scale: u32, x: f32, y: f32) -> [u8; 3] {
    let x = (x * scale as f32).floor() as usize;
    let y = (y * scale as f32).floor() as usize;
    let i = (y * (SIZE[0] * scale) as usize + x) * 4;
    bytes[i..i + 3].try_into().unwrap()
}

fn cap_distance(x: f32, y: f32, gap: f32) -> f32 {
    let point = |step: usize| {
        let q = step as f32 / 64. - 1.;
        [PLANE - 4. * (1. - q * q).max(0.).cbrt(), gap + 3. * (q + 1.)]
    };
    (0..128).map(|step| {
        let (a, b) = (point(step), point(step + 1));
        let delta = [b[0] - a[0], b[1] - a[1]];
        let t = (((x - a[0]) * delta[0] + (y - a[1]) * delta[1])
            / (delta[0] * delta[0] + delta[1] * delta[1])).clamp(0., 1.);
        (x - a[0] - t * delta[0]).hypot(y - a[1] - t * delta[1])
    }).fold(f32::INFINITY, f32::min)
}

#[test]
fn gpu_panel_surface_audit_glass_matches_backdrop_regions_at_odd_extents() {
    let mut artwork = Artwork::new([48; 2]).unwrap();
    crate::test_support::add_paint(&mut artwork, "Two backdrop edges", [48; 2]);
    let index = Arc::new(SceneIndex::build(&artwork).unwrap());
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut failures = Vec::new();
    for size in [[100, 100], [101, 99]] {
        let mut renderer = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        assert_ne!(renderer.adapter.get_info().device_type, wgpu::DeviceType::Cpu);
        crate::test_support::submit_settled(&mut renderer, FramePacket { view: view(size), reset_layers: true,
            ..packet(SceneView::new(&artwork, &index), [48; 2]) });
        let target = crate::create_target(renderer.device(), size, format, "panel glass comparison").0;
        let bounds = [16., 16., 72., 72.];
        let region = crate::BackdropRegion::rounded(bounds, [0.; 4], crate::BackdropRegion::SQUIRCLE);
        let style = crate::BackdropBlurStyle { levels: 3, offset: 0.5 };
        let mut ordinary = ViewportPresenter::for_renderer(&renderer, format);
        ordinary.set_backdrop(&renderer, &[region], style, false);
        let mut panel = ViewportPresenter::for_renderer(&renderer, format);
        panel.set_backdrop(&renderer, &[region.with_strength(0.)], style, false);
        let mut receiver = group(bounds);
        receiver.radii = [0.; 4];
        receiver.color = [0.; 4];
        panel.set_panel_surfaces(&renderer, &[PanelSurface {
            sidebar: [0.; 4], sidebar_radii: [0.; 4], plane: 16., bridge_width: 6.,
            direction: 1., scale: 1., color: [0.; 4], groups: vec![receiver], anchors: Vec::new(),
        }]);
        for translation in [[0., 0.], [4., 3.]] {
            let mut camera = view(size);
            camera.document_to_surface[4..].copy_from_slice(&translation);
            let render = |presenter: &mut ViewportPresenter| {
                presenter.present(&renderer, &target.create_view(&Default::default()), camera, [0., 0., 0., 1.]).unwrap();
                crate::layer_tests::page_bytes(&renderer, &target)
            };
            let expected = render(&mut ordinary);
            let actual = render(&mut panel);
            let mut worst = [(0, [0, 0]); 2];
            for axis in 0..2 { for at in 28..76 {
                let [x, y] = if axis == 0 { [at, 28] } else { [28, at] };
                let offset = ((y * size[0] + x) * 4) as usize;
                let error = expected[offset].abs_diff(actual[offset]);
                if error > worst[axis].0 { worst[axis] = (error, [x, y]); }
            }}
            eprintln!("panel-glass size={size:?} translation={translation:?} worst x/y={worst:?}");
            for (axis, (error, point)) in worst.into_iter().enumerate() {
                if error > 1 { failures.push(format!("glass mismatch size={size:?} translation={translation:?} axis={axis} point={point:?}: {error} byte levels")); }
            }
            if translation != [0.; 2] {
                assert_eq!(ordinary.backdrop_frames(), [1, 1], "ordinary glass must reuse its reprojected cache");
                assert_eq!(panel.backdrop_frames(), [1, 1], "panel glass must reuse its reprojected cache");
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn gpu_panel_surface_audit_absent_sidebar_casts_no_origin_shadow() {
    let renderer = renderer();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut presenter = ViewportPresenter::for_renderer(&renderer, format);
    let mut scene = surface(104.);
    scene.sidebar = [0.; 4];
    scene.anchors.clear();
    scene.groups = vec![group([16., 24., 144., 64.])];
    presenter.set_panel_surfaces(&renderer, &[scene]);
    let target = crate::create_target(renderer.device(), SIZE, format, "absent sidebar shadow").0;
    presenter.present(&renderer, &target.create_view(&Default::default()), camera(SIZE), WHITE).unwrap();
    let bytes = crate::layer_tests::page_bytes(&renderer, &target);
    for [x, y] in [[4.5, 8.5], [8.5, 4.5]] {
        let value = red(&bytes, 1, x, y);
        assert!(value >= 254, "absent sidebar darkened the distant white surround at {x},{y}: {value}");
    }
}

#[test]
fn gpu_panel_surface_audit_outer_envelope_uses_normalized_facing_radii() {
    let renderer = renderer();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut presenter = ViewportPresenter::for_renderer(&renderer, format);
    let mut failures = Vec::new();
    for (height, radii, anchor, offset) in [
        (176., [0., 18., 18., 0.], [54., 80., 36., 48.], 0.625),
        (24., [36., 18., 18., 36.], [54., 42., 36., 6.], 4.625),
    ] {
        for horizontal in [false, true] {
            let mut scene = surface(104.);
            scene.groups = vec![group([PLANE, 24., 144., height])];
            scene.groups[0].radii = radii;
            scene.anchors = vec![anchor];
            if horizontal { scene = mirrored(scene, false); }
            let bytes = pixels(&renderer, &mut presenter, &scene, 4, format);
            let x = if horizontal { SIZE[0] as f32 - PLANE - offset } else { PLANE + offset };
            let value = red(&bytes, 4, x, 24. + offset);
            if value < 253 { failures.push(format!("outer envelope clipped the original receiver interior: height={height} radii={radii:?} horizontal={horizontal}: {value}")); }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn gpu_panel_surface_connected_bridge_has_no_corner_radius_shelf_or_source_seam() {
    let renderer = renderer();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut presenter = ViewportPresenter::for_renderer(&renderer, format);
    let mut failures = Vec::new();
    for scale in [2, 4] { for horizontal in [false, true] { for vertical in [false, true] {
        for delta in [-54.25, -54., -51., -48., -24., -6., -3., 0., 0.001, 0.25, 1., 2., 3., 4., 5., 6., 8., 12., 18.] {
            let gap = 128. + delta;
            let mut scene = surface(gap);
            if horizontal { scene = mirrored(scene, false); }
            if vertical { scene = mirrored(scene, true); }
            let bytes = pixels(&renderer, &mut presenter, &scene, scale, format);
            let point = |x: f32, y: f32| [
                if horizontal { SIZE[0] as f32 - x } else { x },
                if vertical { SIZE[1] as f32 - y } else { y },
            ];
            for x in [90.5, 93.5] {
                let [x, y] = point(x, 100.);
                if red(&bytes, scale, x, y) < 253 {
                    failures.push(format!("source seam at guide gap={gap} scale={scale} horizontal={horizontal} vertical={vertical}: {x},{y}"));
                }
            }
            if gap <= 74. || gap >= 128. {
                let y = if gap <= 74. { gap - 0.5 / scale as f32 } else { gap + 6. + 0.5 / scale as f32 };
                let [x, y] = point(PLANE + 0.5 / scale as f32, y);
                if red(&bytes, scale, x, y) > 2 {
                    failures.push(format!("unattached corner filled at guide gap={gap} scale={scale} horizontal={horizontal} vertical={vertical}: {x},{y}"));
                }
            }
            let cut = ((gap - 18.) * scale as f32).floor() / scale as f32;
            let mut probes = Vec::new();
            for x in [90. + 0.5 / scale as f32, 93. + 0.5 / scale as f32] {
                let [bx, by] = point(x, cut - 0.5 / scale as f32);
                let [ax, ay] = point(x, cut + 1.5 / scale as f32);
                let before = red(&bytes, scale, bx, by);
                let after = red(&bytes, scale, ax, ay);
                probes.push([before, after]);
                if (0. ..=8.).contains(&delta) && (before < 253 || after < 253) {
                    failures.push(format!("connected bridge cut: delta={delta} scale={scale} horizontal={horizontal} vertical={vertical} x={x} y={cut}: {before} above, {after} below"));
                }
            }
            eprintln!("connected-bridge delta={delta} scale={scale} horizontal={horizontal} vertical={vertical}: source/core above/below {probes:?}");
        }
    }}}
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn gpu_panel_surface_corner_fill_enters_over_the_whole_six_pixel_gap() {
    let renderer = renderer();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut presenter = ViewportPresenter::for_renderer(&renderer, format);
    let travels = [0., 0.001, 0.25, 1., 2., 3., 4., 5., 6.];
    let mut failures = Vec::new();
    for scale in [1, 2] { for horizontal in [false, true] { for vertical in [false, true] {
        for tint in [1., 0.32] {
            let mut fills = Vec::new();
            for travel in travels {
                let top = 134. - travel;
                let mut scene = surface(104.);
                scene.groups = vec![group([PLANE, top, 144., 64.])];
                scene.color = [tint, tint, tint, 1.];
                scene.groups[0].color = scene.color;
                if horizontal { scene = mirrored(scene, false); }
                if vertical { scene = mirrored(scene, true); }
                let bytes = pixels(&renderer, &mut presenter, &scene, scale, format);
                let point = |x: f32, y: f32| [
                    if horizontal { SIZE[0] as f32 - x } else { x },
                    if vertical { SIZE[1] as f32 - y } else { y },
                ];
                let mut coverage = 0.;
                let mut samples = 0;
                for py in 0..SIZE[1] * scale { for px in 0..SIZE[0] * scale {
                    let x = (px as f32 + 0.5) / scale as f32;
                    let y = (py as f32 + 0.5) / scale as f32;
                    let [x, y] = point(x, y);
                    let dx = x - PLANE;
                    let dy = y - top;
                    if !(0. ..18.).contains(&dx) || !(0. ..18.).contains(&dy) { continue; }
                    if ((18. - dx).powi(4) + (18. - dy).powi(4)).powf(0.25) <= 18. { continue; }
                    coverage += bytes[((py * SIZE[0] * scale + px) * 4) as usize] as f32 / (255. * tint);
                    samples += 1;
                }}
                fills.push(coverage / samples as f32);
                let [x, y] = point(PLANE + 24., top - 1.5);
                assert!(red(&bytes, scale, x, y) <= 2, "corner fill escaped above the receiver: {travel}, {horizontal}, {vertical}");
                if travel == 0. {
                    let [x, y] = point(PLANE - 3., top + 12.);
                    assert!(red(&bytes, scale, x, y) <= 2, "a disjoint receiver acquired a bridge");
                }
                if travel == 6. {
                    for x in [96.5, 99.5, 104.5] {
                        let [x, y] = point(x, top + 8.5);
                        assert!(red(&bytes, scale, x, y) as f32 >= tint * 255. - 2., "attached bridge seam at {x},{y}");
                    }
                }
            }
            eprintln!("corner-entry scale={scale} horizontal={horizontal} vertical={vertical} tint={tint}: {fills:?}");
            let context = format!("scale={scale} horizontal={horizontal} vertical={vertical} tint={tint}");
            if fills[0] > 0.25 || fills[8] < 0.95 {
                failures.push(format!("{context}: rounded/full endpoints: {} / {}", fills[0], fills[8]));
            }
            for (index, maximum) in [(1, 0.05), (2, 0.15), (3, 0.25)] {
                let progress = (fills[index] - fills[0]) / (fills[8] - fills[0]);
                if progress > maximum {
                    failures.push(format!("{context}: travel {} filled {:.1}% of the whole missing R18 corner (maximum {:.0}%)", travels[index], 100. * progress, 100. * maximum));
                }
            }
            for (index, pair) in fills.windows(2).enumerate() {
                if pair[1] + 0.04 < pair[0] {
                    failures.push(format!("{context}: fill decreased from travel {} to {}: {pair:?}", travels[index], travels[index + 1]));
                }
            }
        }
    }}}
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn gpu_panel_surfaces_mirror_full_and_partial_attachments_on_both_axes() {
    let renderer = renderer();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut presenter = ViewportPresenter::for_renderer(&renderer, format);
    let scale = 2;
    let size = SIZE.map(|v| v * scale);
    for gap in [77., 104., 125.] {
        let surface = surface(gap);
        let original = pixels(&renderer, &mut presenter, &surface, scale, format);
        assert_eq!(red(&original, scale, 112., 64.), 255);
        for vertical in [false, true] {
            let reflected = pixels(&renderer, &mut presenter, &mirrored(surface.clone(), vertical), scale, format);
            for y in 0..size[1] { for x in 0..size[0] {
                let [rx, ry] = if vertical { [x, size[1] - y - 1] } else { [size[0] - x - 1, y] };
                let a = original[((y * size[0] + x) * 4) as usize];
                let b = reflected[((ry * size[0] + rx) * 4) as usize];
                assert!(a.abs_diff(b) <= 2, "gap {gap}, vertical {vertical}, {x},{y}: {a} vs {b}");
            }}
        }
    }
}

#[test]
fn gpu_panel_surfaces_keep_a_constant_broad_cap_and_two_pixel_bridge() {
    let renderer = renderer();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut presenter = ViewportPresenter::for_renderer(&renderer, format);
    let scale = 4;
    let mut previous = None;
    for gap in [90., 104., 116.] {
        let bytes = pixels(&renderer, &mut presenter, &surface(gap), scale, format);
        let mut cap = Vec::new();
        for row in 0..6 * scale {
            let y = gap + (row as f32 + 0.5) / scale as f32;
            let q = (y - gap - 3.) / 3.;
            let boundary = PLANE - 4. * (1. - q * q).cbrt();
            for column in 0..6 * scale {
                let x = PLANE - 6. + (column as f32 + 0.5) / scale as f32;
                let value = red(&bytes, scale, x, y);
                cap.push(value);
                if cap_distance(x, y, gap) > 0.3 {
                    if x < boundary { assert!(value >= 253, "solid bridge at {x},{y}: {value}"); }
                    else { assert!(value <= 2, "gap cap at {x},{y}: {value}"); }
                }
            }
        }
        for y in [gap + 2.625, gap + 3.375] {
            for x in [90.375, 90.875, 91.375, 91.625] {
                assert!(red(&bytes, scale, x, y) >= 253, "the six-pixel bridge keeps two solid pixels at {x},{y}");
            }
            assert!(red(&bytes, scale, 93., y) <= 2, "the cap reaches four pixels into the bridge");
        }
        for y in [gap - 0.75, gap + 6.75] {
            assert!(red(&bytes, scale, 94., y) >= 253, "the gap cap cannot cut the adjacent panel at {y}");
        }
        if let Some(previous) = previous {
            assert_eq!(cap, previous, "translating an attached gap must preserve the cap profile");
        }
        previous = Some(cap);
    }
}

#[test]
fn gpu_panel_surfaces_have_flat_aligned_edges_and_no_join_seam() {
    let renderer = renderer();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut presenter = ViewportPresenter::for_renderer(&renderer, format);
    let scale = 2;
    let mut surface = surface(104.);
    surface.anchors[0] = [54., 24., 36., 176.];
    let bytes = pixels(&renderer, &mut presenter, &surface, scale, format);
    for column in 0..80 {
        let x = 90.75 + column as f32 * 0.5;
        for y in [23.25, 200.75] { assert!(red(&bytes, scale, x, y) <= 2, "flat edge leaked at {x},{y}"); }
        for y in [24.75, 199.25] { assert!(red(&bytes, scale, x, y) >= 253, "flat edge missing at {x},{y}"); }
    }
    for y in [48.25, 88.25, 103.25, 110.75, 150.25, 180.25] {
        for column in 0..44 {
            let x = 89.25 + column as f32 * 0.5;
            assert!(red(&bytes, scale, x, y) >= 253, "panel/sidebar/bridge seam at {x},{y}");
        }
    }
}

#[test]
fn gpu_panel_surfaces_remain_finite_through_float_surface_entry_and_exit() {
    let renderer = renderer();
    let format = wgpu::TextureFormat::Rgba16Float;
    let mut presenter = ViewportPresenter::for_renderer(&renderer, format);
    for gap in [73.75, 76.999, 77., 77.001, 80., 104., 121.999, 122., 122.001, 128., 131.25] {
        let bytes = pixels(&renderer, &mut presenter, &surface(gap), 1, format);
        for (i, value) in bytes.chunks_exact(2).enumerate() {
            let bits = u16::from_le_bytes([value[0], value[1]]);
            assert_ne!(bits & 0x7c00, 0x7c00, "nonfinite half float at gap {gap}, channel {i}: {bits:#06x}");
        }
        let interior = ((64 * SIZE[0] + 112) * 8) as usize;
        let white = u16::from_le_bytes([bytes[interior], bytes[interior + 1]]);
        assert!(white >= 0x3bfe && white <= 0x3c02, "float surface must draw the panel at gap {gap}: {white:#06x}");
    }
}

#[test]
fn gpu_panel_surface_background_preserves_tab_bar_corners_and_clips_selected_tabs() {
    let renderer = renderer();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut presenter = ViewportPresenter::for_renderer(&renderer, format);
    let scale = 4;
    let mut surface = surface(104.);
    surface.groups = vec![group([PLANE, 24., 144., 176.])];
    surface.groups[0].tab_bar = [PLANE, 24., 144., 30.];
    surface.groups[0].active_tab = [132., 24., 60., 30.];
    surface.groups[0].tab_clip = [131.5, 24., 52.5, 30.];
    surface.groups[0].strip_color = [0., 0., 1., 1.];
    surface.anchors[0] = [54., 24., 36., 176.];
    for reflect in [false, true] {
        let displayed = if reflect { mirrored(surface.clone(), false) } else { surface.clone() };
        let bytes = pixels(&renderer, &mut presenter, &displayed, scale, format);
        let at = |x: f32, y| rgb(&bytes, scale, if reflect { SIZE[0] as f32 - x } else { x }, y);
        let mut distinguishes_eighteen_from_fifteen = 0;
        for row in 0..18 * scale { for column in 0..18 * scale {
            let x = (column as f32 + 0.5) / scale as f32;
            let y = (row as f32 + 0.5) / scale as f32;
            let field = |radius: f32| ((radius - x).powi(4) + (radius - y).powi(4)).sqrt().sqrt() - radius;
            let distance = field(18.);
            if distance.abs() <= 0.3 { continue; }
            let actual = at(PLANE + x, 24. + y);
            let expected = if distance < 0. { [0, 0, 255] } else { [255; 3] };
            assert!(actual.into_iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
                "tab corner reflect {reflect} at {x},{y}: {actual:?} vs {expected:?}");
            if distance > 0.3 && field(15.) < -0.2 { distinguishes_eighteen_from_fifteen += 1; }
        }}
        assert!(distinguishes_eighteen_from_fifteen > 0, "the corner oracle must reject half-height radius clamping");
        for (x, y, expected) in [
            (120.125, 40.125, [0, 0, 255]),
            (144.125, 40.125, [255; 3]),
            (131.875, 53.625, [255; 3]),
            (131.375, 53.625, [0, 0, 255]),
            (188.125, 44.125, [0, 0, 255]),
            (194.125, 53.625, [0, 0, 255]),
            (120.125, 54.625, [255; 3]),
        ] {
            let actual = at(x, y);
            assert!(actual.into_iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
                "tab/foot viewport reflect {reflect} at {x},{y}: {actual:?} vs {expected:?}");
        }
    }
}

#[test]
fn gpu_panel_surface_background_keeps_the_receiver_when_the_sidebar_is_hidden() {
    let renderer = renderer();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut presenter = ViewportPresenter::for_renderer(&renderer, format);
    let mut surface = surface(104.);
    surface.sidebar = [0.; 4];
    surface.anchors.clear();
    surface.groups = vec![group([PLANE, 24., 144., 176.])];
    let bytes = pixels(&renderer, &mut presenter, &surface, 2, format);
    for y in [12.25, 40.25, 104.25, 180.25, 212.25] {
        for x in [54.25, 72.25, 89.25, 93.25] {
            assert_eq!(rgb(&bytes, 2, x, y), [0; 3], "hidden sidebar or bridge leaked at {x},{y}");
        }
    }
    for (x, y) in [(114.25, 42.25), (150.25, 104.25), (220.25, 180.25)] {
        assert_eq!(rgb(&bytes, 2, x, y), [255; 3], "the kept receiver disappeared at {x},{y}");
    }
    assert_eq!(rgb(&bytes, 2, 96.25, 24.25), [0; 3], "an unattached receiving corner must remain rounded");
}

#[test]
fn gpu_panel_surface_scene_replacement_clears_old_sources_and_anchor_attachments() {
    let renderer = renderer();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let scale = 2;
    let size = SIZE.map(|v| v * scale);
    let render = |presenter: &mut ViewportPresenter, surfaces: &[PanelSurface], target: &wgpu::Texture| {
        presenter.set_panel_surfaces(&renderer, surfaces);
        assert!(presenter.needs_present(&renderer, camera(size), [0., 0., 0., 1.]));
        presenter.present(&renderer, &target.create_view(&Default::default()), camera(size), [0., 0., 0., 1.]).unwrap();
        crate::layer_tests::page_bytes(&renderer, target)
    };
    for retained in [false, true] {
        let target = crate::create_target(renderer.device(), size, format, "replaced panel scenes").0;
        let mut presenter = ViewportPresenter::for_renderer(&renderer, format);
        presenter.set_target_retention(retained);
        let a = render(&mut presenter, &[surface(104.).scaled(scale as f32)], &target);
        assert_eq!(rgb(&a, scale, 72.25, 12.25), [255; 3]);
        assert_eq!(rgb(&a, scale, 92.25, 88.25), [255; 3]);
        let empty = render(&mut presenter, &[], &target);
        assert!(empty.chunks_exact(4).all(|p| p == [0, 0, 0, 255]), "retention={retained}: clearing the scene must erase every former panel pixel");

        let mut receiver = group([16., 80., 80., 104.]);
        receiver.color = [0., 0., 1., 1.];
        receiver.strip_color = receiver.color;
        let b = PanelSurface { sidebar: [0.; 4], sidebar_radii: [0.; 4], plane: 96.,
            bridge_width: 6., direction: -1., scale: 1., color: receiver.color,
            groups: vec![receiver], anchors: Vec::new() }.scaled(scale as f32);
        let replaced = render(&mut presenter, std::slice::from_ref(&b), &target);
        let fresh_target = crate::create_target(renderer.device(), size, format, "fresh panel scene").0;
        let mut fresh_presenter = ViewportPresenter::for_renderer(&renderer, format);
        fresh_presenter.set_target_retention(retained);
        let fresh = render(&mut fresh_presenter, &[b], &fresh_target);
        assert_eq!(replaced.len(), fresh.len());
        let mismatch = replaced.iter().zip(&fresh).position(|(a, b)| a != b);
        assert!(mismatch.is_none(), "replacement differs from a fresh presenter at byte {mismatch:?}");
        assert_eq!(rgb(&replaced, scale, 72.25, 12.25), [0; 3], "scene A's source column must disappear");
        assert_eq!(rgb(&replaced, scale, 100.25, 88.25), [0; 3], "scene A's bridge or anchor must not survive");
        assert_eq!(rgb(&replaced, scale, 95.25, 80.25), [0; 3], "an old anchor at the new panel's top must not flatten its corner");
        assert_eq!(rgb(&replaced, scale, 72.25, 140.25), [0, 0, 255]);
    }
}

#[test]
fn gpu_panel_surface_cached_shapes_follow_resize_scale_palette_and_rotation() {
    let renderer=renderer();
    let format=wgpu::TextureFormat::Rgba8Unorm;
    let mut retained=ViewportPresenter::for_renderer(&renderer,format);
    for (size,turns,scale,palette,geometry) in [
        (SIZE,0,1.,false,false),([263,229],0,1.,false,false),([263,229],1,1.,false,false),
        ([263,229],1,1.,true,false),([263,229],1,1.,true,true),([512,448],3,2.,true,true),
    ] {
        let mut scene=tabbed_receiver_column();
        if palette {scene.color=[0.1,0.3,0.2,0.8];scene.groups[0].color=[0.4,0.2,0.7,0.8];}
        if geometry {scene.groups[0].radii=[12.,18.,6.,0.];scene.anchors[0][1]+=8.;}
        scene.groups[1].active_tab=[126.,78.,54.,30.];
        scene.groups[1].tab_clip=[132.,78.,40.,30.];
        let surfaces=[scene.clone().scaled(scale),mirrored(surface(104.),false).scaled(scale)];
        let render=|presenter:&mut ViewportPresenter| {
            presenter.set_surface_rotation(turns);
            presenter.set_panel_surfaces(&renderer,&surfaces);
            let extent=if turns%2==0 {size}else {[size[1],size[0]]};
            let target=crate::create_target(renderer.device(),extent,format,"cached panel shape transition").0;
            presenter.present(&renderer,&target.create_view(&Default::default()),camera(size),[0.,0.,0.,1.]).unwrap();
            crate::layer_tests::page_bytes(&renderer,&target)
        };
        let actual=render(&mut retained);
        let mut fresh=ViewportPresenter::for_renderer(&renderer,format);
        assert_eq!(actual,render(&mut fresh),"stale shape after size={size:?},turns={turns},scale={scale}");
        assert_eq!(actual,render(&mut retained),"unchanged shape must preserve its exact pixels");
    }
}

#[test]
fn gpu_panel_shape_cache_recovers_a_discarded_presentation() {
    let renderer=renderer();
    let format=wgpu::TextureFormat::Rgba8Unorm;
    let target=crate::create_target(renderer.device(),SIZE,format,"discarded panel mask").0;
    let mut presenter=ViewportPresenter::for_renderer(&renderer,format);
    presenter.set_target_retention(true);
    presenter.set_panel_surfaces(&renderer,&[surface(104.)]);
    presenter.present(&renderer,&target.create_view(&Default::default()),camera(SIZE),[0.,0.,0.,1.]).unwrap();
    let replacement=mirrored(surface(77.),false);
    presenter.set_panel_surfaces(&renderer,&[replacement.clone()]);
    let mut discarded=renderer.device().create_command_encoder(&Default::default());
    presenter.encode(&renderer,&mut discarded,&target.create_view(&Default::default()),camera(SIZE),[0.,0.,0.,1.]).unwrap();
    drop(discarded);
    assert!(presenter.needs_present(&renderer,camera(SIZE),[0.,0.,0.,1.]),"discarded shape rendering must schedule another presentation");
    presenter.present(&renderer,&target.create_view(&Default::default()),camera(SIZE),[0.,0.,0.,1.]).unwrap();
    let actual=crate::layer_tests::page_bytes(&renderer,&target);
    let expected=pixels(&renderer,&mut ViewportPresenter::for_renderer(&renderer,format),&replacement,1,format);
    assert_eq!(actual,expected,"discarded mask work must recover every original surface pixel");
}

#[test]
fn gpu_panel_shape_cache_keeps_reprojection_and_recovers_discarded_geometry() {
    let renderer=renderer();
    let device=renderer.device();
    let uniform=device.create_buffer(&wgpu::BufferDescriptor {label:Some("panel cache viewport"),
        size:crate::present::CAMERA_SIZE,usage:wgpu::BufferUsages::UNIFORM,mapped_at_creation:false});
    let shader=crate::present::viewport_shader(&renderer.device,wgpu::TextureFormat::Rgba8Unorm,crate::SdrSurfaceColor::Srgb,false);
    let mut panels=super::PanelSurfaces::new(&renderer.device,&uniform,&shader,wgpu::TextureFormat::Rgba8Unorm);
    pollster::block_on(crate::Deferred::prepare_all([&panels.pipeline,&panels.mask_pipeline])).unwrap();
    let blur=crate::create_target(device,[64,56],wgpu::TextureFormat::Rgba16Float,"panel cached blur").0;
    let frame=|panels:&mut super::PanelSurfaces,affine| {
        let mut encoder=device.create_command_encoder(&Default::default());
        let mut uploads=crate::Uploads::new(device,4096);
        panels.prepare(device,&mut uploads,&mut encoder,Some((&blur,affine)),SIZE,0).unwrap();
        uploads.finish(&encoder);
        encoder
    };
    let mut scene=surface(104.);
    panels.set(&[scene.clone()]);
    renderer.queue().submit([frame(&mut panels,layer_core::Affine::IDENTITY).finish()]);
    assert!(panels.mask_valid.load(std::sync::atomic::Ordering::Acquire));
    drop(frame(&mut panels,layer_core::Affine::translation(layer_core::Point{x:4.,y:3.})));
    assert!(panels.mask_valid.load(std::sync::atomic::Ordering::Acquire),"blur reprojection must not encode panel shape work");
    scene.anchors[0][1]+=8.;
    panels.set(&[scene]);
    drop(frame(&mut panels,layer_core::Affine::IDENTITY));
    assert!(!panels.mask_valid.load(std::sync::atomic::Ordering::Acquire),"discarded geometry cannot publish a valid mask");
    renderer.queue().submit([frame(&mut panels,layer_core::Affine::IDENTITY).finish()]);
    assert!(panels.mask_valid.load(std::sync::atomic::Ordering::Acquire));
    let mask=panels.masks[0].as_ref().unwrap();
    assert!(mask.texture.width()<=SIZE[0] && mask.texture.height()<=SIZE[1],"cache allocations stay inside the physical viewport");
}

#[test]
fn gpu_panel_surface_remote_anchors_preserve_unattached_receiver_corners() {
    let renderer = renderer();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut presenter = ViewportPresenter::for_renderer(&renderer, format);
    let scale = 4;
    let surface = tabbed_receiver_column();
    let mut failures = Vec::new();
    for reflect in [false, true] {
        let displayed = if reflect { mirrored(surface.clone(), false) } else { surface.clone() };
        let bytes = pixels(&renderer, &mut presenter, &displayed, scale, format);
        for (index, top) in [(0, false), (1, true), (1, false), (2, true)] {
            let bounds = surface.groups[index].bounds;
            let edge = if top { bounds[1] } else { bounds[1] + bounds[3] };
            assert!(surface.anchors.iter().all(|a| edge < a[1] - 6. || edge > a[1] + a[3] + 6.),
                "the oracle corner must have no nearby source tile");
            let inset = 0.125f32;
            let distance = ((18. - inset).powi(4) * 2f32).sqrt().sqrt() - 18.;
            assert!(distance > 3., "the probe must lie well outside the original corner");
            let x = PLANE + inset;
            let x = if reflect { SIZE[0] as f32 - x } else { x };
            let y = edge + if top { inset } else { -inset };
            let actual = rgb(&bytes, scale, x, y);
            if actual.iter().any(|&channel| channel > 2) {
                failures.push((reflect, index, top, x, y, actual));
            }
        }
    }
    assert!(failures.is_empty(), "remote source tiles filled unconnected R18 corners: {failures:?}");
}

#[test]
fn gpu_panel_surface_single_anchors_preserve_remote_receivers_and_adjacent_gap_geometry() {
    let renderer = renderer();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let mut presenter = ViewportPresenter::for_renderer(&renderer, format);
    let scale = 4;
    let mut failures = Vec::new();
    for (name, anchor, corners) in [
        ("upper", 24., vec![(0, false), (1, true), (1, false), (2, true), (2, false)]),
        ("lower", 164., vec![(0, true), (0, false), (1, true), (1, false), (2, true)]),
        ("adjacent gap", 60., vec![(0, true), (1, false), (2, true), (2, false)]),
    ] {
        let mut surface = tabbed_receiver_column();
        surface.anchors = vec![[54., anchor, 36., 36.]];
        for reflect in [false, true] {
            let displayed = if reflect { mirrored(surface.clone(), false) } else { surface.clone() };
            let bytes = pixels(&renderer, &mut presenter, &displayed, scale, format);
            let at = |x: f32, y| rgb(&bytes, scale, if reflect { SIZE[0] as f32 - x } else { x }, y);
            for &(index, top) in &corners {
                let bounds = surface.groups[index].bounds;
                let edge = if top { bounds[1] } else { bounds[1] + bounds[3] };
                assert!(edge < anchor - 6. || edge > anchor + 42.);
                'corner: for row in 0..18 * scale { for column in 0..18 * scale {
                    let dx = (column as f32 + 0.5) / scale as f32;
                    let dy = (row as f32 + 0.5) / scale as f32;
                    let distance = ((18. - dx).powi(4) + (18. - dy).powi(4)).sqrt().sqrt() - 18.;
                    if distance.abs() <= 0.4 { continue; }
                    let x = PLANE + dx;
                    let y = edge + if top { dy } else { -dy };
                    let expected = if distance > 0. { [0; 3] } else if top { [0, 0, 255] } else { [153; 3] };
                    let actual = at(x, y);
                    if actual.into_iter().zip(expected).any(|(a, b)| a.abs_diff(b) > 2) {
                        failures.push((name, reflect, index, top, x, y, actual, expected));
                        break 'corner;
                    }
                }}
            }
            if name == "adjacent gap" {
                for x in [90.375, 90.875, 91.375, 91.625] {
                    assert_eq!(at(x, 75.125), [153; 3], "the adjacent gap must keep a two-pixel bridge on side {reflect}");
                }
                assert_eq!(at(93.125, 75.125), [0; 3], "the adjacent gap cap must retain its intrusion on side {reflect}");
                assert_eq!(at(92.125, 129.125), [0; 3], "a remote gap must have no source bridge on side {reflect}");
            }
        }
    }
    assert!(failures.is_empty(), "a single source tile changed remote receiver corners: {failures:?}");
}
