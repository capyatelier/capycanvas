#[test]
fn packaged_language_tags_match_the_shared_shipped_inventory() {
    let xml = include_str!("../../app/src/main/res/xml/locales_config.xml");
    let tags = xml.split("android:name=\"").skip(1)
        .map(|value| value.split_once('"').unwrap().0).collect::<Vec<_>>();
    let shipped = layer_ui::localization::SHIPPED_LANGUAGES.iter()
        .map(|language| language.tag()).collect::<Vec<_>>();
    assert_eq!(tags, shipped);
}
#[test]
fn android_launch_uses_supplied_preferences_before_first_view_without_gpu() {
    for (saved, tag) in [("", "ja"), ("broken", "ja"), (r#"{"language":{"Explicit":"en"},"pan_speed":"broken"}"#, "en"),
        (r#"{"language":{"Explicit":"ja"}}"#, "ja")] {
        let host = layer_host::NativeHost::launch(layer_ui::Platform::Android, saved, &["ja-JP", "zh-Hant", "en-US"]).unwrap();
        let expected = layer_ui::launch_localization(saved, &["ja-JP", "zh-Hant", "en-US"]);
        assert!(std::sync::Arc::ptr_eq(host.session.localization(), &expected));
        assert_eq!(host.bootstrap_view().active_tag, tag);
        assert_eq!(host.bootstrap_view().shipped_tags, layer_ui::localization::SHIPPED_LANGUAGES.iter().map(|language| language.tag()).collect::<Vec<_>>());
        assert!(host.session.engine().backend().0.is_none());
    }
}
#[test]
fn every_jni_function_runs_inside_jni_entry() {
    let mut pending = vec![std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))];
    let mut unguarded = Vec::new();
    let mut functions = 0;
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(std::fs::read_dir(path).unwrap().map(|entry| entry.unwrap().path()));
            continue;
        }
        if path.extension().is_none_or(|extension| extension != "rs") { continue; }
        let source = std::fs::read_to_string(&path).unwrap();
        for function in source.split("extern \"system\" fn ").skip(1) {
            functions += 1;
            let (signature, body) = function.split_once('{').unwrap();
            if !body.trim_start().starts_with("crate::jni_entry!(") {
                unguarded.push(signature.split('(').next().unwrap().to_owned());
            }
        }
    }
    assert!(functions > 100);
    assert!(unguarded.is_empty(), "{unguarded:?}");
}

#[test]
fn jni_entry_returns_load_failure_after_a_panic() {
    let value = crate::jni_entry!(result => { result.unwrap_or(-1) }; {
        std::panic::resume_unwind(Box::new("load failed"))
    });
    assert_eq!(value, -1);
}

#[test]
#[cfg(unix)]
fn jni_entry_writes_photo_failures_to_the_error_pipe() {
    use std::io::{Read, Write};
    for panic in [false, true] {
        let (mut reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
        let value = crate::jni_entry!(result => {
            match result {
                Ok(()) => 1,
                Err(error) => { writer.write_all(error.as_bytes()).unwrap(); 0 }
            }
        }; {
            if panic { std::panic::resume_unwind(Box::new("decoder failed")); }
            Err("decoder refused input".to_owned())
        });
        drop(writer);
        let mut error = String::new();
        reader.read_to_string(&mut error).unwrap();
        assert_eq!(value, 0);
        assert_eq!(error, if panic { "Native operation panicked: decoder failed" } else { "decoder refused input" });
    }
}
