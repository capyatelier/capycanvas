//! Android owns brightness and tone mapping for the absolute BT.2100 PQ surface.
//! Proof uses the same float format with sRGB encoding, without activating HDR.
pub(crate) fn hdr_surface(caps: &wgpu::SurfaceCapabilities) -> bool {
    caps.color_spaces(wgpu::TextureFormat::Rgba16Float)
        .contains(wgpu::SurfaceColorSpaces::BT2100_PQ | wgpu::SurfaceColorSpaces::SRGB)
}

#[cfg(test)]
mod tests {
    #[test]
    fn require_matching_float_pq_and_sdr_formats() {
        let mut caps=wgpu::SurfaceCapabilities::default();
        caps.format_capabilities=vec![wgpu::SurfaceFormatCapabilities {
            format:wgpu::TextureFormat::Rgba8Unorm,
            color_spaces:wgpu::SurfaceColorSpaces::BT2100_PQ | wgpu::SurfaceColorSpaces::SRGB,
        }];
        assert!(!super::hdr_surface(&caps));
        caps.format_capabilities[0].format=wgpu::TextureFormat::Rgba16Float;
        assert!(super::hdr_surface(&caps));
        caps.format_capabilities[0].color_spaces=wgpu::SurfaceColorSpaces::SRGB;
        assert!(!super::hdr_surface(&caps));
        caps.format_capabilities[0].color_spaces=wgpu::SurfaceColorSpaces::BT2100_PQ;
        assert!(!super::hdr_surface(&caps));
    }
}
