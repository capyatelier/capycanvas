use super::*;
use crate::{Platform, ToolbarTile, WorkspacePreset};

const VIEWPORTS: [[f32; 2]; 4] = [[412., 891.], [891., 412.], [280., 560.], [560., 280.]];

fn resolve(layout: &DockLayout, viewport: [f32; 2], swapped: bool) -> PhoneLayout {
    PhoneLayout::resolve(layout, viewport, [0.; 4], [0.; 2], [0; 2], swapped, false).unwrap()
}

fn center(bounds: Bounds) -> [f32; 2] { [bounds.x + bounds.width * 0.5, bounds.y + bounds.height * 0.5] }

fn begin(phone: &PhoneLayout, tiles: &[ToolbarTile], point: [f32; 2]) -> PhoneBarContact {
    let mut contact = PhoneBarContact::begin(phone, point, false).unwrap();
    contact.set_tools(tiles);
    contact
}

fn near(actual: f32, expected: f32) { assert!((actual - expected).abs() < 0.001, "{actual} != {expected}"); }

#[test]
fn phone_tile_contact_distinguishes_tap_quick_drag_and_hold_release() {
    for preset in WorkspacePreset::ALL {
        let layout = preset.layout(Platform::Gtk);
        let tiles = layout.phone.as_ref().unwrap();
        for viewport in VIEWPORTS {
            for swapped in [false, true] {
                let phone = resolve(&layout, viewport, swapped);
                let origin = center(phone.tools[2]);
                let mut tap = begin(&phone, tiles, origin);
                let slight = origin.map(|value| value + 1.);
                assert_eq!(tap.motion(&phone, tiles, slight, false), PhoneBarOutcome::Pending);
                assert_eq!(tap.finish(&phone, tiles, slight), PhoneBarOutcome::Tap(tiles[2].id));
                let mut quick = begin(&phone, tiles, origin);
                assert_eq!(quick.motion(&phone, tiles, center(phone.tools[4]), true), PhoneBarOutcome::WindowMove);
                let mut held = begin(&phone, tiles, origin);
                assert_eq!(held.arm(), PhoneBarOutcome::Held);
                assert_eq!(held.lifted(), Some(2));
                assert!(!held.move_window(true));
                let lifted = held.geometry(&phone);
                assert!(lifted[2].width > phone.tools[2].width && lifted[2].height > phone.tools[2].height);
                assert!(lifted.iter().zip(phone.tools).enumerate().all(|(index, (shown, original))| index == 2 || *shown == original));
                assert_eq!(held.motion(&phone, tiles, slight, false), PhoneBarOutcome::Held);
                assert_eq!(held.finish(&phone, tiles, slight), PhoneBarOutcome::Menu(Some(tiles[2].id)));
            }
        }
    }
}

#[test]
fn phone_title_contacts_move_the_window_without_a_tile_and_cancel_when_not_movable() {
    let layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    let tiles = layout.phone.as_ref().unwrap();
    for viewport in [[891., 412.], [560., 280.]] {
        for swapped in [false, true] {
            for hidden in [false, true] {
                let state = PhoneState { platform: Some(Platform::Gtk), swapped, hidden, movable: true, ..Default::default() };
                let phone = state.resolve(&layout, viewport).unwrap();
                let origin = center(phone.title);
                let mut tap = begin(&phone, tiles, origin);
                assert!(!tap.editable && tap.index.is_none());
                let slight = [origin[0] + 1., origin[1]];
                assert_eq!(tap.motion(&phone, tiles, slight, false), PhoneBarOutcome::Pending);
                assert_eq!(tap.finish(&phone, tiles, slight), PhoneBarOutcome::Pending);
                let mut drag = begin(&phone, tiles, origin);
                let moved = [origin[0] + 20., origin[1]];
                assert_eq!(drag.motion(&phone, tiles, moved, true), PhoneBarOutcome::WindowMove);
                let mut held = begin(&phone, tiles, origin);
                assert_eq!(held.arm(), PhoneBarOutcome::Pending);
                assert_eq!(held.lifted(), None);
                assert_eq!(held.finish(&phone, tiles, origin), PhoneBarOutcome::Pending);
                let fixed = PhoneState { movable: false, ..state }.resolve(&layout, viewport).unwrap();
                assert_eq!(fixed.bars, phone.bars);
                assert_eq!(fixed.work_area, phone.work_area);
                assert!(PhoneBarContact::begin(&fixed, origin, false).is_none());
                assert!(!drag.valid(&fixed, tiles));
                assert_eq!(drag.motion(&fixed, tiles, moved, true), PhoneBarOutcome::Cancel);
                assert_eq!(drag.finish(&fixed, tiles, moved), PhoneBarOutcome::Cancel);
            }
        }
    }
}

#[test]
fn held_phone_tile_moves_in_the_drop_band_and_cancels_outside_it() {
    let layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    let tiles = layout.phone.as_ref().unwrap();
    for viewport in VIEWPORTS {
        for swapped in [false, true] {
            let phone = resolve(&layout, viewport, swapped);
            let cross = usize::from(phone.edge == Edge::Top);
            let bar = phone.bars[0];
            for inside in [false, true] {
                let mut contact = begin(&phone, tiles, center(phone.tools[0]));
                assert_eq!(contact.arm(), PhoneBarOutcome::Held);
                let mut point = center(phone.tools[3]);
                point[cross] = if cross == 0 { bar.x } else { bar.y } - phone.tile + if inside { 1. } else { -1. };
                assert_eq!(contact.motion(&phone, tiles, point, true), PhoneBarOutcome::Dragging);
                assert_eq!(contact.finish(&phone, tiles, point), if inside {
                    PhoneBarOutcome::Move { tile: tiles[0].id, before: Some(tiles[4].id) }
                } else { PhoneBarOutcome::Cancel });
            }
            let mut cancelled = begin(&phone, tiles, center(phone.tools[0]));
            cancelled.arm();
            cancelled.motion(&phone, tiles, center(phone.tools[3]), true);
            assert_eq!(cancelled.cancel(), PhoneBarOutcome::Cancel);
            assert!(!cancelled.valid(&phone, tiles));
            assert_eq!(cancelled.finish(&phone, tiles, center(phone.tools[3])), PhoneBarOutcome::Cancel);
        }
    }
}

#[test]
fn held_phone_contacts_cancel_after_rotation_resize_or_toolbar_identity_change() {
    let layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    let tiles = layout.phone.as_ref().unwrap();
    let phone = resolve(&layout, VIEWPORTS[0], false);
    let mut contact = begin(&phone, tiles, center(phone.tools[0]));
    contact.arm();
    contact.motion(&phone, tiles, center(phone.tools[3]), true);
    for viewport in [[891., 412.], [413., 891.], [412., 892.]] {
        let changed = resolve(&layout, viewport, false);
        let mut active = contact;
        assert!(!active.valid(&changed, tiles));
        assert_eq!(active.motion(&changed, tiles, center(changed.tools[3]), true), PhoneBarOutcome::Cancel);
        assert_eq!(active.finish(&changed, tiles, center(changed.tools[3])), PhoneBarOutcome::Cancel);
    }
    let mut changed = tiles.clone();
    changed.swap(0, 1);
    assert!(!contact.valid(&phone, &changed));
    assert_eq!(contact.finish(&phone, &changed, center(phone.tools[3])), PhoneBarOutcome::Cancel);
    assert!(PhoneLayout::resolve(&layout, [480., 891.], [0.; 4], [0.; 2], [0; 2], false, false).is_none());
}

#[test]
fn empty_bar_opens_toolbar_menu_while_capy_and_settings_keep_their_native_actions() {
    let mut layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    for tile in layout.phone.clone().unwrap().into_iter().skip(3) { layout.remove_phone_tool(tile.id).unwrap(); }
    let tiles = layout.phone.as_ref().unwrap();
    for viewport in VIEWPORTS {
        for swapped in [false, true] {
            let phone = resolve(&layout, viewport, swapped);
            let mut empty = center(phone.bars[0]);
            let axis = usize::from(phone.edge != Edge::Top);
            empty[axis] = if axis == 0 { phone.bars[0].x + phone.bars[0].width } else { phone.bars[0].y + phone.bars[0].height } - 1.;
            let mut contact = begin(&phone, tiles, empty);
            contact.arm();
            assert_eq!(contact.finish(&phone, tiles, empty), PhoneBarOutcome::Menu(None));
            for bounds in [phone.capy, phone.settings] {
                let origin = center(bounds);
                let mut contact = begin(&phone, tiles, origin);
                assert_eq!(contact.arm(), PhoneBarOutcome::Pending);
                assert_eq!(contact.motion(&phone, tiles, origin.map(|value| value + 2.), true), PhoneBarOutcome::Pending);
                assert_eq!(contact.geometry(&phone), phone.tools);
                assert_eq!(contact.finish(&phone, tiles, origin), PhoneBarOutcome::Pending);
            }
        }
    }
}

#[test]
fn phone_divider_hold_targets_the_existing_divider_identity() {
    let mut layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    let before = layout.phone.as_ref().unwrap()[2].id;
    layout.insert_phone_tools(Some(before), &[ToolbarControl::Divider]).unwrap();
    let tiles = layout.phone.as_ref().unwrap();
    let index = tiles.iter().position(|tile| tile.control == ToolbarControl::Divider).unwrap();
    for viewport in VIEWPORTS {
        for swapped in [false, true] {
            let phone = resolve(&layout, viewport, swapped);
            let point = center(phone.tools[index]);
            let mut contact = begin(&phone, tiles, point);
            assert_eq!(contact.arm(), PhoneBarOutcome::Held);
            assert_eq!(contact.finish(&phone, tiles, point), PhoneBarOutcome::Menu(Some(tiles[index].id)));
        }
    }
}

#[test]
fn phone_drag_preview_follows_the_bar_axis_and_the_committed_insertion_slot() {
    let mut layout = WorkspacePreset::Painter.layout(Platform::Gtk);
    let divider_before = layout.phone.as_ref().unwrap()[2].id;
    layout.insert_phone_tools(Some(divider_before), &[ToolbarControl::Divider]).unwrap();
    let tiles = layout.phone.as_ref().unwrap();
    for viewport in VIEWPORTS {
        for swapped in [false, true] {
            let phone = resolve(&layout, viewport, swapped);
            let axis = usize::from(phone.edge != Edge::Top);
            for (source, before) in [(0, Some(tiles[4].id)), (0, None), (tiles.len() - 1, Some(tiles[1].id))] {
                let mut contact = begin(&phone, tiles, center(phone.tools[source]));
                contact.arm();
                let target = before.and_then(|id| tiles.iter().position(|tile| tile.id == id));
                let mut point = center(phone.bars[0]);
                point[axis] = if let Some(target) = target {
                    if axis == 0 { phone.tools[target].x + 1. } else { phone.tools[target].y + 1. }
                } else if axis == 0 { phone.bars[0].x + phone.bars[0].width } else { phone.bars[0].y + phone.bars[0].height };
                point[1 - axis] += phone.tile * 0.25;
                assert_eq!(contact.motion(&phone, tiles, point, true), PhoneBarOutcome::Dragging);
                let geometry = contact.geometry(&phone);
                near(center(geometry[source])[axis], point[axis]);
                near(center(geometry[source])[1 - axis], center(phone.tools[source])[1 - axis]);
                point[1 - axis] -= phone.tile * 0.5;
                contact.motion(&phone, tiles, point, true);
                assert_eq!(contact.geometry(&phone), geometry, "perpendicular motion does not move the pickup or insertion feedback");
                assert_eq!(contact.finish(&phone, tiles, point), PhoneBarOutcome::Move { tile: tiles[source].id, before });
                let mut moved = layout.clone();
                moved.move_phone_tool(tiles[source].id, before).unwrap();
                let expected = resolve(&moved, viewport, swapped);
                for (index, tile) in tiles.iter().enumerate().filter(|(index, _)| *index != source) {
                    let slot = moved.phone.as_ref().unwrap().iter().position(|shown| shown.id == tile.id).unwrap();
                    let (actual, expected) = (geometry[index], expected.tools[slot]);
                    for (actual, expected) in [actual.x, actual.y, actual.width, actual.height].into_iter()
                        .zip([expected.x, expected.y, expected.width, expected.height]) { near(actual, expected); }
                }
            }
        }
    }
}
