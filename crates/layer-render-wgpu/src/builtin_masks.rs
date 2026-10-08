use super::*;

pub(super) type Pixels = (u32, u32, Vec<u8>);
type Generate = fn() -> Result<Pixels, GpuRasterError>;

type PreparedPixels = std::sync::Mutex<Option<Result<Pixels, GpuRasterError>>>;
struct Mask {
    id: AssetId,
    pixels: Deferred<PreparedPixels>,
    priority: u8,
    uploaded: bool,
}
pub(super) struct Masks(Vec<Mask>);
impl Masks {
    pub fn new() -> Self {
        Self(
            builtin_masks()
                .into_iter()
                .filter(|(id, _)| *id != WHITE_MASK_ASSET)
                .map(|(id, generate)| Mask {
                    id: AssetId::from(id),
                    pixels: Deferred::new(move || std::sync::Mutex::new(Some(generate()))),
                    priority: u8::MAX,
                    uploaded: false,
                })
                .collect(),
        )
    }
    pub fn style(
        &mut self,
        compiler: &startup::Compiler,
        style: &layer_render::DabStyle,
        priority: u8,
    ) {
        let key = WgpuRasterizer::texture_set_key(style);
        for id in [key.primary, key.grain, key.transport] {
            if let Some(mask) = self.0.iter_mut().find(|m| m.id == id) {
                mask.priority = mask.priority.min(priority);
                compiler.pipeline(&mask.pixels, priority);
            }
        }
    }
    pub fn ready_through(&self, priority: u8) -> bool {
        self.0.iter().all(|m| m.priority > priority || m.uploaded)
    }
    pub fn take_ready(&mut self, allow_optional: bool) -> Result<Vec<(AssetId, Pixels)>, GpuRasterError> {
        let mut ready = Vec::new();
        let mut optional = allow_optional;
        for mask in &mut self.0 {
            if !mask.uploaded
                && (mask.priority < startup::OTHER || optional)
                && mask.pixels.ready()
                && let Some(result) = mask.pixels.compile().lock().unwrap().take()
            {
                ready.push((mask.id.clone(), result?));
                mask.uploaded = true;
                if mask.priority >= startup::OTHER { optional = false; }
            }
        }
        Ok(ready)
    }
}

// Recipes are cheap to enumerate. Procedural textures are generated only when
// selected by the startup dependency queue (or immediately by eager hosts).
pub(super) fn builtin_masks() -> [(&'static str, Generate); 10] {
    fn square(pixels: Vec<u8>) -> Result<Pixels, GpuRasterError> {
        Ok((PROCEDURAL_GRAIN_SIZE, PROCEDURAL_GRAIN_SIZE, pixels))
    }
    [
        (WHITE_MASK_ASSET, || Ok((1, 1, vec![255]))),
        (bristle_table::HAIRS_ASSET, || Ok(bristle_table::hairs())),
        (bristle_table::FIELD_ASSET, || Ok(bristle_table::field())),
        (PAPER_GRAIN_TEXTURE_ASSET, || {
            square(procedural_paper_grain())
        }),
        (layer_core::CONTACT_PAPER_TEXTURE_ASSET, || {
            Ok((1024, 1024, procedural_contact_paper()))
        }),
        (WATERCOLOR_TIP_TEXTURE_ASSET, || {
            square(procedural_watercolor_tip())
        }),
        (WATERCOLOR_TRANSPORT_LONG_NARROW_ASSET, || {
            square(procedural_transport_field(TransportFieldKind::LongNarrow))
        }),
        (WATERCOLOR_TRANSPORT_LONG_BROAD_ASSET, || {
            square(procedural_transport_field(TransportFieldKind::LongBroad))
        }),
        (WATERCOLOR_TRANSPORT_SHORT_NARROW_ASSET, || {
            square(procedural_transport_field(TransportFieldKind::ShortNarrow))
        }),
        (WATERCOLOR_TRANSPORT_SHORT_BROAD_ASSET, || {
            square(procedural_transport_field(TransportFieldKind::ShortBroad))
        }),
    ]
}

#[cfg(test)]
#[test]
fn generated_pixels_do_not_signal_readiness_before_upload() {
    let mut masks = Masks(vec![Mask {
        id: AssetId::from("test:tip"),
        pixels: Deferred::new(|| std::sync::Mutex::new(Some(Ok((1, 1, vec![255]))))),
        priority: 3,
        uploaded: false,
    }]);
    assert!(masks.ready_through(2));
    masks.0[0].pixels.compile();
    assert!(!masks.ready_through(3));
    assert_eq!(masks.take_ready(false).unwrap().len(), 1);
    assert!(masks.ready_through(3));
    assert!(masks.take_ready(false).unwrap().is_empty());
}

#[cfg(test)]
#[test]
fn optional_mask_uploads_yield_to_input_and_upload_one_per_poll() {
    let mut masks = Masks((0..2).map(|i| Mask {
        id: AssetId::from(format!("test:tip-{i}").as_str()),
        pixels: Deferred::new(|| std::sync::Mutex::new(Some(Ok((1, 1, vec![255]))))),
        priority: startup::OTHER,
        uploaded: false,
    }).collect());
    for mask in &masks.0 { mask.pixels.compile(); }
    assert!(masks.take_ready(false).unwrap().is_empty());
    assert_eq!(masks.take_ready(true).unwrap().len(), 1);
    assert!(!masks.ready_through(startup::OTHER));
    assert_eq!(masks.take_ready(true).unwrap().len(), 1);
    assert!(masks.ready_through(startup::OTHER));
}
