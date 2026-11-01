use layer_core::authored::{Image, PortableId};
use layer_core::color::source::SourceImage;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::{Arc, Weak}};

pub const PAPER_LIBRARY_ENTRIES: usize = 64;
pub const PAPER_LIBRARY_BYTES: u64 = 256 * 1024 * 1024;
const LOADED_IMAGES: usize = 3;
const MAX_NAME_CHARS: usize = 64;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaperEntry {
    pub id: String,
    pub name: String,
    pub bytes: u64,
    #[serde(skip)]
    pub preview: Arc<[u8]>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PaperRequest {
    Import { layer: Option<u64> },
    Add { layer: u64, name: String },
    Load { id: String },
    Rename { id: String, name: String },
    Delete { id: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaperMaterial {
    Ready(layer_core::TextureSource),
    Stored { id: String, fallback: layer_core::TextureSource },
}
impl PaperMaterial {
    pub fn capture(settings: &crate::NewDocumentSettings, cached: Option<Image>) -> Self {
        if let Some(image) = cached { return Self::Ready(layer_core::TextureSource::Image(image)); }
        let fallback = settings.paper();
        match settings.paper.as_ref().filter(|id| valid_paper_id(id)) {
            Some(id) => Self::Stored { id: id.clone(), fallback },
            None => Self::Ready(fallback),
        }
    }
    pub fn fallback(&self) -> layer_core::TextureSource {
        match self { Self::Ready(texture) | Self::Stored { fallback: texture, .. } => texture.clone() }
    }
}

pub fn paper_identity(image: &SourceImage) -> Result<String, String> {
    let mut hash = Sha256::new();
    for v in image.extent { hash.update(v.to_le_bytes()); }
    for v in layer_core::paper::image_ppi(image.resolution) { hash.update(v.to_le_bytes()); }
    let mut rows = image.rows();
    let mut row = vec![0; image.row_bytes()];
    for y in 0..image.extent[1] {
        rows.read(y, &mut row)?;
        hash.update(&row);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn valid_paper_id(id: &str) -> bool {
    id.len() == 64 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn paper_image_id(id: &str) -> Option<PortableId> {
    if !valid_paper_id(id) { return None; }
    let bytes: Vec<u8> = (0..16).map(|i| u8::from_str_radix(&id[2 * i..2 * i + 2], 16).unwrap()).collect();
    Some(PortableId::from_bytes(bytes.try_into().unwrap()))
}

pub fn paper_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty() && !name.chars().any(char::is_control)).then(|| name.chars().take(MAX_NAME_CHARS).collect())
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaperLibraryError {
    Full,
    Unsupported,
    Failed(String),
}
impl PaperLibraryError {
    pub fn message(&self, localization: &crate::Localizer) -> String {
        match self {
            Self::Full => localization.text(crate::MessageId::RESOURCES_PAPER_LIBRARY_FULL).to_string(),
            Self::Unsupported => localization.text(crate::MessageId::RESOURCES_PAPER_UNSUPPORTED).to_string(),
            Self::Failed(detail) => detail.clone(),
        }
    }
}
impl From<String> for PaperLibraryError {
    fn from(detail: String) -> Self { Self::Failed(detail) }
}

pub fn library_paper(source: &SourceImage, cancelled: &dyn Fn() -> bool) -> Result<SourceImage, PaperLibraryError> {
    layer_core::paper::paper_image(source, cancelled).map_err(|error|
        if error == layer_core::paper::CMYK { PaperLibraryError::Unsupported } else { PaperLibraryError::Failed(error) })
}

pub struct StoredPaper {
    pub name: String,
    pub png: Vec<u8>,
    pub preview: Vec<u8>,
}
/// Encodes a paper that is not yet in `stored`, within the library limits.
pub fn prepare_paper(id: &str, paper: &SourceImage, name: &str, stored: &[PaperEntry]) -> Result<StoredPaper, PaperLibraryError> {
    let png = layer_core::paper::encode_png(paper)?;
    let total = stored.iter().map(|entry| entry.bytes).sum::<u64>() + png.len() as u64;
    if stored.len() >= PAPER_LIBRARY_ENTRIES || total > PAPER_LIBRARY_BYTES { return Err(PaperLibraryError::Full); }
    Ok(StoredPaper { name: paper_name(name).unwrap_or_else(|| id[..8].into()), preview: layer_core::paper::image_preview(paper)?, png })
}

pub fn stored_paper(id: &str, png: &[u8]) -> Result<SourceImage, String> {
    if !valid_paper_id(id) { return Err("Unknown paper".into()); }
    let paper = layer_core::paper::decode_png(png)?;
    if paper_identity(&paper)? != id { return Err("The stored paper changed".into()); }
    Ok(paper)
}

pub fn canonical_source(id: &str, source: SourceImage, retained: &[Image]) -> Result<SourceImage, String> {
    let parent = paper_image_id(id).ok_or("Unknown paper")?;
    if let Some(image) = retained.iter().find(|image| image.id() == parent) {
        if image.extent != source.extent || layer_core::paper::image_ppi(image.resolution) != layer_core::paper::image_ppi(source.resolution)
            || image.interpretation != source.interpretation || paper_identity(image.storage())? != id { return Err("The stored paper changed".into()); }
        return Ok(image.storage().as_ref().clone());
    }
    Ok(source)
}

pub enum PaperResult {
    Entries(Vec<PaperEntry>),
    Loaded(String, SourceImage),
    Imported(Option<u64>, String, SourceImage, Vec<PaperEntry>),
}

/// A library request as captured on the session owner. A request of `None`
/// lists the library.
pub struct PaperCapture {
    pub request: Option<u32>,
    pub action: Option<PaperRequest>,
    pub image: Option<Arc<SourceImage>>,
    pub retained: Vec<Image>,
    epoch: u64,
    document: PortableId,
}
impl<R: layer_render::CanvasRenderer> crate::UiSession<R> {
    pub fn capture_paper(&self, request: Option<u32>) -> Result<PaperCapture, String> {
        let action = request.map(|request| self.state().requests.iter().find_map(|item| match &item.kind {
            crate::HostRequestKind::Paper { request: action } if item.id == request => Some(action.clone()),
            _ => None,
        }).ok_or_else(|| PaperLibraryError::Unsupported.message(self.localization()))).transpose()?;
        let image = match &action { Some(PaperRequest::Add { layer, .. }) => self.paper_image(*layer), _ => None };
        let retained = if matches!(&action, Some(PaperRequest::Load { .. } | PaperRequest::Import { .. })) { self.paper_sources() } else { Vec::new() };
        Ok(PaperCapture { request, action, image, retained, epoch: self.state().document_file.epoch, document: self.engine().document().artwork.id })
    }
    /// An Import… that only adds a paper to the library, for New drawing.
    pub fn capture_library_import(&self) -> PaperCapture {
        PaperCapture { request: None, action: Some(PaperRequest::Import { layer: None }), image: None, retained: self.paper_sources(),
            epoch: self.state().document_file.epoch, document: self.engine().document().artwork.id }
    }
    /// Applies a finished library request and completes it. A late import
    /// into a changed drawing only refreshes the library.
    pub fn adopt_paper(&mut self, capture: &PaperCapture, result: Result<PaperResult, String>, cancelled: bool) -> Result<Option<crate::UiChange>, String> {
        if let Some(request) = capture.request && !self.state().requests.iter().any(|item| item.id == request) { return Ok(None); }
        let current = capture.epoch == self.state().document_file.epoch && capture.document == self.engine().document().artwork.id;
        let applied = match result {
            Ok(PaperResult::Entries(entries)) => self.set_paper_library(entries).map(Some),
            Ok(PaperResult::Loaded(id, image)) if !cancelled => self.paper_loaded(&id, image).map(Some),
            Ok(PaperResult::Loaded(..)) => Ok(None),
            Ok(PaperResult::Imported(layer, id, image, entries)) if current && !cancelled => self.paper_imported(layer, entries, &id, image).map(Some),
            Ok(PaperResult::Imported(_, _, _, entries)) => self.set_paper_library(entries).map(Some),
            Err(_) if cancelled => Ok(None),
            Err(error) => Err(error),
        };
        let Some(id) = capture.request else { return applied };
        let (change, error) = match applied { Ok(change) => (change, None), Err(error) => (None, Some(error)) };
        let done = self.dispatch(crate::UiAction::CompleteRequest { id, error })?;
        Ok(Some(match change {
            Some(mut change) => { change.regions |= done.regions; change.revision = done.revision; change.canvas_wake |= done.canvas_wake; change.presentation_wake |= done.presentation_wake; change }
            None => done,
        }))
    }
}

pub(crate) struct PendingPaper {
    pub id: String,
    pub layer: u64,
    pub epoch: u64,
    pub image: Option<Image>,
}

#[derive(Default)]
pub(crate) struct PaperLibrary {
    pub entries: Option<Vec<PaperEntry>>,
    images: Vec<(String, Image)>,
    sources: BTreeMap<String, Weak<SourceImage>>,
    pub pending: Option<PendingPaper>,
    pub prepared_default: Option<String>,
}
impl PaperLibrary {
    pub fn inherit(&mut self, previous: &Self) {
        self.entries.clone_from(&previous.entries);
        self.images.clone_from(&previous.images);
        self.sources.retain(|_, source| source.strong_count() != 0);
        for (id,source) in previous.sources.iter().filter(|(_,source)|source.strong_count()!=0) {self.sources.entry(id.clone()).or_insert_with(||source.clone());}
        for (id,image) in &mut self.images {
            if let Some(source)=self.sources.get(id).and_then(Weak::upgrade) {*image=Image::with_id(image.id(),source);}
        }
        self.prepared_default.clone_from(&previous.prepared_default);
    }
    pub fn image(&self, id: &str) -> Option<&Image> {
        self.images.iter().find(|(entry, _)| entry == id).map(|(_, image)| image)
    }
    pub fn set_entries(&mut self, entries: Vec<PaperEntry>) {
        self.images.retain(|(id, _)| entries.iter().any(|entry| entry.id == *id));
        self.sources.retain(|_, source| source.strong_count() != 0);
        self.entries = Some(entries);
    }
    pub fn retain_source(&mut self, id: &str, image: &Image) {
        if Some(image.id()) == paper_image_id(id) {self.sources.insert(id.to_string(),Arc::downgrade(image.storage()));}
    }
    pub fn keep(&mut self, id: &str, image: SourceImage) -> Option<Image> {
        self.keep_image(id, Image::with_id(paper_image_id(id)?, Arc::new(image)))
    }
    pub fn keep_image(&mut self, id: &str, image: Image) -> Option<Image> {
        if Some(image.id()) != paper_image_id(id) { return None; }
        self.sources.retain(|_, source| source.strong_count() != 0);
        let image = match self.sources.get(id).and_then(Weak::upgrade) {
            Some(source) => Image::with_id(image.id(), source),
            None => image,
        };
        self.sources.insert(id.to_string(), Arc::downgrade(image.storage()));
        self.images.retain(|(entry, _)| entry != id);
        self.images.push((id.to_string(), image.clone()));
        if self.images.len() > LOADED_IMAGES { self.images.remove(0); }
        Some(image)
    }
    pub fn entry_of(&self, image: &Image) -> Option<&PaperEntry> {
        self.entries.iter().flatten().find(|entry| paper_image_id(&entry.id) == Some(image.id()))
    }
    pub fn preview(&self, id: &str) -> Option<Arc<[u8]>> {
        layer_core::paper::built_in_papers().iter().find(|paper| &*paper.id == id).map(|paper| paper.preview.clone())
            .or_else(|| self.entries.iter().flatten().find(|entry| entry.id == id).map(|entry| entry.preview.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_papers_keep_one_identity_and_names_are_plain_text() {
        let id = "0123456789abcdef".repeat(4);
        assert!(valid_paper_id(&id));
        assert!(!valid_paper_id("../escape"));
        assert_eq!(paper_image_id(&id).unwrap().bytes(), [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]);
        assert_eq!(paper_name("  scan_0042 "), Some("scan_0042".into()));
        assert_eq!(paper_name(" \n"), None);
        assert_eq!(paper_name(&"x".repeat(100)).unwrap().len(), 64);
    }

    #[test]
    fn paper_source_registry_does_not_retain_pixels_after_owners_retire() {
        let mut builder = layer_core::color::source::SourceBuilder::new([1,1],
            layer_core::color::source::SourceInterpretation { channels:layer_core::color::source::SourceChannels::Gray,
                depth:layer_core::color::SampleDepth::U8,profile:Default::default(),profile_assumed:false },1 << 20).unwrap();
        builder.push_row(&[127]).unwrap();
        let mut library = PaperLibrary::default();
        let source = builder.finish().unwrap();let id = paper_identity(&source).unwrap();
        let image = library.keep(&id,source).unwrap();
        let source = Arc::downgrade(image.storage());
        library.set_entries(Vec::new());
        assert!(source.upgrade().is_some());
        let mut inherited = PaperLibrary::default(); inherited.inherit(&library);
        assert!(inherited.sources[&id].upgrade().is_some());
        drop(image);
        assert!(source.upgrade().is_none(),"weak canonical owners must not retain pixels");
        library.set_entries(Vec::new()); inherited.set_entries(Vec::new());
        assert!(library.sources.is_empty() && inherited.sources.is_empty());
        assert!(library.images.is_empty() && inherited.images.is_empty());
    }
}
