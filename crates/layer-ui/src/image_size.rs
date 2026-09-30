//! Image Size: scale the whole image to new pixel dimensions, or change only
//! its print resolution, from a shared dialog model. Paint layers and masks
//! resample on the GPU; placed photos, selections, guides and effect
//! distances scale as metadata. Hosts only present the view.
use super::canvas_size::{CanvasSizeUnit, CanvasUnitChoice, number};
use super::*;
use layer_core::Edit;
use layer_core::{CanvasGeometry, ImageResolution, Interpolation};

/// The resolution shown for a document that declares none.
const DEFAULT_PPI: f64 = 72.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageResample {
    #[default]
    Automatic,
    Bicubic,
    Lanczos,
    Bilinear,
    Nearest,
}
impl ImageResample {
    pub const ALL: [Self; 5] = [Self::Automatic, Self::Bicubic, Self::Lanczos, Self::Bilinear, Self::Nearest];
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Bicubic => "Bicubic",
            Self::Lanczos => "Lanczos",
            Self::Bilinear => "Bilinear",
            Self::Nearest => "Nearest neighbor",
        }
    }
    /// The filter for scaling `from` pixels to `to`. Automatic keeps detail
    /// sharp when reducing and smooth when enlarging.
    fn interpolation(self, from: [u32; 2], to: [u32; 2]) -> Interpolation {
        match self {
            Self::Automatic if to[0] < from[0] || to[1] < from[1] => Interpolation::Lanczos,
            Self::Automatic | Self::Bicubic => Interpolation::Bicubic,
            Self::Lanczos => Interpolation::Lanczos,
            Self::Bilinear => Interpolation::Linear,
            Self::Nearest => Interpolation::Nearest,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ImageResampleChoice {
    pub resample: ImageResample,
    pub label: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ImageSizeAction {
    Width { value: f64 },
    Height { value: f64 },
    Unit { unit: CanvasSizeUnit },
    Resolution { value: f64 },
    Constrain { constrain: bool },
    Resample { resample: ImageResample },
    Apply,
    Cancel,
}

/// The open Image Size dialog. Width and height are in `unit`; the
/// resolution is in pixels per inch.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ImageSizeView {
    pub title: &'static str,
    pub labels: [&'static str; 2],
    pub values: [f64; 2],
    pub numeric: [NumericControl; 2],
    pub unit: CanvasSizeUnit,
    pub units: Vec<CanvasUnitChoice>,
    pub resolution_label: &'static str,
    pub resolution: f64,
    pub resolution_numeric: NumericControl,
    pub constrain: bool,
    pub constrain_label: &'static str,
    pub resample: ImageResample,
    pub resample_label: &'static str,
    pub resamples: Vec<ImageResampleChoice>,
    /// The resulting size, or why it can't be applied.
    pub message: String,
    pub can_apply: bool,
}

pub(super) struct ImageSizeDraft {
    current: [u32; 2],
    ppi: f64,
    values: [f64; 2],
    unit: CanvasSizeUnit,
    resolution: f64,
    constrain: bool,
    resample: ImageResample,
    view: ImageSizeView,
}


impl ImageSizeDraft {
    fn new(current: [u32; 2], resolution: Option<ImageResolution>) -> Self {
        let ppi = resolution.map_or(DEFAULT_PPI, |r| r.pixels_per_inch()[0].round().max(1.));
        Self {
            current,
            ppi,
            values: current.map(f64::from),
            unit: CanvasSizeUnit::Pixels,
            resolution: ppi,
            constrain: true,
            resample: ImageResample::Automatic,
            view: ImageSizeView {
                title: "Image Size",
                labels: ["Width", "Height"],
                values: [0.; 2],
                numeric: [number(1., 1., 0, "px"), number(1., 1., 0, "px")],
                unit: CanvasSizeUnit::Pixels,
                units: CanvasSizeUnit::ALL.map(|unit| CanvasUnitChoice { unit, label: unit.label() }).into(),
                resolution_label: "Resolution",
                resolution: ppi,
                resolution_numeric: number(1., 10_000., 0, "ppi"),
                constrain: true,
                constrain_label: "Constrain proportions",
                resample: ImageResample::Automatic,
                resample_label: "Resample",
                resamples: ImageResample::ALL.map(|resample| ImageResampleChoice { resample, label: resample.label() }).into(),
                message: String::new(),
                can_apply: false,
            },
        }
    }

    fn pixels(&self, axis: usize) -> Option<u32> {
        let pixels = match self.unit {
            CanvasSizeUnit::Pixels => self.values[axis],
            CanvasSizeUnit::Percent => f64::from(self.current[axis]) * self.values[axis] / 100.,
        }
        .round();
        (pixels >= 1. && pixels <= f64::from(u32::MAX)).then_some(pixels as u32)
    }

    fn size(&self) -> Option<[u32; 2]> {
        Some([self.pixels(0)?, self.pixels(1)?])
    }

    fn round(&self, value: f64) -> f64 {
        match self.unit {
            CanvasSizeUnit::Pixels => value.round(),
            CanvasSizeUnit::Percent => (value * 100.).round() / 100.,
        }
    }

    /// Set one side; with Constrain proportions the other follows.
    fn set(&mut self, axis: usize, value: f64) {
        self.values[axis] = self.round(value);
        if self.constrain {
            let other = 1 - axis;
            self.values[other] = match self.unit {
                CanvasSizeUnit::Percent => self.values[axis],
                CanvasSizeUnit::Pixels => self.round(
                    self.values[axis] * f64::from(self.current[other]) / f64::from(self.current[axis]),
                ),
            };
        }
    }

    fn set_unit(&mut self, unit: CanvasSizeUnit) {
        if unit == self.unit {
            return;
        }
        for axis in 0..2 {
            let current = f64::from(self.current[axis]);
            self.values[axis] = match unit {
                CanvasSizeUnit::Percent => self.values[axis] / current * 100.,
                CanvasSizeUnit::Pixels => self.values[axis] * current / 100.,
            };
        }
        self.unit = unit;
        self.values = self.values.map(|v| self.round(v));
    }

    fn resolution_changed(&self) -> bool {
        self.resolution != self.ppi
    }

    fn update(&mut self, document: &Document, limits: layer_core::GeometryLimits) {
        let limit = f64::from(limits.canvas_dimension());
        self.view.numeric = std::array::from_fn(|axis| match self.unit {
            CanvasSizeUnit::Pixels => number(1., limit, 0, "px"),
            CanvasSizeUnit::Percent => number(0.01, limit * 100. / f64::from(self.current[axis]), 2, "%"),
        });
        self.view.values = self.values;
        self.view.unit = self.unit;
        self.view.resolution = self.resolution;
        self.view.constrain = self.constrain;
        self.view.resample = self.resample;
        let [width, height] = self.current;
        (self.view.message, self.view.can_apply) = match self.size() {
            None => (layer_core::CanvasGeometryError::Empty.to_string(), false),
            Some(size) if size == self.current && self.resolution_changed() => {
                (format!("Only the resolution changes, to {} ppi", self.resolution), true)
            }
            Some(size) if size == self.current => (format!("Current size: {width} × {height} px"), false),
            Some(size) => {
                let geometry = CanvasGeometry::resize(self.current, size, self.resample.interpolation(self.current, size));
                match document.check_canvas_geometry(&geometry, limits) {
                    Ok(()) => (format!("New size: {} × {} px", size[0], size[1]), true),
                    Err(error) => (error.to_string(), false),
                }
            }
        };
    }
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn image_size_view(&self) -> Option<ImageSizeView> {
        self.image_size.as_ref().map(|draft| draft.view.clone())
    }

    pub(super) fn open_image_size(&mut self) -> Result<(), String> {
        self.require_document_idle()?;
        refused(self.canvas_geometry_refusal())?;
        let doc = self.engine.document();
        let mut draft = ImageSizeDraft::new([doc.width, doc.height], doc.resolution);
        draft.update(doc, self.engine.geometry_limits());
        self.image_size = Some(draft);
        self.refresh_tools();
        Ok(())
    }

    pub(super) fn image_size_action(&mut self, action: ImageSizeAction) -> Result<(), String> {
        if action == ImageSizeAction::Cancel {
            self.image_size = None;
            self.refresh_tools();
            return Ok(());
        }
        let draft = self.image_size.as_mut().ok_or("Image Size is not open")?;
        let finite = |value: f64| if value.is_finite() { Ok(value) } else { Err("Enter a number") };
        match action {
            ImageSizeAction::Width { value } => draft.set(0, finite(value)?),
            ImageSizeAction::Height { value } => draft.set(1, finite(value)?),
            ImageSizeAction::Unit { unit } => draft.set_unit(unit),
            ImageSizeAction::Resolution { value } => {
                draft.view.resolution_numeric.validate(finite(value)? as f32, draft.view.resolution_label)?;
                draft.resolution = value.round();
            }
            ImageSizeAction::Constrain { constrain } => {
                draft.constrain = constrain;
                let width = draft.values[0];
                draft.set(0, width);
            }
            ImageSizeAction::Resample { resample } => draft.resample = resample,
            ImageSizeAction::Apply => return self.apply_image_size(),
            ImageSizeAction::Cancel => unreachable!(),
        }
        let limits = self.engine.geometry_limits();
        let draft = self.image_size.as_mut().unwrap();
        draft.update(self.engine.document(), limits);
        self.refresh_tools();
        Ok(())
    }

    /// Scale the image and set its resolution in one undo step.
    fn apply_image_size(&mut self) -> Result<(), String> {
        refused(self.canvas_geometry_refusal())?;
        let draft = self.image_size.as_ref().ok_or("Image Size is not open")?;
        let doc = self.engine.document();
        if draft.current != [doc.width, doc.height] {
            self.image_size = None;
            self.refresh_tools();
            return Err("The canvas changed; open Image Size again".into());
        }
        let size = draft.size().ok_or_else(|| layer_core::CanvasGeometryError::Empty.to_string())?;
        let resolution = draft
            .resolution_changed()
            .then(|| Edit::SetResolution(Some(ImageResolution::ppi(draft.resolution as u32))));
        if size != draft.current {
            let geometry = CanvasGeometry::resize(draft.current, size, draft.resample.interpolation(draft.current, size));
            self.apply_canvas_geometry(&geometry, resolution.into_iter().collect()).map_err(|e| e.to_string())?;
        } else if let Some(edit) = resolution {
            self.engine.apply_edit(edit).map_err(error)?;
        } else {
            return Err("Change the size or the resolution first".into());
        }
        self.image_size = None;
        self.refresh_tools();
        Ok(())
    }
}
