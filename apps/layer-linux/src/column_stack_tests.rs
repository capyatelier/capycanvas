// Native input coverage for collapsed column stacks.
use super::*;

#[test]
#[ignore = "isolated Mutter mouse/touch driver: --column-stacks"]
fn native_column_stack_input() {
    let dir = std::path::PathBuf::from(std::env::var("LAYER_NATIVE_INPUT_DIR").unwrap());
    let output = std::path::PathBuf::from(
        std::env::var("LAYER_TEST_ARTIFACTS").unwrap_or_else(|_| dir.to_string_lossy().into()),
    );
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.ColumnStacks");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !w.workspaces.ready.get() || w.workspaces.busy.get() {
        assert!(Instant::now() < deadline, "workspace storage startup");
        pump(10);
    }
    let mut step = 0;
    let mut perform = |events: serde_json::Value| {
        let file = dir.join(format!("step-{step}.json"));
        let temporary = file.with_extension("tmp");
        std::fs::write(&temporary, serde_json::to_vec(&events).unwrap()).unwrap();
        std::fs::rename(temporary, file).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !dir.join(format!("done-{step}")).exists() {
            assert!(Instant::now() < deadline, "native input timed out");
            pump(5);
        }
        step += 1;
        pump(160);
    };
    let center = |b: Bounds| [b.x + b.width * 0.5, b.y + b.height * 0.5];
    std::fs::write(dir.join("ready"), "ready").unwrap();
    pump(500);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    for theme in [Theme::Dark, Theme::Light] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: layer_ui::WorkspaceState {
                layout: layer_ui::WorkspacePreset::Illustrator.layout(Platform::Gtk),
                ..Default::default()
            },
        });
        pump(300);
        let resolved = w.resolved();
        assert_eq!(resolved.collapsed.len(), 1);
        assert_eq!(resolved.collapsed[0].id, 12);
        assert!(
            resolved
                .groups
                .iter()
                .any(|g| g.panels.contains(&Panel::Brushes))
        );
        for column in &resolved.collapsed {
            let stack = state(&w).workspace.layout.column_stack(column.id);
            assert!(!stack.auto_hide && !stack.drawers);
            let open = column.open.as_ref().unwrap();
            assert_eq!(open.bounds.y, column.bounds.y);
            assert_eq!(open.bounds.height, column.bounds.height);
            for group in &column.groups {
                let view = w
                    .groups
                    .borrow()
                    .iter()
                    .find(|g| g.id == group.group)
                    .unwrap()
                    .root
                    .clone();
                assert!(view.is_mapped() && view.width() > 100 && view.height() > 30);
            }
        }
        let canvas = center(resolved.work_area);
        perform(serde_json::json!([{"point":canvas,"down":true},{"down":false}]));
        assert!(w.resolved().collapsed.iter().all(|c| c.open.is_some()));
        crate::capture(
            &w,
            output
                .join(format!("paint-default-{theme:?}.png"))
                .to_str()
                .unwrap(),
        );
    }
    for (theme, edge) in [(Theme::Dark, Edge::Left), (Theme::Light, Edge::Right)] {
        for touch in [false, true] {
            let event = |phase: &str, point: [f32; 2]| {
                if touch {
                    serde_json::json!({"touch":phase,"point":point})
                } else {
                    match phase {
                        "down" => serde_json::json!({"point":point,"down":true}),
                        "up" => serde_json::json!({"down":false}),
                        _ => serde_json::json!({"point":point}),
                    }
                }
            };
            let click = |p| serde_json::json!([event("down", p), event("up", p)]);
            let mut initial = layer_ui::WorkspaceState::default();
            initial.layout.bands[0].edge = edge;
            initial.layout.bands[1].edge = if edge == Edge::Left {
                Edge::Right
            } else {
                Edge::Left
            };
            initial
                .layout
                .set_column_collapsed(5, true, viewport)
                .unwrap();
            initial
                .layout
                .set_column_collapsed(8, true, viewport)
                .unwrap();
            w.dispatch(UiAction::RestoreWorkspace { workspace: initial });
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            pump(300);
            let original = layer_ui::durable_layout(&state(&w).workspace.layout);
            assert!(find_named(w.surface.upcast_ref(), "expand-column-4").is_none());
            let r = w.resolved();
            let source = center(r.collapsed.iter().find(|c| c.id == 8).unwrap().grip);
            let target = center(r.collapsed.iter().find(|c| c.id == 4).unwrap().grip);
            // Handles pick up immediately for mouse and touch, without a hold.
            perform(serde_json::json!([
                event("down", source),
                event("move", target),
                event("up", target)
            ]));
            let stack = state(&w).workspace.layout.column_stack(4);
            assert_eq!(stack.members, [4, 8], "native column-handle drop");
            let stacked = layer_ui::durable_layout(&state(&w).workspace.layout);
            w.dispatch(UiAction::Invoke {
                command: CommandId::UndoWorkspace,
            });
            pump(150);
            assert_eq!(
                layer_ui::durable_layout(&state(&w).workspace.layout),
                original
            );
            w.dispatch(UiAction::Invoke {
                command: CommandId::RedoWorkspace,
            });
            pump(150);
            assert_eq!(
                layer_ui::durable_layout(&state(&w).workspace.layout),
                stacked
            );
            let resolved = w.resolved();
            let layout = state(&w).workspace.layout;
            let fixed = resolved
                .dividers
                .iter()
                .find(|d| layout.fixed_stack_divider(d))
                .unwrap();
            let handle = w
                .surface
                .imp()
                .children
                .borrow()
                .iter()
                .find(|(slot, _)| *slot == Slot::Divider(fixed.id))
                .unwrap()
                .1
                .clone();
            assert!(!handle.can_target() && !handle.is_focusable());
            let start = center(fixed.bounds);
            let moved = [viewport[0] * 0.5, start[1]];
            perform(serde_json::json!([
                event("down", start),
                event("move", moved),
                event("up", moved)
            ]));
            assert_eq!(
                layer_ui::durable_layout(&state(&w).workspace.layout),
                stacked
            );
            let empty = center(
                w.resolved()
                    .collapsed
                    .into_iter()
                    .find(|c| c.id == 8)
                    .unwrap()
                    .empty,
            );
            if touch {
                perform(serde_json::json!([event("down", empty)]));
                pump(800);
                perform(serde_json::json!([event("up", empty)]));
            } else {
                perform(
                    serde_json::json!([{"point":empty,"button":273,"down":true},{"button":273,"down":false}]),
                );
            }
            let menu = find_css(w.surface.upcast_ref(), "panel-context-menu")
                .unwrap()
                .downcast::<gtk::PopoverMenu>()
                .unwrap();
            let action =
                menu_action(&menu.menu_model().unwrap(), "Open individual panels").unwrap();
            assert!(menu_action(&menu.menu_model().unwrap(), "Group panel").is_none());
            menu.activate_action(&action, None).unwrap();
            menu.popdown();
            pump(160);
            assert!(!state(&w).workspace.layout.column_stack(4).drawers);
            let icon = |panel| {
                w.resolved()
                    .collapsed
                    .into_iter()
                    .flat_map(|c| c.groups)
                    .flat_map(|g| g.icons)
                    .find(|i| i.panel == panel)
                    .unwrap()
                    .bounds
            };
            perform(click(center(icon(Panel::Brushes))));
            let r = w.resolved();
            let member = r.collapsed.iter().find(|c| c.id == 4).unwrap();
            let open = member.open.as_ref().unwrap();
            let second = r.collapsed.iter().find(|c| c.id == 8).unwrap();
            assert_eq!(open.bounds.y, member.bounds.y);
            assert_eq!(
                open.bounds.y + open.bounds.height,
                second.bounds.y + second.bounds.height
            );
            assert_eq!(
                second.bounds.y - member.bounds.y - member.bounds.height,
                WORKSPACE_SPACING
            );
            for column in [member, second] {
                let root = find_named(
                    w.surface.upcast_ref(),
                    &format!("collapsed-column-{}", column.id),
                )
                .unwrap();
                let bounds = root.compute_bounds(&w.surface).unwrap();
                assert!((bounds.y() - column.bounds.y).abs() <= 1.);
                assert!((bounds.height() - column.bounds.height).abs() <= 1.);
                fn separators(root: &gtk::Widget) -> usize {
                    let mut count = usize::from(root.is::<gtk::Separator>());
                    let mut child = root.first_child();
                    while let Some(widget) = child {
                        count += separators(&widget);
                        child = widget.next_sibling();
                    }
                    count
                }
                assert_eq!(separators(&root), column.groups.len().saturating_sub(1));
                for icon in column.groups.iter().flat_map(|g| &g.icons) {
                    let tile = w
                        .columns
                        .button(column.id, icon.panel)
                        .unwrap()
                        .compute_bounds(&w.surface)
                        .unwrap();
                    assert!(
                        (tile.y() - icon.bounds.y).abs() <= 1.,
                        "tile geometry follows shared top padding"
                    );
                }
            }
            for group in &member.groups {
                let shared = r.groups.iter().find(|g| g.id == group.group).unwrap();
                let views = w.groups.borrow();
                let native = views.iter().find(|v| v.id == group.group).unwrap();
                let b = native.root.compute_bounds(&w.surface).unwrap();
                assert!(
                    (b.x() - shared.bounds.x).abs() <= 1. && (b.y() - shared.bounds.y).abs() <= 1.
                );
                assert!(
                    (b.width() - shared.bounds.width).abs() <= 1.
                        && (b.height() - shared.bounds.height).abs() <= 1.
                );
                let button = w.columns.button(4, group.active).unwrap();
                assert!(
                    button.has_css_class("selected-tool"),
                    "all active tabs highlight their sidebar tiles"
                );
            }
            assert!(open.connections.len() >= 2);
            capture_reference(
                &w,
                output
                    .join(format!("column-stack-{theme:?}-{touch}.png"))
                    .to_str()
                    .unwrap(),
                1.,
            );
            // Width and internal split gestures retain ordinary GTK group widgets.
            let ids: Vec<_> = r
                .dividers
                .iter()
                .filter(|d| r.open_column_at_divider(d.id) == Some(4) || d.id == 4)
                .map(|d| d.id)
                .collect();
            assert_eq!(ids.len(), 2);
            for id in ids {
                eprintln!("Column resize {theme:?}, touch={touch}, divider={id}");
                let r = w.resolved();
                let divider = r.dividers.iter().find(|d| d.id == id).unwrap();
                let start = center(divider.bounds);
                let axis = usize::from(divider.axis == Axis::Vertical);
                let mut moved = start;
                moved[axis] += 45.;
                let root = w
                    .groups
                    .borrow()
                    .iter()
                    .find(|g| g.id == 5)
                    .unwrap()
                    .root
                    .clone();
                perform(serde_json::json!([
                    event("down", start),
                    event("move", moved)
                ]));
                let revision = w.publication.content_revision.get();
                let refreshes = w.publication.refreshes.get();
                let samples = Rc::new(RefCell::new(Vec::new()));
                let missing = Rc::new(RefCell::new(None));
                let clock = w.surface.frame_clock().unwrap();
                let painted = clock.connect_after_paint(glib::clone!(
                    #[strong]
                    samples,
                    #[strong]
                    root,
                    #[strong]
                    missing,
                    #[weak]
                    w,
                    move |clock| {
                        let resolved = w.resolved();
                        let Some(group) = resolved.groups.iter().find(|g| g.id == 5) else {
                            *missing.borrow_mut() = Some(format!(
                                "native={:?}; state={:?}; open={:?}",
                                w.surface.imp().layout.borrow().collapsed,
                                state(&w).workspace.layout.collapsed,
                                state(&w).workspace.layout.column_stacks
                            ));
                            return;
                        };
                        let shared = group.bounds;
                        let actual = root.compute_bounds(&w.surface).unwrap();
                        let size = if axis == 0 {
                            actual.width()
                        } else {
                            actual.height()
                        };
                        let expected = if axis == 0 {
                            shared.width
                        } else {
                            shared.height
                        };
                        samples.borrow_mut().push((
                            clock.frame_time(),
                            size,
                            (size - expected).abs(),
                        ));
                    }
                ));
                let events: Vec<_> = (0..120)
                    .map(|i| {
                        let mut point = moved;
                        point[axis] += (i as f32 * 0.12).sin() * 35.;
                        event("move", point)
                    })
                    .collect();
                perform(serde_json::to_value(events).unwrap());
                clock.disconnect(painted);
                assert!(
                    missing.borrow().is_none(),
                    "column vanished while resizing: {:?}",
                    missing.borrow()
                );
                let samples = samples.borrow();
                let changing: Vec<_> = samples
                    .windows(2)
                    .filter(|p| p[0].1 != p[1].1)
                    .map(|p| p[1].0)
                    .collect();
                assert!(
                    changing.len() > 20,
                    "ordinary dock contents must resize continuously"
                );
                let error = samples.iter().map(|s| s.2).fold(0., f32::max);
                assert!(
                    error <= 1.,
                    "painted child allocation differs from shared geometry: {error}"
                );
                let report = serde_json::json!({"theme":format!("{theme:?}"),"touch":touch,"axis":axis,
                    "changed_frames":changing.len(), "max_error_px":error,
                    "geometry_hz":(changing.len()-1) as f64 * 1e6 / (changing.last().unwrap()-changing[0]) as f64});
                std::fs::write(
                    output.join(format!("resize-{theme:?}-{touch}-{axis}.json")),
                    serde_json::to_vec_pretty(&report).unwrap(),
                )
                .unwrap();
                assert_eq!(w.publication.content_revision.get(), revision);
                assert_eq!(w.publication.refreshes.get(), refreshes);
                assert_eq!(
                    w.groups.borrow().iter().find(|g| g.id == 5).unwrap().root,
                    root
                );
                perform(serde_json::json!([
                    event("move", moved),
                    event("up", moved)
                ]));
                let shared = w
                    .resolved()
                    .groups
                    .into_iter()
                    .find(|g| g.id == 5)
                    .unwrap()
                    .bounds;
                let actual = root.compute_bounds(&w.surface).unwrap();
                assert!(
                    (actual.width() - shared.width).abs() <= 1.
                        && (actual.height() - shared.height).abs() <= 1.
                );
            }
            perform(click(center(icon(Panel::Layers))));
            assert_eq!(
                state(&w).workspace.layout.column_stack(4).open_column,
                Some(8)
            );
            let resolved = w.resolved();
            let divider = resolved
                .dividers
                .iter()
                .find(|d| resolved.open_column_at_divider(d.id) == Some(8))
                .unwrap();
            let open = resolved
                .collapsed
                .iter()
                .find(|c| c.id == 8)
                .unwrap()
                .open
                .as_ref()
                .unwrap();
            let start = center(divider.bounds);
            let inward = if open.direction == Edge::Right {
                -600.
            } else {
                600.
            };
            let moved = [(start[0] + inward).clamp(2., viewport[0] - 2.), start[1]];
            perform(serde_json::json!([
                event("down", start),
                event("move", moved),
                event("up", moved)
            ]));
            let resolved = w.resolved();
            let open = resolved
                .collapsed
                .iter()
                .find(|c| c.id == 8)
                .unwrap()
                .open
                .as_ref()
                .unwrap();
            assert!(open.bounds.width >= layer_ui::LAYERS_MIN_WIDTH);
            assert_eq!(state(&w).workspace.layout.column_stack(4).members, [4, 8]);
            let group = resolved
                .groups
                .iter()
                .find(|g| g.panels.contains(&Panel::Layers))
                .unwrap();
            let root = w
                .groups
                .borrow()
                .iter()
                .find(|g| g.id == group.id)
                .unwrap()
                .root
                .clone();
            let actual = root.compute_bounds(&w.surface).unwrap();
            assert!((actual.width() - group.bounds.width).abs() <= 1.);
            assert_eq!(
                w.resolved()
                    .collapsed
                    .iter()
                    .filter(|c| c.open.is_some())
                    .count(),
                1
            );
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetColumnAutoHide {
                    column: 8,
                    auto_hide: true,
                },
            });
            pump(160);
            perform(click([viewport[0] * 0.5, viewport[1] * 0.5]));
            assert!(
                state(&w)
                    .workspace
                    .layout
                    .column_stack(4)
                    .open_column
                    .is_none()
            );
            w.dispatch(UiAction::Customize {
                action: CustomizationAction::SetColumnDrawers {
                    column: 8,
                    drawers: true,
                },
            });
            pump(160);
            perform(click(center(icon(Panel::Layers))));
            assert!(state(&w).customization.column_drawers[0].tabs.is_some());
            perform(serde_json::json!([{"key":65307,"down":true},{"key":65307,"down":false}]));
            assert!(state(&w).customization.column_drawers.is_empty());
            // Pulling a member to the opposite side makes it a singleton stack.
            let r = w.resolved();
            let source = center(r.collapsed.iter().find(|c| c.id == 8).unwrap().grip);
            let target = [
                if edge == Edge::Left {
                    viewport[0] - 2.
                } else {
                    2.
                },
                viewport[1] * 0.5,
            ];
            perform(serde_json::json!([
                event("down", source),
                event("move", target),
                event("up", target)
            ]));
            assert_eq!(state(&w).workspace.layout.column_stack(8).members, [8]);
            assert_eq!(state(&w).workspace.layout.column_stack(4).members, [4]);
        }
    }
}
