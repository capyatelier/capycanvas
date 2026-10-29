use super::effect_records::{self, ResourceWriter};
use super::values::{DecodeError, DecodeResult};
use crate::{EffectProgram, Lut3d, authored::Resource};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, io::Write, sync::Arc};

#[derive(Clone, Debug, Default)]
pub struct TrustedPrograms(BTreeMap<[u8; 32], Arc<EffectProgram>>);
impl TrustedPrograms {
    pub fn from_approved(programs: impl IntoIterator<Item = Arc<EffectProgram>>) -> Result<Self, String> {
        let mut approved = Self::default();
        let mut owners = BTreeSet::new();
        for program in programs {
            if !owners.insert(Arc::as_ptr(&program) as usize) { continue; }
            let contract = contract(&program)?;
            if crate::bundled_effect_catalog().get(&program.id).is_none() {
                approved.0.insert(contract, program);
            }
        }
        Ok(approved)
    }
    pub(crate) fn from_roots(roots: &crate::RootInventory<'_>) -> Result<Self, String> {
        Self::from_approved(roots.programs.iter().map(|program| (*program).clone()))
    }
    pub(crate) fn resolve(&self, program: &Arc<EffectProgram>) -> DecodeResult<Arc<EffectProgram>> {
        self.0.get(&contract(program).map_err(DecodeError::from)?).cloned()
            .ok_or_else(unapproved)
    }
}

pub(crate) fn unapproved() -> DecodeError {
    DecodeError::Unsupported("Recovery contains a filter that is not approved in this session".into())
}

struct Contents;
impl ResourceWriter for Contents {
    fn code(&mut self, source: &Resource<str>) -> Result<Value, String> {
        Ok(json!(<[u8; 32]>::from(Sha256::digest(source.as_bytes()))))
    }
    fn lut(&mut self, lut: &Lut3d) -> Result<Value, String> {
        let payload = lut.payload().ok_or("Missing approved color lookup samples")?;
        Ok(json!({"size":lut.size(),"domain":lut.domain(),"title":lut.title(),
            "sha256":<[u8; 32]>::from(Sha256::digest(payload))}))
    }
    fn image(&mut self, _: &crate::authored::Image) -> Result<Value, String> {
        Err("Custom filters cannot contain image defaults".into())
    }
}
#[derive(Default)]
pub struct HashWriter(Sha256);
impl HashWriter {
    pub fn hex(self)->String {
        let mut hex=String::with_capacity(64);
        for byte in self.0.finalize() {std::fmt::Write::write_fmt(&mut hex,format_args!("{byte:02x}")).unwrap();}
        hex
    }
}
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes); Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}
fn contract(program: &Arc<EffectProgram>) -> Result<[u8; 32], String> {
    let value = effect_records::encode_program(program, &mut Contents)?;
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(&mut writer, &value).map_err(|error| error.to_string())?;
    Ok(writer.0.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EffectShader, EffectValue, effects::EffectParameterKind};

    #[test]
    fn custom_programs_require_independent_approval() {
        let program = crate::effect_catalog::custom_program("exposure");
        assert!(matches!(TrustedPrograms::default().resolve(&program), Err(DecodeError::Unsupported(_))));
        let approved = TrustedPrograms::from_approved([program.clone()]).unwrap();
        assert!(Arc::ptr_eq(&approved.resolve(&program).unwrap(), &program));
        let mut changed = (*program).clone();
        changed.entry = "different_entry".into();
        assert!(approved.resolve(&Arc::new(changed)).is_err());
    }
    #[test]
    fn historical_versions_are_distinct_and_code_identity_is_content() {
        let first = crate::effect_catalog::custom_program("exposure");
        let code = first.wgsl.sources().unwrap()[0].clone();
        let mut next = (*first).clone();
        let mut sources=first.wgsl.sources().unwrap().to_vec();
        sources[0]=format!("{}\n ", &*code).into();
        next.wgsl = EffectShader::Linked {sources:sources.into()};
        let next = Arc::new(next);
        let approved = TrustedPrograms::from_approved([first.clone(), next.clone()]).unwrap();
        assert!(Arc::ptr_eq(&approved.resolve(&first).unwrap(), &first));
        assert!(Arc::ptr_eq(&approved.resolve(&next).unwrap(), &next));
        let mut aliased = (*first).clone();
        let mut sources=first.wgsl.sources().unwrap().iter().map(|source| Resource::with_id(crate::PortableId::random(),source.storage().clone())).collect::<Vec<_>>();
        aliased.wgsl = EffectShader::Linked {sources:sources.clone().into()};
        assert!(Arc::ptr_eq(&approved.resolve(&Arc::new(aliased.clone())).unwrap(), &first));
        sources[0]=Resource::with_id(code.id(), Arc::from("different code"));
        aliased.wgsl = EffectShader::Linked {sources:sources.into()};
        assert!(approved.resolve(&Arc::new(aliased)).is_err());
    }
    #[test]
    fn trusted_ui_fields_are_retained_while_execution_contract_is_checked() {
        let original = crate::effect_catalog::custom_program("exposure");
        let approved = TrustedPrograms::from_approved([original.clone()]).unwrap();
        let mut normalized = (*original).clone();
        for parameter in Arc::make_mut(&mut normalized.parameters) {
            parameter.section = None; parameter.page = None; parameter.visible_when = None;
            parameter.soft_bounds = None; parameter.mapping = crate::effects::NumericMapping::Linear;
            if let EffectParameterKind::Number {step, decimals, ..} = &mut parameter.kind {
                *step = 0.01; *decimals = 6;
            }
        }
        assert!(Arc::ptr_eq(&approved.resolve(&Arc::new(normalized.clone())).unwrap(), &original));
        let parameter = &mut Arc::make_mut(&mut normalized.parameters)[0];
        parameter.default = EffectValue::Number(0.75);
        assert!(approved.resolve(&Arc::new(normalized)).is_err());
    }
    #[test]
    fn lookup_approval_hashes_samples_instead_of_declared_digest() {
        let mut program = (*crate::effect_catalog::custom_program("color_lookup")).clone();
        let first = Arc::new(Lut3d::from_samples(2, [[0.;3],[1.;3]], "Lookup".into(), vec![[0.25;3];8].into()).unwrap());
        let parameter = Arc::make_mut(&mut program.parameters).iter_mut().find(|parameter| matches!(parameter.kind, EffectParameterKind::Lut3d)).unwrap();
        parameter.default = EffectValue::Lut3d(Some(first.clone()));
        let original = Arc::new(program.clone());
        let approved = TrustedPrograms::from_approved([original.clone()]).unwrap();
        let second = Lut3d::from_samples(2, first.domain(), first.title().into(), vec![[0.75;3];8].into()).unwrap();
        let forged = Lut3d::from_verified_resource(first.size(), first.domain(), first.title().into(), first.digest(),
            second.resource().unwrap().clone(), first.admitted_spaces()).unwrap();
        assert_eq!(first.digest(), forged.digest());
        let parameter = Arc::make_mut(&mut program.parameters).iter_mut().find(|parameter| matches!(parameter.kind, EffectParameterKind::Lut3d)).unwrap();
        parameter.default = EffectValue::Lut3d(Some(Arc::new(forged)));
        assert!(approved.resolve(&Arc::new(program)).is_err());
    }
}
