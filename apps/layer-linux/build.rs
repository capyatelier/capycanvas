fn main() {
    let source = "../layer-web/icons";
    println!("cargo:rerun-if-changed={source}");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let mut names: Vec<_> = std::fs::read_dir(source)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".svg"))
        .collect();
    names.sort();
    let mut xml = String::from("<gresources><gresource prefix=\"/dev/layer/icons\">");
    for name in names {
        xml.push_str(&format!(
            "<file alias=\"scalable/actions/{name}\">{name}</file>"
        ));
    }
    xml.push_str("</gresource></gresources>");
    let manifest = output.join("icons.gresource.xml");
    std::fs::write(&manifest, xml).unwrap();
    let target = output.join("layer-icons.gresource");
    let result = std::process::Command::new("glib-compile-resources")
        .arg(manifest)
        .args(["--sourcedir=../layer-web/icons", "--target"])
        .arg(target)
        .status()
        .expect("glib-compile-resources is required by the GTK build");
    assert!(result.success(), "compile bundled symbolic icons");
}
