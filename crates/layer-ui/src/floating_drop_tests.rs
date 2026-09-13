use super::*;

fn drop_panel(
    natural: f32,
    scroll: bool,
    source: Option<f32>,
    room: f32,
    viewport: [f32; 2],
    established: bool,
    footer: bool,
) -> Bounds {
    let mut layout = DockLayout {
        bottom_inset: 24.,
        ..Default::default()
    };
    let panel = if scroll { Panel::Layers } else { Panel::Color };
    if !scroll {
        let DockNode::Split { first, .. } = &mut layout.bands[0].root else {
            unreachable!()
        };
        **first = DockNode::Tabs {
            id: 5,
            panels: vec![panel],
            active: panel,
            tab_style: Default::default(),
        };
    }
    layout
        .panels
        .iter_mut()
        .find(|p| p.id == panel)
        .unwrap()
        .hide_tab = footer;
    layout
        .move_panel(
            viewport,
            panel,
            DockTarget::Float {
                position: [500., 200.],
            },
        )
        .unwrap();
    let group = layout.panel_group(panel).unwrap();
    let chrome = if footer {
        PANEL_GRIP_HEIGHT
    } else {
        TAB_BAR_HEIGHT
    };
    layout.measurements = vec![PanelMeasurement {
        panel,
        tab_width: 100.,
        content_height: natural - chrome,
        scroll: scroll.then_some(PanelScrollMeasurement {
            fixed_height: 72.,
            unit_height: 40.,
        }),
    }];
    let preview = Bounds {
        x: 500.,
        y: if footer {
            crate::HEADER_HEIGHT + room - source.unwrap_or(700.)
        } else {
            viewport[1] - layout.bottom_inset - WORKSPACE_SPACING - room
        },
        width: 350.,
        height: source.unwrap_or(700.),
    };
    layout
        .settle_floating_drop(group, viewport, preview, source, !established)
        .unwrap();
    let placed = layout
        .workspace(
            viewport[0],
            viewport[1],
            crate::HEADER_HEIGHT,
            crate::STATUS_HEIGHT,
        )
        .groups
        .into_iter()
        .find(|g| g.id == group)
        .unwrap()
        .bounds;
    assert!(placed.y >= crate::HEADER_HEIGHT);
    assert!(
        placed.y + placed.height <= viewport[1] - layout.bottom_inset - WORKSPACE_SPACING + 0.01
    );
    // Once settled, content changes (even another ten thousand rows) cannot
    // change the persisted window size or position.
    layout.measurements[0].content_height += 400_000.;
    assert_eq!(
        layout
            .workspace(
                viewport[0],
                viewport[1],
                crate::HEADER_HEIGHT,
                crate::STATUS_HEIGHT
            )
            .groups
            .into_iter()
            .find(|g| g.id == group)
            .unwrap()
            .bounds,
        placed
    );
    placed
}

#[test]
fn content_aware_drop_height_cases() {
    // natural, scrollable, source height, available room, established, expected
    for (natural, scroll, source, room, established, expected) in [
        (280., false, Some(700.), 50., false, 280.), // square stays whole
        (280., false, Some(80.), 700., false, 280.), // squished picker expands
        (188., true, Some(700.), 50., false, 188.),  // only two rows, no blank rows
        (188., true, Some(80.), 700., false, 188.),
        (508., true, Some(80.), 700., false, 400.), // squished sidebar expands
        (508., true, Some(320.), 700., false, 320.), // useful sidebar preserved
        (508., true, Some(700.), 700., false, 400.),
        (900_000., true, None, 700., false, 400.), // icon, massive virtual list
        (900_000., true, Some(700.), 290., false, 290.), // use remaining room
        (900_000., true, Some(700.), 10., false, 268.), // four complete rows
        (900_000., true, Some(700.), 800., true, 700.), // existing manual size
        (900_000., true, Some(700.), 10., true, 268.),
        (900_000., true, Some(80.), 10., true, 80.), // preserve explicit tiny size
        (280., false, Some(500.), 10., true, 500.),
    ] {
        let actual = drop_panel(
            natural,
            scroll,
            source,
            room,
            [1200., 1000.],
            established,
            false,
        );
        assert_eq!(
            actual.height, expected,
            "natural={natural}, source={source:?}, room={room}, established={established}"
        );
    }
}

#[test]
fn footer_anchor_and_small_viewports_are_fitted() {
    let placed = drop_panel(280., false, Some(700.), 500., [1200., 1000.], false, true);
    assert_eq!(placed.height, 280.);
    assert_eq!(placed.y + placed.height, crate::HEADER_HEIGHT + 500.);
    let placed = drop_panel(
        900_000.,
        true,
        Some(700.),
        290.,
        [1200., 1000.],
        false,
        true,
    );
    assert_eq!(placed.height, 290.);
    assert_eq!(placed.y, crate::HEADER_HEIGHT);
    for scroll in [false, true] {
        for footer in [false, true] {
            let placed = drop_panel(
                900_000.,
                scroll,
                Some(700.),
                1.,
                [500., 240.],
                false,
                footer,
            );
            assert_eq!(
                placed.height,
                240. - 24. - WORKSPACE_SPACING - crate::HEADER_HEIGHT
            );
        }
    }
}
