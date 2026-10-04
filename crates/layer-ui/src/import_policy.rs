//! Source kind and adoption policy travel with decoded data. Picker handles and
//! URI permissions cannot grant overwrite authority to an imported photograph.
use crate::{DocumentLocation, MissingProfilePolicy, PhotoOpenPolicy};
use layer_core::{
    Document, DocumentNames, ProjectLimits,
    color::{ColorProfile, source::SourceImage},
};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Read, Seek},
    sync::atomic::AtomicBool,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportIntent {
    Open,
    Place,
    Recovery,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportSource {
    #[default]
    Master,
    Photo,
}
impl ImportSource {
    pub fn identify(prefix: &[u8], intent: ImportIntent) -> Result<Self, String> {
        let source = if prefix.starts_with(b"PK\x03\x04") {
            Self::Master
        } else {
            Self::Photo
        };
        match (intent, source) {
            (ImportIntent::Place, Self::Master) => Err("Choose a supported photo to place".into()),
            (ImportIntent::Recovery, Self::Photo) => {
                Err("Recovery file is not a native drawing".into())
            }
            _ => Ok(source),
        }
    }
    pub fn adoption_location(self, selected: Option<DocumentLocation>) -> Option<DocumentLocation> {
        match self {
            Self::Master => selected,
            Self::Photo => None,
        }
    }
}
pub fn photo_document_names(name: &str, localization: &crate::Localizer) -> DocumentNames {
    DocumentNames {
        paint: if name.is_empty() { localization.text(crate::MessageId::DOCUMENTS_PHOTO_NAME) } else { name.into() },
        paper: localization.text(crate::MessageId::DOCUMENTS_PAPER),
    }
}

impl PhotoOpenPolicy {
    pub fn needs_interpretation(self, source: &SourceImage) -> bool {
        source.interpretation.profile_assumed && self.missing_profile == MissingProfilePolicy::Ask
    }
    pub fn photo_project(
        self,
        source: SourceImage,
        metadata: layer_core::PhotoMetadata,
        names: DocumentNames,
    ) -> Result<Document, String> {
        let depth = self.editing_depth(source.interpretation.depth);
        layer_color::photo_project(source, metadata, names, depth)
    }
}
#[derive(Clone)]
struct NativeOrigin {
    source: layer_core::package::ImmutableBacking,
    preview: Option<layer_core::package::preview::Preview>,
    outputs: Vec<layer_core::package::codec::OutputInfo>,
}
#[derive(Clone)]
pub struct ImportedDocument {
    pub project: Document,
    pub source: ImportSource,
    native: Option<NativeOrigin>,
}
pub enum ImportOutcome {
    Editable(ImportedDocument),
    Package(layer_core::package::codec::OpenOutcome),
}
impl ImportedDocument {
    pub fn new(project: Document, source: ImportSource) -> Self { Self { project, source, native: None } }
    pub fn preserve_unsupported(&self, reason: impl Into<String>) -> Option<layer_core::package::codec::OpenOutcome> {
        let native = self.native.as_ref()?;
        Some(layer_core::package::codec::OpenOutcome::Preserved {
            source: native.source.clone(), preview: native.preview.clone(), outputs: native.outputs.clone(), reason: reason.into(),
        })
    }
    pub fn interpretation_required(&self, policy: PhotoOpenPolicy) -> Option<&SourceImage> {
        if self.source != ImportSource::Photo { return None; }
        self.project.artwork.paint.iter().find_map(|(_, _, p)| p.original.as_deref())
            .filter(|s| policy.needs_interpretation(s))
    }
    pub fn interpret(&mut self, profile: ColorProfile) -> Result<(), String> {
        if self.source != ImportSource::Photo { return Err("A native master keeps its saved color interpretation".into()); }
        let scene = self.project.scene();
        let handle = scene.order().first().copied().ok_or("Photo source unavailable")?;
        let occurrence = scene.occurrence(handle).ok_or("Photo source unavailable")?;
        let source = scene.paint_source(handle).and_then(|p| p.original.as_ref()).ok_or("Photo source unavailable")?;
        let source = layer_color::assume_source_profile((**source).clone(), profile)?;
        let names = DocumentNames {
            paint: occurrence.name.clone(),
            paper: scene.order().get(1).and_then(|h| scene.occurrence(*h)).map_or_else(|| "".into(), |o| o.name.clone()),
        };
        self.project = layer_color::photo_project(source, (*self.project.artwork.metadata).clone(), names, self.project.composition().color.depth)?;
        Ok(())
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn package_backing(input: &mut impl Read, limit: u64, cancelled: &AtomicBool) -> Result<layer_core::package::ImmutableBacking, String> {
    layer_core::package::transport::spool(input, &std::env::temp_dir(), limit, cancelled)
}
#[cfg(target_arch = "wasm32")]
fn package_backing(input: &mut impl Read, limit: u64, cancelled: &AtomicBool) -> Result<layer_core::package::ImmutableBacking, String> {
    let mut chunks = Vec::new();
    let mut length = 0u64;
    loop {
        let mut chunk = vec![0; layer_core::package::MAX_RANGE_BYTES];
        let mut used = 0;
        while used < chunk.len() {
            if cancelled.load(std::sync::atomic::Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
            let count = input.read(&mut chunk[used..]).map_err(|e| e.to_string())?;
            if count == 0 { break; }
            used += count;
            length = length.checked_add(count as u64).filter(|n| *n <= limit).ok_or("Package stream exceeds admission limit")?;
        }
        if used == 0 { break; }
        chunk.truncate(used);
        let complete = used == layer_core::package::MAX_RANGE_BYTES;
        chunks.push(std::sync::Arc::from(chunk));
        if !complete { break; }
    }
    let bytes = layer_core::package::transport::ChunkedBytes::new(chunks)?;
    layer_core::package::ImmutableBacking::new(std::sync::Arc::new(bytes)).map_err(str::to_owned)
}
/// Blocking decode, run by each host's file/worker executor. Cancellation and
/// memory limits are transport observations, never alternate import semantics.
pub fn read_import(
    input: impl Read + Seek,
    intent: ImportIntent,
    policy: PhotoOpenPolicy,
    mut names: DocumentNames,
    project_limits: ProjectLimits,
    photo_limits: layer_color::photo::DecodeLimits,
    cancelled: &AtomicBool,
) -> Result<ImportOutcome, String> {
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err("Image import cancelled".into());
    }
    let mut reader = BufReader::new(input);
    let source = ImportSource::identify(reader.fill_buf().map_err(|e| e.to_string())?, intent)?;
    let mut native = None;
    let project = match source {
        ImportSource::Master => {
            let limit = project_limits.metadata_bytes.saturating_add(project_limits.asset_bytes).saturating_add(project_limits.raster_bytes);
            let backing = package_backing(&mut reader, limit, cancelled)?;
            let outcome = layer_core::package::codec::open(backing, project_limits, cancelled)?;
            match outcome {
                layer_core::package::codec::OpenOutcome::Candidate { artwork, source, preview } => {
                    match Document::from_artwork(artwork).map_err(|e| e.to_string()).and_then(|document| { document.validate_integrity()?; Ok(document) }) {
                        Ok(document) => {
                            let outputs = document.artwork.outputs.iter().map(|(_, id, output)| layer_core::package::codec::OutputInfo { id, name: output.name.clone() }).collect();
                            native = Some(NativeOrigin { source, preview, outputs });
                            document
                        },
                        Err(reason) => {
                            if cancelled.load(std::sync::atomic::Ordering::Acquire) { return Err("Image import cancelled".into()); }
                            return Ok(ImportOutcome::Package(match preview {
                                Some(preview) => layer_core::package::codec::OpenOutcome::RecoveredView { source, preview, reason },
                                None => layer_core::package::codec::OpenOutcome::Failure { source, reason },
                            }));
                        },
                    }
                }
                outcome => return Ok(ImportOutcome::Package(outcome)),
            }
        },
        ImportSource::Photo => {
            let photo = layer_color::photo::read_photo_detailed_with_cancel(
                reader,
                photo_limits,
                cancelled,
            )?;
            let stem = names.paint.rsplit_once('.').map_or(names.paint.as_ref(), |(stem, _)| stem);
            names.paint = photo.display_name(stem).into();
            policy.photo_project(photo.source, photo.metadata, names)?
        }
    };
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err("Image import cancelled".into());
    }
    let imported = ImportedDocument { project, source, native };
    if let Err(reason) = imported.project.admit(project_limits).and_then(|_| layer_color::validate_document_color(&imported.project)) {
        if cancelled.load(std::sync::atomic::Ordering::Acquire) { return Err("Image import cancelled".into()); }
        return match imported.preserve_unsupported(reason.clone()) {
            Some(outcome) => Ok(ImportOutcome::Package(outcome)),
            None => Err(reason),
        };
    }
    Ok(ImportOutcome::Editable(imported))
}

/// Bounded, all-or-nothing retained-image preparation. Worker adapters may
/// decode locally or supply decoded sources from a browser worker.
pub struct ImageImportBatch {
    limits: layer_color::photo::DecodeLimits,
    policy: PhotoOpenPolicy,
    working: layer_core::color::RgbSpace,
    sources: Vec<(String, SourceImage)>,
    pending: Option<(String, SourceImage)>,
    finished: bool,
}
impl ImageImportBatch {
    pub fn new(
        policy: PhotoOpenPolicy,
        working: layer_core::color::RgbSpace,
        limits: layer_color::photo::DecodeLimits,
    ) -> Self {
        Self {
            limits,
            policy,
            working,
            sources: Vec::new(),
            pending: None,
            finished: false,
        }
    }
    pub fn limits(&self) -> layer_color::photo::DecodeLimits {
        self.limits
    }
    pub fn invalidate(&mut self) {
        self.finished = true;
        self.pending = None;
        self.sources.clear();
    }
    pub fn pending_source(&self) -> Option<&SourceImage> {
        self.pending.as_ref().map(|(_, source)| source)
    }
    fn check(&self, cancelled: bool) -> Result<(), String> {
        if cancelled {
            Err("Image import cancelled".into())
        } else if self.finished {
            Err("Image import is no longer prepared".into())
        } else if self.pending.is_some() {
            Err("Choose the previous image's interpretation first".into())
        } else {
            Ok(())
        }
    }
    fn push(&mut self, name: String, source: SourceImage) -> Result<(), String> {
        source.validate()?;
        layer_color::WorkingDecoder::new(&source.interpretation, self.working, Default::default())?;
        self.limits.source_bytes = self
            .limits
            .source_bytes
            .checked_sub(source.resident_bytes())
            .ok_or("The image batch exceeds the source memory budget. No images were imported.")?;
        self.sources.push((name, source));
        Ok(())
    }
    pub fn append(
        &mut self,
        name: String,
        source: SourceImage,
        cancelled: bool,
    ) -> Result<(), String> {
        let result = self.check(cancelled).and_then(|_| {
            if self.policy.needs_interpretation(&source) {
                self.pending = Some((name, source));
                Ok(())
            } else {
                self.push(name, source)
            }
        });
        if result.is_err() {
            self.invalidate();
        }
        result
    }
    pub fn read(
        &mut self,
        input: impl Read + Seek,
        name: &str,
        cancelled: &AtomicBool,
    ) -> Result<(), String> {
        let result = (|| {
            self.check(cancelled.load(std::sync::atomic::Ordering::Acquire))?;
            let photo = layer_color::photo::read_photo_detailed_with_cancel(
                BufReader::new(input),
                self.limits,
                cancelled,
            )?;
            self.append(
                photo.display_name(name),
                photo.source,
                cancelled.load(std::sync::atomic::Ordering::Acquire),
            )
        })();
        if result.is_err() {
            self.invalidate();
        }
        result
    }
    pub fn interpret(&mut self, profile: ColorProfile, cancelled: bool) -> Result<(), String> {
        if cancelled {
            self.invalidate();
            return Err("Image import cancelled".into());
        }
        // A rejected form choice must remain editable without decoding the
        // batch again. Validate before consuming the pending original.
        let (_, source) = self.pending.as_ref().ok_or("No pending image interpretation")?;
        let source = layer_color::assume_source_profile(source.clone(), profile)?;
        let result = (|| {
            let (name, _) = self
                .pending
                .take()
                .ok_or("No pending image interpretation")?;
            self.check(false)?;
            self.push(name, source)
        })();
        if result.is_err() {
            self.invalidate();
        }
        result
    }
    pub fn take_sources(&mut self, cancelled: bool) -> Result<Vec<(String, SourceImage)>, String> {
        let result = self.check(cancelled).and_then(|_| {
            if self.sources.is_empty() {
                return Err("No images to import".into());
            }
            Ok(std::mem::take(&mut self.sources))
        });
        self.invalidate();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::{package_bytes, package_roundtrip};
    fn photo_source(document: &Document) -> &SourceImage {
        document.artwork.paint.iter().find_map(|(_, _, p)| p.original.as_deref()).expect("photo source")
    }
    fn native_bytes(document: &Document) -> Vec<u8> {
        let capture = layer_core::Editor::new(document.clone()).capture(0, document.output().context.clone()).unwrap();
        package_bytes(&capture)
    }
    use layer_core::color::{SampleDepth, RgbSpace, source::*};
    fn source() -> SourceImage {
        let mut builder = SourceBuilder::new(
            [3, 1],
            SourceInterpretation {
                channels: SourceChannels::Rgba,
                depth: SampleDepth::U16,
                profile: ColorProfile::Builtin(RgbSpace::ProPhoto),
                profile_assumed: true,
            },
            1024 * 1024,
        )
        .unwrap();
        builder
            .push_row(&[
                1, 0, 2, 0, 3, 0, 0, 0, 4, 0, 5, 0, 6, 0, 255, 255, 7, 0, 8, 0, 9, 0, 1, 0,
            ])
            .unwrap();
        builder.finish().unwrap()
    }
    fn open(bytes: &[u8], intent: ImportIntent, policy: PhotoOpenPolicy, name: &str) -> Result<ImportedDocument, String> {
        match read_import(std::io::Cursor::new(bytes), intent, policy,
            photo_document_names(name, &crate::Localizer::shared(crate::UiLanguage::English)),
            Default::default(), Default::default(), &Default::default())? {
            ImportOutcome::Editable(document) => Ok(document),
            ImportOutcome::Package(outcome) => Err(format!("Expected editable artwork: {outcome:?}")),
        }
    }
    fn native_with_preview(document: &Document, preview: layer_core::package::preview::Preview) -> Vec<u8> {
        let capture = layer_core::Editor::new(document.clone()).capture(0, document.output().context.clone()).unwrap();
        let captured_preview = layer_core::package::codec::CapturedPreview { checkpoint: capture.checkpoint, context: document.output().context.clone(), preview };
        let cancelled = AtomicBool::new(false);
        let prepared = layer_core::package::codec::PreparedPackage::prepare(&capture, Some(captured_preview), &cancelled).unwrap();
        let mut bytes = Vec::new(); prepared.write(&mut bytes, &cancelled).unwrap(); bytes
    }
    #[test]
    fn aggregate_admission_preserves_valid_native_artwork_and_invalid_payload_recovers_preview() {
        use layer_core::package::{archive::{Directory, StoredMember, write_archive, MIMETYPE}, codec::OpenOutcome, preview::Preview};
        let document = Document::new(layer_core::PortableId::random(), 8, 8, DocumentNames { paint: "Ink".into(), paper: "Paper".into() });
        let preview = Preview::from_rgba([1, 1], vec![11, 23, 37, 255].into()).unwrap();
        let bytes = native_with_preview(&document, preview.clone());
        let limits = ProjectLimits { layers: 1, ..Default::default() };
        document.validate_integrity().unwrap();
        assert!(document.admit(limits).is_err());
        let outcome = read_import(std::io::Cursor::new(&bytes), ImportIntent::Open, Default::default(), DocumentNames { paint: "".into(), paper: "".into() }, limits, Default::default(), &AtomicBool::new(false)).unwrap();
        let ImportOutcome::Package(OpenOutcome::Preserved { source, preview: representation, outputs, .. }) = outcome else { panic!("valid package over aggregate admission must remain preserved") };
        assert_eq!(representation.unwrap().encoded(), preview.encoded());
        assert_eq!(outputs[0].id, document.artwork.outputs.id(document.artwork.default_output).unwrap());
        let mut copied = Vec::new(); layer_core::package::codec::copy_original(&source, &mut copied, &AtomicBool::new(false)).unwrap();
        assert_eq!(copied, bytes);
        let mut original = std::io::Cursor::new(&bytes);
        let directory = Directory::read(&mut original, 262_144, ProjectLimits::default().metadata_bytes).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_slice(&directory.read_member(&mut original, directory.member("manifest.json").unwrap(), ProjectLimits::default().metadata_bytes as usize).unwrap()).unwrap();
        let occurrence = manifest["objects"].as_array_mut().unwrap().iter_mut().find(|record| record["type"] == "capy.occurrence/1").unwrap();
        occurrence["data"]["opacity"] = "invalid required value".into();
        let manifest = serde_json::to_vec(&manifest).unwrap();
        let checksum = |bytes: &[u8]| { let mut crc = flate2::Crc::new(); crc.update(bytes); crc.sum() };
        let mut mime_reader = std::io::Cursor::new(MIMETYPE);
        let mut manifest_reader = std::io::Cursor::new(&manifest);
        let mut preview_reader = std::io::Cursor::new(preview.encoded().as_ref());
        let mut members = [
            StoredMember { name: "mimetype", length: MIMETYPE.len() as u64, crc32: checksum(MIMETYPE), input: &mut mime_reader },
            StoredMember { name: "manifest.json", length: manifest.len() as u64, crc32: checksum(&manifest), input: &mut manifest_reader },
            StoredMember { name: "preview.png", length: preview.encoded().len() as u64, crc32: checksum(preview.encoded()), input: &mut preview_reader },
        ];
        let mut corrupted = Vec::new(); write_archive(&mut corrupted, &mut members, ProjectLimits::default().metadata_bytes as usize).unwrap();
        let outcome = read_import(std::io::Cursor::new(&corrupted), ImportIntent::Open, Default::default(), DocumentNames { paint: "".into(), paper: "".into() }, Default::default(), Default::default(), &AtomicBool::new(false)).unwrap();
        let ImportOutcome::Package(OpenOutcome::RecoveredView { source, preview: representation, .. }) = outcome else { panic!("invalid required payload must recover only its verified representation") };
        assert_eq!(representation.encoded(), preview.encoded());
        let mut copied = Vec::new(); layer_core::package::codec::copy_original(&source, &mut copied, &AtomicBool::new(false)).unwrap();
        assert_eq!(copied, corrupted);
        assert!(read_import(std::io::Cursor::new(&corrupted), ImportIntent::Open, Default::default(), DocumentNames { paint: "".into(), paper: "".into() }, Default::default(), Default::default(), &AtomicBool::new(true)).is_err());
    }
    #[test]
    fn native_candidate_retains_original_bytes_until_execution_admission() {
        let policy = PhotoOpenPolicy::default();
        let document = policy.photo_project(source(), Default::default(), photo_document_names("Native photo", &crate::Localizer::new(crate::UiLanguage::English))).unwrap();
        let preview = layer_core::package::preview::Preview::from_rgba([1, 1], vec![7, 19, 31, 255].into()).unwrap();
        let bytes = native_with_preview(&document, preview.clone());
        let cancelled = AtomicBool::new(false);
        let imported = open(&bytes, ImportIntent::Open, policy, "native.capy").unwrap();
        let retained = imported.clone();
        drop(imported);
        let outcome = retained.preserve_unsupported("Renderer execution is unavailable").unwrap();
        let layer_core::package::codec::OpenOutcome::Preserved { source, preview: representation, outputs, reason } = outcome else { panic!("preserved native package") };
        assert_eq!(reason, "Renderer execution is unavailable");
        assert_eq!(outputs.len(), document.artwork.outputs.len());
        assert!(outputs.iter().any(|output| output.id == document.artwork.outputs.id(document.artwork.default_output).unwrap() && output.name == document.output().name));
        assert_eq!(representation.unwrap().encoded(), preview.encoded());
        let mut copied = Vec::new(); layer_core::package::codec::copy_original(&source, &mut copied, &cancelled).unwrap();
        assert_eq!(copied, bytes);
        assert!(ImportedDocument::new(document, ImportSource::Master).preserve_unsupported("new document").is_none());
    }
    #[test]
    fn native_color_execution_failure_preserves_package_instead_of_adopting_it() {
        let policy = PhotoOpenPolicy::default();
        let mut builder = SourceBuilder::new([1, 1], SourceInterpretation { channels: SourceChannels::Rgba, depth: SampleDepth::F32, profile: ColorProfile::Builtin(RgbSpace::Srgb), profile_assumed: false }, 1024 * 1024).unwrap();
        let samples = [2f32, 0.25, 0.5, 1.].into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>();
        builder.push_row(&samples).unwrap();
        let mut document = layer_color::photo_project(builder.finish().unwrap(), Default::default(), photo_document_names("Unsupported HDR placement", &crate::Localizer::new(crate::UiLanguage::English)), SampleDepth::F32).unwrap();
        document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth = SampleDepth::U16;
        document.validate(Default::default()).unwrap();
        assert!(layer_color::validate_document_color(&document).is_err());
        let bytes = native_bytes(&document);
        let outcome = read_import(std::io::Cursor::new(&bytes), ImportIntent::Open, policy, DocumentNames { paint: "".into(), paper: "".into() }, Default::default(), Default::default(), &AtomicBool::new(false)).unwrap();
        let ImportOutcome::Package(layer_core::package::codec::OpenOutcome::Preserved { source, preview, outputs, reason }) = outcome else { panic!("unsupported execution must preserve native artwork") };
        assert!(preview.is_none());
        assert!(!reason.is_empty());
        assert_eq!(outputs.len(), document.artwork.outputs.len());
        let mut copied = Vec::new(); layer_core::package::codec::copy_original(&source, &mut copied, &AtomicBool::new(false)).unwrap();
        assert_eq!(copied, bytes);
        assert!(read_import(std::io::Cursor::new(&bytes), ImportIntent::Open, policy, DocumentNames { paint: "".into(), paper: "".into() }, Default::default(), Default::default(), &AtomicBool::new(true)).is_err());
    }
    #[test]
    fn photo_creation_and_reinterpretation_keep_supplied_names_and_source_pixels() {
        let localization = crate::Localizer::shared(crate::UiLanguage::Japanese);
        let literal = "  Photo { $name }「写真」🖼️\u{2068}literal\u{2069}  ";
        let source = source();
        let names = photo_document_names(literal, &localization);
        assert_eq!(names.paint.as_ref(), literal);
        assert_eq!(names.paper.as_ref(), "用紙");
        let fallback = photo_document_names("", &localization);
        assert_eq!(fallback.paint, localization.text(crate::MessageId::DOCUMENTS_PHOTO_NAME));
        let policy = PhotoOpenPolicy { promote_to_16: true, missing_profile: MissingProfilePolicy::Ask };
        let mut imported = ImportedDocument::new(policy.photo_project(source.clone(), Default::default(), names).unwrap(), ImportSource::Photo);
        let document = &imported.project;
        assert_eq!(document.scene().occurrence(document.scene().order()[0]).unwrap().name.as_ref(), literal);
        assert_eq!(document.scene().occurrence(document.scene().order()[1]).unwrap().name.as_ref(), "用紙");
        assert!(!document.scene().occurrence(document.scene().order()[1]).unwrap().visible);
        let source_before = photo_source(document).clone();
        imported.interpret(ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
        let document = &imported.project;
        assert_eq!(document.scene().occurrence(document.scene().order()[0]).unwrap().name.as_ref(), literal);
        assert_eq!(document.scene().occurrence(document.scene().order()[1]).unwrap().name.as_ref(), "用紙");
        let source_after = photo_source(document);
        assert_eq!(source_after.tiles.len(), source_before.tiles.len());
        assert!(source_after.tiles.iter().all(|(position, tile)|
            source_before.tiles.get(position).is_some_and(|before| std::sync::Arc::ptr_eq(tile, before))));
        let reopened = package_roundtrip(&imported.project);
        assert_eq!(reopened.artwork.metadata, imported.project.artwork.metadata);
        assert_eq!(reopened.composition(), imported.project.composition());
        assert_eq!(photo_source(&reopened), photo_source(&imported.project));
        assert_eq!(reopened.scene().order().iter().map(|h| reopened.scene().occurrence(*h).unwrap().name.as_ref()).collect::<Vec<_>>(), imported.project.scene().order().iter().map(|h| imported.project.scene().occurrence(*h).unwrap().name.as_ref()).collect::<Vec<_>>());
    }

    #[test]
    fn source_kind_owns_master_location_profile_decision_and_photo_depth() {
        let source = source();
        let policy = PhotoOpenPolicy {
            promote_to_16: true,
            missing_profile: MissingProfilePolicy::Ask,
        };
        let mut photo = ImportedDocument::new(policy.photo_project(source.clone(), Default::default(), photo_document_names("photo", &crate::Localizer::shared(crate::UiLanguage::English))).unwrap(), ImportSource::Photo);
        assert!(photo.interpretation_required(policy).is_some());
        assert!(
            photo
                .source
                .adoption_location(Some(DocumentLocation {
                    uri: "original.png".into(),
                    name: "original.png".into()
                }))
                .is_none()
        );
        photo
            .interpret(ColorProfile::Builtin(RgbSpace::DisplayP3))
            .unwrap();
        assert!(photo.interpretation_required(policy).is_none());
        let corrected = photo_source(&photo.project);
        assert!(
            corrected
                .tiles
                .iter()
                .zip(&source.tiles)
                .all(|((_, a), (_, b))| std::sync::Arc::ptr_eq(a, b))
        );
        assert_eq!(photo.project.composition().color.depth, SampleDepth::U16);
        let native = native_bytes(&photo.project);
        assert!(open(&native, ImportIntent::Place, policy, "photo.png").is_err());
        let mut master = open(&native, ImportIntent::Open, policy, "photo.png").unwrap();
        assert_eq!(master.source, ImportSource::Master);
        assert!(
            master
                .interpret(ColorProfile::Builtin(RgbSpace::Srgb))
                .is_err()
        );
        let mut png = Vec::new();
        layer_color::photo::write_png(&mut png, &source).unwrap();
        let photo = open(&png, ImportIntent::Open, policy, "misleading.capy").unwrap();
        assert_eq!(photo.source, ImportSource::Photo);
        assert_eq!(
            photo_source(&photo.project)
                .tiles
                .iter()
                .map(|(key, blob)| (key, blob.content_digest().unwrap()))
                .collect::<Vec<_>>(),
            source
                .tiles
                .iter()
                .map(|(key, blob)| (key, blob.content_digest().unwrap()))
                .collect::<Vec<_>>()
        );
        assert!(open(&png, ImportIntent::Recovery, policy, "recovery.capy").is_err());
    }
    #[test]
    fn an_opened_photo_keeps_its_metadata_through_interpretation_and_saving() {
        let artist = [b"II\x2a\0\x08\0\0\0\x01\0\x3b\x01\x02\0\x04\0\0\0Ada\0".as_slice(), &[0; 4]].concat();
        let delivery = layer_color::photo::DeliveryMetadata {
            photo: layer_core::PhotoMetadata { exif: Some(artist.into()), ..Default::default() },
            ..Default::default()
        };
        let source = source();
        let mut png = Vec::new();
        let mut rows = source.rows();
        layer_color::photo::write_png_rows(&mut png, source.extent, &source.interpretation, &delivery, |y, row| rows.read(y, row)).unwrap();
        let policy = PhotoOpenPolicy { promote_to_16: false, missing_profile: MissingProfilePolicy::Ask };
        let open_photo = |intent| open(&png, intent, policy, "photo.png");
        let mut photo = open_photo(ImportIntent::Open).unwrap();
        let kept = photo.project.artwork.metadata.clone();
        assert!(kept.exif.as_ref().unwrap().windows(4).any(|w| w == b"Ada\0"));
        photo.interpret(ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
        assert_eq!(photo.project.artwork.metadata, kept, "choosing an interpretation keeps the metadata");
        let native = native_bytes(&photo.project);
        assert_eq!(open(&native, ImportIntent::Open, policy, "drawing.capy").unwrap().project.artwork.metadata, kept);
        let mut batch = ImageImportBatch::new(Default::default(), RgbSpace::Srgb, Default::default());
        batch.read(std::io::Cursor::new(&png), "placed", &Default::default()).unwrap();
        assert_eq!(batch.take_sources(false).unwrap().len(), 1, "imports carry only their pixels");
    }
    #[test]
    fn batch_budget_interpretation_and_cancellation_never_publish_a_partial_batch() {
        let source = source();
        let mut batch = ImageImportBatch::new(
            Default::default(),
            RgbSpace::Srgb,
            layer_color::photo::DecodeLimits {
                source_bytes: source.resident_bytes() * 2 - 1,
                ..Default::default()
            },
        );
        batch.append("first".into(), source.clone(), false).unwrap();
        assert!(
            batch
                .append("second".into(), source.clone(), false)
                .is_err()
        );
        assert!(batch.take_sources(false).is_err());
        let policy = PhotoOpenPolicy {
            promote_to_16: false,
            missing_profile: MissingProfilePolicy::Ask,
        };
        let mut batch = ImageImportBatch::new(policy, RgbSpace::DisplayP3, Default::default());
        batch.append("first".into(), source.clone(), false).unwrap();
        assert!(batch.pending_source().is_some());
        assert!(batch.interpret(ColorProfile::Icc(Vec::new().into()), false).is_err());
        assert_eq!(batch.pending_source(), Some(&source));
        batch
            .interpret(ColorProfile::Builtin(RgbSpace::Srgb), false)
            .unwrap();
        batch.append("second".into(), source, false).unwrap();
        batch
            .interpret(ColorProfile::Builtin(RgbSpace::DisplayP3), false)
            .unwrap();
        assert!(batch.take_sources(true).is_err());
        assert!(batch.take_sources(false).is_err());
    }
}
