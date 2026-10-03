use super::*;
use crate::lut3d::Lut3d;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Target { layer: LayerId, key: Arc<str>, default: bool }

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding { target: Target, descriptor: Lut3d, payload: usize }

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload { offset: u64, bytes: u64, digest: [u8; 32] }

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceIndex { bindings: Vec<Binding>, payloads: Vec<Payload> }

impl ResourceIndex {
    pub fn is_empty(&self) -> bool { self.bindings.is_empty() }

    pub fn detach(document: &mut Document) -> Result<(Self, Vec<Arc<Lut3d>>), String> {
        let mut index = Self::default();
        let mut payloads = Vec::new();
        let mut ids = BTreeMap::new();
        for layer in &mut document.layers {
            let Some(effect) = &mut layer.effect else { continue; };
            let effect = Arc::make_mut(effect);
            if effect.values.len() != effect.program.parameters.len() { return Err("Mismatched effect values".into()); }
            let program = Arc::make_mut(&mut effect.program);
            for (parameter, value) in Arc::make_mut(&mut program.parameters).iter_mut().zip(&mut effect.values) {
                for (default, value) in [(false, value), (true, &mut parameter.default)] {
                    let EffectValue::Lut3d(resource) = value else { continue; };
                    let Some(resource) = resource.take() else { continue; };
                    let bytes = resource.storage().ok_or("Unresolved effect resource")?;
                    let payload = *ids.entry(resource.digest()).or_insert_with(|| {
                        let id = payloads.len(); payloads.push(resource.clone());
                        index.payloads.push(Payload { offset: 0, bytes: bytes.len() as u64, digest: resource.digest() }); id
                    });
                    index.bindings.push(Binding {
                        target: Target { layer: layer.id, key: parameter.key.clone(), default },
                        descriptor: (*resource).clone(), payload,
                    });
                }
            }
        }
        Ok((index, payloads))
    }

    pub(super) fn index(&mut self, offset: &mut u64) -> Result<(), String> {
        for payload in &mut self.payloads {
            payload.offset = *offset;
            *offset = offset.checked_add(payload.bytes).ok_or("Effect resource index overflow")?;
        }
        Ok(())
    }

    pub fn validate(&self, document: &Document, limit: u64) -> Result<u64, String> {
        if self.bindings.len() > document.layers.len().saturating_mul(2)
            || self.payloads.len() > self.bindings.len() {
            return Err("Oversized effect resource index".into());
        }
        let layers: BTreeMap<_, _> = document.layers.iter().map(|l| (l.id, l)).collect();
        let mut targets = BTreeSet::new();
        let mut used = BTreeSet::new();
        let mut descriptors: Vec<Option<&Lut3d>> = vec![None; self.payloads.len()];
        for binding in &self.bindings {
            let effect = layers.get(&binding.target.layer).and_then(|l| l.effect.as_deref()).ok_or("Missing effect resource owner")?;
            if effect.values.len() != effect.program.parameters.len() { return Err("Mismatched effect values".into()); }
            let i = effect.program.parameters.iter().position(|p| p.key == binding.target.key).ok_or("Missing effect resource parameter")?;
            let parameter = &effect.program.parameters[i];
            let value = if binding.target.default { &parameter.default } else { &effect.values[i] };
            let payload = self.payloads.get(binding.payload).ok_or("Missing effect resource payload")?;
            binding.descriptor.validate_descriptor()?;
            if !targets.insert(binding.target.clone())
                || parameter.kind != EffectParameterKind::Lut3d
                || !matches!(value, EffectValue::Lut3d(None))
                || payload.digest != binding.descriptor.digest()
                || payload.bytes != binding.descriptor.expected_bytes() as u64 {
                return Err("Invalid effect resource binding".into());
            }
            if let Some(other) = descriptors[binding.payload] {
                if other.size() != binding.descriptor.size()
                    || other.domain().iter().flatten().map(|v| v.to_bits()).ne(binding.descriptor.domain().iter().flatten().map(|v| v.to_bits())) {
                    return Err("Inconsistent effect resource descriptors".into());
                }
            } else { descriptors[binding.payload] = Some(&binding.descriptor); }
            used.insert(binding.payload);
        }
        if used.len() != self.payloads.len() { return Err("Unused effect resource payload".into()); }
        let mut bytes = 0u64;
        let mut digests = BTreeSet::new();
        for payload in &self.payloads {
            if !digests.insert(payload.digest) {
                return Err("Invalid effect resource chunk index".into());
            }
            bytes = bytes.checked_add(payload.bytes).filter(|b| *b <= limit).ok_or("Effect resources exceed the memory limit")?;
        }
        Ok(bytes)
    }

    pub(super) fn validate_offsets(&self, offset: &mut u64) -> Result<(), String> {
        for payload in &self.payloads {
            if payload.offset != *offset { return Err("Invalid effect resource chunk offset".into()); }
            *offset = offset.checked_add(payload.bytes).ok_or("Effect resource index overflow")?;
        }
        Ok(())
    }

    pub(super) fn read(&self, input: &mut impl Read, document: &mut Document) -> Result<(), String> {
        let mut resources = Vec::with_capacity(self.payloads.len());
        for (payload, descriptor) in self.payloads.iter().zip(self.descriptors()?) {
            let data = read_block(input, payload.bytes, payload.bytes)?;
            resources.push(descriptor.with_owned_payload(data.into())?);
        }
        self.attach(document, &resources)
    }

    pub fn descriptors(&self) -> Result<Vec<&Lut3d>, String> {
        let mut descriptors = vec![None; self.payloads.len()];
        for binding in &self.bindings {
            descriptors.get_mut(binding.payload).ok_or("Missing effect resource payload")?.get_or_insert(&binding.descriptor);
        }
        descriptors.into_iter().map(|value| value.ok_or_else(|| "Unused effect resource payload".into())).collect()
    }

    pub fn attach(&self, document: &mut Document, resources: &[Lut3d]) -> Result<(), String> {
        self.validate(document, ProjectLimits::default().asset_bytes)?;
        if resources.len() != self.payloads.len() { return Err("Mismatched effect resource payloads".into()); }
        let bound: Vec<_> = self.bindings.iter().map(|binding| binding.descriptor.with_shared_payload(&resources[binding.payload]).map(Arc::new))
            .collect::<Result<_, _>>()?;
        let owners: BTreeMap<_, _> = document.layers.iter().enumerate().map(|(i,l)| (l.id,i)).collect();
        for (binding, resource) in self.bindings.iter().zip(bound) {
            let layer = &mut document.layers[owners[&binding.target.layer]];
            let effect = Arc::make_mut(layer.effect.as_mut().ok_or("Missing effect resource owner")?);
            let i = effect.program.parameters.iter().position(|p| p.key == binding.target.key).ok_or("Missing effect resource parameter")?;
            if binding.target.default {
                Arc::make_mut(&mut Arc::make_mut(&mut effect.program).parameters)[i].default = EffectValue::Lut3d(Some(resource));
            } else { effect.values[i] = EffectValue::Lut3d(Some(resource)); }
        }
        Ok(())
    }
}
