use super::*;
use layer_core::{
    Edit, Point, Rect, Selection,
    color::{
        DocumentColor,
        source::SourceImage,
    },
};
use layer_core::authored::{Occurrence, OccurrenceContent, OccurrenceHandle, PaintBase, PaintBasePolicy, PaintSource, RecordChange, SceneSnapshot, SceneScope, SourceTarget};
use std::sync::Arc;

/// Copies larger than this show the import-style progress with Cancel.
pub const LARGE_CLIP_PIXELS: u64 = 1 << 21;
const NO_COVERAGE: MessageId = MessageId::COMMANDS_REFUSAL_CLIPBOARD_THE_SELECTION_DOESN_T_COVER_ANY_OF_THE_CANVAS;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteMode {
    #[default]
    Paste,
    /// Always at the copied position, with no handles.
    InPlace,
    AtView,
    AtCursor,
    /// In place, with a mask from the selection.
    Into,
    NewImage,
}

/// Pixels on the clipboard, held by the window, or on GTK the application.
#[derive(Clone)]
pub struct PixelClip {
    /// Written to the system clipboard beside the PNG. A clipboard that still
    /// carries it holds this clip.
    pub nonce: String,
    pub name: String,
    pub source: Arc<SourceImage>,
    pub policy: PaintBasePolicy,
    /// Document pixels of the source's top-left corner where it was copied.
    pub origin: [i64; 2],
    pub color: DocumentColor,
    pub blend: layer_core::BlendSpace,
    /// sRGB 8-bit rendition for other applications.
    pub png: Arc<[u8]>,
    pub objects: Option<Arc<ObjectClip>>,
    pub layers: Option<Arc<LayerClip>>,
}

/// Selected image objects in document coordinates, front to back. The pixel
/// fields of their clip are the rendered fallback of their unclipped bounds.
#[derive(Clone, Debug)]
pub struct ObjectClip {
    pub objects: Vec<layer_core::ImageObject>,
}
#[derive(Clone, Debug)]
pub struct LayerClip {
    pub scene: Arc<SceneSnapshot>,
    pub roots: Vec<OccurrenceHandle>,
}
impl PixelClip {
    pub fn document(&self, localization: &Localizer) -> Result<layer_core::Document, String> {
        let mut document = layer_color::photo_project((*self.source).clone(), Default::default(),
            photo_document_names(&self.name, localization), self.color.depth)?;
        let composition = document.artwork.compositions.get_mut(document.artwork.root).unwrap();
        composition.color = self.color;
        composition.blend = self.blend;
        let SourceTarget::Paint(paint) = document.working.target.unwrap() else { unreachable!() };
        document.artwork.paint.get_mut(paint).unwrap().base = Some(self.source_for(self.color));
        if let Some(layers) = &self.layers {
            let initial = document.working.occurrence.unwrap();
            let delta = self.origin.map(|v| -v);
            let (edit, _) = document.import_layers_edit(&layers.scene, &layers.roots, None, 0, delta).map_err(error)?;
            document.apply(edit).map_err(error)?;
            document.apply(document.delete_layers_edit(&[initial]).map_err(error)?).map_err(error)?;
        }
        if let Some(objects) = &self.objects {
            let shift = layer_core::Affine64([1., 0., 0., 1., -self.origin[0] as f64, -self.origin[1] as f64]);
            clipboard_objects(&mut document, &self.name, objects.objects.iter().map(|object| layer_core::ImageObject {
                affine: shift.compose(object.affine), ..object.clone()
            }).collect())?;
        }
        document.validate(Default::default())?;
        layer_color::validate_document_color(&document)?;
        Ok(document)
    }
    /// The source as a paste into a drawing of `color` holds it: document
    /// pixels when the colour settings match, otherwise an original image
    /// converted through its explicit profile.
    pub fn source_for(&self, color: DocumentColor) -> PaintBase {
        PaintBase { image: self.source.clone().into(), offset: [0; 2],
            policy: if color == self.color { self.policy } else { PaintBasePolicy::SourceProfile } }
    }
}

pub fn clipboard_document(sources: Vec<(String, SourceImage)>, policy: PhotoOpenPolicy, localization: &Localizer) -> Result<layer_core::Document, String> {
    let (name, first) = sources.first().ok_or("Copy an image to paste")?;
    let extent = sources.iter().fold([0, 0], |extent, (_, source)| std::array::from_fn(|i| extent[i].max(source.extent[i])));
    let depth = sources.iter().map(|(_, source)| policy.editing_depth(source.interpretation.depth)).max_by_key(|depth| (depth.is_float(), depth.bits())).unwrap();
    let mut document = layer_color::photo_project(first.clone(), Default::default(), photo_document_names(name, localization), depth)?;
    if sources.len() > 1 {
        document.artwork.compositions.get_mut(document.artwork.root).unwrap().size = extent;
        let objects = sources.into_iter().map(|(name, source)| {
            let at = std::array::from_fn(|i| f64::from((extent[i] - source.extent[i]) / 2));
            placed(&name, Arc::new(source).into(), at)
        }).collect();
        clipboard_objects(&mut document, &localization.text(MessageId::OBJECTS_LAYER_NAME), objects)?;
    }
    document.validate(Default::default())?;
    layer_color::validate_document_color(&document)?;
    Ok(document)
}

fn clipboard_objects(document: &mut layer_core::Document, name: &str, objects: Vec<layer_core::ImageObject>) -> Result<(), String> {
    let paint = document.working.occurrence.unwrap();
    let (layer, edit) = document.create_object_layer_edit(layer_core::bounded_name(name), None, 0).map_err(error)?;
    document.apply(edit).map_err(error)?;
    let (handles, edit) = document.import_image_objects_edit(layer, objects, 0).map_err(error)?;
    document.apply(edit).map_err(error)?;
    document.apply(document.delete_layers_edit(&[paint]).map_err(error)?).map_err(error)?;
    document.working.occurrence = Some(layer);
    document.working.target = None;
    document.working.layer_selection = [layer].into();
    document.working.layer_anchor = Some(layer);
    document.working.objects = handles.into_iter().collect();
    Ok(())
}

/// A frozen copy request for the host's clip worker.
#[derive(Clone)]
pub struct ClipboardCapture {
    pub scene: Arc<SceneSnapshot>,
    pub scope: SceneScope,
    /// `[x, y, width, height]` of the copied document pixels.
    pub crop: [u32; 4],
    /// Coverage the worker multiplies into the captured rows.
    pub coverage: Option<Arc<Selection>>,
    /// An untouched photo copied whole; the clip keeps its original samples.
    pub original: Option<Arc<SourceImage>>,
    pub policy: PaintBasePolicy,
    pub name: String,
    /// Show the import-style progress with Cancel.
    pub large: bool,
    pub origin: [i64; 2],
    /// A signed document window evaluated instead of the composition frame;
    /// `crop` is then relative to it.
    pub window: Option<([i64; 2], [u32; 2])>,
    pub objects: Option<Arc<ObjectClip>>,
    pub layers: Option<Arc<LayerClip>>,
}
impl ClipboardCapture {
    pub fn color(&self) -> DocumentColor {
        self.scene.view().composition().color
    }
    pub fn finish(mut self, nonce: String, source: Arc<SourceImage>, png: Vec<u8>) -> Result<PixelClip, String> {
        if let Some(layers) = &mut self.layers {
            let layers = Arc::make_mut(layers);
            Arc::make_mut(&mut layers.scene).context = self.scene.context.clone();
            let composition = layers.scene.view().composition();
            let mut artwork = layer_core::Artwork::new(composition.size).map_err(error)?;
            let target = artwork.compositions.get_mut(artwork.root).unwrap();
            target.color = composition.color; target.blend = composition.blend;
            let mut document = layer_core::Document::from_artwork(artwork).map_err(error)?;
            let (edit, roots) = document.import_layers_edit(&layers.scene, &layers.roots, None, 0, [0; 2]).map_err(error)?;
            document.apply(edit).map_err(error)?;
            *layers = LayerClip { scene: document.snapshot(), roots };
        }
        Ok(PixelClip {
            nonce,
            name: self.name,
            color: self.scene.view().composition().color,
            blend: self.scene.view().composition().blend,
            source,
            policy: self.policy,
            origin: self.origin,
            png: png.into(),
            objects: self.objects,
            layers: self.layers,
        })
    }
}

/// The capture a pending Cut erases once the host reports success.
pub(super) struct PendingCut {
    epoch: u64,
    revision: u64,
    layer: OccurrenceHandle,
    target: Option<SourceTarget>,
    objects: std::collections::BTreeSet<layer_core::ImageObjectHandle>,
    layers: Vec<OccurrenceHandle>,
}

/// Whether `selection` covers every pixel of `canvas` fully, as Select All does.
fn selects_everything(selection: &Selection, [width, height]: [u32; 2]) -> bool {
    if selection.inverted {
        return selection.coverage_bounds().is_empty();
    }
    let [contour] = selection.contours() else {
        return false;
    };
    let corners: Vec<_> = contour.iter().map(|p| selection.affine.map(*p)).collect();
    let bounds = Rect::around(corners.iter().copied());
    corners.len() == 4
        && corners.iter().all(|p| (p.x == bounds.min.x || p.x == bounds.max.x) && (p.y == bounds.min.y || p.y == bounds.max.y))
        && bounds.min.x <= 0.
        && bounds.min.y <= 0.
        && bounds.max.x >= width as f32
        && bounds.max.y >= height as f32
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn mark_startup_drawing(&mut self) { self.files.startup = true; }
    pub fn pasting_new_image(&self) -> bool {
        self.state.requests.iter().any(|request| matches!(request.kind,
            HostRequestKind::Document { request: DocumentRequest::Paste { mode: PasteMode::NewImage } }))
    }
    /// Why Copy, Cut or Copy Merged can't run on the idle document.
    pub(super) fn copy_refusal(&self, command: CommandId) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        let document = self.engine.document();
        if self.selection_masks.quick() {
            return Some(l.text(MessageId::COMMANDS_REFUSAL_CLIPBOARD_QUICK_MASK_EDITS_THE_SELECTION_LEAVE_IT_TO_COPY_ARTWORK));
        }
        if self.selection_masks.target().is_some() {
            return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST));
        }
        if matches!(command, CommandId::Copy | CommandId::Cut) && !document.working.objects.is_empty() && let Some(layer) = self.object_target() {
            return (command == CommandId::Cut && !document.objects_editable(layer)).then(|| l.text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED));
        }
        if self.copies_layers(command) {
            let roots = self.clipboard_layer_roots();
            if roots.is_empty() { return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_SELECT_LAYERS_FIRST)); }
            return (command == CommandId::Cut && !document.can_delete_layers(&document.layer_subtrees(&roots).into_iter().collect::<Vec<_>>()))
                .then(|| l.text(MessageId::COMMANDS_THE_ACTIVE_LAYER_IS_LOCKED));
        }
        if !self.selection_meets_canvas() {
            return Some(l.text(MessageId::COMMANDS_REFUSAL_CLIPBOARD_THE_SELECTION_DOESN_T_COVER_ANY_OF_THE_CANVAS));
        }
        if command == CommandId::CopyMerged {
            return None;
        }
        if matches!(document.working.target, Some(SourceTarget::Coverage(_))) {
            return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST));
        }
        let Some(layer) = document.working.occurrence.and_then(|id| document.scene().occurrence(id)) else {
            return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_SELECT_A_LAYER_FIRST));
        };
        match layer.kind() {
            LayerKind::Paint | LayerKind::Object => {}
            LayerKind::Group => return Some(notices::drawing_refusal_text(layer_core::DrawingRefusal::Group, l)),
            LayerKind::Effect => return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_AN_EFFECT_LAYER_HAS_NO_PIXELS_OF_ITS_OWN)),
            LayerKind::Selection => {
                return Some(notices::drawing_refusal_text(layer_core::DrawingRefusal::SelectionLayer, l));
            }
        }
        if command == CommandId::Cut {
            return self.clear_refusal();
        }
        None
    }

    pub(super) fn paste_refusal(&self) -> Option<Arc<str>> {
        if self.selection_masks.target().is_some() || matches!(self.engine.document().working.target, Some(SourceTarget::Coverage(_))) {
            return Some(self.localization().text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST));
        }
        self.image_layer_destination(None).err().map(|_| self.localization().text(MessageId::COMMANDS_REFUSAL_RETOUCH_LAYERS_THE_DESTINATION_GROUP_IS_LOCKED))
    }

    pub(super) fn paste_into_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        if let Some(reason) = self.paste_refusal() { return Some(reason); }
        self.engine.document().working.selection.is_none().then_some(l.text(MessageId::COMMANDS_REFUSAL_CLIPBOARD_MAKE_A_SELECTION_TO_PASTE_INTO))
    }

    /// Whether the selection's conservative bounds reach the canvas. Command
    /// states read this every frame, so it never scans pixel coverage.
    fn selection_meets_canvas(&self) -> bool {
        let document = self.engine.document();
        document.working.selection.as_ref().filter(|s| !s.inverted).is_none_or(|selection| {
            let bounds = selection.bounds();
            !bounds.is_empty()
                && bounds.max.x > 0.
                && bounds.max.y > 0.
                && bounds.min.x < document.composition().size[0] as f32
                && bounds.min.y < document.composition().size[1] as f32
        })
    }

    /// Document pixels the copy covers: the selection's bounds on the canvas,
    /// or the whole canvas.
    fn clipboard_crop(&self) -> Result<[u32; 4], String> {
        let document = self.engine.document();
        let canvas = document.composition().size;
        let Some(selection) = document.working.selection.as_ref().filter(|s| !s.inverted) else {
            return Ok([0, 0, canvas[0], canvas[1]]);
        };
        let bounds = selection.coverage_bounds();
        const TOLERANCE: f32 = 1e-3;
        let min = [bounds.min.x, bounds.min.y].map(|v| (v + TOLERANCE).floor().max(0.));
        let max = [(bounds.max.x, canvas[0]), (bounds.max.y, canvas[1])]
            .map(|(v, limit)| (v - TOLERANCE).ceil().min(limit as f32));
        if bounds.is_empty() || (0..2).any(|axis| max[axis] <= min[axis]) {
            return Err(self.localization().text(NO_COVERAGE).to_string());
        }
        Ok([min[0] as u32, min[1] as u32, (max[0] - min[0]) as u32, (max[1] - min[1]) as u32])
    }

    pub(super) fn request_copy(&mut self, command: CommandId) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.copy_refusal(command))?;
        if command == CommandId::Cut && !self.copies_objects(command) && !self.copies_layers(command) && self.refuse_image_content() {
            return Ok(());
        }
        if self.copies_objects(command) { self.object_clip_window()?; } else { self.clipboard_crop()?; }
        self.request_document(DocumentRequest::Copy {
            merged: command == CommandId::CopyMerged,
            cut: command == CommandId::Cut,
            pixels: command == CommandId::CopyPixels,
        })
    }

    pub(super) fn request_paste(&mut self, mode: PasteMode) -> Result<(), String> {
        let mode = if mode == PasteMode::Paste && self.files.startup
            && self.state.document_file.location.is_none() && !self.state.document_file.modified
            && !self.engine.can_undo() && !self.engine.can_redo() && self.engine.document().working.selection.is_none()
        { PasteMode::NewImage } else { mode };
        if mode == PasteMode::Into
            && let Some(reason) = self.paste_into_refusal()
        {
            return Err(reason.to_string());
        }
        if mode != PasteMode::NewImage { refused(self.paste_refusal())?; }
        self.request_document(DocumentRequest::Paste { mode })?;
        self.files.paste_center = Some(self.paste_center(mode));
        Ok(())
    }

    /// Freeze a pending Copy, Cut or Copy Merged for the host's clip worker.
    pub fn capture_clipboard(&mut self, id: u32) -> Result<ClipboardCapture, String> {
        let DocumentRequest::Copy { merged, cut, pixels } = *self.document_request(id)? else {
            return Err("This is not a copy request".into());
        };
        self.require_document_idle()?;
        let command = match (merged, cut, pixels) {
            (true, ..) => CommandId::CopyMerged,
            (_, true, _) => CommandId::Cut,
            (.., true) => CommandId::CopyPixels,
            _ => CommandId::Copy,
        };
        refused(self.copy_refusal(command))?;
        if self.copies_objects(command) { return self.capture_objects(cut); }
        if self.copies_layers(command) { return self.capture_layers(cut); }
        let crop = self.clipboard_crop()?;
        let selection = self.engine.document().working.selection.clone();
        let canvas = self.engine.document().composition().size;
        let coverage = selection.filter(|s| !selects_everything(s, canvas)).map(Arc::new);
        self.require_raster_snapshot()?;
        let scene = self.engine.scene_snapshot();
        let document = self.engine.document();
        let active = document.working.occurrence;
        let (scope, name, original, policy) = if merged {
            (SceneScope::All, "Merged copy".to_string(), None, PaintBasePolicy::WorkingPixels)
        } else {
            let active = active.ok_or("Select a layer first")?;
            let occurrence = document.scene().occurrence(active).ok_or("Select a layer first")?;
            if document.scene().object_layer(active).is_some() {
                return Ok(self.pixel_capture(scene, SceneScope::RawObjects(active), crop, coverage, None, PaintBasePolicy::WorkingPixels, occurrence.name.to_string(), None));
            }
            let paint = document.scene().paint_source(active).ok_or("Select a paint layer first")?;
            let target = document.scene().source_target(active).ok_or("Select a paint layer first")?;
            let offset = document.layer_offset(active);
            let base = paint.base.as_ref().filter(|base| coverage.is_none() && base.offset == [0; 2]
                && !source_edit::baked(paint) && occurrence.mask.is_none()
                && offset == [0; 2]
                && base.image.extent == document.composition().size);
            (SceneScope::Raw(target), occurrence.name.to_string(), base.map(|base| base.image.storage().clone()),
                base.map_or(PaintBasePolicy::WorkingPixels, |base| base.policy))
        };
        self.files.cut = cut.then(|| PendingCut {
            epoch: self.state.document_file.epoch,
            revision: self.engine.document().revision,
            layer: active.expect("cut requires an active paint layer"),
            target: document.working.target,
            objects: Default::default(),
            layers: Vec::new(),
        });
        Ok(self.pixel_capture(scene, scope, crop, coverage, original, policy, name, None))
    }

    #[expect(clippy::too_many_arguments, reason = "A clip capture keeps scope, region, coverage and source policy explicit")]
    fn pixel_capture(&self, scene: Arc<SceneSnapshot>, scope: SceneScope, crop: [u32; 4], coverage: Option<Arc<Selection>>,
        original: Option<Arc<SourceImage>>, policy: PaintBasePolicy, name: String, objects: Option<(Arc<ObjectClip>, [i64; 2])>) -> ClipboardCapture {
        let (objects, window) = objects.map_or((None, None), |(objects, origin)| (Some(objects), Some((origin, [crop[2], crop[3]]))));
        ClipboardCapture {
            scene, scope, crop, coverage, original, policy, name,
            large: u64::from(crop[2]) * u64::from(crop[3]) > LARGE_CLIP_PIXELS,
            origin: window.map_or([i64::from(crop[0]), i64::from(crop[1])], |(origin, _)| origin),
            window,
            objects,
            layers: None,
        }
    }

    fn copies_objects(&self, command: CommandId) -> bool {
        matches!(command, CommandId::Copy | CommandId::Cut) && self.object_target().is_some() && !self.engine.document().working.objects.is_empty()
    }

    fn copies_layers(&self, command: CommandId) -> bool {
        matches!(command, CommandId::Copy | CommandId::Cut) && !self.copies_objects(command)
            && self.engine.document().working.selection.is_none()
            && !matches!(self.engine.document().working.target, Some(SourceTarget::Coverage(_)))
    }

    fn clipboard_layer_roots(&self) -> Vec<OccurrenceHandle> {
        let doc = self.engine.document();
        let mut selected = doc.working.layer_selection.clone();
        if selected.is_empty() { selected.extend(doc.working.occurrence); }
        for id in selected.clone() { selected.extend(doc.scene().attached_effects(id)); }
        doc.layer_roots(&selected)
    }

    fn capture_layers(&mut self, cut: bool) -> Result<ClipboardCapture, String> {
        self.require_raster_snapshot()?;
        let roots = self.clipboard_layer_roots();
        let document = self.engine.document();
        let scene = self.engine.scene_snapshot();
        let layers = Arc::new(LayerClip { scene: scene.clone(), roots: roots.clone() });
        let mut members = document.layer_subtrees(&roots);
        let mut public = (*scene).clone();
        for id in &roots {
            let mut parent = document.scene().parent(*id);
            while let Some(id) = parent {
                if members.insert(id) {
                    let occurrence = public.artwork.occurrences.get_mut(id).ok_or("Missing group")?;
                    occurrence.opacity = 1.; occurrence.visible = true; occurrence.mask = None;
                    occurrence.blend = layer_core::LayerBlend::Normal; occurrence.attachment = layer_core::Attachment::None;
                }
                parent = document.scene().parent(id);
            }
        }
        for id in &roots {
            if document.scene().attachment_target(*id).is_some_and(|target| !members.contains(&target)) {
                public.artwork.occurrences.get_mut(*id).unwrap().attachment = layer_core::Attachment::None;
            }
        }
        public.index = Arc::new(layer_core::SceneIndex::build(&public.artwork)?);
        let scope = SceneScope::Members(members.into_iter().collect::<Vec<_>>().into());
        let (at, extent) = document.bake_window(public.view().with_scope(&scope)).map_err(|_| self.localization().text(MessageId::COMMANDS_COPY_PIXELS_TOO_LARGE).to_string())?;
        let origin = layer_core::offsets::rounded(at).ok_or("The copied layers exceed the editor's range")?;
        let name = document.scene().occurrence(document.working.occurrence.unwrap_or(roots[0])).unwrap().name.to_string();
        self.files.cut = cut.then(|| PendingCut {
            epoch: self.state.document_file.epoch, revision: document.revision,
            layer: document.working.occurrence.unwrap_or(roots[0]), target: document.working.target,
            objects: Default::default(), layers: document.layer_subtrees(&roots).into_iter().collect(),
        });
        let mut capture = self.pixel_capture(Arc::new(public), scope,
            [0, 0, extent[0], extent[1]], None, None, PaintBasePolicy::WorkingPixels, name, None);
        capture.origin = origin; capture.window = Some((origin, extent)); capture.layers = Some(layers);
        Ok(capture)
    }

    fn object_clip_window(&self) -> Result<([i64; 2], [u32; 2]), String> {
        let document = self.engine.document();
        let [min, max] = document.object_document_bounds(document.working.objects.iter().copied()).ok_or_else(|| self.localization().text(MessageId::OBJECTS_SELECT_IMAGES_FIRST).to_string())?;
        let origin = min.map(f64::floor);
        let extent = [0, 1].map(|axis| (max[axis].ceil() - origin[axis]).max(1.));
        if origin.iter().any(|v| v.abs() > 1e15) || extent.iter().any(|v| *v > f64::from(layer_core::MAX_EXTENT))
            || extent[0] * extent[1] > (layer_core::MAX_EXTENT as f64) * (layer_core::MAX_EXTENT as f64) / 4. {
            return Err(self.localization().text(MessageId::OBJECTS_COPY_TOO_LARGE).to_string());
        }
        Ok((origin.map(|v| v as i64), extent.map(|v| v as u32)))
    }

    fn capture_objects(&mut self, cut: bool) -> Result<ClipboardCapture, String> {
        self.require_raster_snapshot()?;
        let (origin, extent) = self.object_clip_window()?;
        let document = self.engine.document();
        let layer = document.working.occurrence.ok_or("Select an object layer")?;
        let selected = document.working.objects.clone();
        let children: Vec<_> = document.object_layer_children(layer).unwrap_or_default().iter().copied().filter(|h| selected.contains(h)).collect();
        let objects = children.iter().map(|h| {
            let mut object = document.scene().object(*h).ok_or("Unknown image")?.clone();
            object.affine = document.object_document_affine(*h).ok_or("Unknown image")?;
            Ok(object)
        }).collect::<Result<Vec<_>, String>>()?;
        let mut scene = (*self.engine.scene_snapshot()).clone();
        let OccurrenceContent::Objects(record) = scene.artwork.occurrences.get(layer).ok_or("Unknown layer")?.content else { return Err("Choose an object layer".into()); };
        scene.artwork.object_layers.get_mut(record).ok_or("Unknown layer")?.children = children;
        scene.index = Arc::new(layer_core::SceneIndex::build(&scene.artwork)?);
        let name = objects.first().map_or_else(String::new, |object| object.name.to_string());
        let name = if name.is_empty() { self.localization().text(MessageId::OBJECTS_UNNAMED_IMAGE).to_string() } else { name };
        self.files.cut = cut.then_some(PendingCut {
            epoch: self.state.document_file.epoch,
            revision: document.revision,
            layer,
            target: None,
            objects: selected,
            layers: Vec::new(),
        });
        Ok(self.pixel_capture(Arc::new(scene), SceneScope::RawObjects(layer), [0, 0, extent[0], extent[1]], None, None,
            PaintBasePolicy::WorkingPixels, name, Some((Arc::new(ObjectClip { objects }), origin))))
    }

    /// Erase what a successful Cut copied, unless the drawing changed while
    /// the host captured it.
    pub(super) fn finish_cut(&mut self, cut: PendingCut) {
        let document = self.engine.document();
        if cut.epoch != self.state.document_file.epoch
            || cut.revision != document.revision
            || Some(cut.layer) != document.working.occurrence
            || cut.target != document.working.target
        {
            self.notify("The drawing changed while cutting, so the pixels were copied but not erased");
        } else if !cut.layers.is_empty() {
            if let Err(error) = document.delete_layers_edit(&cut.layers).map_err(super::error).and_then(|edit| self.layer_edit(edit)) { self.notify(error); }
        } else if !cut.objects.is_empty() {
            if let Err(error) = document.delete_image_objects_edit(&cut.objects).map_err(super::error).and_then(|edit| self.layer_edit(edit)) { self.notify(error); }
        } else if let Err(error) = self.clear_selection(false) {
            self.notify(error);
        }
    }

    fn paste_center(&self, mode: PasteMode) -> [f64; 2] {
        if mode == PasteMode::Paste && self.state.settings.keymap.as_ref().is_some_and(|preset| preset.id == "photoshop") {
            return self.engine.document().composition().size.map(|v| f64::from(v) * 0.5);
        }
        if mode == PasteMode::AtCursor && let Some(event) = self.cursor.event {
            return self.pointer64([event.surface_position.x, event.surface_position.y]);
        }
        self.view_centre64()
    }

    fn pending_paste_center(&self, mode: PasteMode) -> [f64; 2] {
        let pending = self.state.requests.iter().any(|request| matches!(request.kind,
            HostRequestKind::Document { request: DocumentRequest::Paste { mode: requested } } if mode == requested));
        pending.then_some(self.files.paste_center).flatten().unwrap_or_else(|| self.paste_center(mode))
    }

    fn view_centred(&self, extent: [u32; 2]) -> Point {
        self.centred_clip(extent, PasteMode::AtView)
    }

    fn centred_clip(&self, extent: [u32; 2], mode: PasteMode) -> Point {
        let centre = self.pending_paste_center(mode);
        let [x, y] = std::array::from_fn(|axis| (centre[axis] - f64::from(extent[axis]) * 0.5).round() as f32);
        Point { x, y }
    }

    pub fn clip_position(&self, clip: &PixelClip, mode: PasteMode) -> Point {
        if matches!(mode, PasteMode::AtView | PasteMode::AtCursor)
            || mode == PasteMode::Paste && self.state.settings.keymap.as_ref().is_some_and(|preset| preset.id == "photoshop") {
            self.centred_clip(clip.source.extent, mode)
        } else { Point { x: clip.origin[0] as f32, y: clip.origin[1] as f32 } }
    }

    pub fn paste_clip(&mut self, clip: &PixelClip, mode: PasteMode) -> Result<(), String> {
        if mode == PasteMode::NewImage { return Err("Open the clipboard as a new drawing".into()); }
        refused(self.paste_refusal())?;
        let position = self.clip_position(clip, mode);
        if mode != PasteMode::Into && let Some(layers) = &clip.layers
            && clip.color == self.engine.document().composition().color {
            self.require_document_idle()?;
            let (index, parent) = self.image_layer_destination(None)?;
            let at = layer_core::offsets::rounded(position).ok_or("The pasted layers exceed the editor's range")?;
            let delta = layer_core::offsets::checked_sub(at, clip.origin).ok_or("The pasted layers exceed the editor's range")?;
            let (edit, _) = self.engine.document().import_layers_edit(&layers.scene, &layers.roots, parent, index, delta).map_err(error)?;
            self.source_edit_candidates(&edit, Default::default())?;
            self.layer_edit(edit)?;
            self.refresh_document(); self.refresh_commands();
            return Ok(());
        }
        if let Some(objects) = &clip.objects {
            let delta = [f64::from(position.x) - clip.origin[0] as f64, f64::from(position.y) - clip.origin[1] as f64];
            let objects = objects.objects.iter().map(|object| layer_core::ImageObject {
                affine: layer_core::Affine64([1., 0., 0., 1., delta[0], delta[1]]).compose(object.affine), ..object.clone()
            }).collect();
            return self.paste_objects(objects, None, mode == PasteMode::Into);
        }
        if mode == PasteMode::Into {
            return self.paste_objects(vec![placed(&clip.name, clip.source.clone().into(), [f64::from(position.x), f64::from(position.y)])], None, true);
        }
        let source = clip.source_for(self.engine.document().composition().color);
        self.insert_pasted(vec![(clip.name.clone(), source)], |_, _| position)
    }

    pub fn paste_layer_sources(
        &mut self,
        sources: Vec<(String, SourceImage)>,
        mode: PasteMode,
        context: &ImagePlacementContext,
    ) -> Result<(), String> {
        self.validate_image_placement(context)?;
        refused(self.paste_refusal())?;
        match mode {
            PasteMode::NewImage => Err("Open the clipboard as a new drawing".into()),
            PasteMode::Paste | PasteMode::AtView | PasteMode::AtCursor => {
                let [x, y] = self.pending_paste_center(mode).map(|v| v as f32);
                self.place_image_objects(sources, Some(Point { x, y }), context.destination, mode == PasteMode::Paste, false)
            }
            PasteMode::InPlace => {
                let objects = sources.into_iter().map(|(name, source)| placed(&name, Arc::new(source).into(), [0.; 2])).collect();
                self.paste_objects(objects, context.destination, false)
            }
            PasteMode::Into => {
                let objects = sources.into_iter().map(|(name, source)| {
                    let at = self.view_centred(source.extent);
                    placed(&name, layer_core::Image::new(Arc::new(source)), [f64::from(at.x), f64::from(at.y)])
                }).collect();
                self.paste_objects(objects, context.destination, true)
            }
        }
    }

    fn insert_pasted(
        &mut self,
        sources: Vec<(String, PaintBase)>,
        position: impl Fn(&Self, [u32; 2]) -> Point,
    ) -> Result<(), String> {
        self.require_document_idle()?;
        if self.operation.placing() {
            return Err("Apply or cancel the photo placement first".into());
        }
        if !self.engine.backend().supports_tiled_sources() {
            return Err("This renderer does not support tiled photo layers".into());
        }
        if sources.is_empty() {
            return Err("Copy an image to paste".into());
        }
        let (index, parent) = self.image_layer_destination(None)?;
        let document = self.engine.document();
        let parent_offset = parent.map_or([0; 2], |id| document.layer_offset(id));
        let containing = parent.and_then(|h| match document.scene().occurrence(h)?.content { OccurrenceContent::Stack(stack) => Some(stack), _ => None }).unwrap_or(document.composition().result);
        let mut artwork = document.artwork.clone();
        let mut stack = artwork.stacks.get(containing).ok_or("Missing destination group")?.clone();
        let mut edits = Vec::with_capacity(sources.len() * 3 + 2);
        let mut layers = Vec::new();
        for (offset, (name, source)) in sources.into_iter().enumerate() {
            let extent = source.image.extent;
            let domain = std::array::from_fn(|i| document.composition().size[i].max(extent[i]));
            source.validate(domain, document.composition().color)?;
            let at = position(self, extent);
            let paint = RecordChange::insert(&artwork.paint, PaintSource { color_mode: Default::default(),
                domain, raster: Default::default(), base: Some(source), operations: Arc::default(),
            });
            artwork.paint.change(paint.handle, paint.id, paint.value.clone())?;
            let mut occurrence = Occurrence::new(OccurrenceContent::Paint(paint.handle), layer_core::bounded_name(&name));
            occurrence.offset = layer_core::offsets::rounded(at).and_then(|at| layer_core::offsets::checked_sub(at, parent_offset)).ok_or("The pasted image exceeds the editor's range")?;
            let occurrence = RecordChange::insert(&artwork.occurrences, occurrence);
            artwork.occurrences.change(occurrence.handle, occurrence.id, occurrence.value.clone())?;
            stack.entries.insert(index + offset, occurrence.handle);
            layers.push(occurrence.handle);
            edits.extend([Edit::Paint(paint), Edit::Occurrence(occurrence)]);
        }
        edits.push(Edit::Stack(RecordChange::replace(&artwork.stacks, containing, Some(stack))?));
        let mut working = document.working.clone();
        working.occurrence = Some(layers[0]);
        working.layer_selection = layers.iter().copied().collect();
        working.layer_anchor = Some(layers[0]);
        working.target = match artwork.occurrences.get(layers[0]).unwrap().content { OccurrenceContent::Paint(h) => Some(SourceTarget::Paint(h)), _ => unreachable!() };
        working.inspect_mask = None;
        edits.push(Edit::Working(working));
        let edit = Edit::Batch(edits);
        self.source_edit_candidates(&edit, Default::default())?;
        self.layer_edit(edit)?;
        self.refresh_document();
        self.refresh_commands();
        self.layer_interaction.changed = true;
        Ok(())
    }

    fn paste_objects(&mut self, objects: Vec<layer_core::ImageObject>, destination: Option<ImageLayerDestination>, masked: bool) -> Result<(), String> {
        self.require_document_idle()?;
        if self.operation.placing() || self.objects.placing() { return Err(self.localization().text(MessageId::COMMANDS_APPLY_OR_CANCEL_THE_TRANSFORM_FIRST).to_string()); }
        if masked { refused(self.paste_into_refusal())?; }
        if !self.engine.backend().supports_tiled_sources() {
            return Err("This renderer does not support tiled photo layers".into());
        }
        let objects = if masked { self.centred_in_selection(objects)? } else { objects };
        let (edit, _, operations) = self.insert_objects_edit(objects, destination, masked)?;
        self.source_edit_candidates(&edit, Default::default())?;
        self.layer_edit_with(edit, operations)?;
        self.object_tool(LayerCanvasTool::Move)?;
        self.refresh_document();
        self.refresh_commands();
        self.layer_interaction.changed = true;
        Ok(())
    }

    fn centred_in_selection(&self, objects: Vec<layer_core::ImageObject>) -> Result<Vec<layer_core::ImageObject>, String> {
        let [x, y, width, height] = self.clipboard_crop()?.map(f64::from);
        let [min, max] = layer_core::placed_bounds(objects.iter().map(|object| (object.affine, object.image.extent))).ok_or("Copy an image to paste")?;
        let delta = [x + width * 0.5 - (min[0] + max[0]) * 0.5, y + height * 0.5 - (min[1] + max[1]) * 0.5].map(f64::round);
        Ok(objects.into_iter().map(|object| layer_core::ImageObject {
            affine: layer_core::Affine64([1., 0., 0., 1., delta[0], delta[1]]).compose(object.affine), ..object
        }).collect())
    }
}

fn placed(name: &str, image: layer_core::Image, at: [f64; 2]) -> layer_core::ImageObject {
    let mut object = layer_core::ImageObject::new(image, layer_core::bounded_name(name));
    object.affine = layer_core::Affine64([1., 0., 0., 1., at[0], at[1]]);
    object
}
