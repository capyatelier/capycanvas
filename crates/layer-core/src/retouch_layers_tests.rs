use super::*;
use color::{DocumentColor, RgbSpace, SampleDepth};
use crate::operation_test_support as fixture;
use fixture::*;
fn document(names: &[&str]) -> Document {
    let mut doc = fixture::document([600, 400], names);
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().blend = BlendSpace::Perceptual;
    doc
}
fn filters() -> SeparationFilters {
    SeparationFilters::new(bundled_effect_catalog(), 6.5).unwrap()
}
#[test]
fn the_neutral_gray_encodes_to_the_middle_code_when_blending_perceptually() {
    for space in RgbSpace::ALL {
        for (depth, maximum, middle) in [(SampleDepth::U8, 255., 128.), (SampleDepth::U16, 65535., 32768.)] {
            let mut doc = document(&["Photo"]);
            doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().color = DocumentColor { space, depth };
            let gray = f64::from(doc.soft_light_neutral());
            assert_eq!((space.encode(gray) * maximum).round(), middle, "{space:?} {depth:?}");
            assert!((space.encode(gray) * maximum - middle).abs() < 1e-3, "{space:?} {depth:?}");
            doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().blend = BlendSpace::Linear;
            assert_eq!(doc.soft_light_neutral(), 0.5);
        }
        let mut doc = document(&["Photo"]);
        let composition = doc.artwork.compositions.get_mut(doc.artwork.root).unwrap();
        composition.color = DocumentColor { space, depth: SampleDepth::F32 };
        composition.blend = BlendSpace::Linear;
        assert_eq!(doc.soft_light_neutral(), 0.5);
    }
}
#[test]
fn a_dodge_and_burn_layer_goes_above_the_active_clipping_stack_filled_with_the_neutral_gray() {
    let mut doc = document(&["Clipped", "Base", "Under"]);
    occurrence_mut(&mut doc, "Clipped").clipped = true;
    activate(&mut doc, "Base");
    let plan = doc.dodge_burn_plan("Dodge & Burn").unwrap();
    let active = plan.active;
    let [(target, fill)] = &plan.operations[..] else { panic!("one fill") };
    let target = *target;
    let gray = doc.soft_light_neutral();
    assert_eq!(fill.kind, RasterOperationKind::Fill { color: [gray, gray, gray, 1.], alpha_locked: false });
    assert_eq!(fill.coverage.target, doc.artwork.coverage.next_handle());
    doc.apply(Edit::Batch(plan.edits)).unwrap();
    assert_eq!(names(&doc), ["Dodge & Burn", "Clipped", "Base", "Under", "Paper"]);
    let o = doc.scene().occurrence(active).unwrap();
    assert_eq!(o.blend, LayerBlend::SoftLight);
    assert!(!o.clipped);
    assert_eq!(doc.working.occurrence, Some(active));
    assert_eq!(doc.working.target, Some(target));
}
#[test]
fn a_dodge_and_burn_layer_stays_in_the_active_layer_group_unless_it_is_locked() {
    let mut doc = document(&["Group", "Inside", "Under"]);
    let group = nest(&mut doc, "Group", &["Inside"]);
    activate(&mut doc, "Inside");
    let plan = doc.dodge_burn_plan("Dodge & Burn").unwrap();
    let active = plan.active;
    let mut applied = doc.clone();
    applied.apply(Edit::Batch(plan.edits)).unwrap();
    assert_eq!(names(&applied), ["Group", "Dodge & Burn", "Inside", "Under", "Paper"]);
    assert_eq!(applied.scene().parent(active), Some(group));
    occurrence_mut(&mut doc, "Group").locked = true;
    assert_eq!(doc.dodge_burn_plan("Dodge & Burn").unwrap_err(), RetouchLayerRefusal::GroupLocked);
}
#[test]
fn frequency_separation_needs_a_visible_normal_paint_layer_in_a_perceptual_document() {
    use RetouchLayerRefusal as R;
    let mut doc = document(&["Photo", "Tint"]);
    let photo = id(&doc, "Photo");
    assert_eq!(doc.separation_refusal(photo), None);
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().blend = BlendSpace::Linear;
    assert_eq!(doc.separation_refusal(photo), Some(R::Linear));
    doc.artwork.compositions.get_mut(doc.artwork.root).unwrap().blend = BlendSpace::Perceptual;
    assert_eq!(doc.separation_refusal(id(&doc, "Paper")), Some(R::NotPaint));
    assert_eq!(doc.separation_refusal(OccurrenceHandle::INVALID), Some(R::NoLayer));
    occurrence_mut(&mut doc, "Tint").blend = LayerBlend::Multiply;
    assert_eq!(doc.separation_refusal(id(&doc, "Tint")), Some(R::NotNormal));
    occurrence_mut(&mut doc, "Photo").visible = false;
    assert_eq!(doc.separation_refusal(photo), Some(R::Hidden));
    assert!(doc.separation_plan(photo, &filters(), ["Frequency Separation", "Low", "High"].map(Arc::from)).is_err());
}
#[test]
fn frequency_separation_bakes_low_and_high_in_an_isolated_group_above_the_hidden_layer() {
    let mut doc = document(&["Above", "Photo", "Under"]);
    let group = nest(&mut doc, "Above", &["Photo", "Under"]);
    occurrence_mut(&mut doc, "Above").translation = Point { x: 30., y: -10. };
    let photo = id(&doc, "Photo");
    occurrence_mut(&mut doc, "Photo").opacity = 0.8;
    occurrence_mut(&mut doc, "Photo").translation = Point { x: 4., y: 5. };
    activate(&mut doc, "Photo");
    let filters = filters();
    let plan = doc.separation_plan(photo, &filters, ["Frequency Separation", "Low", "High"].map(Arc::from)).unwrap();
    let low = plan.operations[0].0;
    let high = plan.operations[1].0;
    let active = plan.active;
    let RasterOperationKind::Bake { scene: low_scene, scope: low_scope, offset } = &plan.operations[0].1.kind else {
        panic!("Low is baked")
    };
    assert_eq!(*offset, Point::default());
    let SceneScope::Members(members) = low_scope else { panic!("members") };
    assert_eq!(members.len(), 2);
    let blur = members[0];
    let source = low_scene.view().occurrence(members[1]).unwrap();
    assert_eq!(members[1], photo);
    assert_eq!(low_scene.view().effect(blur).unwrap().values, filters.blur.values);
    assert!(low_scene.view().occurrence(blur).unwrap().clipped);
    assert!(source.visible && source.opacity == 1.);
    assert_eq!(source.translation, Point { x: 4., y: 5. });
    assert_eq!(low_scene.view().target_offset(low_scene.view().source_target(photo).unwrap()), Point { x: 34., y: -5. });
    let RasterOperationKind::FrequencyDetail { scene: original, scope: original_scope, offset: detail_offset, low: reference } =
        &plan.operations[1].1.kind
    else {
        panic!("High reuses Low")
    };
    let SourceTarget::Paint(low_handle) = low else { panic!("paint") };
    assert_eq!((*detail_offset, *reference), (*offset, low_handle));
    assert_eq!(original_scope, &SceneScope::Members(vec![photo].into()));
    assert_eq!(original.view().occurrence(photo).unwrap(), source);
    assert_eq!(plan.operations[0].1.coverage.target, doc.artwork.coverage.next_handle());
    assert_eq!(plan.operations[1].1.coverage.target, doc.artwork.coverage.next_handle());
    assert_eq!(filters.blur.value("sigma"), Some(&EffectValue::Number(6.5)));
    let before = doc.clone();
    let inverse = doc.apply(Edit::Batch(plan.edits)).unwrap();
    assert_eq!(names(&doc), ["Above", "Frequency Separation", "High", "Low", "Photo", "Under", "Paper"]);
    let separation = id(&doc, "Frequency Separation");
    let g = occurrence(&doc, "Frequency Separation");
    assert_eq!(g.kind(), LayerKind::Group);
    assert_eq!(g.blend, LayerBlend::Normal);
    assert_eq!(doc.scene().parent(separation), Some(group));
    assert_eq!(g.opacity, 0.8);
    for (name, target, blend) in [("High", high, LayerBlend::LinearLight), ("Low", low, LayerBlend::Normal)] {
        let h = id(&doc, name);
        assert_eq!((doc.scene().parent(h), occurrence(&doc, name).blend), (Some(separation), blend));
        assert_eq!(doc.target_offset(target), Point::default());
    }
    assert!(!occurrence(&doc, "Photo").visible);
    assert_eq!(doc.working.occurrence, Some(active));
    doc.apply(inverse).unwrap();
    restored(&before, &doc);
}
#[test]
fn separation_filters_come_from_the_catalog_and_the_radius_follows_the_blur() {
    let catalog = bundled_effect_catalog();
    let radius = SeparationFilters::radius_parameter(catalog).unwrap();
    assert!(matches!(radius.kind,EffectParameterKind::Number {max,..} if max>1.));
    assert!(SeparationFilters::new(catalog, 1e6).is_err());
    assert!(SeparationFilters::new(&EffectCatalog::default(), 4.).is_err());
}
#[test]
fn retouch_creation_names_round_trip_literally_and_undo_preserves_the_source_name() {
    let source_name = "私の写真 { $name } \u{2068}صورة\u{2069} 🎨";
    let mut doc = document(&[source_name]);
    let before = doc.clone();
    let dodge_name = "보정 { $name } \u{2068}لون\u{2069}";
    let plan = doc.dodge_burn_plan(dodge_name).unwrap();
    let active = plan.active;
    let inverse = doc.apply(Edit::Batch(plan.edits)).unwrap();
    assert_eq!(doc.scene().occurrence(active).unwrap().name.as_ref(), dodge_name);
    assert_eq!(names(&roundtrip(&doc)), names(&doc));
    doc.apply(inverse).unwrap();
    restored(&before, &doc);
    let labels = ["分離 { $name }", "低频 \u{2068}منخفض\u{2069}", "높음 🎨"];
    let plan = doc.separation_plan(doc.working.occurrence.unwrap(), &filters(), labels.map(Arc::from)).unwrap();
    let inverse = doc.apply(Edit::Batch(plan.edits)).unwrap();
    for name in labels {
        assert_eq!(occurrence(&doc, name).name.as_ref(), name);
    }
    assert!(names(&doc).iter().any(|name| name == source_name));
    assert_eq!(names(&roundtrip(&doc)), names(&doc));
    doc.apply(inverse).unwrap();
    restored(&before, &doc);
}
