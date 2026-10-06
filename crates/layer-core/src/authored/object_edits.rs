use super::{Affine64, ImageInterpolation, ImageObject, ImageObjectHandle, ObjectLayer, ObjectLayerHandle, OccurrenceContent, OccurrenceHandle, RecordChange, Store};
use crate::{Document, DocumentError, Edit};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectOrder { Front, Forward, Backward, Back }

const INVALID: fn(&'static str) -> DocumentError = DocumentError::InvalidLayerOperation;

fn insert<T: Clone>(store: &mut Store<T>, value: T) -> Result<RecordChange<T>, DocumentError> {
    let change = RecordChange::insert(store, value);
    store.change(change.handle, change.id, change.value.clone())?;
    Ok(change)
}

pub fn placed_bounds(placements: impl IntoIterator<Item = (Affine64, [u32; 2])>) -> Option<[[f64; 2]; 2]> {
    placements.into_iter().map(|(affine, extent)| affine.bounds(extent))
        .reduce(|a, b| [[a[0][0].min(b[0][0]), a[0][1].min(b[0][1])], [a[1][0].max(b[1][0]), a[1][1].max(b[1][1])]])
}

pub fn ordered_objects(children: &[ImageObjectHandle], selected: &BTreeSet<ImageObjectHandle>, order: ObjectOrder) -> Vec<ImageObjectHandle> {
    let (chosen, rest): (Vec<_>, Vec<_>) = children.iter().copied().partition(|h| selected.contains(h));
    match order {
        ObjectOrder::Front => chosen.into_iter().chain(rest).collect(),
        ObjectOrder::Back => rest.into_iter().chain(chosen).collect(),
        ObjectOrder::Forward | ObjectOrder::Backward => {
            let mut result = children.to_vec();
            let forward = order == ObjectOrder::Forward;
            let indices: Vec<usize> = if forward { (1..result.len()).collect() } else { (0..result.len().saturating_sub(1)).rev().collect() };
            for index in indices {
                let other = if forward { index - 1 } else { index + 1 };
                if selected.contains(&result[index]) && !selected.contains(&result[other]) { result.swap(index, other); }
            }
            result
        }
    }
}

impl Document {
    pub fn object_layer_children(&self, layer: OccurrenceHandle) -> Option<&[ImageObjectHandle]> {
        self.scene().object_layer(layer).map(|layer| layer.children.as_slice())
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
        self.scene().object_layer(layer)?.children.iter().copied().find(|h| self.object_contains(*h, point))
    }
    pub fn object_contains(&self, object: ImageObjectHandle, point: [f64; 2]) -> bool {
        let Some(value) = self.scene().object(object).filter(|object| object.visible) else { return false; };
        self.object_document_affine(object).and_then(Affine64::inverse).is_some_and(|inverse| {
            let [x, y] = inverse.map(point);
            (0. ..f64::from(value.image.extent[0])).contains(&x) && (0. ..f64::from(value.image.extent[1])).contains(&y)
        })
    }
    fn object_layer_record(&self, layer: OccurrenceHandle) -> Result<ObjectLayerHandle, DocumentError> {
        if self.is_locked(layer) { return Err(DocumentError::ProtectedOccurrence(layer)); }
        match self.scene().occurrence(layer).map(|o| &o.content) {
            Some(OccurrenceContent::Objects(handle)) => Ok(*handle),
            _ => Err(INVALID("Choose an object layer")),
        }
    }
    fn selected_owner(&self, objects: &BTreeSet<ImageObjectHandle>) -> Result<OccurrenceHandle, DocumentError> {
        let scene = self.scene();
        let mut owners = objects.iter().map(|h| scene.object_owner(*h));
        let owner = owners.next().flatten().ok_or(INVALID("Select image objects first"))?;
        if owners.any(|other| other != Some(owner)) { return Err(INVALID("Selected images belong to different layers")); }
        Ok(owner)
    }
    fn replace_children(&self, layer: OccurrenceHandle, children: Vec<ImageObjectHandle>) -> Result<Edit, DocumentError> {
        let record = self.object_layer_record(layer)?;
        Ok(Edit::ObjectLayer(RecordChange::replace(&self.artwork.object_layers, record, Some(ObjectLayer { children }))?))
    }
    fn replace_object(&self, object: ImageObjectHandle, change: impl FnOnce(&mut ImageObject), admit: impl FnOnce(&ImageObject) -> Result<(), String>) -> Result<Edit, DocumentError> {
        let owner = self.scene().object_owner(object).ok_or(INVALID("Choose an image object"))?;
        self.object_layer_record(owner)?;
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
    pub fn set_image_object_visible_edit(&self, object: ImageObjectHandle, visible: bool) -> Result<Edit, DocumentError> {
        self.replace_object(object, |value| value.visible = visible, ImageObject::validate)
    }
    pub fn rename_image_object_edit(&self, object: ImageObjectHandle, name: &str) -> Result<Edit, DocumentError> {
        let name = name.trim();
        if name.chars().count() > super::MAX_NAME_CHARS || name.chars().any(char::is_control) { return Err(INVALID("Use a name with up to 128 characters")); }
        self.replace_object(object, |value| value.name = Arc::from(name), ImageObject::validate)
    }
    pub fn set_image_objects_interpolation_edit(&self, objects: &BTreeSet<ImageObjectHandle>, interpolation: ImageInterpolation) -> Result<Edit, DocumentError> {
        self.selected_owner(objects)?;
        objects.iter().map(|object| self.replace_object(*object, |value| value.interpolation = interpolation, ImageObject::validate)).collect::<Result<_, _>>().map(Edit::Batch)
    }
    pub fn delete_image_objects_edit(&self, objects: &BTreeSet<ImageObjectHandle>) -> Result<Edit, DocumentError> {
        let owner = self.selected_owner(objects)?;
        let children = self.object_layer_children(owner).ok_or(INVALID("Choose an object layer"))?;
        let mut edits = vec![self.replace_children(owner, children.iter().copied().filter(|h| !objects.contains(h)).collect())?];
        for object in objects { edits.push(Edit::ImageObject(RecordChange::remove(&self.artwork.objects, *object)?)); }
        let mut working = self.working.clone();
        working.objects.retain(|h| !objects.contains(h));
        edits.push(Edit::Working(working));
        Ok(Edit::Batch(edits))
    }
    pub fn duplicate_image_objects_edit(&self, objects: &BTreeSet<ImageObjectHandle>) -> Result<(Vec<ImageObjectHandle>, Edit), DocumentError> {
        let owner = self.selected_owner(objects)?;
        self.object_layer_record(owner)?;
        let mut store = self.artwork.objects.clone();
        let mut children = Vec::new();
        let mut copies = Vec::new();
        let mut edits = Vec::new();
        for &child in self.object_layer_children(owner).ok_or(INVALID("Choose an object layer"))? {
            if objects.contains(&child) {
                let change = insert(&mut store, self.artwork.objects.get(child).ok_or(INVALID("Choose an image object"))?.clone())?;
                children.push(change.handle);
                copies.push(change.handle);
                edits.push(Edit::ImageObject(change));
            }
            children.push(child);
        }
        edits.push(self.replace_children(owner, children)?);
        let mut working = self.working.clone();
        working.objects = copies.iter().copied().collect();
        edits.push(Edit::Working(working));
        Ok((copies, Edit::Batch(edits)))
    }
    pub fn reorder_image_objects_edit(&self, objects: &BTreeSet<ImageObjectHandle>, order: ObjectOrder) -> Result<Edit, DocumentError> {
        let owner = self.selected_owner(objects)?;
        let children = self.object_layer_children(owner).ok_or(INVALID("Choose an object layer"))?;
        let reordered = ordered_objects(children, objects, order);
        if reordered == children { return Err(INVALID("The selected images are already in that position")); }
        self.replace_children(owner, reordered)
    }
    pub fn move_image_object_edit(&self, object: ImageObjectHandle, index: usize) -> Result<Edit, DocumentError> {
        let owner = self.scene().object_owner(object).ok_or(INVALID("Choose an image object"))?;
        let mut children = self.object_layer_children(owner).ok_or(INVALID("Choose an object layer"))?.to_vec();
        let from = children.iter().position(|h| *h == object).ok_or(INVALID("Choose an image object"))?;
        children.remove(from);
        children.insert(index.min(children.len()), object);
        self.replace_children(owner, children)
    }
}

#[cfg(test)]
#[path = "object_edits_tests.rs"]
mod tests;
