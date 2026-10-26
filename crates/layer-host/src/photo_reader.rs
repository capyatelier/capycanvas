use layer_color::photo::{DecodeLimits, DecodedPhoto};
use layer_core::{ProjectLimits, temp_files};
use std::{fs::File, io::{BufReader, Read, Seek, SeekFrom, Write}, sync::{Arc, atomic::{AtomicBool, Ordering}}};

pub const HELPER: &str = if cfg!(windows) { "capy-photo-reader.exe" } else { "capy-photo-reader" };

pub trait Input: Read + Seek {}
impl<T: Read + Seek> Input for T {}
pub type Decoder = fn(&mut dyn Input, DecodeLimits, &AtomicBool) -> Result<DecodedPhoto, String>;

pub fn local(input: &mut dyn Input, limits: DecodeLimits, cancelled: &AtomicBool) -> Result<DecodedPhoto, String> {
    layer_color::photo::read_photo_detailed_with_cancel(BufReader::new(input), limits, cancelled)
}

pub struct PhotoJob { pub input: File, pub output: File, pub errors: File, pub limits: DecodeLimits }

pub fn active(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) { Err("Image import cancelled".into()) } else { Ok(()) }
}

pub fn helper() -> Result<std::path::PathBuf, String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    executable.ancestors().skip(1).take(2).map(|folder| folder.join(HELPER))
        .find(|path| path.is_file()).ok_or_else(|| "Photo decoder is unavailable".into())
}

pub fn arguments(limits: DecodeLimits) -> [String; 3] {
    [limits.source_bytes.to_string(), limits.codec_bytes.to_string(), limits.dimension.to_string()]
}

pub fn limits(arguments: &[String]) -> Result<DecodeLimits, String> {
    let [source_bytes, codec_bytes, dimension] = arguments else { return Err("Invalid photo decoder arguments".into()) };
    let number = |value: &String| value.parse::<usize>().map_err(|e| e.to_string());
    Ok(DecodeLimits { source_bytes: number(source_bytes)?, codec_bytes: number(codec_bytes)?,
        dimension: u32::try_from(number(dimension)?).map_err(|e| e.to_string())? })
}

pub fn decode(mut input: impl Read, limits: DecodeLimits, cancelled: &AtomicBool, run: impl FnOnce(&PhotoJob) -> Result<bool, String>) -> Result<DecodedPhoto, String> {
    active(cancelled)?;
    let directory = temp_files::directory()?;
    let file = |name| temp_files::anonymous(directory, name).map_err(|e| e.to_string());
    let mut job = PhotoJob { input: file("photo-input")?, output: file("photo-output")?, errors: file("photo-errors")?, limits };
    let mut buffer = [0; 64 * 1024];
    let mut length = 0usize;
    loop {
        active(cancelled)?;
        let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 { break; }
        length = length.checked_add(count).filter(|length| *length <= limits.codec_bytes).ok_or("The encoded image exceeds the codec budget")?;
        job.input.write_all(&buffer[..count]).map_err(|e| e.to_string())?;
    }
    job.input.rewind().map_err(|e| e.to_string())?;
    let succeeded = run(&job)?;
    active(cancelled)?;
    if !succeeded {
        job.errors.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut detail = String::new();
        (&job.errors).take(8192).read_to_string(&mut detail).map_err(|e| e.to_string())?;
        return Err(if detail.trim().is_empty() { "Photo decoder failed".into() } else { detail });
    }
    job.output.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut output = BufReader::new(&job.output);
    let mut flags = [0];
    output.read_exact(&mut flags).map_err(|e| e.to_string())?;
    if flags[0] > 3 { return Err("Invalid photo decoder response".into()); }
    let project = ProjectLimits { asset_bytes: limits.source_bytes as u64 + layer_core::color::source::MAX_PROFILE_BYTES as u64 + layer_core::PhotoMetadata::MAX_BYTES as u64,
        dimension: limits.dimension, ..Default::default() };
    let backing = layer_core::package::transport::spool(&mut output, directory, project.asset_bytes + project.metadata_bytes, cancelled)?;
    let layer_core::package::codec::OpenOutcome::Candidate { artwork, .. } = layer_core::package::codec::open(backing, project, cancelled)? else {
        return Err("Invalid photo decoder package".into());
    };
    let mut sources = artwork.paint.iter().filter_map(|(_, _, paint)| paint.base.as_ref().map(|base| base.image.storage().clone()));
    let source = sources.next().ok_or("Photo decoder returned no image")?;
    if sources.next().is_some() || source.extent.iter().any(|size| *size > limits.dimension) || source.resident_bytes() > limits.source_bytes {
        return Err("Photo decoder image exceeds admission".into());
    }
    source.validate()?;
    Ok(DecodedPhoto { source: Arc::unwrap_or_clone(source), metadata: (*artwork.metadata).clone(), first_frame: flags[0] & 1 != 0, primary_image: flags[0] & 2 != 0 })
}

pub fn serve(input: File, output: &mut impl Write, limits: DecodeLimits) -> Result<(), String> {
    let cancel = AtomicBool::new(false);
    let photo = layer_color::photo::read_photo_detailed_with_cancel(BufReader::new(input), limits, &cancel)?;
    let depth = photo.source.interpretation.depth;
    let document = layer_color::photo_project(photo.source, photo.metadata, layer_core::DocumentNames { paint: "Photo".into(), paper: "Paper".into() }, depth)?;
    let capture = layer_core::Editor::new(document).capture(1, Default::default()).map_err(|e| e.to_string())?;
    let package = layer_core::package::codec::PreparedPackage::prepare(&capture, None, &cancel)?;
    output.write_all(&[u8::from(photo.first_frame) | u8::from(photo.primary_image) << 1]).map_err(|e| e.to_string())?;
    package.write(output, &cancel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confined_photos_match_local_decoding() {
        let _ = temp_files::set_directory(std::env::temp_dir().join("capy-layer-host-tests"));
        let source = layer_core::color::source::rgba8_source([16, 12], |_, _| [210, 45, 83, 180]);
        let png = layer_color::source_png(&source, Default::default(), usize::MAX, || false).unwrap();
        let limits = DecodeLimits { source_bytes: 1 << 20, codec_bytes: 1 << 20, dimension: 64 };
        let cancelled = AtomicBool::new(false);
        let local = local(&mut std::io::Cursor::new(&png), limits, &cancelled).unwrap();
        let confined = decode(std::io::Cursor::new(&png), limits, &cancelled, |job| {
            serve(job.input.try_clone().unwrap(), &mut job.output.try_clone().unwrap(), job.limits).map(|()| true)
        }).unwrap();
        let pixels = |photo: &DecodedPhoto| photo.source.tiles.iter().map(|(at, tile)| (*at, tile.descriptor, tile.decode().unwrap())).collect::<Vec<_>>();
        assert_eq!(pixels(&local), pixels(&confined));
        assert_eq!((local.source.extent, local.source.resolution, &local.source.interpretation, &local.metadata, local.first_frame, local.primary_image),
            (confined.source.extent, confined.source.resolution, &confined.source.interpretation, &confined.metadata, confined.first_frame, confined.primary_image));
    }
}
