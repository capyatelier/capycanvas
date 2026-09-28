fn image_size(s: &mut UiSession<Recorder>, action: ImageSizeAction) -> UiChange {
    s.dispatch(UiAction::ImageSize { action }).unwrap()
}

fn image_size_view(s: &UiSession<Recorder>) -> ImageSizeView {
    s.state.layer_tools.image_size.clone().expect("Image Size is open")
}

fn resampled(s: &mut UiSession<Recorder>) -> Vec<(LayerId, layer_core::ImageTransform)> {
    s.renderer_mut()
        .pending_operations
        .iter()
        .map(|(id, op)| match &op.kind {
            layer_core::LayerOperationKind::Transform(transform) => (*id, transform.clone()),
            kind => panic!("a resample, not {kind:?}"),
        })
        .collect()
}

#[test]
fn image_size_scales_down_with_constrained_proportions_in_one_undo_step() {
    let mut s = crop_session();
    let paint = s.engine.document().layers[0].id;
    rectangle_selection(&mut s, [100., 100., 300., 200.]);
    let before = s.engine.document().clone();
    invoke(&mut s, CommandId::ImageSize);
    let view = image_size_view(&s);
    assert_eq!((view.title, view.values, view.unit), ("Image Size", [1000., 800.], CanvasSizeUnit::Pixels));
    assert_eq!((view.constrain, view.resample, view.resolution), (true, ImageResample::Automatic, 72.));
    assert_eq!(view.resamples.iter().map(|r| r.label).collect::<Vec<_>>(), ["Automatic", "Bicubic", "Lanczos", "Bilinear", "Nearest neighbor"]);
    assert!(!view.can_apply);
    assert_eq!(view.message, "Current size: 1000 × 800 px");
    image_size(&mut s, ImageSizeAction::Width { value: 500.4 });
    let view = image_size_view(&s);
    assert_eq!(view.values, [500., 400.], "the height follows");
    assert_eq!(view.message, "New size: 500 × 400 px");
    image_size(&mut s, ImageSizeAction::Height { value: 200. });
    assert_eq!(image_size_view(&s).values, [250., 200.], "and the width follows the height");
    image_size(&mut s, ImageSizeAction::Width { value: 500. });
    let change = image_size(&mut s, ImageSizeAction::Apply);
    assert_ne!(change.regions & regions::DOCUMENT, 0);
    assert!(s.state.layer_tools.image_size.is_none());
    s.frame(20, 20).unwrap();
    let doc = s.engine.document();
    assert_eq!([doc.width, doc.height], [500, 400]);
    let [(id, transform)] = resampled(&mut s).try_into().unwrap();
    assert_eq!(id, paint);
    assert_eq!(transform.interpolation, layer_core::Interpolation::Lanczos, "Automatic keeps detail when reducing");
    let half = layer_core::Affine([0.5, 0., 0., 0.5, 0., 0.]);
    assert_eq!(s.engine.document().selection, Some(before.selection.as_ref().unwrap().transformed(half).unwrap()));
    invoke(&mut s, CommandId::Undo);
    s.frame(21, 21).unwrap();
    assert_eq!([s.engine.document().width, s.engine.document().height], [1000, 800]);
    assert_eq!(s.engine.document().layers, before.layers);
    assert!(s.engine.can_undo(), "only the Image Size step was undone");
    invoke(&mut s, CommandId::Undo);
    assert!(s.engine.document().selection.is_none());
}

#[test]
fn image_size_in_percent_with_free_proportions_and_a_chosen_filter() {
    let mut s = crop_session();
    invoke(&mut s, CommandId::ImageSize);
    image_size(&mut s, ImageSizeAction::Unit { unit: CanvasSizeUnit::Percent });
    let view = image_size_view(&s);
    assert_eq!(view.values, [100., 100.]);
    assert_eq!(view.numeric[0].unit, "%");
    image_size(&mut s, ImageSizeAction::Constrain { constrain: false });
    image_size(&mut s, ImageSizeAction::Width { value: 150. });
    assert_eq!(image_size_view(&s).values, [150., 100.]);
    assert_eq!(image_size_view(&s).message, "New size: 1500 × 800 px");
    image_size(&mut s, ImageSizeAction::Unit { unit: CanvasSizeUnit::Pixels });
    assert_eq!(image_size_view(&s).values, [1500., 800.]);
    image_size(&mut s, ImageSizeAction::Resample { resample: ImageResample::Nearest });
    image_size(&mut s, ImageSizeAction::Apply);
    s.frame(20, 20).unwrap();
    assert_eq!([s.engine.document().width, s.engine.document().height], [1500, 800]);
    let [(_, transform)] = resampled(&mut s).try_into().unwrap();
    assert_eq!(transform.interpolation, layer_core::Interpolation::Nearest);
    assert_eq!(transform.as_affine(), Some(layer_core::Affine([1.5, 0., 0., 1., 0., 0.])));
    invoke(&mut s, CommandId::Undo);
    invoke(&mut s, CommandId::ImageSize);
    image_size(&mut s, ImageSizeAction::Width { value: 1200. });
    image_size(&mut s, ImageSizeAction::Apply);
    s.frame(22, 22).unwrap();
    let [(_, transform)] = resampled(&mut s).try_into().unwrap();
    assert_eq!(transform.interpolation, layer_core::Interpolation::Bicubic, "Automatic stays smooth when enlarging");
}

#[test]
fn image_size_changes_only_the_resolution_or_both_in_one_step() {
    let mut s = crop_session();
    invoke(&mut s, CommandId::ImageSize);
    image_size(&mut s, ImageSizeAction::Resolution { value: 300. });
    let view = image_size_view(&s);
    assert!(view.can_apply);
    assert_eq!(view.message, "Only the resolution changes, to 300 ppi");
    assert!(s.dispatch(UiAction::ImageSize { action: ImageSizeAction::Resolution { value: 0. } }).is_err());
    image_size(&mut s, ImageSizeAction::Apply);
    s.frame(20, 20).unwrap();
    let doc = s.engine.document();
    assert_eq!([doc.width, doc.height], [1000, 800]);
    assert_eq!(doc.resolution, Some(layer_core::ImageResolution::ppi(300)));
    assert!(s.renderer_mut().pending_operations.is_empty(), "no pixel work");
    invoke(&mut s, CommandId::ImageSize);
    assert_eq!(image_size_view(&s).resolution, 300.);
    image_size(&mut s, ImageSizeAction::Resolution { value: 150.4 });
    image_size(&mut s, ImageSizeAction::Width { value: 2000. });
    image_size(&mut s, ImageSizeAction::Apply);
    s.frame(21, 21).unwrap();
    let doc = s.engine.document();
    assert_eq!([doc.width, doc.height], [2000, 1600]);
    assert_eq!(doc.resolution, Some(layer_core::ImageResolution::ppi(150)), "whole pixels per inch");
    invoke(&mut s, CommandId::Undo);
    let doc = s.engine.document();
    assert_eq!(([doc.width, doc.height], doc.resolution), ([1000, 800], Some(layer_core::ImageResolution::ppi(300))), "one step");
}

#[test]
fn image_size_validates_limits_and_refuses_while_cropping() {
    let mut s = crop_session();
    s.renderer_mut().max_dimension = Some(1500);
    invoke(&mut s, CommandId::ImageSize);
    image_size(&mut s, ImageSizeAction::Width { value: 2000. });
    let view = image_size_view(&s);
    assert!(!view.can_apply);
    assert_eq!(view.message, "The canvas can be at most 1500 px on each side");
    assert!(s.dispatch(UiAction::ImageSize { action: ImageSizeAction::Width { value: f64::NAN } }).is_err());
    image_size(&mut s, ImageSizeAction::Cancel);
    assert!(s.state.layer_tools.image_size.is_none());
    assert!(s.dispatch(UiAction::ImageSize { action: ImageSizeAction::Apply }).is_err());
    invoke(&mut s, CommandId::Crop);
    assert!(!s.command(CommandId::ImageSize).enabled);
    assert_eq!(s.command_disabled_reason(CommandId::ImageSize).as_deref(), Some("Apply or cancel the crop first"));
}

#[test]
fn image_size_has_ctrl_alt_i_in_the_photoshop_and_affinity_keys() {
    let chord = crate::shortcuts::KeyChord { key: "i".into(), command: true, shift: false, alt: true };
    assert!(chord.available(Platform::Web));
    for id in ["photoshop", "affinity"] {
        let preset = crate::keymaps::preset(id).unwrap();
        assert_eq!(preset.keys_for("command.ImageSize"), Some(std::slice::from_ref(&chord)), "{id}");
        assert!(preset.keys.iter().filter(|(_, keys)| keys.contains(&chord)).count() == 1, "{id}: one command per chord");
    }
    assert!(!crate::keymaps::preset("capy").is_some_and(|p| p.binds(&chord)));
}
