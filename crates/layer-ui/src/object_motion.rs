use super::*;
use layer_core::Edit;
use layer_core::authored::{Affine64, ImageObjectHandle};

pub(super) struct ObjectMotion {
    owner: OccurrenceHandle,
    start: Vec<(ImageObjectHandle, Affine64)>,
    delta: Affine64,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn begin_object_motion(&mut self, objects: &[ImageObjectHandle]) -> Result<UiChange, String> {
        self.require_idle()?;
        self.start_object_motion(objects)?;
        Ok(self.changed(0, true))
    }
    pub(super) fn start_object_motion(&mut self, objects: &[ImageObjectHandle]) -> Result<(), String> {
        let scene = self.engine.document().scene();
        let owner = objects.first().and_then(|object| scene.object_owner(*object)).ok_or("Choose an image object")?;
        let start = objects.iter().map(|&object| match (scene.object_owner(object), scene.object(object)) {
            (Some(layer), Some(value)) if layer == owner => Ok((object, value.affine)),
            _ => Err("Move images within one object layer"),
        }).collect::<Result<Vec<_>, _>>()?;
        self.engine.document().set_image_object_affine_edit(start[0].0, start[0].1).map_err(error)?;
        self.engine.backend_mut().prepare_moving_layer(Some(owner));
        self.object_motion = Some(ObjectMotion { owner, start, delta: Affine64::default() });
        Ok(())
    }
    pub fn preview_object_motion(&mut self, delta: Affine64) -> Result<UiChange, String> {
        let edit = self.object_motion_edit(delta)?;
        self.engine.preview_edit(edit).map_err(error)?;
        self.object_motion.as_mut().unwrap().delta = delta;
        Ok(self.changed(0, true))
    }
    pub fn commit_object_motion(&mut self) -> Result<UiChange, String> {
        let delta = self.object_motion.as_ref().ok_or("Move an image first")?.delta;
        let restore = self.object_motion_edit(Affine64::default())?;
        let edit = self.object_motion_edit(delta)?;
        if delta != Affine64::default() { self.engine.preview_edit(restore).map_err(error)?; }
        self.end_object_motion();
        if delta != Affine64::default() { self.layer_edit(edit)?; }
        self.refresh_document();
        self.refresh_commands();
        Ok(self.changed(regions::DOCUMENT | regions::COMMANDS, true))
    }
    pub fn cancel_object_motion(&mut self) -> Result<UiChange, String> {
        let restore = self.object_motion_edit(Affine64::default())?;
        if self.object_motion.as_ref().is_some_and(|motion| motion.delta != Affine64::default()) { self.engine.preview_edit(restore).map_err(error)?; }
        self.end_object_motion();
        Ok(self.changed(0, true))
    }
    fn end_object_motion(&mut self) {
        self.object_motion = None;
        self.engine.backend_mut().prepare_moving_layer(None);
    }
    pub(super) fn object_motion_edit(&self, delta: Affine64) -> Result<Edit, String> {
        let motion = self.object_motion.as_ref().ok_or("Move an image first")?;
        delta.validate().map_err(error)?;
        let document = self.engine.document();
        let offset = document.scene().occurrence_offset64(motion.owner);
        let local = Affine64([1., 0., 0., 1., -offset[0], -offset[1]]).compose(delta).compose(Affine64([1., 0., 0., 1., offset[0], offset[1]]));
        let view = self.engine.view();
        let edits = motion.start.iter().map(|&(object, start)| {
            let affine = local.compose(start);
            if delta != Affine64::default() { self.engine.backend().preflight_image_object_affine(document.scene(), object, affine, view).map_err(error)?; }
            document.set_image_object_affine_edit(object, affine).map_err(error)
        }).collect::<Result<Vec<_>, String>>()?;
        Ok(if edits.len() == 1 { edits.into_iter().next().unwrap() } else { Edit::Batch(edits) })
    }
}
