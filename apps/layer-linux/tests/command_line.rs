use std::process::{Command, Output};

fn without_display(args: &[&str]) -> Output {
    let storage = std::env::temp_dir().join(format!("capy-cli-{}", layer_core::PortableId::random()));
    let output = Command::new(env!("CARGO_BIN_EXE_layer-linux"))
        .args(args)
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .env("GDK_BACKEND", "wayland")
        .env("XDG_RUNTIME_DIR", &storage)
        .env("CAPY_STORAGE_DIR", &storage)
        .env("LC_ALL", "C")
        .output().unwrap();
    if storage.exists() { std::fs::remove_dir_all(&storage).unwrap(); }
    output
}

#[test]
fn help_is_available_without_a_display() {
    for option in ["--help", "--help-all"] {
        let output = without_display(&[option]);
        assert!(output.status.success(), "{option}: {}", String::from_utf8_lossy(&output.stderr));
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.contains("--window-size=WIDTHxHEIGHT"), "{option}: {help}");
    }
}

#[test]
fn editor_still_requires_a_display() {
    let output = without_display(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Failed to initialize GTK"));
}
