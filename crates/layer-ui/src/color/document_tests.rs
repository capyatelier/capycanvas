use super::*;

#[test]
fn definitions_survive_picker_display_alpha_and_workspace_changes() {
    let definition = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0., 0.37]).unwrap();
    let mut state = ColorState::default();
    state.set_color(definition).unwrap();
    for space in RgbSpace::ALL {
        state.set_rgb_space(space).unwrap();
        assert_eq!(state.definition(), definition);
        assert_eq!(state.rgba(), definition.encoded_in(space).unwrap());
        assert_eq!(state.view().marker_color, [1., 0., 0.]);
        for shape in [ColorShape::Square, ColorShape::Triangle, ColorShape::Circle] {
            state.apply(ColorAction::Shape { shape }).unwrap();
            assert_eq!(state.definition(), definition);
            assert!(state.wheel_components().into_iter().all(f32::is_finite));
            state.validate().unwrap();
        }
        let bytes = serde_json::to_vec(&state).unwrap();
        state = serde_json::from_slice(&bytes).unwrap();
        state.validate().unwrap();
        assert_eq!(state.definition(), definition);
        let mut color = state.definition();
        color.rgba[3] = 1. / 65535.;
        state.apply(ColorAction::Definition { color }).unwrap();
        assert_eq!(state.definition().space, definition.space);
        assert_eq!(state.definition().rgba[..3], definition.rgba[..3]);
        assert_eq!(state.definition().rgba[3], 1. / 65535.);
        state.set_color(definition).unwrap();
    }
    let before = state.clone();
    let invalid = RgbColor { linear_rgb: None,
        space: RgbSpace::ProPhoto,
        rgba: [f32::MAX, 0., 0., 1.],
    };
    assert!(state.set_color(invalid).is_err());
    assert_eq!(state, before);
}
