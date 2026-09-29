use layer_core::{Document, LayerOperationKind, Point, Rect};
use layer_render::{DabBatch, DabBatchKind};

#[derive(Clone, Copy, Default)]
pub(super) struct BakeSteps {
    batch: usize,
    cursor: Option<[u32; 2]>,
}

impl BakeSteps {
    const SIDE: u32 = 1024;

    pub fn new(document: &Document, batches: &[DabBatch]) -> Option<Self> {
        (!batches.is_empty() && batches.iter().all(|batch| {
            let DabBatchKind::LayerOperation(index) = batch.kind else { return false };
            document.layer(batch.layer_id).is_some_and(|layer| {
                matches!(layer.pending_operations[index as usize].kind,
                    LayerOperationKind::Bake { .. } | LayerOperationKind::FrequencyDetail { .. })
            })
        }) && batches.iter().any(|batch| {
            batch.damage.max.x - batch.damage.min.x > Self::SIDE as f32
                || batch.damage.max.y - batch.damage.min.y > Self::SIDE as f32
        })).then(Self::default)
    }

    pub fn next(self, document: &Document, batches: &[DabBatch]) -> (DabBatch, Option<Self>) {
        let mut batch = batches[self.batch].clone();
        let extent = document.layer(batch.layer_id).unwrap().local_extent([document.width, document.height]);
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
        } else if self.batch + 1 < batches.len() {
            Some(Self { batch: self.batch + 1, cursor: None })
        } else { None };
        (batch, next)
    }
}
