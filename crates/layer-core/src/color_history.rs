//! Prepare color history without publishing it, then pair it with host resources.
use super::*;

pub enum ColorTransition {
    Apply {
        color: color::DocumentColor,
        layers: Vec<Layer>,
    },
    Undo,
    Redo,
}

enum Direction {
    Apply,
    Undo,
    Redo,
}

/// A candidate for resource preparation and complete-stack preview. It retains
/// immutable backing, not a copy of the editor's history or decoded image.
pub struct PreparedColorTransition {
    before: Document,
    before_history: (u64, usize, usize),
    document: Document,
    inverse: HistoryEntry,
    direction: Direction,
}
impl PreparedColorTransition {
    pub fn document(&self) -> &Document {
        &self.document
    }
}

impl Editor {
    pub fn prepare_color_transition(
        &self,
        transition: ColorTransition,
    ) -> Result<PreparedColorTransition, DocumentError> {
        let (document, inverse, direction) = match transition {
            ColorTransition::Apply { color, layers } => {
                let (document, inverse) = self.prepare_history_edit(
                    Edit::SetColor { color, layers },
                    history_budget::BYTE_BUDGET,
                )?;
                (document, inverse, Direction::Apply)
            }
            ColorTransition::Undo | ColorTransition::Redo => {
                let undo = matches!(transition, ColorTransition::Undo);
                let history = if undo { &self.undo } else { &self.redo };
                let entry = history.last().ok_or(DocumentError::InvalidLayerOperation(
                    "No color history remains",
                ))?;
                let mut candidate = self.document.clone();
                let inverse =
                    HistoryEntry::new(candidate.apply(entry.edit.clone())?, self.checkpoint);
                (
                    candidate,
                    inverse,
                    if undo {
                        Direction::Undo
                    } else {
                        Direction::Redo
                    },
                )
            }
        };
        if document.color == self.document.color {
            return Err(DocumentError::InvalidLayerOperation(
                "This transition does not change document color",
            ));
        }
        Ok(PreparedColorTransition {
            before: self.document.clone(),
            before_history: (self.checkpoint, self.undo.len(), self.redo.len()),
            document,
            inverse,
            direction,
        })
    }

    /// The final fallible resource-adoption step runs after stale-state checks
    /// and before publication. An error must leave host resources unchanged.
    /// After success, model/history publication has no further fallible steps.
    pub fn commit_color_transition<E: From<DocumentError>>(
        &mut self,
        prepared: PreparedColorTransition,
        adopt: impl FnOnce(&Document) -> Result<(), E>,
    ) -> Result<(), E> {
        if self.document != prepared.before
            || (self.checkpoint, self.undo.len(), self.redo.len()) != prepared.before_history
        {
            return Err(DocumentError::InvalidLayerOperation(
                "The document or history changed during color preparation",
            )
            .into());
        }
        let next_checkpoint = if matches!(prepared.direction, Direction::Apply) {
            Some(self.next_checkpoint.checked_add(1).ok_or(
                DocumentError::InvalidLayerOperation("Document history exhausted"),
            )?)
        } else {
            None
        };
        adopt(&prepared.document)?;
        self.document = prepared.document;
        match prepared.direction {
            Direction::Apply => {
                self.undo.push(prepared.inverse);
                self.redo.clear();
                self.checkpoint = self.next_checkpoint;
                self.next_checkpoint = next_checkpoint.unwrap();
                self.trim_history(history_budget::BYTE_BUDGET);
            }
            Direction::Undo => {
                self.checkpoint = self.undo.pop().unwrap().checkpoint;
                self.redo.push(prepared.inverse);
            }
            Direction::Redo => {
                self.checkpoint = self.redo.pop().unwrap().checkpoint;
                self.undo.push(prepared.inverse);
            }
        }
        Ok(())
    }
}
