use crate::{ArtworkSampleRequest, ArtworkSource, Document, DocumentNames, Edit, authored::*};
use std::sync::Arc;
use crate::operation_test_support as fixture;
fn document() -> Document {
    Document::new(PortableId::random(), 128, 96, DocumentNames { paint: "Ink".into(), paper: "Paper".into() })
}
fn change_occurrence(document: &mut Document, handle: OccurrenceHandle, mutate: impl FnOnce(&mut Occurrence)) {
    let mut value = document.artwork.occurrences.get(handle).unwrap().clone();
    mutate(&mut value);
    let edit = Edit::Occurrence(RecordChange::replace(&document.artwork.occurrences, handle, Some(value)).unwrap());
    document.apply(edit).unwrap();
}
fn change_paint(document: &mut Document, handle: PaintHandle, mutate: impl FnOnce(&mut PaintSource)) {
    let mut value = document.artwork.paint.get(handle).unwrap().clone();
    mutate(&mut value);
    let edit = Edit::Paint(RecordChange::replace(&document.artwork.paint, handle, Some(value)).unwrap());
    document.apply(edit).unwrap();
}
fn change_mask(document: &mut Document, handle: OccurrenceHandle, value: f32) {
    let target = document.artwork.coverage.next_handle();
    let mut coverage = crate::CoverageSnapshot::reveal_all(target, document.composition().size, [0; 2]);
    coverage.source.default_coverage = value;
    let mut occurrence = document.artwork.occurrences.get(handle).unwrap().clone();
    occurrence.mask = Some(coverage.use_);
    let edit = Edit::Batch(vec![
        Edit::Coverage(RecordChange::insert(&document.artwork.coverage, coverage.source)),
        Edit::Occurrence(RecordChange::replace(&document.artwork.occurrences, handle, Some(occurrence)).unwrap()),
    ]);
    document.apply(edit).unwrap();
}
fn set_effect_values(document: &mut Document, handle: OccurrenceHandle, values: &[(&str, crate::EffectValue)]) {
    let effect = document.scene().effect_handle(handle).unwrap();
    let mut application = document.artwork.effects.get(effect).unwrap().clone();
    let program = application.program.clone();
    let mut instance = crate::EffectInstance::new(program);
    instance.values = application.values;
    for (key, value) in values { instance.set(key, value.clone()).unwrap(); }
    application.values = instance.values;
    let edit = Edit::Effect(RecordChange::replace(&document.artwork.effects, effect, Some(application)).unwrap());
    document.apply(edit).unwrap();
}
fn change_effect(document: &mut Document, handle: OccurrenceHandle) {
    set_effect_values(document, handle, &[("exposure", crate::EffectValue::Number(1.))]);
}
#[test]
fn artwork_sample_admission_checks_contact_width_extent_and_source_kind() {
    let doc = document();
    let ink = fixture::id(&doc, "Ink");
    let target = fixture::target(&doc, "Ink");
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
        request.set_context(EvaluationContext { elapsed: value, phases: vec![].into() });
        assert!(request.validate().is_err());
        request.set_context(EvaluationContext { elapsed: 0., phases: vec![(EffectHandle::INVALID, value)].into() });
        assert!(request.validate().is_err());
    }
    assert!(ArtworkSampleRequest::new(&doc, ArtworkSource::Source(target), [0.; 2], 5).validate().is_ok());
    for source in [
        ArtworkSource::Source(SourceTarget::Coverage(CoverageHandle::INVALID)),
        ArtworkSource::Source(SourceTarget::Paint(PaintHandle::INVALID)),
        ArtworkSource::EffectInput(ink),
    ] {
        assert!(ArtworkSampleRequest::new(&doc, source, [0.; 2], 5).validate().is_err());
    }
    let mut oversized = doc.clone();
    oversized.artwork.compositions.get_mut(oversized.artwork.root).unwrap().size[0] = 32769;
    assert!(ArtworkSampleRequest::new(&oversized, ArtworkSource::Visible, [0.; 2], 5).validate().is_err());
}
#[test]
fn frozen_artwork_identity_ignores_names_and_detects_pixel_dependencies() {
    let doc = document();
    let ink = fixture::id(&doc, "Ink");
    let request = ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [7.5, 9.5], 5);
    let mut renamed = doc.clone();
    change_occurrence(&mut renamed, ink, |o| o.name = "Renamed ink".into());
    assert!(request.matches_artwork(&renamed));
    let mutations: [fn(&mut Occurrence); 3] = [
        |o| o.visible = false,
        |o| o.opacity = 0.5,
        |o| o.offset = [1, 2],
    ];
    for mutate in mutations {
        let mut changed = doc.clone();
        change_occurrence(&mut changed, ink, mutate);
        assert!(!request.matches_artwork(&changed));
    }
    for mutate in [|c: &mut Composition| c.size[0] += 1, |c: &mut Composition| c.color.depth = crate::color::SampleDepth::F32] {
        let mut changed = doc.clone();
        let mut composition = changed.composition().clone();
        mutate(&mut composition);
        let edit =
            Edit::Composition(RecordChange::replace(&changed.artwork.compositions, changed.artwork.root, Some(composition)).unwrap());
        changed.apply(edit).unwrap();
        assert!(!request.matches_artwork(&changed));
    }
    let mut advanced = request.clone();
    advanced.set_context(EvaluationContext { elapsed: 123., phases: vec![(EffectHandle::INVALID, 456.)].into() });
    assert!(advanced.matches_artwork(&doc));
}
#[test]
fn reference_query_identity_rejects_changed_membership() {
    let doc = document();
    let ink = fixture::id(&doc, "Ink");
    let visible = ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [0.; 2], 5);
    let reference = ArtworkSampleRequest::new(&doc, ArtworkSource::Reference, [0.; 2], 5);
    let mut changed = doc.clone();
    change_occurrence(&mut changed, ink, |o| o.reference = true);
    assert!(visible.matches_artwork(&changed));
    assert!(!reference.matches_artwork(&changed));
}
#[test]
fn effect_source_identity_ignores_own_composition_but_keeps_lower_source_dependencies_strict() {
    for effect in ["curves", "levels"] {
        let mut doc = fixture::document([128, 96], &["Effect", "Ink"]);
        fixture::effect(&mut doc, "Effect", effect);
        let target = fixture::id(&doc, "Effect");
        let lower = fixture::id(&doc, "Ink");
        let mutations: [fn(&mut Document, OccurrenceHandle); 3] = [
            |d, h| change_occurrence(d, h, |o| o.opacity = 0.5),
            |d, h| change_mask(d, h, 0.5),
            |d, h| change_occurrence(d, h, |o| o.blend = crate::LayerBlend::Multiply),
        ];
        for source in [ArtworkSource::EffectInput(target), ArtworkSource::EffectChannels(target)] {
            let query = crate::ArtworkQuery::new(&doc, source);
            for mutate in mutations {
                let mut own = doc.clone();
                mutate(&mut own, target);
                assert!(query.matches_source(&own), "{effect}: own composition excluded");
                assert!(!query.matches_artwork(&own), "explicit corrections require strict frozen state");
                let mut below = doc.clone();
                mutate(&mut below, lower);
                assert!(!query.matches_source(&below), "{effect}: lower composition contributes");
            }
        }
    }
}
#[test]
fn artwork_sample_identity_rejects_unpublished_pixel_commands() {
    let doc = document();
    let SourceTarget::Paint(target) = fixture::target(&doc, "Ink") else { panic!() };
    let request = ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [0.; 2], 5);
    let mut changed = doc.clone();
    let coverage =
        crate::CoverageSnapshot::reveal_all(changed.artwork.coverage.next_handle(), changed.composition().size, [0; 2]);
    change_paint(&mut changed, target, |p| {
        Arc::make_mut(&mut p.operations).push(crate::RasterOperation {
            placement: crate::Affine::IDENTITY,
            coverage,
            kind: crate::RasterOperationKind::Fill { color: [1., 0., 0., 1.], alpha_locked: false },
        })
    });
    assert!(!request.matches_artwork(&changed));
}
#[test]
fn artwork_sample_identity_freezes_source_backing_and_raster_roots() {
    let mut doc = document();
    let SourceTarget::Paint(target) = fixture::target(&doc, "Ink") else { panic!() };
    change_paint(&mut doc, target, |p| p.base = Some(PaintBase::new(crate::color::source::rgba8_source([128, 96], |_, _| [32, 64, 128, 255]).into())));
    let request = ArtworkSampleRequest::new(&doc, ArtworkSource::Visible, [0.; 2], 5);
    assert!(request.matches_artwork(&doc));
    let mut replacement = doc.clone();
    change_paint(&mut replacement, target, |p| p.base = Some(PaintBase::new(Arc::new(p.base.as_ref().unwrap().image.as_ref().clone()).into())));
    assert!(!request.matches_artwork(&replacement));
    let mut changed = doc.clone();
    let descriptor = doc.composition().color.paint_descriptor();
    let mut data = crate::raster::RasterData::default();
    data.tiles.insert(
        crate::raster::TileKey { plane: crate::raster::RasterPlane::Color, coordinate: [0, 0] },
        crate::raster::RasterTile::backed(crate::raster::TileBlob::encode(descriptor, &[0; 256 * 256 * 4]).unwrap()),
    );
    change_paint(&mut changed, target, |p| p.raster = crate::raster::RasterRevision::backed(data));
    assert!(!request.matches_artwork(&changed));
}
fn input_key_document() -> Document {
    let mut doc = fixture::document([128, 96], &["Upper", "Target", "Ink"]);
    fixture::effect(&mut doc, "Upper", "exposure");
    fixture::effect(&mut doc, "Target", "exposure");
    doc
}
fn insert_effect(doc: &mut Document, index: usize) -> OccurrenceHandle {
    let handle = fixture::insert_paint(doc, "Inserted", index, None);
    fixture::effect(doc, "Inserted", "exposure");
    handle
}
#[test]
fn effect_input_key_ignores_upper_changes_and_own_consuming_values() {
    let doc = input_key_document();
    let target = fixture::id(&doc, "Target");
    let upper = fixture::id(&doc, "Upper");
    let ink = fixture::id(&doc, "Ink");
    let query = crate::ArtworkQuery::new(&doc, ArtworkSource::EffectInput(target));
    let mutations: [fn(&mut Document, OccurrenceHandle); 4] = [
        |d, h| change_occurrence(d, h, |o| o.opacity = 0.4),
        |d, h| change_occurrence(d, h, |o| o.blend = crate::LayerBlend::Multiply),
        |d, h| change_mask(d, h, 0.25),
        change_effect,
    ];
    for mutate in mutations {
        let mut changed = doc.clone();
        mutate(&mut changed, upper);
        assert!(query.matches_source(&changed), "upper");
    }
    for mutate in mutations {
        let mut changed = doc.clone();
        mutate(&mut changed, target);
        assert!(query.matches_source(&changed), "own consuming state");
    }
    let mut inserted = doc.clone();
    insert_effect(&mut inserted, 0);
    assert!(query.matches_source(&inserted));
    let mut removed = doc.clone();
    let edit = removed.delete_layers_edit(&[upper]).unwrap();
    removed.apply(edit).unwrap();
    assert!(query.matches_source(&removed));
    let mut lower = doc.clone();
    change_occurrence(&mut lower, ink, |o| o.opacity = 0.3);
    assert!(!query.matches_source(&lower));
    let mut lower = doc.clone();
    let stack = lower.composition().result;
    let mut value = lower.artwork.stacks.get(stack).unwrap().clone();
    value.entries.swap(2, 3);
    let edit = Edit::Stack(RecordChange::replace(&lower.artwork.stacks, stack, Some(value)).unwrap());
    lower.apply(edit).unwrap();
    assert!(!query.matches_source(&lower));
    let mut lower = doc.clone();
    insert_effect(&mut lower, 2);
    assert!(!query.matches_source(&lower));
}
#[test]
fn effect_input_snapshot_identity_expires_on_elapsed_animation_and_preserves_frozen_phases() {
    let mut doc = fixture::document([128, 96], &["Upper", "Target", "Animated", "Ink"]);
    fixture::effect(&mut doc, "Upper", "domain_warp");
    fixture::effect(&mut doc, "Target", "exposure");
    fixture::effect(&mut doc, "Animated", "domain_warp");
    let target = fixture::id(&doc, "Target");
    let animated = fixture::id(&doc, "Animated");
    let upper = fixture::id(&doc, "Upper");
    for handle in [upper, animated] {
        set_effect_values(&mut doc, handle, &[("animate", crate::EffectValue::Toggle(true)), ("speed", crate::EffectValue::Number(2.))]);
    }
    let effect = doc.scene().effect_handle(animated).unwrap();
    let upper_effect = doc.scene().effect_handle(upper).unwrap();
    let context = |elapsed, phases| EvaluationContext { elapsed, phases: std::sync::Arc::new(phases) };
    let query = crate::ArtworkQuery::from_snapshot(doc.snapshot_with_context(context(1., vec![])), ArtworkSource::EffectInput(target), None);
    assert!(query.matches_snapshot(&doc.snapshot_with_context(context(1., vec![]))));
    assert!(!query.matches_snapshot(&doc.snapshot_with_context(context(2., vec![]))), "elapsed fallback changes the contributed pixels");
    assert!(query.matches_source(&doc), "authored source identity has no frame context");
    assert!(query.matches_snapshot(&doc.snapshot_with_context(context(9., vec![(effect, 2.), (upper_effect, 100.)]))), "equal effective phase survives a different clock and unrelated upper phase");
    let frozen = crate::ArtworkQuery::from_snapshot(doc.snapshot_with_context(context(1., vec![(effect, 7.)])), ArtworkSource::EffectInput(target), None);
    assert!(frozen.matches_snapshot(&doc.snapshot_with_context(context(9., vec![(effect, 7.)]))));
    assert!(!frozen.matches_snapshot(&doc.snapshot_with_context(context(1., vec![(effect, 8.)]))));
    assert!(!frozen.matches_snapshot(&doc.snapshot_with_context(context(1., vec![]))));
}
#[test]
fn snapshot_identity_ignores_elapsed_for_static_time_and_unrelated_effects() {
    let mut doc = fixture::document([128, 96], &["Upper", "Target", "Static", "Ink"]);
    fixture::effect(&mut doc, "Upper", "domain_warp");
    fixture::effect(&mut doc, "Target", "exposure");
    fixture::effect(&mut doc, "Static", "domain_warp");
    let upper = fixture::id(&doc, "Upper");
    let target = fixture::id(&doc, "Target");
    let static_effect = fixture::id(&doc, "Static");
    set_effect_values(&mut doc, upper, &[("animate", crate::EffectValue::Toggle(true))]);
    set_effect_values(&mut doc, static_effect, &[("animate", crate::EffectValue::Toggle(false)), ("time", crate::EffectValue::Number(3.))]);
    let target_effect = doc.scene().effect_handle(target).unwrap();
    let query = crate::ArtworkQuery::from_snapshot(doc.snapshot_with_context(EvaluationContext { elapsed: 1., phases: vec![].into() }), ArtworkSource::EffectInput(target), None);
    let later = doc.snapshot_with_context(EvaluationContext { elapsed: 20., phases: vec![(target_effect, 8.)].into() });
    assert!(query.matches_snapshot(&later), "static lower input and consuming non-time effect ignore elapsed and unrelated animation");
    set_effect_values(&mut doc, upper, &[("animate", crate::EffectValue::Toggle(false))]);
    let visible = crate::ArtworkQuery::from_snapshot(doc.snapshot_with_context(EvaluationContext { elapsed: 1., phases: vec![].into() }), ArtworkSource::Visible, None);
    assert!(visible.matches_snapshot(&doc.snapshot_with_context(EvaluationContext { elapsed: 20., phases: vec![(target_effect, 8.)].into() })), "effects without a time input ignore irrelevant captured phases");
    set_effect_values(&mut doc, static_effect, &[("animate", crate::EffectValue::Toggle(true))]);
    let visible = crate::ArtworkQuery::from_snapshot(doc.snapshot_with_context(EvaluationContext { elapsed: 1., phases: vec![].into() }), ArtworkSource::Visible, None);
    assert!(!visible.matches_snapshot(&doc.snapshot_with_context(EvaluationContext { elapsed: 20., phases: vec![].into() })));
}
#[test]
fn raw_source_snapshot_identity_ignores_unrelated_effect_phases() {
    let mut doc=fixture::document([128,96], &["Animated","Ink"]);
    fixture::effect(&mut doc,"Animated","domain_warp");
    let animated=fixture::id(&doc,"Animated");
    set_effect_values(&mut doc,animated,&[("animate",crate::EffectValue::Toggle(true))]);
    let effect=doc.scene().effect_handle(animated).unwrap();let target=fixture::target(&doc,"Ink");
    let query=crate::ArtworkQuery::from_snapshot(doc.snapshot_with_context(EvaluationContext{elapsed:1.,phases:vec![].into()}),ArtworkSource::Source(target),None);
    assert!(query.matches_snapshot(&doc.snapshot_with_context(EvaluationContext{elapsed:20.,phases:vec![].into()})));
    assert!(query.matches_snapshot(&doc.snapshot_with_context(EvaluationContext{elapsed:20.,phases:vec![(effect,73.)].into()})));
    let SourceTarget::Paint(paint)=target else {panic!()};
    change_paint(&mut doc,paint,|source|source.base=Some(PaintBase::new(crate::color::source::rgba8_source([128,96],|_,_|[0,0,0,255]).into())));
    assert!(!query.matches_snapshot(&doc.snapshot_with_context(EvaluationContext{elapsed:1.,phases:vec![].into()})),"Raw source pixel dependencies remain strict");
}
#[test]
fn effect_input_key_tracks_noncontiguous_group_contributors_and_clipping() {
    for pass_through in [false, true] {
        for attached in [false, true] {
            let mut doc = fixture::document([128, 96], &["Child", "Target", "Group"]);
            fixture::effect(&mut doc, "Target", "exposure");
            fixture::nest(&mut doc, "Group", &["Child"]);
            let target = fixture::id(&doc, "Target");
            let child = fixture::id(&doc, "Child");
            let group = fixture::id(&doc, "Group");
            change_occurrence(&mut doc, group, |o| {
                o.blend = if pass_through { crate::LayerBlend::PassThrough } else { crate::LayerBlend::Normal }
            });
            if attached && pass_through && doc.scene().parent(target)!=Some(group) {assert!(doc.attachment_edit(target,true,false).is_err());continue;}
            change_occurrence(&mut doc, target, |o| o.attachment = if attached { crate::Attachment::Effect } else { crate::Attachment::None });
            let key = crate::artwork_query::EffectInputKey::new(doc.snapshot(), target).unwrap();
            assert!(key.contributors().any(|h| h == child), "pass-through={pass_through} attached={attached}");
            let query = crate::ArtworkQuery::new(&doc, ArtworkSource::EffectInput(target));
            let mutations: [fn(&mut Document, OccurrenceHandle); 3] = [
                |d, h| change_occurrence(d, h, |o| o.opacity = 0.4),
                |d, h| change_occurrence(d, h, |o| o.offset = [1, 2]),
                |d, h| change_mask(d, h, 0.2),
            ];
            for mutate in mutations {
                let mut changed = doc.clone();
                mutate(&mut changed, child);
                assert!(!query.matches_source(&changed));
            }
            let mut changed = doc.clone();
            change_occurrence(&mut changed, group, |o| o.opacity = 0.4);
            assert!(!query.matches_source(&changed));
            let mut changed = doc.clone();
            let root = changed.composition().result;
            let child_stack = changed.scene().stack(child).unwrap();
            let mut roots = changed.artwork.stacks.get(root).unwrap().clone();
            roots.entries.insert(0, child);
            let mut children = changed.artwork.stacks.get(child_stack).unwrap().clone();
            children.entries.retain(|h| *h != child);
            changed.apply(Edit::Batch(vec![
                Edit::Stack(RecordChange::replace(&changed.artwork.stacks, root, Some(roots)).unwrap()),
                Edit::Stack(RecordChange::replace(&changed.artwork.stacks, child_stack, Some(children)).unwrap()),
            ])).unwrap();
            assert!(!query.matches_source(&changed));
        }
    }
}
#[test]
fn artwork_query_public_source_and_snapshot_mutations_cannot_reuse_an_obsolete_input_key() {
    let doc = input_key_document();
    let target = fixture::id(&doc, "Target");
    let upper = fixture::id(&doc, "Upper");
    let ink = fixture::id(&doc, "Ink");
    let mut query = crate::ArtworkQuery::new(&doc, ArtworkSource::EffectInput(target));
    query.source = ArtworkSource::EffectInput(upper);
    let mut changed = doc.clone();
    change_effect(&mut changed, target);
    assert!(!query.matches_source(&changed), "old target is now a contributing lower adjustment");
    query.source = ArtworkSource::EffectInput(target);
    let mut snapshot = doc.clone();
    change_occurrence(&mut snapshot, ink, |o| o.opacity = 0.25);
    query.snapshot = snapshot.snapshot();
    assert!(query.matches_source(&snapshot));
    assert!(!query.matches_source(&doc));
    let captured = Arc::make_mut(&mut query.snapshot);
    captured.artwork.occurrences.get_mut(ink).unwrap().opacity = 0.75;
    let mut changed = doc.clone();
    changed.artwork = query.snapshot.artwork.clone();
    fixture::refresh(&mut changed);
    assert!(query.matches_source(&changed));
    assert!(!query.matches_source(&doc));
}
#[test]
fn effect_input_key_distinguishes_isolated_and_pass_through_ancestor_backdrops() {
    for pass_through in [false, true] {
        for attached in [false, true] {
            let mut doc = fixture::document([128, 96], &["Target", "Sibling", "Group", "Backdrop"]);
            fixture::effect(&mut doc, "Target", "exposure");
            fixture::nest(&mut doc, "Group", &["Target", "Sibling"]);
            let target = fixture::id(&doc, "Target");
            let sibling = fixture::id(&doc, "Sibling");
            let group = fixture::id(&doc, "Group");
            let backdrop = fixture::id(&doc, "Backdrop");
            change_occurrence(&mut doc, group, |o| {
                o.blend = if pass_through { crate::LayerBlend::PassThrough } else { crate::LayerBlend::Normal }
            });
            if attached && pass_through && doc.scene().parent(target)!=Some(group) {assert!(doc.attachment_edit(target,true,false).is_err());continue;}
            change_occurrence(&mut doc, target, |o| o.attachment = if attached { crate::Attachment::Effect } else { crate::Attachment::None });
            let query = crate::ArtworkQuery::new(&doc, ArtworkSource::EffectInput(target));
            let mut changed = doc.clone();
            change_occurrence(&mut changed, sibling, |o| o.opacity = 0.25);
            assert!(!query.matches_source(&changed));
            let mut changed = doc.clone();
            change_occurrence(&mut changed, backdrop, |o| o.opacity = 0.25);
            assert_eq!(query.matches_source(&changed), !pass_through || attached, "pass-through={pass_through} attached={attached}");
            let mut changed = doc.clone();
            change_occurrence(&mut changed, group, |o| o.offset = [2, 3]);
            assert!(!query.matches_source(&changed));
            let mut changed = doc.clone();
            change_occurrence(&mut changed, group, |o| {
                o.blend = if pass_through { crate::LayerBlend::Normal } else { crate::LayerBlend::PassThrough }
            });
            assert!(!query.matches_source(&changed));
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
fn histogram_typed_curve_domains_use_coordinate_bins_for_rgb_and_luminance() {
    use crate::color::{
        DocumentColor, RgbSpace, SampleDepth,
        histogram::{Histogram, HistogramDomain},
    };
    let pixels = [[0.0625, 0.25, 0.375, 0.5], [-0.5, 0.5, 4., 1.], [0.; 4]];
    for space in RgbSpace::ALL {
        for domain in [HistogramDomain::Encoded, HistogramDomain::CurveLog { stops: 4. }] {
            let mut actual = Histogram::new(DocumentColor { space, depth: SampleDepth::F32 });
            actual.domain = domain;
            actual.add(&pixels).unwrap();
            let mut bins: [Vec<u64>; 4] = std::array::from_fn(|_| vec![0; 256]);
            for pixel in &pixels[..2] {
                let rgb = [0, 1, 2].map(|c| f64::from(pixel[c]) / f64::from(pixel[3]));
                let weights = space.to_xyz()[1];
                let y = rgb[1] + weights[0] * (rgb[0] - rgb[1]) + weights[2] * (rgb[2] - rgb[1]);
                for (channel, value) in [rgb[0], rgb[1], rgb[2], y].into_iter().enumerate() {
                    let coordinate = match domain {
                        HistogramDomain::Encoded => space.encode(value),
                        _ => {
                            let toe = std::f64::consts::E / 256.;
                            if value <= toe { value / (toe * 12. * std::f64::consts::LN_2) } else { (value.log2() + 8.) / 12. }
                        }
                    };
                    bins[channel][(coordinate.clamp(0., 1.) * 256.).floor().min(255.) as usize] += 1;
                }
            }
            for (channel, expected) in actual.channels.iter().zip(bins) {
                assert_eq!(channel.bins, expected, "{space:?} {domain:?}");
            }
            assert_eq!((actual.pixels, actual.transparent), (2, 1));
            assert_eq!(actual.plot_bins(), 0..256);
            assert_eq!(actual.axis().bins, [0, 256]);
            assert!(actual.axis().stops.is_none());
        }
    }
}
#[test]
fn scoped_effect_dependencies_ignore_excluded_ancestors_and_release_missing_clipping_bases() {
    let mut doc=fixture::document([128,96],&["Target","Child","Group","Backdrop"]);
    fixture::effect(&mut doc,"Target","exposure");fixture::nest(&mut doc,"Group",&["Target","Child"]);
    let target=fixture::id(&doc,"Target");let child=fixture::id(&doc,"Child");let group=fixture::id(&doc,"Group");let backdrop=fixture::id(&doc,"Backdrop");
    change_occurrence(&mut doc,group,|o|{o.visible=false;o.blend=crate::LayerBlend::PassThrough;});
    let scope=SceneScope::Members(vec![target,child,backdrop].into());let scene=doc.scene().with_scope(&scope);
    assert!(scene.visible(target));assert_eq!(scene.evaluation_parent(target),None);
    assert_eq!(crate::isolated_scope(scene,scene.parent(target)),None);
    assert_eq!(crate::composite_input_layers(scene,target),[child,backdrop]);
    let included=SceneScope::Members(vec![target,child,group,backdrop].into());assert!(!doc.scene().with_scope(&included).visible(target));
    change_occurrence(&mut doc,target,|o|o.attachment = crate::Attachment::Effect);
    assert_eq!(doc.scene().with_scope(&scope).effect_owner(target),Some(child));
    assert_eq!(crate::composite_input_layers(doc.scene().with_scope(&scope),target),[child]);
    let missing=SceneScope::Members(vec![target,backdrop].into());let scene=doc.scene().with_scope(&missing);
    assert!(!scene.effective_clipped(target));assert_eq!(crate::composite_input_scope(scene,target),Some(child));
    assert!(crate::composite_input_layers(scene,target).is_empty());
}

#[test]
fn spatial_reference_edits_invalidate_effect_outputs_and_downstream_inputs() {
    let mut doc=fixture::document([128,96],&["Target","Spatial","Ink"]);
    fixture::effect(&mut doc,"Target","exposure");
    fixture::effect(&mut doc,"Spatial","motion_blur");
    let target=fixture::id(&doc,"Target");
    let spatial=fixture::id(&doc,"Spatial");
    let output=crate::ArtworkQuery::new(&doc,ArtworkSource::Visible);
    let downstream=crate::ArtworkQuery::new(&doc,ArtworkSource::EffectInput(target));
    let handle=doc.scene().effect_handle(spatial).unwrap();
    let mut application=doc.artwork.effects.get(handle).unwrap().clone();
    application.spatial.as_mut().unwrap().mapping=Affine64([0.,1.,-1.,0.,96.,0.]);
    doc.apply(Edit::Effect(RecordChange::replace(&doc.artwork.effects,handle,Some(application)).unwrap())).unwrap();
    assert!(!output.matches_artwork(&doc));
    assert!(!downstream.matches_artwork(&doc));
}

#[test]
fn raw_object_queries_track_owner_and_ancestor_placement_without_layer_appearance() {
    let mut doc=fixture::document([128,96],&["Group"]);
    let image=Image::new(crate::color::source::rgba8_source([4;2],|_,_|[255,0,0,255]));
    let (layer,edit)=doc.create_object_layer_edit("Objects",ImageObject::new(image),None,0).unwrap();doc.apply(edit).unwrap();
    let group=fixture::nest(&mut doc,"Group",&["Objects"]);
    let assert_current=|query:&crate::ArtworkQuery,doc:&Document,current:bool| {
        assert_eq!(query.matches_artwork(doc),current);
        assert_eq!(query.matches_source(doc),current);
        assert_eq!(query.matches_source_identity(doc),current);
        assert_eq!(query.matches_snapshot(&doc.snapshot()),current);
    };
    let query=crate::ArtworkQuery::new(&doc,ArtworkSource::Objects(layer));
    change_occurrence(&mut doc,layer,|o|{o.opacity=0.2;o.visible=false;});
    change_occurrence(&mut doc,group,|o|{o.opacity=0.3;o.visible=false;});
    assert_current(&query,&doc,true);
    change_occurrence(&mut doc,layer,|o|o.offset=[3, -7]);
    assert_current(&query,&doc,false);
    let query=crate::ArtworkQuery::new(&doc,ArtworkSource::Objects(layer));
    change_occurrence(&mut doc,group,|o|o.offset=[-11, 5]);
    assert_current(&query,&doc,false);
}

#[test]
fn object_query_identity_reuses_shared_roots_but_tracks_rebound_owners_and_changed_content() {
    let mut doc=document();
    let image=Image::new(crate::color::source::rgba8_source([4;2],|_,_|[255,0,0,255]));
    let (first,edit)=doc.create_object_layer_edit("First",ImageObject::new(image.clone()),None,0).unwrap();doc.apply(edit).unwrap();let child=doc.scene().object_handle(first).unwrap();
    let mut moved=ImageObject::new(image);moved.affine.0[4]=8.;
    let (second,edit)=doc.create_object_layer_edit("Second",moved,None,0).unwrap();doc.apply(edit).unwrap();
    let query=crate::ArtworkQuery::new(&doc,ArtworkSource::Objects(first));
    let paint=doc.artwork.paint.iter().next().unwrap().0;
    change_paint(&mut doc,paint,|source|source.domain=[129,96]);
    assert!(query.snapshot.view().artwork().objects.same_root(&doc.artwork.objects));
    assert!(query.matches_source(&doc));
    let mut a=doc.scene().occurrence(first).unwrap().clone();
    let mut b=doc.scene().occurrence(second).unwrap().clone();
    std::mem::swap(&mut a.content,&mut b.content);
    doc.apply(Edit::Batch(vec![
        Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,first,Some(a)).unwrap()),
        Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,second,Some(b)).unwrap()),
    ])).unwrap();
    assert!(query.snapshot.view().artwork().objects.same_root(&doc.artwork.objects));
    assert!(!query.matches_source(&doc),"Shared object stores cannot hide an owner rebound to different content");
    let query=crate::ArtworkQuery::new(&doc,ArtworkSource::Objects(second));
    let mut renamed=doc.scene().occurrence(second).unwrap().clone();renamed.name="Renamed".into();
    doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,second,Some(renamed)).unwrap())).unwrap();
    assert!(query.snapshot.view().artwork().objects.same_root(&doc.artwork.objects));
    assert!(query.matches_source(&doc),"Image names do not change sampled content");
    doc.apply(doc.set_image_object_affine_edit(child,Affine64([1.,0.,0.,1.,16.,0.])).unwrap()).unwrap();
    assert!(!query.matches_source(&doc));
}

#[test]
fn frozen_queries_detect_paint_color_mode_changes_without_raster_changes() {
    let doc=document();
    let target=fixture::target(&doc,"Ink");
    let SourceTarget::Paint(handle)=target else {panic!("paint")};
    let requests=[ArtworkSource::Visible,ArtworkSource::Source(target)].map(|source|ArtworkSampleRequest::new(&doc,source,[7.5,9.5],5));
    for mode in [crate::color::LayerColorMode::Grayscale,crate::color::LayerColorMode::TwoTone] {
        let mut changed=doc.clone();change_paint(&mut changed,handle,|paint|paint.color_mode=mode);
        for request in &requests {assert!(!request.matches_artwork(&changed));}
    }
}
