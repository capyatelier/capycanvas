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
        let writes_full_page = batch.style.execution == BrushExecution::Dry || from_persistent;
        let layer_index = self
            .paint_layers
            .iter()
            .position(|layer| layer.id == batch.layer_id)
            .ok_or(GpuRasterError::MissingPaintLayer(batch.layer_id))?;
        if !preview
            && batch.stroke_start
            && plan.reservoir
            && let Some(first) = batch_dabs.first()
        {
            let amount = batch.style.wet_mix.amount_of_paint * first.material[2];
            let color = first.color_rgba_linear;
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer initialize brush reservoir"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.reservoir.active().view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: color[0] as f64,
                            g: color[1] as f64,
                            b: color[2] as f64,
                            a: amount as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        // Full-page dry evaluation writes the new coverage page too. An old
        // owner binds the shared zero scalar as its source, so no clear pass is
        // needed before the first contact of a stroke reaches this tile.
        if !preview && plan.state.coverage && !writes_full_page {
            for tile in tiles {
                let coordinate = tile.coordinate;
                let page = self.paint_layers[layer_index]
                    .coverage_pages
                    .iter_mut()
                    .find(|page| page.coordinate == coordinate)
                    .expect("stroke coverage page is prepared before encoding");
                if page.owner != Some(batch.stroke_id) {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("layer reset stroke coverage page"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &page.active().view,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    page.owner = Some(batch.stroke_id);
                }
            }
        }

        let mut jobs = mem::take(&mut self.material_jobs);
        let damage =
            batch_pixel_rect(batch, context.document_extent).intersect(self.preview_damage);
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
                    .position(|p| p.coordinate == coordinate)
                    .expect("material coverage prepared before encoding")
            });
            let coverage = coverage_index.map(|i| &coverage_pages[i]);
            if !writes_full_page {
                page.active()
                    .copy_to(page.surface(!page.active_secondary), encoder);
                if let Some(coverage) = coverage {
                    coverage.active().copy_to(
                        if coverage.active_secondary {
                            &coverage.primary
                        } else {
                            &coverage.secondary
                        },
                        encoder,
                    );
                }
            }
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
        self.prepare_dry_records(
            batch,
            jobs.iter().map(|j| (j.local, j.dabs.clone())),
            encoder,
        )?;
        let texture_key = Self::texture_set_key(&batch.style);
        let mut compute_jobs = mem::take(&mut self.dry_jobs);
        for (record_index, job) in jobs.iter().enumerate() {
            // A decoded original slot may be overwritten by the next source
            // preparation. Owned paint pages never have that eviction hazard.
            let borrowed = from_persistent
                && !self.paint_layers[layer_index]
                    .pages
                    .iter()
                    .any(|p| p.coordinate == job.coordinate)
                && (self.tiled_sources.contains_key(&batch.layer_id)
                    || self
                        .native_color_tile(batch.layer_id, job.coordinate)?
                        .is_some());
            if borrowed {
                self.encode_dry_material_jobs(encoder, batch_index, batch, &compute_jobs);
                compute_jobs.clear();
            }
            let source_bind_group = self.material_source_binding(
                batch_index,
                batch,
                batch_dabs,
                job.coordinate,
                preview && !from_persistent,
                encoder,
            )?;
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
            let coverage_view = coverage_surface.map(|p| &p.view);
            let record_offset = if batch.style.execution == BrushExecution::Dry {
                self.dry_records.offset(record_index)
            } else {
                0
            };
            let local = if from_persistent && !self.preview_full_pages {
                damage
                    .intersect(page_rect(job.coordinate))
                    .page_local(job.coordinate)
            } else if writes_full_page {
                PixelRect::full([PAGE_SIZE; 2])
            } else {
                job.local
            };
            if local.is_empty() {
                continue;
            }
            if self.compute_dry_material(batch) && local == PixelRect::full([PAGE_SIZE; 2]) {
                let output = self.dry_material_pipeline(batch).output(
                    self,
                    destination,
                    coverage_surface,
                );
                compute_jobs.push((
                    output,
                    source_bind_group,
                    job.coordinate,
                    coverage_view.is_some(),
                    record_offset,
                ));
                if borrowed {
                    self.encode_dry_material_jobs(encoder, batch_index, batch, &compute_jobs);
                    compute_jobs.clear();
                }
                continue;
            }
            let scalar_state_view = if from_persistent {
                None
            } else if plan.state.watercolor_wetness {
                let pages = if preview {
                    &self.preview_watercolor_wetness_pages
                } else {
                    &self.paint_layers[layer_index].watercolor_wetness_pages
                };
                pages
                    .iter()
                    .find(|p| p.coordinate == job.coordinate)
                    .map(|p| &p.inactive().view)
            } else if !preview {
                self.paint_layers[layer_index]
                    .material_pages
                    .iter()
                    .find(|p| p.coordinate == job.coordinate)
                    .map(|p| &p.wetness.view)
            } else {
                None
            };
            let portable_scalar = if self.device.portable_blend() {
                scalar_state_view.map(|view| {
                    self.portable_blend
                        .source(&self.device, view, wgpu::TextureFormat::R32Float)
                })
            } else {
                None
            };
            let color_load = if writes_full_page {
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
            } else {
                wgpu::LoadOp::Load
            };
            let attachment = |view, load| wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            };
            let color_attachments = [
                Some(attachment(&destination.view, color_load)),
                coverage_view.map(|v| attachment(v, color_load)),
                scalar_state_view.map(|v| {
                    attachment(
                        portable_scalar.as_ref().unwrap_or(v),
                        if portable_scalar.is_some() {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        } else {
                            wgpu::LoadOp::Load
                        },
                    )
                }),
            ];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer material tile"),
                color_attachments: &color_attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_scissor_rect(local.min_x(), local.min_y(), local.width(), local.height());
            pass.set_pipeline(self.pipelines.material(
                plan.material,
                !from_persistent && plan.state.watercolor_wetness,
                coverage_view.is_some(),
                scalar_state_view.is_some(),
            ));
            pass.set_bind_group(
                0,
                &self.style_bind_group,
                &[batch_index as u32 * self.style_stride as u32],
            );
            pass.set_bind_group(
                1,
                self.paint_target_binding(&batch.style),
                &[self.layer_target_offset(batch.layer_id, job.coordinate)],
            );
            pass.set_bind_group(2, &source_bind_group, &[record_offset]);
            pass.set_bind_group(
                3,
                &self
                    .texture_sets
                    .iter()
                    .find(|set| set.key == texture_key)
                    .unwrap()
                    .bind_group,
                &[],
            );
            pass.draw(0..3, 0..1);
            drop(pass);
            if let Some(source) = portable_scalar {
                self.portable_blend.apply(
                    &self.device,
                    encoder,
                    &source,
                    scalar_state_view.unwrap(),
                    local,
                    2,
                );
            }
        }
        self.encode_dry_material_jobs(encoder, batch_index, batch, &compute_jobs);
        compute_jobs.clear();
        self.dry_jobs = compute_jobs;
        // Reservoir exchange samples the immutable pre-batch canvas. Keep a
        // bind group to that generation before the page ping-pong state flips.
        let reservoir_exchange = if !preview && plan.reservoir {
            batch_dabs
                .last()
                .map(|last| {
                    let center = batch.style.brush_to_layer.map(last.center);
                    let coordinate = [
                        (center.x.max(0.0) as u32 / PAGE_SIZE).min(
                            self.target_extent(batch.layer_id)[0].saturating_sub(1) / PAGE_SIZE,
                        ),
                        (center.y.max(0.0) as u32 / PAGE_SIZE).min(
                            self.target_extent(batch.layer_id)[1].saturating_sub(1) / PAGE_SIZE,
                        ),
                    ];
                    self.material_bind_group(batch, coordinate, false, None, encoder)
                        .map(|bind_group| (coordinate, bind_group))
                })
                .transpose()?
        } else {
            None
        };

        let (pages, coverage_pages) = if preview {
            (&mut self.preview_pages, &mut self.preview_coverage_pages)
        } else {
            let layer = &mut self.paint_layers[layer_index];
            (&mut layer.pages, &mut layer.coverage_pages)
        };
        for job in &jobs {
            pages[job.page_index].active_secondary = job.destination_secondary;
            if let Some((i, secondary)) = job.coverage_index.zip(job.coverage_destination_secondary)
            {
                coverage_pages[i].active_secondary = secondary;
                coverage_pages[i].owner = Some(batch.stroke_id);
            }
        }
        jobs.clear();
        self.material_jobs = jobs;
        if let Some((coordinate, source_bind_group)) = reservoir_exchange {
            self.encode_reservoir_update(
                encoder,
                batch_index,
                batch,
                coordinate,
                &source_bind_group,
            )?;
        }
        Ok(())
    }
}
