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
        let view = state.view();
        assert_eq!(
            view.outside_document_gamut,
            !definition.in_gamut(space).unwrap()
        );
        assert!(view.outside_display_gamut);
        assert_eq!(view.marker_color, [1., 0., 0.]);
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
        state
            .apply(ColorAction::RgbaComponent {
                index: 3,
                value: 1. / 65535.,
            })
            .unwrap();
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

#[test]
fn all_working_fields_show_the_color_they_pick_in_srgb_fallback() {
    let side = 137;
    let geometry = ColorWheelGeometry::new(side as f32).unwrap();
    for space in RgbSpace::ALL {
        let mut state = ColorState::default();
        state.set_rgb_space(space).unwrap();
        state.set_rgba([0.13, 0.82, 0.47, 1.]).unwrap();
        for shape in [ColorShape::Circle, ColorShape::Square, ColorShape::Triangle] {
            state.apply(ColorAction::Shape { shape }).unwrap();
            let mut pixels = vec![0; side as usize * side as usize * 4];
            assert!(state.render_field(side, &mut pixels));
            for y in (20..side - 20).step_by(7) {
                for x in (20..side - 20).step_by(7) {
                    let point = [x as f32 + 0.5, y as f32 + 0.5];
                    let inside = match shape {
                        ColorShape::Circle => {
                            (point[0] - geometry.center[0]).hypot(point[1] - geometry.center[1])
                                < geometry.disc_radius() - 1.
                        }
                        ColorShape::Square => point.into_iter().enumerate().all(|(i, v)| {
                            v > geometry.square[i] && v < geometry.square[i] + geometry.square[2]
                        }),
                        ColorShape::Triangle => barycentric(geometry.triangle, point)
                            .into_iter()
                            .all(|v| v > 0.01),
                    };
                    if !inside {
                        continue;
                    }
                    let mut picked = state.clone();
                    picked
                        .apply(ColorAction::PickWheel {
                            part: ColorWheelPart::Field,
                            point,
                            size: side as f32,
                        })
                        .unwrap();
                    assert_eq!(picked.definition().space, space);
                    picked.validate().unwrap();
                    let expected = picked
                        .preview(picked.definition())
                        .map(|v| (v * 255.).round() as u8);
                    let actual = &pixels[(y as usize * side as usize + x as usize) * 4..][..4];
                    assert!(
                        actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 1),
                        "{space:?} {shape:?} {point:?}: {actual:?} != {expected:?}"
                    );
                }
            }
        }
    }
}
