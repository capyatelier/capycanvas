//! Direction oracles for Liquify modes: a marker's displacement under one dab.
use super::*;
use layer_core::{DefaultBrushPreset, LiquifyMode, Point, StrokeId};

const CENTER: [f32; 2] = [64., 64.];
const MARKER: [f32; 2] = [96., 64.];

fn marker_centroid(r: &mut WgpuRasterizer) -> [f32; 2] {
    let image = r.readback_srgb_rgba8().unwrap();
    let mut sum = [0.; 3];
    for (i, p) in image.as_chunks::<4>().0.iter().enumerate() {
        let weight = f32::from(p[3]) / 255.;
        sum[0] += weight * (i % 128) as f32;
        sum[1] += weight * (i / 128) as f32;
        sum[2] += weight;
    }
    assert!(sum[2] > 10., "the marker must stay visible");
    [sum[0] / sum[2] + 0.5, sum[1] / sum[2] + 0.5]
}

/// Paints a small marker right of the dab centre, applies one stationary
/// Liquify dab with the preset's mode, and returns the marker's displacement.
fn displacement(preset: DefaultBrushPreset) -> [f32; 2] {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let document = paint_document([128; 2], "marker");
    let mut marker = dab([1., 0., 0., 1.]);
    marker.center = Point { x: MARKER[0], y: MARKER[1] };
    marker.radii = [5.; 2];
    submit(&mut r, document.scene(), &[marker], &[batch(target(&document))], true);
    let before = marker_centroid(&mut r);
    let style = preset_style(preset);
    assert_eq!(style.execution, BrushExecution::Liquify);
    let mut deform = dab([0.; 4]);
    deform.center = Point { x: CENTER[0], y: CENTER[1] };
    deform.material = [0., 0., 0., 0.6];
    let liquify = DabBatch {
        stroke_id: StrokeId(2),
        ..crate::test_support::dab_batch(target(&document), style, batch(target(&document)).damage)
    };
    submit(&mut r, document.scene(), &[deform], &[liquify], false);
    let after = marker_centroid(&mut r);
    [after[0] - before[0], after[1] - before[1]]
}

#[test]
fn liquify_modes_move_a_marker_in_their_labelled_direction() {
    let pinch = displacement(DefaultBrushPreset::LiquifyPinch);
    assert!(pinch[0] < -3. && pinch[1].abs() < 0.5, "Pinch shrinks toward the centre: {pinch:?}");
    let expand = displacement(DefaultBrushPreset::LiquifyExpand);
    assert!(expand[0] > 3. && expand[1].abs() < 0.5, "Expand bulges away from the centre: {expand:?}");
    let counter_clockwise = displacement(DefaultBrushPreset::LiquifyTwirl);
    assert!(
        counter_clockwise[1] < -3.,
        "Twirl Counterclockwise lifts a marker right of the centre on the y-down canvas: {counter_clockwise:?}"
    );
    let clockwise = displacement(DefaultBrushPreset::LiquifyTwirlClockwise);
    assert!(
        clockwise[1] > 3.,
        "Twirl Clockwise lowers a marker right of the centre on the y-down canvas: {clockwise:?}"
    );
    assert_eq!(
        preset_style(DefaultBrushPreset::LiquifyTwirl).deform.mode,
        LiquifyMode::TwirlCounterclockwise,
        "the shipped Twirl keeps its behaviour"
    );
}

/// Render and compute passes that one committed frame of `style` adds on a
/// 4×4-page layer when each of its `batches` contacts covers `radius` around
/// the centre.
fn committed_frame_passes(style: &DabStyle, radius: f32, batches: usize) -> u64 {
    let extent = [4 * PAGE_SIZE; 2];
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let document = paint_document(extent, "material batch pages");
    let layer = target(&document);
    let mut base = dab([0.2, 0.4, 0.8, 1.]);
    base.center = Point { x: 512., y: 512. };
    base.radii = [500.; 2];
    let full = layer_core::Rect::from_extent(extent);
    let frame = |dabs, batches| FramePacket { dabs, dab_batches: batches, commit_rasters: false, ..packet(document.scene(), extent) };
    let paint = crate::test_support::dab_batch(layer, crate::tests::test_style(BrushExecution::Dry), full);
    r.submit(FramePacket { reset_layers: true, ..frame(std::slice::from_ref(&base), std::slice::from_ref(&paint)) }).unwrap();
    let contacts: Vec<_> = (0..batches).map(|i| {
        let mut contact = dab([0.9, 0.2, 0.1, 1.]);
        contact.center = Point { x: 400. + 6. * i as f32, y: 400. };
        contact.radii = [radius; 2];
        contact.motion = [48., 32.];
        contact.material = [0., 1., 1., 0.8];
        contact
    }).collect();
    let strokes: Vec<_> = contacts.iter().enumerate().map(|(i, contact)| DabBatch { stroke_id: StrokeId(2), first_dab: i as u32,
        stroke_start: i == 0, stroke_end: false, ..crate::test_support::dab_batch(layer, style.clone(), contact.bounds()) }).collect();
    let before = r.metrics().command_passes;
    r.submit(frame(&contacts, &strokes)).unwrap();
    r.wait_idle().unwrap();
    r.metrics().command_passes - before
}

#[test]
fn committed_brush_pass_counts_are_independent_of_page_count() {
    use DefaultBrushPreset as P;
    let mut multiply = preset_style(P::GPen);
    multiply.rendering.blend_mode = layer_core::BrushBlendMode::Multiply;
    let liquify = [P::LiquifyTwirl, P::LiquifyPush].map(|preset| (format!("{preset:?}"), preset_style(preset), [3, 3, 6]));
    let destination = [P::Smudge, P::NaturalBlender, P::WetRound, P::OpaqueGouache, P::LoadedOil,
        P::PaletteKnife, P::WatercolorWash, P::WetWatercolor].map(|preset| (format!("{preset:?}"), preset_style(preset), [1; 3]));
    let dry = [P::GPen, P::Pencil, P::Airbrush].map(|preset| (format!("{preset:?}"), preset_style(preset), [1, 1, 4]));
    for (name, style, expected) in liquify.into_iter().chain(destination).chain(dry).chain([("GPen Multiply".to_string(), multiply, [1, 1, 4])]) {
        let passes = [(40., 1), (380., 1), (380., 4)].map(|(radius, batches)| committed_frame_passes(&style, radius, batches));
        println!("{name}: one page {}, sixteen pages {}, four batches {}", passes[0], passes[1], passes[2]);
        assert_eq!(passes, expected, "{name} adds passes per page or batch");
    }
    for preset in [P::LiquifyTwirl, P::LiquifyPush] {
        let style=preset_style(preset);
        assert_eq!(committed_frame_passes(&style,380.,2),4,"{preset:?} batches two chunks into one reconstruction");
        assert_eq!(committed_frame_passes(&style,380.,3),5,"{preset:?} batches three chunks into one reconstruction");
    }
}

#[test]
fn liquify_crystals_matches_integer_hash_and_zero_influence_preserves_pixels() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let document = paint_document([128; 2], "crystals integer oracle");
    let mut marker = dab([0.7, 0.2, 0.9, 1.]);
    marker.center = Point { x: 80., y: 64. };
    marker.radii = [8.; 2];
    submit(&mut r, document.scene(), &[marker], &[batch(target(&document))], true);
    let before = r.readback_srgb_rgba8().unwrap();
    let style = preset_style(DefaultBrushPreset::LiquifyCrystals);
    let distortion = style.deform.distortion;
    let mut deform = dab([0.; 4]);
    deform.material[3] = 0.;
    let mut liquify = crate::test_support::dab_batch(target(&document), style, batch(target(&document)).damage);
    liquify.stroke_id = StrokeId(2);
    submit(&mut r, document.scene(), &[deform], &[liquify.clone()], false);
    assert_eq!(r.readback_srgb_rgba8().unwrap(), before);
    deform.material[3] = 0.6;
    liquify.stroke_id = StrokeId(3);
    submit(&mut r, document.scene(), &[deform], &[liquify], false);
    let after = r.readback_srgb_rgba8().unwrap();
    fn hash(mut value: u32) -> u32 {
        value = (value ^ (value >> 16)).wrapping_mul(2146121005);
        value = (value ^ (value >> 15)).wrapping_mul(2221713035);
        value ^ (value >> 16)
    }
    let mut changed = 0;
    for y in 35..94usize {
        for x in 35..94usize {
            let seed = hash((x / 4) as u32 ^ hash((y / 4) as u32) ^ hash(0));
            let offset = [2654435769, 2246822507].map(|salt| {
                let random = (hash(seed ^ salt) >> 8) as f32 / 16777216.;
                (24. * distortion * 0.6 * (random - 0.5) + 0.5).floor() as i32
            });
            let source = ((y as i32 + offset[1]) as usize * 128 + (x as i32 + offset[0]) as usize) * 4;
            let destination = (y * 128 + x) * 4;
            assert_eq!(&after[destination..destination + 4], &before[source..source + 4], "pixel {x},{y}, offset {offset:?}");
            changed += usize::from(after[destination..destination + 4] != before[destination..destination + 4]);
        }
    }
    assert!(changed > 20, "the oracle must exercise moved marker pixels");
}

#[test]
fn liquify_bfloat_footprint_keeps_range_and_rounds_to_nearest_even_on_gpu() {
    let r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let prefix = include_str!("liquify_map.wgsl").split("fn liquify_hash").next().unwrap();
    let values = [0., 1., -1., f32::from_bits(0x3f808000), f32::from_bits(0x3f818000),
        100000., -1e20, f32::from_bits(0x7f7f0000)];
    let literals = values.map(|v| format!("{v:?}")).join(",");
    let shader = format!("{prefix}\n@group(0) @binding(0) var result:texture_storage_2d<rgba32uint,write>;\n@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id:vec3<u32>) {{ let values=array<f32,8>({literals}); if id.x<8u {{ let p=pack_footprint(vec2(values[id.x],-values[id.x])); textureStore(result,vec2<i32>(i32(id.x),0),vec4(p,bitcast<vec2<u32>>(unpack_footprint(p)),0u)); }} }}");
    let module = r.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("liquify bfloat oracle"), source: wgpu::ShaderSource::Wgsl(shader.into()) });
    let pipeline = r.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: None, layout: None, module: &module,
        entry_point: Some("main"), compilation_options: Default::default(), cache: None });
    let texture = r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("liquify bfloat readback"), size: wgpu::Extent3d {width:256,height:256,depth_or_array_layers:1},
        mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Rgba32Uint,
        usage:wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,view_formats:&[],
    });
    let view = texture.create_view(&Default::default());
    let bind = r.device.create_bind_group(&wgpu::BindGroupDescriptor {label: None, layout: &pipeline.get_bind_group_layout(0), entries: &[wgpu::BindGroupEntry {binding:0,resource:wgpu::BindingResource::TextureView(&view)}]});
    let mut encoder = r.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline); pass.set_bind_group(0,&bind,&[]); pass.dispatch_workgroups(8,1,1);
    }
    r.queue.submit([encoder.finish()]);
    let bytes = page_bytes(&r, &texture);
    for (i, value) in values.into_iter().enumerate() {
        let round = |v: f32| { let bits=v.to_bits(); (bits.wrapping_add(32767 + ((bits >> 16) & 1))) & 0xffff0000 };
        let words: Vec<_> = bytes[i*16..i*16+16].as_chunks::<4>().0.iter().map(|b| u32::from_ne_bytes(*b)).collect();
        assert_eq!(words[1],round(value), "positive component {value}");
        assert_eq!(words[2],round(-value), "negative component {value}");
        assert!(f32::from_bits(words[1]).is_finite());
    }
}

#[test]
fn liquify_composes_fractional_pushes_before_reconstructing_color() {
    let render = |steps: usize| {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let document = paint_document([128; 2], "push composition oracle");
        let mut marker = dab([0.7, 0.2, 0.9, 1.]);
        marker.center = Point { x: 80., y: 64. };
        marker.radii = [5.; 2];
        submit(&mut r, document.scene(), &[marker], &[batch(target(&document))], true);
        let mut deform = dab([0.; 4]);
        deform.material[3] = 1.;
        deform.motion = [1. / steps as f32, 0.];
        let mut liquify = crate::test_support::dab_batch(target(&document), preset_style(DefaultBrushPreset::LiquifyPush), batch(target(&document)).damage);
        liquify.stroke_id = StrokeId(2);
        liquify.stroke_end = false;
        for i in 0..steps {
            liquify.stroke_start = i == 0;
            submit(&mut r, document.scene(), &[deform], &[liquify.clone()], false);
        }
        r.readback_srgb_rgba8().unwrap()
    };
    let whole = render(1);
    let halves = render(2);
    for y in 45..84 {
        for x in 50..94 {
            let at = (y * 128 + x) * 4;
            assert_eq!(&halves[at..at+4], &whole[at..at+4], "pixel {x},{y}");
        }
    }
}

fn published_liquify(r: &mut WgpuRasterizer, document: &mut Document, mut style: DabStyle, dabs: &[Dab], id: u64) {
    let locked = placement::occurrence(document).alpha_locked;
    style.alpha_locked = locked;
    style.liquify = Some(Arc::new(layer_core::liquify::Context::for_source(paint(document), locked)));
    paint_mut(document).raster = layer_core::raster::RasterRevision::pending();
    let mut contact = crate::test_support::dab_batch(target(document), style, batch(target(document)).damage);
    contact.stroke_id = StrokeId(id);
    contact.dab_count = dabs.len() as u32;
    submit(r, document.scene(), dabs, &[contact], false);
    assert!(paint(document).raster.liquify().is_some(), "completed stroke publishes its field");
}

#[test]
fn liquify_published_sessions_continue_across_pen_lifts_and_chunk_splits() {
    let render = |split: bool| {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut document = paint_document([128; 2], "published push composition");
        placement::set_source(&mut document, layer_core::color::source::rgba8_source([128; 2], |x, _| {
            if x % 2 == 0 { [0,0,0,255] } else { [255;4] }
        }));
        submit(&mut r, document.scene(), &[], &[], true);
        let mut deform = dab([0.;4]); deform.material[3]=1.; deform.motion=[0.5,0.];
        let style = preset_style(DefaultBrushPreset::LiquifyPush);
        if split {
            published_liquify(&mut r, &mut document, style.clone(), &[deform], 20);
            let context = layer_core::liquify::Context::for_source(paint(&document), false);
            assert!(context.field.is_some());
            published_liquify(&mut r, &mut document, style, &[deform], 21);
        } else {
            published_liquify(&mut r, &mut document, style, &[deform,deform], 20);
        }
        r.readback_srgb_rgba8().unwrap()
    };
    let combined=render(false); let split=render(true);
    for y in 45..84 { for x in 45..84 {
        let at=(y*128+x)*4; assert_eq!(&combined[at..at+4], &split[at..at+4], "{x},{y}");
        assert!(split[at]==0 || split[at]==255, "whole shift stays sharp at {x},{y}");
    }}
}

#[test]
fn liquify_undo_then_new_stroke_uses_the_restored_field_and_pixels() {
    let render=|undo_intervening:bool| {
        let mut r=WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let mut document=paint_document([128;2],"liquify undo and fork");
        placement::set_source(&mut document,layer_core::color::source::rgba8_source([128;2],|x,y| {
            let value=((x*37+y*71+x*y%251)%256) as u8;
            [value,value,value,255]
        }));
        submit(&mut r,document.scene(),&[],&[],true);
        let style=preset_style(DefaultBrushPreset::LiquifyPush);
        let mut contact=dab([0.;4]);
        contact.material[3]=1.;
        contact.motion=[0.7,0.];
        published_liquify(&mut r,&mut document,style.clone(),&[contact],40);
        let restored_revision=paint(&document).raster.clone();
        let restored_pixels=r.readback_srgb_rgba8().unwrap();
        if undo_intervening {
            contact.motion=[1.3,0.];
            published_liquify(&mut r,&mut document,style.clone(),&[contact],41);
            assert_ne!(r.readback_srgb_rgba8().unwrap(),restored_pixels,"the undone stroke must change pixels");
            paint_mut(&mut document).raster=restored_revision.clone();
            crate::test_support::submit_ready(&mut r, FramePacket{restore_rasters:&[(target(&document),restored_revision)],
                ..packet(document.scene(),[128;2])}).unwrap();
            assert_eq!(r.readback_srgb_rgba8().unwrap(),restored_pixels,"Undo restores the prior pixels");
        }
        contact.motion=[-0.4,0.];
        published_liquify(&mut r,&mut document,style,&[contact],42);
        r.readback_srgb_rgba8().unwrap()
    };
    assert_eq!(render(true),render(false),"a new stroke after Undo must use the restored displacement field");
}

#[test]
fn liquify_alpha_lock_changes_start_from_current_published_pixels() {
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let mut document = paint_document([128;2], "alpha lock session transition");
    placement::set_source(&mut document, layer_core::color::source::rgba8_source([128;2], |x,_| if x<64 { [255,0,0,255] } else { [0;4] }));
    submit(&mut r,document.scene(),&[],&[],true);
    let mut deform=dab([0.;4]); deform.material[3]=1.; deform.motion=[4.,0.];
    let style=preset_style(DefaultBrushPreset::LiquifyPush);
    published_liquify(&mut r,&mut document,style.clone(),&[deform],30);
    let moved=r.readback_srgb_rgba8().unwrap();
    assert_eq!(moved[(64*128+66)*4+3],255);
    placement::occurrence_mut(&mut document).alpha_locked=true;
    assert!(layer_core::liquify::Context::for_source(paint(&document),true).field.is_none());
    published_liquify(&mut r,&mut document,style.clone(),&[deform],31);
    let locked=r.readback_srgb_rgba8().unwrap();
    for y in 45..84 { for x in 45..84 { let at=(y*128+x)*4+3; assert_eq!(locked[at],moved[at],"alpha {x},{y}"); }}
    placement::occurrence_mut(&mut document).alpha_locked=false;
    assert!(layer_core::liquify::Context::for_source(paint(&document),false).field.is_none());
    published_liquify(&mut r,&mut document,style,&[deform],32);
    let unlocked=r.readback_srgb_rgba8().unwrap();
    assert_eq!(locked[(64*128+70)*4+3],0);
    assert_eq!(unlocked[(64*128+70)*4+3],255);
}
