//! One material tile executor for committed paint and disposable prediction.
//! Resource selection differs; initialization, evaluation and publication share
//! the same ordering contract. Nonlocal inputs remain immutable for a batch.
use super::*;

impl WgpuRasterizer {
    pub(super) fn encode_material_batch(
        &mut self,
        encoder: &mut crate::submission::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        context: BrushEncodingContext<'_>,
    ) -> Result<(), GpuRasterError> {
        let tiles = context.tiles;
        let batch_dabs =
            &context.dabs[batch.first_dab as usize..(batch.first_dab + batch.dab_count) as usize];
        let preview = context.target.is_preview();
        let in_place = self.in_place_dry_material(batch);
        let from_persistent = context.target
            == (BrushEncodingTarget::Preview {
                from_persistent: true,
            });
        let plan = BrushPassPlan::for_device(&batch.style, &self.device);
        let transported = destination_material::transported(&batch.style);
        let layer_index = self
            .paint_layers
            .iter()
            .position(|layer| layer.id == batch.target)
            .ok_or(GpuRasterError::MissingPaintLayer(batch.target))?;
        if !preview && batch.stroke_start {
            for layer in &mut self.paint_layers {
                for page in &mut layer.pages { page.clear_stroke_start(); }
            }
        }

        let mut jobs = mem::take(&mut self.material_jobs);
        let damage =
            batch_pixel_rect(batch, context.target_extent).intersect(self.preview_damage);
        // Only non-sparse direct predictions need the rectangular plan. Dry
        // contacts already have a precise tile list shared with committed paint.
        let rectangular = from_persistent && self.preview_contact_tiles.is_none();
        let mut add_job = |coordinate, local, dabs| {
            let (pages, coverage_pages) = if preview {
                (&self.preview_pages, &self.preview_coverage_pages)
            } else {
                (
                    &self.paint_layers[layer_index].pages,
                    &self.paint_layers[layer_index].coverage_pages,
                )
            };
            let page_index = pages
                .iter()
                .position(|p| p.coordinate == coordinate)
                .expect("material destination prepared before encoding");
            let page = &pages[page_index];
            let coverage_index = (!from_persistent && plan.state.coverage).then(|| {
                coverage_pages
                    .iter()
                    .position(|p| p.matches(coordinate, &batch.style))
                    .expect("material coverage prepared before encoding")
            });
            let coverage = coverage_index.map(|i| &coverage_pages[i]);
            jobs.push(MaterialJob {
                coordinate,
                local,
                dabs,
                page_index,
                coverage_index,
                destination_secondary: if in_place { page.active_secondary }
                    else { !from_persistent && !page.active_secondary },
                coverage_destination_secondary: coverage.map(|p| !p.active_secondary),
            });
        };
        if rectangular {
            for coordinate in page_coordinates(damage) {
                let tile = tiles.iter().find(|t| t.coordinate == coordinate);
                add_job(
                    coordinate,
                    PixelRect::full([PAGE_SIZE; 2]),
                    tile.map_or(batch.first_dab..batch.first_dab, |t| t.dabs.clone()),
                );
            }
        } else {
            for tile in tiles {
                add_job(tile.coordinate, tile.local, tile.dabs.clone());
            }
        }
        let retouches = batch.style.execution.retouches();
        let mappings = if retouches {
            jobs.iter().map(|job| self.retouch_mapping(batch, job.coordinate, job.local)).collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        let first_record = if pointwise(&batch.style) {
            self.prepare_records(jobs.iter().enumerate()
                .map(|(i, j)| material_sources::dry_record(j.local, j.dabs.clone(), mappings.get(i).map_or([0; 16], |m| m.words))), encoder)?
        } else { 0 };
        if transported {
            self.encode_transport_jobs(encoder, batch_index, batch, batch_dabs, &jobs, layer_index, preview, preview && !from_persistent)?;
        }
        let gathers = self.dry_display_gathers(batch);
        if gathers {
            while self.dry_display_inputs.len() < jobs.len().min(dry_material::DISPLAY_INPUT_SETS) {
                let inputs = dry_material::DisplayInputs::new(&self.device);
                self.dry_display_inputs.push(inputs);
            }
        }
        let mut compute_jobs = mem::take(&mut self.dry_jobs);
        for (record_index, job) in jobs.iter().enumerate().filter(|_| !transported) {
            // A decoded original slot may be overwritten by the next source
            // preparation. Owned paint pages never have that eviction hazard.
            let borrowed = from_persistent && !self.compact_preview_contribution(batch)
                && !self.paint_layers[layer_index]
                    .pages
                    .iter()
                    .any(|p| p.coordinate == job.coordinate)
                && (self.tiled_sources.contains_key(&batch.target)
                    || self
                        .native_color_tile(batch.target, job.coordinate)?
                        .is_some());
            if borrowed || (retouches && !self.retouch_resident(&mappings[record_index]))
                || (gathers && compute_jobs.len() == dry_material::DISPLAY_INPUT_SETS) {
                self.encode_dry_material_jobs(encoder, batch_index, batch, &compute_jobs);
                compute_jobs.clear();
            }
            let display_slot = gathers.then_some(compute_jobs.len());
            let (source_bind_group, display) = if retouches {
                self.clone_source_binding(batch, job.coordinate, &mappings[record_index], preview && !from_persistent, display_slot, encoder)?
            } else {
                self.material_source_binding(batch, job.coordinate, preview && !from_persistent, display_slot, encoder)?
            };
            let (pages, coverage_pages) = if preview {
                (&self.preview_pages, &self.preview_coverage_pages)
            } else {
                (
                    &self.paint_layers[layer_index].pages,
                    &self.paint_layers[layer_index].coverage_pages,
                )
            };
            let destination = pages[job.page_index].surface(job.destination_secondary);
            let coverage_surface = job
                .coverage_index
                .zip(job.coverage_destination_secondary)
                .map(|(i, secondary)| {
                    let coverage = &coverage_pages[i];
                    if secondary {
                        &coverage.secondary
                    } else {
                        &coverage.primary
                    }
                });
            let pipelines = &self.pipelines.dry_material;
            let tracked = in_place || (self.compact_preview_contribution(batch) && pipelines.in_place()
                && self.changed_cells.as_ref().unwrap().preview_reusable(self,batch.target,job.coordinate,destination));
            compute_jobs.push(dry_material::Job {
                output: pipelines.output(self, destination, coverage_surface, tracked.then_some((batch.target, job.coordinate))),
                source: source_bind_group,
                display: display_slot.zip(display).map(|(slot, inputs)| dry_material::DisplayJob {
                    gather: self.dry_display_inputs[slot].gather(self, destination), inputs,
                }),
                coordinate: job.coordinate,
                coverage: coverage_surface.is_some(),
                record_offset: self.material_records.offset(first_record + record_index),
                dabs: job.dabs.clone(),
                dirty: job.local,
            });
            destination.preview.set(None);
            if borrowed {
                self.encode_dry_material_jobs(encoder, batch_index, batch, &compute_jobs);
                compute_jobs.clear();
            }
        }
        self.encode_dry_material_jobs(encoder, batch_index, batch, &compute_jobs);
        compute_jobs.clear();
        self.dry_jobs = compute_jobs;

        let contribution=self.compact_preview_contribution(batch);
        let (pages, coverage_pages) = if preview {
            (&mut self.preview_pages, &mut self.preview_coverage_pages)
        } else {
            let layer = &mut self.paint_layers[layer_index];
            (&mut layer.pages, &mut layer.coverage_pages)
        };
        for job in &jobs {
            pages[job.page_index].active_secondary = job.destination_secondary;
            if preview && contribution && !job.destination_secondary {
                pages[job.page_index].primary.preview.set(Some((batch.target,job.coordinate)));
            }
            if let Some((i, secondary)) = job.coverage_index.zip(job.coverage_destination_secondary)
            {
                coverage_pages[i].active_secondary = secondary;
                coverage_pages[i].owner = Some(batch.stroke_id);
            }
        }
        jobs.clear();
        self.material_jobs = jobs;
        Ok(())
    }
}
