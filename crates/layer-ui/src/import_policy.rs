//! Source kind and adoption policy travel with decoded data. Picker handles and
//! URI permissions cannot grant overwrite authority to an imported photograph.
use crate::{DocumentLocation, MissingProfilePolicy, PhotoOpenPolicy};
use layer_core::{
    Project, ProjectLimits,
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
impl PhotoOpenPolicy {
    pub fn needs_interpretation(self, source: &SourceImage) -> bool {
        source.interpretation.profile_assumed && self.missing_profile == MissingProfilePolicy::Ask
    }
    pub fn photo_project(self, source: SourceImage, name: &str) -> Result<Project, String> {
        let depth = self.editing_depth(source.interpretation.depth);
        layer_color::photo_project(source, name, depth)
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
        self.project =
            layer_color::photo_project(source, &layer.name, self.project.document.color.depth)?;
        Ok(())
    }
}
/// Blocking decode, run by each host's file/worker executor. Cancellation and
/// memory limits are transport observations, never alternate import semantics.
pub fn read_import(
    input: impl Read + Seek,
    intent: ImportIntent,
    policy: PhotoOpenPolicy,
    name: &str,
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
            let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
            let name = photo.display_name(stem);
            policy.photo_project(photo.source, &name)?
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
    #[test]
    fn source_kind_owns_master_location_profile_decision_and_photo_depth() {
        let source = source();
        let policy = PhotoOpenPolicy {
            promote_to_16: true,
            missing_profile: MissingProfilePolicy::Ask,
        };
        let mut photo = ImportedDocument {
            project: policy.photo_project(source.clone(), "photo").unwrap(),
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
        assert!(
            read_import(
                std::io::Cursor::new(&native),
                ImportIntent::Place,
                policy,
                "photo.png",
                Default::default(),
                Default::default(),
                &Default::default()
            )
            .is_err()
        );
        let mut master = read_import(
            std::io::Cursor::new(&native),
            ImportIntent::Open,
            policy,
            "photo.png",
            Default::default(),
            Default::default(),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(master.source, ImportSource::Master);
        assert!(
            master
                .interpret(ColorProfile::Builtin(RgbSpace::Srgb))
                .is_err()
        );
        let mut png = Vec::new();
        layer_color::photo::write_png(&mut png, &source).unwrap();
        let photo = read_import(
            std::io::Cursor::new(&png),
            ImportIntent::Open,
            policy,
            "misleading.capy",
            Default::default(),
            Default::default(),
            &Default::default(),
        )
        .unwrap();
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
        assert!(
            read_import(
                std::io::Cursor::new(&png),
                ImportIntent::Recovery,
                policy,
                "recovery.capy",
                Default::default(),
                Default::default(),
                &Default::default()
            )
            .is_err()
        );
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

/// Hosts expose HDR only after integrating presentation, recovery and delivery.
/// Reject before replacing the live document, including recovered/imported masters.
pub fn require_sdr_host(document: &layer_core::Document, host: &str) -> Result<(), String> {
    if document.color.depth.is_float() || document.layers.iter().any(|l| l.source.as_ref().is_some_and(|s| s.interpretation.depth.is_float())) {
        return Err(format!("HDR editing is not enabled on {host}. Open this master in GTK or export its SDR rendition there."));
    }
    Ok(())
}
