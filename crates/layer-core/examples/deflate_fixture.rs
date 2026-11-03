use layer_core::{EffectApplication, EffectInstance, EffectValue, Editor, EvaluationContext, ImageObject, Lut3d, Occurrence, OccurrenceContent, PaintSource, PhotoMetadata,
    authored::{Artwork, PortableId, Resource, vector::*},
    color::{DocumentColor, RgbColor, RgbSpace, SampleDepth, source::{SourceBuilder, SourceChannels, SourceInterpretation}},
    package::{archive::{self, Directory, InputMember}, codec::PreparedPackage, resources::parse_descriptor},
    raster::{RasterData, RasterPlane, RasterRevision, RasterTile, TileBlob, TileKey}};
use serde_json::Value;
use std::{io::{Cursor, Write}, sync::{Arc, atomic::AtomicBool}};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn zlib(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(6));
    encoder.write_all(bytes)?;
    Ok(encoder.finish()?)
}

fn filtered_planes(samples: &[u8], bpp: usize) -> Vec<u8> {
    let pixels = samples.len() / bpp;
    let mut planes = vec![0; samples.len()];
    for (pixel, bytes) in samples.chunks_exact(bpp).enumerate() { for (plane, byte) in bytes.iter().enumerate() { planes[plane * pixels + pixel] = *byte; } }
    for row in planes.as_chunks_mut::<256>().0 { for x in (1..256).rev() { row[x] = row[x].wrapping_sub(row[x - 1]); } }
    planes
}

fn artwork() -> Result<Artwork> {
    let mut artwork = Artwork::new([300, 200])?;
    let color = DocumentColor { space: RgbSpace::DisplayP3, depth: SampleDepth::U16 };
    artwork.compositions.get_mut(artwork.root).ok_or("root")?.color = color;
    let stack = artwork.compositions.get(artwork.root).ok_or("root")?.result;
    let samples: Vec<u8> = (0..256u32 * 256).flat_map(|i| { let (x, y) = (i % 256, i / 256); [x * 257 + y, y * 131 + x * 7, (x ^ y) * 251, 40000 + x * 99].into_iter().flat_map(|v| (v as u16).to_le_bytes()) }).collect();
    let tile = TileBlob::encode(color.paint_descriptor(), &samples)?;
    let paint = artwork.paint.insert(PortableId::random(), PaintSource { color_mode: Default::default(), domain: [300, 200],
        raster: RasterRevision::backed(RasterData { tiles: [(TileKey { plane: RasterPlane::Color, coordinate: [0, 0] }, RasterTile::backed(tile))].into(), watercolor: None }),
        base: None, operations: Default::default() })?;
    let interpretation = SourceInterpretation { channels: SourceChannels::Rgb, depth: SampleDepth::U8, profile: Default::default(), profile_assumed: false };
    let mut builder = SourceBuilder::new([300, 200], interpretation, 1 << 20)?;
    for y in 0..200u32 { builder.push_row(&(0..300u32).flat_map(|x| [x as u8, y as u8, (x ^ y) as u8]).collect::<Vec<_>>())?; }
    let photo = artwork.objects.insert(PortableId::random(), ImageObject::new(Arc::new(builder.finish()?).into()))?;
    let anchors = [[10., 20.], [150., 90.], [290., 180.]].map(Anchor::corner);
    let path = PathItem::new(ItemId(7), Arc::new(PathGeometry::new(vec![Subpath::new(&anchors, false)])?),
        Arc::new(Style::new(vec![PaintEntry::Stroke(Stroke::new(Paint::color(RgbColor::BLACK), 4.))])));
    let vector = artwork.vectors.insert(PortableId::random(), VectorObject::from_records(vec![Record::Path(path)]))?;
    let lut = Arc::new(Lut3d::from_samples(2, [[0.; 3], [1.; 3]], "Deflated".into(), (0..8).map(|i| [i as f32 / 8., 0.5, 1. - i as f32 / 8.]).collect())?);
    let mut lookup = EffectInstance::new(layer_core::bundled_effect_catalog().get("color_lookup").ok_or("lookup")?.program());
    lookup.set("resource", EffectValue::Lut3d(Some(lut)))?;
    let effect = artwork.effects.insert(PortableId::random(), EffectApplication::new(lookup.program, lookup.values, [300, 200]))?;
    for (name, content) in [("Paint", OccurrenceContent::Paint(paint)), ("Photo", OccurrenceContent::Objects(photo)), ("Ink", OccurrenceContent::Vector(vector)), ("Lookup", OccurrenceContent::Effect(effect))] {
        let occurrence = artwork.occurrences.insert(PortableId::random(), Occurrence::new(content, name))?;
        artwork.stacks.get_mut(stack).ok_or("stack")?.entries.push(occurrence);
    }
    artwork.metadata = Arc::new(PhotoMetadata { exif: None, xmp: Some(Resource::from(b"<xmp>deflated metadata</xmp>".to_vec())), iptc: None });
    Ok(artwork)
}

fn main() -> Result<()> {
    let output = std::env::args().nth(1).ok_or("OUTPUT.capy is required")?;
    let editor = Editor::new(layer_core::Document::from_artwork(artwork()?)?);
    let capture = editor.capture(0, EvaluationContext::default())?;
    let cancelled = AtomicBool::new(false);
    let mut lz4 = Vec::new();
    PreparedPackage::prepare(&capture, None, &cancelled)?.write(&mut lz4, &cancelled)?;
    let directory = Directory::read(&mut Cursor::new(&lz4), 16, 1 << 24).map_err(|e| format!("{e:?}"))?;
    let mut manifest: Value = serde_json::from_slice(&directory.read_member(&mut Cursor::new(&lz4), directory.member("manifest.json").ok_or("manifest")?, 1 << 24)?)?;
    let pack = directory.member("data/tiles-1.bin").ok_or("pack")?.clone();
    let mut packed = Vec::new();
    for resource in manifest["resources"].as_array_mut().ok_or("resources")? {
        let offset: usize = resource["location"]["offset"].as_str().ok_or("offset")?.parse()?;
        let length: usize = resource["bytes"].as_str().ok_or("bytes")?.parse()?;
        let stored = &lz4[pack.offset as usize + offset..pack.offset as usize + offset + length];
        let encoded = match (resource["type"].as_str(), resource["encoding"].as_str()) {
            (Some("capy.raster-tile/1"), _) => {
                let descriptor = parse_descriptor(&resource["data"]).map_err(|e| format!("{e:?}"))?;
                let samples = TileBlob::from_package(PortableId::random(), descriptor, stored.into())?.decode_on_worker()?;
                resource["encoding"] = "capy.deflate-tile/1".into();
                zlib(&filtered_planes(&samples, descriptor.bytes_per_pixel().ok_or("pixel size")?))?
            }
            (_, Some("capy.lz4-bytes/1")) => {
                let mut decoded = vec![0; resource["data"]["decoded_bytes"].as_str().ok_or("decoded")?.parse()?];
                lz4_flex::block::decompress_into(stored, &mut decoded)?;
                resource["encoding"] = "capy.deflate-bytes/1".into();
                zlib(&decoded)?
            }
            _ => {
                resource["encoding"] = "capy.deflate-bytes/1".into();
                resource["data"]["decoded_bytes"] = stored.len().to_string().into();
                zlib(stored)?
            }
        };
        resource["location"]["offset"] = packed.len().to_string().into();
        resource["bytes"] = encoded.len().to_string().into();
        resource["crc32"] = format!("{:08x}", crc32fast::hash(&encoded)).into();
        packed.extend(encoded);
    }
    let json = serde_json::to_vec(&manifest)?;
    let (mut mime, mut manifest, mut data) = (Cursor::new(archive::MIMETYPE), Cursor::new(&json[..]), Cursor::new(&packed[..]));
    let mut members = [InputMember { name: "mimetype", length: archive::MIMETYPE.len() as u64, crc32: crc32fast::hash(archive::MIMETYPE), input: &mut mime },
        InputMember { name: "manifest.json", length: json.len() as u64, crc32: crc32fast::hash(&json), input: &mut manifest },
        InputMember { name: "data/tiles-1.bin", length: packed.len() as u64, crc32: crc32fast::hash(&packed), input: &mut data }];
    let mut bytes = Vec::new();
    archive::write_archive(&mut bytes, &mut members, 1 << 24)?;
    std::fs::write(output, bytes)?;
    Ok(())
}
