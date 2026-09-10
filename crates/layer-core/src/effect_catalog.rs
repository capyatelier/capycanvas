//! Runtime filter packages. Hosts supply bytes; no files, GPU types, built-in
//! switches or filter-specific constructors are involved in loading a catalog.
use crate::{EffectInstance, EffectProgram, EffectShader, EffectValue};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

include!(concat!(env!("OUT_DIR"), "/filter_resources.rs"));

/// Startup fallback only. Runtime packages use precisely the same parser and
/// resolver, without modifying or rebuilding this embedded resource copy.
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectCategory {
    pub id: Arc<str>,
    pub label: Arc<str>,
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
    pub fn label(&self) -> &str {
        &self.program.label
    }
    pub fn program(&self) -> Arc<EffectProgram> {
        self.program.clone()
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
        if package.format != 1 || package.filters.len() > 1024 || package.categories.len() > 64 {
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
        if self.format != 1 {
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
            modules.insert(name, code);
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
    /// Explicitly update known IDs and add new ones, e.g. a shipped resource
    /// catalog newer than the executable's startup fallback. Omitted IDs stay.
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
            if !id(&c.id) || c.label.is_empty() || c.label.len() > 160 || !categories.insert(&c.id)
            {
                return Err("Invalid or duplicate filter category".into());
            }
        }
        let mut filters = HashSet::new();
        for f in &self.filters {
            if !id(f.id())
                || f.label().is_empty()
                || f.label().len() > 160
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
    fn all_definitions_load_from_disk_and_share_module_storage() {
        let catalog = disk_catalog();
        assert_eq!(catalog.filters().len(), 40);
        assert_eq!(catalog.filters(), bundled_effect_catalog().filters());
        let common = &catalog
            .get("curves")
            .unwrap()
            .program
            .wgsl
            .sources()
            .unwrap()[0];
        for filter in catalog.filters() {
            assert!(Arc::ptr_eq(
                common,
                &filter.program.wgsl.sources().unwrap()[0]
            ));
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
        assert_eq!(added.filters().len(), 41);
        Arc::make_mut(&mut custom.filters[0].program).label = "Updated kernel".into();
        let replaced = added
            .stage(custom.clone(), EffectInstallMode::Replace)
            .unwrap();
        assert_eq!(
            replaced.get("user:custom").unwrap().label(),
            "Updated kernel"
        );
        assert_eq!(added.get("user:custom").unwrap().label(), "Custom kernel");
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
        Arc::make_mut(&mut added.program).id = "user:new_kernel".into();
        resources.filters.push(added);
        Arc::make_mut(&mut resources.filters[0].program).label = "Updated filter".into();
        let merged = original.stage(resources, EffectInstallMode::Merge).unwrap();
        assert_eq!(merged.filters().len(), 41);
        assert!(merged.get("user:new_kernel").is_some());
        assert_eq!(merged.filters()[0].label(), "Updated filter");
        assert_eq!(original.filters().len(), 40);
        assert_ne!(original.filters()[0].label(), "Updated filter");
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
}
