use crate::{artwork_sample_tests::gpu,artwork_statistics_tests::generated,snapshot::CaptureControl};
use layer_core::{ArtworkQuery,ArtworkSource,Document,EffectInstance,EffectValue,Layer,LayerId,LayerKind};
use layer_core::color::{DocumentColor,RgbSpace,SampleDepth};
use std::sync::Arc;

fn fixture(space:RgbSpace,depth:SampleDepth,pixels:&[[f32;4]])->Document {
    let mut doc=generated([pixels.len() as u32,1],DocumentColor {space,depth},pixels);
    let mut layer=Layer::paint(LayerId(90),"Levels");layer.kind=LayerKind::Effect;
    layer.effect=Some(Arc::new(EffectInstance::new(crate::tests::fixture("levels").program().for_depth(depth))));
    doc.layers.insert(0,layer);doc
}
fn stats(doc:&Document,channels:bool)->Result<layer_core::levels::LevelsStatistics,String> {
    let source=if channels {ArtworkSource::EffectChannels(LayerId(90))}else{ArtworkSource::EffectInput(LayerId(90))};
    pollster::block_on(gpu().levels_statistics(ArtworkQuery::new(doc,source),CaptureControl::default()))
}
fn close(a:f64,b:f64,tolerance:f64) {assert!((a-b).abs()<=tolerance,"{a} != {b}, tolerance{tolerance}");}

#[test]
fn levels_statistics_full_tiles_cover_every_pixel_across_window_edges() {
    let extent=[517,259];let pixels=[[0.02,0.11,0.33,1.],[0.27,0.39,0.51,1.],[0.83,0.72,0.61,1.]];
    let mut doc=generated(extent,DocumentColor {space:RgbSpace::Srgb,depth:SampleDepth::F32},&pixels);
    let mut layer=fixture(RgbSpace::Srgb,SampleDepth::F32,&pixels).layers.remove(0);
    layer.id=LayerId(90);doc.layers.insert(0,layer);
    let result=stats(&doc,false).unwrap();let count=u64::from(extent[0])*u64::from(extent[1]);
    assert_eq!(result.pixels,count);
    for c in 0..3 {
        assert_eq!(result.bins[c].iter().sum::<u64>(),count);
        let mut values:Vec<_>=pixels.iter().map(|p|RgbSpace::Srgb.encode(f64::from(p[c]))).collect();values.sort_by(f64::total_cmp);
        close(result.minimum[c],values[0],3e-6);close(result.maximum[c],values[2],3e-6);
        let anchors=result.stretch(c as u8+1).unwrap();let tolerance=(values[2]-values[0])/4096.+3e-6;
        close(anchors[0],values[0],tolerance);close(anchors[1],values[2],tolerance);
    }
}

#[test]
fn levels_statistics_quantiles_match_independent_sorted_samples_across_profiles_and_depths() {
    let pixels:Vec<_>=(0..1001).map(|i| {
        let t=i as f32/1000.;let alpha=if i%7==0 {0.25}else{1.};
        [(0.02+0.9*t)*alpha,(0.13+0.6*t)*alpha,(0.05+0.8*t)*alpha,alpha]
    }).collect();
    for space in RgbSpace::ALL {for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F16,SampleDepth::F32] {
        let doc=fixture(space,depth,&pixels);let result=stats(&doc,false).unwrap();
        assert_eq!(result.pixels,1001);
        for c in 0..3 {
            assert_eq!(result.bins[c].iter().sum::<u64>(),1001);
            let mut sorted:Vec<_>=pixels.iter().map(|p|space.encode(f64::from(p[c])/f64::from(p[3]))).collect();
            sorted.sort_by(f64::total_cmp);
            let width=(sorted[1000]-sorted[0])/4096.;
            close(result.minimum[c],sorted[0],3e-6);
            close(result.maximum[c],sorted[1000],3e-6);
            let anchors=result.stretch(c as u8+1).unwrap();
            close(anchors[0],sorted[1],width+3e-6);
            close(anchors[1],sorted[999],width+3e-6);
        }
    }}
}

#[test]
fn levels_statistics_reads_real_integer_and_float_source_codecs_at_every_profile() {
    let pixels=[[0.,0.,0.,1.],[1.;4],[1.,0.,0.,1.],[0.,1.,0.,1.],[0.,0.,1.,1.],[0.;4]];
    for space in RgbSpace::ALL {for depth in [SampleDepth::U8,SampleDepth::U16,SampleDepth::F16,SampleDepth::F32] {
        let mut doc=fixture(space,depth,&pixels);doc.layers.remove(1);
        doc.layers[1].source=Some(crate::test_support::depth_source([6,1],depth,space,8*1024*1024,|x,_|pixels[x as usize]));
        let result=stats(&doc,false).unwrap();assert_eq!(result.pixels,5);
        for c in 0..3 {
            close(result.minimum[c],0.,1e-6);close(result.maximum[c],1.,1e-6);
            assert_eq!(result.bins[c][0],3);assert_eq!(result.bins[c][4095],2);
            assert_eq!(result.bins[c].iter().sum::<u64>(),5);
            let anchors=result.stretch(c as u8+1).unwrap();close(anchors[0],0.,1./4096.);close(anchors[1],1.,1./4096.);
        }
    }}
}

#[test]
fn levels_statistics_channels_precede_master_and_complete_shader_uses_channels_then_master() {
    let pixels=[[0.04,0.18,0.39,0.5],[0.4,0.6,0.8,1.],[0.;4]];
    let mut doc=fixture(RgbSpace::Srgb,SampleDepth::F32,&pixels);
    let effect=Arc::make_mut(doc.layers[0].effect.as_mut().unwrap());
    for (key,value) in [("output_black",0.1),("output_white",0.8),("gamma",1.8),("red_output_white",0.5),("green_gamma",1.4),("blue_output_black",0.1)] {
        effect.set(key,EffectValue::Number(value)).unwrap();
    }
    for channels in [false,true] {
        let result=stats(&doc,channels).unwrap();assert_eq!(result.pixels,2);
        for c in 0..3 {
            let mut values:Vec<_>=pixels[..2].iter().map(|p| {
                let x=RgbSpace::Srgb.encode(f64::from(p[c])/f64::from(p[3]));
                if channels {match c {0=>x*0.5,1=>x.powf(1./f64::from(1.4f32)),_=>0.1+0.9*x}}else{x}
            }).collect();values.sort_by(f64::total_cmp);
            close(result.minimum[c],values[0],4e-6);close(result.maximum[c],values[1],4e-6);
        }
    }
    let request=layer_core::ArtworkSampleRequest::new(&doc,ArtworkSource::Visible,[1.,0.],1);
    let layer_core::ArtworkSample::Color(actual)=pollster::block_on(gpu().artwork_sample(request,CaptureControl::default())).unwrap() else {panic!("missing Levels output")};
    for c in 0..3 {
        let x=RgbSpace::Srgb.encode(f64::from(pixels[1][c]));
        let channel=match c {0=>x*0.5,1=>x.powf(1./f64::from(1.4f32)),_=>0.1+0.9*x};
        let expected=RgbSpace::Srgb.decode(f64::from(0.1f32)+(f64::from(0.8f32)-f64::from(0.1f32))*channel.powf(1./f64::from(1.8f32)));
        close(f64::from(actual[c]),expected,5e-6);
    }
}

#[test]
fn levels_statistics_constant_empty_hdr_tiny_alpha_and_cancellation_are_explicit() {
    for space in RgbSpace::ALL {
        let pixels=[[0.025,0.1,0.4,0.5],[0.1,0.2,0.8,1.]];
        let doc=fixture(space,SampleDepth::F32,&pixels);let result=stats(&doc,false).unwrap();
        assert_eq!(result.minimum[1],result.maximum[1]);
        assert!(result.stretch(2).is_err());assert!(result.stretch(0).is_ok());
        let doc=fixture(space,SampleDepth::F32,&[[0.2;4]]);let result=stats(&doc,false).unwrap();
        assert!(result.stretch(0).is_err());
        let doc=fixture(space,SampleDepth::F32,&[[0.;4]]);assert!(stats(&doc,false).is_err());
        let doc=fixture(space,SampleDepth::F32,&[[1e-30,2e-30,4e-30,1e-30],[2.,4.,8.,1.]]);
        let result=stats(&doc,false).unwrap();assert_eq!(result.pixels,2);
        for c in 0..3 {close(result.minimum[c],space.encode((1u32<<c) as f64),4e-6);close(result.maximum[c],space.encode((2u32<<c) as f64),4e-6);}
        let subnormal=fixture(space,SampleDepth::F32,&[[f32::from_bits(1);4]]);
        let result=stats(&subnormal,false).unwrap();assert_eq!(result.pixels,1);
        for c in 0..3 {close(result.minimum[c],1.,4e-6);close(result.maximum[c],1.,4e-6);}
        let extreme=[[-2.,-4.,-8.,1.],[1e25,2e25,4e25,1e-30]];
        let signed=fixture(space,SampleDepth::F32,&extreme);let result=stats(&signed,false).unwrap();
        assert_eq!(result.pixels,2);
        for c in 0..3 {
            let low=space.encode(f64::from(extreme[0][c]));
            let high=space.encode(f64::from(extreme[1][c])/f64::from(extreme[1][3]));
            close(result.minimum[c],low,low.abs()*2e-5);
            close(result.maximum[c],high,high.abs()*2e-5);
            assert_eq!(result.bins[c].iter().sum::<u64>(),2);
        }
        let control=CaptureControl::default();control.cancel();
        assert!(pollster::block_on(gpu().levels_statistics(ArtworkQuery::new(&doc,ArtworkSource::EffectInput(LayerId(90))),control)).is_err());
    }
}
