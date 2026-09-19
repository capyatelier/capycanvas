//! Android 15 extended-range surfaces use SDR-relative linear values and the
//! SurfaceView headroom request. Android owns the physical white/peak brightness.
pub(crate) fn requested_headroom(stops: f32) -> f32 {
    // The saved rendition endpoint is EV, Android's API is a linear ratio.
    stops.exp2().clamp(1., 100.)
}
/// Near-unity compositor feedback is not a useful HDR presentation. Switching
/// from the authored SDR appearance to the HDR shoulder for a 0.4% brightness
/// gain loses the rendition's highlight/gamut treatment without revealing HDR.
/// Keep the saved SDR appearance until Android grants at least 5% headroom.
pub(crate) fn presentation_headroom(reported: f32, requested: f32) -> f32 {
    let headroom = reported.min(requested);
    if headroom >= 1.05 { headroom } else { 1. }
}
pub(crate) fn hdr_surface(available: bool, caps: &wgpu::SurfaceCapabilities) -> bool {
    available && caps.color_spaces(wgpu::TextureFormat::Rgba16Float)
        .contains(wgpu::SurfaceColorSpaces::EXTENDED_SRGB_LINEAR)
}

#[cfg(test)]
mod tests {
    #[test]
    fn android_headroom_is_a_ratio_not_stops() {
        assert_eq!(super::requested_headroom(0.), 1.);
        assert_eq!(super::requested_headroom(1.), 2.);
        assert_eq!(super::requested_headroom(2.), 4.);
        assert_eq!(super::requested_headroom(16.), 100.);
    }
    #[test]
    fn near_sdr_feedback_retains_the_authored_sdr_appearance() {
        for reported in [1., 1.004, 1.049] {
            assert_eq!(super::presentation_headroom(reported, 4.), 1.);
        }
        assert_eq!(super::presentation_headroom(1.05, 4.), 1.05);
        assert_eq!(super::presentation_headroom(4., 2.), 2.);
        assert_eq!(super::presentation_headroom(4., 1.), 1.);
    }
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
