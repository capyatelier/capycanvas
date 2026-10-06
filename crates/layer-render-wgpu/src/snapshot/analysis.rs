use super::*;
use crate::effect_analysis::Prepared;
use layer_core::{ArtworkQuery, ArtworkSource};

impl SnapshotRenderer {
    pub(super) async fn prepare_effect_analysis_async(&mut self, output: scene::Output) -> Result<(), String> {
        self.check_cancelled().map_err(|e| e.to_string())?;
        let scene = self.scene.view().with_scope(&self.scope).with_offset(self.offset);
        let contributors = if matches!(self.scope,SceneScope::Raw(_)) {Vec::new()} else {match output {
            scene::Output::EffectInput(id) | scene::Output::EffectChannels(id) | scene::Output::EffectComposite(id) => layer_core::composite_input_layers(scene, id),
            scene::Output::Source(_) => Vec::new(), _ => scene.effect_input().map_or_else(||scene.order().to_vec(),|h|layer_core::composite_input_layers(scene,h)),
        }};
        let mut pending: Vec<_> = contributors.into_iter().filter(|&h| scene.visible(h)
            && scene.effect(h).is_some_and(|effect| effect.program.analysis().is_some()) && !self.analysis_ready.contains(&h)).collect();
        while !pending.is_empty() {
            self.check_cancelled().map_err(|e| e.to_string())?;
            let next = pending.iter().position(|&h| {
                let input = layer_core::composite_input_layers(self.scene.view(), h);
                !pending.iter().any(|other| input.contains(other))
            }).ok_or("Cyclic effect analysis")?;
            let layer = pending.remove(next);
            let kind = self.scene.view().effect(layer).unwrap().program.analysis().unwrap();
            if self.renderer.effect_analyses.iter().any(|analysis| analysis.layer() == layer && analysis.kind == kind && analysis.matches(&self.scene, &self.renderer)) {
                self.analysis_ready.insert(layer); continue;
            }
            let query = ArtworkQuery::from_snapshot(self.scene.clone(), ArtworkSource::EffectInput(layer), None);
            let previous = std::mem::replace(&mut self.scope, SceneScope::EffectInput(layer));
            let output = scene::Output::EffectInput(layer);
            let result = self.build_effect_guide_async(kind, output).await;
            self.scope = previous;
            let resource = result?;
            self.check_cancelled().map_err(|e| e.to_string())?;
            self.renderer.effect_analyses.retain(|analysis| analysis.layer() != layer);
            self.renderer.effect_analyses.push(Arc::new(Prepared { query, kind,
                resource: Arc::new(resource) }));
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
        snapshot.scope = SceneScope::Members(layer_core::composite_input_layers(snapshot.scene.view(), target).into());
        snapshot.prepare_effect_analysis_async(scene::Output::Artwork(None)).await?;
        snapshot.scope = SceneScope::EffectInput(target);
        let kind = snapshot.scene.view().effect(target).unwrap().program.analysis().ok_or("Missing effect analysis")?;
        if !snapshot.renderer.effect_analyses.iter().any(|entry|
            entry.layer() == target && entry.kind == kind && entry.matches(&snapshot.scene, &snapshot.renderer)) {
            let resource = snapshot.build_effect_guide_async(kind, scene::Output::EffectInput(target)).await?;
            snapshot.renderer.effect_analyses.retain(|entry| entry.layer() != target);
            snapshot.renderer.effect_analyses.push(Arc::new(Prepared {query,kind,resource: Arc::new(resource)}));
        }
        snapshot.analysis_ready.insert(target);
        control.check().map_err(|e| e.to_string())?;
        Ok(crate::effect_analysis::Candidate { masks: None, entries: snapshot.renderer.effect_analyses.into_iter()
            .filter(|entry| snapshot.analysis_ready.contains(&entry.layer())).collect() })
    }
}

impl SnapshotGpu {
    pub(crate) async fn bake_analysis(&self, input: crate::effect_analysis::BakeInput, control: CaptureControl)
        -> Result<crate::effect_analysis::Candidate, String> {
        control.check().map_err(|e| e.to_string())?;
        #[cfg(target_arch = "wasm32")]
        if let Some(waiter) = &self.analysis_backing_waiter {
            let scene = input.scene.view().with_scope(&input.scope).with_offset(input.offset).snapshot(input.scene.context.clone());
            waiter(Arc::new(scene), control.clone()).await?;
        }
        let mut snapshot = SnapshotRenderer::construct(input.scene, input.scope, control.clone(), self).map_err(|e| e.to_string())?;
        snapshot.offset = input.offset;
        snapshot.extent = input.extent;
        snapshot.renderer.ensure_document_metadata(input.extent, snapshot.scene.view().with_scope(&snapshot.scope).with_offset(input.offset)).map_err(|e| e.to_string())?;
        snapshot.prepare_effect_analysis_async(scene::Output::Artwork(None)).await?;
        control.check().map_err(|e| e.to_string())?;
        let mut masks = crate::layer_masks::SnapshotMasks::default();
        let view = snapshot.scene.view().with_scope(&snapshot.scope).with_offset(snapshot.offset);
        let window = scene::Scene::capture_window(view, PixelRect::full(snapshot.extent), snapshot.extent);
        let mut regions = std::collections::HashMap::new();
        let mut count = 0_u64;
        for (&target, data) in snapshot.backing.iter().filter(|(target, _)| target.is_coverage()) {
            let SourceTarget::Coverage(handle) = target else { unreachable!() };
            let source = view.coverage(handle).unwrap();
            masks.definitions.insert(target, source.clone());
            let domain = source.domain;
            let region = window.translated(view.target_offset(target).map(|v| -v)).in_frame(domain).expand(1, domain);
            let mut needed = std::collections::BTreeSet::new();
            needed.extend(data.tiles.keys().filter(|key| !page_rect(key.coordinate).intersect(region).is_empty()).map(|key| key.coordinate));
            count += needed.len() as u64;
            regions.insert(target, region);
        }
        if count != 0 {
            let stride = u64::from(snapshot.renderer.device.scalar_format().block_copy_size(None).unwrap());
            let bytes = count.checked_mul(u64::from(PAGE_SIZE).pow(2) * stride).ok_or("Merge mask storage overflow")?;
            masks._lease = Some(Arc::new(crate::effect_analysis::Lease::reserve(&snapshot.renderer.device, bytes)?));
            let r = &mut snapshot.renderer;
            let mut encoder = crate::submission::CommandEncoder::new(&r.device, &Default::default());
            r.layer_masks.prepare_regions(&r.device, &mut encoder, (view, &[]), snapshot.extent, false,
                &mut r.selection_clip, Some(&regions)).map_err(|e| e.to_string())?;
            r.uploads.finish(&encoder);
            r.last_submission = Some(encoder.submit(&r.queue));
            for (&target, data) in snapshot.backing.iter().filter(|(target, _)| target.is_coverage()) {
                let region = regions[&target];
                for (key, tile) in data.tiles.iter().filter(|(key, _)| !page_rect(key.coordinate).intersect(region).is_empty()) {
                    control.check().map_err(|e| e.to_string())?;
                    let blob = tile.wait_backing_cancellable(control.cancellation_flag())?;
                    let page = crate::layer_masks::MaskPage::new(&r.device);
                    r.restore_native_scalars(&[crate::native_tiles::scalar::NativeScalarRestore {
                        blob: &blob, working: &page.texture,
                    }]).map_err(|e| e.to_string())?;
                    r.layer_masks.pages.insert((target, key.coordinate), page);
                }
            }
            masks.pages = std::mem::take(&mut r.layer_masks.pages);
        }
        control.check().map_err(|e| e.to_string())?;
        Ok(crate::effect_analysis::Candidate {masks: (!regions.is_empty()).then(|| Arc::new(masks)), entries: snapshot.renderer.effect_analyses.into_iter()
            .filter(|entry| snapshot.analysis_ready.contains(&entry.layer())).collect()})
    }
}
