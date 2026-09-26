//! Included inside the filter-library test module in each historical checkout.
use super::*;

#[test]
#[ignore = "release hardware comparison using CAPY_FILTER_SOURCE_JPEG"]
fn historical_photo_filter() {
    let path = std::env::var("CAPY_FILTER_SOURCE_JPEG").unwrap();
    let source = layer_color::photo::read_photo(
        std::io::BufReader::new(std::fs::File::open(path).unwrap()),
        Default::default(),
    )
    .unwrap();
    let project =
        layer_color::photo_project(source, "Water", layer_core::color::SampleDepth::U8).unwrap();
    let extent = [project.document.width, project.document.height];
    let mut r = WgpuRasterizer::new_native_headless(project.document.color).unwrap();
    eprintln!("adapter={:?}", r.adapter.get_info());
    let base = project.document.layers[0].clone();
    submit(&mut r, extent, &[base.clone()], 0., true, true, None);
    r.wait_idle().unwrap();
    for name in ["gaussian_blur", "domain_warp"] {
        let mut layers = vec![filter(fixture(name)), base.clone()];
        if name == "domain_warp" {
            Arc::make_mut(layers[0].effect.as_mut().unwrap())
                .set("animate", EffectValue::Toggle(true))
                .unwrap();
        }
        for i in 0..12 {
            if name == "gaussian_blur" {
                layers[0].opacity = if i % 2 == 0 { 0.99 } else { 1. };
            }
            let start = std::time::Instant::now();
            submit(
                &mut r,
                extent,
                &layers,
                i as f32 / 60.,
                false,
                name == "gaussian_blur",
                None,
            );
            let cpu = start.elapsed().as_secs_f64() * 1000.;
            r.wait_idle().unwrap();
            eprintln!(
                "history,{name},{i},{cpu:.3},{:.3}",
                start.elapsed().as_secs_f64() * 1000.
            );
        }
    }
}
