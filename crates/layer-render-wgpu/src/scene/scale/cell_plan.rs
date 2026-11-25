use super::*;
use std::collections::BTreeMap;
use graph::Expression;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Resolution { Level, Native, Display }

pub(super) fn resolution(scene: SceneView<'_>, chain: &[OccurrenceHandle], level: u32, at_rest: bool) -> Resolution {
    let program = |handle: &OccurrenceHandle| scene.effect(*handle).unwrap().program;
    if chain.iter().any(|handle| native_pointwise_alpha(program(handle))) && (level > 0 || !chain.iter().any(|handle| program(handle).fusion_boundary())) {
        return Resolution::Native;
    }
    if level == 0 || !chain.iter().any(|handle| program(handle).resolution == layer_core::EffectResolution::Native) { return Resolution::Level; }
    if at_rest { Resolution::Native } else { Resolution::Display }
}

fn chain_resolution(scene: SceneView<'_>, chain: &[(OccurrenceHandle, u64, u32)], level: u32, at_rest: bool) -> Resolution {
    resolution(scene, &chain.iter().map(|(handle, ..)| *handle).collect::<Vec<_>>(), level, at_rest)
}

#[derive(Default)]
pub(super) struct Footprint { pub reduced: BTreeMap<SourceTarget, Vec<DocRect>>, pub native: BTreeMap<SourceTarget, Vec<DocRect>> }
impl Footprint {
    pub(super) fn domains(&self) -> [BTreeSet<SourceTarget>; 2] { [self.reduced.keys().copied().collect(), self.native.keys().copied().collect()] }
    pub(super) fn approximate(node: &Expression, scene: SceneView<'_>, level: u32, at_rest: bool) -> bool { Self::resolves(node, scene, level, at_rest, Resolution::Display) }
    pub(super) fn captures(node: &Expression, scene: SceneView<'_>, level: u32, at_rest: bool) -> bool { Self::resolves(node, scene, level, at_rest, Resolution::Native) }
    fn resolves(node: &Expression, scene: SceneView<'_>, level: u32, at_rest: bool, wanted: Resolution) -> bool {
        match node {
            Expression::Opacity { input, .. } => Self::resolves(input, scene, level, at_rest, wanted),
            Expression::Combine { front, back, .. } => Self::resolves(front, scene, level, at_rest, wanted) || Self::resolves(back, scene, level, at_rest, wanted),
            Expression::Effect { input, chain, masks, .. } => chain_resolution(scene, chain, level, at_rest) == wanted
                || Self::resolves(input, scene, level, at_rest, wanted) || masks.iter().flatten().any(|mask| Self::resolves(mask, scene, level, at_rest, wanted)),
            _ => false,
        }
    }
    pub(super) fn of(node: &Expression, scene: SceneView<'_>, region: impl Into<Damage>, extent: [u32; 2], level: u32, at_rest: bool) -> Self {
        let mut footprint = Self::default();
        footprint.visit(node, scene, region.into().intersect(PixelRect::full(extent)).regions.into_iter().map(DocRect::from).collect(), extent, level, at_rest, false);
        footprint
    }
    #[expect(clippy::too_many_arguments, reason = "The walk carries the region, grid and domain of each node it visits")]
    fn visit(&mut self, node: &Expression, scene: SceneView<'_>, region: Vec<DocRect>, extent: [u32; 2], level: u32, at_rest: bool, native: bool) {
        if region.iter().all(|area| area.is_empty()) { return; }
        match node {
            Expression::Source { id, .. } => {
                let domain = if native { &mut self.native } else { &mut self.reduced };
                let areas = domain.entry(*id).or_default();
                for area in region { if !area.is_empty() && !areas.contains(&area) { areas.push(area); } }
            }
            Expression::Opacity { input, .. } => self.visit(input, scene, region, extent, level, at_rest, native),
            Expression::Combine { front, back, .. } => {
                self.visit(front, scene, region.clone(), extent, level, at_rest, native);
                self.visit(back, scene, region, extent, level, at_rest, native);
            }
            Expression::Effect { input, chain, masks, .. } => {
                let captured = chain_resolution(scene, chain, level, at_rest) == Resolution::Native;
                let grid = if captured { 0 } else { level };
                let output: Vec<DocRect> = if captured { region.iter().map(|area| area.aligned(1 << level)).collect() } else { region };
                let spatial: Vec<_> = chain.iter().map(|(handle, ..)| scene.effect(*handle).unwrap()).filter(|effect| !effect.program.passes.is_empty()).collect();
                let reads = if spatial.is_empty() { output.clone() } else {
                    let support = input.support(scene, extent, level);
                    output.iter().map(|area| spatial.iter().fold(*area, |area, effect| crate::effects::document_pass_regions(*effect, area, support, grid)[0])).collect()
                };
                self.visit(input, scene, reads, extent, level, at_rest, native || captured);
                for mask in masks.iter().flatten() { self.visit(mask, scene, output.clone(), extent, level, at_rest, native || captured); }
            }
            Expression::Color(_) | Expression::Objects { .. } | Expression::Vector { .. } | Expression::VectorPreview { .. } | Expression::VectorMotion { .. } => {}
        }
    }
}

#[cfg(test)]
pub(super) fn level_texels(local: PixelRect, level: u32) -> PixelRect {
    let side = 1 << level;
    PixelRect::new(local.min_x() / side, local.min_y() / side, local.max_x().div_ceil(side), local.max_y().div_ceil(side))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Write { pub page: [u32; 2], pub source: SourceTarget, pub coordinate: [u32; 2] }

pub(super) struct Planned { pub id: SourceTarget, pub frame: SourceFrame, pub required: Damage }

pub(super) fn writes(planned: &[Planned]) -> impl Iterator<Item = Write> {
    let mut writes: Vec<_> = planned.iter().flat_map(|source| page_coordinates(source.required.bounds())
        .filter(|coordinate| source.required.intersects(page_rect(*coordinate)))
        .map(|coordinate| Write { page: source.frame.document(page_rect(coordinate)).min.map(|value| value.clamp(0, i64::from(u32::MAX)) as u32 / PAGE_SIZE), source: source.id, coordinate }))
        .collect();
    writes.sort_by_key(|write| [write.page[1], write.page[0]]);
    writes.into_iter()
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Read { Tiles, Uniform }

#[cfg(test)]
impl Write {
    pub(super) fn texels(&self, planned: &[Planned], level: u32) -> PixelRect {
        level_texels(planned.iter().find(|source| source.id == self.source).unwrap().required.intersect(page_rect(self.coordinate)).bounds(), level)
    }
    pub(super) fn read(&self, occupied: &dyn Fn(SourceTarget, [u32; 2]) -> bool) -> Read {
        if occupied(self.source, self.coordinate) { Read::Tiles } else { Read::Uniform }
    }
}

#[cfg(test)]
pub(super) fn stripes(cell: PixelRect, level: u32, fits: &mut impl FnMut(PixelRect) -> bool) -> Option<Vec<PixelRect>> {
    if fits(cell) { return Some(vec![cell]); }
    let texels = level_texels(cell, level);
    let horizontal = texels.width() >= texels.height();
    let span = if horizontal { texels.width() } else { texels.height() };
    if span < 2 { return None; }
    let middle = (if horizontal { texels.min_x() } else { texels.min_y() } + span / 2) << level;
    let (first, second) = if horizontal {
        (PixelRect::new(cell.min_x(), cell.min_y(), middle, cell.max_y()), PixelRect::new(middle, cell.min_y(), cell.max_x(), cell.max_y()))
    } else {
        (PixelRect::new(cell.min_x(), cell.min_y(), cell.max_x(), middle), PixelRect::new(cell.min_x(), middle, cell.max_x(), cell.max_y()))
    };
    let mut parts = stripes(first, level, fits)?;
    parts.extend(stripes(second, level, fits)?);
    Some(parts)
}

pub(super) fn view_grid(transform: [f32; 6], mut fits_device: impl FnMut(u32) -> bool) -> Option<u32> {
    let normal = display_mips::view_level(transform, 4)?;
    let coarsest = display_mips::view_level(transform, display_mips::MAX_LEVEL)?;
    (normal..=coarsest.max(normal)).find(|level| fits_device(*level))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{document_at, effect_occurrence, insert_occurrence, paint_occurrence, coverage_mask};
    use layer_core::color::source::rgba8_source;
    use layer_core::Document;
    use layer_core::authored::OccurrenceContent;
    use crate::test_support::packet;
    use std::collections::HashSet;

    fn root(doc: &Document, level: u32) -> graph::Node {
        graph::compose(packet(doc.scene(), doc.composition().size), None, None, level, layer_core::color::RgbSpace::Srgb, None, None).unwrap()
    }
    fn effect(doc: &mut Document, id: &str) -> OccurrenceHandle {
        let handle = effect_occurrence(doc, layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get(id).unwrap().program()), id);
        insert_occurrence(doc, handle, 0);
        handle
    }
    fn ink(doc: &Document) -> SourceTarget { doc.scene().source_target(*doc.scene().order().last().unwrap()).unwrap() }
    fn declared(doc: &Document, footprint: &BTreeMap<SourceTarget, Vec<DocRect>>) -> Vec<Planned> {
        let scene = doc.scene();
        scene.order().iter().rev().flat_map(|&handle| scene.source_target(handle).into_iter().chain(scene.mask(handle).map(|(mask, _)| SourceTarget::Coverage(mask.source))))
            .filter_map(|id| footprint.get(&id).map(|required| {
                let frame = source_frame(None, scene, id);
                Planned { id, frame, required: Damage::from_regions(required.iter().map(|area| frame.local(*area, scene.target_extent(id)))) }
            }))
            .collect()
    }
    fn bounds(areas: &[DocRect]) -> DocRect { areas.iter().fold(DocRect::default(), |bounds, area| bounds.union(*area)) }

    #[test]
    fn a_native_effect_below_full_size_captures_at_rest_and_approximates_during_a_gesture() {
        let mut doc = document_at([600, 400]);
        let crystallize = effect(&mut doc, "crystallize");
        let scene = doc.scene();
        assert_eq!(resolution(scene, &[crystallize], 0, true), Resolution::Level);
        assert_eq!(resolution(scene, &[crystallize], 0, false), Resolution::Level);
        for level in [1, 3] {
            assert_eq!(resolution(scene, &[crystallize], level, true), Resolution::Native);
            assert_eq!(resolution(scene, &[crystallize], level, false), Resolution::Display);
        }
        let node = root(&doc, 2);
        assert!(!Footprint::approximate(&node, scene, 2, true));
        assert!(Footprint::approximate(&node, scene, 2, false));
        assert!(!Footprint::approximate(&node, scene, 0, false));
    }

    #[test]
    fn pointwise_alpha_conversions_stay_native_and_display_effects_stay_on_the_level() {
        let mut doc = document_at([600, 400]);
        let threshold = effect(&mut doc, "threshold");
        let blur = effect(&mut doc, "gaussian_blur");
        let scene = doc.scene();
        for at_rest in [true, false] {
            assert_eq!(resolution(scene, &[threshold], 0, at_rest), Resolution::Native);
            assert_eq!(resolution(scene, &[threshold], 2, at_rest), Resolution::Native);
            for level in [0, 2] { assert_eq!(resolution(scene, &[blur], level, at_rest), Resolution::Level); }
        }
        let mut imported = (*layer_core::bundled_effect_catalog().get("threshold").unwrap().program()).clone();
        imported.id = "imported_threshold".into();
        let mut doc = document_at([600, 400]);
        let handle = effect_occurrence(&mut doc, layer_core::EffectInstance::new(Arc::new(imported)), "imported");
        insert_occurrence(&mut doc, handle, 0);
        assert!(doc.scene().effect(handle).unwrap().program.fusion_boundary());
        assert_eq!(resolution(doc.scene(), &[handle], 0, true), Resolution::Level);
        assert_eq!(resolution(doc.scene(), &[handle], 1, true), Resolution::Native);
    }

    #[test]
    fn the_footprint_expands_a_native_effect_on_the_native_grid_and_names_its_sources_native() {
        let extent = [2048, 1024];
        let mut doc = document_at(extent);
        let crystallize = effect(&mut doc, "crystallize");
        let level = 2;
        let cell = PixelRect::new(1024, 0, 2048, 1024);
        let node = root(&doc, level);
        let scene = doc.scene();
        let native = crate::effects::damage_radius(scene.effect(crystallize).unwrap(), 0).unwrap();
        let reduced = crate::effects::damage_radius(scene.effect(crystallize).unwrap(), level).unwrap();
        assert!(native < reduced, "the level grid rounds each pass up by a level texel: {native} vs {reduced}");
        let at_rest = Footprint::of(&node, scene, cell, extent, level, true);
        assert!(at_rest.reduced.is_empty());
        assert_eq!(bounds(&at_rest.native[&ink(&doc)]), DocRect::from(cell.expand(native, extent)));
        let moving = Footprint::of(&node, scene, cell, extent, level, false);
        assert!(moving.native.is_empty());
        assert_eq!(bounds(&moving.reduced[&ink(&doc)]), DocRect::from(cell.expand(reduced, extent)));
        let full = Footprint::of(&node, scene, cell, extent, 0, true);
        assert_eq!(full.domains(), [BTreeSet::from([ink(&doc)]), BTreeSet::new()]);
        assert_eq!(bounds(&full.reduced[&ink(&doc)]), DocRect::from(cell.expand(native, extent)));
    }

    #[test]
    fn a_source_read_by_a_native_effect_and_by_its_surroundings_needs_both_representations() {
        let extent = [1024, 512];
        let build = |attachment: layer_core::authored::Attachment| {
            let mut doc = document_at(extent);
            let backdrop = paint_occurrence(&mut doc, "backdrop", Some(rgba8_source(extent, |x, _| [x as u8, 9, 9, 255])));
            insert_occurrence(&mut doc, backdrop, 1);
            let threshold = effect_occurrence(&mut doc, layer_core::EffectInstance::new(layer_core::bundled_effect_catalog().get("threshold").unwrap().program()), "threshold");
            let occurrence = doc.artwork.occurrences.get_mut(threshold).unwrap();
            occurrence.opacity = 0.6;
            occurrence.attachment = attachment;
            insert_occurrence(&mut doc, threshold, 0);
            let mask = SourceTarget::Coverage(coverage_mask(&mut doc, threshold, [3, -5], None));
            let ink = SourceTarget::Paint(doc.artwork.paint.iter().next().unwrap().0);
            let behind = doc.scene().source_target(backdrop).unwrap();
            let node = root(&doc, 1);
            let domains = Footprint::of(&node, doc.scene(), PixelRect::full(extent), extent, 1, true).domains();
            (doc, threshold, [ink, behind, mask], domains)
        };
        let (_, _, [ink, behind, mask], attached) = build(layer_core::authored::Attachment::Effect);
        assert_eq!(attached, [BTreeSet::from([behind]), BTreeSet::from([ink, mask])], "an attached conversion captures only its owner");
        let (doc, threshold, _, unattached) = build(layer_core::authored::Attachment::None);
        assert_eq!(unattached, [BTreeSet::new(), BTreeSet::from([ink, behind, mask])], "the effect's scope and mask are read on its native grid");
        let scene = doc.scene();
        let source = Arc::new(Expression::Source { id: ink, frame: source_frame(None, scene, ink), extent: scene.target_extent(ink), outside: 0 });
        let converted = Arc::new(Expression::Effect { input: source.clone(), chain: vec![(threshold, 0, 0)], masks: vec![None], radius: Some(0) });
        let both = Expression::Combine { front: converted, back: source, blend: layer_core::LayerBlend::Multiply as u32, flags: 0 };
        assert_eq!(Footprint::of(&both, scene, PixelRect::full(extent), extent, 1, true).domains(), [BTreeSet::from([ink]), BTreeSet::from([ink])]);
        assert_eq!(Footprint::of(&both, scene, PixelRect::full(extent), extent, 0, true).domains(), [BTreeSet::from([ink]), BTreeSet::from([ink])]);
    }

    #[test]
    fn a_global_effect_requires_its_whole_dependency_for_one_output_texel() {
        let extent = [1536, 768];
        let mut doc = document_at(extent);
        let handle = effect(&mut doc, "gaussian_blur");
        let application = match doc.artwork.occurrences.get(handle).unwrap().content { OccurrenceContent::Effect(effect) => effect, _ => unreachable!() };
        let program = Arc::make_mut(&mut doc.artwork.effects.get_mut(application).unwrap().program);
        program.passes = program.passes.iter().cloned().map(|pass| layer_core::EffectPass { sampling: layer_core::EffectSampling::Document, ..pass }).collect();
        let node = root(&doc, 1);
        assert_eq!(crate::effects::damage_radius(doc.scene().effect(handle).unwrap(), 1), None);
        let footprint = Footprint::of(&node, doc.scene(), PixelRect::new(700, 300, 702, 302), extent, 1, true);
        assert_eq!(bounds(&footprint.reduced[&ink(&doc)]), DocRect::from(PixelRect::full(extent)));
    }

    #[test]
    fn shifted_sources_write_each_level_texel_once_from_the_page_that_holds_it() {
        let extent = [1280, 1024];
        let mut doc = document_at(extent);
        let layer = paint_occurrence(&mut doc, "shifted", Some(rgba8_source([700, 500], |x, y| [x as u8, y as u8, 1, 255])));
        insert_occurrence(&mut doc, layer, 0);
        doc.artwork.occurrences.get_mut(layer).unwrap().offset = [301, -77];
        let id = doc.scene().source_target(layer).unwrap();
        let frame = source_frame(None, doc.scene(), id);
        assert_eq!(frame.phase, [45, 179]);
        for level in [1, 2, 3] {
            let mut texels = HashSet::new();
            let plan = display_mips::Plan::at(extent, level);
            for coordinate in pages::coordinates(plan.bounds, level) {
                let cell = pages::bounds(coordinate, level, extent);
                let node = root(&doc, level);
                let footprint = Footprint::of(&node, doc.scene(), cell, extent, level, true);
                let planned = declared(&doc, &footprint.reduced);
                for write in writes(&planned).filter(|write| write.source == id) {
                    let page = level_texels(page_rect(write.coordinate), level);
                    let written = write.texels(&planned, level);
                    assert_eq!(written.intersect(page), written, "level {level}: a write stays inside its page");
                    for y in written.min_y()..written.max_y() {
                        for x in written.min_x()..written.max_x() { assert!(texels.insert([x, y]), "level {level}: texel {x},{y} written twice"); }
                    }
                }
            }
            let local = level_texels(frame.local(DocRect::from(PixelRect::full(extent)), doc.scene().target_extent(id)), level);
            assert_eq!(texels.len() as u64, local.area(), "level {level}: every texel of the layer inside the drawing is written");
        }
    }

    #[test]
    fn writes_visit_a_native_coordinate_then_its_declared_layers_and_skip_reads_of_absent_tiles() {
        let extent = [1024, 512];
        let mut doc = document_at(extent);
        let upper = paint_occurrence(&mut doc, "upper", Some(rgba8_source(extent, |_, _| [1, 2, 3, 255])));
        insert_occurrence(&mut doc, upper, 0);
        let node = root(&doc, 1);
        let cell = PixelRect::new(0, 0, 512, 512);
        let footprint = Footprint::of(&node, doc.scene(), cell, extent, 1, true);
        let planned = declared(&doc, &footprint.reduced);
        let upper = doc.scene().source_target(upper).unwrap();
        let absent = |id: SourceTarget, coordinate: [u32; 2]| id != upper || coordinate != [1, 0];
        let order = writes(&planned).map(|write| (write.page, write.source, write.read(&absent))).collect::<Vec<_>>();
        let lower = ink(&doc);
        assert_eq!(order, vec![
            ([0, 0], lower, Read::Tiles), ([0, 0], upper, Read::Tiles), ([1, 0], lower, Read::Tiles), ([1, 0], upper, Read::Uniform),
            ([0, 1], lower, Read::Tiles), ([0, 1], upper, Read::Tiles), ([1, 1], lower, Read::Tiles), ([1, 1], upper, Read::Tiles),
        ]);
    }

    #[test]
    fn an_oversized_cell_splits_its_longest_side_on_the_level_grid_down_to_one_texel() {
        let level = 2;
        let cell = PixelRect::new(1024, 0, 2048, 600);
        let mut fits = |part: PixelRect| level_texels(part, level).area() <= 4096;
        let parts = stripes(cell, level, &mut fits).unwrap();
        assert!(parts.len() > 1);
        assert_eq!(parts.iter().map(|part| part.area()).sum::<u64>(), cell.area());
        for (i, a) in parts.iter().enumerate() {
            assert!(level_texels(*a, level).area() <= 4096);
            assert!([a.min_x() - cell.min_x(), a.min_y()].iter().all(|v| v % (1 << level) == 0), "{a:?} starts on a level texel");
            for b in &parts[i + 1..] { assert!(a.intersect(*b).is_empty(), "{a:?} overlaps {b:?}"); }
        }
        assert_eq!(parts[..2], [PixelRect::new(1024, 0, 1280, 148), PixelRect::new(1024, 148, 1280, 300)], "the longer side splits first: {parts:?}");
        let mut single = |part: PixelRect| level_texels(part, level).area() <= 1;
        assert_eq!(stripes(PixelRect::new(0, 0, 9, 6), level, &mut single).unwrap().len(), 6);
        let mut never = |_: PixelRect| false;
        assert_eq!(stripes(cell, level, &mut never), None);
    }

    #[test]
    fn a_gigapixel_fit_coarsens_to_the_finest_level_whose_texel_stays_within_a_device_pixel() {
        let side = 131_072u32;
        let scale = 1. / 128.;
        let transform = [scale, 0., 0., scale, 0., 0.];
        let root_bytes = |level: u32| u64::from(side >> level).pow(2) * 16;
        assert_eq!(root_bytes(4), 1 << 30);
        let budget = (1u64 << 30) - 1;
        let mut tried = Vec::new();
        let level = view_grid(transform, |level| {tried.push(level); (side >> level) <= 8192 && root_bytes(level) <= budget});
        assert_eq!(level, Some(5));
        assert_eq!(tried, vec![4, 5]);
        assert_eq!(view_grid(transform, |level| root_bytes(level) <= 16 << 20), Some(7));
        assert_eq!(view_grid(transform, |level| root_bytes(level) < 16 << 20), None, "a texel wider than a device pixel is never a candidate");
        assert_eq!(view_grid([1., 0., 0., 1., 0., 0.], |level| level > 0), None, "100% admits only level zero");
    }

    struct Worksheet { cells: usize, writes: Vec<(SourceTarget, [u32; 2], [u32; 4])>, coordinates: BTreeSet<[u32; 2]> }
    fn worksheet(doc: &Document, plan: display_mips::Plan) -> Worksheet {
        let node = root(doc, plan.level);
        let authored = |id: SourceTarget, coordinate: [u32; 2]| match id {
            SourceTarget::Paint(paint) => doc.artwork.paint.get(paint).and_then(|paint| paint.raster.try_data()).and_then(Result::ok)
                .is_some_and(|data| data.tiles.contains_key(&layer_core::raster::TileKey { plane: layer_core::raster::RasterPlane::Color, coordinate })),
            _ => false,
        };
        let empty = |id: SourceTarget| matches!(id, SourceTarget::Paint(paint) if doc.artwork.paint.get(paint).unwrap().raster.is_empty());
        let mut sheet = Worksheet { cells: 0, writes: Vec::new(), coordinates: BTreeSet::new() };
        for coordinate in pages::coordinates(plan.bounds, plan.level) {
            let cell = pages::bounds(coordinate, plan.level, plan.extent).intersect(plan.bounds);
            let footprint = Footprint::of(&node, doc.scene(), cell, plan.extent, plan.level, true);
            assert!(footprint.native.is_empty());
            sheet.cells += 1;
            let planned = declared(doc, &footprint.reduced);
            for write in writes(&planned) {
                if write.read(&authored) == Read::Uniform { assert!(empty(write.source), "{write:?} skips an authored tile"); continue; }
                sheet.coordinates.insert(write.coordinate);
                let texels = write.texels(&planned, plan.level);
                sheet.writes.push((write.source, write.coordinate, [texels.min_x(), texels.min_y(), texels.max_x(), texels.max_y()]));
            }
        }
        sheet
    }
    fn bytes(plan: display_mips::Plan) -> u64 {
        let texels = |size: [u32; 2]| size.map(u64::from).into_iter().product::<u64>() * 16;
        texels(plan.size) + texels(plan.size.map(|side| side.div_ceil(2)))
    }
    fn tablet(doc: &Document, size: [u32; 2], scale: f32) -> FramePacket<'_> {
        let extent = doc.composition().size;
        let mut frame = packet(doc.scene(), extent);
        frame.view.width_px = size[0]; frame.view.height_px = size[1];
        frame.view.document_to_surface = [scale, 0., 0., scale, size[0] as f32 / 2. - scale * extent[0] as f32 / 2., size[1] as f32 / 2. - scale * extent[1] as f32 / 2.];
        frame
    }

    #[test]
    fn the_tablet_fit_worksheet_plans_64_cells_of_112_distinct_writes() {
        let doc = crate::memory::memory_tests::layered_raster_document(28, 16);
        let frame = tablet(&doc, [1366; 2], 1. / 3.);
        let level = display_mips::view_level(frame.view.document_to_surface, 4).unwrap();
        assert_eq!(level, 1);
        let plan = view_plan(frame, level).unwrap();
        assert_eq!((plan.bounds, plan.size), (PixelRect::full([4096; 2]), [2048; 2]));
        let sheet = worksheet(&doc, plan);
        assert_eq!(sheet.cells, 64);
        assert_eq!(sheet.writes.len(), 64 * 4 * 28);
        assert_eq!(sheet.writes.iter().collect::<HashSet<_>>().len(), 7168, "the cells' native rectangles partition the drawing");
        assert_eq!(sheet.coordinates.len(), 256);
        let navigator = display_mips::Plan::new(plan.extent).unwrap();
        assert_eq!(navigator.size, [512; 2]);
        assert_eq!(2 * (bytes(plan) + 2 * bytes(navigator)), 180 << 20);
        assert_eq!(28 * u64::from(PAGE_SIZE).pow(2) * 16, 28 << 20);
    }

    #[test]
    fn the_tablet_zoom_worksheet_pairs_the_padded_view_with_the_overview_traversal() {
        let doc = crate::memory::memory_tests::layered_raster_document(28, 12);
        let frame = tablet(&doc, [1920, 1200], 1.);
        let extent = doc.composition().size;
        let visible = display_mips::view_bounds(frame.view, extent, 0).unwrap();
        assert_eq!(visible, PixelRect::new(576, 936, 2496, 2136));
        let visible = page_coordinates(visible).collect::<BTreeSet<_>>();
        assert_eq!(visible.len(), 48);
        let plan = view_plan(frame, 0).unwrap();
        assert_eq!(plan.size, [2560, 2048]);
        let view = worksheet(&doc, plan);
        assert_eq!((view.cells, view.coordinates.len(), view.writes.len()), (80, 80, 80 * 28));
        let overview_plan = overview_plan(plan);
        assert_eq!((overview_plan.level, overview_plan.size), (3, [384; 2]));
        let overview = worksheet(&doc, overview_plan);
        assert_eq!((overview.cells, overview.coordinates.len(), overview.writes.len()), (4, 144, 144 * 28));
        assert!(view.coordinates.is_subset(&overview.coordinates), "every padded view coordinate is visited by the overview traversal");
        assert!(visible.is_subset(&view.coordinates));
        let paired = 28 * (overview.coordinates.len() + visible.len());
        assert_eq!(paired, 5376);
        assert_eq!(28 * (overview.coordinates.len() + view.coordinates.len()), 6272, "independent traversals read more than the bound");
        assert_eq!(2 * (bytes(plan) + 3 * bytes(overview_plan)), 216 * (1 << 20) + 7 * (1 << 20) / 8);
    }
}
