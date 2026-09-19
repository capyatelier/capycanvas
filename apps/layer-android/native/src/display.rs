//! Android 15 extended-range surfaces use SDR-relative linear values and the
//! SurfaceView headroom request. Android owns the physical white/peak brightness.
pub(crate) fn hdr_surface(available: bool, caps: &wgpu::SurfaceCapabilities) -> bool {
    available && caps.color_spaces(wgpu::TextureFormat::Rgba16Float)
        .contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR)
}

#[cfg(test)]
mod tests {
    #[test]
    fn require_the_matching_format_and_color_space_and_host_metadata_path() {
        let mut caps=wgpu::SurfaceCapabilities::default();
        caps.format_capabilities=vec![wgpu::SurfaceFormatCapabilities {
            format:wgpu::TextureFormat::Rgba8Unorm,
            color_spaces:wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR,
        }];
        assert!(!super::hdr_surface(true,&caps));
        caps.format_capabilities[0].format=wgpu::TextureFormat::Rgba16Float;
        assert!(super::hdr_surface(true,&caps));
        assert!(!super::hdr_surface(false,&caps));
        caps.format_capabilities[0].color_spaces=wgpu::SurfaceColorSpaces::SRGB;
        assert!(!super::hdr_surface(true,&caps));
    }
}
