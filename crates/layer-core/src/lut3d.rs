//! Immutable creative color tables; parsing, hashing and verification run on file workers.
use crate::color::{RgbSpace, rgb};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    size: u32,
    domain: [[f32; 3]; 2],
    title: Arc<str>,
    digest: [u8; 32],
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lut3d {
    #[serde(flatten)]
    descriptor: Descriptor,
    #[serde(skip)]
    payload: Option<Arc<[u8]>>,
    #[serde(skip)]
    spaces: u8,
}
impl PartialEq for Lut3d {
    fn eq(&self, other: &Self) -> bool { self.descriptor == other.descriptor }
}
impl std::fmt::Debug for Lut3d {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lut3d").field("descriptor", &self.descriptor)
            .field("ready", &self.payload.is_some()).field("bytes", &self.bytes()).finish()
    }
}
impl Lut3d {
    pub const MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;
    pub const MAX_SIZE: u32 = 65;
    pub const HEADER_BYTES: usize = 96;

    pub fn size(&self) -> u32 { self.descriptor.size }
    pub fn domain(&self) -> [[f32; 3]; 2] { self.descriptor.domain }
    pub fn title(&self) -> &str { &self.descriptor.title }
    pub fn digest(&self) -> [u8; 32] { self.descriptor.digest }
    pub fn payload(&self) -> Option<&[u8]> { self.payload.as_deref() }
    pub fn storage(&self) -> Option<&Arc<[u8]>> { self.payload.as_ref() }
    pub fn expected_bytes(&self) -> usize { Self::HEADER_BYTES + (self.size() as usize).pow(3) * 16 }
    pub fn samples(&self) -> Option<impl Iterator<Item = [f32; 3]> + '_> {
        self.payload.as_deref().map(|bytes| bytes[Self::HEADER_BYTES..].chunks_exact(16).map(|record| std::array::from_fn(|i| f32::from_le_bytes(record[i*4..i*4+4].try_into().unwrap()))))
    }
    pub fn bytes(&self) -> usize { self.payload.as_ref().map_or(0, |payload| payload.len()) }
    pub fn accepts(&self, space: RgbSpace) -> bool {
        self.payload.is_some() && self.spaces & (1 << RgbSpace::ALL.iter().position(|s| *s == space).unwrap()) != 0
    }
    pub fn validate_descriptor(&self) -> Result<(), &'static str> {
        let d = &self.descriptor;
        if !(2..=Self::MAX_SIZE).contains(&d.size) || d.title.chars().count() > 256
            || d.title.chars().any(char::is_control)
            || d.domain.iter().flatten().any(|v| !v.is_finite() || v.abs() > 1e37)
            || (0..3).any(|i| d.domain[0][i] >= d.domain[1][i]) {
            return Err("Invalid color lookup descriptor");
        }
        let tiny = f64::from(f32::MIN_POSITIVE);
        for axis in 0..3 {
            let lo = f64::from(d.domain[0][axis]); let hi = f64::from(d.domain[1][axis]);
            if lo < tiny && hi > -tiny {
                let loss = lo.max(-tiny).abs().max(hi.min(tiny).abs()) / (hi-lo) * f64::from(d.size-1);
                if loss > f64::from(f32::EPSILON) { return Err("Color lookup domain exceeds GPU precision"); }
            }
        }
        Ok(())
    }
    fn headers(&self) -> [[f32;4];6] {
        let domain = self.domain();
        let mut records = [[0.;4];6];
        records[0][0] = self.size() as f32;
        records[1][..3].copy_from_slice(&domain[0]); records[2][..3].copy_from_slice(&domain[1]);
        for axis in 0..3 {
            let lo = f64::from(domain[0][axis]); let hi = f64::from(domain[1][axis]);
            let exponent = (-(lo.abs().max(hi.abs()).log2().floor() as i32)).clamp(-126,126);
            let scale = 2f64.powi(exponent);
            records[3][axis] = exponent as f32;
            records[4][axis] = (lo*scale) as f32;
            records[5][axis] = (1./(hi*scale-lo*scale)) as f32;
        }
        records
    }
    pub fn from_samples(size: u32, domain: [[f32; 3]; 2], title: Arc<str>, samples: Arc<[[f32; 3]]>) -> Result<Self, &'static str> {
        let mut value = Self { descriptor: Descriptor { size, domain, title, digest: [0; 32] }, payload: None, spaces: 0 };
        value.validate_descriptor()?;
        if samples.len() != (size as usize).pow(3) {
            return Err("Invalid color lookup samples");
        }
        value.spaces = Self::space_bits(samples.iter().copied())?;
        let mut payload = Vec::with_capacity(value.expected_bytes());
        for header in value.headers() {
            for component in header { payload.extend(component.to_le_bytes()); }
        }
        for sample in samples.iter() { for component in sample.iter().copied().chain([0.]) { payload.extend(component.to_le_bytes()); } }
        drop(samples);
        value.descriptor.digest = Sha256::digest(&payload).into();
        value.payload = Some(payload.into());
        Ok(value)
    }
    fn space_bits(samples: impl Iterator<Item=[f32;3]>) -> Result<u8, &'static str> {
        let mut bounds = [[f32::INFINITY;3],[f32::NEG_INFINITY;3]];
        for sample in samples {
            for i in 0..3 {
                if !sample[i].is_finite() || sample[i].abs() > 1e37 { return Err("Invalid color lookup samples"); }
                bounds[0][i] = bounds[0][i].min(sample[i]);
                bounds[1][i] = bounds[1][i].max(sample[i]);
            }
        }
        let limit = f32::MAX * (1. - 2048. * f32::EPSILON);
        let admitted = |v:f32| v.is_finite() && v.abs() <= limit;
        let mut spaces = 0;
        for (index, space) in RgbSpace::ALL.into_iter().enumerate() {
            let transforms = RgbSpace::ALL.map(|destination| space.linear_transform(destination));
            let valid = (0..8).all(|corner| {
                let sample: [f32;3] = std::array::from_fn(|i| bounds[(corner >> i) & 1][i]);
                let linear = sample.map(|v| space.decode(f64::from(v)));
                linear.iter().all(|v| admitted(*v as f32)) && transforms.iter().all(|matrix| {
                    rgb::apply(*matrix, linear).iter().all(|v| admitted(*v as f32))
                        && matrix.iter().all(|row| {
                            let products: [f32; 3] = std::array::from_fn(|i| row[i] as f32 * linear[i] as f32);
                            products.iter().all(|v| admitted(*v))
                                && (0..3).all(|i| (0..3).all(|j| i == j || admitted(products[i] + products[j])))
                                && admitted((products[0] + products[1]) + products[2])
                                && admitted((products[0] + products[2]) + products[1])
                                && admitted((products[1] + products[2]) + products[0])
                        })
                })
            });
            if valid { spaces |= 1 << index; }
        }
        Ok(spaces)
    }
    pub fn parse_cube_named(bytes: &[u8], filename: &str) -> Result<Self, &'static str> {
        let mut resource = Self::parse_cube(bytes)?;
        if resource.title().is_empty() {
            resource.descriptor.title = filename.chars().filter(|c| !c.is_control()).take(256).collect::<String>().into();
        }
        Ok(resource)
    }
    pub fn parse_cube(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() > Self::MAX_TEXT_BYTES { return Err("Color lookup file is too large"); }
        let text = std::str::from_utf8(bytes).map_err(|_| "Invalid color lookup text")?;
        let mut size = None;
        let mut title = None;
        let mut domain = [None, None];
        let mut samples = Vec::new();
        let mut data = false;
        let number = |word: &str| -> Result<f32, &'static str> {
            let value = word.parse::<f64>().map_err(|_| "Invalid color lookup number")?;
            if !value.is_finite() || value.abs() > 1e37 { return Err("Color lookup number is out of range"); }
            Ok(value as f32)
        };
        for source in text.split('\n') {
            let source = source.strip_suffix('\r').unwrap_or(source);
            if source.len() > 4096 { return Err("Color lookup line is too long"); }
            let line = source.trim_matches([' ', '\t']);
            if line.is_empty() || line.starts_with('#') { continue; }
            let mut words = line.split_ascii_whitespace();
            let first = words.next().ok_or("Invalid color lookup line")?;
            if first == "TITLE" {
                if data || title.is_some() { return Err("Misplaced color lookup title"); }
                let value = line[first.len()..].trim_matches([' ', '\t']);
                let value = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')).ok_or("Invalid color lookup title")?;
                if value.contains('"') || value.chars().count() > 256 || value.chars().any(char::is_control) { return Err("Invalid color lookup title"); }
                title = Some(Arc::from(value));
                continue;
            }
            if !line.is_ascii() || line.bytes().any(|b| b.is_ascii_control() && b != b'\t') { return Err("Invalid color lookup syntax"); }
            match first {
                "LUT_3D_SIZE" => {
                    if data || size.is_some() { return Err("Duplicate or misplaced color lookup size"); }
                    let n: u32 = words.next().ok_or("Missing color lookup size")?.parse().map_err(|_| "Invalid color lookup size")?;
                    if !(2..=Self::MAX_SIZE).contains(&n) || words.next().is_some() { return Err("Invalid color lookup size"); }
                    samples.try_reserve_exact((n as usize).pow(3)).map_err(|_| "Color lookup allocation failed")?;
                    size = Some(n);
                }
                "DOMAIN_MIN" | "DOMAIN_MAX" => {
                    let index = usize::from(first == "DOMAIN_MAX");
                    if data || domain[index].is_some() { return Err("Duplicate or misplaced color lookup domain"); }
                    let mut point = [0.; 3];
                    for value in &mut point { *value = number(words.next().ok_or("Incomplete color lookup domain")?)?; }
                    if words.next().is_some() { return Err("Invalid color lookup domain"); }
                    domain[index] = Some(point);
                }
                _ => {
                    let n = size.ok_or("Color lookup samples precede its size")?;
                    if samples.len() >= (n as usize).pow(3) { return Err("Too many color lookup samples"); }
                    let sample = [number(first)?, number(words.next().ok_or("Incomplete color lookup sample")?)?, number(words.next().ok_or("Incomplete color lookup sample")?)?];
                    if words.next().is_some() { return Err("Invalid color lookup sample"); }
                    samples.push(sample);data = true;
                }
            }
        }
        Self::from_samples(size.ok_or("Missing color lookup size")?, [domain[0].unwrap_or([0.; 3]), domain[1].unwrap_or([1.; 3])], title.unwrap_or_else(|| Arc::from("")), samples.into())
    }
    pub fn with_payload(&self, bytes: &[u8]) -> Result<Self, &'static str> {
        self.with_owned_payload(Arc::from(bytes))
    }
    pub fn with_shared_payload(&self, donor: &Self) -> Result<Self, &'static str> {
        self.validate_descriptor()?;
        if self.size() != donor.size() || self.digest() != donor.digest()
            || self.domain().iter().flatten().zip(donor.domain().iter().flatten()).any(|(a,b)| a.to_bits() != b.to_bits()) {
            return Err("Color lookup descriptor does not match verified payload");
        }
        Ok(Self { descriptor: self.descriptor.clone(),
            payload: Some(donor.payload.clone().ok_or("Color lookup donor is not verified")?),
            spaces: donor.spaces })
    }
    pub fn admitted_spaces(&self) -> u8 { self.spaces }
    #[cfg(target_arch = "wasm32")]
    pub fn from_verified_worker(&self, payload: Arc<[u8]>, spaces: u8) -> Result<Self, &'static str> {
        self.validate_descriptor()?;
        if payload.len() != self.expected_bytes() || spaces == 0 || spaces & !15 != 0 {
            return Err("Invalid verified color lookup payload");
        }
        for (record, values) in self.headers().iter().enumerate() {
            for (i, value) in values.iter().enumerate() {
                if payload[record*16+i*4..record*16+i*4+4] != value.to_le_bytes() { return Err("Invalid verified color lookup header"); }
            }
        }
        Ok(Self {descriptor: self.descriptor.clone(), payload: Some(payload), spaces})
    }
    pub fn with_owned_payload(&self, payload: Arc<[u8]>) -> Result<Self, &'static str> {
        self.validate_descriptor()?;
        let bytes = payload.as_ref();
        if bytes.len() != self.expected_bytes() { return Err("Invalid color lookup payload length"); }
        let component = |record:usize,index:usize| f32::from_le_bytes(bytes[record*16+index*4..record*16+index*4+4].try_into().unwrap());
        if self.headers().iter().enumerate().any(|(record,values)| values.iter().enumerate().any(|(i,v)|component(record,i).to_bits()!=v.to_bits()))
            || (6..bytes.len()/16).any(|record| component(record,3).to_bits() != 0) {
            return Err("Invalid color lookup payload header or padding");
        }
        if self.digest() != <[u8;32]>::from(Sha256::digest(bytes)) { return Err("Color lookup integrity check failed"); }
        let samples = bytes[Self::HEADER_BYTES..].chunks_exact(16).map(|sample| std::array::from_fn(|i| f32::from_le_bytes(sample[i*4..i*4+4].try_into().unwrap())));
        Ok(Self { descriptor: self.descriptor.clone(), spaces: Self::space_bits(samples)?, payload: Some(payload) })
    }
}
