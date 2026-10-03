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
