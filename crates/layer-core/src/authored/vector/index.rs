use super::{IdBuild, ItemId, PathItem, Position, Resource, VectorChunk, VectorObject};
use std::{collections::{BTreeSet, HashMap}, sync::Arc};

const CELL: f64 = 256.;
const SPREAD: i64 = 64;

#[derive(Clone, Copy)]
struct Entry { chunk: usize, index: u32, bounds: [f64; 4] }

#[derive(Default)]
pub struct VectorIndex {
    root: Option<VectorObject>,
    order: HashMap<usize, u32, IdBuild>,
    cells: HashMap<[i32; 2], Vec<Entry>, IdBuild>,
    large: Vec<Entry>,
    retired: HashMap<usize, Resource<VectorChunk>, IdBuild>,
}

fn key(chunk: &Resource<VectorChunk>) -> usize { Arc::as_ptr(chunk.storage()) as usize }
fn span(bounds: [f64; 4]) -> [i32; 4] {
    let cell = |v: f64| (v / CELL).floor().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    [cell(bounds[0]), cell(bounds[1]), cell(bounds[2]), cell(bounds[3])]
}
fn spread([x0, y0, x1, y1]: [i32; 4]) -> i64 { (i64::from(x1) - i64::from(x0) + 1).saturating_mul(i64::from(y1) - i64::from(y0) + 1) }
fn meets(a: [f64; 4], b: [f64; 4]) -> bool { a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3] }

impl VectorIndex {
    fn update(&mut self, object: &VectorObject) {
        if self.root.as_ref().is_some_and(|root| root.same_root(object)) { return; }
        let order: HashMap<usize, u32, IdBuild> = object.chunks().iter().enumerate().map(|(i, chunk)| (key(chunk), i as u32)).collect();
        if let Some(previous) = self.root.take() {
            self.retired.extend(previous.chunks().iter().filter(|chunk| !order.contains_key(&key(chunk))).map(|chunk| (key(chunk), chunk.clone())));
        }
        let known = std::mem::replace(&mut self.order, order);
        for chunk in object.chunks().iter().filter(|chunk| !known.contains_key(&key(chunk))) {
            if self.retired.remove(&key(chunk)).is_none() { self.insert(chunk); }
        }
        if self.retired.len() > self.order.len().max(64) {
            let order = &self.order;
            self.cells.retain(|_, entries| { entries.retain(|entry| order.contains_key(&entry.chunk)); !entries.is_empty() });
            self.large.retain(|entry| order.contains_key(&entry.chunk));
            self.retired.clear();
        }
        self.root = Some(object.clone());
    }
    fn visible(chunk: &Resource<VectorChunk>) -> impl Iterator<Item = (u32, [f64; 4])> + '_ {
        chunk.records().iter().enumerate().filter_map(|(index, record)| record.path().filter(|path| !path.hidden).map(|path| (index as u32, path.bounds())))
    }
    fn insert(&mut self, chunk: &Resource<VectorChunk>) {
        let chunk_key = key(chunk);
        for (index, bounds) in Self::visible(chunk) {
            let entry = Entry { chunk: chunk_key, index, bounds };
            let s = span(bounds);
            if spread(s) > SPREAD { self.large.push(entry); continue; }
            for y in s[1]..=s[3] { for x in s[0]..=s[2] { self.cells.entry([x, y]).or_default().push(entry); } }
        }
    }
    fn candidates(&self, area: [f64; 4]) -> Vec<(u32, u32)> {
        let s = span(area);
        let mut found: Vec<(u32, u32)> = Vec::new();
        let mut take = |entries: &[Entry]| {
            let mut chunk = None;
            for entry in entries {
                if chunk.is_none_or(|(key, _)| key != entry.chunk) { chunk = Some((entry.chunk, self.order.get(&entry.chunk).copied())); }
                if let Some((_, Some(order))) = chunk
                    && meets(entry.bounds, area) { found.push((order, entry.index)); }
            }
        };
        if spread(s) > self.cells.len() as i64 {
            for (cell, entries) in &self.cells { if (s[0]..=s[2]).contains(&cell[0]) && (s[1]..=s[3]).contains(&cell[1]) { take(entries); } }
        } else {
            for y in s[1]..=s[3] { for x in s[0]..=s[2] { if let Some(entries) = self.cells.get(&[x, y]) { take(entries); } } }
        }
        take(&self.large);
        found.sort_unstable();
        found.dedup();
        found
    }
    fn path(&self, (chunk, index): (u32, u32)) -> Option<&PathItem> {
        self.root.as_ref()?.chunks().get(chunk as usize)?.records().get(index as usize)?.path()
    }
    pub fn hit(&mut self, object: &VectorObject, point: [f64; 2], slop: f64) -> Option<ItemId> {
        self.update(object);
        self.candidates([point[0] - slop, point[1] - slop, point[0] + slop, point[1] + slop]).into_iter()
            .filter_map(|at| self.path(at)).find(|path| path.pickable() && path.hit(point, slop)).map(|path| path.id)
    }
    pub fn near(&mut self, object: &VectorObject, area: [f64; 4]) -> Vec<Position> {
        self.update(object);
        self.candidates(area).into_iter().map(|(chunk, index)| Position { chunk, index }).collect()
    }
    pub fn within(&mut self, object: &VectorObject, area: [f64; 4]) -> BTreeSet<ItemId> {
        self.update(object);
        let inside = |b: [f64; 4]| b[0] >= area[0] && b[1] >= area[1] && b[2] <= area[2] && b[3] <= area[3];
        self.candidates(area).into_iter().filter_map(|at| self.path(at)).filter(|path| path.pickable() && inside(path.visual_bounds())).map(|path| path.id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authored::vector::*;
    use std::collections::HashSet;

    fn line(id: u64, from: [f64; 2], to: [f64; 2]) -> Record {
        let geometry = PathGeometry::new(vec![Subpath::new(&[Anchor::corner(from), Anchor::corner(to)], false)]).unwrap();
        let paint = Paint::color(crate::color::RgbColor::new(Default::default(), [0., 0., 0., 1.]).unwrap());
        Record::Path(PathItem::new(ItemId(id), Arc::new(geometry), Arc::new(Style::new(vec![PaintEntry::Stroke(Stroke::new(paint, 2.))]))))
    }
    fn hatching(count: u64) -> VectorObject {
        let columns = (count as f64).sqrt().ceil() as u64;
        VectorObject::from_records((0..count).map(|i| {
            let seed = i.wrapping_mul(0x9e37_79b9_7f4a_7c15);
            let jitter = |shift: u32| ((seed >> shift) & 1023) as f64 / 1024. * 12.;
            let (x, y) = ((i % columns) as f64 * 12. + jitter(8), (i / columns) as f64 * 12. + jitter(24));
            line(i + 1, [x, y], [x + 8., y + 10.])
        }).collect())
    }
    fn scattered(count: u64) -> VectorObject {
        let columns = (count as f64).sqrt().ceil() as u64;
        let cells = columns * columns;
        let step = (1..cells).rev().find(|s| gcd(*s, cells) == 1 && *s > cells / 3).unwrap();
        VectorObject::from_records((0..count).map(|i| {
            let slot = i * step % cells;
            let (x, y) = ((slot % columns) as f64 * 12., (slot / columns) as f64 * 12.);
            line(i + 1, [x, y], [x + 8., y + 10.])
        }).collect())
    }
    fn gcd(a: u64, b: u64) -> u64 { if b == 0 { a } else { gcd(b, a % b) } }
    fn probes(object: &VectorObject) -> Vec<[f64; 2]> {
        let [x0, y0, x1, y1] = object.bounds().unwrap();
        (0..50).map(|i| [x0 + (x1 - x0) * (0.1 + 0.8 * f64::from(i % 10) / 9.), y0 + (y1 - y0) * (0.1 + 0.8 * f64::from(i / 10) / 4.)]).collect()
    }

    #[test]
    fn the_index_follows_edits_order_changes_and_undo() {
        let mut hidden = line(5, [0., 70.], [100., 70.]);
        if let Record::Path(path) = &mut hidden { path.hidden = true; }
        let object = VectorObject::from_records(vec![line(1, [0., 0.], [100., 0.]), line(2, [50., -20.], [50., 20.]), line(3, [0., 600.], [9000., 6400.]), hidden]);
        let mut index = VectorIndex::default();
        assert_eq!(index.hit(&object, [4500., 3500.], 1.), Some(ItemId(3)), "a path spanning many cells");
        assert_eq!(index.hit(&object, [20., 70.], 2.), None, "hidden paths are not picked");
        assert_eq!(index.within(&object, [-10., -30., 110., 30.]), [ItemId(1), ItemId(2)].into());
        let moved = object.transform_items(&[ItemId(2)].into(), crate::authored::Affine64([1., 0., 0., 1., 500., 0.]), true).unwrap();
        assert_eq!(index.hit(&moved, [50., 10.], 0.), None, "the index follows edits");
        assert_eq!(index.hit(&moved, [550., 10.], 0.), Some(ItemId(2)));
        let front = moved.insert_front(vec![line(9, [40., 0.], [60., 0.])]);
        assert_eq!(index.hit(&front, [50., 0.], 0.), Some(ItemId(9)), "chunk order changes reorder candidates");
        assert_eq!(index.hit(&object, [50., 10.], 0.), Some(ItemId(2)), "undo restores the earlier version");
    }

    #[test]
    fn picking_and_edit_work_stay_flat_from_ten_to_fifty_thousand_paths() {
        for layout in [hatching as fn(u64) -> VectorObject, scattered] {
            let counts: Vec<(usize, usize)> = [10_000, 50_000].into_iter().map(|count| {
                let object = layout(count);
                let mut index = VectorIndex::default();
                let candidates = probes(&object).iter().map(|p| { index.update(&object); index.candidates([p[0] - 6., p[1] - 6., p[0] + 6., p[1] + 6.]).len() }).max().unwrap();
                let one = BTreeSet::from([ItemId(count / 2)]);
                let moved = object.transform_items(&one, crate::authored::Affine64([1., 0., 0., 1., 3., 3.]), true).unwrap();
                let keys: HashSet<usize> = object.chunks().iter().map(key).collect();
                let rewritten = moved.chunks().iter().filter(|chunk| !keys.contains(&key(chunk))).map(|chunk| chunk.records().len()).sum();
                index.update(&moved);
                assert!(index.hit(&moved, probes(&moved)[0], 6.).is_none_or(|id| moved.contains(id)));
                (candidates, rewritten)
            }).collect();
            assert!(counts[0].0 > 0 && counts[1].0 <= counts[0].0 + 4, "probe candidates grew with the layer: {counts:?}");
            assert_eq!(counts[0].1, counts[1].1, "a one-path edit reindexes the same number of records: {counts:?}");
        }
    }

    #[test]
    fn replaced_chunks_stay_bounded_in_the_index_and_return_with_undo() {
        let object = hatching(10_000);
        let mut index = VectorIndex::default();
        index.update(&object);
        let entries = |index: &VectorIndex| index.cells.values().map(Vec::len).sum::<usize>() + index.large.len();
        let fresh = entries(&index);
        let mut current = object.clone();
        for i in 0..2_000u64 {
            let one = BTreeSet::from([ItemId(i * 7_919 % 10_000 + 1)]);
            current = current.transform_items(&one, crate::authored::Affine64([1., 0., 0., 1., 0.5, 0.5]), true).unwrap();
            index.update(&current);
            assert!(entries(&index) <= fresh * 2 + CHUNK_RECORDS * 64, "edit {i}: {} entries against {fresh}", entries(&index));
        }
        let area = [100., 100., 900., 900.];
        assert_eq!(index.within(&object, area), VectorIndex::default().within(&object, area), "undo to the first version finds what a fresh index finds");
        assert_eq!(index.within(&current, area), VectorIndex::default().within(&current, area));
    }
    #[test]
    fn extreme_finite_queries_match_the_linear_oracle() {
        assert_eq!(spread([i32::MIN, i32::MIN, i32::MAX, i32::MAX]), i64::MAX);
        let object = VectorObject::from_records(vec![line(1, [0., 0.], [100., 0.]), line(2, [0., 600.], [9000., 6400.])]);
        let mut index = VectorIndex::default();
        index.update(&object);
        for area in [
            [-f64::MAX, -f64::MAX, f64::MAX, f64::MAX],
            [-f64::MAX, 0., f64::MAX, f64::MAX],
            [250., 250., 260., 260.],
        ] {
            let expected: Vec<_> = object.chunks().iter().enumerate().flat_map(|(chunk, records)|
                VectorIndex::visible(records).filter(move |(_, bounds)| meets(*bounds, area))
                    .map(move |(record, _)| (chunk as u32, record))).collect();
            assert_eq!(index.candidates(area), expected);
        }
        assert_eq!(index.within(&object, [-f64::MAX, -f64::MAX, f64::MAX, f64::MAX]), [ItemId(1), ItemId(2)].into());
    }

}
