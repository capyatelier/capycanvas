use crate::artwork_sample_tests::{document_in, doubled_effect, gpu, insert_effect, paint_mut, paint_occurrence, add_group, refresh, set_effect};
use crate::snapshot::CaptureControl;
use layer_core::{ArtworkQuery, ArtworkSource, ArtworkStatisticsRequest, BlendSpace, Document, Selection, SelectionPixels};
use layer_core::color::{DocumentColor, RgbSpace, SampleDepth, histogram::Histogram};
use std::sync::Arc;
use layer_core::authored::*;

pub(super) fn generated(extent: [u32; 2], color: DocumentColor, pixels: &[[f32; 4]]) -> Document {
    let mut doc = document_in(extent, color.space, |_, _| [0.; 4]);
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().color=color;
    paint_mut(&mut doc).raster=Default::default();
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().blend=BlendSpace::Linear;
    let mut effect=doubled_effect();
    let program=Arc::make_mut(&mut effect.program);
    program.kind = layer_core::EffectKind::Generator;
    program.alpha = layer_core::EffectAlpha::Filter;
    program.entry = "statistics_fixture".into();
    let cases: String = pixels.iter().enumerate().map(|(i, pixel)| {
        let bits = pixel.map(f32::to_bits);
        format!("case {i}u:{{return bitcast<vec4<f32>>(vec4<u32>({}u,{}u,{}u,{}u));}}", bits[0], bits[1], bits[2], bits[3])
    }).collect();
    program.wgsl = format!("fn statistics_fixture(c:vec4<f32>,p:vec2<f32>,b:u32)->vec4<f32>{{let i=(u32(floor(p.x))+3u*u32(floor(p.y)))%{}u;switch i{{{cases}default:{{return vec4<f32>(0.);}}}}}}", pixels.len()).into();
    insert_effect(&mut doc,effect,0);
    doc
}

fn statistics(doc: &Document, preview: bool, selection: bool) -> Result<Histogram, String> {
    pollster::block_on(gpu().artwork_statistics(ArtworkStatisticsRequest { waveform: false,
        query: ArtworkQuery::new(doc, ArtworkSource::Visible), preview, selection,
    }, CaptureControl::default()))
}

fn oracle(doc: &Document, pixels: &[[f32; 4]], preview: bool, admitted: impl Fn(u32, u32) -> bool) -> Histogram {
    let mut result = Histogram::new(doc.composition().color);
    let extent = [doc.composition().size[0], doc.composition().size[1]];
    let counts = if preview { extent.map(|v| v.min(256)) } else { extent };
    for iy in 0..counts[1] {
        for ix in 0..counts[0] {
            let x = if preview { ((2 * u64::from(ix) + 1) * u64::from(extent[0]) / (2 * u64::from(counts[0]))) as u32 } else { ix };
            let y = if preview { ((2 * u64::from(iy) + 1) * u64::from(extent[1]) / (2 * u64::from(counts[1]))) as u32 } else { iy };
            if admitted(x, y) { result.add(&[pixels[((x + 3 * y) as usize) % pixels.len()]]).unwrap(); }
        }
    }
    result
}

fn same(actual: Histogram, expected: Histogram, context: impl std::fmt::Debug) {
    assert_eq!((actual.color,actual.domain),(expected.color,expected.domain),"{context:?}: interpretation");
    assert_eq!((actual.pixels, actual.transparent), (expected.pixels, expected.transparent), "{context:?}: counts");
    for (index, (a, e)) in actual.channels.iter().zip(&expected.channels).enumerate() {
        assert_eq!((a.below, a.above, a.black, a.white), (e.below, e.above, e.black, e.white), "{context:?}: channel {index} endpoints");
        let differences: Vec<_> = a.bins.iter().zip(&e.bins).enumerate().filter(|(_, (a, e))| a != e).collect();
        assert!(differences.is_empty(), "{context:?}: channel {index} bins {differences:?}");
    }
}

#[test]
fn statistics_exact_matches_f64_oracle_at_every_profile_and_depth() {
    let pixels = [[0.; 4], [1., 1., 1., 1.], [0.125, 0.25, 0.375, 0.5], [-0.25, 0.5, 1.5, 1.], [0.01, 0.005, 0.0025, 0.125], [0.75, 0.25, 0.125, 1.]];
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
            let doc = generated([31, 19], DocumentColor { space, depth }, &pixels);
            same(statistics(&doc, false, false).unwrap(), oracle(&doc, &pixels, false, |_, _| true), (space, depth));
        }
    }
}

#[test]
fn statistics_preview_samples_original_grid_without_replacing_transparent_points() {
    let pixels = [[0.; 4], [0.125, 0.25, 0.5, 1.], [4., -0.125, 2., 0.5], [1., 1., 1., 1.], [0.25, 0.125, 0.0625, 0.25]];
    for extent in [[513, 273], [257, 3], [1, 517], [17, 13]] {
        let doc = generated(extent, DocumentColor { depth: SampleDepth::F32, ..Default::default() }, &pixels);
        let expected = oracle(&doc, &pixels, true, |_, _| true);
        let actual = statistics(&doc, true, false).unwrap();
        assert_eq!(actual.pixels + actual.transparent, u64::from(extent[0].min(256)) * u64::from(extent[1].min(256)));
        same(actual, expected, extent);
    }
}

fn coverage_selection(extent: [u32; 2]) -> Selection {
    let mut words = vec![0u32; (extent[0].div_ceil(4) * extent[1]) as usize];
    for y in 0..extent[1] {
        for x in 0..extent[0] {
            let value = [0, 1, 17, 128, 255][((x + 3 * y) % 5) as usize];
            words[(y * extent[0].div_ceil(4) + x / 4) as usize] |= value << ((x % 4) * 8);
        }
    }
    Selection::pixels(Arc::new(SelectionPixels::bytes(extent, [0, 0, extent[0], extent[1]], words).unwrap()))
}

#[test]
fn statistics_selection_counts_partial_coverage_once_in_exact_and_preview() {
    let pixels = [[0.25, 0.125, 0.0625, 0.25], [0.5, 0.25, 0.125, 0.5], [0.; 4], [1.; 4]];
    let mut doc = generated([513, 273], DocumentColor { depth: SampleDepth::F32, ..Default::default() }, &pixels);
    doc.working.selection = Some(coverage_selection([doc.composition().size[0], doc.composition().size[1]]));
    for preview in [false, true] {
        same(statistics(&doc, preview, true).unwrap(), oracle(&doc, &pixels, preview, |x, y| (x + 3 * y) % 5 != 0), preview);
    }
    doc.working.selection.as_mut().unwrap().inverted = true;
    for preview in [false, true] {
        same(statistics(&doc, preview, true).unwrap(), oracle(&doc, &pixels, preview, |x, y| (x + 3 * y) % 5 != 4), (preview, "inverted"));
    }
    doc.working.selection = None;
    assert!(statistics(&doc, false, true).is_err());
}

#[test]
fn statistics_frozen_source_and_cancel_do_not_publish_changed_pixels() {
    let mut doc = generated([17, 13], DocumentColor { depth: SampleDepth::F32, ..Default::default() }, &[[0.25, 0.5, 1., 1.]]);
    let request = ArtworkStatisticsRequest { waveform: false, query: ArtworkQuery::new(&doc, ArtworkSource::Visible), preview: false, selection: false };
    let expected = oracle(&doc, &[[0.25, 0.5, 1., 1.]], false, |_, _| true);
    let owner=doc.scene().children(None)[0];set_effect(&mut doc,owner,doubled_effect());
    assert!(!request.query.matches_artwork(&doc));
    assert_eq!(pollster::block_on(gpu().artwork_statistics(request.clone(), CaptureControl::default())).unwrap(), expected);
    let control = CaptureControl::default();
    control.cancel();
    assert!(pollster::block_on(gpu().artwork_statistics(request, control)).is_err());
}

#[test]
fn statistics_extreme_finite_rgb_uses_f64_classification_without_unassociation_overflow() {
    let pixels = [[1e10, 5e9, 2.5e9, 1e-30], [f32::MAX, f32::MAX, f32::MAX, 1.], [-f32::MAX, -f32::MAX, -f32::MAX, 1.], [f32::from_bits(1); 4], [0., 0., 0., 1.], [1.; 4]];
    for space in RgbSpace::ALL {
        for (index, pixel) in pixels.iter().enumerate() {
            let doc = generated([1, 1], DocumentColor { space, depth: SampleDepth::F32 }, std::slice::from_ref(pixel));
            same(statistics(&doc, false, false).unwrap_or_else(|error| panic!("{space:?} pixel {index} {pixel:?}: {error}")), oracle(&doc, std::slice::from_ref(pixel), false, |_, _| true), (space, index));
        }
    }
}

#[test]
fn statistics_reads_real_source_codecs_at_every_profile_and_depth() {
    let pixels = [[0., 0., 0., 1.], [1.; 4], [1., 0., 0., 1.], [0., 1., 0., 1.], [0., 0., 1., 1.], [0.; 4]];
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
            let mut doc = document_in([pixels.len() as u32, 1], space, |_, _| [0.; 4]);
            doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().color=DocumentColor {space,depth};
            doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().blend=BlendSpace::Linear;
            paint_mut(&mut doc).raster=Default::default();
            paint_mut(&mut doc).original=Some(crate::test_support::depth_source([pixels.len() as u32, 1], depth, space, 8 * 1024 * 1024, |x, _| pixels[x as usize]));
            same(statistics(&doc, false, false).unwrap(), oracle(&doc, &pixels, false, |_, _| true), (space, depth));
        }
    }
}

#[test]
fn statistics_luminance_preserves_sign_and_bins_under_extreme_cancellation() {
    for space in RgbSpace::ALL {
        let red = 1e30f32;
        let weight = space.to_xyz()[1][0];
        let green = (-f64::from(red) * weight / (1. - weight)) as f32;
        let pixels: Vec<_> = (-8i32..=8).map(|offset| {
            let value = f32::from_bits((i64::from(green.to_bits()) + i64::from(offset)) as u32);
            [red, value, value, 1.]
        }).collect();
        let doc = generated([pixels.len() as u32, 1], DocumentColor { space, depth: SampleDepth::F32 }, &pixels);
        same(statistics(&doc, false, false).unwrap(), oracle(&doc, &pixels, false, |_, _| true), space);
    }
}

#[test]
fn statistics_luminance_bins_match_f64_on_both_sides_of_boundaries() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::F16] {
            let color = DocumentColor { space, depth };
            let weights = space.to_xyz()[1];
            let pixels: Vec<_> = Histogram::new(color).boundaries(3).into_iter().skip(1).take(255).flat_map(|boundary| {
                let green = (boundary / (1. - weights[0] - weights[2])) as f32;
                [green.to_bits() - 1, green.to_bits(), green.to_bits() + 1].map(|bits| [0., f32::from_bits(bits), 0., 1.])
            }).collect();
            let doc = generated([pixels.len() as u32, 1], color, &pixels);
            same(statistics(&doc, false, false).unwrap(), oracle(&doc, &pixels, false, |_, _| true), (space, depth));
        }
    }
}

#[test]
fn statistics_rgb_exact_correction_matches_f64_at_boundary_neighbors() {
    for space in RgbSpace::ALL {
        for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
            let color = DocumentColor { space, depth };
            let pixels: Vec<_> = Histogram::new(color).boundaries(0).into_iter().skip(1).take(255).flat_map(|boundary| {
                let value = (boundary * 0.5) as f32;
                let bits = value.to_bits();
                [bits.saturating_sub(1), bits, bits + 1].map(|bits| {
                    let value = f32::from_bits(bits);
                    [value, value, value, 0.5]
                })
            }).collect();
            let doc = generated([pixels.len() as u32, 1], color, &pixels);
            same(statistics(&doc, false, false).unwrap(), oracle(&doc, &pixels, false, |_, _| true), (space, depth));
        }
    }
}

#[test]
fn presenter_clipping_marks_each_rgb_lane_without_changing_artwork_or_statistics() {
    use layer_render::CanvasRenderer;
    let pixels = [[0.;4],[-0.25,0.25,0.125,1.],[-0.25,1.,0.125,1.],[0.25,0.5,0.125,0.5],[0.,0.125,0.25,0.5],[0.1,0.2,0.3,1.],[0.5;4],[-0.5,-0.25,-0.125,0.5]];
    let extent = [8,8];
    let mut doc = generated(extent, DocumentColor { depth:SampleDepth::F32, ..Default::default() }, &pixels);
    let owner=doc.scene().children(None)[0];
    let coverage=doc.artwork.coverage.next_handle();let mut mask=layer_core::CoverageSnapshot::reveal_all(coverage,extent,layer_core::Point::default());
    mask.source.default_coverage=0.5;doc.artwork.coverage.insert(PortableId::random(),mask.source).unwrap();doc.artwork.occurrences.get_mut(owner).unwrap().mask=Some(mask.use_);refresh(&mut doc);
    let mut r = crate::WgpuRasterizer::new_native_headless(doc.composition().color).unwrap();
    let view = crate::test_support::view(extent);
    r.submit(layer_render::FramePacket { blend_space:doc.composition().blend, ..crate::test_support::packet(doc.scene(), extent) }).unwrap();
    while r.has_pending_work() {r.wait_idle().unwrap();r.submit(layer_render::FramePacket { blend_space:doc.composition().blend, composite_all:false, ..crate::test_support::packet(doc.scene(), extent) }).unwrap();}
    let mut presenter = crate::test_support::float_presenter(&r);
    let (texture,target) = crate::create_target(&r.device,extent,wgpu::TextureFormat::Rgba32Float,"clipping regression");
    presenter.present(&r,&target,view,[0.;4]).unwrap();
    let baseline = crate::test_support::float_pixels(&r,&texture);
    let artwork = r.readback_srgb_rgba8().unwrap();
    let request = ArtworkStatisticsRequest { waveform: false,query:ArtworkQuery::new(&doc,ArtworkSource::Visible),preview:false,selection:false};
    let before = pollster::block_on(r.snapshot_gpu().artwork_statistics(request.clone(),CaptureControl::default())).unwrap();
    let mut references=Vec::new();
    for (shadows,highlights) in [(true,false),(false,true),(true,true)] {
        r.set_clipping_preview(shadows,highlights);
        assert!(presenter.needs_present(&r,view,[0.;4]));
        presenter.present(&r,&target,view,[0.;4]).unwrap();
        let actual = crate::test_support::float_pixels(&r,&texture);
        references.push(actual.clone());
        for y in 0..extent[1] {for x in 0..extent[0] {
            let color = pixels[((x+3*y) as usize)%pixels.len()];
            let index = (y*extent[0]+x) as usize;
            let dark = color[3]>0. && shadows && color[..3].iter().any(|v| *v<=0.);
            let light = color[3]>0. && highlights && color[..3].iter().any(|v| *v>=color[3]);
            let stripe = ((x+y+1)/4)%2==0;
            let expected = match (dark,light) {
                (true,true) => [if stripe {1.}else{0.};3],
                (true,false) => if stripe {[0.12,0.3,1.]}else{[0.02,0.05,0.5]},
                (false,true) => if stripe {[1.,0.2,0.05]}else{[0.5,0.02,0.02]},
                _ => baseline[index][..3].try_into().unwrap(),
            };
            for channel in 0..3 {assert!((actual[index][channel]-expected[channel]).abs()<2e-5,"flags {shadows}/{highlights}, pixel {x}/{y}, channel {channel}: {:?} != {expected:?}",actual[index]);}
        }}
        assert_eq!(r.readback_srgb_rgba8().unwrap(),artwork);
        assert_eq!(pollster::block_on(r.snapshot_gpu().artwork_statistics(request.clone(),CaptureControl::default())).unwrap(),before);
    }
    doc.working.inspect_mask=Some(owner);
    let masked_request=ArtworkStatisticsRequest {waveform:false,query:ArtworkQuery::new(&doc,ArtworkSource::Visible),preview:false,selection:false};
    let mut attached=crate::AttachedRenderer(Some(Box::new(r)));
    attached.set_clipping_preview(false,false);
    attached.submit(layer_render::FramePacket {blend_space:doc.composition().blend,inspect_mask:doc.working.inspect_mask,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let r=attached.0.as_mut().unwrap();
    presenter.present(r,&target,view,[0.;4]).unwrap();
    let tinted=crate::test_support::float_pixels(r,&texture);
    assert!(tinted.iter().zip(&baseline).any(|(a,b)|a.iter().zip(b).any(|(a,b)|(a-b).abs()>0.01)));
    for ((shadows,highlights),expected) in [(true,false),(false,true),(true,true)].into_iter().zip(references) {
        attached.set_clipping_preview(shadows,highlights);
        attached.submit(layer_render::FramePacket {blend_space:doc.composition().blend,inspect_mask:doc.working.inspect_mask,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
        let r=attached.0.as_mut().unwrap();
        presenter.present(r,&target,view,[0.;4]).unwrap();
        let actual=crate::test_support::float_pixels(r,&texture);
        assert!(actual.iter().zip(expected).all(|(a,b)|a.iter().zip(b).all(|(a,b)|(a-b).abs()<2e-5)),"mask tint changed clipping {shadows}/{highlights}");
        assert_eq!(r.readback_srgb_rgba8().unwrap(),artwork);
        assert_eq!(pollster::block_on(r.snapshot_gpu().artwork_statistics(masked_request.clone(),CaptureControl::default())).unwrap(),before);
        assert_eq!(doc.working.inspect_mask,Some(owner));
    }
    attached.set_clipping_preview(false,false);
    attached.submit(layer_render::FramePacket {blend_space:doc.composition().blend,inspect_mask:doc.working.inspect_mask,..crate::test_support::packet(doc.scene(),extent)}).unwrap();
    let r=attached.0.as_mut().unwrap();
    presenter.present(r,&target,view,[0.;4]).unwrap();
    assert_eq!(crate::test_support::float_pixels(r,&texture),tinted);
    assert_eq!(r.readback_srgb_rgba8().unwrap(),artwork);
}

fn dense_preview_oracle(doc: &Document) -> Histogram {
    let mut snapshot = gpu().capture_scene(doc.snapshot(),SceneScope::All,CaptureControl::default()).unwrap();
    let pixels = snapshot.read_region([0,0,doc.composition().size[0],doc.composition().size[1]]).unwrap();
    let mut result = Histogram::new(doc.composition().color);
    let grid = [doc.composition().size[0].min(256),doc.composition().size[1].min(256)];
    for row in 0..grid[1] {for column in 0..grid[0] {
        let x = ((2*u64::from(column)+1)*u64::from(doc.composition().size[0])/(2*u64::from(grid[0]))) as usize;
        let y = ((2*u64::from(row)+1)*u64::from(doc.composition().size[1])/(2*u64::from(grid[1]))) as usize;
        result.add(&[pixels[y*doc.composition().size[0] as usize+x]]).unwrap();
    }}
    result
}

#[test]
fn sparse_statistics_transformed_source_matches_original_dense_pixels_across_windows() {
    let extent = [1033,517];
    let mut doc = document_in(extent,RgbSpace::DisplayP3,|_,_|[0.;4]);
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().blend=BlendSpace::Linear;
    paint_mut(&mut doc).raster=Default::default();
    let source_extent = [797,401];
    paint_mut(&mut doc).original=Some(crate::test_support::depth_source(source_extent,SampleDepth::F32,doc.composition().color.space,32*1024*1024,|x,y| {
        let alpha = [0.,0.125,0.5,1.][((x/7+y/11)%4) as usize];
        [0.1+x as f32/397.,0.2+y as f32/199.,0.37,alpha]
    }));
    paint_mut(&mut doc).domain=source_extent;
    let owner=paint_occurrence(&doc);
    doc.artwork.occurrences.get_mut(owner).unwrap().placement= layer_core::LayerPlacement::from_projective(layer_core::Projective::rect_to_quad(layer_core::Rect::from_extent(source_extent),
        [[7.,13.],[1021.,1.],[1000.,499.],[-12.,507.]].map(|[x,y]|layer_core::Point{x,y})).unwrap());
    same(statistics(&doc,true,false).unwrap(),dense_preview_oracle(&doc),"transformed source");
}

#[test]
fn sparse_statistics_nested_clipped_spatial_and_document_image_match_dense_pixels() {
    for global in [false,true] {
        let extent = [1033,517];
        let pixels = [[0.2,0.1,0.4,0.5],[0.75,0.25,0.5,1.],[0.;4]];
        let mut doc = generated(extent,DocumentColor {depth:SampleDepth::F32,..Default::default()},&pixels);
        let base=doc.scene().children(None)[0];
        let spatial=insert_effect(&mut doc,crate::tests::image_windows::program(false,global),0);
        let occurrence=doc.artwork.occurrences.get_mut(spatial).unwrap();occurrence.opacity=0.63;occurrence.clipped=true;
        let coverage=doc.artwork.coverage.next_handle();let mut mask=layer_core::CoverageSnapshot::reveal_all(coverage,extent,layer_core::Point{x:7.,y:-9.});
        mask.source.default_coverage=0.;mask.source.initial=Some(Selection::polygon([[0.,0.],[1020.,99.],[440.,517.]].map(|[x,y]|layer_core::Point{x,y}).to_vec()).unwrap());
        doc.artwork.coverage.insert(PortableId::random(),mask.source).unwrap();doc.artwork.occurrences.get_mut(spatial).unwrap().mask=Some(mask.use_);
        let group=add_group(&mut doc,vec![spatial,base],0);doc.artwork.occurrences.get_mut(group).unwrap().opacity=0.79;
        let outside=insert_effect(&mut doc,crate::tests::image_windows::program(true,false),1);
        let root=doc.composition().result;doc.artwork.stacks.get_mut(root).unwrap().entries=vec![group,outside];refresh(&mut doc);
        same(statistics(&doc,true,false).unwrap(),dense_preview_oracle(&doc),("nested clipped image",global));
    }
}

fn curve_coordinate(value:f64, space:RgbSpace, logarithmic:bool) -> f64 {
    if !logarithmic {return space.encode(value);}
    let toe = std::f64::consts::E/256.;
    if value<=toe {value/(toe*12.*std::f64::consts::LN_2)}else{(value.log2()+8.)/12.}
}
fn curve_linear(value:f64, space:RgbSpace, logarithmic:bool) -> f64 {
    if !logarithmic {return space.decode(value);}
    let knee = 1./(12.*std::f64::consts::LN_2);
    if value<=knee {value*(std::f64::consts::E/256.)*12.*std::f64::consts::LN_2}else{2f64.powf(12.*value-8.)}
}
fn curve_histogram_oracle(doc:&Document,pixels:&[[f32;4]],logarithmic:bool,channels:bool) -> Histogram {
    use layer_core::color::histogram::HistogramDomain;
    let mut result = Histogram::new(doc.composition().color);
    result.domain = if logarithmic {HistogramDomain::CurveLog{stops:4.}}else{HistogramDomain::Encoded};
    for pixel in pixels {
        if pixel[3]==0. {result.transparent+=1;continue;}
        result.pixels+=1;
        let mut rgb = [0,1,2].map(|c|f64::from(pixel[c])/f64::from(pixel[3]));
        if channels {for (value,factor) in rgb.iter_mut().zip([0.5,1.,0.75]) {
            *value=curve_linear(curve_coordinate(*value,doc.composition().color.space,logarithmic)*factor,doc.composition().color.space,logarithmic);
        }}
        let weights = doc.composition().color.space.to_xyz()[1];
        let y = rgb[1]+weights[0]*(rgb[0]-rgb[1])+weights[2]*(rgb[2]-rgb[1]);
        for (channel,value) in result.channels.iter_mut().zip([rgb[0],rgb[1],rgb[2],y]) {
            let coordinate = curve_coordinate(value,doc.composition().color.space,logarithmic);
            let index = (coordinate.clamp(0.,1.)*256.).floor().min(255.) as usize;
            channel.bins[index]+=1;
            channel.below+=u64::from(value<0.);channel.above+=u64::from(value>1.);
            channel.black+=u64::from(value<=0.);channel.white+=u64::from(value>=1.);
        }
    }
    result
}

#[test]
fn statistics_curve_input_and_channels_use_typed_domain_without_master_mask_or_opacity() {
    use layer_core::{EffectInstance,EffectValue,Point};
    for space in RgbSpace::ALL {for logarithmic in [false,true] {for channels in [false,true] {
        let pixels = if logarithmic {[[0.05,0.2,0.75,1.],[0.025,0.05,0.1,0.5],[2.3,4.7,8.9,1.],[0.;4]]}
            else {[[0.05,0.2,0.75,1.],[0.025,0.05,0.1,0.5],[0.17,0.39,0.81,1.],[0.;4]]};
        let mut doc = generated([pixels.len() as u32,1],DocumentColor {space,depth:SampleDepth::F32},&pixels);
        let mut effect = EffectInstance::new(crate::tests::fixture("curves").program());
        effect.set("domain",EffectValue::Choice(u32::from(logarithmic))).unwrap();
        effect.set("curve_0",EffectValue::Curve(vec![[0.,0.],[1.,0.25]].into())).unwrap();
        if channels {
            effect.set("curve_1",EffectValue::Curve(vec![[0.,0.],[1.,0.5]].into())).unwrap();
            effect.set("curve_3",EffectValue::Curve(vec![[0.,0.],[1.,0.75]].into())).unwrap();
        }
        let adjustment=insert_effect(&mut doc,effect,0);doc.artwork.occurrences.get_mut(adjustment).unwrap().opacity=0.;
        let coverage=doc.artwork.coverage.next_handle();let mut mask=layer_core::CoverageSnapshot::reveal_all(coverage,doc.composition().size,Point::default());mask.source.default_coverage=0.;
        doc.artwork.coverage.insert(PortableId::random(),mask.source).unwrap();doc.artwork.occurrences.get_mut(adjustment).unwrap().mask=Some(mask.use_);
        for source in [ArtworkSource::EffectInput(adjustment),ArtworkSource::EffectChannels(adjustment)] {
            let channel_source = matches!(source,ArtworkSource::EffectChannels(_));
            let expected = curve_histogram_oracle(&doc,&pixels,logarithmic,channels && channel_source);
            let actual = pollster::block_on(gpu().artwork_statistics(ArtworkStatisticsRequest { waveform: false,query:ArtworkQuery::new(&doc,source.clone()),preview:false,selection:false},CaptureControl::default())).unwrap();
            same(actual,expected,(space,logarithmic,channels,source));
        }
    }}}
}

#[cfg(test)]
mod waveform {
    use super::*;

    fn inspect(doc: &Document, preview: bool, selection: bool, waveform: bool) -> Result<Histogram, String> {
        pollster::block_on(gpu().artwork_statistics(ArtworkStatisticsRequest {
            query: ArtworkQuery::new(doc, ArtworkSource::Visible), preview, selection, waveform,
        }, CaptureControl::default()))
    }

    fn encoded(value: f64, space: RgbSpace) -> f64 {
        let x = value.abs();
        let y = match space {
            RgbSpace::Srgb | RgbSpace::DisplayP3 => if x <= 0.0031308 { 12.92 * x } else { 1.055 * x.powf(1. / 2.4) - 0.055 },
            RgbSpace::AdobeRgb => x.powf(256. / 563.),
            RgbSpace::ProPhoto => if x <= 1. / 512. { 16. * x } else { x.powf(1. / 1.8) },
        };
        y.copysign(value)
    }

    fn reference(doc: &Document, preview: bool, pixel: impl Fn(u32, u32) -> [f32; 4], admitted: impl Fn(u32, u32) -> bool) -> (Histogram, Vec<u32>) {
        let mut histogram = Histogram::new(doc.composition().color);
        let mut counts = vec![0u32; 4 * 256 * 256];
        let extent = doc.composition().size;
        let grid = if preview { extent.map(|n| n.min(256)) } else { extent };
        let row = doc.composition().color.space.to_xyz()[1];
        let weights = [row[0], 1. - row[0] - row[2], row[2]];
        for iy in 0..grid[1] {
            for ix in 0..grid[0] {
                let world = [0, 1].map(|axis| if preview {
                    ((2 * u64::from([ix, iy][axis]) + 1) * u64::from(extent[axis]) / (2 * u64::from(grid[axis]))) as u32
                } else { [ix, iy][axis] });
                let [x, y] = world;
                if !admitted(x, y) { continue; }
                let rgba = pixel(x, y);
                if rgba[3] == 0. { histogram.transparent += 1; continue; }
                histogram.pixels += 1;
                let rgb = [0, 1, 2].map(|i| f64::from(rgba[i]) / f64::from(rgba[3]));
                let luminance: f64 = if rgb[0] == rgb[1] && rgb[1] == rgb[2] { rgb[0] }
                    else { rgb.into_iter().zip(weights).map(|(value, weight)| value * weight).sum() };
                for (channel, value) in rgb.into_iter().chain([luminance]).enumerate() {
                    let bin = if doc.composition().color.depth.is_float() {
                        let (low, span) = if doc.composition().color.depth == SampleDepth::F32 { (-149., 277.) } else { (-12., 28.) };
                        if value <= 0. { 0 } else { 1 + (((value.log2() - low) / span).clamp(0., 1.) * 254.).floor() as usize }
                    } else {
                        let coordinate = if channel < 3 { encoded(value, doc.composition().color.space) } else { value };
                        (coordinate.clamp(0., 1.) * 256.).floor().min(255.) as usize
                    };
                    let lane = &mut histogram.channels[channel];
                    lane.bins[bin] += 1;
                    lane.below += u64::from(value < 0.); lane.above += u64::from(value > 1.);
                    lane.black += u64::from(value <= 0.); lane.white += u64::from(value >= 1.);
                    let column = (u64::from(x) * 256 / u64::from(extent[0])) as usize;
                    counts[(channel * 256 + bin) * 256 + column] += 1;
                }
            }
        }
        (histogram, counts)
    }

    fn matches(actual: &Histogram, expected: (Histogram, Vec<u32>), context: impl std::fmt::Debug) {
        same(actual.clone(), expected.0, &context);
        let counts = &actual.waveform.as_ref().expect("requested waveform").counts;
        assert_eq!(counts.len(), 4 * 256 * 256, "{context:?}: waveform layout");
        let differences: Vec<_> = counts.iter().zip(&expected.1).enumerate().filter(|(_, (a, e))| a != e)
            .take(12).map(|(index, (a, e))| (index / 65536, (index / 256) % 256, index % 256, *a, *e)).collect();
        assert!(differences.is_empty(), "{context:?}: (channel, bin, world-column, actual, expected) {differences:?}");
        for channel in 0..4 {
            for bin in 0..256 {
                let row = &counts[(channel * 256 + bin) * 256..(channel * 256 + bin + 1) * 256];
                assert_eq!(row.iter().map(|n| u64::from(*n)).sum::<u64>(), actual.channels[channel].bins[bin], "{context:?}: waveform collapses to histogram");
            }
        }
    }

    fn row_document(extent: [u32; 2], color: DocumentColor, row: &[[f32; 4]]) -> Document {
        let mut doc = generated(extent, color, row);
        let handle=doc.scene().order()[0];let definition=doc.scene().effect_application(handle).unwrap().definition;
        let program=Arc::make_mut(&mut doc.artwork.definitions.get_mut(definition).unwrap().program);
        program.wgsl = program.wgsl.sources().unwrap()[0].replace("+3u*u32(floor(p.y))", "").into();
        doc
    }

    #[test]
    fn reversed_artwork_keeps_histogram_but_reverses_waveform_columns() {
        let row: Vec<_> = (0..256).map(|x| { let red = x as f32 / 255.; [red, 0.25, 1. - red, 1.] }).collect();
        let reversed: Vec<_> = row.iter().copied().rev().collect();
        for space in RgbSpace::ALL {
            let color = DocumentColor { space, depth: SampleDepth::U8 };
            let doc = row_document([256, 7], color, &row);
            let flipped = row_document([256, 7], color, &reversed);
            let a = inspect(&doc, false, false, true).unwrap();
            let b = inspect(&flipped, false, false, true).unwrap();
            matches(&a, reference(&doc, false, |x, _| row[x as usize], |_, _| true), (space, "original"));
            matches(&b, reference(&flipped, false, |x, _| reversed[x as usize], |_, _| true), (space, "reversed"));
            assert_eq!(a.channels, b.channels, "{space:?}: reversal keeps distributions");
            let a = &a.waveform.as_ref().unwrap().counts;
            let b = &b.waveform.as_ref().unwrap().counts;
            assert_ne!(a, b, "{space:?}: spatial inspection distinguishes distributions");
            for channel in 0..4 { for bin in 0..256 { for column in 0..256 {
                assert_eq!(a[(channel * 256 + bin) * 256 + column], b[(channel * 256 + bin) * 256 + 255 - column]);
            } } }
        }
    }

    #[test]
    fn waveform_counts_original_positions_at_odd_preview_and_selection_edges() {
        let pixels = [[0.; 4], [0.125, 0.25, 0.5, 1.], [2., -0.0625, 1., 0.5], [1.; 4],
            [1e-8, 2e-8, 4e-8, 8e-8], [0.25, 0.125, 0.0625, 0.25], [0.0625, 0.125, 0.25, 0.5]];
        for space in RgbSpace::ALL {
            let mut doc = generated([517, 259], DocumentColor { space, depth: SampleDepth::F32 }, &pixels);
            doc.working.selection = Some(coverage_selection(doc.composition().size));
            for preview in [false, true] {
                for selected in [false, true] {
                    let actual = inspect(&doc, preview, selected, true).unwrap();
                    matches(&actual, reference(&doc, preview, |x, y| pixels[((x + 3 * y) as usize) % pixels.len()], |x, y| !selected || (x + 3 * y) % 5 != 0), (space, preview, selected));
                    if !selected { assert_eq!(actual.pixels + actual.transparent, if preview { 256 * 256 } else { 517 * 259 }); }
                }
                doc.working.selection.as_mut().unwrap().inverted = true;
                let actual = inspect(&doc, preview, true, true).unwrap();
                matches(&actual, reference(&doc, preview, |x, y| pixels[((x + 3 * y) as usize) % pixels.len()], |x, y| (x + 3 * y) % 5 != 4), (space, preview, "inverted"));
                doc.working.selection.as_mut().unwrap().inverted = false;
            }
        }
    }

    #[test]
    fn waveform_reuses_exact_rgb_and_luminance_classification_for_hdr_and_tiny_alpha() {
        let pixels = [[0.; 4], [0., 0., 0., 1.], [-1., 0.5, 4., 1.], [1.; 4], [4., 4., 4., 1.],
            [3e38, -2e38, 1e38, 1.], [f32::from_bits(1), f32::from_bits(2), f32::from_bits(3), f32::from_bits(1)]];
        for space in RgbSpace::ALL {
            for depth in [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16, SampleDepth::F32] {
                let doc = generated([7, 3], DocumentColor { space, depth }, &pixels);
                matches(&inspect(&doc, false, false, true).unwrap(), reference(&doc, false, |x, y| pixels[((x + 3 * y) as usize) % pixels.len()], |_, _| true), (space, depth));
            }
        }
    }

    #[test]
    fn histogram_without_waveform_demand_does_not_return_spatial_counts() {
        let pixels = [[0.; 4], [0.125, 0.25, 0.5, 1.], [1.; 4]];
        let doc = generated([19, 13], DocumentColor::default(), &pixels);
        let plain = inspect(&doc, false, false, false).unwrap();
        assert!(plain.waveform.is_none());
        let spatial = inspect(&doc, false, false, true).unwrap();
        same(plain, spatial, "optional waveform preserves histogram");
        let control = CaptureControl::default(); control.cancel();
        assert!(pollster::block_on(gpu().artwork_statistics(ArtworkStatisticsRequest {
            query: ArtworkQuery::new(&doc, ArtworkSource::Visible), preview: false, selection: false, waveform: true,
        }, control)).is_err());
    }

    #[test]
    fn uniform_waveform_preserves_every_full_scan_count_without_stale_demand() {
        let pixel = [0.25, 0.25, 0.25, 1.];
        let doc = generated([1025, 1027], DocumentColor::default(), &[pixel]);
        let expected = reference(&doc, false, |_, _| pixel, |_, _| true);
        inspect(&doc, true, false, false).unwrap();
        for demand in [false, true, false, true] {
            let started = std::time::Instant::now();
            let actual = inspect(&doc, false, false, demand).unwrap();
            eprintln!("uniform full statistics waveform={demand} elapsed_ms={}", started.elapsed().as_millis());
            assert_eq!(actual.pixels, 1025 * 1027);
            if demand { matches(&actual, expected.clone(), "uniform hot bins"); }
            else { assert!(actual.waveform.is_none()); same(actual, expected.0.clone(), "uniform histogram only"); }
        }
    }
}
