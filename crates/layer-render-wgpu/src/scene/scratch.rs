use std::collections::HashMap;
use std::sync::{Arc, Weak};

#[derive(Clone, Default)]
pub(super) enum Usage {
    #[default]
    Free,
    Reserved(Arc<()>),
    Leased(Weak<()>),
}

#[derive(Clone)]
pub(super) struct Lease {
    pub index: usize,
    _owner: Arc<()>,
}
impl Lease {
    pub fn into_owner(self) -> Arc<()> { self._owner }
}

impl Usage {
    pub fn occupied(&self) -> bool {
        match self {
            Self::Free => false,
            Self::Reserved(_) => true,
            Self::Leased(owner) => owner.strong_count() != 0,
        }
    }

    pub fn reset(&mut self) {
        if let Self::Reserved(owner) = self { *self = Self::Leased(Arc::downgrade(owner)); }
        if !self.occupied() { *self = Self::Free; }
    }

    pub fn lease(&mut self, index: usize) -> Lease {
        let owner = match self {
            Self::Reserved(owner) => owner.clone(),
            Self::Leased(owner) => owner.upgrade().expect("scratch reservation expired"),
            Self::Free => panic!("scratch page is not reserved"),
        };
        *self = Self::Leased(Arc::downgrade(&owner));
        Lease { index, _owner: owner }
    }

    pub fn reserve(&mut self) { *self = Self::Reserved(Arc::new(())); }

    pub fn keep(&mut self, lease: Lease) -> usize {
        let index = lease.index;
        *self = Self::Reserved(lease._owner);
        index
    }
}

pub(super) fn settle(pages: &mut [[u32; 2]]) -> usize {
    let span = |axis: usize| pages.iter().map(|page| page[axis]).max().unwrap_or(0) - pages.iter().map(|page| page[axis]).min().unwrap_or(0);
    let across = usize::from(span(0) > span(1));
    pages.sort_unstable_by_key(|page| (page[1 - across], page[across]));
    Frontier::new(pages).peak(pages) + pages.len().min(1)
}

pub(super) struct Frontier<T> {
    unread: HashMap<[u32; 2], usize>,
    held: Vec<([u32; 2], T)>,
}

impl<T> Frontier<T> {
    pub fn new(resolved: &[[u32; 2]]) -> Self {
        let mut unread: HashMap<_, _> = resolved.iter().map(|page| (*page, 0)).collect();
        for read in resolved.iter().flat_map(|page| super::watercolor_stencil(*page)).flatten() {
            if let Some(readers) = unread.get_mut(&read) { *readers += 1; }
        }
        Self { unread, held: Vec::new() }
    }

    pub fn read(&mut self, reader: [u32; 2]) -> Vec<T> {
        if !self.unread.contains_key(&reader) { return Vec::new(); }
        for read in super::watercolor_stencil(reader).flatten() {
            if let Some(readers) = self.unread.get_mut(&read) { *readers -= 1; }
        }
        self.held.extract_if(.., |(page, _)| self.unread[page] == 0).map(|(_, output)| output).collect()
    }

    pub fn hold(&mut self, page: [u32; 2], output: T) -> Option<T> {
        if self.unread.get(&page).is_some_and(|readers| *readers != 0) {
            self.held.push((page, output));
            None
        } else {
            Some(output)
        }
    }
}

impl Frontier<()> {
    fn peak(mut self, pages: &[[u32; 2]]) -> usize {
        pages.iter().map(|page| {
            self.read(*page);
            let held = self.held.len() + 1;
            self.hold(*page, ());
            held
        }).max().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leases_release_on_unwind_and_keep_cloned_consumers_reserved() {
        let mut usage = Usage::default();
        usage.reserve();
        let lease = usage.lease(0);
        let consumer = lease.clone();
        usage.reset();
        assert!(usage.occupied());
        drop(lease);
        assert!(usage.occupied());
        drop(consumer);
        assert!(!usage.occupied());
        let result: Result<(), ()> = (|| {
            usage.reserve();
            let _lease = usage.lease(0);
            Err(())?;
            Ok(())
        })();
        assert!(result.is_err());
        assert!(!usage.occupied());
    }

    #[test]
    fn a_swept_frontier_frees_an_output_after_its_last_reader_and_holds_the_shorter_side() {
        let grid = |[width, height]: [u32; 2]| (0..height).flat_map(move |y| (0..width).map(move |x| [x, y]));
        let sparse: Vec<_> = grid([9, 6]).filter(|[x, y]| (x * 3 + y * 5) % 7 != 0).collect();
        for mut pages in [[1, 1], [2, 1], [1, 5], [3, 3], [7, 4], [4, 9]].map(|size| grid(size).collect::<Vec<_>>()).into_iter().chain([sparse]) {
            pages.reverse();
            let scratch = settle(&mut pages);
            let side = |axis: usize| pages.iter().map(|page| page[axis]).max().unwrap() - pages.iter().map(|page| page[axis]).min().unwrap() + 1;
            let shorter = side(0).min(side(1)) as usize;
            let mut frontier = Frontier::new(&pages);
            let (mut freed, mut most) = (Vec::new(), 0);
            for (index, page) in pages.iter().enumerate() {
                let mut outputs = frontier.read(*page);
                most = most.max(frontier.held.len() + 1);
                outputs.extend(frontier.hold(*page, *page));
                for output in outputs {
                    assert!(pages[index + 1..].iter().all(|reader| super::super::watercolor_stencil(*reader).flatten().all(|read| read != output)),
                        "{output:?} was freed before a page that reads it");
                    freed.push(output);
                }
            }
            assert!(most <= shorter, "{most} outputs held at once across a side of {shorter}");
            assert_eq!(scratch, most + 1, "the reservation is the outputs held and one resolve");
            freed.sort_unstable();
            pages.sort_unstable();
            assert_eq!(freed, pages, "every output is freed once");
        }
    }

    #[test]
    fn an_operation_that_resolves_no_page_frees_each_output_at_once() {
        let pages = [[0, 0], [1, 0], [0, 1]];
        let mut frontier = Frontier::new(&[]);
        for page in pages {
            assert!(frontier.read(page).is_empty());
            assert_eq!(frontier.hold(page, page), Some(page));
        }
        assert_eq!(settle(&mut []), 0);
    }
}
