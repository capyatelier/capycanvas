use super::*;
use color::{DocumentColor, RgbSpace, SampleDepth};

/// Top to bottom: the named paint layers, then the paper, blending
/// perceptually.
fn document(names: &[&str]) -> Document {
    let mut doc = Document::new("retouch", 600, 400);
    doc.blend_space = BlendSpace::Perceptual;
    doc.layers.remove(0);
    for (i, name) in names.iter().enumerate() {
        let id = doc.allocate_layer_id();
        doc.layers.insert(i, Layer::paint(id, *name));
    }
    doc.active_layer = doc.layers[0].id;
    doc
}

fn id(doc: &Document, name: &str) -> LayerId {
    doc.layers.iter().find(|l| &*l.name == name).unwrap().id
}

fn layer_mut<'a>(doc: &'a mut Document, name: &str) -> &'a mut Layer {
    doc.layers.iter_mut().find(|l| &*l.name == name).unwrap()
}

fn names(doc: &Document) -> Vec<String> {
    doc.layers.iter().map(|l| l.name.to_string()).collect()
}

fn ids() -> [LayerId; SEPARATION_IDS] {
    std::array::from_fn(|i| LayerId(100 + i as u64))
}

fn filters() -> SeparationFilters {
    SeparationFilters::new(bundled_effect_catalog(), 6.5).unwrap()
}

#[test]
fn the_neutral_gray_encodes_to_the_middle_code_when_blending_perceptually() {
    for space in RgbSpace::ALL {
        for (depth, maximum, middle) in [(SampleDepth::U8, 255., 128.), (SampleDepth::U16, 65535., 32768.)] {
            let mut doc = document(&["Photo"]);
            doc.color = DocumentColor { space, depth };
            let gray = f64::from(doc.soft_light_neutral());
            assert_eq!((space.encode(gray) * maximum).round(), middle, "{space:?} {depth:?}");
            assert!((space.encode(gray) * maximum - middle).abs() < 1e-3, "{space:?} {depth:?}: exact, not rounded");
            doc.blend_space = BlendSpace::Linear;
            assert_eq!(doc.soft_light_neutral(), 0.5, "{space:?} {depth:?} Linear light");
        }
        let mut doc = document(&["Photo"]);
        doc.color = DocumentColor { space, depth: SampleDepth::F32 };
        doc.blend_space = BlendSpace::Linear;
        assert_eq!(doc.soft_light_neutral(), 0.5);
    }
}

#[test]
fn a_dodge_and_burn_layer_goes_above_the_active_clipping_stack_filled_with_the_neutral_gray() {
    let mut doc = document(&["Clipped", "Base", "Under"]);
    layer_mut(&mut doc, "Clipped").properties.clipped = true;
    doc.active_layer = id(&doc, "Base");
    let plan = doc.dodge_burn_plan([LayerId(100), LayerId(101)]).unwrap();
    assert_eq!(plan.active, LayerId(100));
    let [(target, fill)] = &plan.operations[..] else { panic!("one fill") };
    assert_eq!(*target, LayerId(100));
    let gray = doc.soft_light_neutral();
    assert_eq!(fill.kind, LayerOperationKind::Fill { color: [gray, gray, gray, 1.], alpha_locked: false });
    assert_eq!(fill.coverage.id, LayerId(101));
    doc.apply(Edit::Batch(plan.edits)).unwrap();
    assert_eq!(names(&doc), ["Dodge & Burn", "Clipped", "Base", "Under", "Paper"]);
    let layer = doc.layer(LayerId(100)).unwrap();
    assert_eq!(layer.properties.blend, LayerBlend::SoftLight);
    assert!(!layer.properties.clipped);
    assert_eq!(doc.active_layer, LayerId(100));
}

#[test]
fn a_dodge_and_burn_layer_stays_in_the_active_layer_group_unless_it_is_locked() {
    let mut doc = document(&["Group", "Inside", "Under"]);
    let group = id(&doc, "Group");
    layer_mut(&mut doc, "Group").kind = LayerKind::Group;
    layer_mut(&mut doc, "Inside").properties.parent = Some(group);
    doc.active_layer = id(&doc, "Inside");
    let plan = doc.dodge_burn_plan([LayerId(100), LayerId(101)]).unwrap();
    let mut applied = doc.clone();
    applied.apply(Edit::Batch(plan.edits)).unwrap();
    assert_eq!(names(&applied), ["Group", "Dodge & Burn", "Inside", "Under", "Paper"]);
    assert_eq!(applied.layer(LayerId(100)).unwrap().properties.parent, Some(group));
    layer_mut(&mut doc, "Group").properties.locked = true;
    assert_eq!(doc.dodge_burn_plan([LayerId(100), LayerId(101)]).unwrap_err(), RetouchLayerRefusal::GroupLocked);
}

#[test]
fn frequency_separation_needs_a_visible_normal_paint_layer_in_a_perceptual_document() {
    use RetouchLayerRefusal as R;
    let mut doc = document(&["Photo", "Tint"]);
    let photo = id(&doc, "Photo");
    assert_eq!(doc.separation_refusal(photo), None);
    doc.blend_space = BlendSpace::Linear;
    assert_eq!(doc.separation_refusal(photo), Some(R::Linear));
    doc.blend_space = BlendSpace::Perceptual;
    assert_eq!(doc.separation_refusal(id(&doc, "Paper")), Some(R::NotPaint));
    assert_eq!(doc.separation_refusal(LayerId(999)), Some(R::NoLayer));
    layer_mut(&mut doc, "Tint").properties.blend = LayerBlend::Multiply;
    assert_eq!(doc.separation_refusal(id(&doc, "Tint")), Some(R::NotNormal));
    layer_mut(&mut doc, "Photo").visible = false;
    assert_eq!(doc.separation_refusal(photo), Some(R::Hidden));
    assert!(doc.separation_plan(photo, &filters(), ids()).is_err());
}

#[test]
fn frequency_separation_bakes_low_and_high_in_an_isolated_group_above_the_hidden_layer() {
    let mut doc = document(&["Above", "Photo", "Under"]);
    let group = id(&doc, "Above");
    layer_mut(&mut doc, "Above").kind = LayerKind::Group;
    for name in ["Photo", "Under"] {
        layer_mut(&mut doc, name).properties.parent = Some(group);
    }
    layer_mut(&mut doc, "Above").properties.offset = Point { x: 30., y: -10. };
    let photo = id(&doc, "Photo");
    layer_mut(&mut doc, "Photo").opacity = 0.8;
    layer_mut(&mut doc, "Photo").properties.offset = Point { x: 4., y: 5. };
    doc.active_layer = photo;
    let filters = filters();
    let plan = doc.separation_plan(photo, &filters, ids()).unwrap();
    let [separation, low, high, blur, low_coverage, high_coverage] = ids();
    assert_eq!(plan.active, high);
    assert_eq!(plan.operations.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [low, high]);
    let LayerOperationKind::Bake { members, offset } = &plan.operations[0].1.kind else { panic!("Low is baked") };
    assert_eq!(*offset, Point { x: 30., y: -10. });
    assert_eq!(plan.operations[0].1.coverage.id, low_coverage);
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].id, blur);
    assert_eq!(members[0].effect.as_ref(), Some(&filters.blur));
    assert!(members[0].properties.clipped);
    let source = &members[1];
    assert_eq!(source.id, photo);
    assert!(source.visible && source.opacity == 1. && source.properties.parent.is_none());
    assert_eq!(source.properties.offset, Point { x: 4., y: 5. });
    let LayerOperationKind::FrequencyDetail { members: original, offset: detail_offset, low: reference } = &plan.operations[1].1.kind else {
        panic!("High reuses Low")
    };
    assert_eq!((*detail_offset, *reference), (*offset, low));
    assert_eq!(original.as_ref(), std::slice::from_ref(source));
    assert_eq!(plan.operations[1].1.coverage.id, high_coverage);
    assert_eq!(filters.blur.value("sigma"), Some(&EffectValue::Number(6.5)));
    let before = doc.clone();
    let inverse = doc.apply(Edit::Batch(plan.edits)).unwrap();
    assert_eq!(names(&doc), ["Above", "Frequency Separation", "High", "Low", "Photo", "Under", "Paper"]);
    let group_layer = doc.layer(separation).unwrap();
    assert_eq!(group_layer.kind, LayerKind::Group);
    assert_eq!(group_layer.properties.blend, LayerBlend::Normal);
    assert_eq!(group_layer.properties.parent, Some(group));
    assert_eq!(group_layer.opacity, 0.8, "the group takes the layer's opacity");
    for (id, blend) in [(high, LayerBlend::LinearLight), (low, LayerBlend::Normal)] {
        let part = doc.layer(id).unwrap();
        assert_eq!((part.properties.parent, part.properties.blend), (Some(separation), blend));
        assert_eq!(doc.layer_offset(id), Point::default(), "{id:?} covers the canvas");
    }
    assert!(!doc.layer(photo).unwrap().visible);
    assert_eq!(doc.active_layer, high);
    doc.apply(inverse).unwrap();
    assert_eq!(doc.layers, before.layers);
}

#[test]
fn separation_filters_come_from_the_catalog_and_the_radius_follows_the_blur() {
    let catalog = bundled_effect_catalog();
    let radius = SeparationFilters::radius_parameter(catalog).unwrap();
    assert!(matches!(radius.kind, EffectParameterKind::Number { max, .. } if max > 1.));
    assert!(SeparationFilters::new(catalog, 1e6).is_err(), "radii past the blur's range are refused");
    assert!(SeparationFilters::new(&EffectCatalog::default(), 4.).is_err());
}

