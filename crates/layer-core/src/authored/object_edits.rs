use super::{Affine64, ImageInterpolation, ImageObject, ImageObjectHandle, OccurrenceHandle, RecordChange};
use crate::{Document, DocumentError, Edit};
use std::collections::BTreeSet;

const INVALID: fn(&'static str) -> DocumentError = DocumentError::InvalidLayerOperation;

pub fn placed_bounds(placements: impl IntoIterator<Item = (Affine64, [u32; 2])>) -> Option<[[f64; 2]; 2]> {
    placements.into_iter().map(|(affine, extent)| affine.bounds(extent))
        .reduce(|a, b| [[a[0][0].min(b[0][0]), a[0][1].min(b[0][1])], [a[1][0].max(b[1][0]), a[1][1].max(b[1][1])]])
}

impl Document {
    pub fn selected_objects(&self) -> BTreeSet<ImageObjectHandle> {
        self.working.layer_selection.iter().copied().chain(self.working.occurrence)
            .filter_map(|h| self.scene().object_handle(h)).collect()
    }
    pub fn object_document_affine(&self, object: ImageObjectHandle) -> Option<Affine64> {
        let scene = self.scene();
        let owner = scene.object_owner(object)?;
        let [x, y] = scene.occurrence_offset64(owner);
        Some(Affine64([1., 0., 0., 1., x, y]).compose(scene.object(object)?.affine))
    }
    pub fn object_document_bounds(&self, objects: impl IntoIterator<Item = ImageObjectHandle>) -> Option<[[f64; 2]; 2]> {
        placed_bounds(objects.into_iter().filter_map(|h| Some((self.object_document_affine(h)?, self.scene().object(h)?.image.extent))))
    }
    pub fn objects_editable(&self, layer: OccurrenceHandle) -> bool {
        self.scene().object_layer(layer).is_some() && !self.is_locked(layer)
    }
    pub fn pick_image_object(&self, point: [f64; 2]) -> Option<(OccurrenceHandle, ImageObjectHandle)> {
        self.scene().order().iter().copied().find_map(|layer| self.pick_image_object_in(layer, point).map(|h| (layer, h)))
    }
    pub fn pick_image_object_in(&self, layer: OccurrenceHandle, point: [f64; 2]) -> Option<ImageObjectHandle> {
        if !self.layer_is_visible(layer) || self.is_locked(layer) { return None; }
        self.scene().object_handle(layer).filter(|h| self.object_contains(*h, point))
    }
    pub fn object_contains(&self, object: ImageObjectHandle, point: [f64; 2]) -> bool {
        let Some(value) = self.scene().object(object) else { return false; };
        self.object_document_affine(object).and_then(Affine64::inverse).is_some_and(|inverse| {
            let [x, y] = inverse.map(point);
            (0. ..f64::from(value.image.extent[0])).contains(&x) && (0. ..f64::from(value.image.extent[1])).contains(&y)
        })
    }
    fn replace_object(&self, object: ImageObjectHandle, change: impl FnOnce(&mut ImageObject), admit: impl FnOnce(&ImageObject) -> Result<(), String>) -> Result<Edit, DocumentError> {
        let owner = self.scene().object_owner(object).ok_or(INVALID("Choose an image object"))?;
        if self.is_locked(owner) { return Err(DocumentError::ProtectedOccurrence(owner)); }
        let mut value = self.artwork.objects.get(object).ok_or(INVALID("Choose an image object"))?.clone();
        change(&mut value);
        admit(&value).map_err(DocumentError::InvalidArtwork)?;
        Ok(Edit::ImageObject(RecordChange::replace(&self.artwork.objects, object, Some(value))?))
    }
    pub fn set_image_object_affines_edit(&self, changes: &[(ImageObjectHandle, Affine64)]) -> Result<Edit, DocumentError> {
        if changes.is_empty() { return Err(INVALID("Select image objects first")); }
        changes.iter().map(|(object, affine)| self.replace_object(*object, |value| value.affine = *affine, |value| value.admit_affine().map_err(String::from)))
            .collect::<Result<_, _>>().map(Edit::Batch)
    }
    pub fn set_image_objects_interpolation_edit(&self, objects: &BTreeSet<ImageObjectHandle>, interpolation: ImageInterpolation) -> Result<Edit, DocumentError> {
        if objects.is_empty() { return Err(INVALID("Select object layers first")); }
        objects.iter().map(|object| self.replace_object(*object, |value| value.interpolation = interpolation, ImageObject::validate)).collect::<Result<_, _>>().map(Edit::Batch)
    }
}

#[cfg(test)]
#[path = "object_edits_tests.rs"]
mod tests;
