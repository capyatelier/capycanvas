//! Bounded, disposable exact samples. The compressed immutable tile remains the
//! authority; cache keys never keep document/history backing alive.
use super::TileBlob;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, Weak},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DecodedTileCacheStats {
    /// Sample allocation capacities; map/Arc metadata and decode scratch are
    /// separate. A caller can briefly retain one evicted tile during upload.
    pub resident_bytes: usize,
    pub physical_bytes: usize,
    pub pinned_bytes: usize,
    pub peak_bytes: usize,
    pub limit_bytes: usize,
    pub entries: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
}

struct Entry {
    tile: Weak<TileBlob>,
    samples: Arc<Vec<u8>>,
    used: u64,
}
#[derive(Default)]
struct State {
    entries: HashMap<u64, Entry>,
    retired: VecDeque<(Weak<Vec<u8>>, usize)>,
    retired_bytes: usize,
    clock: u64,
    stats: DecodedTileCacheStats,
}
pub struct DecodedTileCache {
    state: Mutex<State>,
}
impl DecodedTileCache {
    pub fn new(limit_bytes: usize) -> Self {
        Self {
            state: Mutex::new(State {
                stats: DecodedTileCacheStats {
                    limit_bytes,
                    ..Default::default()
                },
                ..Default::default()
            }),
        }
    }

    pub fn set_limit(&self, limit_bytes: usize) { self.state.lock().unwrap().stats.limit_bytes = limit_bytes; }
    pub fn trim_unused(&self, max_entries: usize) -> usize {
        let mut state = self.state.lock().unwrap();
        let mut released = Vec::new();
        state.reap(max_entries);
        let held = state.retired_bytes;
        for _ in 0..max_entries {
            if state.stats.resident_bytes.saturating_add(held) <= state.stats.limit_bytes { break; }
            let Some(key) = state.entries.iter().filter(|(_,entry)|Arc::strong_count(&entry.samples)==1)
                .min_by_key(|(_,entry)|(entry.tile.strong_count()!=0,entry.used)).map(|(&key,_)|key) else { break; };
            let entry = state.remove(key);
            state.track(&entry.samples);
            released.push(entry.samples);
        }
        drop(state);
        released.into_iter().map(|samples| {
            let bytes=samples.capacity();let weak=Arc::downgrade(&samples);drop(samples);
            if weak.strong_count()==0 {bytes}else {0}
        }).sum()
    }
    pub fn stats(&self) -> DecodedTileCacheStats {
        let mut state = self.state.lock().unwrap();
        state.reap(64);
        let held = state.retired_bytes;
        DecodedTileCacheStats {
            entries: state.entries.len(), physical_bytes: state.stats.resident_bytes.saturating_add(held),
            pinned_bytes: held.saturating_add(state.entries.values().filter(|entry|Arc::strong_count(&entry.samples)>1).map(|entry|entry.samples.capacity()).sum::<usize>()),
            ..state.stats
        }
    }

    #[cfg(not(target_arch="wasm32"))]
    pub fn decode_on_worker(&self,tile:&Arc<TileBlob>)->Result<Arc<Vec<u8>>,String> {
        if let Some(samples)=self.cached(tile)? {return Ok(samples);}
        self.retain(tile,Arc::new(tile.decode_on_worker()?))
    }
    pub fn decode_bytes_on_worker(&self,tile:&Arc<TileBlob>,compressed:&[u8])->Result<Arc<Vec<u8>>,String> {
        if let Some(samples)=self.cached(tile)? {return Ok(samples);}
        self.retain(tile,Arc::new(tile.decode_bytes_on_worker(compressed)?))
    }
    pub fn cached(&self,tile:&Arc<TileBlob>)->Result<Option<Arc<Vec<u8>>>,String> {
        let mut state=self.state.lock().map_err(|_|"Source sample cache failed")?;
        state.clock=state.clock.saturating_add(1);let used=state.clock;
        if let Some(entry)=state.entries.get_mut(&tile.owner_identity()) {
            entry.used=used;let samples=entry.samples.clone();state.stats.hits+=1;return Ok(Some(samples));
        }
        state.stats.misses+=1;Ok(None)
    }
    fn retain(&self,tile:&Arc<TileBlob>,samples:Arc<Vec<u8>>)->Result<Arc<Vec<u8>>,String> {
        let key=tile.owner_identity();
        let bytes = samples.capacity();
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Source sample cache failed")?;
        if let Some(entry) = state.entries.get(&key) {
            return Ok(entry.samples.clone());
        }
        state.reap(1);
        if bytes > state.stats.limit_bytes {
            state.track(&samples);
            return Ok(samples);
        }
        while state.stats.resident_bytes > state.stats.limit_bytes - bytes {
            // Expired sources retire first; otherwise evict the least recently
            // requested tile. Active readers retain their immutable samples.
            let oldest = *state
                .entries
                .iter()
                .min_by_key(|(_, entry)| (entry.tile.strong_count() != 0, entry.used))
                .unwrap()
                .0;
            let entry = state.remove(oldest);
            state.track(&entry.samples);
        }
        state.clock = state.clock.saturating_add(1);
        let used = state.clock;
        state.entries.insert(
            key,
            Entry {
                tile: Arc::downgrade(tile),
                samples: samples.clone(),
                used,
            },
        );
        state.stats.resident_bytes += bytes;
        state.stats.peak_bytes = state.stats.peak_bytes.max(state.stats.resident_bytes);
        Ok(samples)
    }
}
impl State {
    fn track(&mut self, samples:&Arc<Vec<u8>>) {
        self.retired_bytes+=samples.capacity();self.retired.push_back((Arc::downgrade(samples),samples.capacity()));
    }
    fn reap(&mut self, max_entries:usize) {
        for _ in 0..max_entries.min(self.retired.len()) {
            let (samples,bytes)=self.retired.pop_front().unwrap();
            if samples.strong_count()==0 {self.retired_bytes-=bytes;} else {self.retired.push_back((samples,bytes));}
        }
    }
    fn remove(&mut self, key:u64) -> Entry {
        let entry=self.entries.remove(&key).unwrap();
        self.stats.resident_bytes-=entry.samples.capacity();self.stats.evictions+=1;entry
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{DocumentColor, SampleDepth, PixelDescriptor, RgbSpace};

    fn tile(value: u8) -> Arc<TileBlob> {
        Arc::new(TileBlob::encode(PixelDescriptor::COVERAGE8, &vec![value; 65536]).unwrap())
    }

    #[test]
    fn cache_evicts_within_budget_without_changing_active_readers_or_owning_sources() {
        let cache = DecodedTileCache::new(2 * 65536);
        let a = tile(13);
        let b = tile(71);
        let c = tile(191);
        let first = cache.decode_on_worker(&a).unwrap();
        cache.decode_on_worker(&b).unwrap();
        assert!(Arc::ptr_eq(&cache.decode_on_worker(&a).unwrap(), &first));
        cache.decode_on_worker(&c).unwrap(); // B is least recently used.
        assert_eq!(cache.stats().evictions, 1);
        assert!(Arc::ptr_eq(&cache.decode_on_worker(&a).unwrap(), &first));
        let weak = Arc::downgrade(&a);
        assert_eq!(Arc::strong_count(&a), 1);
        drop(a);
        assert!(weak.upgrade().is_none());
        cache.decode_on_worker(&b).unwrap(); // Expired A retires before still-live C.
        assert_eq!(cache.stats().evictions, 2);
        assert!(first.iter().all(|&v| v == 13));
        assert_eq!(cache.stats().resident_bytes, 2 * 65536);
        assert_eq!(cache.stats().peak_bytes, 2 * 65536);
        assert_eq!(cache.stats().entries, 2);
    }

    #[test]
    fn shrinking_grants_trims_unused_lru_in_bounded_steps_and_preserves_readers() {
        let cache=DecodedTileCache::new(3*65536);
        let a=tile(11);let b=tile(22);let c=tile(33);
        let held=cache.decode_on_worker(&a).unwrap();cache.decode_on_worker(&b).unwrap();cache.decode_on_worker(&c).unwrap();
        cache.set_limit(65536);
        assert_eq!(cache.stats().resident_bytes,3*65536);
        assert_eq!(cache.trim_unused(0),0);
        assert_eq!(cache.trim_unused(1),65536);
        assert!(!cache.state.lock().unwrap().entries.contains_key(&b.owner_identity()));
        assert_eq!(cache.stats().resident_bytes,2*65536);
        assert_eq!(cache.trim_unused(1),65536);
        assert_eq!(cache.stats().physical_bytes,65536);assert_eq!(cache.stats().pinned_bytes,65536);
        assert!(held.iter().all(|&byte|byte==11));
        cache.set_limit(0);assert_eq!(cache.trim_unused(8),0);
        drop(held);assert_eq!(cache.trim_unused(1),65536);assert_eq!(cache.stats().physical_bytes,0);
        cache.set_limit(65536);assert!(cache.decode_on_worker(&a).unwrap().iter().all(|&byte|byte==11));
        assert_eq!(cache.stats().resident_bytes,65536);
    }
    #[test]
    fn removed_and_uncached_samples_remain_physically_charged_until_readers_drop() {
        let cache=DecodedTileCache::new(65536);
        let a=tile(41);let b=tile(42);let held=cache.decode_on_worker(&a).unwrap();cache.decode_on_worker(&b).unwrap();
        assert_eq!(cache.stats().resident_bytes,65536);assert_eq!(cache.stats().physical_bytes,2*65536);
        assert_eq!(cache.stats().pinned_bytes,65536);
        assert_eq!(cache.trim_unused(1),65536);assert!(held.iter().all(|&byte|byte==41));
        assert_eq!(cache.stats().physical_bytes,65536);
        drop(held);assert_eq!(cache.stats().physical_bytes,0);
        cache.set_limit(0);let held=cache.decode_on_worker(&b).unwrap();let clone=held.clone();
        assert_eq!(cache.stats().resident_bytes,0);assert_eq!(cache.stats().physical_bytes,65536);
        assert_eq!(cache.stats().pinned_bytes,65536);drop(held);assert_eq!(cache.stats().physical_bytes,65536);
        drop(clone);assert_eq!(cache.stats().physical_bytes,0);assert_eq!(cache.stats().pinned_bytes,0);
    }

    #[test]
    fn retired_weak_records_are_reaped_in_bounded_steps() {
        let cache=DecodedTileCache::new(0);let source=tile(81);
        let readers:Vec<_>=(0..8).map(|_|cache.decode_on_worker(&source).unwrap()).collect();
        assert_eq!(cache.state.lock().unwrap().retired.len(),8);drop(readers);
        assert_eq!(cache.trim_unused(0),0);assert_eq!(cache.state.lock().unwrap().retired.len(),8);
        cache.trim_unused(2);assert_eq!(cache.state.lock().unwrap().retired.len(),6);
        cache.trim_unused(3);assert_eq!(cache.state.lock().unwrap().retired.len(),3);
        cache.trim_unused(3);assert_eq!(cache.state.lock().unwrap().retired.len(),0);
        assert_eq!(cache.stats().physical_bytes,0);
    }

    #[test]
    fn cache_keeps_exact_u16_samples_and_rejects_corrupt_tiles() {
        let color = DocumentColor {
            space: RgbSpace::ProPhoto,
            depth: SampleDepth::U16,
        };
        let bytes: Vec<_> = (0..65536u32)
            .flat_map(|i| {
                [i as u16, (i as u16).wrapping_mul(107), 17001, 49157]
                    .into_iter()
                    .flat_map(u16::to_le_bytes)
            })
            .collect();
        let good = Arc::new(TileBlob::encode(color.paint_descriptor(), &bytes).unwrap());
        let mut bad = TileBlob::encode(color.paint_descriptor(), &bytes).unwrap();
        bad.compressed = Arc::new(Arc::<[u8]>::from([0u8]).into());
        let cache = DecodedTileCache::new(bytes.len());
        assert_eq!(cache.decode_on_worker(&good).unwrap().as_slice(), bytes);
        assert!(cache.decode_on_worker(&Arc::new(bad)).is_err());
        assert_eq!(cache.decode_on_worker(&good).unwrap().as_slice(), bytes);
        let disabled = DecodedTileCache::new(0);
        assert_eq!(disabled.decode_on_worker(&good).unwrap().as_slice(), bytes);
        assert_eq!(disabled.stats().resident_bytes, 0);
    }

    #[test]
    fn distinct_owners_with_the_same_resource_id_have_independent_cache_entries() {
        let first = tile(23);
        let restored = Arc::new(TileBlob::from_package(first.resource_id(), first.descriptor, first.compressed_on_worker().unwrap().into_arc()).unwrap());
        assert_ne!(first.owner_identity(), restored.owner_identity());
        assert_eq!(first.owner_identity(), first.clone().owner_identity());
        let cache = DecodedTileCache::new(2 * 65536);
        let a = cache.decode_on_worker(&first).unwrap();
        let b = cache.decode_on_worker(&restored).unwrap();
        assert_eq!(a, b);
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(cache.stats().misses, 2);
        assert!(Arc::ptr_eq(&a, &cache.decode_on_worker(&first).unwrap()));
        let owner = Arc::downgrade(&first);
        drop(first);
        assert!(owner.upgrade().is_none());
    }

    #[test]
    fn concurrent_readers_share_retained_tiles_and_remain_correct_during_eviction() {
        let cache = Arc::new(DecodedTileCache::new(2 * 65536));
        let tiles: Vec<_> = (0..8).map(tile).collect();
        let barrier = Arc::new(std::sync::Barrier::new(4));
        std::thread::scope(|scope| {
            for offset in 0..4 {
                let cache = &cache;
                let tiles = &tiles;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    for i in 0..32 {
                        let index = (i + offset) % tiles.len();
                        let samples = cache.decode_on_worker(&tiles[index]).unwrap();
                        assert!(samples.iter().all(|&v| v == index as u8));
                        assert!(cache.stats().resident_bytes <= 2 * 65536);
                    }
                });
            }
        });
        let sample = cache.decode_on_worker(&tiles[0]).unwrap();
        assert!(Arc::ptr_eq(&sample, &cache.decode_on_worker(&tiles[0]).unwrap()));
        assert!(cache.stats().evictions > 0);
        assert!(cache.stats().peak_bytes <= 2 * 65536);
    }
}
