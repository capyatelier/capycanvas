use super::*;
use crate::package::archive::{Member, MIMETYPE};
use serde_json::json;

fn id(n: u8) -> PortableId { let mut bytes = [0; 16]; bytes[15] = n; PortableId::from_bytes(bytes) }
fn reference_value(n: u8) -> Value { json!({"ref":id(n)}) }
fn fixture(name: &str) -> Value {
    serde_json::from_str(match name {
        "empty" => include_str!("../../../tests/fixtures/capy/empty.json"),
        "paint" => include_str!("../../../tests/fixtures/capy/paint-and-mask.json"),
        "ancillary" => include_str!("../../../tests/fixtures/capy/ancillary.json"),
        "future" => include_str!("../../../tests/fixtures/capy/retained-future.json"),
        "shared" => include_str!("../../../tests/fixtures/capy/reused-group.json"),
        _ => unreachable!(),
    }).unwrap()
}
fn directory(extra: &[(&str, &[u8])]) -> Directory {
    let mut offset = 1024;
    let members = [("mimetype", MIMETYPE), ("manifest.json", b"{}".as_slice())].into_iter().chain(extra.iter().copied()).map(|(name, bytes)| {
        let member = Member { name:name.into(), offset, length:bytes.len() as u64, compressed_length:None, crc32:crc32fast::hash(bytes) };
        offset += bytes.len() as u64 + 64; member
    }).collect();
    Directory { members, length:offset }
}
fn parse(value: &Value, directory: &Directory) -> DecodeResult<ManifestRead> {
    Manifest::parse(&serde_json::to_vec(value).unwrap(), directory, ManifestLimits::default())
}
fn known(value: &Value, directory: &Directory) -> Manifest {
    let ManifestRead::Known(manifest) = parse(value, directory).unwrap() else { panic!("unknown envelope") }; manifest
}
fn resource_value(n: u8, kind: &str, encoding: &str, data: Value, location: Value, bytes: &[u8]) -> Value {
    json!({"id":id(n),"type":kind,"data":data,"encoding":encoding,"location":location,
        "bytes":bytes.len().to_string(),"crc32":format!("{:08x}",crc32fast::hash(bytes))})
}
fn pack_resource(n: u8, offset: u64, bytes: &[u8]) -> Value {
    resource_value(n, "capy.wgsl/1", "utf8", json!({}), json!({"pack":"data/tiles-1.bin","offset":offset.to_string()}), bytes)
}
fn push_resource(value: &mut Value, resource: Value) { value["resources"].as_array_mut().unwrap().push(resource); }
fn push_object(value: &mut Value, record: Value) { value["objects"].as_array_mut().unwrap().push(record); }

#[test]
fn retained_fixtures_distinguish_editable_content_from_unsupported_sharing() {
    for (name, editable) in [("empty",true),("paint",true),("ancillary",true),("future",false),("shared",false)] {
        let value = fixture(name);
        let manifest = known(&value, &directory(&[]));
        assert_eq!(manifest.support == Support::Editable, editable, "{name}");
        for record in value["objects"].as_array().unwrap() {
            assert_eq!(&manifest.objects[&identity(&record["id"]).unwrap()], record);
        }
    }
    let mut unplaced = fixture("paint");
    push_object(&mut unplaced,json!({"id":id(9),"type":"capy.occurrence/2","data":{"content":{"paint":reference_value(4)}}}));
    assert!(matches!(known(&unplaced,&directory(&[])).support,Support::Preserved(_)));
    unplaced["objects"][1]["data"]["entries"] = json!([reference_value(3),reference_value(3)]);
    assert!(parse(&unplaced,&directory(&[])).is_err());
}

#[test]
fn unsupported_envelopes_are_opaque_and_duplicate_keys_are_always_invalid() {
    for edit in [0,1,2] {
        let mut value = fixture("empty");
        match edit { 0=>value["version"]=2.into(),1=>value["future"]=json!({"unknown":true}),_=>value["format"]="future.canvas".into() }
        let ManifestRead::UnsupportedEnvelope(retained) = parse(&value,&directory(&[])).unwrap() else { panic!() };
        assert_eq!(retained,value);
    }
    assert!(Manifest::parse(br#"{"format":"capy.canvas","version":2,"future":{"x":1,"x":2}}"#,&directory(&[]),ManifestLimits::default()).is_err());
    for bad in [json!("1"),json!(-1),json!(1.5)] {
        let mut value=fixture("empty");value["version"]=bad;assert!(parse(&value,&directory(&[])).is_err());
    }
    let mut value=fixture("empty");value["objects"][1]["future"]=json!({"keep":17});
    let manifest=known(&value,&directory(&[]));assert!(matches!(manifest.support,Support::Preserved(_)));
    assert_eq!(manifest.objects[&id(2)]["future"],json!({"keep":17}));
}

#[test]
fn identity_reference_and_ancillary_rules_cover_every_retained_record() {
    for edit in [0,1,2,3,4,5] {
        let mut value=fixture("ancillary");
        match edit {
            0=>value["objects"][1]["id"]=json!(id(100)),
            1=>value["objects"][3]["data"]["subject"]=reference_value(88),
            2=>value["objects"][3]["data"]["subject"]=reference_value(100),
            3=>value["objects"][3]["data"]["subject"]=json!({"ref":id(2),"other":true}),
            4=>value["objects"][1]["ancillary"]=true.into(),
            _=>value["objects"][3]["ancillary"]=false.into(),
        }
        assert!(parse(&value,&directory(&[])).is_err(),"edit {edit}");
    }
    let mut value=fixture("ancillary");
    push_object(&mut value,json!({"id":id(8),"type":"future.note/1","ancillary":true,"data":{"link":reference_value(7)}}));
    assert!(parse(&value,&directory(&[])).is_err());
    value["objects"].as_array_mut().unwrap().pop();
    value["objects"][1]["extra"]=reference_value(7);
    assert!(parse(&value,&directory(&[])).is_err());
    let mut value=fixture("empty");
    push_resource(&mut value,pack_resource(100,0,b"abc"));
    assert!(parse(&value,&directory(&[("data/tiles-1.bin",b"abc")])).is_err());
}

#[test]
fn root_outputs_and_topology_never_guess_relationships() {
    for edit in [0,1,2,3,4] {
        let mut value=fixture("empty");
        match edit {
            0=>value["root"]=reference_value(8),
            1=>value["outputs"]=json!([reference_value(2)]),
            2=>value["default_output"]=reference_value(2),
            3=>value["outputs"]=json!([reference_value(5),reference_value(5)]),
            _=>{value.as_object_mut().unwrap().remove("default_output");},
        }
        assert!(parse(&value,&directory(&[])).is_err(),"edit {edit}");
    }
    let mut value=fixture("empty");value["objects"][0]["type"]="future.collection/1".into();
    value["outputs"]=json!([]);value.as_object_mut().unwrap().remove("default_output");
    assert!(matches!(known(&value,&directory(&[])).support,Support::Preserved(_)));
    let mut value=fixture("empty");value["objects"][0]["data"]["result"]["port"]="future_color".into();
    assert!(matches!(known(&value,&directory(&[])).support,Support::Preserved(_)));
    value["objects"][0]["data"]["result"].as_object_mut().unwrap().remove("port");
    assert!(parse(&value,&directory(&[])).is_err());
}

#[test]
fn opaque_ancillary_resources_are_retained_without_requiring_their_decoder() {
    let mut value=fixture("ancillary");
    value["objects"][3]["data"]["payload"]=reference_value(10);
    let opaque=resource_value(10,"future.attachment/1","future.codec/1",json!({"nested":reference_value(11)}),
        json!({"pack":"data/tiles-1.bin","offset":"0"}),b"abc");
    push_resource(&mut value,opaque.clone());
    push_resource(&mut value,resource_value(11,"future.dictionary/1","raw",json!({}),json!({"pack":"data/tiles-1.bin","offset":"3"}),b"def"));
    let dir=directory(&[("data/tiles-1.bin",b"abcdef")]);
    let manifest=known(&value,&dir);
    assert_eq!(manifest.support,Support::Editable);assert_eq!(manifest.resources[&id(10)].value,opaque);
    value["metadata"]=json!({"xmp":reference_value(11)});
    assert!(matches!(known(&value,&dir).support,Support::Preserved(_)));
    value.as_object_mut().unwrap().remove("metadata");value["objects"][3]["data"].as_object_mut().unwrap().remove("payload");
    assert!(matches!(known(&value,&dir).support,Support::Preserved(_)));
}

#[test]
fn pack_aliases_require_the_complete_unchanged_descriptor_and_integrity_contract() {
    let mut value=fixture("empty");push_resource(&mut value,pack_resource(10,1,b"bcd"));push_resource(&mut value,pack_resource(11,1,b"bcd"));
    let dir=directory(&[("data/tiles-1.bin",b"abcdef")]);
    let manifest=known(&value,&dir);
    assert_eq!(manifest.support,Support::Editable);assert_eq!(manifest.resources[&id(10)].range,manifest.resources[&id(11)].range);
    for edit in [0,1,2,3,4] {
        let mut value=value.clone();
        match edit {
            0=>value["resources"][1]["data"]["future"]=true.into(),
            1=>value["resources"][1]["encoding"]="future".into(),
            2=>value["resources"][1]["crc32"]="00000000".into(),
            3=>value["resources"][1]["location"]["offset"]="2".into(),
            _=>value["resources"][1]["bytes"]="2".into(),
        }
        assert!(parse(&value,&dir).is_err(),"alias edit {edit}");
    }
    value["resources"].as_array_mut().unwrap().pop();
    value["resources"][0]["location"]=json!({"future_store":"key"});
    let manifest=known(&value,&dir);assert!(matches!(manifest.support,Support::Preserved(_)));assert!(manifest.resources[&id(10)].range.is_none());
}

#[test]
fn standalone_inventory_checks_lengths_checksums_and_namespace() {
    let name=format!("data/{}",id(10));
    let mut value=fixture("empty");
    push_resource(&mut value,resource_value(10,"capy.wgsl/1","utf8",json!({}),json!({"member":name}),b"code"));
    let dir=directory(&[(&name,b"code")]);
    assert_eq!(known(&value,&dir).support,Support::Editable);
    for edit in [0,1,2,3] {
        let mut value=value.clone();
        match edit {0=>value["resources"][0]["bytes"]="3".into(),1=>value["resources"][0]["crc32"]="00000000".into(),
            2=>value["resources"][0]["location"]["member"]="manifest.json".into(),_=>{value["resources"]=json!([]);},}
        assert!(parse(&value,&dir).is_err());
    }
    assert!(parse(&value,&directory(&[])).is_err());
    assert_eq!(known(&fixture("empty"),&directory(&[("readme.txt",b"hidden")])).support,Support::Editable);
    assert!(parse(&fixture("empty"),&directory(&[("data/tiles-01.bin",b"hidden")])).is_err());
    assert_eq!(known(&fixture("empty"),&directory(&[("META-INF/content_credential.c2pa",b"opaque")])).support,Support::Editable);
}

#[test]
fn integers_and_resource_ranges_remain_exact_above_javascript_precision() {
    let offset=(1u64<<54)+1;
    let mut value=fixture("empty");push_resource(&mut value,pack_resource(10,offset,b"a"));
    let mut dir=directory(&[("data/tiles-1.bin",b"a")]);let pack=dir.members.last_mut().unwrap();
    pack.length=offset+1;dir.length=pack.offset+pack.length;
    let manifest=known(&value,&dir);assert_eq!(manifest.resources[&id(10)].range.unwrap().offset,dir.members[2].offset+offset);
    for number in [json!(offset),json!("01"),json!("-1"),json!("1.0"),json!("1e2"),json!("18446744073709551616")] {
        let mut value=value.clone();value["resources"][0]["location"]["offset"]=number;assert!(parse(&value,&dir).is_err());
    }
    value["resources"][0]["location"]["offset"]=u64::MAX.to_string().into();assert!(parse(&value,&dir).is_err());
    value["resources"][0]["location"]["offset"]=(offset+1).to_string().into();assert!(parse(&value,&dir).is_err());
    assert_eq!(decimal_u64(&json!(u64::MAX.to_string())).unwrap(),u64::MAX);
}

#[test]
fn compressed_nonimage_resources_require_explicit_exact_decoded_lengths() {
    let dir=directory(&[("data/tiles-1.bin",b"abc")]);
    for kind in ["capy.icc/1","capy.photo-metadata/1","capy.wgsl/1","capy.lut3d/1"] {
        let mut value=fixture("empty");
        push_resource(&mut value,resource_value(10,kind,"capy.lz4-bytes/1",json!({"decoded_bytes":"9007199254740993"}),json!({"pack":"data/tiles-1.bin","offset":"0"}),b"abc"));
        assert_eq!(known(&value,&dir).support,Support::Editable);
        value["resources"][0]["data"]["decoded_bytes"]=json!(123);assert!(parse(&value,&dir).is_err());
        value["resources"][0]["data"].as_object_mut().unwrap().remove("decoded_bytes");assert!(parse(&value,&dir).is_err());
    }
    let mut value=fixture("empty");push_resource(&mut value,pack_resource(10,0,b"abc"));
    value["resources"][0]["data"]["decoded_bytes"]="123".into();assert!(parse(&value,&dir).is_err());
}

#[test]
fn metadata_and_reference_limits_apply_before_graph_adoption() {
    let bytes=serde_json::to_vec(&fixture("paint")).unwrap();
    for limits in [ManifestLimits{metadata_bytes:bytes.len()-1,..Default::default()},
        ManifestLimits{graph:GraphLimits{objects:2,..Default::default()},..Default::default()},
        ManifestLimits{graph:GraphLimits{edges:2,..Default::default()},..Default::default()},
        ManifestLimits{traversal_nodes:2,..Default::default()}] {
        let outcome=Manifest::parse(&bytes,&directory(&[]),limits);
        assert!(matches!(outcome,Err(DecodeError::Unsupported(_))|Ok(ManifestRead::Limited {..})|Ok(ManifestRead::Known(Manifest {support:Support::Preserved(_),..}))),"{outcome:?}");
    }
}
