//! Stable-ID palette moves. Native hosts supply a grid slot; shared
//! Rust validates the insertion and records one reversible move per drop.
use super::*;

/// A reversible visual proposal; producing it never edits colors or history.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorReorderPreview {
    pub order: Vec<u64>,
    pub action: Option<ColorLibraryAction>,
}

#[derive(Clone, Debug, PartialEq)]
struct Move {
    palette: u64,
    id: u64,
    before: Option<u64>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct ReorderHistory {
    undo: Vec<Move>,
    redo: Vec<Move>,
}
impl ColorLibrary {
    /// Native grids supply a stable cell index, independent of animated tiles.
    /// The trailing add cell inserts at the end; the original slot is a no-op.
    pub fn preview_reorder(
        &self,
        palette: u64,
        id: u64,
        slot: usize,
    ) -> Option<ColorReorderPreview> {
        let colors = &self.palettes.iter().find(|p| p.id == palette)?.swatches;
        let source = colors.iter().position(|c| c.id == id)?;
        if slot > colors.len() {
            return None;
        }
        let target = slot.min(colors.len() - 1);
        let mut order: Vec<_> = colors.iter().map(|c| c.id).collect();
        order.remove(source);
        let action = (source != target).then(|| ColorLibraryAction::Reorder {
            palette,
            id,
            before: order.get(target).copied(),
        });
        order.insert(target, id);
        Some(ColorReorderPreview { order, action })
    }

    pub fn can_undo_reorder(&self, palette: u64, redo: bool) -> bool {
        let steps = if redo {
            &self.reorders.redo
        } else {
            &self.reorders.undo
        };
        steps.iter().any(|m| m.palette == palette)
    }
    fn move_swatch(&mut self, movement: Move) -> Result<Option<Move>, String> {
        let colors = &mut self
            .palettes
            .iter_mut()
            .find(|p| p.id == movement.palette)
            .ok_or("Palette no longer exists")?
            .swatches;
        let source = colors
            .iter()
            .position(|c| c.id == movement.id)
            .ok_or("Swatch no longer exists")?;
        let slot = match movement.before {
            Some(id) => colors
                .iter()
                .position(|c| c.id == id)
                .ok_or("Drop target no longer exists")?,
            None => colors.len(),
        };
        let target = slot - usize::from(source < slot);
        if source == target {
            return Ok(None);
        }
        let inverse = Move {
            before: colors.get(source + 1).map(|c| c.id),
            ..movement
        };
        let swatch = colors.remove(source);
        colors.insert(target, swatch);
        Ok(Some(inverse))
    }
    pub(super) fn reorder(
        &mut self,
        palette: u64,
        id: u64,
        before: Option<u64>,
    ) -> Result<(), String> {
        if let Some(inverse) = self.move_swatch(Move {
            palette,
            id,
            before,
        })? {
            self.reorders.redo.retain(|m| m.palette != palette);
            if self.reorders.undo.len() == 64 {
                self.reorders.undo.remove(0);
            }
            self.reorders.undo.push(inverse);
        }
        Ok(())
    }
    pub(super) fn restore_reorder(&mut self, palette: u64, redo: bool) -> Result<(), String> {
        let steps = if redo {
            &self.reorders.redo
        } else {
            &self.reorders.undo
        };
        let index = steps
            .iter()
            .rposition(|m| m.palette == palette)
            .ok_or("No color reorder to undo or redo")?;
        let movement = steps[index].clone();
        let inverse = self.move_swatch(movement)?;
        let (from, to) = if redo {
            (&mut self.reorders.redo, &mut self.reorders.undo)
        } else {
            (&mut self.reorders.undo, &mut self.reorders.redo)
        };
        from.remove(index);
        if let Some(inverse) = inverse {
            to.push(inverse);
        }
        Ok(())
    }
    pub(super) fn forget_reorders(&mut self, palette: u64) {
        self.reorders.undo.retain(|m| m.palette != palette);
        self.reorders.redo.retain(|m| m.palette != palette);
    }
}

#[test]
fn reorders_are_atomic_reversible_and_preserve_color_identity() {
    let mut library = ColorLibrary::default();
    for name in ["One", "Two", "Three", "Four"] {
        library
            .apply(ColorLibraryAction::Store {
                palette: 1,
                name: name.into(),
                color: RgbColor::BLACK,
            })
            .unwrap();
    }
    let original = library.palettes[0].swatches.clone();
    let ids: Vec<_> = original.iter().map(|c| c.id).collect();
    let before = library.clone();
    let unchanged = library.preview_reorder(1, ids[0], 0).unwrap();
    assert_eq!(unchanged.order, ids);
    assert!(unchanged.action.is_none());
    assert!(library.preview_reorder(1, ids[0], 999).is_none());
    assert!(library.preview_reorder(999, ids[0], 0).is_none());
    assert!(library.preview_reorder(1, 999, 0).is_none());
    let preview = library.preview_reorder(1, ids[0], ids.len()).unwrap();
    assert_eq!(preview.order, [ids[1], ids[2], ids[3], ids[0]]);
    assert_eq!(
        library, before,
        "preview does not edit the library or history"
    );
    let action = preview.action.unwrap();
    library.apply(action).unwrap();
    assert_eq!(
        library.palettes[0]
            .swatches
            .iter()
            .map(|c| c.id)
            .collect::<Vec<_>>(),
        [ids[1], ids[2], ids[3], ids[0]]
    );
    assert!(library.history.is_empty());
    library
        .apply(ColorLibraryAction::UndoReorder { palette: 1 })
        .unwrap();
    assert_eq!(library.palettes[0].swatches, original);
    assert!(!library.can_undo_reorder(1, false));
    library
        .apply(ColorLibraryAction::RedoReorder { palette: 1 })
        .unwrap();
    let before = library.clone();
    assert!(
        library
            .apply(ColorLibraryAction::Reorder {
                palette: 1,
                id: ids[0],
                before: Some(999)
            })
            .is_err()
    );
    assert_eq!(library, before);
    let restored: ColorLibrary =
        serde_json::from_slice(&serde_json::to_vec(&library).unwrap()).unwrap();
    assert_eq!(restored.palettes, library.palettes);
    assert!(!restored.can_undo_reorder(1, false));
    library
        .apply(ColorLibraryAction::Remove { id: ids[2] })
        .unwrap();
    assert!(!library.can_undo_reorder(1, false));
}

#[test]
fn previews_match_committed_order_in_both_directions() {
    let mut library = ColorLibrary::default();
    for name in ["A", "B", "C", "D", "E"] {
        library
            .apply(ColorLibraryAction::Store {
                palette: 1,
                name: name.into(),
                color: RgbColor::BLACK,
            })
            .unwrap();
    }
    for source in 0..5 {
        for slot in 0..=5 {
            let id = library.palettes[0].swatches[source].id;
            let preview = library.preview_reorder(1, id, slot).unwrap();
            let mut dropped = library.clone();
            if let Some(action) = preview.action {
                dropped.apply(action).unwrap();
            }
            assert_eq!(
                dropped.palettes[0]
                    .swatches
                    .iter()
                    .map(|c| c.id)
                    .collect::<Vec<_>>(),
                preview.order
            );
            assert_eq!(preview.order[slot.min(4)], id);
            assert!(dropped.history.is_empty());
        }
    }
}
