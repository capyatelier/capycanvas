//! Source kind and adoption policy travel with decoded data. Picker handles and
//! URI permissions cannot grant overwrite authority to an imported photograph.
use crate::{DocumentLocation, MissingProfilePolicy, PhotoOpenPolicy};
use layer_core::{
    DocumentNames, Project, ProjectLimits,
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
        let source = if prefix.starts_with(b"CAPY") {
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
    ) -> Result<Project, String> {
        let depth = self.editing_depth(source.interpretation.depth);
        layer_color::photo_project(source, metadata, names, depth)
    }
}
pub struct ImportedDocument {
    pub project: Project,
    pub source: ImportSource,
}
impl ImportedDocument {
    pub fn interpretation_required(&self, policy: PhotoOpenPolicy) -> Option<&SourceImage> {
        if self.source != ImportSource::Photo {
            return None;
        }
        self.project
            .document
            .layers
            .iter()
            .find_map(|l| l.source.as_deref())
            .filter(|s| policy.needs_interpretation(s))
    }
    pub fn interpret(&mut self, profile: ColorProfile) -> Result<(), String> {
        if self.source != ImportSource::Photo {
            return Err("A native master keeps its saved color interpretation".into());
        }
        let layer = self
            .project
            .document
            .layers
            .first()
            .ok_or("Photo source unavailable")?;
        let source = layer.source.as_ref().ok_or("Photo source unavailable")?;
        let source = layer_color::assume_source_profile((**source).clone(), profile)?;
        let metadata = self.project.document.metadata.clone();
        self.project = layer_color::photo_project(
            source,
            metadata,
            DocumentNames { paint: layer.name.clone(),
                paper: self.project.document.layers.get(1).map_or_else(|| "".into(), |layer| layer.name.clone()) },
            self.project.document.color.depth,
        )?;
        Ok(())
    }
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
) -> Result<ImportedDocument, String> {
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err("Image import cancelled".into());
    }
    let mut reader = BufReader::new(input);
    let source = ImportSource::identify(reader.fill_buf().map_err(|e| e.to_string())?, intent)?;
    let project = match source {
        ImportSource::Master => Project::read(reader, project_limits)?,
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
    Ok(ImportedDocument { project, source })
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
        read_import(std::io::Cursor::new(bytes), intent, policy,
            photo_document_names(name, &crate::Localizer::shared(crate::UiLanguage::English)),
            Default::default(), Default::default(), &Default::default())
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
        let mut imported = ImportedDocument {
            project: policy.photo_project(source.clone(), Default::default(), names).unwrap(),
            source: ImportSource::Photo,
        };
        let document = &imported.project.document;
        assert_eq!(document.layers[0].name.as_ref(), literal);
        assert_eq!(document.layers[1].name.as_ref(), "用紙");
        assert!(!document.layers[1].visible);
        let source_before = document.layers[0].source.clone().unwrap();
        imported.interpret(ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
        let document = &imported.project.document;
        assert_eq!(document.layers[0].name.as_ref(), literal);
        assert_eq!(document.layers[1].name.as_ref(), "用紙");
        let source_after = document.layers[0].source.as_ref().unwrap();
        assert_eq!(source_after.tiles.len(), source_before.tiles.len());
        assert!(source_after.tiles.iter().all(|(position, tile)|
            source_before.tiles.get(position).is_some_and(|before| std::sync::Arc::ptr_eq(tile, before))));
        let mut bytes = Vec::new();
        imported.project.write(&mut bytes).unwrap();
        let reopened = Project::read(std::io::Cursor::new(bytes), Default::default()).unwrap();
        assert_eq!(reopened, imported.project);
    }

    #[test]
    fn source_kind_owns_master_location_profile_decision_and_photo_depth() {
        let source = source();
        let policy = PhotoOpenPolicy {
            promote_to_16: true,
            missing_profile: MissingProfilePolicy::Ask,
        };
        let mut photo = ImportedDocument {
            project: policy.photo_project(source.clone(), Default::default(), photo_document_names("photo", &crate::Localizer::shared(crate::UiLanguage::English))).unwrap(),
            source: ImportSource::Photo,
        };
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
        let corrected = photo.project.document.layers[0].source.as_ref().unwrap();
        assert!(
            corrected
                .tiles
                .iter()
                .zip(&source.tiles)
                .all(|((_, a), (_, b))| std::sync::Arc::ptr_eq(a, b))
        );
        assert_eq!(photo.project.document.color.depth, SampleDepth::U16);
        let mut native = Vec::new();
        photo.project.write(&mut native).unwrap();
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
            photo.project.document.layers[0]
                .source
                .as_ref()
                .unwrap()
                .tiles
                .iter()
                .map(|(key, blob)| (key, blob.digest))
                .collect::<Vec<_>>(),
            source
                .tiles
                .iter()
                .map(|(key, blob)| (key, blob.digest))
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
        let kept = photo.project.document.metadata.clone();
        assert!(kept.exif.as_ref().unwrap().windows(4).any(|w| w == b"Ada\0"));
        photo.interpret(ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
        assert_eq!(photo.project.document.metadata, kept, "choosing an interpretation keeps the metadata");
        let mut native = Vec::new();
        photo.project.write(&mut native).unwrap();
        assert_eq!(Project::read(native.as_slice(), Default::default()).unwrap().document.metadata, kept);
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
