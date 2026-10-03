use crate::{ArtworkSampleRequest, ArtworkSource, Document, DocumentNames, LayerId, Point};

fn document() -> Document {
    Document::new("Query", 128, 96, DocumentNames { paint: "Ink".into(), paper: "Paper".into() })
}

#[test]
fn artwork_sample_admission_checks_contact_width_extent_and_source_kind() {
    let doc = document();
    for width in [1, 5, 15, 51, 101] {
        assert!(ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [-1., 96.], width).validate().is_ok());
    }
    for width in [0, 2, 3, 102, u32::MAX] {
        assert!(ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [0.; 2], width).validate().is_err());
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut request = ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [value, 0.], 5);
        assert!(request.validate().is_err());
        request.position = [0.; 2];
        request.time = value;
        assert!(request.validate().is_err());
        request.time = 0.;
        request.effect_times.push((doc.layers[0].id, value));
        assert!(request.validate().is_err());
    }
    assert!(ArtworkSampleRequest::new(&doc, ArtworkSource::LayerContent(doc.layers[0].id), [0.; 2], 5).validate().is_ok());
    for source in [ArtworkSource::LayerContent(doc.layers[1].id), ArtworkSource::LayerContent(LayerId(u64::MAX)), ArtworkSource::EffectInput(doc.layers[0].id)] {
        assert!(ArtworkSampleRequest::new(&doc, source, [0.; 2], 5).validate().is_err());
    }
    let mut oversized = doc.clone();
    oversized.width = 32769;
    assert!(ArtworkSampleRequest::new(&oversized, ArtworkSource::Visible, [0.; 2], 5).validate().is_err());
}

#[test]
fn frozen_artwork_identity_ignores_names_and_detects_pixel_dependencies() {
    let doc = document();
    let request = ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [7.5, 9.5], 5);
    let mut renamed = doc.clone();
    renamed.layers[0].name = "Renamed ink".into();
    assert!(request.matches_artwork(&renamed));
    let mutations: [fn(&mut Document); 5] = [
        |d: &mut Document| d.width += 1,
        |d: &mut Document| d.layers[0].visible = false,
        |d: &mut Document| d.layers[0].opacity = 0.5,
        |d: &mut Document| d.layers[0].properties.placement = crate::LayerPlacement::from_affine(crate::Affine::translation(Point { x: 1., y: 2. })),
        |d: &mut Document| d.color.depth = crate::color::SampleDepth::F32,
    ];
    for mutate in mutations {
        let mut changed = doc.clone();
        mutate(&mut changed);
        assert!(!request.matches_artwork(&changed));
    }
    let mut advanced = request.clone();
    advanced.time = 123.;
    advanced.effect_times.push((doc.layers[0].id, 456.));
    assert!(advanced.matches_artwork(&doc));
}

#[test]
fn reference_query_identity_rejects_changed_membership() {
    let doc = document();
    let visible = ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [0.; 2], 5);
    let reference = ArtworkSampleRequest::new(&doc, ArtworkSource::Reference, [0.; 2], 5);
    let mut changed = doc.clone();
    changed.reference_layers.insert(changed.layers[0].id);
    assert!(visible.matches_artwork(&changed));
    assert!(!reference.matches_artwork(&changed));
}

#[test]
fn effect_source_identity_ignores_own_composition_but_keeps_lower_source_dependencies_strict() {
    for effect in ["curves","levels"] {
        let mut doc=document();for _ in 0..100 {doc.allocate_layer_id();}
        let mut layer=crate::Layer::paint(LayerId(91),effect);
        layer.kind=crate::LayerKind::Effect;layer.effect=Some(std::sync::Arc::new(crate::EffectInstance::new(crate::bundled_effect_catalog().get(effect).unwrap().program())));
        doc.layers.insert(0,layer);
        let mutations:[fn(&mut crate::Layer);3]=[
            |layer|layer.opacity=0.5,
            |layer|{let mut mask=crate::LayerMask::reveal_all(LayerId(99),Point::default());mask.default_coverage=0.5;layer.mask=Some(mask);},
            |layer|layer.properties.blend=crate::LayerBlend::Multiply,
        ];
        for source in [ArtworkSource::EffectInput(LayerId(91)),ArtworkSource::EffectChannels(LayerId(91))] {
            let query=crate::ArtworkQuery::new(&doc,source);
            for mutate in mutations {
                let mut own=doc.clone();mutate(&mut own.layers[0]);
                assert!(query.matches_source(&own),"{effect}: own composition excluded");
                assert!(!query.matches_artwork(&own),"explicit corrections require strict frozen state");
                let mut lower=doc.clone();mutate(&mut lower.layers[1]);
                assert!(!query.matches_source(&lower),"{effect}: lower composition contributes");
            }
        }
    }
}

#[test]
fn white_balance_solver_matches_independent_gain_ratios_without_rounding() {
    for space in [crate::color::RgbSpace::Srgb, crate::color::RgbSpace::DisplayP3, crate::color::RgbSpace::ProPhoto] {
        for preserve in [false, true] {
            for (temperature, tint) in [(0., 0.), (137.25f64, -63.875f64), (-1000., 0.), (1000., 0.), (0., -800.), (0., 800.)] {
                let gains = [0.008 * temperature + 0.0025 * tint, -0.005 * tint, -0.008 * temperature + 0.0025 * tint].map(f64::exp2);
                let input = gains.map(|gain| (0.25 / gain) as f32);
                let solved = crate::white_balance_neutral(input, space, preserve).unwrap();
                assert!((f64::from(solved[0]) - temperature).abs() < 0.0001);
                assert!((f64::from(solved[1]) - tint).abs() < 0.0001);
            }
        }
    }
}

#[test]
fn white_balance_solver_refuses_invalid_channels_and_unreachable_casts() {
    let space = crate::color::RgbSpace::Srgb;
    for rgb in [[0., 1., 1.], [-1., 1., 1.], [f32::NAN, 1., 1.], [f32::INFINITY, 1., 1.], [1., 1., 131072.], [1., 128., 1.]] {
        assert!(crate::white_balance_neutral(rgb, space, false).is_err(), "accepted {rgb:?}");
    }
    assert_eq!(crate::white_balance_neutral([1e-20; 3], space, true).unwrap(), [0.; 2]);
}

#[test]
fn artwork_sample_identity_rejects_unpublished_pixel_commands() {
    let doc = document();
    let request = ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [0.; 2], 5);
    let mut changed = doc.clone();
    changed.layers[0].pending_operations.push(crate::LayerOperation {
        placement: crate::Affine::IDENTITY,
        coverage: crate::LayerMask::reveal_all(LayerId(99), Point::default()),
        kind: crate::LayerOperationKind::Fill { color: [1., 0., 0., 1.], alpha_locked: false },
    });
    assert!(!request.matches_artwork(&changed));
}

#[test]
fn artwork_sample_identity_freezes_source_backing_and_raster_roots() {
    let mut doc = document();
    doc.layers[0].source = Some(crate::color::source::rgba8_source([128, 96], |_, _| [32, 64, 128, 255]));
    let request = ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [0.; 2], 5);
    assert!(request.matches_artwork(&doc));
    let mut replacement = doc.clone();
    replacement.layers[0].source = Some(std::sync::Arc::new((**doc.layers[0].source.as_ref().unwrap()).clone()));
    assert!(!request.matches_artwork(&replacement));
    let mut changed = doc.clone();
    let mut data = crate::raster::RasterData::default();
    data.tiles.insert(crate::raster::TileKey { plane: crate::raster::RasterPlane::Color, coordinate: [0, 0] },
        crate::raster::RasterTile::backed(crate::raster::TileBlob::encode(doc.color.paint_descriptor(), &[0; 256 * 256 * 4]).unwrap()));
    changed.layers[0].raster = crate::raster::RasterRevision::backed(data);
    assert!(!request.matches_artwork(&changed));
}

#[test]
fn histogram_typed_curve_domains_use_coordinate_bins_for_rgb_and_luminance() {
    use crate::color::{DocumentColor,RgbSpace,SampleDepth,histogram::{Histogram,HistogramDomain}};
    let pixels=[[0.0625,0.25,0.375,0.5],[-0.5,0.5,4.,1.],[0.;4]];
    for space in RgbSpace::ALL {for domain in [HistogramDomain::Encoded,HistogramDomain::CurveLog{stops:4.}] {
        let mut actual=Histogram::new(DocumentColor{space,depth:SampleDepth::F32});actual.domain=domain;
        actual.add(&pixels).unwrap();
        let mut bins:[Vec<u64>;4]=std::array::from_fn(|_|vec![0;256]);
        for pixel in &pixels[..2] {
            let rgb=[0,1,2].map(|c|f64::from(pixel[c])/f64::from(pixel[3]));
            let weights=space.to_xyz()[1];let y=rgb[1]+weights[0]*(rgb[0]-rgb[1])+weights[2]*(rgb[2]-rgb[1]);
            for (channel,value) in [rgb[0],rgb[1],rgb[2],y].into_iter().enumerate() {
                let coordinate=match domain {
                    HistogramDomain::Encoded=>space.encode(value),
                    _=>{let toe=std::f64::consts::E/256.;if value<=toe {value/(toe*12.*std::f64::consts::LN_2)}else{(value.log2()+8.)/12.}},
                };
                bins[channel][(coordinate.clamp(0.,1.)*256.).floor().min(255.) as usize]+=1;
            }
        }
        for (channel,expected) in actual.channels.iter().zip(bins) {assert_eq!(channel.bins,expected,"{space:?} {domain:?}");}
        assert_eq!((actual.pixels,actual.transparent),(2,1));
        assert_eq!(actual.plot_bins(),0..256);
        assert_eq!(actual.axis().bins,[0,256]);assert!(actual.axis().stops.is_none());
    }}
}

fn input_key_effect(id:u64)->crate::Layer {
    let mut layer=crate::Layer::paint(LayerId(id),"Exposure");layer.kind=crate::LayerKind::Effect;
    layer.effect=Some(std::sync::Arc::new(crate::bundled_effect_catalog().get("exposure").unwrap().preview().unwrap()));layer
}
fn input_key_document()->Document {
    let mut doc=document();for _ in 0..100 {doc.allocate_layer_id();}
    doc.layers.insert(0,input_key_effect(20));doc.layers.insert(0,input_key_effect(30));doc
}
#[test]
fn effect_input_key_ignores_upper_changes_and_own_consuming_values() {
    let doc=input_key_document();let query=crate::ArtworkQuery::new(&doc,ArtworkSource::EffectInput(LayerId(20)));
    let mutations:[fn(&mut crate::Layer);5]=[
        |l|l.opacity=0.4,|l|l.properties.blend=crate::LayerBlend::Multiply,
        |l|l.properties.offset=Point{x:4.,y:7.},
        |l|{let mut m=crate::LayerMask::reveal_all(LayerId(99),Point::default());m.default_coverage=0.25;l.mask=Some(m);},
        |l|{std::sync::Arc::make_mut(l.effect.as_mut().unwrap()).set("exposure",crate::EffectValue::Number(1.)).unwrap();},
    ];
    for mutate in mutations {let mut changed=doc.clone();mutate(&mut changed.layers[0]);assert!(query.matches_source(&changed),"upper");}
    for mutate in [mutations[0],mutations[1],mutations[3],mutations[4]] {let mut changed=doc.clone();mutate(&mut changed.layers[1]);assert!(query.matches_source(&changed),"own consuming state");}
    let mut inserted=doc.clone();inserted.layers.insert(0,input_key_effect(40));assert!(query.matches_source(&inserted));
    let mut removed=doc.clone();removed.layers.remove(0);assert!(query.matches_source(&removed));
    let mut lower=doc.clone();lower.layers[2].opacity=0.3;assert!(!query.matches_source(&lower));
    let mut lower=doc.clone();lower.layers.swap(2,3);assert!(!query.matches_source(&lower));
    let mut lower=doc.clone();lower.layers.insert(2,input_key_effect(40));assert!(!query.matches_source(&lower));
}

#[test]
fn effect_input_key_tracks_noncontiguous_group_contributors_and_clipping() {
    for pass_through in [false,true] {for clipped in [false,true] {
        let mut doc=document();for _ in 0..100 {doc.allocate_layer_id();}
        let mut child=crate::Layer::paint(LayerId(1),"Child");child.properties.parent=Some(LayerId(10));
        let mut target=input_key_effect(20);target.properties.clipped=clipped;
        let mut group=crate::Layer::paint(LayerId(10),"Lower group");group.kind=crate::LayerKind::Group;
        group.properties.blend=if pass_through {crate::LayerBlend::PassThrough}else{crate::LayerBlend::Normal};
        doc.layers=vec![child,target,group];
        let key=crate::artwork_query::EffectInputKey::new(std::sync::Arc::new(doc.clone()),LayerId(20)).unwrap();
        assert!(key.contributors().any(|l|l.id==LayerId(1)),"pass-through={pass_through} clipped={clipped}");
        let query=crate::ArtworkQuery::new(&doc,ArtworkSource::EffectInput(LayerId(20)));
        for mutate in [|l:&mut crate::Layer|l.opacity=0.4,|l:&mut crate::Layer|l.properties.offset=Point{x:1.,y:2.},|l:&mut crate::Layer|{let mut m=crate::LayerMask::reveal_all(LayerId(99),Point::default());m.default_coverage=0.2;l.mask=Some(m);}] {
            let mut changed=doc.clone();mutate(&mut changed.layers[0]);assert!(!query.matches_source(&changed));
        }
        let mut changed=doc.clone();changed.layers[2].opacity=0.4;assert!(!query.matches_source(&changed));
        let mut changed=doc.clone();changed.layers[0].properties.parent=None;assert!(!query.matches_source(&changed));
    }}
}

#[test]
fn artwork_query_public_source_and_snapshot_mutations_cannot_reuse_an_obsolete_input_key() {
    let doc=input_key_document();let mut query=crate::ArtworkQuery::new(&doc,ArtworkSource::EffectInput(LayerId(20)));
    query.source=ArtworkSource::EffectInput(LayerId(30));
    let mut changed=doc.clone();std::sync::Arc::make_mut(changed.layers[1].effect.as_mut().unwrap()).set("exposure",crate::EffectValue::Number(1.)).unwrap();
    assert!(!query.matches_source(&changed),"old target is now a contributing lower adjustment");
    query.source=ArtworkSource::EffectInput(LayerId(20));
    let mut snapshot=doc.clone();snapshot.layers[2].opacity=0.25;query.document=std::sync::Arc::new(snapshot.clone());
    assert!(query.matches_source(&snapshot));assert!(!query.matches_source(&doc));
    let snapshot=std::sync::Arc::make_mut(&mut query.document);snapshot.layers[2].opacity=0.75;
    assert!(query.matches_source(&query.document));assert!(!query.matches_source(&doc));
}

#[test]
fn effect_input_key_distinguishes_isolated_and_pass_through_ancestor_backdrops() {
    for pass_through in [false,true] {for clipped in [false,true] {
        let mut doc=document();for _ in 0..100 {doc.allocate_layer_id();}
        let mut target=input_key_effect(20);target.properties.parent=Some(LayerId(10));target.properties.clipped=clipped;
        let mut sibling=crate::Layer::paint(LayerId(1),"Lower sibling");sibling.properties.parent=Some(LayerId(10));
        let mut group=crate::Layer::paint(LayerId(10),"Parent");group.kind=crate::LayerKind::Group;group.properties.blend=if pass_through {crate::LayerBlend::PassThrough}else{crate::LayerBlend::Normal};
        doc.layers=vec![target,sibling,group,crate::Layer::paint(LayerId(2),"Root backdrop")];
        let query=crate::ArtworkQuery::new(&doc,ArtworkSource::EffectInput(LayerId(20)));
        let mut changed=doc.clone();changed.layers[1].opacity=0.25;assert!(!query.matches_source(&changed));
        let mut changed=doc.clone();changed.layers[3].opacity=0.25;
        assert_eq!(query.matches_source(&changed),!pass_through||clipped,"pass-through={pass_through} clipped={clipped}");
        let mut changed=doc.clone();changed.layers[2].properties.offset=Point{x:2.,y:3.};assert!(!query.matches_source(&changed));
        let mut changed=doc.clone();changed.layers[2].properties.blend=if pass_through {crate::LayerBlend::Normal}else{crate::LayerBlend::PassThrough};assert!(!query.matches_source(&changed));
    }}
}
