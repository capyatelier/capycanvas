//! Image Size: scale the whole image to new pixel dimensions, or change only
//! its print resolution, from a shared dialog model. Paint layers and masks
//! resample on the GPU; placed photos, selections, guides and effect
//! distances scale as metadata. Hosts only present the view.
use super::canvas_size::{CanvasSizeUnit, CanvasUnitChoice, number, set_size_unit, size_message};
use super::*;
use std::sync::Arc;
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
    pub fn localized_label(self, localization: &Localizer) -> Arc<str> {
        localization.text(match self {
            Self::Automatic => MessageId::RESOURCES_SIZE_RESAMPLE_AUTOMATIC,
            Self::Bicubic => MessageId::RESOURCES_SIZE_RESAMPLE_BICUBIC,
            Self::Lanczos => MessageId::RESOURCES_SIZE_RESAMPLE_LANCZOS,
            Self::Bilinear => MessageId::RESOURCES_SIZE_RESAMPLE_BILINEAR,
            Self::Nearest => MessageId::RESOURCES_SIZE_RESAMPLE_NEAREST,
        })
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
    pub label: Arc<str>,
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
    pub title: Arc<str>,
    pub apply_label: Arc<str>,
    pub cancel_label: Arc<str>,
    pub labels: [Arc<str>; 2],
    pub values: [f64; 2],
    pub numeric: [NumericControl; 2],
    pub unit: CanvasSizeUnit,
    pub units: Vec<CanvasUnitChoice>,
    pub resolution_label: Arc<str>,
    pub resolution: f64,
    pub resolution_numeric: NumericControl,
    pub constrain: bool,
    pub constrain_label: Arc<str>,
    pub resample: ImageResample,
    pub resample_label: Arc<str>,
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
    fn new(current: [u32; 2], resolution: Option<ImageResolution>, localization: &Localizer) -> Self {
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
                title: localization.text(MessageId::RESOURCES_SIZE_IMAGE_TITLE),
                apply_label: localization.text(MessageId::COMMON_APPLY),
                cancel_label: localization.text(MessageId::COMMON_CANCEL),
                labels: [localization.text(MessageId::RESOURCES_SIZE_WIDTH), localization.text(MessageId::RESOURCES_SIZE_HEIGHT)],
                values: [0.; 2],
                numeric: [number(1., 1., 0, "px"), number(1., 1., 0, "px")],
                unit: CanvasSizeUnit::Pixels,
                units: CanvasSizeUnit::ALL.map(|unit| CanvasUnitChoice { unit, label: unit.localized_label(localization) }).into(),
                resolution_label: localization.text(MessageId::RESOURCES_SIZE_RESOLUTION),
                resolution: ppi,
                resolution_numeric: number(1., 10_000., 0, "ppi"),
                constrain: true,
                constrain_label: localization.text(MessageId::RESOURCES_SIZE_CONSTRAIN),
                resample: ImageResample::Automatic,
                resample_label: localization.text(MessageId::RESOURCES_SIZE_RESAMPLE),
                resamples: ImageResample::ALL.map(|resample| ImageResampleChoice { resample, label: resample.localized_label(localization) }).into(),
                message: String::new(),
                can_apply: false,
            },
        }
    }

    pub(super) fn set_localization(&mut self, document: &Document, limits: layer_core::GeometryLimits, localization: &Localizer) {
        self.view = Self::new(self.current, document.composition().resolution, localization).view;
        self.update(document, limits, localization);
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

    /// Set one side; with Constrain proportions the other follows.
    fn set(&mut self, axis: usize, value: f64) {
        self.values[axis] = self.unit.round(value);
        if self.constrain {
            let other = 1 - axis;
            self.values[other] = match self.unit {
                CanvasSizeUnit::Percent => self.values[axis],
                CanvasSizeUnit::Pixels => self.unit.round(
                    self.values[axis] * f64::from(self.current[other]) / f64::from(self.current[axis]),
                ),
            };
        }
    }

    fn resolution_changed(&self) -> bool {
        self.resolution != self.ppi
    }

    fn update(&mut self, document: &Document, limits: layer_core::GeometryLimits, localization: &Localizer) {
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
                ({ let mut args = FluentArgs::new(); args.set("resolution", self.resolution.to_string()); localization.format(MessageId::RESOURCES_SIZE_RESOLUTION_ONLY, &args) }, true)
            }
            Some(size) if size == self.current => (size_message(localization, MessageId::RESOURCES_SIZE_CURRENT, [width, height]), false),
            Some(size) => {
                let geometry = CanvasGeometry::resize(self.current, size, self.resample.interpolation(self.current, size));
                match document.check_canvas_geometry(&geometry, limits) {
                    Ok(()) => (size_message(localization, MessageId::RESOURCES_SIZE_NEW, size), true),
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
        let mut draft = ImageSizeDraft::new(doc.composition().size, doc.composition().resolution, self.localization());
        draft.update(doc, self.engine.geometry_limits(), self.localization());
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
        let finite = |value: f64| if value.is_finite() { Ok(value) } else { Err(NumericError::FiniteNumber.message(&self.state.localization)) };
        match action {
            ImageSizeAction::Width { value } => draft.set(0, finite(value)?),
            ImageSizeAction::Height { value } => draft.set(1, finite(value)?),
            ImageSizeAction::Unit { unit } => set_size_unit(draft.current, &mut draft.values, &mut draft.unit, unit),
            ImageSizeAction::Resolution { value } => {
                draft.view.resolution_numeric.validate(finite(value)? as f32, draft.view.resolution_label.as_ref()).map_err(|reason| reason.message(&self.state.localization))?;
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
        draft.update(self.engine.document(), limits, &self.state.localization);
        self.refresh_tools();
        Ok(())
    }

    /// Scale the image and set its resolution in one undo step.
    fn apply_image_size(&mut self) -> Result<(), String> {
        refused(self.canvas_geometry_refusal())?;
        let draft = self.image_size.as_ref().ok_or("Image Size is not open")?;
        let doc = self.engine.document();
        if draft.current != doc.composition().size {
            self.image_size = None;
            self.refresh_tools();
            return Err("The canvas changed; open Image Size again".into());
        }
        let size = draft.size().ok_or_else(|| layer_core::CanvasGeometryError::Empty.to_string())?;
        let resolution = draft.resolution_changed().then(|| {
            let mut composition = doc.composition().clone();
            composition.size = size;
            composition.resolution = Some(ImageResolution::ppi(draft.resolution as u32));
            Edit::Composition(layer_core::authored::RecordChange::replace(
                &doc.artwork.compositions, doc.artwork.root, Some(composition),
            ).expect("admitted composition"))
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::test_support::{session, invoke};
    #[test]
    fn retained_image_size_status_survives_ordinary_publication() {
        let mut session = session(Platform::Gtk);
        invoke(&mut session, CommandId::ImageSize);
        let message = session.image_size.as_ref().unwrap().view.message.as_ptr();
        for size in [8., 17.] {
            session.dispatch(UiAction::SetBrushSize { value: size }).unwrap();
            session.dispatch(UiAction::SetZoom { zoom: size / 8. }).unwrap();
            session.set_viewport([640. + size, 480.], [640 + size as u32, 480]).unwrap();
            assert_eq!(session.image_size.as_ref().unwrap().view.message.as_ptr(), message);
        }
    }
}

#[cfg(test)]
mod localization_tests {
    use super::*;
    use crate::session::test_support::{session, invoke};

    #[test]
    fn image_size_language_refresh_preserves_resolution_resample_and_linked_values() {
        let mut session = session(Platform::Gtk);
        invoke(&mut session, CommandId::ImageSize);
        session.image_size_action(ImageSizeAction::Width { value: 117. }).unwrap();
        session.image_size_action(ImageSizeAction::Resolution { value: 240. }).unwrap();
        session.image_size_action(ImageSizeAction::Resample { resample: ImageResample::Lanczos }).unwrap();
        let before = session.image_size_view().unwrap();
        let checkpoint = session.engine.checkpoint();
        assert!(session.set_localization(Localizer::shared(UiLanguage::Japanese)));
        let after = session.image_size_view().unwrap();
        assert_ne!(before.title, after.title);
        assert_ne!(before.message, after.message);
        assert_eq!(after.values, before.values);
        assert_eq!(after.numeric, before.numeric);
        assert_eq!(after.resolution, before.resolution);
        assert_eq!(after.resample, before.resample);
        assert_eq!(after.constrain, before.constrain);
        assert_eq!(session.engine.checkpoint(), checkpoint);
    }
}
