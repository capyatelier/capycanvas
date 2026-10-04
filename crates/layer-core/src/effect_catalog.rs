//! Runtime custom-filter packages. Hosts supply bytes; shared code reserves
//! built-in identities and stages catalog updates without editing documents.
use crate::{EffectInstance, EffectProgram, EffectShader, EffectValue, EffectView};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

include!(concat!(env!("OUT_DIR"), "/filter_resources.rs"));

pub fn bundled_effect_catalog() -> &'static EffectCatalog {
    static CATALOG: std::sync::OnceLock<EffectCatalog> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        let read = |name: &str| {
            FILTER_RESOURCES
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, source)| Arc::from(*source))
                .ok_or_else(|| format!("Missing filter resource: {name}"))
        };
        EffectPackage::parse(&read("manifest.json").unwrap())
            .unwrap()
            .resolve(read)
            .expect("valid bundled effect resources")
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum ResourceLabel {
    Literal(Arc<str>),
    Message { message: Arc<str> },
}
impl ResourceLabel {
    pub fn valid(&self, limit: usize) -> bool {
        match self {
            Self::Literal(text) => !text.is_empty() && text.len() <= limit,
            Self::Message { message } => !message.is_empty() && message.len() <= 128
                && message.as_bytes()[0].is_ascii_lowercase()
                && message.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'),
        }
    }
}
impl From<&str> for ResourceLabel {
    fn from(text: &str) -> Self { Self::Literal(text.into()) }
}
impl From<String> for ResourceLabel {
    fn from(text: String) -> Self { Self::Literal(text.into()) }
}
impl From<Arc<str>> for ResourceLabel {
    fn from(text: Arc<str>) -> Self { Self::Literal(text) }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectCategory {
    pub id: Arc<str>,
    pub label: ResourceLabel,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectDefinition {
    pub program: Arc<EffectProgram>,
    pub category: Arc<str>,
    pub icon: Arc<str>,
    #[serde(default)]
    pub preview: BTreeMap<Arc<str>, EffectValue>,
}
impl EffectDefinition {
    pub fn id(&self) -> &str {
        &self.program.id
    }
    pub fn label(&self) -> &ResourceLabel {
        &self.program.label
    }
    pub fn program(&self) -> Arc<EffectProgram> {
        self.program.clone()
    }
    pub fn view<'a>(&'a self, values: &'a [EffectValue]) -> EffectView<'a> {
        EffectView::new(&self.program, values)
    }
    pub fn preview(&self) -> Result<EffectInstance, String> {
        let mut instance = EffectInstance::new(self.program.clone());
        for (key, value) in &self.preview {
            instance.set(key, value.clone()).map_err(str::to_string)?;
        }
        instance.validate().map_err(str::to_string)?;
        Ok(instance)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectPackage {
    pub format: u32,
    pub categories: Vec<EffectCategory>,
    pub filters: Vec<EffectDefinition>,
}
impl EffectPackage {
    pub fn parse(json: &str) -> Result<Self, String> {
        if json.len() > 16 * 1024 * 1024 {
            return Err("Filter manifest is too large".into());
        }
        let package: Self =
            serde_json::from_str(json).map_err(|e| format!("Invalid filter manifest: {e}"))?;
        if package.format != 2 || package.filters.len() > 1024 || package.categories.len() > 64 {
            return Err("Unsupported or oversized filter package".into());
        }
        package.module_names()?;
        Ok(package)
    }
    /// Manifest-local filenames only: a package cannot request arbitrary paths
    /// from a native host or fetch a different origin through its module list.
    pub fn module_names(&self) -> Result<Vec<Arc<str>>, String> {
        let mut names = Vec::new();
        for filter in &self.filters {
            for shader in std::iter::once(&filter.program.wgsl)
                .chain(filter.program.lookups.iter().map(|l| &l.wgsl))
            {
                if let EffectShader::Modules(modules) = shader {
                    if modules.is_empty() || modules.len() > 64 {
                        return Err("Invalid shader module list".into());
                    }
                    for name in modules.iter() {
                        if name.len() > 128
                            || !name.ends_with(".wgsl")
                            || !name.bytes().all(|c| {
                                c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.')
                            })
                            || name.starts_with('.')
                        {
                            return Err(format!("Invalid shader module filename: {name}"));
                        }
                        if !names.contains(name) {
                            names.push(name.clone());
                        }
                    }
                }
            }
        }
        if names.len() > 256 {
            return Err("Too many shader modules".into());
        }
        Ok(names)
    }
    pub fn resolve(
        mut self,
        mut read: impl FnMut(&str) -> Result<Arc<str>, String>,
    ) -> Result<EffectCatalog, String> {
        if self.format != 2 {
            return Err("Unsupported filter package format".into());
        }
        let mut modules = BTreeMap::new();
        let mut total = 0usize;
        for name in self.module_names()? {
            let code = read(&name)?;
            total = total
                .checked_add(code.len())
                .ok_or("Shader module size overflow")?;
            if code.len() > 1024 * 1024 || total > 16 * 1024 * 1024 {
                return Err("Shader modules exceed package limits".into());
            }
            modules.insert(name, crate::authored::Resource::from(code));
        }
        let resolve = |shader: &mut EffectShader| {
            if let EffectShader::Modules(names) = shader {
                *shader = EffectShader::Linked {
                    sources: names.iter().map(|name| modules[name].clone()).collect(),
                };
            }
        };
        for filter in &mut self.filters {
            let program = Arc::make_mut(&mut filter.program);
            resolve(&mut program.wgsl);
            for lookup in Arc::make_mut(&mut program.lookups) {
                resolve(&mut lookup.wgsl);
            }
        }
        let catalog = EffectCatalog {
            categories: self.categories,
            filters: self.filters,
        };
        catalog.validate()?;
        Ok(catalog)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectInstallMode {
    Add,
    Replace,
    /// Update custom IDs and add new ones. Omitted IDs stay.
    Merge,
}

#[derive(Clone, Debug, Default)]
pub struct EffectCatalog {
    categories: Vec<EffectCategory>,
    filters: Vec<EffectDefinition>,
}
impl EffectCatalog {
    pub fn categories(&self) -> &[EffectCategory] {
        &self.categories
    }
    pub fn filters(&self) -> &[EffectDefinition] {
        &self.filters
    }
    pub fn get(&self, id: &str) -> Option<&EffectDefinition> {
        self.filters.iter().find(|f| f.id() == id)
    }
    /// Build a candidate without mutating the published catalog. The host must
    /// also validate shader interfaces/device compilation before publication.
    pub fn stage(&self, package: EffectCatalog, mode: EffectInstallMode) -> Result<Self, String> {
        let mut candidate = self.clone();
        for category in package.categories {
            if let Some(builtin)=bundled_effect_catalog().categories().iter().find(|c|c.id==category.id) {
                if builtin!=&category {return Err("Reserved built-in filter category".into());}
            } else if !matches!(category.label,ResourceLabel::Literal(_)) {return Err("Custom categories require literal labels".into());}
            match candidate
                .categories
                .iter_mut()
                .find(|c| c.id == category.id)
            {
                Some(old) if *old == category => {}
                Some(old) if !matches!(mode, EffectInstallMode::Add) => *old = category,
                Some(_) => return Err("Conflicting filter category".into()),
                None => candidate.categories.push(category),
            }
        }
        for filter in package.filters {
            if let Some(builtin) = bundled_effect_catalog().get(filter.id()) {
                if matches!(mode,EffectInstallMode::Add) || filter != *builtin { return Err(format!("Reserved built-in filter ID: {}", filter.id())); }
                continue;
            }
            if !filter.program.literal_labels() { return Err("Custom filters require literal labels".into()); }
            if let Some(old) = candidate.filters.iter_mut().find(|f| f.id() == filter.id()) {
                if matches!(mode, EffectInstallMode::Add) {
                    return Err(format!("Filter ID already exists: {}", filter.id()));
                }
                *old = filter;
            } else if matches!(mode, EffectInstallMode::Replace) {
                return Err(format!("Cannot replace missing filter: {}", filter.id()));
            } else {
                candidate.filters.push(filter);
            }
        }
        candidate.validate()?;
        Ok(candidate)
    }
    fn validate(&self) -> Result<(), String> {
        if self.filters.len() > 1024 || self.categories.len() > 64 {
            return Err("Filter catalog exceeds resource limits".into());
        }
        fn id(value: &str) -> bool {
            !value.is_empty()
                && value.len() <= 128
                && value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.' | b':'))
        }
        let mut categories = HashSet::new();
        for c in &self.categories {
            if !id(&c.id) || !c.label.valid(160) || !categories.insert(&c.id)
            {
                return Err("Invalid or duplicate filter category".into());
            }
        }
        let mut filters = HashSet::new();
        for f in &self.filters {
            if !id(f.id())
                || !f.label().valid(160)
                || !id(&f.icon)
                || !categories.contains(&f.category)
                || !filters.insert(f.id())
            {
                return Err("Invalid or duplicate filter definition".into());
            }
            EffectInstance::new(f.program.clone())
                .validate()
                .map_err(str::to_string)?;
            f.preview()?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn custom_program(id: &str) -> Arc<EffectProgram> {
    fn literal(value: &mut serde_json::Value) {
        if let Some(message) = value.get("message").and_then(serde_json::Value::as_str) { *value = serde_json::Value::String(message.into()); }
        else { match value { serde_json::Value::Object(fields) => fields.values_mut().for_each(literal),
            serde_json::Value::Array(items) => items.iter_mut().for_each(literal), _ => {} } }
    }
    let mut value = serde_json::to_value(bundled_effect_catalog().get(id).unwrap().program()).unwrap();
    literal(&mut value);
    value["id"] = serde_json::json!(format!("custom_{id}"));
    let mut program:EffectProgram=serde_json::from_value(value).unwrap();
    let builtin=bundled_effect_catalog().get(id).unwrap().program();
    program.wgsl=builtin.wgsl.clone();program.lookups=builtin.lookups.clone();
    Arc::new(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn disk_catalog() -> EffectCatalog {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/filters");
        EffectPackage::parse(&std::fs::read_to_string(directory.join("manifest.json")).unwrap())
            .unwrap()
            .resolve(|name| {
                std::fs::read_to_string(directory.join(name))
                    .map(Arc::from)
                    .map_err(|e| e.to_string())
            })
            .unwrap()
    }
    #[test]
    fn catalog_dimensions_and_opaque_color_capabilities_are_explicit() {
        let mut opaque=0;
        for filter in bundled_effect_catalog().filters() {
            for parameter in filter.program.parameters.iter() {
                if let crate::EffectParameterKind::Number {unit,..}=&parameter.kind {
                    let expected=match unit.as_ref() {"deg"=>Some(crate::authored::Dimension::Angle),"s"=>Some(crate::authored::Dimension::Time),"px"=>Some(crate::authored::Dimension::SourcePixels),_=>None};
                    if let Some(dimension)=expected {assert_eq!(parameter.dimension,dimension,"{}.{}",filter.id(),parameter.key);}
                }
                if parameter.opaque {
                    opaque+=1;
                    let mut instance=EffectInstance::new(filter.program());
                    let EffectValue::Color(mut color)=parameter.default else {panic!("opaque requires color")};
                    assert_eq!(color.rgba[3],1.);
                    color.rgba[3]=0.2;
                    let index=instance.program.parameters.iter().position(|p|p.key==parameter.key).unwrap();
                    instance.values[index]=EffectValue::Color(color);
                    instance.validate().unwrap();
                    instance.set(&parameter.key,EffectValue::Color(color)).unwrap();
                    color.rgba[3]=1.;assert_eq!(instance.value(&parameter.key),Some(&EffectValue::Color(color)));
                }
            }
        }
        assert_eq!(opaque,9);
    }
    #[test]
    fn all_definitions_load_from_disk_and_share_module_storage() {
        let catalog = disk_catalog();
        assert_eq!(catalog.filters().len(), 52);
        assert_eq!(catalog.filters(), bundled_effect_catalog().filters());
        let mut shared = std::collections::HashMap::<crate::authored::Resource<str>,crate::authored::Resource<str>>::new();
        for filter in catalog.filters() {
            for module in filter.program.wgsl.sources().unwrap() {
                if let Some(existing) = shared.get(module) { assert!(Arc::ptr_eq(existing.storage(), module.storage())); assert_eq!(existing.id(), module.id()); }
                else { shared.insert(module.clone(),module.clone()); }
            }
            filter.preview().unwrap();
        }
        let instance = catalog.get("gaussian_blur").unwrap().preview().unwrap();
        let copy: EffectInstance =
            serde_json::from_str(&serde_json::to_string(&instance).unwrap()).unwrap();
        assert_eq!(
            copy, instance,
            "resolved programs remain self-contained in documents"
        );
    }
    #[test]
    fn explicit_add_replace_and_failed_staging_leave_catalog_untouched() {
        let original = disk_catalog();
        assert!(
            original
                .stage(disk_catalog(), EffectInstallMode::Add)
                .is_err()
        );
        let mut custom = disk_catalog();
        custom.filters.retain(|f| f.id() == "gaussian_blur");
        custom.filters[0].program=custom_program("gaussian_blur");
        let p = Arc::make_mut(&mut custom.filters[0].program);
        p.id = "user:custom".into();
        p.label = "Custom kernel".into();
        assert!(
            original
                .stage(custom.clone(), EffectInstallMode::Replace)
                .is_err()
        );
        let added = original
            .stage(custom.clone(), EffectInstallMode::Add)
            .unwrap();
        assert!(original.get("user:custom").is_none());
        assert_eq!(added.filters().len(), 53);
        Arc::make_mut(&mut custom.filters[0].program).label = "Updated kernel".into();
        let replaced = added
            .stage(custom.clone(), EffectInstallMode::Replace)
            .unwrap();
        assert_eq!(
            replaced.get("user:custom").unwrap().label(),
            &ResourceLabel::from("Updated kernel")
        );
        assert_eq!(added.get("user:custom").unwrap().label(), &ResourceLabel::from("Custom kernel"));
        Arc::make_mut(&mut custom.filters[0].program).abi = 999;
        assert!(added.stage(custom, EffectInstallMode::Replace).is_err());
        assert_eq!(
            added.get("user:custom").unwrap().program.abi,
            crate::EFFECT_ABI
        );
    }
    #[test]
    fn resource_catalog_merge_updates_and_adds_without_changing_the_fallback() {
        let original = disk_catalog();
        let mut resources = original.clone();
        let mut added = resources.get("gaussian_blur").unwrap().clone();
        added.program=custom_program("gaussian_blur");
        Arc::make_mut(&mut added.program).id = "user:new_kernel".into();
        resources.filters.push(added);
        let merged = original.stage(resources, EffectInstallMode::Merge).unwrap();
        assert_eq!(merged.filters().len(), 53);
        assert!(merged.get("user:new_kernel").is_some());
        assert_eq!(merged.filters()[0],original.filters()[0]);
        assert_eq!(original.filters().len(), 52);
        assert_ne!(original.filters()[0].label(), &ResourceLabel::from("Updated filter"));
    }
    #[test]
    fn builtin_ids_reject_package_overrides_in_every_install_mode() {
        let original=disk_catalog();
        let mut replacement=original.clone();
        replacement.filters.retain(|f|f.id()=="curves");
        Arc::make_mut(&mut replacement.filters[0].program).label="Custom curves".into();
        for mode in [EffectInstallMode::Add,EffectInstallMode::Replace,EffectInstallMode::Merge] {
            assert!(original.stage(replacement.clone(),mode).is_err());
        }
    }
    #[test]
    fn resource_message_references_are_explicit_and_structurally_bounded() {
        for value in [r#"{"message":"resources-filter-curves"}"#, r#""literal { text }""#] {
            let label: ResourceLabel = serde_json::from_str(value).unwrap();
            assert!(label.valid(256));
            assert_eq!(serde_json::to_string(&label).unwrap(), value);
        }
        assert!(serde_json::from_str::<ResourceLabel>(r#"{"message":"common-save","extra":true}"#).is_err());
        for message in ["", "Invalid key", "common-save.attribute", "-term"] {
            let label = ResourceLabel::Message { message: message.into() };
            assert!(!label.valid(256), "{message}");
        }
    }

    #[test]
    fn obsolete_and_unknown_package_formats_are_rejected_before_module_loading() {
        let manifest = FILTER_RESOURCES.iter().find(|(name, _)| *name == "manifest.json").unwrap().1;
        for format in [0, 1, 3] {
            let mut value: serde_json::Value = serde_json::from_str(manifest).unwrap();
            value["format"] = format.into();
            assert!(EffectPackage::parse(&serde_json::to_string(&value).unwrap()).is_err());
            let mut package = EffectPackage::parse(manifest).unwrap();
            package.format = format;
            assert!(package.resolve(|_| panic!("unsupported package must not request modules")).is_err());
        }
    }

    #[test]
    fn modules_cannot_escape_the_package_or_silently_go_missing() {
        let manifest = FILTER_RESOURCES
            .iter()
            .find(|(n, _)| *n == "manifest.json")
            .unwrap()
            .1;
        let mut package = EffectPackage::parse(manifest).unwrap();
        Arc::make_mut(&mut package.filters[0].program).wgsl =
            EffectShader::Modules(Arc::from([Arc::from("../private.wgsl")]));
        assert!(package.module_names().is_err());
        assert!(
            EffectPackage::parse(manifest)
                .unwrap()
                .resolve(|_| Err("missing".into()))
                .is_err()
        );
        let mut package = EffectPackage::parse(manifest).unwrap();
        package.filters.push(package.filters[0].clone());
        assert!(
            package
                .resolve(|name| Ok(FILTER_RESOURCES
                    .iter()
                    .find(|(key, _)| *key == name)
                    .unwrap()
                    .1
                    .into()))
                .is_err()
        );
    }
#[test]
fn gaussian_consumers_admit_extended_sigma_with_bounded_preparation_and_support() {
    use crate::{EffectParameterKind,EffectValue,NumericMapping};
    for id in ["gaussian_blur","unsharp_mask","high_pass","bloom","soft_focus","pencil"] {
        let mut effect=crate::bundled_effect_catalog().get(id).unwrap().preview().unwrap();
        let sigma=effect.program.parameters.iter().find(|p|p.key.as_ref()=="sigma").unwrap();
        assert!(matches!(sigma.kind,EffectParameterKind::Number{min:0.,max:85.,..}),"{id}");
        assert_eq!(sigma.soft_bounds,Some([0.,21.]),"{id}");
        assert_eq!(sigma.mapping,NumericMapping::Power{exponent:0.5},"{id}");
        let lookup=&effect.program.lookups[0];
        assert_eq!((lookup.values,lookup.workgroup_size,lookup.workgroups),(129,[256,1,1],[1,1,1]),"{id}");
        assert_eq!(lookup.dependencies.iter().map(|k|k.as_ref()).collect::<Vec<_>>(),["sigma"],"{id}");
        effect.set("sigma",EffectValue::Number(85.)).unwrap();
        let radii=effect.program.passes.iter().map(|pass|pass.sampling.radius((&effect).into()).unwrap()).collect::<Vec<_>>();
        assert_eq!(radii,[255,255],"{id}");assert_eq!(radii.iter().sum::<u32>(),510);
        let encoded=serde_json::to_vec(&effect).unwrap();let restored:crate::EffectInstance=serde_json::from_slice(&encoded).unwrap();
        assert_eq!(restored,effect);assert!(effect.set("sigma",EffectValue::Number(85.1)).is_err(),"{id}");
    }
}

}
