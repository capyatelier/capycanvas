//! Drawing identities/order are independent of native tab widgets and renderers.
#[derive(Debug)]
pub struct DocumentTabs {
    order: Vec<u64>,
    selected: u64,
    next: u64,
    undo: Vec<Vec<u64>>,
    redo: Vec<Vec<u64>>,
}
impl Default for DocumentTabs {
    fn default() -> Self {
        Self {
            order: vec![1],
            selected: 1,
            next: 2,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}
impl DocumentTabs {
    pub fn order(&self) -> &[u64] {
        &self.order
    }
    pub fn selected(&self) -> u64 {
        self.selected
    }
    pub fn add(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        self.order.push(id);
        self.selected = id;
        self.undo.clear();
        self.redo.clear();
        id
    }
    pub fn select(&mut self, id: u64) -> bool {
        if !self.order.contains(&id) {
            return false;
        }
        self.selected = id;
        true
    }
    pub fn adjacent(&self, forward: bool) -> Option<u64> {
        let index = self.order.iter().position(|id| *id == self.selected)?;
        Some(
            self.order[(index + if forward { 1 } else { self.order.len() - 1 }) % self.order.len()],
        )
    }
    pub fn close(&mut self, id: u64) -> bool {
        let Some(index) = self.order.iter().position(|v| *v == id) else {
            return false;
        };
        self.order.remove(index);
        if id == self.selected {
            self.selected = self
                .order
                .get(index)
                .or_else(|| self.order.last())
                .copied()
                .unwrap_or(0);
        }
        // Opening/closing changes membership; order undo must never resurrect a
        // discarded document or silently remove a newly opened drawing.
        self.undo.clear();
        self.redo.clear();
        true
    }
    pub fn after_close(&self) -> Option<u64> {
        let i = self.order.iter().position(|&id| id == self.selected)?;
        self.order
            .get(i + 1)
            .or_else(|| i.checked_sub(1).and_then(|i| self.order.get(i)))
            .copied()
    }
    pub fn reorder(&mut self, id: u64, before: Option<u64>) -> bool {
        if !self.order.contains(&id) || before.is_some_and(|v| !self.order.contains(&v) || v == id)
        {
            return false;
        }
        let mut order = self.order.clone();
        order.retain(|v| *v != id);
        let index = before
            .and_then(|v| order.iter().position(|i| *i == v))
            .unwrap_or(order.len());
        order.insert(index, id);
        if order == self.order {
            return false;
        }
        self.undo.push(std::mem::replace(&mut self.order, order));
        if self.undo.len() > 64 {
            self.undo.remove(0);
        }
        self.redo.clear();
        true
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo(&mut self) {
        if let Some(order) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.order, order));
        }
    }
    pub fn redo(&mut self) {
        if let Some(order) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.order, order));
        }
    }
    pub fn compact(width: f32, count: usize) -> bool {
        !width.is_finite() || width < count.max(1) as f32 * 140.
    }
    pub fn step(&self, id: u64, forward: bool) -> Option<Option<u64>> {
        let i = self.order.iter().position(|&v| v == id)?;
        if forward {
            (i + 1 < self.order.len()).then(|| self.order.get(i + 2).copied())
        } else {
            i.checked_sub(1).map(|i| Some(self.order[i]))
        }
    }
    /// Only measured, visible members accept a drop. Out-of-strip movement is
    /// cancellation, never an implicit move-to-end or a window tear-off.
    pub fn drop_target(
        &self,
        hits: &[DocumentTabHit],
        point: [f32; 2],
        vertical: bool,
    ) -> Option<Option<u64>> {
        if !point.iter().all(|v| v.is_finite()) {
            return None;
        }
        let hit = hits
            .iter()
            .find(|h| self.order.contains(&h.id) && h.bounds.contains(point[0], point[1]))?;
        let before = if vertical {
            point[1] < hit.bounds.y + hit.bounds.height / 2.
        } else {
            point[0] < hit.bounds.x + hit.bounds.width / 2.
        };
        if before {
            Some(Some(hit.id))
        } else {
            let i = self.order.iter().position(|&id| id == hit.id)?;
            Some(self.order.get(i + 1).copied())
        }
    }
}

#[derive(serde::Deserialize)]
pub struct DocumentTabHit {
    pub id: u64,
    pub bounds: crate::Bounds,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn order_history_never_changes_selection_or_resurrects_closed_drawings() {
        let mut tabs = DocumentTabs::default();
        let b = tabs.add();
        let c = tabs.add();
        assert!(tabs.reorder(c, Some(1)));
        assert_eq!(tabs.order(), &[c, 1, b]);
        tabs.undo();
        assert_eq!(tabs.order(), &[1, b, c]);
        assert_eq!(tabs.selected(), c);
        tabs.redo();
        assert_eq!(tabs.order(), &[c, 1, b]);
        assert!(!tabs.reorder(55, None));
        assert!(!tabs.reorder(1, Some(55)));
        assert!(tabs.close(c));
        assert_eq!(tabs.selected(), 1);
        assert!(!tabs.can_undo());
        tabs.close(1);
        assert_eq!(tabs.selected(), b);
        tabs.close(b);
        assert!(tabs.order().is_empty());
        assert_eq!(tabs.adjacent(true), None);
    }
    #[test]
    fn width_and_cycle_boundaries() {
        assert!(!DocumentTabs::compact(420., 3));
        assert!(DocumentTabs::compact(419., 3));
        assert!(DocumentTabs::compact(f32::NAN, 3));
        let mut tabs = DocumentTabs::default();
        let b = tabs.add();
        assert_eq!(tabs.adjacent(true), Some(1));
        tabs.select(1);
        assert_eq!(tabs.adjacent(false), Some(b));
    }
    #[test]
    fn flexible_header_respects_neighbors_and_native_buttons_in_every_zone() {
        use crate::*;
        for zone in HeaderZone::ALL {
            let mut layout = HeaderLayout::default();
            let title = layout
                .entries()
                .find(|e| e.item == HeaderItem::DocumentTitle)
                .unwrap()
                .id;
            // Resolve default and custom placements using the real allocator.
            if zone != HeaderZone::Center {
                layout.zones[1].retain(|e| e.id != title);
                layout.zones[zone.index()].push(HeaderEntry {
                    id: title,
                    item: HeaderItem::DocumentTitle,
                });
            }
            let metrics: Vec<_> = layout
                .entries()
                .map(|e| HeaderMetric {
                    id: e.id,
                    width: 80.,
                    compact: 60.,
                })
                .collect();
            let geometry = layout.resolve_documents(1400., [48., 100.], &metrics, false, 3);
            let title_bounds = geometry
                .items
                .iter()
                .find(|i| i.id == title)
                .unwrap()
                .bounds;
            assert!(title_bounds.width > 80.);
            assert!(title_bounds.x >= 48.);
            assert!(title_bounds.x + title_bounds.width <= 1300.);
            for item in geometry.items.iter().filter(|i| i.id != title) {
                assert!(
                    title_bounds.x + title_bounds.width + 11.9 <= item.bounds.x
                        || item.bounds.x + item.bounds.width + 11.9 <= title_bounds.x
                );
            }
        }
    }
}
