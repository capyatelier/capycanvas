fn main() {
    let source = "../layer-web/icons";
    println!("cargo:rerun-if-changed={source}");
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let previews = "../layer-web/filter-previews";
    println!("cargo:rerun-if-changed={previews}");
    let mut swatches: Vec<_> = std::fs::read_dir(previews).unwrap().map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "webp")).collect();
    swatches.sort();
    let entries = swatches.iter().map(|path| format!("({:?}, include_bytes!({:?}) as &'static [u8])",
        path.file_stem().unwrap().to_str().unwrap(), path.canonicalize().unwrap())).collect::<Vec<_>>().join(",\n");
    std::fs::write(output.join("filter_swatches.rs"), format!("&[{entries}]")).unwrap();
    let mut names: Vec<_> = std::fs::read_dir(source)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".svg"))
        .collect();
    names.sort();
    let mut xml = String::from("<gresources><gresource prefix=\"/dev/layer/icons\">");
    let mark = std::fs::read_to_string(format!("{source}/layer-zen-looking-up-symbolic.svg")).unwrap();
    let view: Vec<f64> = mark.split_once("viewBox=\"").unwrap().1.split('"').next().unwrap()
        .split_whitespace().map(|value| value.parse().unwrap()).collect();
    assert!(view.len() == 4 && view[2] > 0. && view[2] == view[3]);
    let geometry = mark.split_once('>').unwrap().1.rsplit_once("</svg>").unwrap().0;
    let icon = output.join("art.capycanvas.CapyCanvas.svg");
    std::fs::write(&icon, format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"512\" height=\"512\"><rect width=\"512\" height=\"512\" rx=\"76.8\" fill=\"#767676\"/><g color=\"#f6f5f4\" transform=\"translate(36 36) scale({}) translate({} {})\">{geometry}</g></svg>", 440. / view[2], -view[0], -view[1])).unwrap();
    xml.push_str(&format!(
        "<file alias=\"scalable/apps/art.capycanvas.CapyCanvas.svg\">{}</file>",
        icon.display()
    ));
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
