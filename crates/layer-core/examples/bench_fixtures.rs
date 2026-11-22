//! Benchmark drawings for one canvas size: heavy.capy, with 32 layers of every kind, and vector.capy, with crowded vector paths.
//! Usage: bench_fixtures OUT_DIR WIDTH HEIGHT
mod capy_fixture;
use capy_fixture::{Ids, Random};
use layer_core::{EffectApplication, EffectInstance, LayerBlend, bundled_effect_catalog,
    authored::{Affine64, Artwork, CoverageSource, Image, ImageObject, MaskUse, Occurrence, OccurrenceContent, OccurrenceHandle, PaintBase, PaintSource, Stack},
    color::{ColorProfile, PixelDescriptor, RgbSpace, SampleDepth, source::{SourceChannels, SourceImage, SourceInterpretation}},
    raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TILE_SIZE, TileBlob, TileKey}};
use std::{collections::BTreeMap, error::Error, f64::consts::{FRAC_PI_2, TAU}, fs::File, io::{BufWriter, Write}, sync::Arc};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Tiles = BTreeMap<[u32; 2], Arc<TileBlob>>;
const LOW_TIER_PIXELS: f64 = 4248. * 2832.;

fn tiles(ids: &mut Ids, descriptor: PixelDescriptor, extent: [u32; 2], mut fill: impl FnMut([u32; 2], &mut [u8]) -> bool) -> Result<Tiles> {
    let mut bytes = vec![0; descriptor.byte_len([TILE_SIZE; 2]).ok_or("Unsupported pixels")?];
    let mut tiles = BTreeMap::new();
    for y in 0..extent[1].div_ceil(TILE_SIZE) {
        for x in 0..extent[0].div_ceil(TILE_SIZE) {
            bytes.fill(0);
            if fill([x * TILE_SIZE, y * TILE_SIZE], &mut bytes) {
                tiles.insert([x, y], Arc::new(TileBlob::from_package(ids.next(), descriptor, lz4_flex::block::compress(&bytes).into())?));
            }
        }
    }
    Ok(tiles)
}
fn pixels(origin: [u32; 2], extent: [u32; 2], bytes: &mut [u8], mut each: impl FnMut([u32; 2], &mut [u8])) {
    let size = bytes.len() / (TILE_SIZE * TILE_SIZE) as usize;
    for (row, line) in bytes.chunks_exact_mut(TILE_SIZE as usize * size).take((extent[1] - origin[1]) as usize).enumerate() {
        for (column, pixel) in line.chunks_exact_mut(size).take((extent[0] - origin[0]) as usize).enumerate() {
            each([origin[0] + column as u32, origin[1] + row as u32], pixel);
        }
    }
}
fn raster(plane: RasterPlane, tiles: Tiles) -> RasterRevision {
    let mut data = RasterData { tiles: Default::default(), watercolor: None };
    for (coordinate, tile) in tiles { data.tiles.insert(TileKey { plane, coordinate }, RasterTile::backed_shared(tile)); }
    RasterRevision::backed(data)
}
fn image(ids: &mut Ids, extent: [u32; 2], color: impl Fn([u32; 2]) -> [u8; 3]) -> Result<Image> {
    let interpretation = SourceInterpretation { channels: SourceChannels::Rgba, depth: SampleDepth::U8, profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false };
    let tiles = tiles(ids, interpretation.descriptor(), extent, |origin, bytes| {
        pixels(origin, extent, bytes, |p, pixel| { pixel[..3].copy_from_slice(&color(p)); pixel[3] = 255; });
        true
    })?;
    Ok(Image::with_id(ids.next(), Arc::new(SourceImage { extent, resolution: None, interpretation, tiles })))
}
fn grain(p: [u32; 2]) -> u8 { ((p[0].wrapping_mul(1_664_525) ^ p[1].wrapping_mul(1_013_904_223)).wrapping_mul(2_654_435_761) >> 27) as u8 }

struct Wave { axis: usize, start: f64, end: f64, center: f64, amplitude: f64, frequency: f64, phase: f64, half: f64, color: [u8; 3] }
impl Wave {
    fn reaches(&self, origin: [u32; 2]) -> bool {
        let (along, across, tile, reach) = (f64::from(origin[self.axis]), f64::from(origin[1 - self.axis]), f64::from(TILE_SIZE), self.amplitude + self.half + 1.);
        along < self.end && along + tile > self.start && across < self.center + reach && across + tile > self.center - reach
    }
    fn paint(&self, origin: [u32; 2], extent: [u32; 2], bytes: &mut [u8]) {
        let centers: Vec<f64> = (0..TILE_SIZE).map(|k| self.center + self.amplitude * (self.frequency * (f64::from(origin[self.axis] + k) + 0.5 - self.start) + self.phase).sin()).collect();
        pixels(origin, extent, bytes, |p, pixel| {
            let along = f64::from(p[self.axis]) + 0.5;
            if along < self.start || along > self.end { return; }
            let distance = (f64::from(p[1 - self.axis]) + 0.5 - centers[(p[self.axis] - origin[self.axis]) as usize]).abs();
            let alpha = ((self.half - distance + 0.5).clamp(0., 1.) * 255.).round() as u8;
            if alpha > pixel[3] { pixel[..3].copy_from_slice(&self.color); pixel[3] = alpha; }
        });
    }
}
fn waves(random: &mut Random, extent: [u32; 2]) -> Vec<Wave> {
    let size = extent.map(f64::from);
    let window = size.map(|side| side * (LOW_TIER_PIXELS / (size[0] * size[1])).min(1.).sqrt());
    let origin = [0, 1].map(|axis| random.range(0., size[axis] - window[axis]));
    let mut waves: Vec<Wave> = (0..12).map(|_| {
        let axis = usize::from(random.next() < 0.5);
        let length = random.range(0.3, 0.5) * window[axis];
        let start = origin[axis] + random.range(0., window[axis] - length);
        Wave { axis, start, end: start + length, center: origin[1 - axis] + random.range(0.1, 0.9) * window[1 - axis], amplitude: random.range(0.02, 0.08) * window[1 - axis],
            frequency: random.range(1., 3.) * TAU / length, phase: random.range(0., TAU), half: 0., color: [0; 3].map(|_| random.range(0., 255.) as u8) }
    }).collect();
    let half = 0.2 * window[0] * window[1] / (2. * waves.iter().map(|wave| wave.end - wave.start).sum::<f64>());
    for wave in &mut waves { wave.half = half; }
    waves
}
fn mask(ids: &mut Ids, descriptor: PixelDescriptor, extent: [u32; 2], center: [f64; 2]) -> Result<CoverageSource> {
    let unit = f64::from(extent[0].min(extent[1]));
    let coverage = tiles(ids, descriptor, extent, |origin, bytes| {
        let mut partial = false;
        pixels(origin, extent, bytes, |p, value| {
            let distance = (f64::from(p[0]) - center[0] * f64::from(extent[0])).hypot(f64::from(p[1]) - center[1] * f64::from(extent[1]));
            value[0] = ((distance / unit - 0.25) / 0.1 * 255.).clamp(0., 255.).round() as u8;
            partial |= value[0] < 255;
        });
        partial
    })?;
    Ok(CoverageSource { domain: extent, raster: raster(RasterPlane::Mask, coverage), default_coverage: 1., operations: Default::default() })
}

fn layer(art: &mut Artwork, ids: &mut Ids, content: OccurrenceContent, name: &str, blend: LayerBlend) -> Result<OccurrenceHandle> {
    let mut occurrence = Occurrence::new(content, name);
    occurrence.blend = blend;
    Ok(art.occurrences.insert(ids.next(), occurrence)?)
}
fn paint(art: &mut Artwork, ids: &mut Ids, name: &str, blend: LayerBlend, raster: RasterRevision, base: Option<PaintBase>) -> Result<OccurrenceHandle> {
    let domain = art.compositions.get(art.root).ok_or("Missing composition")?.size;
    let source = art.paint.insert(ids.next(), PaintSource { color_mode: Default::default(), domain, raster, base, operations: Default::default() })?;
    layer(art, ids, OccurrenceContent::Paint(source), name, blend)
}
fn group(art: &mut Artwork, ids: &mut Ids, name: &str, blend: LayerBlend, entries: Vec<OccurrenceHandle>) -> Result<OccurrenceHandle> {
    let stack = art.stacks.insert(ids.next(), Stack { entries })?;
    layer(art, ids, OccurrenceContent::Stack(stack), name, blend)
}
fn effect(art: &mut Artwork, ids: &mut Ids, program: &str, name: &str) -> Result<OccurrenceHandle> {
    let size = art.compositions.get(art.root).ok_or("Missing composition")?.size;
    let instance = EffectInstance::new(bundled_effect_catalog().get(program).ok_or("Missing effect program")?.program());
    let effect = art.effects.insert(ids.next(), EffectApplication::new(instance.program, instance.values, size))?;
    layer(art, ids, OccurrenceContent::Effect(effect), name, LayerBlend::Normal)
}

fn heavy(size: [u32; 2], ids: &mut Ids) -> Result<Artwork> {
    let mut art = Artwork::new(size)?;
    let color = art.compositions.get(art.root).ok_or("Missing composition")?.color;
    let mut random = Random(0x2545_f491_4f6c_dd1d);
    let mut painted = Vec::new();
    for slot in 0..24 {
        let blend = match slot { 0 => LayerBlend::Screen, 9 => LayerBlend::Multiply, _ => LayerBlend::Normal };
        let name = if blend == LayerBlend::Normal { format!("Paint {}", 24 - slot) } else { blend.label().into() };
        let waves = waves(&mut random, size);
        let strokes = tiles(ids, RasterPlane::Color.descriptor(color), size, |origin, bytes| {
            for wave in waves.iter().filter(|wave| wave.reaches(origin)) { wave.paint(origin, size, bytes); }
            bytes.chunks_exact(4).any(|pixel| pixel[3] > 0)
        })?;
        painted.push(paint(&mut art, ids, &name, blend, raster(RasterPlane::Color, strokes), None)?);
    }
    let masked = paint(&mut art, ids, "Masked", LayerBlend::Normal, Default::default(), None)?;
    let glaze = group(&mut art, ids, "Glaze", LayerBlend::PassThrough, [&[masked][..], &painted[..3]].concat())?;
    let shade = group(&mut art, ids, "Shade", LayerBlend::Normal, painted[9..16].to_vec())?;
    let detail = group(&mut art, ids, "Detail", LayerBlend::Normal, painted[16..].to_vec())?;
    for (index, owner) in [masked, shade, painted[5], painted[19]].into_iter().enumerate() {
        let angle = index as f64 * FRAC_PI_2 + 0.6;
        let source = art.coverage.insert(ids.next(), mask(ids, RasterPlane::Mask.descriptor(color), size, [0.5 + 0.2 * angle.cos(), 0.5 + 0.2 * angle.sin()])?)?;
        art.occurrences.get_mut(owner).ok_or("Missing masked layer")?.mask = Some(MaskUse { source, enabled: true, linked: true, inverted: false, offset: [0; 2] });
    }
    let base = image(ids, size, |p| [(p[0] * 200 / size[0]) as u8 + grain(p), (p[1] * 200 / size[1]) as u8 + grain(p), ((p[0] + p[1]) * 120 / (size[0] + size[1])) as u8 + 60 + grain(p)])?;
    let photo = paint(&mut art, ids, "Photo", LayerBlend::Normal, Default::default(), Some(PaintBase::new(base)))?;
    let mut object = ImageObject::new(image(ids, [512; 2], |p| [(p[0] / 2) as u8, (p[1] / 2) as u8, if (p[0] / 64 + p[1] / 64) % 2 == 0 { 220 } else { 40 }])?);
    object.affine = Affine64([1., 0., 0., 1., f64::from(size[0]) / 2. - 256., f64::from(size[1]) / 2. - 256.]);
    let object = art.objects.insert(ids.next(), object)?;
    let image = layer(&mut art, ids, OccurrenceContent::Objects(object), "Image", LayerBlend::Normal)?;
    let levels = effect(&mut art, ids, "levels", "Levels")?;
    let paper = effect(&mut art, ids, layer_core::paper::PAPER_PROGRAM, "Paper")?;
    let root = art.compositions.get(art.root).ok_or("Missing composition")?.result;
    art.stacks.get_mut(root).ok_or("Missing root stack")?.entries = [&[levels, image, glaze][..], &painted[3..9], &[shade, detail, photo, paper]].concat();
    Ok(art)
}

fn fixture(name: &str, size: [u32; 2], output: &mut impl Write) -> Result<()> {
    let mut ids = Ids::new(&format!("{name} {}x{}", size[0], size[1]));
    let paths = match u64::from(size[0]) * u64::from(size[1]) { pixels if pixels < 20_000_000 => 10_000, pixels if pixels < 40_000_000 => 20_000, _ => 50_000 };
    let artwork = if name == "heavy" { heavy(size, &mut ids)? } else { capy_fixture::crowded_vector(size[0], size[1], paths, false)?.artwork };
    capy_fixture::package(artwork, ids, output)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let [output, width, height] = [1, 2, 3].map(|i| args.get(i).cloned().ok_or("Usage: bench_fixtures OUT_DIR WIDTH HEIGHT"));
    let (output, size) = (std::path::PathBuf::from(output?), [width?.parse()?, height?.parse()?]);
    for name in ["heavy", "vector"] {
        let mut writer = BufWriter::new(File::create(output.join(format!("{name}.capy")))?);
        fixture(name, size, &mut writer)?;
        writer.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::{Document, package::{ImmutableBacking, codec::{OpenOutcome, open}}};
    use std::sync::atomic::AtomicBool;

    fn reopen(name: &str) -> Document {
        let package = || { let mut bytes = Vec::new(); fixture(name, [512, 384], &mut bytes).unwrap(); bytes };
        let bytes = package();
        assert!(bytes == package(), "{name} must not change between runs");
        let OpenOutcome::Candidate { artwork, .. } = open(ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap(), Default::default(), &AtomicBool::new(false)).unwrap()
            else { panic!("{name} must reopen as editable") };
        Document::from_artwork(artwork).unwrap()
    }

    #[test]
    fn heavy_reopens_with_groups_masks_blends_effects_and_an_image() {
        let document = reopen("heavy");
        let scene = document.scene();
        let layers: Vec<_> = scene.order().iter().map(|h| (*h, scene.occurrence(*h).unwrap())).collect();
        let named = |name: &str| *layers.iter().find(|(_, layer)| layer.name.as_ref() == name).unwrap_or_else(|| panic!("{name} is missing"));
        assert_eq!(layers.len(), 32);
        assert!(layers.iter().all(|(_, layer)| layer.visible));
        let groups: Vec<_> = layers.iter().filter(|(_, layer)| matches!(layer.content, OccurrenceContent::Stack(_))).map(|(_, layer)| layer.blend).collect();
        assert_eq!((groups.len(), groups.iter().filter(|blend| **blend == LayerBlend::PassThrough).count()), (3, 1));
        assert_eq!(layers.iter().filter(|(_, layer)| layer.mask.is_some()).count(), 4);
        let painted = layers.iter().filter(|(handle, _)| scene.paint_source(*handle).is_some_and(|paint| !paint.raster.is_empty())).count();
        assert_eq!(painted, 24);
        let (masked, layer) = named("Masked");
        assert!(layer.mask.is_some() && scene.parent(masked).is_some() && scene.paint_source(masked).is_some_and(|paint| paint.raster.is_empty()));
        assert!(scene.paint_source(named("Photo").0).is_some_and(|paint| paint.base.is_some()));
        assert!(matches!(named("Image").1.content, OccurrenceContent::Objects(_)));
        assert_eq!([named("Multiply").1.blend, named("Screen").1.blend], [LayerBlend::Multiply, LayerBlend::Screen]);
        let OccurrenceContent::Effect(levels) = named("Levels").1.content else { panic!("Levels must be an effect layer") };
        assert_eq!(document.artwork.effects.get(levels).unwrap().program.id.as_ref(), "levels");
    }

    #[test]
    fn vector_reopens_with_crowded_paths() {
        let document = reopen("vector");
        let scene = document.scene();
        let ink = scene.order().iter().map(|h| scene.occurrence(*h).unwrap()).find(|layer| layer.name.as_ref() == "Ink").unwrap();
        let OccurrenceContent::Vector(ink) = ink.content else { panic!("Ink must be a vector layer") };
        assert_eq!(document.artwork.vectors.get(ink).unwrap().records().count(), 10_000);
    }
}
