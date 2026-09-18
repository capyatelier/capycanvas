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

#[cfg(all(feature = "heif", target_os = "linux"))]
mod native;
pub fn gainmap_available() -> bool {
    #[cfg(all(feature = "heif", target_os = "linux"))]
    {
        return native::available();
    }
    #[cfg(not(all(feature = "heif", target_os = "linux")))]
    {
        false
    }
}

#[allow(unused_variables)]
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
    #[cfg(all(feature = "heif", target_os = "linux"))]
    {
        native::write(
            output, extent, space, rendition, format, quality, resolution, matte, clip, cancelled,
            read,
        )
    }
    #[cfg(not(all(feature = "heif", target_os = "linux")))]
    {
        Err("HDR gain-map export is unavailable on this host".into())
    }
}

#[allow(unused_variables)]
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
    #[cfg(all(feature = "heif", target_os = "linux"))]
    {
        native::preview(
            extent, bounds, space, rendition, format, quality, matte, cancelled, read,
        )
    }
    #[cfg(not(all(feature = "heif", target_os = "linux")))]
    {
        Err("HDR gain-map preview is unavailable on this host".into())
    }
}

#[cfg(all(feature = "heif", target_os = "linux"))]
pub(super) use native::read_gainmap;

#[cfg(test)]
mod tests {
    use super::*;
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
    #[cfg(all(feature="heif",target_os="linux"))]
    #[test]
    #[ignore="requires pinned native HDR codec bundle"]
    fn gainmap_rendition_changes_regenerate_fallback_and_both_jpeg_metadata_paths() {
        let cancel=AtomicBool::new(false);let extent=[32,24];
        for format in [GainMapFormat::Jpeg,GainMapFormat::Avif] {
            let mut fallbacks=Vec::new();
            for exposure in [0.,-1.] {
                let rendition=SdrRendition{exposure,..Default::default()};
                let read=|_:u32,row:&mut [[f32;4]]|{row.fill([2.,2.,2.,1.]);Ok(())};
                let (_,hdr,base,stats)=preview_gainmap_rows(extent,extent,RgbSpace::Srgb,rendition,format,90,None,&cancel,read).unwrap();
                assert_eq!(stats.clipped_channels,0);
                let expected=rendition.map_rgb([2.;3],RgbSpace::Srgb);
                for p in &hdr{for c in 0..3{assert!((p[c]-2.).abs()<0.035,"{format:?}: {p:?}");}}
                for p in &base{for c in 0..3{assert!((p[c]-expected[c]).abs()<0.012,"{format:?}: {p:?} {expected:?}");}}
                fallbacks.push(base[0][0]);
                if let Some(directory)=std::env::var_os("LAYER_GAINMAP_OUTPUT") {
                    let directory=std::path::PathBuf::from(directory);std::fs::create_dir_all(&directory).unwrap();
                    let stem=format!("{}-{}",if format==GainMapFormat::Jpeg{"jpeg"}else{"avif"},if exposure==0.{"neutral"}else{"dark"});
                    let mut file=Vec::new();write_gainmap_rows(&mut file,extent,RgbSpace::Srgb,rendition,format,90,None,None,false,&cancel,read).unwrap();
                    std::fs::write(directory.join(format!("{stem}.{}",if format==GainMapFormat::Jpeg{"jpg"}else{"avif"})),file).unwrap();
                    std::fs::write(directory.join(format!("{stem}.json")),serde_json::to_vec(&base[0].map(|v|(RgbSpace::Srgb.encode(v as f64)*255.).round() as u8)).unwrap()).unwrap();
                }

                if format==GainMapFormat::Jpeg {
                    let mut file=Vec::new();write_gainmap_rows(&mut file,extent,RgbSpace::Srgb,rendition,format,90,None,None,false,&cancel,read).unwrap();
                    let original=read_gainmap(std::io::Cursor::new(&file),format,DecodeLimits::default(),&cancel).unwrap();
                    let mut original_row=vec![0;original.row_bytes()];original.rows().read(0,&mut original_row).unwrap();
                    // Obscure one metadata signature without changing segment
                    // lengths or MPF offsets. The remaining schema must suffice.
                    for signature in [b"urn:iso:std:iso:ts:21496:-1".as_slice(),b"http://ns.adobe.com/xap/1.0/"] {
                        let mut isolated=file.clone();let positions=isolated.windows(signature.len()).enumerate().filter_map(|(i,b)|(b==signature).then_some(i)).collect::<Vec<_>>();assert!(!positions.is_empty());
                        for i in positions{isolated[i]=b'x';}
                        let source=read_gainmap(std::io::Cursor::new(isolated),format,DecodeLimits::default(),&cancel).unwrap();
                        let mut row=vec![0;source.row_bytes()];source.rows().read(0,&mut row).unwrap();
                        for (a,b) in row.chunks_exact(2).zip(original_row.chunks_exact(2)){
                            let a=half_value(a);let b=half_value(b);assert!((a-b).abs()<0.004,"metadata paths diverged: {a} {b}");
                        }
                    }
                }
            }
            assert!(fallbacks[0]-fallbacks[1]>0.1,"SDR exposure must change the encoded base: {fallbacks:?}");
        }
        fn half_value(b:&[u8])->f32 {layer_core::color::hdr::decode_pixel([u16::from_le_bytes([b[0],b[1]]),0,0,0x3c00]).unwrap()[0]}
    }
    #[cfg(all(feature = "heif", target_os = "linux"))]
    #[test]
    #[ignore = "requires pinned native HDR codec bundle"]
    fn gainmap_jpeg_and_transparent_avif_roundtrip_edited_hdr_and_authored_sdr() {
        use std::{io::Cursor, sync::atomic::AtomicBool};
        assert!(gainmap_available());
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
}
