use super::*;
use std::collections::BTreeSet;

fn id(value: u128) -> PortableId { PortableId::from_bytes(value.to_be_bytes()) }
fn graph() -> GraphShape {
    GraphShape {
        objects: [
            (id(1), Shape::Composition { result: id(2) }),
            (id(2), Shape::Stack { entries: vec![id(3)] }),
            (id(3), Shape::Occurrence { content: Content::Paint(id(4)), mask: None }),
            (id(4), Shape::Paint),
            (id(5), Shape::Output { composition: id(1) }),
        ].into(),
        outputs: vec![id(5)], default_output: Some(id(5)), ..Default::default()
    }
}
fn validate(graph: &GraphShape) -> Result<Support, &'static str> { graph.validate(id(1), GraphLimits::default()) }

#[test]
fn portable_identity_is_exact_and_has_one_spelling() {
    let value: PortableId = "fedcba98765432100123456789abcdef".parse().unwrap();
    assert_eq!(value.bytes(), [254, 220, 186, 152, 118, 84, 50, 16, 1, 35, 69, 103, 137, 171, 205, 239]);
    assert_eq!(serde_json::to_string(&value).unwrap(), "\"fedcba98765432100123456789abcdef\"");
    for invalid in ["1", "fedcba98765432100123456789abcdeF", "fedcba98765432100123456789abcdeg", "00000000-0000-0000-0000-000000000000"] {
        assert!(invalid.parse::<PortableId>().is_err());
    }
    assert!(serde_json::from_str::<PortableId>("18446744073709551615").is_err());
}

#[test]
fn undo_retains_identity_and_deleted_handles_never_alias_new_content() {
    let mut store = Store::default();
    let old = store.insert(id(1), "original").unwrap();
    let saved = store.clone();
    let inverse = store.remove(old).unwrap();
    assert_eq!(store.get(old), None);
    assert_eq!(store.resolve(id(1)), None);
    assert!(store.insert(id(1), "impostor").is_err());
    let duplicate = store.insert(id(2), "duplicate").unwrap();
    assert_ne!(old, duplicate);
    assert!(store.restore(old, id(2), "wrong identity").is_err());
    store.restore(old, id(1), inverse).unwrap();
    assert_eq!(store.resolve(id(1)), Some(old));
    assert_eq!(saved.get(old), Some(&"original"));
    assert!(store.restore(old, id(1), "already live").is_err());
    assert_eq!(std::mem::size_of_val(&old), 4);
}

#[test]
fn retained_unplaced_content_is_validated_without_being_collected() {
    let mut g = graph();
    g.objects.insert(id(6), Shape::Paint);
    assert_eq!(validate(&g), Ok(Support::Editable));
    g.objects.insert(id(7), Shape::Unknown { ancillary: false, references: vec![id(6)] });
    assert!(matches!(validate(&g), Ok(Support::Preserved(_))));
    g.objects.insert(id(7), Shape::Unknown { ancillary: false, references: vec![id(99)] });
    assert_eq!(validate(&g), Err("Dangling authored reference"));
}

#[test]
fn sharing_through_reused_groups_is_preserved_even_with_one_direct_source_use() {
    let mut g = graph();
    g.objects.insert(id(6), Shape::Stack { entries: vec![id(3)] });
    g.objects.insert(id(7), Shape::Occurrence { content: Content::Group(id(6)), mask: None });
    g.objects.insert(id(8), Shape::Occurrence { content: Content::Group(id(6)), mask: None });
    g.objects.insert(id(2), Shape::Stack { entries: vec![id(7), id(8)] });
    assert_eq!(validate(&g), Ok(Support::Preserved(BTreeSet::from(["Shared editable content"]))));
    g.objects.insert(id(2), Shape::Stack { entries: vec![id(7)] });
    assert!(matches!(validate(&g), Ok(Support::Preserved(_))), "unplaced second group is retained");
    g.objects.remove(&id(8));
    assert_eq!(validate(&g), Ok(Support::Editable));
}

#[test]
fn independent_duplicate_can_share_binary_resources_but_linked_sources_are_preserved() {
    let mut g = graph();
    g.resources.insert(id(100));
    g.objects.insert(id(6), Shape::Paint);
    g.objects.insert(id(7), Shape::Occurrence { content: Content::Paint(id(6)), mask: None });
    assert_eq!(validate(&g), Ok(Support::Editable));
    g.objects.insert(id(7), Shape::Occurrence { content: Content::Paint(id(4)), mask: None });
    assert!(matches!(validate(&g), Ok(Support::Preserved(_))));
}

#[test]
fn shared_matte_is_not_an_image_alpha_or_a_visibility_dependency() {
    let mut g = graph();
    g.objects.insert(id(6), Shape::Coverage);
    g.objects.insert(id(7), Shape::Definition { dependencies: vec![] });
    g.objects.insert(id(8), Shape::Effect { definition: id(7), inputs: vec![id(6)] });
    g.objects.insert(id(9), Shape::Effect { definition: id(7), inputs: vec![id(6)] });
    assert!(matches!(validate(&g), Ok(Support::Preserved(_))));
    g.objects.insert(id(3), Shape::Occurrence { content: Content::Paint(id(6)), mask: None });
    assert_eq!(validate(&g), Err("Invalid authored relationship"));
}

#[test]
fn output_contexts_do_not_change_source_ownership() {
    let mut g = graph();
    g.objects.insert(id(6), Shape::Output { composition: id(1) });
    g.outputs.push(id(6));
    assert!(matches!(validate(&g), Ok(Support::Preserved(_))));
    g.default_output = Some(id(99));
    assert_eq!(validate(&g), Err("Invalid default output"));
    g.outputs.clear(); g.default_output = None;
    assert!(matches!(validate(&g), Ok(Support::Preserved(_))));
}

#[test]
fn membership_and_evaluation_cycles_are_not_resource_references() {
    let mut g = graph();
    g.objects.insert(id(3), Shape::Occurrence { content: Content::Group(id(2)), mask: None });
    assert_eq!(validate(&g), Err("Cyclic authored dependency"));
    let mut g = graph();
    g.objects.insert(id(6), Shape::Definition { dependencies: vec![id(7)] });
    g.objects.insert(id(7), Shape::Definition { dependencies: vec![id(6)] });
    assert_eq!(validate(&g), Err("Cyclic authored dependency"));
    g.objects.insert(id(6), Shape::Unknown { ancillary: false, references: vec![id(7)] });
    g.objects.insert(id(7), Shape::Unknown { ancillary: false, references: vec![id(6)] });
    assert!(matches!(validate(&g), Ok(Support::Preserved(_))), "unknown reference semantics do not imply execution");
}

#[test]
fn ancillary_dependencies_cannot_change_required_artwork() {
    let mut g = graph();
    g.objects.insert(id(6), Shape::Unknown { ancillary: true, references: vec![id(4)] });
    assert_eq!(validate(&g), Ok(Support::Editable));
    g.objects.insert(id(7), Shape::Unknown { ancillary: true, references: vec![id(6)] });
    assert!(validate(&g).is_err());
    g.objects.insert(id(7), Shape::Unknown { ancillary: false, references: vec![id(6)] });
    assert!(validate(&g).is_err());
}

#[test]
fn duplicate_membership_is_malformed_and_expansion_is_bounded() {
    let mut g = graph();
    g.objects.insert(id(2), Shape::Stack { entries: vec![id(3), id(3)] });
    assert_eq!(validate(&g), Err("Occurrence belongs to multiple stack slots"));
    let g = graph();
    for limits in [
        GraphLimits { objects: 4, ..Default::default() },
        GraphLimits { edges: 3, ..Default::default() },
        GraphLimits { depth: 3, ..Default::default() },
    ] { assert!(g.validate(id(1), limits).is_err()); }
    assert_eq!(validate(&g), Ok(Support::Editable));
}

#[test]
fn unplaced_group_cannot_instance_the_composition_root_in_editable_mode() {
    let mut g = graph();
    g.objects.insert(id(6), Shape::Occurrence { content: Content::Group(id(2)), mask: None });
    assert_eq!(validate(&g), Ok(Support::Preserved(BTreeSet::from(["Shared editable content"]))));
}

#[test]
fn permanent_schema_fixtures_keep_structural_classification_and_visible_references() {
    use serde_json::Value;
    fn reference(value: &Value) -> PortableId { value["ref"].as_str().unwrap().parse().unwrap() }
    for (bytes, editable) in [
        (include_bytes!("../../tests/fixtures/capy/empty.json").as_slice(), true),
        (include_bytes!("../../tests/fixtures/capy/paint-and-mask.json").as_slice(), true),
        (include_bytes!("../../tests/fixtures/capy/ancillary.json").as_slice(), true),
        (include_bytes!("../../tests/fixtures/capy/reused-group.json").as_slice(), false),
        (include_bytes!("../../tests/fixtures/capy/retained-future.json").as_slice(), false),
    ] {
        let value = crate::package::parse_json(bytes, 65536).unwrap();
        let mut graph = GraphShape::default();
        graph.outputs = value["outputs"].as_array().unwrap().iter().map(reference).collect();
        graph.default_output = value.get("default_output").map(reference);
        for object in value["objects"].as_array().unwrap() {
            let id: PortableId = object["id"].as_str().unwrap().parse().unwrap();
            let data = &object["data"];
            let shape = match object["type"].as_str().unwrap() {
                "capy.composition/1" => Shape::Composition { result: reference(&data["result"]["object"]) },
                "capy.stack/1" => Shape::Stack { entries: data.get("entries").map_or(Vec::new(), |v| v.as_array().unwrap().iter().map(reference).collect()) },
                "capy.occurrence/2" => {
                    let content = &data["content"];
                    let content = if let Some(paint) = content.get("paint") { Content::Paint(reference(paint)) }
                        else { Content::Group(reference(&content["stack"])) };
                    Shape::Occurrence { content, mask: data.get("mask").map(|m| reference(&m["source"])) }
                }
                "capy.paint-source/1" => Shape::Paint,
                "capy.coverage-source/1" => Shape::Coverage,
                "capy.output/1" => Shape::Output { composition: reference(&data["source"]["object"]) },
                _ => Shape::Unknown { ancillary: object["ancillary"].as_bool().unwrap_or(false), references: crate::package::references(data, 1000).unwrap() },
            };
            assert!(graph.objects.insert(id, shape).is_none());
        }
        assert_eq!(graph.validate(reference(&value["root"]), GraphLimits::default()).unwrap() == Support::Editable, editable);
        let serialized = serde_json::to_vec(&value).unwrap();
        assert_eq!(crate::package::parse_json(&serialized, 65536).unwrap(), value);
    }
}

#[test]
fn captured_integrated_phase_survives_rate_edits_and_a_new_host_time_origin() {
    use crate::{EffectClock, EffectInstance, EffectValue, bundled_effect_catalog};
    let mut effect = EffectInstance::new(bundled_effect_catalog().get("domain_warp").unwrap().program());
    effect.set("animate", EffectValue::Toggle(true)).unwrap();
    effect.set("speed", EffectValue::Number(1.)).unwrap();
    let mut clock = EffectClock::default();
    for (elapsed, speed, phase) in [(2.,1.,2.), (2.,2.,2.), (3.,2.,4.), (3.,0.,4.), (8.,0.,4.), (8.,2.,4.), (9.,2.,6.)] {
        effect.set("speed", EffectValue::Number(speed)).unwrap();
        assert_eq!(clock.advance(effect.view(), elapsed), phase);
        let mut reopened = EffectClock::at(effect.view(), 0., phase);
        assert_eq!(reopened.advance(effect.view(), 0.), phase);
        assert_eq!(reopened.advance(effect.view(), 0.5), phase + speed * 0.5);
    }
    assert_ne!(clock.advance(effect.view(), 9.), effect.time_seconds(9.), "elapsed alone does not identify saved output");
}
