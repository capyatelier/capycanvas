//! Selection coverage, validation, and saved selection targets.
use super::*;
use crate::authored::Resource;
use std::sync::OnceLock;

use crate::package::SELECTION_CHUNK_BYTES;

/// Painting changes coverage independently of artwork colors and compositing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionPaintBehavior {
    #[default]
    ColorTransparency,
    BlackWhite,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SelectionMaskProperties {
    pub color: color::RgbColor,
    pub opacity: f32,
}
impl Default for SelectionMaskProperties {
    fn default() -> Self {
        Self {
            color: color::RgbColor::new(color::RgbSpace::Srgb, [1., 0., 0., 1.]).unwrap(),
            opacity: 0.5,
        }
    }
}
impl SelectionMaskProperties {
    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.color.validate_working_spaces().is_err()
            || !self.opacity.is_finite()
            || !(0. ..=1.).contains(&self.opacity)
        {
            return Err(DocumentError::InvalidLayerOperation(
                "Invalid selection mask properties",
            ));
        }
        Ok(())
    }
}

/// A paintable selection destination. Artwork and visibility masks have their
/// own targets; a saved mask is never clipped by the current selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SelectionTarget {
    Current,
    Saved(crate::authored::OccurrenceHandle),
}

impl Document {
    pub fn saved_selection(&self,id:crate::authored::OccurrenceHandle)->Result<Selection,DocumentError> {
        let scene=self.scene();let o=scene.occurrence(id).ok_or(DocumentError::MissingOccurrence(id))?;
        let crate::authored::OccurrenceContent::Selection(handle)=o.content else{return Err(DocumentError::InvalidLayerOperation("Choose a Selection Layer"));};
        let selection=&self.artwork.selections.get(handle).ok_or(DocumentError::MissingTarget(crate::authored::SourceTarget::Selection(handle)))?.selection;
        selection.mapped(&scene.target_geometry(crate::authored::SourceTarget::Selection(handle)).placement)
    }
    pub fn selection_edit(&self,target:SelectionTarget,coverage:Selection)->Result<Edit,DocumentError> {
        use crate::authored::*;coverage.validate()?;
        match target {
            SelectionTarget::Current=>{let mut working=self.working.clone();working.selection=Some(coverage);Ok(Edit::Working(working))},
            SelectionTarget::Saved(id)=>{
                self.saved_selection(id)?;if self.is_locked(id){return Err(DocumentError::ProtectedOccurrence(id));}
                let OccurrenceContent::Selection(handle)=self.scene().occurrence(id).unwrap().content else{unreachable!()};
                let inverse=self.affine_edit_transform(SourceTarget::Selection(handle)).and_then(Affine::inverse).ok_or(DocumentError::InvalidLayerOperation("Invalid selection placement"))?;
                let mut value=self.artwork.selections.get(handle).unwrap().clone();value.selection=coverage.transformed(inverse)?;
                Ok(Edit::SavedSelection(RecordChange::replace(&self.artwork.selections,handle,Some(value))?))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    #[default]
    New,
    Add,
    Subtract,
    Intersect,
}

type SelectionChunks = Arc<[Resource<[u8]>]>;

/// Immutable coverage survives subsequent edits, undo and renderer recreation.
/// Nibble masks pack eight 0..4 coverage samples per word; refined masks pack
/// four 0..255 coverage bytes. Rows pad their final word with zero coverage.
/// Pixels are produced by the GPU; this type validates and retains their data.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SelectionPixels {
    /// Feathered selections retain full 8-bit coverage, four pixels per word.
    byte_coverage: bool,
    extent: [u32; 2],
    bounds: [u32; 4],
    words: Arc<[u32]>,
    #[serde(skip)]
    package_chunks: Arc<OnceLock<SelectionChunks>>,
    #[serde(skip)]
    package_chunk_ids: Arc<OnceLock<Arc<[crate::authored::PortableId]>>>,
    #[serde(skip)]
    words_validated: Arc<OnceLock<()>>,
    #[serde(skip)]
    package_validated: Arc<OnceLock<()>>,
}
impl PartialEq for SelectionPixels {
    fn eq(&self, other: &Self) -> bool {
        self.byte_coverage == other.byte_coverage && self.extent == other.extent
            && self.bounds == other.bounds && self.words == other.words
    }
}
impl Eq for SelectionPixels {}
impl SelectionPixels {
    pub fn new(
        extent: [u32; 2],
        bounds: [u32; 4],
        words: impl Into<Arc<[u32]>>,
    ) -> Result<Self, DocumentError> {
        Self::with_coverage(extent, bounds, words.into(), false)
    }
    pub fn bytes(
        extent: [u32; 2],
        bounds: [u32; 4],
        words: impl Into<Arc<[u32]>>,
    ) -> Result<Self, DocumentError> {
        Self::with_coverage(extent, bounds, words.into(), true)
    }
    fn with_coverage(
        extent: [u32; 2],
        bounds: [u32; 4],
        words: Arc<[u32]>,
        byte_coverage: bool,
    ) -> Result<Self, DocumentError> {
        let value = Self {
            extent,
            bounds,
            words,
            byte_coverage,
            package_chunks: Arc::default(),
            package_chunk_ids: Arc::default(),
            words_validated: Arc::default(),
            package_validated: Arc::default(),
        };
        value.validate()?;
        Ok(value)
    }
    pub(crate) fn validate(&self) -> Result<(), DocumentError> {
        let [w, h] = self.extent;
        let [x0, y0, x1, y1] = self.bounds;
        let words = &self.words;
        if w == 0 || h == 0 || x0 > x1 || y0 > y1 || x1 > w || y1 > h
            || u64::from(w.div_ceil(self.pixels_per_word())) * u64::from(h) != words.len() as u64
            // Reject values >4 with eight parallel nibble comparisons.
            || (!self.byte_coverage && self.words_validated.get().is_none() && words.iter().any(|v| v & 0x88888888 != 0 || ((v >> 2) & (v | (v >> 1)) & 0x11111111) != 0))
        {
            return Err(DocumentError::InvalidLayerOperation(
                "Invalid selection coverage",
            ));
        }
        self.words_validated.get_or_init(|| ());
        Ok(())
    }
    pub(crate) fn validate_package(&self) -> Result<(), String> {
        if self.package_validated.get().is_some() { return Ok(()); }
        self.validate().map_err(|e| e.to_string())?;
        if self.extent.iter().any(|v| *v > crate::MAX_EXTENT) { return Err("Oversized selection coverage".into()); }
        let count = self.pixels_per_word();
        let bits = 32 / count;
        let stride = self.extent[0].div_ceil(count) as usize;
        let [x0, y0, x1, y1] = self.bounds;
        for (y, row) in self.words.chunks_exact(stride).enumerate() {
            for (i, word) in row.iter().enumerate() {
                let start = i as u32 * count;
                let lo = x0.saturating_sub(start).min(count) * bits;
                let hi = x1.saturating_sub(start).min(count) * bits;
                let mask = if y >= y0 as usize && y < y1 as usize && lo < hi {
                    ((u64::from(u32::MAX) >> (32 - (hi - lo))) << lo) as u32
                } else { 0 };
                if word & !mask != 0 {
                    return Err("Selection bounds omit coverage or row padding is nonzero".into());
                }
            }
        }
        self.package_validated.get_or_init(|| ());
        Ok(())
    }
    pub(crate) fn package_chunks(&self) -> Result<&[Resource<[u8]>], String> {
        if let Some(chunks) = self.package_chunks.get() { return Ok(chunks); }
        self.validate_package()?;
        Ok(self.package_chunks.get_or_init(|| {
            let ids = self.transfer_chunk_ids();
            self.words.chunks(SELECTION_CHUNK_BYTES / 4).enumerate().map(|(index, words)| {
                let mut bytes = vec![0; SELECTION_CHUNK_BYTES];
                for (word, dest) in words.iter().zip(bytes.as_chunks_mut::<4>().0) {
                    dest.copy_from_slice(&word.to_le_bytes());
                }
                Resource::with_id(ids[index], lz4_flex::block::compress(&bytes).into())
            }).collect::<Vec<_>>().into()
        }))
    }
    pub(crate) fn from_package_chunks(
        extent: [u32; 2], bounds: [u32; 4], byte_coverage: bool, chunks: Vec<Resource<[u8]>>,
    ) -> Result<Self, String> {
        let count = u64::from(extent[0].div_ceil(if byte_coverage { 4 } else { 8 }))
            .checked_mul(u64::from(extent[1])).ok_or("Oversized selection coverage")?;
        let bytes = count.checked_mul(4).ok_or("Oversized selection coverage")?;
        let [x0, y0, x1, y1] = bounds;
        if extent.contains(&0) || extent.iter().any(|v| *v > crate::MAX_EXTENT)
            || x0 > x1 || y0 > y1 || x1 > extent[0] || y1 > extent[1]
            || bytes.div_ceil(SELECTION_CHUNK_BYTES as u64) != chunks.len() as u64 {
            return Err("Invalid selection chunk index".into());
        }
        let count = usize::try_from(count).map_err(|_| "Oversized selection coverage")?;
        let mut words = Vec::new();
        words.try_reserve_exact(count).map_err(|_| "Selection allocation failed")?;
        let mut decoded = vec![0; SELECTION_CHUNK_BYTES];
        for chunk in &chunks {
            if chunk.len() > lz4_flex::block::get_maximum_output_size(SELECTION_CHUNK_BYTES) {
                return Err("Oversized selection chunk".into());
            }
            let length = lz4_flex::block::decompress_into(chunk, &mut decoded).map_err(|e| e.to_string())?;
            if length != SELECTION_CHUNK_BYTES { return Err("Invalid selection chunk length".into()); }
            let used = (count - words.len()).min(SELECTION_CHUNK_BYTES / 4) * 4;
            if decoded[used..].iter().any(|v| *v != 0) { return Err("Nonzero selection chunk padding".into()); }
            words.extend(decoded[..used].as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)));
        }
        let value = Self::with_coverage(extent, bounds, words.into(), byte_coverage).map_err(|e| e.to_string())?;
        value.validate_package()?;
        value.package_chunks.set(chunks.into()).map_err(|_| "Selection chunk cache already initialized")?;
        Ok(value)
    }
    pub(crate) fn transfer_words(&self) -> &Arc<[u32]> { &self.words }
    pub(crate) fn transfer_chunks(&self) -> Option<&[Resource<[u8]>]> { self.package_chunks.get().map(|chunks| chunks.as_ref()) }
    pub(crate) fn transfer_chunk_ids(&self) -> &[crate::authored::PortableId] {
        self.package_chunk_ids.get_or_init(|| {
            if let Some(chunks) = self.package_chunks.get() { return chunks.iter().map(Resource::id).collect::<Vec<_>>().into(); }
            (0..(self.words.len() * 4).div_ceil(SELECTION_CHUNK_BYTES)).map(|_| crate::authored::PortableId::random()).collect::<Vec<_>>().into()
        })
    }
    pub(crate) fn from_verified_words(
        extent: [u32; 2], bounds: [u32; 4], byte_coverage: bool, words: Arc<[u32]>,
        ids: Vec<crate::authored::PortableId>, chunks: Option<Vec<Resource<[u8]>>>,
    ) -> Result<Self, String> {
        let count = u64::from(extent[0].div_ceil(if byte_coverage { 4 } else { 8 })) * u64::from(extent[1]);
        let [x0, y0, x1, y1] = bounds;
        if extent.contains(&0) || extent.iter().any(|v| *v > crate::MAX_EXTENT)
            || x0 > x1 || y0 > y1 || x1 > extent[0] || y1 > extent[1] || count != words.len() as u64
            || (count * 4).div_ceil(SELECTION_CHUNK_BYTES as u64) != ids.len() as u64
            || ids.iter().copied().collect::<std::collections::BTreeSet<_>>().len() != ids.len()
            || chunks.as_ref().is_some_and(|chunks| chunks.len() != ids.len() || chunks.iter().zip(&ids).any(|(chunk, id)| chunk.id() != *id)) {
            return Err("Invalid verified selection descriptor".into());
        }
        let value = Self { extent, bounds, words, byte_coverage, package_chunks: Arc::default(), package_chunk_ids: Arc::new(OnceLock::from(Arc::<[crate::authored::PortableId]>::from(ids))), words_validated: Arc::new(OnceLock::from(())), package_validated: Arc::default() };
        if let Some(chunks) = chunks {
            value.package_chunks.set(chunks.into()).map_err(|_| "Selection chunk cache already initialized")?;
            value.package_validated.get_or_init(|| ());
        }
        Ok(value)
    }
    pub fn pixels_per_word(&self) -> u32 {
        if self.byte_coverage { 4 } else { 8 }
    }
    pub fn coverage_format(&self) -> u32 {
        if self.byte_coverage { 2 } else { 1 }
    }
    pub fn extent(&self) -> [u32; 2] {
        self.extent
    }
    pub fn bounds(&self) -> [u32; 4] {
        self.bounds
    }
    pub fn words(&self) -> &[u32] {
        &self.words
    }
    /// Exact `[x0, y0, x1, y1]` bounds of the nonzero coverage, scanned
    /// within the stored bounds, which may be conservative. Empty when none.
    pub fn coverage_bounds(&self) -> [u32; 4] {
        let count = self.pixels_per_word();
        let bits = 32 / count;
        let stride = self.extent[0].div_ceil(count) as usize;
        let [x0, y0, x1, y1] = self.bounds;
        let mut result = [u32::MAX, u32::MAX, 0, 0];
        for y in y0..y1 {
            let row = &self.words[y as usize * stride..(y as usize + 1) * stride];
            for (index, &word) in row.iter().enumerate().take(x1.div_ceil(count) as usize).skip((x0 / count) as usize) {
                if word == 0 {
                    continue;
                }
                let x = index as u32 * count;
                result[0] = result[0].min(x + word.trailing_zeros() / bits);
                result[2] = result[2].max(x + count - word.leading_zeros() / bits);
                result[1] = result[1].min(y);
                result[3] = y + 1;
            }
        }
        if result[0] == u32::MAX { [0; 4] } else { result }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SelectionShape {
    /// Even/odd interiors support holes and disjoint islands.
    Contours(Arc<[Arc<[Point]>]>),
    Pixels(Arc<SelectionPixels>),
}

/// Geometry or immutable GPU-produced coverage. Affine placement and inversion are
/// metadata, so layer-local stroke snapshots never duplicate a selection image.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Selection {
    pub shape: SelectionShape,
    pub affine: crate::Affine,
    pub inverted: bool,
}
impl Selection {
    pub fn empty() -> Self {
        Self {
            shape: SelectionShape::Contours(Arc::default()),
            affine: Affine::IDENTITY,
            inverted: false,
        }
    }
    pub fn full() -> Self {
        Self {
            inverted: true,
            ..Self::empty()
        }
    }

    /// Shape validation without reading back or resampling coverage. Pixel bounds
    /// are additionally checked against their contents at the project boundary.
    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.affine.inverse().is_none() {
            return Err(DocumentError::InvalidLayerOperation(
                "Invalid selection transform",
            ));
        }
        match &self.shape {
            SelectionShape::Contours(paths) => {
                if paths
                    .iter()
                    .any(|p| p.len() < 3 || p.iter().any(|v| !v.x.is_finite() || !v.y.is_finite()))
                {
                    return Err(DocumentError::InvalidLayerOperation(
                        "Invalid selection contour",
                    ));
                }
            }
            SelectionShape::Pixels(pixels) => pixels.validate()?,
        }
        Ok(())
    }

    pub fn polygon(points: Vec<Point>) -> Result<Self, DocumentError> {
        if points.len() < 3 || points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            return Err(DocumentError::InvalidLayerOperation(
                "A selection needs a closed area",
            ));
        }
        Ok(Self {
            shape: SelectionShape::Contours(vec![points.into()].into()),
            affine: crate::Affine::IDENTITY,
            inverted: false,
        })
    }
    pub fn pixels(pixels: Arc<SelectionPixels>) -> Self {
        Self {
            shape: SelectionShape::Pixels(pixels),
            affine: crate::Affine::IDENTITY,
            inverted: false,
        }
    }
    pub fn contours(&self) -> &[Arc<[Point]>] {
        match &self.shape {
            SelectionShape::Contours(paths) => paths,
            SelectionShape::Pixels(_) => &[],
        }
    }
    /// Conservative local bounds, including one pixel for boundary sampling.
    pub fn bounds(&self) -> Rect {
        let mut bounds = Rect::EMPTY;
        match &self.shape {
            SelectionShape::Contours(paths) => {
                for p in paths.iter().flat_map(|c| c.iter()) {
                    bounds.include_circle(*p, 1.);
                }
            }
            SelectionShape::Pixels(pixels) => {
                let [x0, y0, x1, y1] = pixels.bounds;
                if x0 != x1 && y0 != y1 {
                    bounds.include_circle(
                        Point {
                            x: x0 as f32,
                            y: y0 as f32,
                        },
                        1.,
                    );
                    bounds.include_circle(
                        Point {
                            x: x1 as f32,
                            y: y1 as f32,
                        },
                        1.,
                    );
                }
            }
        }
        self.affine.bounds(bounds)
    }
    /// Document bounds of the nonzero coverage, ignoring `inverted` and
    /// without the sampling margin `bounds` adds.
    pub fn coverage_bounds(&self) -> Rect {
        let local = match &self.shape {
            SelectionShape::Contours(paths) => Rect::around(paths.iter().flat_map(|c| c.iter().copied())),
            SelectionShape::Pixels(pixels) => match pixels.coverage_bounds() {
                [x0, y0, x1, y1] if x0 < x1 && y0 < y1 => Rect {
                    min: Point { x: x0 as f32, y: y0 as f32 },
                    max: Point { x: x1 as f32, y: y1 as f32 },
                },
                _ => Rect::EMPTY,
            },
        };
        self.affine.bounds(local)
    }
    pub fn translated(&self, delta: Point) -> Self {
        Self {
            shape: self.shape.clone(),
            affine: self.affine.then(crate::Affine::translation(delta)),
            inverted: self.inverted,
        }
    }
    /// Compose placement without modifying geometry or resampling coverage.
    /// GPU consumers sample the immutable source only when they need pixels.
    pub fn transformed(&self, affine: crate::Affine) -> Result<Self, DocumentError> {
        let affine = self.affine.then(affine);
        if affine.inverse().is_none() {
            return Err(DocumentError::InvalidLayerOperation(
                "Invalid selection transform",
            ));
        }
        Ok(Self {
            shape: self.shape.clone(),
            affine,
            inverted: self.inverted,
        })
    }
    /// The selection carried by a pixel transform's geometry. Contours map
    /// exactly under perspective, and within a pixel under a mesh, after
    /// clipping away any part with no image; pixel coverage must be resampled
    /// by the renderer instead.
    pub fn mapped(&self, map: &crate::LayerPlacement) -> Result<Self, DocumentError> {
        if let Some(affine)=map.as_affine(){return self.transformed(affine);}
        let invalid=DocumentError::InvalidLayerOperation("Invalid selection transform");
        let SelectionShape::Contours(paths)=&self.shape else{return Err(DocumentError::InvalidLayerOperation(Self::RESAMPLE_PIXELS));};
        let paths=if let Some(mesh)=&map.mesh {map.outer.map_polygons(&mesh.map_polygons(paths,self.affine).ok_or(invalid)?)}
            else {crate::Projective::from_affine(self.affine).then(map.outer).ok_or(invalid)?.map_polygons(paths)};
        Ok(Self{shape:SelectionShape::Contours(paths.into()),affine:crate::Affine::IDENTITY,inverted:self.inverted})
    }
    pub fn needs_resample(&self,map:&crate::LayerPlacement)->bool {matches!(self.shape,SelectionShape::Pixels(_))&&map.as_affine().is_none()}
    /// The error when pixel coverage cannot follow a map without the renderer.
    pub const RESAMPLE_PIXELS: &'static str = "Pixel selections are resampled by the renderer";
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn soft_mask() -> Selection {
        Selection::pixels(Arc::new(
            SelectionPixels::bytes([4, 1], [0, 0, 4, 1], vec![0xff804020]).unwrap(),
        ))
    }

    use crate::operation_test_support as fixture;
    use fixture::*;
    #[test]
    fn saved_selection_roundtrip_and_working_copy_are_independent() {
        let mut doc=fixture::document([64,64],&["Ink","Hair"]);let original=soft_mask();saved(&mut doc,"Hair",original.clone());let id=id(&doc,"Hair");let OccurrenceContent::Selection(handle)=occurrence(&doc,"Hair").content else{panic!("selection")};
        let portable=encoded(&doc);let artwork=doc.artwork.clone();let properties=SelectionMaskProperties {opacity:0.35,..Default::default()};doc.working.selection_overlays.properties.insert(handle,properties.clone());doc.working.selection_overlays.visibility.insert(id,false);assert_eq!(doc.artwork,artwork);assert_eq!(encoded(&doc),portable);assert!(!doc.effective_visibility(id));let mut editor=Editor::new(doc);let saved_checkpoint=editor.checkpoint();let loaded=editor.document().saved_selection(id).unwrap();let (SelectionShape::Pixels(saved),SelectionShape::Pixels(copy))=(&original.shape,&loaded.shape) else{panic!("pixels")};assert!(Arc::ptr_eq(saved,copy));
        editor.perform(editor.document().selection_edit(SelectionTarget::Current,loaded.clone()).unwrap()).unwrap();assert_eq!(editor.checkpoint(),saved_checkpoint);let edit=editor.document().selection_edit(SelectionTarget::Saved(id),Selection::full()).unwrap();assert!(!edit.changes_image(editor.document()));editor.perform(edit).unwrap();assert_ne!(editor.checkpoint(),saved_checkpoint);assert_eq!(editor.document().working.selection,Some(loaded));editor.undo().unwrap();assert_eq!(editor.document().saved_selection(id).unwrap(),original);editor.redo().unwrap();
        let restored=roundtrip(editor.document());let restored_id=fixture::id(&restored,"Hair");let OccurrenceContent::Selection(restored_handle)=occurrence(&restored,"Hair").content else{panic!("selection")};assert_eq!(restored.saved_selection(restored_id).unwrap(),Selection::full());assert_eq!(occurrence(&restored,"Hair").name.as_ref(),"Hair");assert!(restored.artwork.selections.get(restored_handle).is_some());assert!(restored.working.selection_overlays.properties.is_empty());assert!(restored.working.selection_overlays.visibility.is_empty());assert!(restored.effective_visibility(restored_id));assert!(restored.working.selection.is_none());
        editor.perform(editor.document().delete_layers_edit(&[id]).unwrap()).unwrap();assert_eq!(editor.document().working.selection,Some(original));editor.undo().unwrap();assert!(editor.document().scene().occurrence(id).is_some());
    }
    #[test]
    fn saved_selection_resolves_group_placement_and_checks_ancestor_locks() {
        let mut doc=fixture::document([64,64],&["Ink","Character","Hair"]);let group=nest(&mut doc,"Character",&["Hair"]);occurrence_mut(&mut doc,"Character").translation=Point{x:12.,y:8.};saved(&mut doc,"Hair",soft_mask());let id=fixture::id(&doc,"Hair");occurrence_mut(&mut doc,"Hair").placement=LayerPlacement::from_affine(Affine::around(Point::default(),[2.,2.],0.,Point::default()));let target=fixture::target(&doc,"Hair");let world=doc.saved_selection(id).unwrap();assert_eq!(world.affine,doc.affine_edit_transform(target).unwrap());let replacement=world.translated(Point{x:2.,y:4.});doc.apply(doc.selection_edit(SelectionTarget::Saved(id),replacement.clone()).unwrap()).unwrap();assert_eq!(doc.saved_selection(id).unwrap(),replacement);
        let mut locked=doc.scene().occurrence(group).unwrap().clone();locked.locked=true;doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,group,Some(locked)).unwrap())).unwrap();assert!(doc.selection_edit(SelectionTarget::Saved(id),Selection::empty()).is_err());assert!(doc.selection_edit(SelectionTarget::Current,Selection::empty()).is_ok());assert!(doc.saved_selection(id).is_ok());assert!(doc.saved_selection(fixture::id(&doc,"Ink")).is_err());
    }
    #[test]
    fn selection_nodes_reject_artwork_and_project_limits_include_saved_coverage() {
        let mut doc=fixture::document([64,64],&["Ink","Region"]);saved(&mut doc,"Region",soft_mask());let id=fixture::id(&doc,"Region");let region=occurrence(&doc,"Region").clone();let mask=doc.artwork.coverage.next_handle();let coverage=CoverageSnapshot::reveal_all(mask,[64;2],Point::default());doc.artwork.coverage.insert(PortableId::random(),coverage.source).unwrap();
        for mutate in [|o:&mut Occurrence|o.opacity=0.5,|o:&mut Occurrence|o.attachment = crate::Attachment::Clip,|o:&mut Occurrence|o.alpha_locked=true,
            |o:&mut Occurrence|o.visible=false,|o:&mut Occurrence|o.reference=true] {let mut invalid=region.clone();mutate(&mut invalid);assert!(doc.clone().apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,id,Some(invalid)).unwrap())).is_err());}
        let mut invalid=region.clone();invalid.mask=Some(coverage.use_);assert!(doc.clone().apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,id,Some(invalid)).unwrap())).is_err());let mut missing=doc.artwork.clone();let OccurrenceContent::Selection(handle)=region.content else{panic!("selection")};missing.selections.remove(handle);assert!(Document::from_artwork(missing).is_err());
        let bytes=encoded(&doc);let source=crate::package::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes))).unwrap();let outcome=crate::package::codec::open(source,ProjectLimits {raster_bytes:3,..Default::default()},&std::sync::atomic::AtomicBool::new(false)).unwrap();assert!(!matches!(outcome,crate::package::codec::OpenOutcome::Candidate {..}));let restored=roundtrip(&fixture::document([64,64],&["Ink"]));assert!(restored.artwork.selections.is_empty());assert_ne!(Selection::empty(),Selection::full());assert!(Selection::empty().validate().is_ok());
    }
    #[test]
    fn saved_rows_do_not_interrupt_artwork_clipping_or_accept_raster_edits() {
        let mut doc=fixture::document([64,64],&["Highlights","Region","Ink"]);saved(&mut doc,"Region",Selection::empty());occurrence_mut(&mut doc,"Highlights").attachment = crate::Attachment::Clip;let saved_id=fixture::id(&doc,"Region");let clip=fixture::id(&doc,"Highlights");let base=fixture::id(&doc,"Ink");let target=fixture::target(&doc,"Region");assert_eq!(doc.clipping_base(clip),Some(base));assert_eq!(doc.clipping_stack_top(base),Some(clip));assert!(doc.target_raster(target).is_none());assert!(doc.apply(Edit::SetRaster {target,revision:Default::default()}).is_err());let mut invalid=doc.scene().occurrence(saved_id).unwrap().clone();invalid.opacity=0.5;assert!(doc.apply(Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences,saved_id,Some(invalid)).unwrap())).is_err());
    }
    #[test]
    fn affine_placement_keeps_source_and_composes_with_local_offsets() {
        let pixels = Arc::new(SelectionPixels::new([8, 1], [2, 0, 4, 1], vec![0x4400]).unwrap());
        let original = Selection::pixels(pixels.clone());
        let transform = crate::Affine::around(
            Point { x: 2., y: 3. },
            [2., -3.],
            0.4,
            Point { x: 5., y: 7. },
        );
        let placed = original
            .transformed(transform)
            .unwrap()
            .translated(Point { x: -12., y: 21. });
        let SelectionShape::Pixels(shared) = &placed.shape else {
            panic!("pixels")
        };
        assert!(Arc::ptr_eq(shared, &pixels));
        assert_eq!(
            placed.bounds(),
            transform
                .then(crate::Affine::translation(Point { x: -12., y: 21. }))
                .bounds(original.bounds())
        );
        let restored = placed
            .transformed(placed.affine.inverse().unwrap())
            .unwrap();
        for (a, b) in restored.affine.0.into_iter().zip(crate::Affine::IDENTITY.0) {
            assert!((a - b).abs() < 0.0001);
        }
        assert!(original.transformed(crate::Affine([0.; 6])).is_err());
        assert_eq!(original.affine, crate::Affine::IDENTITY);
    }
    #[test]
    fn perspective_clips_contours_beyond_the_horizon() {
        use crate::{Projective, LayerPlacement};
        let source = Rect { min: Point::default(), max: Point { x: 100., y: 100. } };
        let quad = [Point { x: 40., y: 0. }, Point { x: 60., y: 0. }, Point { x: 100., y: 100. }, Point { x: 0., y: 100. }];
        let map = LayerPlacement::from_projective(Projective::rect_to_quad(source, quad).unwrap());
        let tall = Selection::polygon(vec![
            Point { x: 0., y: 0. },
            Point { x: 100., y: 0. },
            Point { x: 100., y: 1000. },
            Point { x: 0., y: 1000. },
        ])
        .unwrap();
        let tall = tall.mapped(&map).unwrap();
        let [clipped] = tall.contours() else { panic!("one ring") };
        assert!(clipped.len() == 4 && clipped.iter().all(|p| p.x.is_finite() && p.y.is_finite()));
        assert!(clipped.iter().all(|p| p.y >= -1.), "the part with no image is clipped away");
        let beyond = Selection::polygon(vec![
            Point { x: 0., y: 200. },
            Point { x: 10., y: 200. },
            Point { x: 5., y: 300. },
        ])
        .unwrap();
        assert!(beyond.mapped(&map).unwrap().contours().is_empty());
        assert!(beyond.mapped(&LayerPlacement::from_projective(Projective([f32::NAN; 9]))).is_err());
        let pixels = Selection::pixels(Arc::new(SelectionPixels::new([8, 1], [0, 0, 8, 1], vec![0x4444]).unwrap()));
        assert_eq!(
            pixels.mapped(&map),
            Err(DocumentError::InvalidLayerOperation(Selection::RESAMPLE_PIXELS))
        );
    }
    #[test]
    fn packed_coverage_validation_checks_all_nibbles_and_dimensions() {
        for value in 0..16 {
            for shift in (0..32).step_by(4) {
                assert_eq!(
                    SelectionPixels::new([8, 1], [0, 0, 8, 1], vec![value << shift]).is_ok(),
                    value <= 4
                );
            }
        }
        for (extent, bounds, words) in [
            ([0, 1], [0, 0, 0, 1], vec![]),
            ([9, 1], [0, 0, 9, 1], vec![0]),
            ([1, 1], [0, 0, 2, 1], vec![0]),
            ([1, 1], [1, 0, 0, 1], vec![0]),
            ([u32::MAX, u32::MAX], [0, 0, 1, 1], vec![0]),
        ] {
            assert!(SelectionPixels::new(extent, bounds, words).is_err());
        }
    }
    #[test]
    fn translating_and_inverting_share_immutable_selection_storage() {
        let pixels = Arc::new(SelectionPixels::new([8, 1], [2, 0, 4, 1], vec![0x4400]).unwrap());
        let original = Selection::pixels(pixels.clone());
        let mut moved = original.translated(Point { x: -2., y: 7.5 });
        moved.inverted = true;
        let SelectionShape::Pixels(shared) = &moved.shape else {
            panic!("pixels")
        };
        assert!(Arc::ptr_eq(shared, &pixels));
        assert!(!original.inverted);
        assert_eq!(original.affine, crate::Affine::IDENTITY);
        assert_eq!(
            moved.bounds(),
            Rect {
                min: Point { x: -1., y: 6.5 },
                max: Point { x: 3., y: 9.5 }
            }
        );

        let polygon = Selection::polygon(vec![
            Point { x: 1., y: 1. },
            Point { x: 5., y: 1. },
            Point { x: 5., y: 5. },
        ])
        .unwrap();
        let moved = polygon.translated(Point { x: 10., y: -4. });
        assert!(Arc::ptr_eq(&polygon.contours()[0], &moved.contours()[0]));
        assert_eq!(
            moved.bounds(),
            Rect {
                min: Point { x: 10., y: -4. },
                max: Point { x: 16., y: 2. }
            }
        );
    }
}

#[cfg(test)]
mod refinement_tests {
    use super::*;
    use crate::operation_test_support as fixture;
    use fixture::*;
    fn mask(word:u32)->Selection {Selection::pixels(Arc::new(SelectionPixels::bytes([4,1],[0,0,4,1],vec![word]).unwrap()))}
    fn document()->Document {let mut doc=fixture::document([4,1],&["Ink","Mask"]);saved(&mut doc,"Mask",Selection::empty());doc}
    #[test]
    fn refined_selections_keep_original_undo_and_final_redo() {
        for current in [false,true] {let doc=document();let saved=id(&doc,"Mask");let target=if current{SelectionTarget::Current}else{SelectionTarget::Saved(saved)};let original=doc.clone();let mut editor=Editor::new(doc);editor.perform(editor.document().selection_edit(target,mask(0xff000000)).unwrap()).unwrap();let checkpoint=editor.checkpoint();for word in [0xff800000,0xff808000]{editor.refine_selection(target,mask(word),editor.document().revision).unwrap();}assert_eq!(editor.checkpoint()==checkpoint,current);editor.undo().unwrap();assert_eq!(editor.document().working.selection,original.working.selection);assert_eq!(editor.document().saved_selection(saved).unwrap(),Selection::empty());assert!(!editor.can_undo());editor.redo().unwrap();let coverage=match target{SelectionTarget::Current=>editor.document().working.selection.clone().unwrap(),SelectionTarget::Saved(id)=>editor.document().saved_selection(id).unwrap()};assert_eq!(coverage,mask(0xff808000));}
    }
    #[test]
    fn refinement_rejects_stale_revision_and_undo_branches() {
        let mut editor=Editor::new(document());editor.perform(editor.document().selection_edit(SelectionTarget::Current,mask(0xff)).unwrap()).unwrap();let revision=editor.document().revision;let mut working=editor.document().working.clone();working.selection=None;editor.perform(Edit::Working(working)).unwrap();assert!(editor.refine_selection(SelectionTarget::Current,mask(0xff00),revision).is_err());assert!(editor.document().working.selection.is_none());editor.undo().unwrap();assert!(editor.refine_selection(SelectionTarget::Current,mask(0xff00),editor.document().revision).is_err());assert!(editor.can_redo());
    }
    #[test]
    fn withdrawn_refinements_leave_no_history() {
        for current in [false,true] {let mut doc=document();let saved_id=id(&doc,"Mask");let OccurrenceContent::Selection(h)=occurrence(&doc,"Mask").content else{panic!("selection")};doc.artwork.selections.get_mut(h).unwrap().selection=mask(0xff);doc.working.selection=Some(mask(0xff00));let target=if current{SelectionTarget::Current}else{SelectionTarget::Saved(saved_id)};let mut editor=Editor::new(doc);editor.perform(editor.document().ruler_edit(Vec::new()).unwrap()).unwrap();let (original,checkpoint)=(editor.document().clone(),editor.checkpoint());editor.perform(editor.document().selection_edit(target,mask(0xff000000)).unwrap()).unwrap();editor.refine_selection(target,mask(0xff800000),editor.document().revision).unwrap();assert!(editor.withdraw_selection(target,editor.document().revision-1).is_err());editor.withdraw_selection(target,editor.document().revision).unwrap();assert_eq!(editor.document().working.selection,original.working.selection);assert_eq!(editor.document().saved_selection(saved_id),original.saved_selection(saved_id));assert_eq!(editor.checkpoint(),checkpoint);assert!(!editor.can_redo());editor.undo().unwrap();assert!(!editor.can_undo());assert!(editor.withdraw_selection(target,editor.document().revision).is_err());}
    }
}
