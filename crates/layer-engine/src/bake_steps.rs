use layer_core::{Document, RasterOperationKind, Point, Rect};
use layer_render::{DabBatch, DabBatchKind};

#[derive(Clone, Copy, Default)]
pub(super) struct BakeSteps {
    batch: usize,
    cursor: Option<[u32; 2]>,
}

impl BakeSteps {
    const SIDE: u32 = 1024;

    pub fn new(document: &Document, batches: &[DabBatch]) -> Option<Self> {
        batches.iter().any(|batch| Self::is_bake(document, batch) && (
            batch.damage.max.x - batch.damage.min.x > Self::SIDE as f32
                || batch.damage.max.y - batch.damage.min.y > Self::SIDE as f32
        )).then(Self::default)
    }

    fn is_bake(document: &Document, batch: &DabBatch) -> bool {
        let DabBatchKind::RasterOperation(index) = batch.kind else { return false; };
        document.target_operations(batch.target)
            .and_then(|operations| operations.get(index as usize)).is_some_and(|operation| matches!(operation.kind,
                RasterOperationKind::Bake { .. } | RasterOperationKind::FrequencyDetail { .. }))
    }
    fn following(self, batches: &[DabBatch]) -> Option<Self> {
        (self.batch + 1 < batches.len()).then_some(Self { batch: self.batch + 1, cursor: None })
    }

    pub fn next(self, document: &Document, batches: &[DabBatch]) -> (DabBatch, Option<Self>) {
        let mut batch = batches[self.batch].clone();
        if !Self::is_bake(document, &batch) { return (batch, self.following(batches)); }
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
        } else { self.following(batches) };
        (batch, next)
    }
}
