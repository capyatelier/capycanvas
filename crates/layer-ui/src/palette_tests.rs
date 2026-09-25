// Included in session::tests: real engine commits with the protocol recorder.
#[test]
fn palette_history_tracks_completed_paint_not_selection_preview_cancel_or_erase() {
    use layer_core::color::{RgbColor, RgbSpace};
    let mut s = session();
    let color = RgbColor::new(RgbSpace::DisplayP3, [0.7, 0.2, 0.1, 0.8]).unwrap();
    s.dispatch(UiAction::Color {
        action: ColorAction::Definition { color },
    })
    .unwrap();
    s.dispatch(UiAction::Color {
        action: ColorAction::Library {
            action: crate::ColorLibraryAction::Store {
                palette: 1,
                name: String::new(),
                color,
            },
        },
    })
    .unwrap();
    let id = s.state.colors.library.palettes[0].swatches[0].id;
    s.dispatch(UiAction::Color {
        action: ColorAction::Library {
            action: crate::ColorLibraryAction::Use { id },
        },
    })
    .unwrap();
    assert!(s.state.colors.library.history.is_empty());
    s.pen(event(&s, 1, PenPhase::Down, 0.8)).unwrap();
    s.frame(10_000_000, 18_000_000).unwrap();
    assert!(
        s.state.colors.library.history.is_empty(),
        "live preview is not history"
    );
    s.pen(event(&s, 2, PenPhase::Cancel, 0.)).unwrap();
    s.frame(20_000_000, 28_000_000).unwrap();
    assert!(s.state.colors.library.history.is_empty());
    s.pen(event(&s, 3, PenPhase::Down, 0.8)).unwrap();
    s.pen(event(&s, 4, PenPhase::Up, 0.)).unwrap();
    s.frame(40_000_000, 48_000_000).unwrap();
    assert_eq!(s.state.colors.library.history, [color]);
    invoke(&mut s, CommandId::Undo);
    assert_eq!(
        s.state.colors.library.history,
        [color],
        "undo does not erase usage history"
    );
    s.dispatch(UiAction::Color {
        action: ColorAction::Definition {
            color: RgbColor::WHITE,
        },
    })
    .unwrap();
    invoke(&mut s, CommandId::Eraser);
    s.pen(event(&s, 5, PenPhase::Down, 0.8)).unwrap();
    s.pen(event(&s, 6, PenPhase::Up, 0.)).unwrap();
    s.frame(60_000_000, 68_000_000).unwrap();
    assert_eq!(s.state.colors.library.history, [color]);
}

#[test]
fn palette_history_records_successful_fill_definitions() {
    use layer_core::color::{RgbColor, RgbSpace};
    let mut s = session();
    let color = RgbColor::new(RgbSpace::DisplayP3, [0.9, 0.2, 0.1, 0.5]).unwrap();
    s.dispatch(UiAction::Color {
        action: ColorAction::Definition { color },
    })
    .unwrap();
    let kind = s.fill_operation();
    s.paint_operation(None, kind, &[color]).unwrap();
    assert_eq!(s.state.colors.library.history, [color]);
    let before = s.state.colors.library.history.clone();
    invoke(&mut s, CommandId::Undo);
    s.dispatch(UiAction::Color {
        action: ColorAction::Definition {
            color: RgbColor::BLACK,
        },
    })
    .unwrap();
    assert_eq!(s.state.colors.library.history, before);
}
