//! The pixel clipboard. Copy, Cut and Copy Merged are captured by a host
//! worker from an immutable snapshot; Paste, Paste in Place and Paste Into
//! add the pixels as one new layer in one undo step. Hosts keep the clip at
//! window level and write its PNG to the system clipboard beside a nonce, so
//! a paste of their own copy reads the full-depth source instead.
use super::*;
use layer_core::{
    Affine, Edit, Layer, LayerBlend, Point, Project, Rect, Selection,
    color::{
        DocumentColor,
        source::{SourceImage, SourceKind},
    },
};
use std::sync::Arc;

/// Copies larger than this show the import-style progress with Cancel.
pub const LARGE_CLIP_PIXELS: u64 = 1 << 21;
const NO_COVERAGE: MessageId = MessageId::COMMANDS_REFUSAL_CLIPBOARD_THE_SELECTION_DOESN_T_COVER_ANY_OF_THE_CANVAS;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PasteMode {
    /// A copy from Capy Canvas keeps its position when that is in view; an
    /// image from another app opens with placement handles.
    #[default]
    Paste,
    /// Always at the copied position, with no handles.
    InPlace,
    /// In place, with a mask from the selection.
    Into,
}

/// Pixels on the clipboard, held by the window, or on GTK the application.
#[derive(Clone)]
pub struct PixelClip {
    /// Written to the system clipboard beside the PNG. A clipboard that still
    /// carries it holds this clip.
    pub nonce: String,
    pub name: String,
    pub source: Arc<SourceImage>,
    /// Document pixels of the source's top-left corner where it was copied.
    pub origin: [u32; 2],
    pub color: DocumentColor,
    /// sRGB 8-bit rendition for other applications.
    pub png: Arc<[u8]>,
}
impl PixelClip {
    /// The source as a paste into a drawing of `color` holds it: document
    /// pixels when the colour settings match, otherwise an original image
    /// converted through its explicit profile.
    pub fn source_for(&self, color: DocumentColor) -> SourceImage {
        let mut source = (*self.source).clone();
        if color != self.color {
            source.kind = SourceKind::Original;
        }
        source
    }
}

/// A frozen copy request for the host's clip worker.
#[derive(Clone)]
pub struct ClipboardCapture {
    /// The active layer alone for Copy, the whole drawing for Copy Merged.
    pub project: Project,
    pub time: f32,
    /// `[x, y, width, height]` of the copied document pixels.
    pub crop: [u32; 4],
    /// Coverage the worker multiplies into the captured rows.
    pub coverage: Option<Arc<Selection>>,
    /// An untouched photo copied whole; the clip keeps its original samples.
    pub original: Option<Arc<SourceImage>>,
    pub name: String,
    /// Show the import-style progress with Cancel.
    pub large: bool,
}
impl ClipboardCapture {
    pub fn color(&self) -> DocumentColor {
        self.project.document.color
    }
    pub fn finish(self, nonce: String, source: Arc<SourceImage>, png: Vec<u8>) -> PixelClip {
        PixelClip {
            nonce,
            name: self.name,
            color: self.project.document.color,
            source,
            origin: [self.crop[0], self.crop[1]],
            png: png.into(),
        }
    }
}

/// The capture a pending Cut erases once the host reports success.
pub(super) struct PendingCut {
    epoch: u64,
    revision: u64,
    layer: LayerId,
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
        if !self.selection_meets_canvas() {
            return Some(l.text(MessageId::COMMANDS_REFUSAL_CLIPBOARD_THE_SELECTION_DOESN_T_COVER_ANY_OF_THE_CANVAS));
        }
        if command == CommandId::CopyMerged {
            return None;
        }
        if document.active_mask {
            return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST));
        }
        let layer = document.layer(document.active_layer)?;
        match layer.kind {
            LayerKind::Paint => {}
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

    pub(super) fn paste_into_refusal(&self) -> Option<std::sync::Arc<str>> {
        let l = self.localization();
        if self.selection_masks.target().is_some() {
            return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST));
        }
        self.engine.document().selection.is_none().then_some(l.text(MessageId::COMMANDS_REFUSAL_CLIPBOARD_MAKE_A_SELECTION_TO_PASTE_INTO))
    }

    /// Whether the selection's conservative bounds reach the canvas. Command
    /// states read this every frame, so it never scans pixel coverage.
    fn selection_meets_canvas(&self) -> bool {
        let document = self.engine.document();
        document.selection.as_ref().filter(|s| !s.inverted).is_none_or(|selection| {
            let bounds = selection.bounds();
            !bounds.is_empty()
                && bounds.max.x > 0.
                && bounds.max.y > 0.
                && bounds.min.x < document.width as f32
                && bounds.min.y < document.height as f32
        })
    }

    /// Document pixels the copy covers: the selection's bounds on the canvas,
    /// or the whole canvas.
    fn clipboard_crop(&self) -> Result<[u32; 4], String> {
        let document = self.engine.document();
        let canvas = [document.width, document.height];
        let Some(selection) = document.selection.as_ref().filter(|s| !s.inverted) else {
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
        self.clipboard_crop()?;
        self.request_document(DocumentRequest::Copy {
            merged: command == CommandId::CopyMerged,
            cut: command == CommandId::Cut,
        })
    }

    pub(super) fn request_paste(&mut self, mode: PasteMode) -> Result<(), String> {
        if mode == PasteMode::Into
            && let Some(reason) = self.paste_into_refusal()
        {
            return Err(reason.to_string());
        }
        self.request_document(DocumentRequest::Paste { mode })
    }

    /// Freeze a pending Copy, Cut or Copy Merged for the host's clip worker.
    /// Copy takes the active layer's own pixels, before its opacity, mask,
    /// blend mode and clipping; Copy Merged takes the visible composite.
    pub fn capture_clipboard(&mut self, id: u32) -> Result<ClipboardCapture, String> {
        let DocumentRequest::Copy { merged, cut } = *self.document_request(id)? else {
            return Err("This is not a copy request".into());
        };
        self.require_document_idle()?;
        let command = match (merged, cut) {
            (true, _) => CommandId::CopyMerged,
            (_, true) => CommandId::Cut,
            _ => CommandId::Copy,
        };
        refused(self.copy_refusal(command))?;
        let crop = self.clipboard_crop()?;
        let selection = self.engine.document().selection.clone();
        let canvas = [self.engine.document().width, self.engine.document().height];
        let coverage = selection.filter(|s| !selects_everything(s, canvas)).map(Arc::new);
        let mut project = self.capture_project_recovery()?;
        let document = &mut project.document;
        let active = document.active_layer;
        let (name, original) = if merged {
            ("Merged copy".to_string(), None)
        } else {
            let offset = document.layer_offset(active);
            let mut layer = document.layer(active).ok_or("Select a layer first")?.clone();
            let original = layer
                .source
                .clone()
                .filter(|source| {
                    coverage.is_none()
                        && !source_edit::baked(&layer)
                        && layer.mask.is_none()
                        && offset == Point::default()
                        && layer.properties.placement.as_affine() == Some(Affine::IDENTITY)
                        && source.extent == [document.width, document.height]
                });
            layer.visible = true;
            layer.opacity = 1.;
            if let Some(mask) = &mut layer.mask { mask.offset = document.layer_offset(mask.id); }
            layer.properties.parent = None;
            layer.properties.offset = offset;
            layer.properties.clipped = false;
            layer.properties.blend = LayerBlend::Normal;
            layer.properties.locked = false;
            layer.properties.alpha_locked = false;
            let name = layer.name.to_string();
            document.layers = vec![layer];
            document.active_mask = false;
            document.selection = None;
            document.reference_layers.clear();
            document.rulers.clear();
            (name, original)
        };
        let pixels = u64::from(crop[2]) * u64::from(crop[3]);
        self.files.cut = cut.then(|| PendingCut {
            epoch: self.state.document_file.epoch,
            revision: self.engine.document().revision,
            layer: active,
        });
        Ok(ClipboardCapture {
            project,
            time: self.engine.animation_time(),
            crop,
            coverage,
            original,
            name,
            large: pixels > LARGE_CLIP_PIXELS,
        })
    }

    /// Erase what a successful Cut copied, unless the drawing changed while
    /// the host captured it.
    pub(super) fn finish_cut(&mut self, cut: PendingCut) {
        let document = self.engine.document();
        if cut.epoch != self.state.document_file.epoch
            || cut.revision != document.revision
            || cut.layer != document.active_layer
        {
            self.notify("The drawing changed while cutting, so the pixels were copied but not erased");
        } else if let Err(error) = self.clear_selection(false) {
            self.notify(error);
        }
    }

    /// The visible canvas area, in document pixels.
    fn visible_canvas(&self) -> Rect {
        let camera = &self.state.camera;
        let [x, y, width, height] = camera.work_area;
        let view = camera.input_transform();
        let visible = Rect::around(
            Rect { min: Point { x, y }, max: Point { x: x + width, y: y + height } }
                .corners()
                .map(|p| view.map(p)),
        );
        let document = self.engine.document();
        Rect {
            min: Point { x: visible.min.x.max(0.), y: visible.min.y.max(0.) },
            max: Point {
                x: visible.max.x.min(document.width as f32),
                y: visible.max.y.min(document.height as f32),
            },
        }
    }

    /// The top-left corner that centres `extent` in the view, on whole pixels.
    fn view_centred(&self, extent: [u32; 2]) -> Point {
        let camera = &self.state.camera;
        let [x, y] = camera.work_area_center();
        let centre = camera.input_transform().map(Point { x, y });
        Point {
            x: (centre.x - extent[0] as f32 * 0.5).round(),
            y: (centre.y - extent[1] as f32 * 0.5).round(),
        }
    }

    /// Where a clip lands: its copied position, or for Paste the view centre
    /// when that position is out of view.
    pub fn clip_position(&self, clip: &PixelClip, mode: PasteMode) -> Point {
        let origin = Point { x: clip.origin[0] as f32, y: clip.origin[1] as f32 };
        let [width, height] = clip.source.extent.map(|v| v as f32);
        let visible = self.visible_canvas();
        let shown = origin.x < visible.max.x
            && origin.y < visible.max.y
            && origin.x + width > visible.min.x
            && origin.y + height > visible.min.y;
        if mode == PasteMode::Paste && !shown { self.view_centred(clip.source.extent) } else { origin }
    }

    /// Paste the window's clip as one new layer, with no placement handles.
    pub fn paste_clip(&mut self, clip: &PixelClip, mode: PasteMode) -> Result<(), String> {
        let position = self.clip_position(clip, mode);
        let source = clip.source_for(self.engine.document().color);
        self.insert_pasted(vec![(clip.name.clone(), source)], |_, _| position, mode == PasteMode::Into)
    }

    /// Paste images from another application. Paste opens the placement
    /// handles; Paste in Place and Paste Into centre them in the view at full size.
    pub fn paste_layer_sources(
        &mut self,
        sources: Vec<(String, SourceImage)>,
        mode: PasteMode,
        context: &ImagePlacementContext,
    ) -> Result<(), String> {
        self.validate_image_placement(context)?;
        if mode == PasteMode::Paste {
            return self.place_layer_sources(sources, context.center, context.destination);
        }
        self.insert_pasted(sources, |s, extent| s.view_centred(extent), mode == PasteMode::Into)
    }

    fn insert_pasted(
        &mut self,
        sources: Vec<(String, SourceImage)>,
        position: impl Fn(&Self, [u32; 2]) -> Point,
        masked: bool,
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
        if masked && let Some(reason) = self.paste_into_refusal() {
            return Err(reason.to_string());
        }
        let (index, parent) = self.image_layer_destination(None)?;
        let document = self.engine.document();
        let parent_offset = parent.map_or(Point::default(), |id| document.layer_offset(id));
        let mut probe = document.clone();
        let mut edits = Vec::with_capacity(sources.len() + 2);
        let mut ids = Vec::new();
        let mut layers = Vec::new();
        for (offset, (name, source)) in sources.into_iter().enumerate() {
            source.validate()?;
            let at = position(self, source.extent);
            let id = probe.allocate_layer_id();
            ids.push(id);
            let mut layer = Layer::paint(id, name.trim());
            layer.properties.parent = parent;
            layer.properties.placement =
                layer_core::LayerPlacement::from_affine(Affine::translation(Point { x: at.x - parent_offset.x, y: at.y - parent_offset.y }));
            layer.source = Some(Arc::new(source));
            if masked {
                let mask = probe.allocate_layer_id();
                ids.push(mask);
                layer.mask = Some(self.selection_mask_with_id(&layer, false, mask)?);
            }
            layers.push(id);
            edits.push(Edit::InsertLayer { index: index + offset, layer: Box::new(layer) });
        }
        edits.push(Edit::SetActiveLayer { id: layers[0] });
        if masked {
            edits.push(Edit::SetSelection(None));
        }
        let edit = Edit::Batch(edits);
        self.source_edit_candidates(&edit, &ids, Default::default())?;
        for id in &ids {
            let allocated = self.engine.allocate_layer_id();
            debug_assert_eq!(allocated, *id);
        }
        self.layer_edit(edit)?;
        self.layer_interaction.selected = layers.into_iter().collect();
        self.refresh_document();
        self.refresh_commands();
        self.layer_interaction.changed = true;
        Ok(())
    }
}
