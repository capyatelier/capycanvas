use super::*;

pub(super) type Pixels = (u32, u32, Vec<u8>);
type Generate = fn() -> Result<Pixels, GpuRasterError>;

// Recipes are cheap to enumerate. Procedural textures are generated only when
// selected by the startup dependency queue (or immediately by eager hosts).
pub(super) fn builtin_masks() -> [(&'static str, Generate); 10] {
    fn square(pixels: Vec<u8>) -> Result<Pixels, GpuRasterError> {
        Ok((PROCEDURAL_GRAIN_SIZE, PROCEDURAL_GRAIN_SIZE, pixels))
    }
    [
        (WHITE_MASK_ASSET, || Ok((1, 1, vec![255]))),
        (PENCIL_TEXTURE_ASSET, || {
            parse_ascii_pgm(include_bytes!("../../../assets/brushes/pencil-grain.pgm"))
                .ok_or(GpuRasterError::InvalidImage)
        }),
        (PAINTBRUSH_TEXTURE_ASSET, || {
            parse_ascii_pgm(include_bytes!("../../../assets/brushes/paint-bristles.pgm"))
                .ok_or(GpuRasterError::InvalidImage)
        }),
        (PAPER_GRAIN_TEXTURE_ASSET, || {
            square(procedural_paper_grain())
        }),
        (BRISTLE_GRAIN_TEXTURE_ASSET, || {
            square(procedural_bristle_grain())
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
