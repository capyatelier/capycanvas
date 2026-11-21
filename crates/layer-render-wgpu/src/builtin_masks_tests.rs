use super::*;

#[test]
fn generated_pixels_do_not_signal_readiness_before_upload() {
    let mut masks = Masks(vec![Mask {
        id: AssetId::from("test:tip"),
        pixels: Deferred::new(|| std::sync::Mutex::new(Some(Ok((1, 1, vec![255]))))),
        requested: true,
        uploaded: false,
    }]);
    crate::compile::execute(||masks.0[0].pixels.compile());
    assert!(!masks.ready());
    assert_eq!(masks.take_ready().unwrap().len(), 1);
    assert!(masks.ready());
    assert!(masks.take_ready().unwrap().is_empty());
}
