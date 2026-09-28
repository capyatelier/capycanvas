use super::exif::{self, Directories};
use super::*;
use layer_core::{ImageResolution, PhotoMetadata};
use std::io::Cursor;

type Raw = (u16, u16, u32, Vec<u8>);

fn ascii(tag: u16, text: &str) -> Raw {
    let value = [text.as_bytes(), &[0]].concat();
    (tag, 2, value.len() as u32, value)
}
fn rationals(tag: u16, values: &[[u32; 2]]) -> Raw {
    let value = values.iter().flat_map(|[n, d]| [n.to_be_bytes(), d.to_be_bytes()]).flatten().collect();
    (tag, 5, values.len() as u32, value)
}
fn short(tag: u16, value: u16) -> Raw {
    (tag, 3, 1, value.to_be_bytes().to_vec())
}
fn long(tag: u16, value: u32) -> Raw {
    (tag, 4, 1, value.to_be_bytes().to_vec())
}

/// Big-endian IFDs laid out one after another; each directory's next link
/// points at the one that follows it, except IFD0's which points at `thumbnail`.
fn big_endian_exif(image: Vec<Raw>, exif: Vec<Raw>, gps: Vec<Raw>, thumbnail: Vec<Raw>) -> Vec<u8> {
    let size = |entries: &[Raw]| {
        6 + 12 * entries.len() + entries.iter().filter(|e| e.3.len() > 4).map(|e| e.3.len().next_multiple_of(2)).sum::<usize>()
    };
    let mut image = image;
    let pointers = [(0x8769u16, &exif), (0x8825, &gps)];
    image.extend(pointers.iter().map(|(tag, _)| long(*tag, 0)));
    let mut at = 8 + size(&image);
    let mut offsets = Vec::new();
    for (tag, entries) in pointers {
        image.iter_mut().find(|e| e.0 == tag).unwrap().3 = (at as u32).to_be_bytes().to_vec();
        offsets.push(at);
        at += size(entries);
    }
    let thumbnail_at = at;
    let mut out = b"MM\0\x2a\0\0\0\x08".to_vec();
    for (entries, next) in [(&image, thumbnail_at), (&exif, 0), (&gps, 0), (&thumbnail, 0)] {
        let mut data_at = out.len() + 6 + 12 * entries.len();
        let mut data = Vec::new();
        out.extend((entries.len() as u16).to_be_bytes());
        for (tag, kind, count, value) in entries {
            out.extend(tag.to_be_bytes());
            out.extend(kind.to_be_bytes());
            out.extend(count.to_be_bytes());
            if value.len() <= 4 {
                let mut inline = [0; 4];
                inline[..value.len()].copy_from_slice(value);
                out.extend(inline);
            } else {
                out.extend((data_at as u32).to_be_bytes());
                data.extend(value);
                if value.len() % 2 == 1 {
                    data.push(0);
                }
                data_at += value.len().next_multiple_of(2);
            }
        }
        out.extend((next as u32).to_be_bytes());
        out.extend(data);
    }
    out
}

fn camera_exif() -> Vec<u8> {
    big_endian_exif(
        vec![
            ascii(0x010f, "Capycam"),
            ascii(0x0110, "C-1"),
            short(0x0112, 6),
            rationals(0x011a, &[[300, 1]]),
            short(0x0128, 2),
            ascii(0x0131, "fw 1.0"),
            ascii(0x013b, "Ada Painter"),
            ascii(0x8298, "(c) 2026 Ada Painter"),
        ],
        vec![
            rationals(0x829a, &[[1, 250]]),
            rationals(0x829d, &[[28, 10]]),
            short(0x8827, 400),
            ascii(0x9003, "2026:09:01 10:00:00"),
            (0x927c, 7, 16, vec![7; 16]),
            long(0xa002, 4000),
            long(0xa003, 3000),
            long(0xa005, 12),
            ascii(0xa434, "Capy 35mm F1.8"),
        ],
        vec![ascii(0x0001, "N"), rationals(0x0002, &[[38, 1], [42, 1], [30, 1]])],
        vec![long(0x0201, 8), long(0x0202, 4)],
    )
}

const PACKET: &str = r#"<?xpacket begin="﻿" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:xmpRights="http://ns.adobe.com/xap/1.0/rights/"
    xmlns:Iptc4xmpCore="http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/"
    xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/"
    xmlns:exif="http://ns.adobe.com/exif/1.0/"
    xmlns:tiff="http://ns.adobe.com/tiff/1.0/"
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/"
    xmlns:stRef="http://ns.adobe.com/xap/1.0/sType/ResourceRef#"
    photoshop:City="Lisbon" exif:GPSLatitude="38,42.5N" tiff:Orientation="6"
    crs:Exposure2012="+0.50" xmp:Rating="4" xmpRights:Marked="True" photoshop:Credit='Ada "&amp;" Co'>
   <dc:creator><rdf:Seq><rdf:li>Ada Painter</rdf:li></rdf:Seq></dc:creator>
   <dc:rights><rdf:Alt><rdf:li xml:lang="x-default">© 2026 Ada &amp; Co</rdf:li></rdf:Alt></dc:rights>
   <dc:title><rdf:Alt><rdf:li xml:lang="x-default">Harbour</rdf:li></rdf:Alt></dc:title>
   <Iptc4xmpCore:CreatorContactInfo rdf:parseType="Resource">
    <Iptc4xmpCore:CiEmailWork>ada@example.com</Iptc4xmpCore:CiEmailWork>
   </Iptc4xmpCore:CreatorContactInfo>
   <xmpMM:DerivedFrom rdf:parseType="Resource"><stRef:filePath>/Users/ada/raw/IMG_1.CR3</stRef:filePath></xmpMM:DerivedFrom>
   <xmpMM:History><rdf:Seq><rdf:li rdf:parseType="Resource"/></rdf:Seq></xmpMM:History>
   <crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li></rdf:Seq></crs:ToneCurvePV2012>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;

const KEPT_ALWAYS: [&str; 5] = ["Ada Painter", "© 2026 Ada &amp; Co", "ada@example.com", "xmpRights:Marked", "photoshop:Credit"];
const DESCRIPTIVE: [&str; 2] = ["Harbour", "xmp:Rating"];
const LOCATION: [&str; 2] = ["Lisbon", "GPSLatitude"];
const NEVER: [&str; 6] = ["Orientation", "crs:", "xmpMM:", "filePath", "IMG_1", "History"];

fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    [&[0xff, marker], ((payload.len() + 2) as u16).to_be_bytes().as_slice(), payload].concat()
}

fn camera_jpeg() -> Vec<u8> {
    let mut builder = SourceBuilder::new(
        [6, 4],
        SourceInterpretation {
            channels: SourceChannels::Rgb,
            depth: SampleDepth::U8,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        1 << 20,
    )
    .unwrap();
    for y in 0..4u8 {
        builder.push_row(&(0..18u8).map(|i| i.wrapping_mul(13).wrapping_add(y * 40)).collect::<Vec<u8>>()).unwrap();
    }
    let mut plain = Vec::new();
    write_jpeg(&mut plain, &builder.finish().unwrap(), 95).unwrap();
    let iptc = [b"8BIM\x04\x04\0\0\0\0\0\x05".as_slice(), b"\x1c\x02\x05\0\0", b"\0"].concat();
    [
        &plain[..2],
        &segment(0xe1, &[b"Exif\0\0".as_slice(), &camera_exif()].concat()),
        &segment(0xe1, &[super::jpeg_markers::XMP, PACKET.as_bytes()].concat()),
        &segment(0xed, &[b"Photoshop 3.0\0".as_slice(), &iptc].concat()),
        &plain[2..],
    ]
    .concat()
}

fn opened() -> DecodedPhoto {
    read_photo_detailed(Cursor::new(camera_jpeg()), Default::default()).unwrap()
}

fn text(directories: &Directories, tag: u16) -> Option<String> {
    [&directories.image, &directories.exif, &directories.gps]
        .into_iter()
        .flatten()
        .find(|e| e.tag == tag)
        .map(|e| String::from_utf8_lossy(e.value.strip_suffix(&[0]).unwrap_or(&e.value)).into_owned())
}
fn tags(entries: &[exif::Entry]) -> Vec<u16> {
    entries.iter().map(|e| e.tag).collect()
}

type RawTags = Vec<(u16, Vec<u8>)>;

/// Every tag in IFD0 and the Exif IFD of a little-endian block, unfiltered.
fn raw_tags(block: &[u8]) -> (RawTags, RawTags) {
    let u16_at = |at: usize| u16::from_le_bytes([block[at], block[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes(block[at..at + 4].try_into().unwrap());
    let entries = |at: usize| {
        (0..usize::from(u16_at(at)))
            .map(|i| (u16_at(at + 2 + 12 * i), block[at + 10 + 12 * i..at + 14 + 12 * i].to_vec()))
            .collect::<Vec<_>>()
    };
    assert_eq!(&block[..4], b"II\x2a\0");
    let image = entries(u32_at(4) as usize);
    let exif = image
        .iter()
        .find(|(tag, _)| *tag == 0x8769)
        .map(|(_, v)| entries(u32::from_le_bytes(v[..4].try_into().unwrap()) as usize))
        .unwrap_or_default();
    (image, exif)
}

fn policy(keep: MetadataKeep, remove_location: bool) -> ExportMetadata {
    ExportMetadata { keep, remove_location }
}

#[test]
fn opening_a_photo_keeps_its_descriptive_directories_xmp_and_iptc() {
    let photo = opened();
    assert_eq!(photo.source.extent, [4, 6], "orientation 6 was applied");
    let metadata = &photo.metadata;
    let found = exif::read_block(metadata.exif.as_ref().unwrap()).unwrap().directories;
    assert_eq!(tags(&found.image), [0x010f, 0x0110, 0x0131, 0x013b, 0x8298]);
    assert_eq!(tags(&found.exif), [0x829a, 0x829d, 0x8827, 0x9003, 0xa434]);
    assert_eq!(tags(&found.gps), [0x0001, 0x0002]);
    assert_eq!(text(&found, 0x010f).as_deref(), Some("Capycam"));
    assert_eq!(text(&found, 0xa434).as_deref(), Some("Capy 35mm F1.8"));
    let exposure = found.exif.iter().find(|e| e.tag == 0x829a).unwrap();
    assert_eq!(exposure.value, [1u32.to_le_bytes(), 250u32.to_le_bytes()].concat(), "values are little-endian");
    assert_eq!(metadata.xmp.as_deref(), Some(PACKET.as_bytes()));
    assert_eq!(metadata.iptc.as_deref(), Some(b"\x1c\x02\x05\0\0".as_slice()));
    metadata.validate().unwrap();

    let broken = [&camera_jpeg()[..2], &segment(0xe1, &[super::jpeg_markers::XMP, b"<x:xmpmeta><rdf:RDF>".as_slice()].concat()), &camera_jpeg()[2..]].concat();
    let reopened = read_photo_detailed(Cursor::new(broken), Default::default()).unwrap();
    assert_eq!(reopened.metadata.xmp, None, "an unreadable packet is left out");
    assert!(reopened.metadata.exif.is_some());
}

#[test]
fn delivery_keeps_the_chosen_fields_and_regenerates_orientation_size_and_density() {
    let photo = opened().metadata;
    let delivery = |keep, remove_location, resolution| DeliveryMetadata {
        resolution,
        photo: photo.clone(),
        policy: policy(keep, remove_location),
    };
    let all = delivery(MetadataKeep::All, true, Some(ImageResolution::ppi(240))).exif([800, 600]).unwrap().unwrap();
    let (image, exif_ifd) = raw_tags(&all);
    let image_tags: Vec<u16> = image.iter().map(|(t, _)| *t).collect();
    assert_eq!(image_tags, [0x010f, 0x0110, 0x0112, 0x011a, 0x011b, 0x0128, 0x0131, 0x013b, 0x8298, 0x8769]);
    assert_eq!(image.iter().find(|(t, _)| *t == 0x0112).unwrap().1[..2], [1, 0], "orientation is 1");
    let exif_tags: Vec<u16> = exif_ifd.iter().map(|(t, _)| *t).collect();
    assert_eq!(exif_tags, [0x829a, 0x829d, 0x8827, 0x9000, 0x9003, 0xa002, 0xa003, 0xa434]);
    for (tag, size) in [(0xa002, 800u32), (0xa003, 600)] {
        assert_eq!(exif_ifd.iter().find(|(t, _)| *t == tag).unwrap().1, size.to_le_bytes(), "delivered size");
    }
    assert_eq!(super::metadata::exif(&all).unwrap().resolution, Some(ImageResolution::ppi(240)));
    let reread = exif::read_block(&all).unwrap().directories;
    assert!(reread.gps.is_empty(), "location is removed by default");
    assert_eq!(text(&reread, 0x8298).as_deref(), Some("(c) 2026 Ada Painter"));

    let located = exif::read_block(&delivery(MetadataKeep::All, false, None).exif([8, 6]).unwrap().unwrap()).unwrap().directories;
    assert_eq!(tags(&located.gps), [0x0001, 0x0002]);
    assert_eq!(located.gps[1].value.len(), 24);

    for remove_location in [false, true] {
        let rights = delivery(MetadataKeep::CopyrightContact, remove_location, None).exif([8, 6]).unwrap().unwrap();
        let (image, exif_ifd) = raw_tags(&rights);
        assert_eq!(image.iter().map(|(t, _)| *t).collect::<Vec<_>>(), [0x0112, 0x013b, 0x8298]);
        assert!(exif_ifd.is_empty(), "no camera settings, dates or GPS");
    }

    assert_eq!(delivery(MetadataKeep::None, false, None).exif([8, 6]).unwrap(), None);
    let density = delivery(MetadataKeep::None, false, Some(ImageResolution::ppi(300))).exif([8, 6]).unwrap().unwrap();
    assert_eq!(raw_tags(&density).0.iter().map(|(t, _)| *t).collect::<Vec<_>>(), [0x0112, 0x011a, 0x011b, 0x0128]);
    assert_eq!(DeliveryMetadata::resolution(None).exif([8, 6]).unwrap(), None, "a new drawing writes no Exif");
}

#[test]
fn xmp_keeps_rights_creator_and_contact_and_drops_paths_history_and_raw_settings() {
    let packet = PACKET.as_bytes();
    let filtered = |keep, remove_location| super::xmp::descriptions(packet, policy(keep, remove_location)).unwrap();
    let check = |text: &str, present: &[&str], absent: &[&str]| {
        for value in present {
            assert!(text.contains(value), "{value} missing from {text}");
        }
        for value in absent {
            assert!(!text.contains(value), "{value} kept in {text}");
        }
        let reparsed = super::xmp::packet(text);
        assert_eq!(super::xmp::descriptions(&reparsed, policy(MetadataKeep::All, false)).unwrap().as_deref(), Some(text));
    };
    let all = filtered(MetadataKeep::All, true).unwrap();
    check(&all, &[KEPT_ALWAYS.as_slice(), &DESCRIPTIVE, &["xmlns:dc=", "xmlns:rdf="]].concat(), &[LOCATION.as_slice(), &NEVER].concat());
    assert!(all.contains(r#"photoshop:Credit="Ada &quot;&amp;&quot; Co""#), "{all}");
    let located = filtered(MetadataKeep::All, false).unwrap();
    check(&located, &[KEPT_ALWAYS.as_slice(), &DESCRIPTIVE, &LOCATION].concat(), &NEVER);
    for remove_location in [false, true] {
        let rights = filtered(MetadataKeep::CopyrightContact, remove_location).unwrap();
        check(&rights, &KEPT_ALWAYS, &[DESCRIPTIVE.as_slice(), &LOCATION, &NEVER].concat());
    }
    assert_eq!(filtered(MetadataKeep::None, false), None);
    let only_paths = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/" xml:lang="en" crs:Exposure2012="+1"><xmpMM:History/></rdf:Description></rdf:RDF>"#;
    assert_eq!(super::xmp::descriptions(only_paths.as_bytes(), policy(MetadataKeep::All, false)).unwrap(), None);
    for invalid in ["<rdf:RDF", "<!DOCTYPE x><x/>", "<a>&custom;</a>"] {
        assert!(super::xmp::descriptions(invalid.as_bytes(), policy(MetadataKeep::All, true)).is_err(), "{invalid}");
    }
}

fn rgba8() -> SourceInterpretation {
    SourceInterpretation {
        channels: SourceChannels::Rgba,
        depth: SampleDepth::U8,
        profile: ColorProfile::Builtin(RgbSpace::Srgb),
        profile_assumed: false,
    }
}

/// Encoded with each writer, then read back with the photo readers.
fn round_trips(delivery: &DeliveryMetadata) -> Vec<(&'static str, DecodedPhoto)> {
    let extent = [8, 6];
    let rgba = rgba8();
    let rgb = SourceInterpretation { channels: SourceChannels::Rgb, ..rgba.clone() };
    let fill = |_: u32, row: &mut [u8]| {
        row.fill(200);
        Ok(())
    };
    let hdr = |_: u32, row: &mut [[f32; 4]]| {
        row.fill([2., 0.5, 0.25, 1.]);
        Ok(())
    };
    let mut files = Vec::new();
    let mut png = Vec::new();
    write_png_rows(&mut png, extent, &rgba, delivery, fill).unwrap();
    files.push(("PNG", png));
    let mut tiff = Cursor::new(Vec::new());
    write_tiff_rows(&mut tiff, extent, &rgba, delivery, fill).unwrap();
    files.push(("TIFF", tiff.into_inner()));
    let mut jpeg = Vec::new();
    write_jpeg_rows(&mut jpeg, extent, &rgb, delivery, JpegEncodeOptions::from_memory_budget(90, PhotoMemoryBudget::current()), fill).unwrap();
    files.push(("JPEG", jpeg));
    let mut webp = Vec::new();
    write_webp_rows(&mut webp, extent, &rgba, delivery, WebpEncodeOptions::from_memory_budget(PhotoMemoryBudget::current()), fill).unwrap();
    files.push(("WebP", webp));
    let mut pq = Vec::new();
    write_hdr_png_rows(&mut pq, extent, RgbSpace::Srgb, delivery, true, hdr).unwrap();
    files.push(("HDR PNG", pq));
    let guide = super::gainmap::test_guide(extent, hdr);
    for (name, format) in [("HDR JPEG", GainMapFormat::Jpeg), ("HDR AVIF", GainMapFormat::Avif)] {
        let mut bytes = Vec::new();
        write_gainmap_rows(
            &mut bytes, extent, RgbSpace::Srgb, Default::default(), &guide, format,
            GainMapEncodeOptions::from_memory_budget(90, PhotoMemoryBudget::current()),
            delivery, Some([1.; 3]), true, &Default::default(), hdr,
        )
        .unwrap();
        files.push((name, bytes));
    }
    let mut exr = Cursor::new(Vec::new());
    write_exr_rows(&mut exr, extent, RgbSpace::Srgb, delivery.resolution, hdr).unwrap();
    files.push(("OpenEXR", exr.into_inner()));
    files
        .into_iter()
        .map(|(name, bytes)| {
            let photo = read_photo_detailed(Cursor::new(bytes), Default::default())
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(photo.source.extent, extent, "{name}");
            (name, photo)
        })
        .collect()
}

#[test]
fn every_writer_delivers_the_kept_metadata_and_openexr_none() {
    let photo = opened().metadata;
    for (keep, remove_location) in [
        (MetadataKeep::All, true),
        (MetadataKeep::All, false),
        (MetadataKeep::CopyrightContact, true),
        (MetadataKeep::None, true),
    ] {
        let delivery = DeliveryMetadata {
            resolution: Some(ImageResolution::ppi(300)),
            photo: photo.clone(),
            policy: policy(keep, remove_location),
        };
        for (name, decoded) in round_trips(&delivery) {
            let case = format!("{name} {keep:?} remove location {remove_location}");
            let metadata = decoded.metadata;
            assert_eq!(metadata.iptc, None, "{case}: IPTC-IIM is not written");
            if name == "OpenEXR" || keep == MetadataKeep::None {
                assert_eq!(metadata.exif, None, "{case}");
                assert_eq!(metadata.xmp.is_some(), name == "HDR JPEG", "{case}: only the gain-map container's own XMP");
                continue;
            }
            let found = exif::read_block(metadata.exif.as_ref().expect(&case)).unwrap().directories;
            assert_eq!(text(&found, 0x013b).as_deref(), Some("Ada Painter"), "{case}");
            assert_eq!(text(&found, 0x8298).as_deref(), Some("(c) 2026 Ada Painter"), "{case}");
            let camera = keep == MetadataKeep::All;
            assert_eq!(text(&found, 0x010f).is_some(), camera, "{case}: make");
            assert_eq!(text(&found, 0xa434).is_some(), camera, "{case}: lens");
            assert_eq!(text(&found, 0x9003).is_some(), camera, "{case}: date taken");
            assert_eq!(found.exif.iter().any(|e| e.tag == 0x829a), camera, "{case}: exposure");
            assert_eq!(!found.gps.is_empty(), camera && !remove_location, "{case}: location");
            let xmp = String::from_utf8(metadata.xmp.expect(&case).to_vec()).unwrap();
            for value in KEPT_ALWAYS {
                assert!(xmp.contains(value), "{case}: {value}");
            }
            for value in NEVER {
                assert!(!xmp.contains(value), "{case}: {value}");
            }
            assert_eq!(xmp.contains("Harbour"), camera, "{case}");
            assert_eq!(xmp.contains("Lisbon"), camera && !remove_location, "{case}");
            assert_eq!(xmp.contains("hdr-gain-map"), name == "HDR JPEG", "{case}: merged into the container XMP");
            if name == "HDR JPEG" || name == "HDR AVIF" {
                assert_eq!(decoded.source.interpretation.depth, SampleDepth::F16, "{case}: the gain map still reads");
            }
        }
    }
}

#[test]
fn metadata_too_large_for_one_jpeg_segment_is_refused_with_a_way_out() {
    let mut photo = opened().metadata;
    let padding = format!("<dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:description>", "x".repeat(70_000));
    photo.xmp = Some(PACKET.replace("</rdf:Description>", &(padding + "</rdf:Description>")).into_bytes().into());
    let delivery = DeliveryMetadata { photo, ..Default::default() };
    let rgb = SourceInterpretation { channels: SourceChannels::Rgb, ..rgba8() };
    let error = write_jpeg_rows(Vec::new(), [2, 2], &rgb, &delivery,
        JpegEncodeOptions::from_memory_budget(90, PhotoMemoryBudget::current()), |_, row| { row.fill(9); Ok(()) }).unwrap_err();
    assert!(error.contains("Choose Copyright & Contact or None"), "{error}");
    let mut png = Vec::new();
    write_png_rows(&mut png, [2, 2], &rgba8(), &delivery, |_, row| { row.fill(9); Ok(()) }).unwrap();
    let decoded = read_photo_detailed(Cursor::new(png), Default::default()).unwrap();
    assert!(decoded.metadata.xmp.unwrap().len() > 70_000, "PNG has no segment limit");
}

#[test]
fn metadata_blocks_share_one_allowance() {
    let big = vec![b' '; PhotoMetadata::MAX_BYTES - 10];
    let packet = [PACKET.as_bytes(), &big].concat();
    let metadata = super::metadata::collect(exif::read_block(&camera_exif()).ok(), Some(packet), Some(vec![1; 64]));
    assert!(metadata.exif.is_some());
    assert_eq!(metadata.xmp, None, "the packet does not fit beside the Exif block");
    assert_eq!(metadata.iptc.as_deref(), Some([1; 64].as_slice()));
    metadata.validate().unwrap();
    assert!(Directories::default().is_empty());
}
