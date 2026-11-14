//! One material tile executor for committed paint and disposable prediction.
//! Resource selection differs; initialization, evaluation and publication share
//! the same ordering contract. Nonlocal inputs remain immutable for a batch.
use super::*;

impl WgpuRasterizer {
    pub(super) fn publish_material(&mut self,batch:&DabBatch,publications:Vec<MaterialWritten>)->Result<(),GpuRasterError> {
        for publication in publications {
            let written=publication.written;
            let state=MaterialState {stroke:batch.stroke_id,input:publication.input,version:written.version().clone()};
            match publication.kind {
                MaterialOutputKind::Color=> {
                    if let Some(ticket)=publication.preview {
                        self.preview_pages.iter_mut().find(|page|page.coordinate==publication.coordinate).ok_or(GpuRasterError::Invariant("prepared publish material state is inconsistent"))?.derived=Some(ticket);
                    }else {self.record_raster_write(batch.target,written)?;}
                },
                MaterialOutputKind::Coverage=> {
                    let pages=if publication.preview.is_some() {&mut self.preview_coverage_pages}else {
                        &mut self.paint_layers.iter_mut().find(|layer|layer.id==batch.target).ok_or(GpuRasterError::MissingPaintLayer(batch.target))?.coverage_pages};
                    pages.iter_mut().find(|page|page.coordinate==publication.coordinate).ok_or(GpuRasterError::Invariant("prepared publish material state is inconsistent"))?.state=Some(state);
                },
                MaterialOutputKind::Reservoir=>self.reservoir.state=Some(state),
                MaterialOutputKind::Wetness=> {
                    let pages=if publication.preview.is_some() {&mut self.preview_watercolor_wetness_pages}else {
                        &mut self.paint_layers.iter_mut().find(|layer|layer.id==batch.target).ok_or(GpuRasterError::MissingPaintLayer(batch.target))?.watercolor_wetness_pages};
                    pages.iter_mut().find(|page|page.coordinate==publication.coordinate).ok_or(GpuRasterError::Invariant("prepared publish material state is inconsistent"))?.pending=Some((state,written,publication.preview));
                },
            }
        }
        Ok(())
    }

    pub(super) fn encode_material_batch(
        &mut self,
        encoder: &mut crate::submission::CommandEncoder,
        batch_index: usize,
        batch: &DabBatch,
        context: BrushEncodingContext<'_>,
    ) -> Result<(), GpuRasterError> {
        let tiles = self.accepted_material_tiles(batch,context)?;
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
        if self.clears_stroke_starts(batch, &tiles, context.target_extent) {
            for layer in &mut self.paint_layers {
                for page in &mut layer.pages { page.clear_stroke_start(); }
            }
        }

        let mut jobs = mem::take(&mut self.material_jobs);
        let mut add_job = |coordinate, local, dabs| -> Result<(), GpuRasterError> {
            if !from_persistent && plan.state.coverage {
                let output=if preview {RasterOutput::PreviewCoverage(coordinate,stroke_coverage_slabs(&batch.style),stable_blender(&batch.style))}
                    else {RasterOutput::Coverage(coordinate,stroke_coverage_slabs(&batch.style),stable_blender(&batch.style))};
                self.accepted_raster_output(batch.target,output)?;
            }
            if !in_place && !from_persistent {
                self.accepted_raster_output(batch.target,if preview {RasterOutput::PreviewCompanion(coordinate)}else {RasterOutput::Companion(coordinate)})?;
            }
            if plan.state.watercolor_wetness {
                self.accepted_raster_output(batch.target,if preview {RasterOutput::PreviewWetness(coordinate)}else {RasterOutput::Wetness(coordinate)})?;
            }
            let (pages, coverage_pages) = if preview {
                (&self.preview_pages, &self.preview_coverage_pages)
            } else {
                (
                    &self.paint_layers[layer_index].pages,
                    &self.paint_layers[layer_index].coverage_pages,
                )
            };
            let page_index = prepared_raster_output(pages.iter().position(|p| p.coordinate == coordinate))?;
            let page = &pages[page_index];
            let coverage_index = if !from_persistent && plan.state.coverage {
                Some(prepared_raster_output(coverage_pages.iter().position(|p| p.matches(coordinate, &batch.style)))?)
            } else { None };
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
            Ok(())
        };
        for tile in tiles.iter() {
            add_job(tile.coordinate, tile.local, tile.dabs.clone())?;
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
            self.encode_transport_jobs(encoder, batch_index, batch, batch_dabs, &jobs, layer_index, context.target)?;
        }
        let gathers = self.dry_display_gathers(batch);
        if gathers {
            while self.dry_display_inputs.len() < jobs.len().min(dry_material::DISPLAY_INPUT_SETS) {
                let inputs = dry_material::DisplayInputs::new(&self.device);
                self.dry_display_inputs.push(inputs);
            }
        }
        let mut publications=Vec::with_capacity(jobs.len()*2);
        let mut written=Vec::with_capacity(jobs.len()*2);
        let mut compute_jobs = mem::take(&mut self.dry_jobs);
        for (record_index, job) in jobs.iter().enumerate().filter(|_| !transported) {
            if gathers && compute_jobs.len()==dry_material::DISPLAY_INPUT_SETS {
                self.encode_dry_material_jobs(encoder, batch_index, batch, &compute_jobs);
                let receipt=encoder.submission();written.extend(publications.drain(..).map(|publication:MaterialPublication|publication.encoded(&receipt)));
                compute_jobs.clear();
            }
            let display_slot = gathers.then_some(compute_jobs.len());
            let intent=self.raster_intent(batch.target,RasterPlane::Color,job.coordinate)?;
            let (source_bind_group, display) = if retouches {
                self.clone_source_binding(batch,batch_index, job.coordinate, &mappings[record_index], context.target, &intent, display_slot, encoder)?
            } else {
                self.material_source_binding(batch,batch_index, job.coordinate, context.target, &intent, display_slot, encoder)?
            };
            let receipt=encoder.submission();
            let color_ticket=preview.then(||self.preview_material_version(batch,batch_index,RasterPlane::Color,job.coordinate,scene::PreviewPass::Color,&intent)).transpose()?;
            let coverage_ticket=(preview && job.coverage_index.is_some()).then(||self.preview_material_version(batch,batch_index,RasterPlane::Color,job.coordinate,scene::PreviewPass::Coverage,&intent)).transpose()?;
            let pipelines=&self.pipelines.dry_material;
            let page=if preview {&self.preview_pages[job.page_index]}else {&self.paint_layers[layer_index].pages[job.page_index]};
            let tracked=in_place || (self.compact_preview_contribution(batch) && pipelines.in_place()
                && page.derived.as_ref().is_some_and(|state|state.input().same_selection(&intent))
                && self.changed_cells.as_ref().unwrap().preview_reusable(self,batch.target,job.coordinate,page.surface(job.destination_secondary)));
            let changed=pipelines.in_place().then(|| {
                let cells=self.changed_cells.as_ref().unwrap();if tracked {cells.buffer(batch.target,job.coordinate).unwrap_or(&cells.disabled)}else {&cells.disabled}
            });
            let (pages,coverage_pages)=if preview {(&mut self.preview_pages,&mut self.preview_coverage_pages)}else {
                let layer=&mut self.paint_layers[layer_index];(&mut layer.pages,&mut layer.coverage_pages)};
            let page=&mut pages[job.page_index];
            let write=if preview {page.surface_mut(job.destination_secondary).overwrite(&receipt)}
                else {page.write(job.destination_secondary,&intent,&receipt)}.ok_or(GpuRasterError::Invariant("accepted destination is not writable"))?;
            let coverage=job.coverage_index.zip(job.coverage_destination_secondary).map(|(index,secondary)| {
                let page=&mut coverage_pages[index];let destination=if secondary {&mut page.secondary}else {&mut page.primary};
                destination.overwrite(&receipt).ok_or(GpuRasterError::Invariant("accepted destination is not writable"))
            }).transpose()?;
            let output=pipelines.output(&self.device,&self.style_buffer,&write,coverage.as_ref(),changed);
            let display=display_slot.zip(display).map(|(slot,inputs)|dry_material::DisplayJob {
                gather:self.dry_display_inputs[slot].gather(&self.device,&self.style_buffer,pipelines.in_place(),pipelines.gather_layout(),&write),inputs,
            });
            write.value().preview.set(None);
            publications.push(MaterialPublication {kind:MaterialOutputKind::Color,coordinate:job.coordinate,preview:color_ticket.clone(),input:intent.clone(),
                encoding:write.encoded(color_ticket.as_ref().map_or_else(||layer_core::raster::ContentName::fresh().into(),|ticket|ticket.version().clone()))});
            if let Some(coverage)=coverage {publications.push(MaterialPublication {kind:MaterialOutputKind::Coverage,coordinate:job.coordinate,preview:coverage_ticket.clone(),
                input:intent,encoding:coverage.encoded(coverage_ticket.as_ref().map_or_else(||layer_core::raster::ContentName::fresh().into(),|ticket|ticket.version().clone()))});}
            compute_jobs.push(dry_material::Job {output,source:source_bind_group,display,coordinate:job.coordinate,
                coverage:job.coverage_index.is_some(),record_offset:self.material_records.offset(first_record+record_index),
                dabs:job.dabs.clone(),dirty:job.local});

        }
        self.encode_dry_material_jobs(encoder, batch_index, batch, &compute_jobs);
                let receipt=encoder.submission();written.extend(publications.drain(..).map(|publication:MaterialPublication|publication.encoded(&receipt)));
        compute_jobs.clear();
        self.dry_jobs = compute_jobs;

        self.publish_material(batch,written)?;
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
                pages[job.page_index].primary.set_preview(Some((batch.target,job.coordinate)));
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
