use super::{NEXT_PUBLICATION, Ordering, RasterPlane, RasterTile, TileKey};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::task::Waker;
use std::sync::atomic::{AtomicU8, AtomicUsize};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ContentName(pub(super) u64);

impl ContentName {
    pub fn fresh() -> Self {
        Self(NEXT_PUBLICATION.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("Raster content identity exhausted"))
    }

    pub(crate) fn captured(tile: &RasterTile) -> Self { Self(tile.identity()) }

    pub fn stamp(self) -> ContentStamp { mix(u128::from(self.0) ^ LEAF) }
}

pub type ContentStamp = u128;
const DEPTH: u32 = 16;
const LEAF: u128 = 0xd1b5_4a32_d192_ed03_94d0_49bb_1331_11eb;

#[derive(Clone, Debug)]
pub struct ContentVersion {
    identity: ContentStamp,
    pending: Option<PendingVersion>,
}

#[derive(Clone, Debug)]
enum PendingVersion { Canonical(Arc<CanonicalName>), Derived(Arc<DerivedVersion>), Cell(Arc<Node>, u32) }

#[derive(Debug)]
struct DerivedVersion {
    recipe: ContentStamp,
    inputs: Box<[ContentVersion]>,
    authored: ContentStamp,
    resolved: OnceLock<ContentStamp>,
}

#[derive(Debug)]
pub struct CanonicalName {
    before: ContentVersion,
    after: ContentName,
    outcome: AtomicU8,
    wakers: Mutex<Vec<Waker>>,
}

impl CanonicalName {
    pub fn new(before: impl Into<ContentVersion>) -> Arc<Self> {
        Arc::new(Self { before: before.into(), after: ContentName::fresh(), outcome: AtomicU8::new(0), wakers: Mutex::new(Vec::new()) })
    }

    pub fn version(self: &Arc<Self>) -> ContentVersion {
        ContentVersion { identity: self.after.stamp(), pending: Some(PendingVersion::Canonical(self.clone())) }
    }

    fn register_waker(&self, waker: &Waker) -> bool {
        match self.outcome.load(Ordering::Acquire) {
            1 => return self.before.register_waker(waker),
            2 => return false,
            _ => {},
        }
        let registration = waker.clone();
        let mut wakers = self.wakers.lock().unwrap();
        match self.outcome.load(Ordering::Acquire) {
            0 => {
                if !wakers.iter().any(|registered| registered.will_wake(waker)) { wakers.push(registration); }
                true
            }
            1 => { drop(wakers); self.before.register_waker(waker) }
            _ => false,
        }
    }

    pub fn resolve(&self, changed: bool) -> Result<(), &'static str> {
        self.outcome.compare_exchange(0, if changed { 2 } else { 1 }, Ordering::Release, Ordering::Relaxed)
            .map_err(|_| "Canonical content name was already resolved")?;
        let wakers = std::mem::take(&mut *self.wakers.lock().unwrap());
        for waker in wakers { waker.wake(); }
        Ok(())
    }
}

impl ContentVersion {
    fn known(identity: ContentStamp) -> Self { Self { identity, pending: None } }

    pub fn identity(&self) -> ContentStamp { self.identity }

    pub fn authored(&self) -> ContentStamp {
        match &self.pending {
            None => self.identity,
            Some(PendingVersion::Canonical(name)) => name.before.authored(),
            Some(PendingVersion::Derived(version)) => version.authored,
            Some(PendingVersion::Cell(node, _)) => node.authored_stamp(),
        }
    }

    pub fn derive(recipe: ContentStamp, inputs: &[Self]) -> Self {
        let combine = |values: &[Self], resolved: bool| {
            values.iter().try_fold(mix(recipe ^ values.len() as u128), |stamp, input|
                Some(mix(stamp ^ if resolved { input.resolved()? } else { input.identity })))
        };
        let authored = inputs.iter().fold(mix(recipe ^ inputs.len() as u128), |stamp, input| mix(stamp ^ input.authored()));
        let resolved = combine(inputs, true);
        if let Some(stamp) = resolved && stamp == authored { return Self::known(stamp); }
        let cache = OnceLock::new();
        if let Some(stamp) = resolved { let _ = cache.set(stamp); }
        Self { identity: combine(inputs, false).unwrap(), pending: Some(PendingVersion::Derived(Arc::new(DerivedVersion {
            recipe, inputs: inputs.into(), authored, resolved: cache,
        }))) }
    }

    pub fn resolved(&self) -> Option<ContentStamp> {
        match &self.pending {
            None => Some(self.identity),
            Some(PendingVersion::Canonical(name)) => match name.outcome.load(Ordering::Acquire) {
                1 => name.before.resolved(), 2 => Some(name.after.stamp()), _ => None,
            },
            Some(PendingVersion::Derived(version)) => {
                if let Some(stamp) = version.resolved.get() { return Some(*stamp); }
                let stamp = version.inputs.iter().try_fold(mix(version.recipe ^ version.inputs.len() as u128),
                    |stamp, input| Some(mix(stamp ^ input.resolved()?)))?;
                let _ = version.resolved.set(stamp);
                Some(stamp)
            },
            Some(PendingVersion::Cell(node, level)) => node.resolved_stamp(*level),
        }
    }

    /// Registers one unresolved canonical dependency and returns whether it subscribed.
    pub fn register_waker(&self, waker: &Waker) -> bool {
        match &self.pending {
            None => false,
            Some(PendingVersion::Canonical(name)) => name.register_waker(waker),
            Some(PendingVersion::Derived(version)) => {
                if version.resolved.get().is_some() { return false; }
                if version.inputs.iter().any(|input| input.register_waker(waker)) { return true; }
                let _ = self.resolved();
                false
            }
            Some(PendingVersion::Cell(node, level)) => node.register_waker(*level, waker),
        }
    }

    pub fn matches(&self, expected: &Self) -> Option<bool> {
        if self.identity == expected.identity { return Some(true); }
        self.resolved().zip(expected.resolved()).map(|(a, b)| a == b)
    }
}

impl From<ContentName> for ContentVersion {
    fn from(name: ContentName) -> Self { Self::known(name.stamp()) }
}

impl From<&ContentVersion> for ContentVersion {
    fn from(version: &ContentVersion) -> Self { version.clone() }
}

#[derive(Clone, Debug, Default)]
pub struct ContentTree {
    root: Option<Arc<Node>>,
    owner: Arc<Metadata>,
}

#[derive(Debug, Default)]
struct Metadata { bytes: AtomicUsize }

#[derive(Debug)]
struct Node {
    stamp: ContentStamp,
    kind: Kind,
    owner: Arc<Metadata>,
}

impl Node {
    const BYTES: usize = std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>();
    fn new(stamp: ContentStamp, kind: Kind, owner: &Arc<Metadata>) -> Arc<Self> {
        owner.bytes.fetch_add(Self::BYTES, Ordering::Relaxed);
        Arc::new(Self { stamp, kind, owner: owner.clone() })
    }

    fn pending(&self) -> bool { matches!(self.kind, Kind::Canonical(_) | Kind::Version(_) | Kind::PendingBranch(_)) }

    fn authored_stamp(&self) -> ContentStamp {
        match &self.kind {
            Kind::Canonical(name) => name.before.authored(),
            Kind::Version(version) => version.authored(),
            Kind::PendingBranch(branch) => branch.authored,
            _ => self.stamp,
        }
    }

    fn register_waker(&self, level: u32, waker: &Waker) -> bool {
        match &self.kind {
            Kind::Canonical(name) => name.register_waker(waker),
            Kind::Version(version) => version.register_waker(waker),
            Kind::PendingBranch(branch) => {
                if branch.resolved.get().is_some() { return false; }
                if branch.children.iter().flatten().any(|child| child.register_waker(level - 1, waker)) { return true; }
                let _ = self.resolved_stamp(level);
                false
            }
            _ => false,
        }
    }

    fn resolved_stamp(&self, level: u32) -> Option<ContentStamp> {
        match &self.kind {
            Kind::Canonical(name) => name.version().resolved(),
            Kind::Version(version) => version.resolved(),
            Kind::PendingBranch(branch) => {
                if let Some(stamp) = branch.resolved.get() { return Some(*stamp); }
                let mut children = [0; 4];
                for (value, child) in children.iter_mut().zip(&branch.children) {
                    *value = match child { Some(child) => child.resolved_stamp(level - 1)?, None => empty(level - 1) };
                }
                let stamp = branch_stamp(level, &children);
                let _ = branch.resolved.set(stamp);
                Some(stamp)
            },
            _ => Some(self.stamp),
        }
    }
}

impl Drop for Node {
    fn drop(&mut self) { self.owner.bytes.fetch_sub(Self::BYTES, Ordering::Relaxed); }
}

#[derive(Debug)]
enum Kind { Leaf, Canonical(Arc<CanonicalName>), Version(Arc<ContentVersion>), Branch([Option<Arc<Node>>; 4]), PendingBranch(Arc<PendingBranch>) }

#[derive(Debug)]
struct PendingBranch {
    children: [Option<Arc<Node>>; 4],
    authored: ContentStamp,
    resolved: OnceLock<ContentStamp>,
    owner: Arc<Metadata>,
}

impl PendingBranch {
    const BYTES: usize = std::mem::size_of::<Self>() + 2 * std::mem::size_of::<usize>();
    fn new(children: [Option<Arc<Node>>; 4], level: u32, owner: &Arc<Metadata>) -> Arc<Self> {
        owner.bytes.fetch_add(Self::BYTES, Ordering::Relaxed);
        let authored = branch_stamp(level, &children.each_ref().map(|child| child.as_ref().map_or_else(|| empty(level - 1), |child| child.authored_stamp())));
        Arc::new(Self { children, authored, resolved: OnceLock::new(), owner: owner.clone() })
    }
}

impl Drop for PendingBranch {
    fn drop(&mut self) { self.owner.bytes.fetch_sub(Self::BYTES, Ordering::Relaxed); }
}

#[derive(Clone)]
enum TileName { Known(ContentName), Version(ContentVersion) }

impl TileName {
    fn identity(&self) -> ContentStamp { match self { Self::Known(name) => name.stamp(), Self::Version(version) => version.identity() } }
}

fn mix(mut value: u128) -> u128 {
    value ^= value >> 47;
    value = value.wrapping_mul(0x9e37_79b9_7f4a_7c15_d1b5_4a32_d192_ed03);
    value ^ (value >> 53)
}

fn empty(level: u32) -> ContentStamp {
    static EMPTY: OnceLock<[ContentStamp; DEPTH as usize + 1]> = OnceLock::new();
    EMPTY.get_or_init(|| {
        let mut stamps = [0; DEPTH as usize + 1];
        stamps[0] = mix(0x94d0_49bb_1331_11eb_9e37_79b9_7f4a_7c15);
        for level in 1..=DEPTH as usize {
            stamps[level] = branch_stamp(level as u32, &[stamps[level - 1]; 4]);
        }
        stamps
    })[level as usize]
}

fn branch_stamp(level: u32, children: &[ContentStamp; 4]) -> ContentStamp {
    let mut stamp = mix(0xbf58_476d_1ce4_e5b9_94d0_49bb_1331_11eb ^ u128::from(level));
    for child in children { stamp = mix(stamp ^ child); }
    stamp
}

fn stamp(node: Option<&Arc<Node>>, level: u32) -> ContentStamp {
    node.map_or_else(|| empty(level), |node| node.stamp)
}

fn child(coordinate: [u32; 2], bit: u32) -> usize {
    (((coordinate[0] >> bit) & 1) | (((coordinate[1] >> bit) & 1) << 1)) as usize
}

fn check_coordinate(coordinate: [u32; 2]) {
    assert!(coordinate.into_iter().all(|c| c < 1 << DEPTH), "Raster content coordinate exceeds its index");
}

fn replace(node: Option<&Arc<Node>>, coordinate: [u32; 2], level: u32, name: Option<TileName>, owner: &Arc<Metadata>) -> Option<Arc<Node>> {
    if level == 0 {
        let name = name?;
        if let Some(node) = node && node.stamp == name.identity() { return Some(node.clone()); }
        let stamp = name.identity();
        let kind = match name {
            TileName::Known(_) => Kind::Leaf,
            TileName::Version(version) => match &version.pending {
                None => Kind::Leaf,
                Some(PendingVersion::Canonical(name)) => Kind::Canonical(name.clone()),
                _ => Kind::Version(Arc::new(version)),
            },
        };
        return Some(Node::new(stamp, kind, owner));
    }
    let mut children = match node.map(|node| &node.kind) {
        Some(Kind::Branch(children)) => children.clone(),
        Some(Kind::PendingBranch(branch)) => branch.children.clone(),
        _ => Default::default(),
    };
    let index = child(coordinate, level - 1);
    let next = replace(children[index].as_ref(), coordinate, level - 1, name, owner);
    if children[index].as_ref().zip(next.as_ref()).is_some_and(|(a, b)| Arc::ptr_eq(a, b))
        || children[index].is_none() && next.is_none() { return node.cloned(); }
    children[index] = next;
    if children.iter().all(Option::is_none) { return None; }
    let stamp = branch_stamp(level, &children.each_ref().map(|node| stamp(node.as_ref(), level - 1)));
    let kind = if children.iter().flatten().any(|node| node.pending()) { Kind::PendingBranch(PendingBranch::new(children, level, owner)) } else { Kind::Branch(children) };
    Some(Node::new(stamp, kind, owner))
}

fn children(node: Option<&Arc<Node>>) -> [Option<&Arc<Node>>; 4] {
    match node.map(|node| &node.kind) {
        Some(Kind::Branch(children)) => children.each_ref().map(Option::as_ref),
        Some(Kind::PendingBranch(branch)) => branch.children.each_ref().map(Option::as_ref),
        _ => [None; 4],
    }
}

fn authored_stamp(node: Option<&Arc<Node>>, level: u32) -> ContentStamp {
    node.map_or_else(|| empty(level), |node| node.authored_stamp())
}

fn differences(before: Option<&Arc<Node>>, after: Option<&Arc<Node>>, coordinate: [u32; 2], level: u32,
    stamp: fn(Option<&Arc<Node>>, u32) -> ContentStamp, changed: &mut impl FnMut([u32; 2])) {
    if stamp(before, level) == stamp(after, level) { return; }
    if level == 0 { changed(coordinate); return; }
    for (index, (before, after)) in children(before).into_iter().zip(children(after)).enumerate() {
        let coordinate = [coordinate[0] * 2 + (index as u32 & 1), coordinate[1] * 2 + (index as u32 >> 1)];
        differences(before, after, coordinate, level - 1, stamp, changed);
    }
}

impl ContentTree {
    pub fn set(&mut self, coordinate: [u32; 2], name: Option<ContentName>) {
        check_coordinate(coordinate);
        self.root = replace(self.root.as_ref(), coordinate, DEPTH, name.map(TileName::Known), &self.owner);
    }

    pub fn root(&self) -> ContentStamp { stamp(self.root.as_ref(), DEPTH) }

    pub fn root_version(&self) -> ContentVersion { self.version([0; 2], DEPTH).unwrap() }

    pub fn region_version(&self, bounds: [u32; 4]) -> ContentVersion {
        assert!(bounds[0] <= bounds[2] && bounds[1] <= bounds[3]
            && bounds[2] <= 1 << DEPTH && bounds[3] <= 1 << DEPTH, "Raster content region exceeds its index");
        fn collect(node: Option<&Arc<Node>>, coordinate: [u32; 2], level: u32, bounds: [u32; 4], inputs: &mut Vec<ContentVersion>) {
            let [x,y] = coordinate.map(|value| value << level);
            let span = 1 << level;
            if x >= bounds[2] || y >= bounds[3] || x + span <= bounds[0] || y + span <= bounds[1] { return; }
            if x >= bounds[0] && y >= bounds[1] && x + span <= bounds[2] && y + span <= bounds[3] {
                inputs.push(ContentVersion { identity: stamp(node, level),
                    pending: node.filter(|node| node.pending()).map(|node| PendingVersion::Cell(node.clone(), level)) });
                return;
            }
            let children = match node.map(|node| &node.kind) {
                Some(Kind::Branch(children)) => Some(children),
                Some(Kind::PendingBranch(branch)) => Some(&branch.children),
                _ => None,
            };
            for index in 0..4 {
                collect(children.and_then(|children| children[index].as_ref()),
                    [coordinate[0] * 2 + (index as u32 & 1), coordinate[1] * 2 + (index as u32 >> 1)], level - 1, bounds, inputs);
            }
        }
        let mut inputs = Vec::new();
        collect(self.root.as_ref(), [0; 2], DEPTH, bounds, &mut inputs);
        let recipe = bounds.into_iter().enumerate().fold(0, |bits, (index, value)| bits | u128::from(value) << (index * 32));
        ContentVersion::derive(recipe, &inputs)
    }

    pub fn changes(&self, after: &Self, mut changed: impl FnMut([u32; 2])) {
        differences(self.root.as_ref(), after.root.as_ref(), [0; 2], DEPTH, stamp, &mut changed);
    }

    pub fn authored_changes(&self, after: &Self, mut changed: impl FnMut([u32; 2])) {
        differences(self.root.as_ref(), after.root.as_ref(), [0; 2], DEPTH, authored_stamp, &mut changed);
    }

    fn node(&self, coordinate: [u32; 2], level: u32) -> Option<Option<&Arc<Node>>> {
        if level > DEPTH || coordinate.into_iter().any(|c| c >= 1 << (DEPTH - level)) {
            return None;
        }
        let coordinate = coordinate.map(|c| c << level);
        let mut node = self.root.as_ref();
        for current in (level..DEPTH).rev() {
            let children = match node.map(|node| &node.kind) {
                Some(Kind::Branch(children)) => children,
                Some(Kind::PendingBranch(branch)) => &branch.children,
                _ => return Some(None),
            };
            node = children[child(coordinate, current)].as_ref();
        }
        Some(node)
    }

    pub fn cell(&self, coordinate: [u32; 2], level: u32) -> Option<ContentStamp> {
        Some(stamp(self.node(coordinate, level)?, level))
    }

    pub fn version(&self, coordinate: [u32; 2], level: u32) -> Option<ContentVersion> {
        let node = self.node(coordinate, level)?;
        let identity = stamp(node, level);
        Some(ContentVersion { identity, pending: node.filter(|node| node.pending()).map(|node| PendingVersion::Cell(node.clone(), level)) })
    }

    pub fn tile_version(&self, coordinate: [u32; 2]) -> Option<ContentVersion> {
        let node = self.node(coordinate, 0)??;
        Some(ContentVersion { identity: node.stamp, pending: node.pending().then(|| PendingVersion::Cell(node.clone(), 0)) })
    }

    pub fn set_version(&mut self, coordinate: [u32; 2], version: ContentVersion) {
        check_coordinate(coordinate);
        self.root = replace(self.root.as_ref(), coordinate, DEPTH, Some(TileName::Version(version)), &self.owner);
    }

    pub fn metadata_bytes(&self) -> usize {
        self.owner.bytes.load(Ordering::Relaxed)
    }

    pub(crate) fn allocation(&self) -> (u64, usize) {
        (Arc::as_ptr(&self.owner) as u64, self.owner.bytes.load(Ordering::Relaxed)
            + std::mem::size_of::<Metadata>() + 2 * std::mem::size_of::<usize>())
    }
}

#[derive(Clone, Debug, Default)]
pub struct RasterTiles {
    tiles: BTreeMap<TileKey, RasterTile>,
    contents: [ContentTree; 3],
}

fn plane_index(plane: RasterPlane) -> usize {
    match plane { RasterPlane::Color => 0, RasterPlane::Mask => 1, RasterPlane::WatercolorWetness => 2 }
}

impl RasterTiles {
    pub fn content(&self, plane: RasterPlane) -> &ContentTree { &self.contents[plane_index(plane)] }

    pub(crate) fn metadata_owners(&self) -> [(u64, usize); 3] { self.contents.each_ref().map(ContentTree::allocation) }

    pub fn insert(&mut self, key: TileKey, tile: RasterTile) -> Option<RasterTile> {
        check_coordinate(key.coordinate);
        let name = ContentName::captured(&tile);
        let canonical = tile.0.canonical.clone();
        let previous = self.tiles.insert(key, tile);
        if previous.as_ref().is_none_or(|old| ContentName::captured(old) != name) {
            if let Some(canonical) = canonical { self.contents[plane_index(key.plane)].set_version(key.coordinate, canonical.version()); }
            else { self.contents[plane_index(key.plane)].set(key.coordinate, Some(name)); }
        }
        previous
    }

    pub fn remove(&mut self, key: &TileKey) -> Option<RasterTile> {
        let tile = self.tiles.remove(key)?;
        self.contents[plane_index(key.plane)].set(key.coordinate, None);
        Some(tile)
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&TileKey, &RasterTile) -> bool) {
        let contents = &mut self.contents;
        self.tiles.retain(|key, tile| {
            if keep(key, tile) { true } else { contents[plane_index(key.plane)].set(key.coordinate, None); false }
        });
    }
}

impl std::ops::Deref for RasterTiles {
    type Target = BTreeMap<TileKey, RasterTile>;
    fn deref(&self) -> &Self::Target { &self.tiles }
}

impl FromIterator<(TileKey, RasterTile)> for RasterTiles {
    fn from_iter<T: IntoIterator<Item = (TileKey, RasterTile)>>(tiles: T) -> Self {
        let mut result = Self::default();
        for (key, tile) in tiles { result.insert(key, tile); }
        result
    }
}

impl Extend<(TileKey, RasterTile)> for RasterTiles {
    fn extend<T: IntoIterator<Item = (TileKey, RasterTile)>>(&mut self, tiles: T) {
        for (key, tile) in tiles { self.insert(key, tile); }
    }
}

impl From<BTreeMap<TileKey, RasterTile>> for RasterTiles {
    fn from(tiles: BTreeMap<TileKey, RasterTile>) -> Self { tiles.into_iter().collect() }
}

impl<const N: usize> From<[(TileKey, RasterTile); N]> for RasterTiles {
    fn from(tiles: [(TileKey, RasterTile); N]) -> Self { tiles.into_iter().collect() }
}

impl<'a> IntoIterator for &'a RasterTiles {
    type Item = (&'a TileKey, &'a RasterTile);
    type IntoIter = std::collections::btree_map::Iter<'a, TileKey, RasterTile>;
    fn into_iter(self) -> Self::IntoIter { self.tiles.iter() }
}

impl IntoIterator for RasterTiles {
    type Item = (TileKey, RasterTile);
    type IntoIter = std::collections::btree_map::IntoIter<TileKey, RasterTile>;
    fn into_iter(self) -> Self::IntoIter { self.tiles.into_iter() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn captures_and_live_writes_keep_old_content_roots_available_to_undo() {
        let mut current = ContentTree::default();
        let initial = current.clone();
        let a = ContentName::fresh();
        current.set([1, 0], Some(a));
        let stroke_a = current.clone();
        let before_b = current.root();
        current.set([0, 0], Some(ContentName::fresh()));
        assert_ne!(current.root(), before_b);
        assert_eq!(current.cell([1, 0], 0), stroke_a.cell([1, 0], 0));
        assert_ne!(current.cell([0; 2], 1), stroke_a.cell([0; 2], 1));
        current.set([0; 2], None);
        assert_eq!(current.root(), stroke_a.root());
        current.set([1, 0], None);
        assert_eq!(current.root(), initial.root());
        assert!(current.metadata_bytes() > 0, "the retained Undo root still owns its index");
        drop(stroke_a);
        assert_eq!(current.metadata_bytes(), 0);
    }

    #[test]
    fn seeded_content_updates_match_cells_rebuilt_from_selected_captures() {
        let mut tree = ContentTree::default();
        let mut selected = BTreeMap::new();
        let mut seed = 0x139au64;
        for step in 0..256 {
            let before = tree.clone();
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let coordinate = [(seed >> 8) as u32 % 64, (seed >> 24) as u32 % 64];
            let name = (step % 4 != 0).then(ContentName::fresh);
            let changed = selected.get(&coordinate).copied() != name;
            tree.set(coordinate, name);
            if let Some(name) = name { selected.insert(coordinate, name); } else { selected.remove(&coordinate); }
            let mut differences = Vec::new();
            before.changes(&tree, |coordinate| differences.push(coordinate));
            assert_eq!(differences, if changed { vec![coordinate] } else { Vec::new() });
            let mut authored = Vec::new();
            before.authored_changes(&tree, |coordinate| authored.push(coordinate));
            assert_eq!(authored, differences);
            for level in 0..=6 {
                let cell = coordinate.map(|c| c >> level);
                let mut rebuilt = ContentTree::default();
                for (&at, &name) in &selected {
                    if at.map(|c| c >> level) == cell { rebuilt.set(at, Some(name)); }
                }
                assert_eq!(tree.cell(cell, level), rebuilt.cell(cell, level), "step={step} cell={cell:?} level={level}");
            }
        }
        assert!(tree.cell([1, 0], DEPTH).is_none());
        assert!(tree.cell([u32::MAX, 0], 1).is_none());
    }

    #[test]
    fn a_dense_gigapixel_plane_keeps_its_content_index_under_four_mib() {
        let mut tree = ContentTree::default();
        for y in 0..120 { for x in 0..128 { tree.set([x, y], Some(ContentName::fresh())); } }
        assert!(tree.metadata_bytes() < 4 * 1024 * 1024, "{}", tree.metadata_bytes());
        let snapshot = tree.clone();
        tree.set([u16::MAX.into(); 2], Some(ContentName::fresh()));
        assert_ne!(tree.root(), snapshot.root());
        assert_eq!(tree.cell([0; 2], 8), snapshot.cell([0; 2], 8));
    }

    #[test]
    fn a_single_leaf_change_alters_every_ancestor_through_all_sixteen_levels() {
        let coordinate = [17, 42];
        let low = ContentName::fresh();
        let replacement = ContentName::fresh();
        let maximum = ContentName::fresh();
        let mut tree = ContentTree::default();
        tree.set(coordinate, Some(low));
        tree.set([18, 42], Some(maximum));
        let before = tree.clone();
        tree.set(coordinate, Some(replacement));
        for level in 0..=DEPTH {
            let cell = coordinate.map(|c| c >> level);
            assert_ne!(before.cell(cell, level), tree.cell(cell, level), "level={level}");
        }
        assert_eq!(before.cell([18, 42], 0), tree.cell([18, 42], 0));
    }

    #[test]
    fn regional_versions_ignore_unrelated_tiles_and_restore_matching_history() {
        let mut tree = ContentTree::default();
        let selected = ContentName::fresh();
        tree.set([7, 11], Some(selected));
        tree.set([12, 11], Some(ContentName::fresh()));
        for bounds in [[7,11,8,12], [3,5,11,17], [0,0,128,128], [7,11,7,12]] {
            let before = tree.region_version(bounds);
            let history = tree.clone();
            tree.set([32767, 42], Some(ContentName::fresh()));
            assert_eq!(before.matches(&tree.region_version(bounds)), Some(true));
            let mut rebuilt = ContentTree::default();
            for y in bounds[1]..bounds[3] { for x in bounds[0]..bounds[2] {
                if let Some(version) = tree.tile_version([x,y]) { rebuilt.set_version([x,y], version); }
            }}
            assert_eq!(before.matches(&rebuilt.region_version(bounds)), Some(true));
            tree.set([7,11], Some(ContentName::fresh()));
            assert_eq!(before.matches(&tree.region_version(bounds)), Some(bounds[0] == bounds[2]));
            tree = history;
            assert_eq!(before.matches(&tree.region_version(bounds)), Some(true));
        }
    }

    #[test]
    fn regional_versions_keep_unresolved_packing_pending_without_renaming_identical_pixels() {
        for changed in [false, true] {
            let mut tree = ContentTree::default();
            let original = ContentName::fresh();
            tree.set([7,11], Some(original));
            let bounds = [3,5,11,17];
            let before = tree.region_version(bounds);
            let canonical = CanonicalName::new(original);
            tree.set_version([7,11], canonical.version());
            let pending = tree.region_version(bounds);
            assert_eq!(before.matches(&pending), None);
            canonical.resolve(changed).unwrap();
            assert_eq!(before.matches(&pending), Some(!changed));
            assert_eq!(pending.matches(&tree.region_version(bounds)), Some(true));
        }
    }

    #[test]
    #[should_panic(expected = "Raster content coordinate exceeds its index")]
    fn content_insertion_rejects_coordinates_outside_the_sixteen_level_grid() {
        ContentTree::default().set([1 << DEPTH, 0], Some(ContentName::fresh()));
    }

    #[test]
    fn selected_capture_mutations_update_only_their_plane_and_preserve_history() {
        let descriptor = crate::color::DocumentColor::default().paint_descriptor();
        let key = TileKey { plane: RasterPlane::Color, coordinate: [1, 0] };
        let mut tiles = RasterTiles::default();
        let tile = RasterTile::pending(descriptor);
        tiles.insert(key, tile.clone());
        let old = tiles.clone();
        let before = tiles.content(RasterPlane::Color).root();
        tiles.insert(key, tile);
        assert_eq!(before, tiles.content(RasterPlane::Color).root());
        tiles.insert(key, RasterTile::pending(descriptor));
        assert_ne!(before, tiles.content(RasterPlane::Color).root());
        assert_eq!(before, old.content(RasterPlane::Color).root());
        assert_eq!(tiles.content(RasterPlane::Mask).metadata_bytes(), 0);
        tiles.retain(|_, _| false);
        assert_eq!(tiles.content(RasterPlane::Color).root(), ContentTree::default().root());
        assert!(old.get(&key).unwrap().try_backing().is_none(), "versions must never request pending pixel bytes");
    }

    #[test]
    fn history_shares_one_physical_metadata_charge_and_releases_old_nodes() {
        let mut tree = ContentTree::default();
        tree.set([1, 1], Some(ContentName::fresh()));
        let old = tree.clone();
        let (owner, before) = tree.allocation();
        tree.set([1, 1], Some(ContentName::fresh()));
        let (_, held) = tree.allocation();
        assert!(held > before);
        assert_eq!(tree.allocation(), old.allocation());
        drop(old);
        assert_eq!(tree.allocation(), (owner, before));
        tree.set([1, 1], None);
        assert_eq!(tree.metadata_bytes(), 0);
    }

    #[test]
    fn pending_canonical_names_preserve_identical_pixels_without_guessing_quantization() {
        let before = ContentName::fresh();
        let name = CanonicalName::new(before);
        let version = name.version();
        let identity = version.identity();
        assert_eq!(version.resolved(), None);
        assert_eq!(version.matches(&before.into()), None);
        assert_eq!(version.matches(&name.version()), Some(true));
        name.resolve(false).unwrap();
        assert_eq!(version.matches(&before.into()), Some(true));
        assert_eq!(version.identity(), identity, "immutable capture keys must not change on resolution");
        assert!(name.resolve(true).is_err());
        let changed = CanonicalName::new(before);
        changed.resolve(true).unwrap();
        assert_eq!(changed.version().matches(&before.into()), Some(false));
        assert_eq!(changed.version().resolved(), Some(changed.version().identity()));
        assert_eq!(changed.version().authored(), before.stamp());
    }

    #[test]
    fn derived_versions_resolve_unchanged_capture_inputs_without_rebuilding_pixels() {
        let before = ContentName::fresh();
        let captured = CanonicalName::new(before);
        let other = ContentName::fresh();
        let original = ContentVersion::derive(17, &[before.into(), other.into()]);
        let pending = ContentVersion::derive(17, &[captured.version(), other.into()]);
        assert_eq!(pending.matches(&original), None);
        assert_eq!(pending.matches(&ContentVersion::derive(17, &[captured.version(), other.into()])), Some(true));
        captured.resolve(false).unwrap();
        assert_eq!(pending.matches(&original), Some(true));
        let changed = CanonicalName::new(before);
        let pending = ContentVersion::derive(17, &[changed.version(), other.into()]);
        changed.resolve(true).unwrap();
        assert_eq!(pending.matches(&original), Some(false));
        assert_eq!(pending.authored(), original.authored());
        let resolved = ContentVersion::derive(17, &[changed.version(), other.into()]);
        assert_eq!(resolved.matches(&pending), Some(true));
        assert_eq!(resolved.authored(), original.authored());
    }

    #[test]
    fn captured_cell_versions_resolve_at_every_level_without_changing_lookup_identity() {
        let name = ContentName::fresh();
        let coordinate = [17, 42];
        let mut live = ContentTree::default();
        live.set(coordinate, Some(name));
        let descriptor = crate::color::DocumentColor::default().paint_descriptor();
        for changed in [false, true] {
            let (tile, comparison) = RasterTile::pending_from_live(descriptor, name.into());
            let mut captured = RasterTiles::default();
            captured.insert(TileKey { plane: RasterPlane::Color, coordinate }, tile);
            let tree = captured.content(RasterPlane::Color);
            let lookup = tree.root();
            let mut authored = Vec::new();
            live.authored_changes(tree, |coordinate| authored.push(coordinate));
            assert!(authored.is_empty());
            for level in 0..=DEPTH {
                let cell = coordinate.map(|c| c >> level);
                let actual = tree.version(cell, level).unwrap();
                let expected = live.version(cell, level).unwrap();
                assert_eq!(actual.matches(&expected), None);
                assert_eq!(actual.authored(), expected.authored());
            }
            comparison.resolve(changed).unwrap();
            for level in 0..=DEPTH {
                let cell = coordinate.map(|c| c >> level);
                let actual = tree.version(cell, level).unwrap();
                let expected = live.version(cell, level).unwrap();
                assert_eq!(actual.matches(&expected), Some(!changed));
                assert_eq!(actual.authored(), expected.authored());
            }
            assert_eq!(tree.root(), lookup);
            live.authored_changes(tree, |coordinate| authored.push(coordinate));
            assert!(authored.is_empty(), "packing is a realization of the same authored write");
        }
    }

    #[test]
    fn a_later_identical_capture_does_not_wait_for_or_forget_its_earlier_comparison() {
        let before = ContentName::fresh();
        let first = CanonicalName::new(before);
        let second = CanonicalName::new(first.version());
        second.resolve(false).unwrap();
        assert_eq!(second.version().resolved(), None);
        first.resolve(false).unwrap();
        assert_eq!(second.version().matches(&before.into()), Some(true));
    }
    #[derive(Default)]
    struct PackingWake(AtomicUsize);
    impl std::task::Wake for PackingWake {
        fn wake(self: Arc<Self>) { self.wake_by_ref(); }
        fn wake_by_ref(self: &Arc<Self>) { self.0.fetch_add(1, Ordering::Relaxed); }
    }

    #[test]
    fn canonical_name_subscriptions_wake_once_after_resolution() {
        for changed in [false, true] {
            let name = CanonicalName::new(ContentName::fresh());
            let count = Arc::new(PackingWake::default());
            let waker = Waker::from(count.clone());
            let version = name.version();
            assert!(version.register_waker(&waker));
            assert!(version.register_waker(&waker.clone()));
            assert_eq!(name.wakers.lock().unwrap().len(), 1);
            name.resolve(changed).unwrap();
            assert_eq!(count.0.load(Ordering::Relaxed), 1);
            assert!(!version.register_waker(&waker));
            assert!(name.wakers.lock().unwrap().is_empty());
            assert!(name.resolve(!changed).is_err());
            assert_eq!(count.0.load(Ordering::Relaxed), 1);
        }
    }

    #[test]
    fn canonical_name_subscriptions_follow_unchanged_nested_cell_inputs() {
        let first = CanonicalName::new(ContentName::fresh());
        let second = CanonicalName::new(first.version());
        second.resolve(false).unwrap();
        let mut tree = ContentTree::default();
        tree.set_version([17, 42], second.version());
        let count = Arc::new(PackingWake::default());
        let waker = Waker::from(count.clone());
        let mut versions = vec![tree.region_version([0, 0, 128, 128])];
        versions.extend((0..=DEPTH).map(|level| tree.version([17 >> level, 42 >> level], level).unwrap()));
        for version in &versions { assert!(version.register_waker(&waker)); }
        assert_eq!(first.wakers.lock().unwrap().len(), 1);
        assert!(second.wakers.lock().unwrap().is_empty());
        first.resolve(false).unwrap();
        assert_eq!(count.0.load(Ordering::Relaxed), 1);
        for version in versions {
            assert!(!version.register_waker(&waker));
            assert!(version.resolved().is_some());
        }
    }

    #[test]
    fn canonical_name_subscriptions_watch_only_one_unresolved_input_per_query() {
        let first = CanonicalName::new(ContentName::fresh());
        let second = CanonicalName::new(ContentName::fresh());
        let version = ContentVersion::derive(17, &[first.version(), second.version()]);
        let count = Arc::new(PackingWake::default());
        let waker = Waker::from(count.clone());
        assert!(version.register_waker(&waker));
        assert_eq!(first.wakers.lock().unwrap().len(), 1);
        assert!(second.wakers.lock().unwrap().is_empty());
        first.resolve(false).unwrap();
        assert_eq!(count.0.load(Ordering::Relaxed), 1);
        assert!(version.register_waker(&waker));
        assert_eq!(second.wakers.lock().unwrap().len(), 1);
        second.resolve(true).unwrap();
        assert_eq!(count.0.load(Ordering::Relaxed), 2);
        assert!(!version.register_waker(&waker));
        assert!(version.resolved().is_some());
    }

    #[test]
    fn canonical_name_subscriptions_racing_resolution_never_lose_a_wake() {
        for subscribed_first in [false, true] { for _ in 0..64 {
            let name = CanonicalName::new(ContentName::fresh());
            let count = Arc::new(PackingWake::default());
            let waker = Waker::from(count.clone());
            if subscribed_first { assert!(name.version().register_waker(&waker)); }
            let barrier = std::sync::Barrier::new(2);
            let registered = std::thread::scope(|scope| {
                let producer = scope.spawn(|| { barrier.wait(); name.resolve(false).unwrap(); });
                barrier.wait();
                let registered = name.version().register_waker(&waker);
                producer.join().unwrap();
                registered
            });
            assert_eq!(count.0.load(Ordering::Relaxed), usize::from(subscribed_first || registered));
            assert!(!name.version().register_waker(&waker));
            assert!(name.wakers.lock().unwrap().is_empty());
        }}
    }

    #[test]
    fn canonical_name_subscriptions_notify_without_holding_the_subscription_lock() {
        struct UnlockedWake { name: std::sync::Weak<CanonicalName>, count: AtomicUsize }
        impl std::task::Wake for UnlockedWake {
            fn wake(self: Arc<Self>) { self.wake_by_ref(); }
            fn wake_by_ref(self: &Arc<Self>) {
                let name = self.name.upgrade().unwrap();
                assert!(name.wakers.try_lock().is_ok());
                assert!(name.version().resolved().is_some());
                self.count.fetch_add(1, Ordering::Relaxed);
            }
        }
        let name = CanonicalName::new(ContentName::fresh());
        let wake = Arc::new(UnlockedWake { name: Arc::downgrade(&name), count: AtomicUsize::new(0) });
        assert!(name.version().register_waker(&Waker::from(wake.clone())));
        name.resolve(false).unwrap();
        assert_eq!(wake.count.load(Ordering::Relaxed), 1);
    }

}
