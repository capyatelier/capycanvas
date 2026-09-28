//! Whole-image commands: flips and quarter turns, and Trim, Reveal All and the
//! crop's Fit Content, which need the content's pixel-tight bounds. A bounds
//! scan decodes a few tiles on the UI thread and moves the rest to a worker,
//! or, without threads, to later frames. Each command is one undo step.
use super::crop::{CropFrame, CropRatio};
use super::*;
use layer_core::{
    Affine, CanvasGeometry, CanvasGeometryError, CanvasRect, ContentBoundsCache, ContentBoundsRequest, ContentScope, Edit,
    ImageOrientation, Point, Rect, ScanBudget,
};
use std::sync::Arc;

/// Tiles a bounds scan may decode on the UI thread in one go.
const UI_TILES: usize = 4;
/// Float noise in bounds must not add a pixel of canvas.
const TOLERANCE: f32 = 1e-3;

/// What a content bounds scan is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ContentUse {
    Trim,
    RevealAll,
    FitContent,
}
impl ContentUse {
    fn scope(self) -> ContentScope {
        match self {
            Self::Trim => ContentScope::Canvas,
            Self::FitContent => ContentScope::Visible,
            Self::RevealAll => ContentScope::All,
        }
    }
    fn command(self) -> CommandId {
        match self {
            Self::Trim => CommandId::Trim,
            Self::RevealAll => CommandId::RevealAll,
            Self::FitContent => CommandId::CropFitContent,
        }
    }
}

/// Where the rest of a scan runs: a worker thread, or, without threads, a few
/// tiles in each later frame.
enum ContentWork {
    #[cfg(not(target_arch = "wasm32"))]
    Worker(std::sync::mpsc::Receiver<Result<Option<Rect>, String>>),
    #[cfg(target_arch = "wasm32")]
    Frames(ContentBoundsRequest),
}

struct ContentJob {
    purpose: ContentUse,
    revision: u64,
    work: ContentWork,
}

/// The scan in flight, and decoded tiles kept by content for the next one.
#[derive(Default)]
pub(super) struct ContentBounds {
    cache: Arc<ContentBoundsCache>,
    job: Option<ContentJob>,
}
impl ContentBounds {
    pub(super) fn busy(&self) -> bool {
        self.job.is_some()
    }
}

pub(super) fn orientation(command: CommandId) -> Option<ImageOrientation> {
    Some(match command {
        CommandId::RotateImageLeft => ImageOrientation::RotateLeft,
        CommandId::RotateImageRight => ImageOrientation::RotateRight,
        CommandId::RotateImage180 => ImageOrientation::Rotate180,
        CommandId::FlipImageHorizontal => ImageOrientation::FlipHorizontal,
        CommandId::FlipImageVertical => ImageOrientation::FlipVertical,
        _ => return None,
    })
}

/// The whole pixels covering `bounds`.
fn pixel_rect(bounds: Rect) -> CanvasRect {
    let min = [bounds.min.x, bounds.min.y].map(|v| (v + TOLERANCE).floor());
    let max = [bounds.max.x, bounds.max.y].map(|v| (v - TOLERANCE).ceil());
    CanvasRect {
        origin: min.map(|v| v as i32),
        size: std::array::from_fn(|axis| (max[axis] - min[axis]).max(1.) as u32),
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    /// Move the canvas in one undo step, with `then` in the same step, and
    /// keep the image still on screen.
    pub(super) fn apply_canvas_geometry(&mut self, geometry: &CanvasGeometry, then: Vec<Edit>) -> Result<(), CanvasGeometryError> {
        if self.operation.placing() {
            return Err(CanvasGeometryError::Unsupported("Apply or cancel the photo placement first"));
        }
        self.engine.apply_canvas_geometry_with(geometry, then)?;
        let to_canvas = geometry.to_canvas();
        if let Some(reselect) = &mut self.selection_masks.reselect {
            *reselect = reselect.transformed(to_canvas)?;
        }
        self.follow_canvas_map(to_canvas);
        Ok(())
    }

    /// Keep the image where it was on screen after the canvas moves through
    /// `to_canvas`: exactly for a crop, and about the view's centre for a turn,
    /// flip or scale.
    fn follow_canvas_map(&mut self, to_canvas: Affine) {
        let camera = &mut self.state.camera;
        let [a, b, c, d, x, y] = to_canvas.0;
        if [a, b, c, d] == [1., 0., 0., 1.] {
            camera.follow_document_origin([-x, -y]);
        } else {
            let [cx, cy] = camera.work_area_center();
            let center = to_canvas.map(camera.input_transform().map(Point { x: cx, y: cy }));
            camera.center_on([center.x, center.y]);
        }
        self.sync_camera();
    }

    pub(super) fn orient_image(&mut self, orientation: ImageOrientation) -> Result<(), String> {
        self.require_document_idle()?;
        if let Some(reason) = self.canvas_geometry_refusal() {
            return Err(reason.into());
        }
        let doc = self.engine.document();
        let geometry = CanvasGeometry::orient([doc.width, doc.height], orientation);
        self.apply_canvas_geometry(&geometry, Vec::new()).map_err(|e| e.to_string())
    }

    /// Why a content bounds command can't run.
    pub(super) fn content_bounds_refusal(&self, purpose: ContentUse) -> Option<&'static str> {
        match purpose {
            ContentUse::FitContent => (!self.cropping()).then_some("Choose the Crop tool first"),
            ContentUse::Trim | ContentUse::RevealAll => self.canvas_geometry_refusal(),
        }
    }

    /// Fit Content edits the open crop; Trim and Reveal All edit the document.
    fn require_content_idle(&self, purpose: ContentUse) -> Result<(), String> {
        match purpose {
            ContentUse::FitContent => self.require_idle(),
            ContentUse::Trim | ContentUse::RevealAll => self.require_document_idle(),
        }
    }

    /// Find the content's bounds, then trim, reveal or fit to them. Small
    /// documents finish at once; larger ones finish on a later frame. Either
    /// way, a result that changes nothing is explained by a notice.
    pub(super) fn request_content_bounds(&mut self, purpose: ContentUse) -> Result<(), String> {
        self.require_content_idle(purpose)?;
        if let Some(reason) = self.content_bounds_refusal(purpose) {
            return Err(reason.into());
        }
        let doc = self.engine.document();
        let request = ContentBoundsRequest::new(doc, purpose.scope());
        let revision = doc.revision;
        let cache = self.content_bounds.cache.clone();
        if let Some(bounds) = request.scan(&cache, ScanBudget::Tiles(UI_TILES))? {
            self.content_bounds.job = None;
            if let Err(message) = self.use_content_bounds(purpose, bounds) {
                self.notify(message);
            }
            return Ok(());
        }
        #[cfg(not(target_arch = "wasm32"))]
        let work = {
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::Builder::new()
                .name("capy-content-bounds".into())
                .spawn(move || {
                    let _ = sender.send(request.scan(&cache, ScanBudget::Worker));
                })
                .map_err(|e| e.to_string())?;
            ContentWork::Worker(receiver)
        };
        #[cfg(target_arch = "wasm32")]
        let work = ContentWork::Frames(request);
        self.content_bounds.job = Some(ContentJob { purpose, revision, work });
        Ok(())
    }

    /// Finish a content bounds scan that has its result.
    pub(super) fn poll_content_bounds(&mut self) -> u32 {
        let Some(job) = &mut self.content_bounds.job else { return 0 };
        let result = match &mut job.work {
            #[cfg(not(target_arch = "wasm32"))]
            ContentWork::Worker(receiver) => match receiver.try_recv() {
                Ok(result) => result,
                Err(std::sync::mpsc::TryRecvError::Empty) => return 0,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("The content bounds scan stopped".into()),
            },
            #[cfg(target_arch = "wasm32")]
            ContentWork::Frames(request) => match request.scan(&self.content_bounds.cache, ScanBudget::Tiles(UI_TILES)) {
                Ok(None) => return 0,
                result => result,
            },
        };
        let job = self.content_bounds.job.take().unwrap();
        if job.purpose == ContentUse::FitContent && !self.cropping() {
            return 0;
        }
        let label = job.purpose.command().label();
        let outcome = if self.engine.document().revision != job.revision {
            Err(format!("{label} stopped because the drawing changed"))
        } else if let Some(reason) = self.content_bounds_refusal(job.purpose) {
            Err(reason.into())
        } else {
            self.require_content_idle(job.purpose)
                .and(result)
                .and_then(|bounds| bounds.ok_or_else(|| "The content bounds scan stopped".into()))
                .and_then(|bounds| self.use_content_bounds(job.purpose, bounds))
        };
        if let Err(message) = outcome {
            self.notify(message);
        }
        self.refresh_tools();
        regions::DOCUMENT | regions::BRUSH | regions::COMMANDS | regions::CAMERA | regions::HOST
    }

    fn use_content_bounds(&mut self, purpose: ContentUse, bounds: Rect) -> Result<(), String> {
        let doc = self.engine.document();
        let canvas = Rect { min: Point::default(), max: Point { x: doc.width as f32, y: doc.height as f32 } };
        let whole = CanvasRect { origin: [0; 2], size: [doc.width, doc.height] };
        match purpose {
            ContentUse::Trim => {
                if bounds.is_empty() {
                    return Err("There are no visible pixels to trim to".into());
                }
                let rect = pixel_rect(bounds);
                if rect == whole {
                    return Err("The visible pixels already reach every edge of the canvas".into());
                }
                self.apply_canvas_geometry(&CanvasGeometry::crop(rect), Vec::new()).map_err(|e| e.to_string())
            }
            ContentUse::RevealAll => {
                let rect = pixel_rect(bounds.union(canvas));
                if rect == whole {
                    return Err("Every pixel is already on the canvas".into());
                }
                self.apply_canvas_geometry(&CanvasGeometry::crop(rect), Vec::new()).map_err(|e| e.to_string())
            }
            ContentUse::FitContent => {
                if bounds.is_empty() {
                    return Err("There are no visible pixels to fit the crop to".into());
                }
                let rect = pixel_rect(bounds);
                self.fit_crop(rect)
            }
        }
    }

    /// Set the crop frame to `rect`, upright and with a free ratio.
    fn fit_crop(&mut self, rect: CanvasRect) -> Result<(), String> {
        let session = self.operation.crop.as_mut().ok_or("Choose the Crop tool first")?;
        let size = rect.size.map(|v| v as f32);
        session.set_frame(CropFrame {
            center: Point { x: rect.origin[0] as f32 + size[0] / 2., y: rect.origin[1] as f32 + size[1] / 2. },
            size,
            angle: 0.,
        });
        self.operation.crop_options.ratio = CropRatio::Free;
        self.layer_interaction.changed = true;
        Ok(())
    }
}
