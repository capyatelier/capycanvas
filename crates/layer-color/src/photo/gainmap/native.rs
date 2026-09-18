use super::*;
use layer_core::color::{hdr, rgb};
use std::{
    fs::{self, File},
    io::{BufReader, BufWriter},
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
const OFFSET: f32 = 1. / 64.;
const MAX_BUDGET: usize = 16 * 1024 * 1024 * 1024;

fn worker() -> Result<PathBuf, String> {
    let mut paths = Vec::new();
    if let Some(p) = std::env::var_os("CAPY_PHOTO_CODEC_DIR") {
        paths.push(PathBuf::from(p));
    } else if let Ok(exe) = std::env::current_exe() {
        if let Some(p) = exe.parent() {
            for path in [
                "../lib/capycanvas/photo",
                "../photo-codecs/prefix/lib",
                "../../photo-codecs/prefix/lib",
            ] {
                paths.push(p.join(path));
            }
        }
    }
    paths
        .into_iter()
        .find(|p| {
            fs::read_to_string(p.join("hdr-codec-abi")).ok().as_deref() == Some("1\n")
                && p.join("capy-hdr-codec").is_file()
        })
        .map(|p| p.join("capy-hdr-codec"))
        .ok_or("HDR JPEG/AVIF codecs are not installed".into())
}
pub(super) fn available() -> bool {
    worker().is_ok()
}
struct Stage(PathBuf);
impl Stage {
    fn new() -> Result<Self, String> {
        use std::os::unix::fs::DirBuilderExt;
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!(
                "capy-hdr-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(err(e)),
            }
        }
        Err("Cannot create private HDR staging directory".into())
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn create(&self, name: &str) -> Result<BufWriter<File>, String> {
        File::create(self.path(name))
            .map(BufWriter::new)
            .map_err(err)
    }
    fn open(&self, name: &str) -> Result<BufReader<File>, String> {
        File::open(self.path(name)).map(BufReader::new).map_err(err)
    }
    fn run(
        &self,
        mode: &str,
        extent: [u32; 2],
        quality: u8,
        budget: usize,
        dimension: u32,
        cancel: &AtomicBool,
    ) -> Result<(), String> {
        check(cancel)?;
        let errors = File::create(self.path("errors")).map_err(err)?;
        let mut child = Command::new(worker()?)
            .current_dir(&self.0)
            .args([
                mode,
                &extent[0].to_string(),
                &extent[1].to_string(),
                &quality.to_string(),
                &budget.to_string(),
                &dimension.to_string(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(errors)
            .spawn()
            .map_err(err)?;
        let start = Instant::now();
        loop {
            let status = match child.try_wait() {
                Ok(v) => v,
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(err(e));
                }
            };
            if let Some(status) = status {
                check(cancel)?;
                if status.success() {
                    return Ok(());
                }
                let mut message = String::new();
                File::open(self.path("errors"))
                    .map_err(err)?
                    .take(2048)
                    .read_to_string(&mut message)
                    .map_err(err)?;
                return Err(format!(
                    "HDR codec failed: {}",
                    if message.trim().is_empty() {
                        "memory or codec limit reached"
                    } else {
                        message.trim()
                    }
                ));
            }
            if cancel.load(Ordering::Acquire) || start.elapsed() > Duration::from_secs(600) {
                let _ = child.kill();
                let _ = child.wait();
                check(cancel)?;
                return Err("HDR codec exceeded its time budget".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn check(c: &AtomicBool) -> Result<(), String> {
    if c.load(Ordering::Acquire) {
        Err("HDR operation cancelled".into())
    } else {
        Ok(())
    }
}
fn budget(extent: [u32; 2], available: usize) -> Result<usize, String> {
    validate_extent(extent, 32768)?;
    let needed = (extent[0] as usize)
        .checked_mul(extent[1] as usize)
        .and_then(|p| p.checked_mul(96))
        .and_then(|p| p.checked_add(128 * 1024 * 1024))
        .ok_or("HDR codec size overflow")?;
    if needed > available.min(MAX_BUDGET) {
        return Err("HDR gain-map output exceeds the available memory budget. Choose a smaller export size.".into());
    }
    Ok(needed)
}
fn copy_checked(
    mut input: impl Read,
    mut output: impl Write,
    cancel: &AtomicBool,
    limit: u64,
) -> Result<(), String> {
    let mut bytes = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        check(cancel)?;
        let n = input.read(&mut bytes).map_err(err)?;
        if n == 0 {
            break;
        }
        total = total.checked_add(n as u64).ok_or("HDR input overflow")?;
        if total > limit {
            return Err("HDR file exceeds the memory budget".into());
        }
        output.write_all(&bytes[..n]).map_err(err)?;
    }
    output.flush().map_err(err)
}
pub(super) fn write(
    output: impl Write,
    extent: [u32; 2],
    space: RgbSpace,
    rendition: SdrRendition,
    format: GainMapFormat,
    quality: u8,
    resolution: Option<layer_core::ImageResolution>,
    matte: Option<[f32; 3]>,
    clip: bool,
    cancel: &AtomicBool,
    mut read: impl FnMut(u32, &mut [[f32; 4]]) -> Result<(), String>,
) -> Result<crate::OutputStatistics, String> {
    rendition.validate().map_err(str::to_string)?;
    if !(1..=100).contains(&quality) {
        return Err("Invalid HDR quality".into());
    }
    worker()?;
    let budget = budget(extent, PhotoMemoryBudget::current().encode_bytes)?;
    let stage = Stage::new()?;
    if let Some(resolution) = resolution {
        fs::write(
            stage.path("exif"),
            super::super::metadata::exif_output(resolution)?,
        )
        .map_err(err)?;
    }
    let jpeg = format == GainMapFormat::Jpeg;
    // Both containers use BT.2020 primaries with an sRGB-encoded SDR base. This
    // carries wide-color HDR in the base application space required by XMP.
    let profile = crate::icc::nclx_profile(
        [0.708, 0.292, 0.170, 0.797, 0.131, 0.046, 0.3127, 0.3290],
        13,
    )?;
    fs::write(stage.path("profile"), profile_bytes(&profile)?).map_err(err)?;
    let matrix = hdr::to_bt2020(space);
    let mapper = rendition.mapper(space, space);
    let photographic = rendition.mapper(space, RgbSpace::Srgb);
    let mut base = stage.create("base.raw")?;
    let mut master = stage.create("master")?;
    let mut row = vec![[0.; 4]; extent[0] as usize];
    let mut peak = 1f32;
    let mut stats = crate::OutputStatistics::default();
    for y in 0..extent[1] {
        check(cancel)?;
        read(y, &mut row)?;
        for p in &row {
            if p.iter().any(|v| !v.is_finite()) || !(0. ..=1.).contains(&p[3]) {
                return Err("Invalid HDR output pixel".into());
            }
            let a = p[3];
            if jpeg && matte.is_none() && a < 1. {
                return Err("Enable Flatten transparency for HDR JPEG".into());
            }
            let raw = if a > 0. {
                [p[0] / a, p[1] / a, p[2] / a]
            } else {
                [0.; 3]
            };
            let mut hdr = rgb::apply(matrix, raw.map(f64::from)).map(|v| v as f32);
            // The photographic base uses the same bounded sRGB rendition as
            // ordinary sharing. Encode those colors in the gain-map application
            // space; RGB gains still reconstruct the original wide-color HDR.
            let mut sdr = if rendition.method == hdr::SdrMethod::Photographic {
                rgb::apply(hdr::srgb_to_bt2020(), photographic.map_rgb(raw).map(f64::from)).map(|v|v as f32)
            } else { rgb::apply(matrix, mapper.tone_rgb(raw).map(f64::from)).map(|v| v as f32) };
            for c in 0..3 {
                sdr[c] = sdr[c].clamp(0., 1.);
                if let Some(background) = matte {
                    hdr[c] = hdr[c] * a + background[c] * (1. - a);
                    sdr[c] = sdr[c] * a + background[c] * (1. - a);
                }
                // Matrix rounding can produce tiny negative neutral channels.
                if hdr[c] < -1e-6 || hdr[c] > hdr::MAX_LINEAR {
                    if !clip {
                        return Err("HDR gain-map output exceeds BT.2020 or the half-float range. Enable Clip out-of-range colors to export a mapped copy.".into());
                    }
                    stats.clipped_channels += 1;
                }
                hdr[c] = hdr[c].clamp(0., hdr::MAX_LINEAR);
                peak = peak.max(hdr[c]);
                master.write_all(&hdr[c].to_le_bytes()).map_err(err)?;
                let code = RgbSpace::Srgb.encode(sdr[c] as f64).clamp(0., 1.);
                if jpeg {
                    base.write_all(&[(code * 255.).round() as u8])
                        .map_err(err)?;
                } else {
                    base.write_all(&((code * 4095.).round() as u16).to_le_bytes())
                        .map_err(err)?;
                }
            }
            if !jpeg {
                base.write_all(
                    &((if matte.is_some() { 1. } else { a } * 4095.).round() as u16).to_le_bytes(),
                )
                .map_err(err)?;
            }
        }
    }
    base.flush().map_err(err)?;
    master.flush().map_err(err)?;
    drop(base);
    drop(master);
    if jpeg {
        stage.run("base-jpeg", extent, quality, budget, 32768, cancel)?;
    }
    let mut master = stage.open("master")?;
    let mut base = stage.open(if jpeg { "base-decoded" } else { "base.raw" })?;
    let mut logs = stage.create("logs")?;
    let mut low = 0f32;
    let mut high = 0f32;
    let mut m = [0u8; 4];
    let mut b = [0u8; 2];
    for _ in 0..extent[1] {
        check(cancel)?;
        for _ in 0..extent[0] {
            for _ in 0..3 {
                master.read_exact(&mut m).map_err(err)?;
                base.read_exact(&mut b[..if jpeg { 1 } else { 2 }])
                    .map_err(err)?;
                let hdr = f32::from_le_bytes(m);
                let code = if jpeg {
                    b[0] as f64 / 255.
                } else {
                    u16::from_le_bytes(b) as f64 / 4095.
                };
                let gain = ((hdr + OFFSET) / (RgbSpace::Srgb.decode(code) as f32 + OFFSET)).log2();
                low = low.min(gain);
                high = high.max(gain);
                logs.write_all(&gain.to_le_bytes()).map_err(err)?;
            }
            if !jpeg {
                base.read_exact(&mut b).map_err(err)?;
            }
        }
    }
    logs.flush().map_err(err)?;
    drop(logs);
    drop(base);
    drop(master);
    let meta = GainMapMetadata {
        min_log2: low,
        max_log2: high.max(low + 0.001),
        offset: OFFSET,
        headroom: peak.log2().max(0.001),
    };
    let bytes = [meta.min_log2, meta.max_log2, meta.offset, meta.headroom]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    fs::write(stage.path("metadata"), bytes).map_err(err)?;
    let mut logs = stage.open("logs")?;
    let mut gain = stage.create("gain.raw")?;
    for _ in 0..extent[1] {
        check(cancel)?;
        for _ in 0..extent[0] * 3 {
            logs.read_exact(&mut m).map_err(err)?;
            let v = meta.encode(f32::from_le_bytes(m));
            if jpeg {
                gain.write_all(&[(v * 255.).round() as u8]).map_err(err)?;
            } else {
                gain.write_all(&((v * 4095.).round() as u16).to_le_bytes())
                    .map_err(err)?;
            }
        }
    }
    gain.flush().map_err(err)?;
    drop(gain);
    drop(logs);
    fs::remove_file(stage.path("master")).map_err(err)?;
    fs::remove_file(stage.path("logs")).map_err(err)?;
    #[cfg(test)]
    if let Some(dir) = std::env::var_os("LAYER_GAINMAP_OUTPUT") {
        let dir = PathBuf::from(dir).join(if jpeg { "jpeg-input" } else { "avif-input" });
        fs::create_dir_all(&dir).map_err(err)?;
        for name in ["base.raw", "gain.raw", "metadata"] {
            fs::copy(stage.path(name), dir.join(name)).map_err(err)?;
        }
    }
    stage.run(
        if jpeg { "mux-jpeg" } else { "encode-avif" },
        extent,
        quality,
        budget,
        32768,
        cancel,
    )?;
    copy_checked(stage.open("encoded")?, output, cancel, budget as u64)?;
    Ok(stats)
}

pub(in super::super) fn read_gainmap(
    input: impl Read,
    format: GainMapFormat,
    limits: DecodeLimits,
    cancel: &AtomicBool,
) -> Result<SourceImage, String> {
    let stage = Stage::new()?;
    let budget = limits.codec_bytes.min(MAX_BUDGET);
    copy_checked(input, stage.create("source")?, cancel, (budget / 4) as u64)?;
    stage.run(
        if format == GainMapFormat::Jpeg {
            "decode-jpeg"
        } else {
            "decode-avif"
        },
        [0, 0],
        90,
        budget,
        limits.dimension,
        cancel,
    )?;
    decoded_source(&stage, format, limits, cancel)
}
fn decoded_source(
    stage: &Stage,
    format: GainMapFormat,
    limits: DecodeLimits,
    cancel: &AtomicBool,
) -> Result<SourceImage, String> {
    let mut input = stage.open("decoded")?;
    let mut header = [0u8; 20];
    input.read_exact(&mut header).map_err(err)?;
    let u = |i| u32::from_le_bytes(header[i..i + 4].try_into().unwrap());
    let extent = [u(0), u(4)];
    limits.extent(extent)?;
    if !matches!(u(12), 8 | 13) || u(16) != 8 {
        return Err("Invalid HDR decoder output".into());
    }
    let matrix = match u(8) {
        1 => RgbSpace::Srgb.linear_transform(RgbSpace::Srgb),
        12 => RgbSpace::DisplayP3.linear_transform(RgbSpace::Srgb),
        9 => hdr::bt2020_to_srgb(),
        _ => return Err("Unsupported HDR color primaries".into()),
    };
    let mut builder = SourceBuilder::new(
        extent,
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::F16,
            profile: ColorProfile::Builtin(RgbSpace::Srgb),
            profile_assumed: false,
        },
        limits.source_bytes,
    )?;
    let avif = format == GainMapFormat::Avif;
    let mut gain = avif.then(|| stage.open("decoded-gain")).transpose()?;
    let metadata = if avif {
        let bytes = fs::read(stage.path("decoded-metadata")).map_err(err)?;
        if bytes.len() != 60 {
            return Err("Invalid AVIF gain metadata".into());
        }
        bytes
            .chunks_exact(4)
            .map(|p| f32::from_le_bytes(p.try_into().unwrap()))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if metadata.iter().any(|v| !v.is_finite()) {
        return Err("Non-finite AVIF gain metadata".into());
    }
    let mut row = vec![0u8; extent[0] as usize * 8];
    let mut gains = vec![0u8; extent[0] as usize * 6];
    for _ in 0..extent[1] {
        check(cancel)?;
        input.read_exact(&mut row).map_err(err)?;
        if let Some(gain) = &mut gain {
            gain.read_exact(&mut gains).map_err(err)?;
        }
        for (i, p) in row.chunks_exact_mut(8).enumerate() {
            let bits = std::array::from_fn(|c| u16::from_le_bytes([p[c * 2], p[c * 2 + 1]]));
            let v = if avif {
                let mut pixel = [0.; 4];
                pixel[3] = bits[3] as f32 / 65535.;
                for c in 0..3 {
                    let code = u16::from_le_bytes([gains[i * 6 + c * 2], gains[i * 6 + c * 2 + 1]])
                        as f32
                        / 65535.;
                    let encoded = code.powf(1. / metadata[6 + c]);
                    let m = GainMapMetadata {
                        min_log2: metadata[c],
                        max_log2: metadata[3 + c],
                        offset: metadata[9 + c],
                        headroom: 1.,
                    };
                    pixel[c] = m.reconstruct(
                        RgbSpace::Srgb.decode(bits[c] as f64 / 65535.) as f32,
                        encoded,
                    ) + metadata[9 + c]
                        - metadata[12 + c];
                }
                pixel
            } else {
                hdr::decode_pixel(bits).map_err(str::to_string)?
            };
            let rgb = rgb::apply(matrix, [v[0] as f64, v[1] as f64, v[2] as f64]);
            let bits = hdr::encode_pixel([rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, v[3]])
                .map_err(str::to_string)?;
            for (b, v) in p.chunks_exact_mut(2).zip(bits) {
                b.copy_from_slice(&v.to_le_bytes());
            }
        }
        builder.push_row(&row)?;
    }
    let mut source = builder.finish()?;
    if avif && stage.path("decoded-exif").exists() {
        source.resolution =
            super::super::metadata::exif(&fs::read(stage.path("decoded-exif")).map_err(err)?)?
                .resolution;
    }
    Ok(source)
}

pub(super) fn preview(
    extent: [u32; 2],
    bounds: [u32; 2],
    space: RgbSpace,
    rendition: SdrRendition,
    format: GainMapFormat,
    quality: u8,
    matte: Option<[f32; 3]>,
    cancel: &AtomicBool,
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
    let stage = Stage::new()?;
    let stats = write(
        stage.create("source")?,
        extent,
        space,
        rendition,
        format,
        quality,
        None,
        matte,
        true,
        cancel,
        read,
    )?;
    let budget = budget(extent, PhotoMemoryBudget::current().decode_bytes)?;
    stage.run(
        if format == GainMapFormat::Jpeg {
            "decode-jpeg"
        } else {
            "decode-avif"
        },
        [0, 0],
        quality,
        budget,
        32768,
        cancel,
    )?;
    let source = decoded_source(&stage, format, DecodeLimits::default(), cancel)?;
    let decoder =
        crate::WorkingDecoder::new(&source.interpretation, RgbSpace::Srgb, Default::default())?;
    let mut hdr_preview = crate::AreaPreview::new(extent, bounds)?;
    let mut rows = source.rows();
    let mut bytes = vec![0u8; source.row_bytes()];
    let mut pixels = vec![[0.; 4]; extent[0] as usize];
    for y in 0..extent[1] {
        check(cancel)?;
        rows.read(y, &mut bytes)?;
        decoder.decode_pixels(&bytes, &mut pixels)?;
        for p in &mut pixels {
            for c in 0..3 {
                p[c] *= p[3];
            }
        }
        hdr_preview.push(&pixels)?;
    }
    let (preview_extent, hdr) = hdr_preview.finish()?;
    drop(rows);
    drop(source);
    let jpeg = format == GainMapFormat::Jpeg;
    let mut base = stage.open(if jpeg { "base-decoded" } else { "fallback" })?;
    if !jpeg {
        let mut header = [0u8; 20];
        base.read_exact(&mut header).map_err(err)?;
    }
    let mut bytes = vec![0u8; extent[0] as usize * if jpeg { 3 } else { 8 }];
    let mut sdr_preview = crate::AreaPreview::new(extent, bounds)?;
    let matrix = hdr::bt2020_to_srgb();
    for _ in 0..extent[1] {
        check(cancel)?;
        base.read_exact(&mut bytes).map_err(err)?;
        for (p, b) in pixels
            .iter_mut()
            .zip(bytes.chunks_exact(if jpeg { 3 } else { 8 }))
        {
            let v = |c| {
                if jpeg {
                    b[c] as f64 / 255.
                } else {
                    u16::from_le_bytes([b[c * 2], b[c * 2 + 1]]) as f64 / 65535.
                }
            };
            let a = if jpeg { 1. } else { v(3) as f32 };
            let rgb = rgb::apply(matrix, [v(0), v(1), v(2)].map(|v| RgbSpace::Srgb.decode(v)));
            *p = [rgb[0] as f32 * a, rgb[1] as f32 * a, rgb[2] as f32 * a, a];
        }
        sdr_preview.push(&pixels)?;
    }
    let (_, sdr) = sdr_preview.finish()?;
    Ok((preview_extent, hdr, sdr, stats))
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    #[ignore="pinned Linux HDR codec bundle; actual process cancellation"]
    fn gainmap_cancel_reaps_an_active_codec_and_removes_staging() {
        let cancelled=std::sync::Arc::new(AtomicBool::new(false));let flag=cancelled.clone();
        let worker=std::thread::spawn(move||{
            write(std::io::sink(),[2048,2048],RgbSpace::Srgb,SdrRendition::default(),GainMapFormat::Avif,90,None,None,false,&flag,|y,row|{
                for (x,p) in row.iter_mut().enumerate(){let n=(x as u32).wrapping_mul(747796405).wrapping_add(y.wrapping_mul(2891336453));let n=(n^(n>>16)).wrapping_mul(2246822519);let v=(n&65535) as f32/65535.;*p=[v*4.,v*2.,0.25,1.];}Ok(())
            })
        });
        let start=Instant::now();let mut codec=None;
        while start.elapsed()<Duration::from_secs(40)&&codec.is_none(){
            for task in fs::read_dir(format!("/proc/{}/task",std::process::id())).unwrap().flatten(){
                if let Ok(children)=fs::read_to_string(task.path().join("children")){for pid in children.split_whitespace(){
                    if fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|p|p.file_name().is_some_and(|n|n=="capy-hdr-codec"))
                        && fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|b|b.windows(11).any(|s|s==b"encode-avif")){
                        codec=Some(pid.to_string());break;
                    }
                }}
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let cancel_at=Instant::now();cancelled.store(true,Ordering::Release);
        let result=worker.join().unwrap();assert!(codec.is_some(),"encoder never started: {result:?}");
        assert!(result.unwrap_err().contains("cancelled"));
        assert!(cancel_at.elapsed()<Duration::from_secs(2),"codec cancellation exceeded two seconds");
        assert!(!std::path::Path::new(&format!("/proc/{}",codec.unwrap())).exists(),"codec must be reaped");
        let prefix=format!("capy-hdr-{}-",std::process::id());
        assert!(!fs::read_dir(std::env::temp_dir()).unwrap().flatten().any(|e|e.file_name().to_string_lossy().starts_with(&prefix)),"private staging must be removed");
        eprintln!("active AVIF codec cancellation: {:.2} ms",cancel_at.elapsed().as_secs_f64()*1000.);
    }
}
