use layer_core::{BrushExecution, Document, RasterOperationKind, Point, Rect, StrokeId, raster::TILE_SIZE};
use layer_render::{Dab, DabBatch, DabBatchKind, MaterialWork};
use std::borrow::Cow;

pub(super) const MATERIAL_PAGE_JOBS_PER_FRAME: u32 = 512;
const PAGE_LANES: u64 = TILE_SIZE as u64 * TILE_SIZE as u64;
const RESERVOIR_LANES: u64 = 64 * 64;

#[derive(Clone, Copy, Default)]
pub(super) struct FrameSteps {
    batch: usize,
    cursor: Option<[u32; 2]>,
}

impl FrameSteps {
    const SIDE: u32 = 1024;

    pub fn new(document: &Document, batches: &[DabBatch], dabs: &[Dab]) -> Option<Self> {
        let bake = dabs.is_empty() && batches.iter().any(|batch| Self::is_bake(document, batch) && (
            batch.damage.max.x - batch.damage.min.x > Self::SIDE as f32
                || batch.damage.max.y - batch.damage.min.y > Self::SIDE as f32
        ));
        (bake || material_page_jobs(batches) > 2 * MATERIAL_PAGE_JOBS_PER_FRAME).then(Self::default)
    }

    fn is_bake(document: &Document, batch: &DabBatch) -> bool {
        let DabBatchKind::RasterOperation(index) = batch.kind else { return false; };
        document.target_operations(batch.target)
            .and_then(|operations| operations.get(index as usize)).is_some_and(|operation| matches!(operation.kind,
                RasterOperationKind::Bake { .. } | RasterOperationKind::FrequencyDetail { .. }))
    }
    fn following(end: usize, batches: &[DabBatch]) -> Option<Self> {
        (end < batches.len()).then_some(Self { batch: end, cursor: None })
    }

    pub fn next<'a>(self, document: &Document, batches: &'a [DabBatch]) -> (Cow<'a, [DabBatch]>, Option<Self>) {
        let mut batch = batches[self.batch].clone();
        if !Self::is_bake(document, &batch) {
            let end = self.window_end(batches);
            return (Cow::Borrowed(&batches[self.batch..end]), Self::following(end, batches));
        }
        let extent = document.target_extent(batch.target);
        let min = [batch.damage.min.x, batch.damage.min.y].map(|v| v.max(0.) as u32 / 256 * 256);
        let max = [batch.damage.max.x.ceil() as u32, batch.damage.max.y.ceil() as u32];
        let max = [max[0].div_ceil(256).saturating_mul(256).min(extent[0]), max[1].div_ceil(256).saturating_mul(256).min(extent[1])];
        let [x, y] = self.cursor.unwrap_or(min);
        let end = [x.saturating_add(Self::SIDE).min(max[0]), y.saturating_add(Self::SIDE).min(max[1])];
        batch.damage = Rect { min: Point { x: x as f32, y: y as f32 }, max: Point { x: end[0] as f32, y: end[1] as f32 } };
        let next = if end[0] < max[0] {
            Some(Self { cursor: Some([end[0], y]), ..self })
        } else if end[1] < max[1] {
            Some(Self { cursor: Some([min[0], end[1]]), ..self })
        } else { Self::following(self.batch + 1, batches) };
        (Cow::Owned(vec![batch]), next)
    }

    fn window_end(self, batches: &[DabBatch]) -> usize {
        if matches!(batches[self.batch].kind, DabBatchKind::RasterOperation(_)) {
            return self.batch + 1;
        }
        let mut jobs = 0;
        let mut end = self.batch;
        while let Some(batch) = batches.get(end).filter(|batch| !matches!(batch.kind, DabBatchKind::RasterOperation(_))) {
            let more = material_page_jobs(std::slice::from_ref(batch));
            if end > self.batch && jobs + more > MATERIAL_PAGE_JOBS_PER_FRAME && !same_watercolor_update(&batches[end - 1], batch) {
                break;
            }
            jobs += more;
            end += 1;
        }
        end
    }
}

pub(super) fn material_page_jobs(batches: &[DabBatch]) -> u32 {
    batches
        .iter()
        .filter(|batch| batch.kind == DabBatchKind::Persistent && batch.style.execution != BrushExecution::Dry
            && !batch.style.execution.retouches())
        .map(|batch| covered_tiles(batch.damage).saturating_mul(if batch.style.execution == BrushExecution::Smudge
            && batch.style.blender.kind == layer_core::BlenderKind::Dither {
                batch.dab_count.div_ceil(super::MAX_SMUDGE_DABS_PER_BATCH as u32)
            } else { 1 }))
        .sum()
}

pub(super) fn covered_tiles(rect: Rect) -> u32 {
    if rect.is_empty() {
        return 0;
    }
    let tile = layer_core::raster::TILE_SIZE as f32;
    let span = |min: f32, max: f32| ((max / tile).ceil() - (min.max(0.) / tile).floor()).max(0.) as u32;
    span(rect.min.x, rect.max.x) * span(rect.min.y, rect.max.y)
}

fn same_watercolor_update(earlier: &DabBatch, later: &DabBatch) -> bool {
    earlier.style.execution == BrushExecution::Watercolor
        && later.style.execution == BrushExecution::Watercolor
        && earlier.kind == later.kind
        && earlier.stroke_id == later.stroke_id
        && earlier.target == later.target
        && earlier.material_update == later.material_update
}

#[derive(Clone, Default)]
pub(super) struct WetPages {
    stroke: Option<StrokeId>,
    written: std::collections::HashMap<[u32; 2], (u8, [u32; 4])>,
}

impl WetPages {
    pub fn work(&mut self, document: &Document, batches: &[DabBatch]) -> MaterialWork {
        let mut work = MaterialWork::default();
        for batch in batches.iter().filter(|batch| batch.style.execution == BrushExecution::Wet && batch.dab_count > 0) {
            let dabs = u64::from(batch.dab_count);
            let pages = pages(batch.damage, document.target_extent(batch.target));
            match batch.kind {
                DabBatchKind::Persistent => {
                    if batch.stroke_start || self.stroke != Some(batch.stroke_id) {
                        self.written.clear();
                        self.stroke = Some(batch.stroke_id);
                    }
                    work.committed += RESERVOIR_LANES;
                    for (page, local) in pages {
                        let (visits, previous) = self.written.entry(page).or_insert((0, local));
                        let lanes = if *visits < 2 { PAGE_LANES } else { area(union(local, *previous)) };
                        work.committed += lanes + area(local) * dabs;
                        (*visits, *previous) = (visits.saturating_add(1), local);
                    }
                }
                DabBatchKind::Preview => work.prediction += pages.count() as u64 * PAGE_LANES * (1 + dabs),
                DabBatchKind::RasterOperation(_) => {}
            }
        }
        work
    }
}

fn pages(damage: Rect, [width, height]: [u32; 2]) -> impl Iterator<Item = ([u32; 2], [u32; 4])> {
    let clamp = |value: f32, side: u32| value.max(0.).min(side as f32) as u32;
    let rect = [clamp(damage.min.x.floor(), width), clamp(damage.min.y.floor(), height),
        clamp(damage.max.x.ceil(), width), clamp(damage.max.y.ceil(), height)];
    let empty = damage.is_empty() || rect[0] >= rect[2] || rect[1] >= rect[3];
    let (xs, ys) = if empty { (0..0, 0..0) } else { (rect[0] / TILE_SIZE..rect[2].div_ceil(TILE_SIZE), rect[1] / TILE_SIZE..rect[3].div_ceil(TILE_SIZE)) };
    ys.flat_map(move |y| xs.clone().map(move |x| ([x, y], [rect[0].max(x * TILE_SIZE), rect[1].max(y * TILE_SIZE),
        rect[2].min((x + 1) * TILE_SIZE), rect[3].min((y + 1) * TILE_SIZE)])))
}

fn area(rect: [u32; 4]) -> u64 { u64::from(rect[2] - rect[0]) * u64::from(rect[3] - rect[1]) }

fn union(a: [u32; 4], b: [u32; 4]) -> [u32; 4] { [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])] }
