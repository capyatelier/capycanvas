//! Android owns brightness and tone mapping for the absolute BT.2100 PQ surface.
//! SDR and proof use the native 8-bit surface, in Display P3 on wide-gamut screens.
pub(crate) fn hdr_surface(caps: &wgpu::SurfaceCapabilities) -> bool {
    caps.color_spaces(wgpu::TextureFormat::Rgba16Float)
        .contains(wgpu::SurfaceColorSpaces::BT2100_PQ)
}

pub(crate) fn p3_surface(caps: &wgpu::SurfaceCapabilities, format: wgpu::TextureFormat) -> bool {
    caps.color_spaces(format).contains(wgpu::SurfaceColorSpaces::DISPLAY_P3)
}

#[cfg(test)]
mod tests {
    #[test]
    fn require_float_pq_surface() {
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
        assert!(super::hdr_surface(&caps));
    }

    #[test]
    fn p3_needs_the_sdr_format_to_offer_display_p3() {
        let mut caps=wgpu::SurfaceCapabilities::default();
        caps.format_capabilities=vec![
            wgpu::SurfaceFormatCapabilities {
                format:wgpu::TextureFormat::Rgba8UnormSrgb,
                color_spaces:wgpu::SurfaceColorSpaces::SRGB,
            },
            wgpu::SurfaceFormatCapabilities {
                format:wgpu::TextureFormat::Rgba16Float,
                color_spaces:wgpu::SurfaceColorSpaces::SRGB | wgpu::SurfaceColorSpaces::DISPLAY_P3,
            },
        ];
        assert!(!super::p3_surface(&caps, wgpu::TextureFormat::Rgba8UnormSrgb));
        caps.format_capabilities[0].color_spaces|=wgpu::SurfaceColorSpaces::DISPLAY_P3;
        assert!(super::p3_surface(&caps, wgpu::TextureFormat::Rgba8UnormSrgb));
    }
}
