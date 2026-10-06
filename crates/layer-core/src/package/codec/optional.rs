use super::*;

#[test]
fn unknown_or_unusable_representations_do_not_gate_editable_artwork() {
    let bytes=serialize(&prepare(&fixture(SampleDepth::U8),true));
    for representation in [json!({"member":"preview.png","size":[2048,2048],"color":"srgb"}),
        json!({"member":"preview.png","size":[2,1],"color":"display_p3"}),
        json!({"member":"preview-large.png","size":[2,1],"color":"srgb"}),
        json!({"ref":"opaque future preview identifier"}),json!(null)] {
        let changed=rewrite(&bytes,|manifest| {
            let output=manifest["objects"].as_array_mut().unwrap().iter_mut().find(|r|r["type"]=="capy.output/2").unwrap();
            output["data"]["representation"]=representation;
        });
        assert!(matches!(open(backing(changed),Default::default(),&AtomicBool::new(false)).unwrap(),OpenOutcome::Candidate {preview:None,..}));
    }
}

#[test]
fn unreferenced_members_are_disposable_but_data_remains_indexed() {
    let bytes=serialize(&prepare(&fixture(SampleDepth::U8),true));
    let changed=rewrite_with(&bytes,vec![("thumbnails/large.png".into(),vec![17,29,41])],|_|{});
    let OpenOutcome::Candidate {artwork,source,..}=open(backing(changed.clone()),Default::default(),&AtomicBool::new(false)).unwrap() else {panic!("Optional member prevented editing")};
    let mut copied=Vec::new();copy_original(&source,&mut copied,&AtomicBool::new(false)).unwrap();assert_eq!(copied,changed);
    let saved=serialize(&prepare(&artwork,false));
    assert!(Directory::read(&mut Cursor::new(saved),262144,64*1024*1024).unwrap().member("thumbnails/large.png").is_none());
    for name in [format!("data/{}",identity(999)),"data/tiles-2.bin".into(),"data/unknown.bin".into()] {
        let changed=rewrite_with(&bytes,vec![(name,vec![17])],|_|{});
        assert!(matches!(open(backing(changed),Default::default(),&AtomicBool::new(false)).unwrap(),OpenOutcome::RecoveredView {..}));
    }
}
