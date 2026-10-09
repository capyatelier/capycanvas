use serde_json::Value;
use super::values::{DecodeError, DecodeResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordContext { Portable, Private }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordRole { Object, Resource }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordKind {
    Composition, Stack, Occurrence, PaintSource, Image, ImageObject,
    CoverageSource, Effect, Selection, Guides, Output, RasterTile, SelectionCoverage, Icc, Wgsl, PhotoMetadata, Lut3d,
}
pub struct RecordDescriptor {
    pub kind: RecordKind,
    pub type_id: &'static str,
    pub role: RecordRole,
    pub contexts: &'static [RecordContext],
    pub data_fields: &'static [&'static str],
    pub ignored_reference_fields: &'static [&'static str],
}
macro_rules! records {
    ($($kind:ident, $id:literal, $role:ident, [$($field:literal),*], [$($ignored:literal),*];)*) => {
        pub static RECORDS: &[RecordDescriptor] = &[$(RecordDescriptor {
            kind: RecordKind::$kind, type_id: $id, role: RecordRole::$role,
            contexts: &[RecordContext::Portable, RecordContext::Private],
            data_fields: &[$($field),*], ignored_reference_fields: &[$($ignored),*],
        }),*];
    };
}
records! {
    Composition, "capy.composition/2", Object, ["size", "result", "color", "blend", "resolution"], [];
    Stack, "capy.stack/1", Object, ["entries"], [];
    Occurrence, "capy.occurrence/3", Object, ["content", "name", "visible", "locked", "alpha_locked", "reference", "opacity", "blend", "attachment", "offset", "mask"], [];
    PaintSource, "capy.paint-source/2", Object, ["domain", "tiles", "material", "base", "color_mode"], [];
    Image, "capy.image/1", Object, ["extent", "interpretation", "tiles", "resolution"], [];
    ImageObject, "capy.image-object/1", Object, ["image", "affine", "interpolation"], [];
    CoverageSource, "capy.coverage-source/2", Object, ["domain", "tiles", "material", "default_coverage"], [];
    Effect, "capy.effect/2", Object, ["builtin", "version", "program", "values", "spatial"], [];
    Selection, "capy.selection/1", Object, ["shape", "affine", "inverted"], [];
    Guides, "capy.guides/1", Object, ["rulers"], [];
    Output, "capy.output/2", Object, ["source", "name", "context", "sdr", "proof", "representation"], ["representation"];
    RasterTile, "capy.raster-tile/1", Resource, ["channels", "depth", "transfer", "alpha", "profile"], [];
    SelectionCoverage, "capy.selection-coverage/1", Resource, ["depth", "extent", "bounds", "chunk"], [];
    Icc, "capy.icc/1", Resource, ["decoded_bytes"], [];
    Wgsl, "capy.wgsl/1", Resource, ["decoded_bytes"], [];
    PhotoMetadata, "capy.photo-metadata/1", Resource, ["kind", "decoded_bytes"], [];
    Lut3d, "capy.lut3d/1", Resource, ["size", "domain", "title", "decoded_bytes"], [];
}
pub fn descriptor(type_id: &str) -> Option<&'static RecordDescriptor> { RECORDS.iter().find(|record| record.type_id == type_id) }
impl RecordKind {
    pub fn descriptor(self) -> &'static RecordDescriptor { RECORDS.iter().find(|record| record.kind == self).unwrap() }
}
pub(crate) fn validate_program_context(context: RecordContext) -> DecodeResult<()> {
    if context == RecordContext::Private { Ok(()) }
    else { Err(DecodeError::Unsupported("Custom effects require private context".into())) }
}
pub fn validate_context(record: &Value, context: RecordContext) -> DecodeResult<()> {
    let Some(descriptor) = record.get("type").and_then(Value::as_str).and_then(descriptor) else {
        return Err(DecodeError::Unsupported("Unknown authored record type".into()));
    };
    if !descriptor.contexts.contains(&context) { return Err(DecodeError::Unsupported("Unsupported authored record context".into())); }
    if descriptor.kind == RecordKind::Effect {
        let data = record.get("data").and_then(Value::as_object).ok_or("Expected effect data")?;
        data.get("values").and_then(Value::as_object).ok_or("Expected keyed effect values")?;
        match (data.get("builtin"), data.get("version"), data.get("program")) {
            (Some(builtin), Some(version), None) => {
                builtin.as_str().filter(|name| !name.is_empty()).ok_or("Invalid built-in effect identity")?;
                version.as_u64().filter(|version| *version > 0 && *version <= u32::MAX as u64).ok_or("Invalid built-in effect version")?;
            },
            (None, None, Some(program)) => {
                program.as_object().ok_or("Expected private effect program")?;
                validate_program_context(context)?;
            },
            _ => return Err("Effect requires one built-in descriptor or private program".into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_adapter_has_one_registered_type_and_context_contract() {
        let mut names = BTreeSet::new();
        for record in RECORDS {
            assert!(names.insert(record.type_id));
            assert!(std::ptr::eq(record,record.kind.descriptor()));
            assert!(record.contexts.contains(&RecordContext::Portable));
            assert!(record.contexts.contains(&RecordContext::Private));
        }
        for removed in ["capy.paint-source/1","capy.effect/1","capy.effect-definition/1","capy.occurrence/2","capy.composition/1","capy.output/1","capy.coverage-source/1"] { assert!(descriptor(removed).is_none()); }
        assert_eq!(descriptor("capy.image/1").unwrap().role,RecordRole::Object);
        assert_eq!(descriptor("capy.output/2").unwrap().ignored_reference_fields,&["representation"]);
    }
}
