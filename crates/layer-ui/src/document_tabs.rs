//! Drawing identities/order are independent of native tab widgets and renderers.
pub(crate) fn drawing_after_close(order: impl IntoIterator<Item = u64>, selected: u64) -> Option<u64> {
    let mut order = order.into_iter();
    let mut previous = None;
    while let Some(id) = order.next() {
        if id == selected { return order.next().or(previous); }
        previous = Some(id);
    }
    None
}

#[derive(Clone, Debug)]
pub struct DocumentTabs {
    order: Vec<u64>,
    selected: u64,
    next: u64,
    undo: Vec<Vec<u64>>,
    redo: Vec<Vec<u64>>,
    reserved: std::collections::BTreeSet<u64>,
}
impl Default for DocumentTabs {
    fn default() -> Self {
        Self {
            order: vec![1],
            selected: 1,
            next: 2,
            undo: Vec::new(),
            redo: Vec::new(),
            reserved: Default::default(),
        }
    }
}
impl DocumentTabs {
    pub(crate) fn reserve_identities(&mut self, ids: &[u64]) -> Result<(), String> {
        let ids: std::collections::BTreeSet<_> = ids.iter().copied().collect();
        if ids.iter().any(|id| *id==0 || *id>crate::session_recovery::MAX_SESSION_DRAWING_ID) {
            return Err("Invalid restored drawing identity".into());
        }
        self.reserved.extend(ids.into_iter().filter(|id| !self.order.contains(id)));
        Ok(())
    }
    pub(crate) fn add_parked_identity(&mut self, id: u64) -> Result<(), String> {
        if id==0 || id>crate::session_recovery::MAX_SESSION_DRAWING_ID || self.selected==0 || self.order.contains(&id) {
            return Err("Invalid restored drawing identity".into());
        }
        self.order.push(id);
        self.reserved.remove(&id);
        self.next=if id==crate::session_recovery::MAX_SESSION_DRAWING_ID {1}else {self.next.max(id+1)};
        self.undo.clear();self.redo.clear();
        Ok(())
    }
    pub(crate) fn restore_identity(&mut self, id: u64) -> Result<(), String> {
        if id==0 || id>crate::session_recovery::MAX_SESSION_DRAWING_ID || self.selected==0 || (id!=self.selected && self.order.contains(&id)) {
            return Err("Invalid restored drawing identity".into());
        }
        let selected=self.order.iter_mut().find(|entry|**entry==self.selected).ok_or("Missing selected drawing")?;
        self.reserved.remove(&id);
        *selected=id;self.selected=id;self.next=if id==crate::session_recovery::MAX_SESSION_DRAWING_ID {1}else {self.next.max(id+1)};self.undo.clear();self.redo.clear();
        Ok(())
    }
    pub(crate) fn restore_order(&mut self, order:&[u64], selected:u64)->Result<(),String> {
        let mut expected=self.order.clone();expected.sort_unstable();
        let mut supplied=order.to_vec();supplied.sort_unstable();
        if expected!=supplied || supplied.windows(2).any(|pair|pair[0]==pair[1]) || selected!=self.selected {
            return Err("Restored drawing membership changed".into());
        }
        self.order=order.to_vec();self.undo.clear();self.redo.clear();Ok(())
    }
    pub fn order(&self) -> &[u64] {
        &self.order
    }
    pub fn selected(&self) -> u64 {
        self.selected
    }
    pub fn add(&mut self) -> u64 {
        let mut id = self.next;
        while self.order.contains(&id) || self.reserved.contains(&id) {
            id = if id == crate::session_recovery::MAX_SESSION_DRAWING_ID { 1 } else { id + 1 };
        }
        self.next = if id == crate::session_recovery::MAX_SESSION_DRAWING_ID { 1 } else { id + 1 };
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
        if id == self.selected {
            self.selected = self.after_close().unwrap_or(0);
        }
        self.order.remove(index);
        // Opening/closing changes membership; order undo must never resurrect a
        // discarded document or silently remove a newly opened drawing.
        self.undo.clear();
        self.redo.clear();
        true
    }
    pub fn after_close(&self) -> Option<u64> {
        drawing_after_close(self.order.iter().copied(), self.selected)
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
    pub fn drag(
        &self,
        id: u64,
        press: [f32; 2],
        hits: &[DocumentTabHit],
        clip: crate::Bounds,
    ) -> Option<DocumentTabDrag> {
        DocumentTabDrag::new(&self.order, id, press, hits, clip)
    }
    /// Only measured, visible members accept a drop. Out-of-strip movement is
    /// cancellation, never an implicit move-to-end or a window tear-off.
    pub fn drop_target(
        &self,
        hits: &[DocumentTabHit],
        point: [f32; 2],
        vertical: bool,
    ) -> Option<Option<u64>> {
        Self::drop_target_in_order(&self.order, hits, point, vertical)
    }
    /// Stateless native geometry query; membership is revalidated on publication.
    pub fn drop_target_in_order(order: &[u64], hits: &[DocumentTabHit], point: [f32; 2], vertical: bool) -> Option<Option<u64>> {
        if !point.iter().all(|v| v.is_finite()) {
            return None;
        }
        let hit = hits
            .iter()
            .find(|h| order.contains(&h.id) && h.bounds.contains(point[0], point[1]))?;
        let before = if vertical {
            point[1] < hit.bounds.y + hit.bounds.height / 2.
        } else {
            point[0] < hit.bounds.x + hit.bounds.width / 2.
        };
        if before {
            Some(Some(hit.id))
        } else {
            let i = order.iter().position(|&id| id == hit.id)?;
            Some(order.get(i + 1).copied())
        }
    }
}

#[derive(serde::Deserialize)]
pub struct DocumentTabHit {
    pub id: u64,
    pub bounds: crate::Bounds,
}

pub struct DocumentTabDrag {
    id: u64,
    order: Vec<u64>,
    clip: crate::Bounds,
    slide: crate::tab_drag::TabDrag,
}

#[derive(Debug, serde::Serialize)]
pub struct DocumentTabSlide {
    pub bounds: crate::Bounds,
    pub offsets: Vec<f32>,
    pub attached: bool,
    pub before: Option<u64>,
}

impl DocumentTabDrag {
    pub fn new(
        order: &[u64],
        id: u64,
        press: [f32; 2],
        hits: &[DocumentTabHit],
        clip: crate::Bounds,
    ) -> Option<Self> {
        if hits.len() != order.len()
            || hits.iter().zip(order).any(|(hit, id)| hit.id != *id)
            || !press[1].is_finite()
            || !clip.y.is_finite()
            || !clip.height.is_finite()
        {
            return None;
        }
        let source = order.iter().position(|v| *v == id)?;
        let tabs: Vec<_> = hits
            .iter()
            .enumerate()
            .map(|(index, hit)| crate::TabHit {
                group: 0,
                index,
                bounds: hit.bounds,
            })
            .collect();
        Some(Self {
            id,
            order: order.to_vec(),
            clip,
            slide: crate::tab_drag::TabDrag::new(0, source, press, &tabs, clip)?,
        })
    }
    pub fn is_current(&self, order: &[u64]) -> bool {
        self.order == order
    }
    pub fn preview(&self, point: [f32; 2]) -> Option<DocumentTabSlide> {
        let preview = self.slide.preview(point)?;
        let reach = self.clip.height / 2.;
        if !point[1].is_finite()
            || point[1] < self.clip.y - reach
            || point[1] > self.clip.y + self.clip.height + reach
        {
            return Some(DocumentTabSlide {
                bounds: self.slide.source_bounds(),
                offsets: vec![0.; self.order.len()],
                attached: false,
                before: None,
            });
        }
        let source = self.order.iter().position(|v| *v == self.id)?;
        let slot = preview.insertion - usize::from(preview.insertion > source);
        let mut offsets = vec![0.; self.order.len()];
        for offset in &preview.offsets {
            offsets[offset.index] = offset.x;
        }
        Some(DocumentTabSlide {
            bounds: preview.bounds,
            offsets,
            attached: true,
            before: self
                .order
                .iter()
                .filter(|v| **v != self.id)
                .nth(slot)
                .copied(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_restore_identities_are_skipped_until_hydrated() {
        let mut tabs = DocumentTabs::default();
        tabs.reserve_identities(&[2, 3, crate::session_recovery::MAX_SESSION_DRAWING_ID]).unwrap();
        assert_eq!(tabs.add(), 4);
        assert!(tabs.reserve_identities(&[5, 0]).is_err());
        assert_eq!(tabs.add(), 5);
        tabs.add_parked_identity(2).unwrap();
        assert!(!tabs.reserved.contains(&2));
        let selected = tabs.selected();
        tabs.next = crate::session_recovery::MAX_SESSION_DRAWING_ID;
        assert_eq!(tabs.add(), 6);
        assert_ne!(tabs.selected(), selected);
    }
    #[test]
    fn restored_identity_keeps_allocation_exact_and_skips_live_collisions() {
        let mut tabs = DocumentTabs::default();
        tabs.add();
        tabs.add();
        tabs.restore_identity(crate::session_recovery::MAX_SESSION_DRAWING_ID).unwrap();
        assert_eq!(tabs.add(), 3);
        tabs.next = crate::session_recovery::MAX_SESSION_DRAWING_ID;
        assert_eq!(tabs.add(), 4);
        let order = tabs.order().to_vec();
        for id in [0, crate::session_recovery::MAX_SESSION_DRAWING_ID + 1, u64::MAX, 1] {
            assert!(tabs.restore_identity(id).is_err());
            assert_eq!(tabs.order(), order);
        }
    }
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
    fn strip_drag_slides_gapped_neighbors_and_detaches_vertically() {
        let mut tabs = DocumentTabs::default();
        let b = tabs.add();
        let c = tabs.add();
        let hits: Vec<_> = tabs
            .order()
            .iter()
            .enumerate()
            .map(|(i, &id)| DocumentTabHit {
                id,
                bounds: crate::Bounds {
                    x: 10. + i as f32 * 106.,
                    y: 1.,
                    width: 100.,
                    height: 32.,
                },
            })
            .collect();
        let clip = crate::Bounds {
            x: 10.,
            y: 1.,
            width: 312.,
            height: 32.,
        };
        let drag = tabs.drag(1, [40., 17.], &hits, clip).unwrap();
        let still = drag.preview([40., 17.]).unwrap();
        assert_eq!(still.offsets, [0., 0., 0.]);
        assert_eq!(still.before, Some(b));
        assert!(!tabs.reorder(1, still.before));
        let over = drag.preview([40. + 57., 30.]).unwrap();
        assert_eq!(over.offsets, [0., -106., 0.]);
        assert_eq!(over.bounds.x, 67.);
        assert_eq!(over.before, Some(c));
        let end = drag.preview([500., 17.]).unwrap();
        assert_eq!(end.offsets, [0., -106., -106.]);
        assert_eq!(end.bounds.x, 222.);
        assert_eq!(end.before, None);
        assert!(end.attached);
        assert!(drag.preview([500., 33. + 16.]).unwrap().attached);
        let away = drag.preview([500., 33. + 17.]).unwrap();
        assert!(!away.attached);
        assert_eq!(away.offsets, [0., 0., 0.]);
        assert_eq!(away.bounds, hits[0].bounds);
        assert!(drag.is_current(tabs.order()));
        assert!(tabs.reorder(1, end.before));
        assert_eq!(tabs.order(), &[b, c, 1]);
        assert!(!drag.is_current(tabs.order()));
        assert!(tabs.drag(1, [40., 17.], &hits, clip).is_none());
        assert!(tabs.drag(1, [40., 17.], &hits[..2], clip).is_none());
        assert!(tabs.drag(9, [40., 17.], &hits, clip).is_none());
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
