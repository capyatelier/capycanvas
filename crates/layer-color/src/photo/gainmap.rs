//! One canonical gain map for edited HDR and saved SDR renditions. Alpha is
//! coverage, never an input to gain calculation. Container adapters use the
//! same RGB log gain and identical channel metadata (required by Adobe XMP).
use super::*;
use layer_core::color::hdr::SdrRendition;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GainMapFormat {
    Jpeg,
    Avif,
}

/// Per-operation codec allowance. Hosts may supply a smaller process/device
/// policy; the default snapshots the same memory budget as ordinary photo IO.
#[derive(Clone, Copy, Debug)]
pub struct GainMapEncodeOptions {
    pub quality: u8,
    pub memory: PhotoMemoryBudget,
}
impl GainMapEncodeOptions {
    pub fn from_memory_budget(quality: u8, memory: PhotoMemoryBudget) -> Self {
        Self { quality, memory }
    }
}
impl From<u8> for GainMapEncodeOptions {
    fn from(quality: u8) -> Self {
        Self::from_memory_budget(quality, PhotoMemoryBudget::current())
    }
}
#[derive(Clone, Copy, Debug)]
pub struct GainMapMetadata {
    pub min_log2: f32,
    pub max_log2: f32,
    pub offset: f32,
    pub headroom: f32,
}
impl GainMapMetadata {
    pub fn encode(self, gain: f32) -> f32 {
        ((gain - self.min_log2) / (self.max_log2 - self.min_log2)).clamp(0., 1.)
    }
    pub fn reconstruct(self, base: f32, encoded: f32) -> f32 {
        (base + self.offset) * (self.min_log2 + encoded * (self.max_log2 - self.min_log2)).exp2()
            - self.offset
    }
}

#[cfg(all(test, feature = "native-codec-reference", target_os = "linux"))]
pub(super) mod native;
mod jpeg;
mod jpeg_container;
mod metadata;
pub(super) use metadata::Metadata;
pub fn write_gainmap_rows(
    output: impl Write,
    extent: [u32; 2],
    space: RgbSpace,
    rendition: SdrRendition,
    format: GainMapFormat,
    quality: u8,
    resolution: Option<layer_core::ImageResolution>,
    matte: Option<[f32; 3]>,
    clip: bool,
    cancelled: &AtomicBool,
    read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<crate::OutputStatistics, String> {
    write_gainmap_rows_with_guide(
        output, extent, space, rendition, None, format, quality, resolution, matte, clip,
        cancelled, read,
    )
}
pub fn write_gainmap_rows_with_guide(
    output: impl Write,
    extent: [u32; 2],
    space: RgbSpace,
    rendition: SdrRendition,
    guide: Option<&layer_core::color::hdr::LocalToneGuide>,
    format: GainMapFormat,
    options: impl Into<GainMapEncodeOptions>,
    resolution: Option<layer_core::ImageResolution>,
    matte: Option<[f32; 3]>,
    clip: bool,
    cancelled: &AtomicBool,
    read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<crate::OutputStatistics, String> {
    let options = options.into();
    if format == GainMapFormat::Jpeg {
        return jpeg::write(output, extent, space, rendition, guide, options, resolution, matte, clip, cancelled, read);
    }
    super::avif_io::write(output, extent, space, rendition, guide, options, resolution, matte,
        clip, cancelled, read)
}

pub fn preview_gainmap_rows(
    extent: [u32; 2],
    bounds: [u32; 2],
    space: RgbSpace,
    rendition: SdrRendition,
    format: GainMapFormat,
    quality: u8,
    matte: Option<[f32; 3]>,
    cancelled: &AtomicBool,
    read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<
    (
        [u32; 2],
        Vec<[f32; 4]>,
        Vec<[f32; 4]>,
        crate::OutputStatistics,
    ),
    String,
> {
    preview_gainmap_rows_with_guide(
        extent, bounds, space, rendition, None, format, quality, matte, cancelled, read,
    )
}
pub fn preview_gainmap_rows_with_guide(
    extent: [u32; 2],
    bounds: [u32; 2],
    space: RgbSpace,
    rendition: SdrRendition,
    guide: Option<&layer_core::color::hdr::LocalToneGuide>,
    format: GainMapFormat,
    options: impl Into<GainMapEncodeOptions>,
    matte: Option<[f32; 3]>,
    cancelled: &AtomicBool,
    read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<
    (
        [u32; 2],
        Vec<[f32; 4]>,
        Vec<[f32; 4]>,
        crate::OutputStatistics,
    ),
    String,
> {
    let options = options.into();
    if format == GainMapFormat::Jpeg {
        return jpeg::preview(extent, bounds, space, rendition, guide, options, matte, cancelled, read);
    }
    super::avif_io::preview(extent, bounds, space, rendition, guide, options, matte, cancelled, read)
}

pub(super) fn read_gainmap(input: impl Read + Seek, format: GainMapFormat, limits: DecodeLimits, cancelled: &AtomicBool) -> Result<SourceImage, String> {
    if format == GainMapFormat::Jpeg { return jpeg::read(input, limits, cancelled); }
    Ok(super::avif_io::read(std::io::BufReader::new(input), limits, cancelled)?.source)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gainmap_host_budgets_cover_encoding_and_preview_decoding() {
        let extent = [16, 16];
        let cancel = AtomicBool::new(false);
        for format in [GainMapFormat::Jpeg, GainMapFormat::Avif] {
            let memory = PhotoMemoryBudget { source_bytes: 0, decode_bytes: 0, encode_bytes: 0 };
            let mut reads = 0;
            let result = write_gainmap_rows_with_guide(
                Vec::new(), extent, RgbSpace::Srgb, Default::default(), None, format,
                GainMapEncodeOptions::from_memory_budget(90, memory), None, None, false,
                &cancel, |_, row| { reads += 1; row.fill([2., 0.5, 0.25, 1.]); Ok(()) },
            );
            assert!(result.unwrap_err().contains("memory budget"));
            assert_eq!(reads, 0, "Reject before consuming the captured image");
            let options = GainMapEncodeOptions::from_memory_budget(90,
                PhotoMemoryBudget { encode_bytes: 128 * 1024 * 1024, ..memory });
            let result = preview_gainmap_rows_with_guide(
                extent, [8, 8], RgbSpace::Srgb, Default::default(), None, format,
                options, None, &cancel, |_, row| { row.fill([2., 0.5, 0.25, 1.]); Ok(()) },
            );
            assert!(result.unwrap_err().contains("memory budget"),
                "Preview decoding must not replace the host's zero allowance with a default");
            let options = GainMapEncodeOptions::from_memory_budget(90,
                PhotoMemoryBudget::from_available_memory(512 * 1024 * 1024));
            let (_, hdr, _, _) = preview_gainmap_rows_with_guide(
                extent, [8, 8], RgbSpace::Srgb, Default::default(), None, format,
                options, None, &cancel, |_, row| { row.fill([2., 0.5, 0.25, 1.]); Ok(()) },
            ).unwrap();
            assert!(hdr.iter().all(|p| p[0] > 1.9), "A later admitted preview retains HDR");
        }
    }
    #[test]
    #[ignore = "large AVIF grid qualification; run in release mode"]
    fn avif_grid_preserves_partial_cells_alpha_and_gain_samples() {
        let started = std::time::Instant::now();
        let extent: [u32; 2] = std::env::var("LAYER_AVIF_GRID_EXTENT")
            .map(|s| serde_json::from_str(&s).unwrap()).unwrap_or([1031, 1037]);
        assert!(extent.into_iter().all(|n| (1024..=16384).contains(&n)));
        let cancel = AtomicBool::new(false);
        let pixel = |x: u32, y: u32| {
            let a = 0.25 + 0.75 * x as f32 / (extent[0] - 1) as f32;
            let v = if x < extent[0].div_ceil(2) { 0.125 } else { 4. };
            [v*a, (0.1+y as f32/(extent[1] - 1) as f32)*a, 0.2*a, a]
        };
        let read = |y, row: &mut [[f32; 4]]| { for (x,p) in row.iter_mut().enumerate() { *p = pixel(x as u32,y); } Ok(()) };
        let mut encoded = Vec::new();
        write_gainmap_rows(&mut encoded, extent, RgbSpace::Srgb, SdrRendition::default(),
            GainMapFormat::Avif, 90, Some(layer_core::ImageResolution::ppi(300)), None, false, &cancel, read).unwrap();
        let encode_time = started.elapsed();
        let encoded_bytes = encoded.len();
        if let Some(directory) = std::env::var_os("LAYER_AVIF_OUTPUT") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(format!("{}x{}-q90.avif", extent[0], extent[1])), &encoded).unwrap();
        }
        let started = std::time::Instant::now();
        let source = read_photo(std::io::Cursor::new(encoded), Default::default()).unwrap();
        let decode_time = started.elapsed();
        assert_eq!(source.extent, extent);
        assert_eq!(source.interpretation.depth, SampleDepth::F16);
        assert!(source.resolution.is_some());
        let mut rows = source.rows();
        let mut row = vec![0; source.row_bytes()];
        for y in [0, 207, 208, 209, 255, 256, 257, 415, 416, 417, extent[1] - 1] {
            rows.read(y, &mut row).unwrap();
            for x in [0, 207, 208, 209, 255, 256, 257, 415, 416, 417, extent[0]/2, extent[0]/2+1, extent[0] - 1] {
                let at = x as usize*8;
                let p = layer_core::color::hdr::decode_pixel(std::array::from_fn(|c| u16::from_le_bytes([row[at+c*2], row[at+c*2+1]]))).unwrap();
                let expected = pixel(x,y);
                for c in 0..3 { assert!((p[c]-expected[c]/expected[3]).abs() < 0.03+0.04*expected[c]/expected[3], "grid ({x},{y}) channel {c}: {p:?} != {expected:?}"); }
                assert!((p[3]-expected[3]).abs() < 0.001);
            }
        }
        eprintln!("AVIF grid {extent:?}: {encoded_bytes} bytes, encode {encode_time:?}, decode {decode_time:?}");
    }
    #[test]
    fn unified_white_fallback_reconstructs_saturated_hdr_in_both_formats() {
        let cancel = AtomicBool::new(false);
        for format in [GainMapFormat::Jpeg, GainMapFormat::Avif] {
            let mut samples = Vec::new();
            for highlight_color in [0., 1.] {
                let recipe = SdrRendition {
                    highlight_color,
                    ..SdrRendition::default()
                };
                let alpha = if format == GainMapFormat::Avif {
                    0.5
                } else {
                    1.
                };
                let read = |_: u32, row: &mut [[f32; 4]]| {
                    row.fill([8. * alpha, 0., 0., alpha]);
                    Ok(())
                };
                let guide = crate::build_local_tone_guide([32,32],RgbSpace::Srgb,||false,read).unwrap();
                let expected = recipe.mapper(RgbSpace::Srgb,RgbSpace::Srgb)
                    .map_local_premultiplied([8.*alpha,0.,0.,alpha],[0.5,0.5],&guide);
                let (_, hdr, base, stats) = preview_gainmap_rows(
                    [32, 32],
                    [32, 32],
                    RgbSpace::Srgb,
                    recipe,
                    format,
                    100,
                    None,
                    &cancel,
                    read,
                )
                .unwrap();
                assert_eq!(stats.clipped_channels, 0);
                let p = hdr[0];
                eprintln!(
                    "UNIFIED_GAINMAP {format:?} color={highlight_color} hdr={p:?} base={:?}",
                    base[0]
                );
                assert!(
                    (p[0] / p[3] - 8.).abs() < 0.10
                        && (p[1] / p[3]).abs() < 0.07
                        && (p[2] / p[3]).abs() < 0.07,
                    "HDR color reconstruction: {p:?}"
                );
                assert!((p[3] - alpha).abs() < 0.002);
                for c in 0..3 {
                    assert!((base[0][c]-expected[c]).abs()/alpha < 0.012,
                        "{format:?} authored SDR: {:?} vs {expected:?}",base[0]);
                }
                samples.push(base[0]);
            }
            let white = samples[0];
            let color = samples[1];
            assert!(
                white[1] / white[3] > color[1] / color[3] + 0.3,
                "SDR fallback must whiten while HDR stays red: {samples:?}"
            );
        }
    }
    #[test]
    fn canonical_gain_preserves_the_authored_pair_and_near_black() {
        let m = GainMapMetadata {
            min_log2: -8.,
            max_log2: 12.,
            offset: 1. / 64.,
            headroom: 4.,
        };
        for (base, hdr) in [(0., 0.), (0., 4.), (0.18, 0.02), (0.8, 8.), (0.4, 0.00001)] {
            let log = ((hdr + m.offset) / (base + m.offset)).log2();
            let restored = m.reconstruct(base, m.encode(log));
            assert!((restored - hdr).abs() < 1e-5, "{base} {hdr} {restored}");
        }
    }
    #[test]
    fn gainmap_rendition_changes_regenerate_fallback_and_both_jpeg_metadata_paths() {
        let cancel = AtomicBool::new(false);
        let extent = [32, 24];
        for format in [GainMapFormat::Jpeg, GainMapFormat::Avif] {
            let mut fallbacks = Vec::new();
            // Bounded SDR brightness preserves white; use a 50% adjustment
            // to exercise a clearly different fallback at this bright input.
            for exposure in [0., -2.] {
                let rendition = SdrRendition {
                    exposure,
                    ..Default::default()
                };
                let read = |_: u32, row: &mut [[f32; 4]]| {
                    row.fill([2., 2., 2., 1.]);
                    Ok(())
                };
                let (_, hdr, base, stats) = preview_gainmap_rows(
                    extent,
                    extent,
                    RgbSpace::Srgb,
                    rendition,
                    format,
                    90,
                    None,
                    &cancel,
                    read,
                )
                .unwrap();
                assert_eq!(stats.clipped_channels, 0);
                let guide=crate::build_local_tone_guide(extent,RgbSpace::Srgb,||false,read).unwrap();
                let adjusted=rendition.mapper(RgbSpace::Srgb,RgbSpace::Srgb).map_local_premultiplied([2.,2.,2.,1.],[0.5,0.5],&guide);
                let expected = [adjusted[0],adjusted[1],adjusted[2]];
                for p in &hdr {
                    for c in 0..3 {
                        assert!((p[c] - 2.).abs() < 0.035, "{format:?}: {p:?}");
                    }
                }
                for p in &base {
                    for c in 0..3 {
                        assert!(
                            (p[c] - expected[c]).abs() < 0.012,
                            "{format:?}: {p:?} {expected:?}"
                        );
                    }
                }
                fallbacks.push(base[0][0]);
                if let Some(directory) = std::env::var_os("LAYER_GAINMAP_OUTPUT") {
                    let directory = std::path::PathBuf::from(directory);
                    std::fs::create_dir_all(&directory).unwrap();
                    let stem = format!(
                        "{}-{}",
                        if format == GainMapFormat::Jpeg {
                            "jpeg"
                        } else {
                            "avif"
                        },
                        if exposure == 0. { "neutral" } else { "dark" }
                    );
                    let mut file = Vec::new();
                    write_gainmap_rows(
                        &mut file,
                        extent,
                        RgbSpace::Srgb,
                        rendition,
                        format,
                        90,
                        None,
                        None,
                        false,
                        &cancel,
                        read,
                    )
                    .unwrap();
                    std::fs::write(
                        directory.join(format!(
                            "{stem}.{}",
                            if format == GainMapFormat::Jpeg {
                                "jpg"
                            } else {
                                "avif"
                            }
                        )),
                        file,
                    )
                    .unwrap();
                    std::fs::write(
                        directory.join(format!("{stem}.json")),
                        serde_json::to_vec(
                            &base[0]
                                .map(|v| (RgbSpace::Srgb.encode(v as f64) * 255.).round() as u8),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                }

                if format == GainMapFormat::Jpeg {
                    let mut file = Vec::new();
                    write_gainmap_rows(
                        &mut file,
                        extent,
                        RgbSpace::Srgb,
                        rendition,
                        format,
                        90,
                        None,
                        None,
                        false,
                        &cancel,
                        read,
                    )
                    .unwrap();
                    let original = read_gainmap(
                        std::io::Cursor::new(&file),
                        format,
                        DecodeLimits::default(),
                        &cancel,
                    )
                    .unwrap();
                    let mut original_row = vec![0; original.row_bytes()];
                    original.rows().read(0, &mut original_row).unwrap();
                    // Obscure one metadata signature without changing segment
                    // lengths or MPF offsets. The remaining schema must suffice.
                    for signature in [
                        b"urn:iso:std:iso:ts:21496:-1".as_slice(),
                        b"http://ns.adobe.com/xap/1.0/",
                    ] {
                        let mut isolated = file.clone();
                        let positions = isolated
                            .windows(signature.len())
                            .enumerate()
                            .filter_map(|(i, b)| (b == signature).then_some(i))
                            .collect::<Vec<_>>();
                        assert!(!positions.is_empty());
                        for i in positions {
                            isolated[i] = b'x';
                        }
                        let source = read_gainmap(
                            std::io::Cursor::new(isolated),
                            format,
                            DecodeLimits::default(),
                            &cancel,
                        )
                        .unwrap();
                        let mut row = vec![0; source.row_bytes()];
                        source.rows().read(0, &mut row).unwrap();
                        for (a, b) in row.chunks_exact(2).zip(original_row.chunks_exact(2)) {
                            let a = half_value(a);
                            let b = half_value(b);
                            assert!((a - b).abs() < 0.004, "metadata paths diverged: {a} {b}");
                        }
                    }
                }
            }
            assert!(
                fallbacks[0] - fallbacks[1] > 0.1,
                "SDR brightness must change the encoded base: {fallbacks:?}"
            );
        }
        fn half_value(b: &[u8]) -> f32 {
            layer_core::color::hdr::decode_pixel([u16::from_le_bytes([b[0], b[1]]), 0, 0, 0x3c00])
                .unwrap()[0]
        }
    }
    #[test]
    fn gainmap_jpeg_and_transparent_avif_roundtrip_edited_hdr_and_authored_sdr() {
        use std::{io::Cursor, sync::atomic::AtomicBool};
        let extent = [64, 48];
        let row = |y: u32, row: &mut [[f32; 4]], transparent: bool| {
            for (x, p) in row.iter_mut().enumerate() {
                let a = if transparent { (x / 8) as f32 / 7. } else { 1. };
                let v = (x as f32 / 63.).powi(2) * 8.;
                *p = [v * a, (y as f32 / 47. * 2.) * a, 0.2 * a, a];
            }
            Ok(())
        };
        for format in [GainMapFormat::Jpeg, GainMapFormat::Avif] {
            let transparent = format == GainMapFormat::Avif;
            let mut encoded = Vec::new();
            write_gainmap_rows(
                &mut encoded,
                extent,
                RgbSpace::Srgb,
                SdrRendition::default(),
                format,
                100,
                Some(layer_core::ImageResolution::ppi(300)),
                None,
                false,
                &AtomicBool::new(false),
                |y, r| row(y, r, transparent),
            )
            .unwrap();
            if format == GainMapFormat::Jpeg {
                for signature in [
                    b"urn:iso:std:iso:ts:21496:-1".as_slice(),
                    b"http://ns.adobe.com/hdr-gain-map/1.0/",
                ] {
                    assert!(
                        encoded.windows(signature.len()).any(|w| w == signature),
                        "both metadata schemas must exist"
                    );
                }
            }
            let directory = std::env::var_os("LAYER_GAINMAP_OUTPUT").map(std::path::PathBuf::from);
            if let Some(d) = directory {
                std::fs::create_dir_all(&d).unwrap();
                std::fs::write(
                    d.join(if transparent {
                        "edited.avif"
                    } else {
                        "edited.jpg"
                    }),
                    &encoded,
                )
                .unwrap();
            }
            let source = read_photo(Cursor::new(&encoded), DecodeLimits::default()).unwrap();
            assert_eq!(source.extent, extent);
            assert_eq!(
                source.resolution,
                Some(layer_core::ImageResolution::ppi(300))
            );
            assert_eq!(source.interpretation.depth, SampleDepth::F16);
            let mut rows = source.rows();
            let mut raw = vec![0u8; 64 * 8];
            let mut expected = vec![[0.; 4]; 64];
            let mut largest = 0f32;
            for y in 0..48 {
                rows.read(y, &mut raw).unwrap();
                row(y, &mut expected, transparent).unwrap();
                for (bytes, p) in raw.chunks_exact(8).zip(&expected) {
                    let bits = std::array::from_fn(|c| {
                        u16::from_le_bytes([bytes[c * 2], bytes[c * 2 + 1]])
                    });
                    let v = layer_core::color::hdr::decode_pixel(bits).unwrap();
                    assert!((v[3] - p[3]).abs() < 0.0005);
                    if p[3] > 0. {
                        for c in 0..3 {
                            let e = (v[c] - p[c] / p[3]).abs();
                            largest = largest.max(e);
                            assert!(e < 0.16, "{format:?} {y}: {v:?} vs {p:?}, {e}");
                        }
                    }
                }
            }
            eprintln!(
                "{format:?}: {} bytes, largest absolute HDR channel error {largest}",
                encoded.len()
            );
        }
    }
    #[test]
    fn local_gainmaps_reconstruct_the_master_from_spatially_different_bases() {
        let extent=[64,48];let cancel=AtomicBool::new(false);
        for format in [GainMapFormat::Jpeg,GainMapFormat::Avif] {
            let alpha=if format==GainMapFormat::Avif {0.5}else{1.};
            let read=|y:u32,row:&mut [[f32;4]]| {for (x,p) in row.iter_mut().enumerate(){let v=if x<32 {0.05}else{8.}*if (x/8+y as usize/8)%2==0 {0.8}else{1.2};*p=[v*alpha,v*0.7*alpha,v*0.4*alpha,alpha];}Ok(())};
            let guide=crate::build_local_tone_guide(extent,RgbSpace::Srgb,||false,read).unwrap();
            let mut bases=Vec::new();
            for (contrast,balance) in [(0.5,-1.),(2.,1.)] {
                let recipe=SdrRendition{contrast,balance,headroom:guide.peak.log2(),..Default::default()};
                let (_,hdr,base,stats)=preview_gainmap_rows_with_guide(extent,extent,RgbSpace::Srgb,recipe,Some(&guide),format,100,None,&cancel,read).unwrap();assert_eq!(stats.clipped_channels,0);
                let mut max_error=0f32;
                for (x,y) in [(12,12),(20,20),(44,12),(52,20)] {
                    let mut row=vec![[0.;4];64];read(y,&mut row).unwrap();let p=row[x];let i=y as usize*64+x;
                    let expected=recipe.mapper(RgbSpace::Srgb,RgbSpace::Srgb).map_local_premultiplied(p,[x as f32+0.5,y as f32+0.5],&guide);
                    for c in 0..3 {
                        let e=(hdr[i][c]-p[c]).abs()/alpha;max_error=max_error.max(e);
                        // Lossy 8-bit RGB JPEG gains amplify code error across the
                        // gain range. Bound reconstruction separately from SDR base.
                        assert!(e < 0.03+0.04*p[c]/alpha,"{format:?} HDR {hdr:?} expected={p:?}");
                        // Two 8-bit BT.2020 code steps can exceed 0.018 in
                        // linear sRGB near white. AVIF's 12-bit lossless base
                        // has a much tighter quantization bound.
                        let base_tolerance=if format==GainMapFormat::Jpeg {0.03}else{0.003};
                        assert!((base[i][c]-expected[c]).abs()/alpha<base_tolerance,"{format:?} SDR {:?} vs {expected:?}",base[i]);
                    }
                    assert!((hdr[i][3]-alpha).abs()<0.001);
                }
                eprintln!("LOCAL_GAINMAP {format:?} contrast={contrast} balance={balance} sampled_max_hdr_error={max_error}");bases.push(base);
            }
            assert!(bases[0].iter().zip(&bases[1]).any(|(a,b)|(a[0]-b[0]).abs()/alpha>0.1),"Contrast/Balance must change the encoded SDR base");
        }
    }

}
