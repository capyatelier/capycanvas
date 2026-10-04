use super::*;
use crate::{effect_analysis::Prepared, effects::Gpu};
use layer_core::{ArtworkQuery, ArtworkSource};

impl SnapshotRenderer {
    pub(super) async fn prepare_effect_analysis_async(&mut self, output: scene::Output) -> Result<(), String> {
        self.check_cancelled().map_err(|e| e.to_string())?;
        let contributors = match output {
            scene::Output::EffectInput(id) | scene::Output::EffectChannels(id) => {
                let index = self.layers.iter().position(|layer| layer.id == id).ok_or("Missing analysis input")?;
                layer_core::composite_input_layers(&self.layers, index)
            }
            scene::Output::LayerContent(_) => Vec::new(),
            _ => (0..self.layers.len()).collect(),
        };
        let mut pending: Vec<_> = contributors.into_iter().filter(|&i| layer_core::layer_is_visible(&self.layers, self.layers[i].id)
            && self.layers[i].effect.as_ref().is_some_and(|effect| effect.program.analysis().is_some())
            && !self.analysis_ready.contains(&self.layers[i].id)).collect();
        while !pending.is_empty() {
            self.check_cancelled().map_err(|e| e.to_string())?;
            let next = pending.iter().position(|&i| {
                let input = layer_core::composite_input_layers(&self.layers, i);
                !pending.iter().any(|j| input.contains(j))
            }).ok_or("Cyclic effect analysis")?;
            let index = pending.remove(next);
            let layer = self.layers[index].id;
            let kind = self.layers[index].effect.as_ref().unwrap().program.analysis().unwrap();
            if self.renderer.effect_analyses.iter().any(|analysis| analysis.layer() == layer && analysis.kind == kind
                && analysis.matches(&self.document, self.time, &self.renderer)) {
                self.analysis_ready.insert(layer); continue;
            }
            let mut query = ArtworkQuery::from_snapshot(self.document.clone(), ArtworkSource::EffectInput(layer), self.time, Vec::new());
            let mut input = vec![false; self.layers.len()];
            for i in layer_core::composite_input_layers(&self.layers, index) {
                input[i] = true;
                if self.layers[i].effect.as_ref().is_some_and(|effect| effect.program.time) {
                    query.effect_times.push((self.layers[i].id, self.renderer.effect_time(&self.layers[i], self.time)));
                }
            }
            let visibility: Vec<_> = self.layers.iter().map(|layer| layer.visible).collect();
            for (i, layer) in self.layers.iter_mut().enumerate() {
                if !input[i] && layer.kind != LayerKind::Group { layer.visible = false; }
            }
            let result = self.build_tone_guide_async(scene::Output::EffectInput(layer)).await;
            for (layer, visible) in self.layers.iter_mut().zip(visibility) { layer.visible = visible; }
            let guide = result?;
            self.check_cancelled().map_err(|e| e.to_string())?;
            self.renderer.effect_analyses.retain(|analysis| analysis.layer() != layer);
            self.renderer.effect_analyses.push(Arc::new(Prepared { query, kind,
                resource: Arc::new(crate::effects::resources::Resource {buffer: guide.buffer.clone(), _analysis: guide.analysis_lease.clone()}) }));
            if let Some(scene) = &mut self.renderer.scene { scene.analysis_changed(); }
            self.analysis_ready.insert(layer);
        }
        Ok(())
    }
}

impl SnapshotGpu {
    pub(crate) async fn effect_analysis(&self, query: ArtworkQuery, control: CaptureControl)
        -> Result<crate::effect_analysis::Candidate, String> {
        control.check().map_err(|e| e.to_string())?;
        let ArtworkSource::EffectInput(target) = query.source else { return Err("Invalid effect analysis input".into()); };
        let (mut snapshot, _) = self.artwork_capture(&query, control.clone()).await?;
        let index = snapshot.layers.iter().position(|layer| layer.id == target).ok_or("Missing effect analysis target")?;
        snapshot.layers[index].visible = true;
        snapshot.prepare_effect_analysis_async(scene::Output::Artwork(None)).await?;
        control.check().map_err(|e| e.to_string())?;
        Ok(crate::effect_analysis::Candidate { entries: snapshot.renderer.effect_analyses.into_iter()
            .filter(|entry| snapshot.analysis_ready.contains(&entry.layer())).collect() })
    }
}

impl SnapshotGpu {
    pub(crate) async fn bake_analysis(&self, input: crate::effect_analysis::BakeInput, control: CaptureControl)
        -> Result<crate::effect_analysis::Candidate, String> {
        control.check().map_err(|e| e.to_string())?;
        let mut document = layer_core::Document::new("", input.extent[0], input.extent[1],
            layer_core::DocumentNames {paint: "".into(), paper: "".into()});
        document.layers = layer_core::bake_layers(&input.members, input.offset);
        document.color = input.color; document.blend_space = input.blend;
        if let Some(layer) = document.layers.first() { document.active_layer = layer.id; }
        let identity = Arc::new(document.clone());
        discard_hidden_backing(&mut document);
        #[cfg(target_arch = "wasm32")]
        if let Some(waiter) = &self.analysis_backing_waiter { waiter(Arc::new(document.clone()), control.clone()).await?; }
        let mut snapshot = SnapshotRenderer::construct(Project {document}, input.time, control.clone(), self).map_err(|e| e.to_string())?;
        snapshot.document = identity;
        snapshot.prepare_effect_analysis_async(scene::Output::Artwork(None)).await?;
        control.check().map_err(|e| e.to_string())?;
        Ok(crate::effect_analysis::Candidate {entries: snapshot.renderer.effect_analyses.into_iter()
            .filter(|entry| snapshot.analysis_ready.contains(&entry.layer())).collect()})
    }
}
