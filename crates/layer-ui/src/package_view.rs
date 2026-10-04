use crate::{Localizer, MessageId};
use layer_core::{PortableId, package::{ImmutableBacking, codec::{OpenOutcome, OutputInfo}, preview::Preview}};
use serde::{Deserialize, Serialize};
use std::{io::Write, sync::{Arc, atomic::{AtomicBool, Ordering}}};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageDisposition { Preserved, Recovered, Failed }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageAction { CopyOriginal, ExportPreview, Close }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct PackageCapabilities {
    pub view: bool,
    pub copy_original: bool,
    pub edit: bool,
    pub save: bool,
    pub export: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackageOutput { pub id: PortableId, pub name: Arc<str> }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackagePresentation {
    pub disposition: PackageDisposition,
    pub extent: Option<[u32; 2]>,
    pub outputs: Vec<PackageOutput>,
    pub reason: String,
}
impl PackagePresentation {
    pub fn capabilities(&self) -> PackageCapabilities {
        PackageCapabilities { view: self.extent.is_some(), copy_original: true, edit: false, save: false, export: self.extent.is_some() }
    }
    pub fn summary(&self, localization: &Localizer) -> PackageViewSummary {
        PackageViewSummary {
            disposition: self.disposition,
            capabilities: self.capabilities(),
            extent: self.extent,
            outputs: self.outputs.clone(),
            status: localization.text(match self.disposition {
                PackageDisposition::Preserved => MessageId::DOCUMENTS_PACKAGE_PRESERVED,
                PackageDisposition::Recovered => MessageId::DOCUMENTS_PACKAGE_RECOVERED,
                PackageDisposition::Failed => MessageId::DOCUMENTS_PACKAGE_FAILED,
            }),
            reason: self.reason.clone(),
            copy_original: localization.text(MessageId::DOCUMENTS_PACKAGE_COPY_ORIGINAL),
            export_preview: localization.text(MessageId::DOCUMENTS_PACKAGE_EXPORT_PREVIEW),
            destination_error: localization.text(MessageId::DOCUMENTS_PACKAGE_CHOOSE_DIFFERENT),
            close: localization.text(MessageId::COMMON_CLOSE),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct PackageViewSummary {
    pub disposition: PackageDisposition,
    pub capabilities: PackageCapabilities,
    pub extent: Option<[u32; 2]>,
    pub outputs: Vec<PackageOutput>,
    pub status: Arc<str>,
    pub reason: String,
    pub copy_original: Arc<str>,
    pub export_preview: Arc<str>,
    pub destination_error: Arc<str>,
    pub close: Arc<str>,
}
#[derive(Clone, Debug)]
pub struct PackageView {
    source: ImmutableBacking,
    preview: Option<Preview>,
    outputs: Vec<OutputInfo>,
    reason: String,
    disposition: PackageDisposition,
}
impl PackageView {
    pub fn new(outcome: OpenOutcome) -> Result<Self, String> {
        let (source, preview, outputs, reason, disposition) = match outcome {
            OpenOutcome::Preserved { source, preview, outputs, reason } => (source, preview, outputs, reason, PackageDisposition::Preserved),
            OpenOutcome::RecoveredView { source, preview, reason } => (source, Some(preview), Vec::new(), reason, PackageDisposition::Recovered),
            OpenOutcome::Failure { source, reason } => (source, None, Vec::new(), reason, PackageDisposition::Failed),
            OpenOutcome::Candidate { .. } => return Err("Editable artwork requires document admission".into()),
        };
        Ok(Self { source, preview, outputs, reason, disposition })
    }
    pub fn source(&self) -> &ImmutableBacking { &self.source }
    pub fn preview(&self) -> Option<&Preview> { self.preview.as_ref() }
    pub fn outputs(&self) -> &[OutputInfo] { &self.outputs }
    pub fn reason(&self) -> &str { &self.reason }
    pub fn disposition(&self) -> PackageDisposition { self.disposition }
    pub fn capabilities(&self) -> PackageCapabilities {
        PackageCapabilities { view: self.preview.is_some(), copy_original: true, edit: false, save: false, export: self.preview.is_some() }
    }
    pub fn presentation(&self) -> PackagePresentation {
        PackagePresentation {
            disposition: self.disposition,
            extent: self.preview.as_ref().map(Preview::size),
            outputs: self.outputs.iter().map(|output| PackageOutput { id: output.id, name: output.name.clone() }).collect(),
            reason: self.reason.clone(),
        }
    }
    pub fn summary(&self, localization: &Localizer) -> PackageViewSummary {
        self.presentation().summary(localization)
    }
    pub fn export_preview(&self, output: &mut impl Write, cancelled: &AtomicBool) -> Result<(), String> {
        let preview = self.preview.as_ref().ok_or("No preview image is available")?;
        for bytes in preview.encoded().chunks(64 * 1024) {
            if cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
            output.write_all(bytes).map_err(|error| error.to_string())?;
        }
        if cancelled.load(Ordering::Relaxed) { return Err("Package operation cancelled".into()); }
        output.flush().map_err(|error| error.to_string())
    }
    pub fn copy_original(&self, output: &mut impl Write, cancelled: &AtomicBool) -> Result<(), String> {
        layer_core::package::codec::copy_original(&self.source, output, cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_core::package::transport::ChunkedBytes;
    fn backing(bytes: &[u8]) -> ImmutableBacking {
        ImmutableBacking::new(Arc::new(ChunkedBytes::new(vec![Arc::from(bytes)]).unwrap())).unwrap()
    }
    #[test]
    fn preserved_and_recovered_views_keep_original_bytes_and_never_enable_editing() {
        let bytes = b"preserve every original byte, including unsupported data";
        let source = backing(bytes);
        let identity = source.identity();
        let preview = Preview::from_rgba([1, 1], Arc::from([16, 32, 48, 255])).unwrap();
        let output = PortableId::random();
        for recovered in [false, true] {
            let outcome = if recovered {
                OpenOutcome::RecoveredView { source: source.clone(), preview: preview.clone(), reason: "invalid authored data".into() }
            } else {
                OpenOutcome::Preserved { source: source.clone(), preview: Some(preview.clone()), outputs: vec![OutputInfo { id: output, name: "Literal output".into() }], reason: "unsupported visible content".into() }
            };
            let view = PackageView::new(outcome).unwrap();
            assert_eq!(view.source().identity(), identity);
            assert_eq!(view.capabilities(), PackageCapabilities { view: true, copy_original: true, edit: false, save: false, export: true });
            assert_eq!(view.preview().unwrap().pixels().as_ref(), &[16, 32, 48, 255]);
            let presentation = view.presentation();
            let wire = serde_json::to_string(&presentation).unwrap();
            let restored: PackagePresentation = serde_json::from_str(&wire).unwrap();
            assert_eq!(restored.capabilities(), view.capabilities());
            assert_eq!(restored.extent, Some([1, 1]));
            assert_eq!(restored.disposition, view.disposition());
            assert_eq!(restored.reason, view.reason());
            assert_eq!(restored.outputs.len(), view.outputs().len());
            assert_eq!(view.outputs().len(), usize::from(!recovered));
            if !recovered { assert_eq!(view.outputs()[0].id, output); }
            let summary = restored.summary(&Localizer::new(crate::UiLanguage::English));
            assert_eq!(summary.export_preview.as_ref(), "Export Preview Image…");
            let mut exported = Vec::new();
            view.export_preview(&mut exported, &AtomicBool::new(false)).unwrap();
            assert_eq!(exported.as_slice(), preview.encoded().as_ref());
            let decoded = Preview::decode(exported.into()).unwrap();
            assert_eq!(decoded.size(), preview.size());
            assert_eq!(decoded.pixels(), preview.pixels());
            let mut cancelled_export = Vec::new();
            assert!(view.export_preview(&mut cancelled_export, &AtomicBool::new(true)).is_err());
            assert!(cancelled_export.is_empty());
            let mut copied = Vec::new();
            view.copy_original(&mut copied, &AtomicBool::new(false)).unwrap();
            assert_eq!(copied, bytes);
            assert!(view.copy_original(&mut Vec::new(), &AtomicBool::new(true)).is_err());
        }
        let failed = PackageView::new(OpenOutcome::Failure { source, reason: "no valid representation".into() }).unwrap();
        assert!(!failed.capabilities().view);
        assert!(failed.capabilities().copy_original);
        assert!(failed.preview().is_none());
        assert!(!failed.capabilities().export);
        assert!(failed.export_preview(&mut Vec::new(), &AtomicBool::new(false)).is_err());
        let unavailable = PackageView::new(OpenOutcome::Preserved { source: failed.source().clone(), preview: None, outputs: vec![], reason: "unsupported without preview".into() }).unwrap();
        assert!(!unavailable.capabilities().view);
        assert!(!unavailable.capabilities().export);
        assert!(!unavailable.presentation().capabilities().export);
        assert!(unavailable.export_preview(&mut Vec::new(), &AtomicBool::new(false)).is_err());
        let mut copied = Vec::new();
        failed.copy_original(&mut copied, &AtomicBool::new(false)).unwrap();
        assert_eq!(copied, bytes);
    }
    #[test]
    fn preview_export_stops_on_cancellation_and_writer_failure() {
        struct Destination<'a> { bytes: Vec<u8>, cancelled: &'a AtomicBool, fail: bool, flushed: bool }
        impl Write for Destination<'_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if self.fail { return Err(std::io::Error::other("destination failed")); }
                self.bytes.extend_from_slice(bytes);
                self.cancelled.store(true, Ordering::Relaxed);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> { self.flushed = true; Ok(()) }
        }
        let mut state = 7u32;
        let pixels = (0..256 * 128 * 4).map(|_| {
            state ^= state << 13; state ^= state >> 17; state ^= state << 5;
            state as u8
        }).collect::<Vec<_>>();
        let preview = Preview::from_rgba([256, 128], pixels.into()).unwrap();
        assert!(preview.encoded().len() > 64 * 1024);
        let view = PackageView::new(OpenOutcome::RecoveredView { source: backing(b"preserved source"), preview: preview.clone(), reason: "recovered image".into() }).unwrap();
        for fail in [false, true] {
            let cancelled = AtomicBool::new(false);
            let mut destination = Destination { bytes: Vec::new(), cancelled: &cancelled, fail, flushed: false };
            assert!(view.export_preview(&mut destination, &cancelled).is_err());
            assert!(!destination.flushed);
            if fail { assert!(destination.bytes.is_empty()); }
            else { assert_eq!(destination.bytes.as_slice(), &preview.encoded()[..64 * 1024]); }
            assert_eq!(view.preview().unwrap().encoded(), preview.encoded());
        }
    }

}
