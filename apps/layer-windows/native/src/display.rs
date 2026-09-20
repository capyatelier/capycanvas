//! Native output discovery only. Shared Rust owns display conversion and tone mapping.
use layer_render_wgpu::SdrSurfaceColor;
use serde::Serialize;
use windows::{
    Win32::{
        Foundation::HWND,
        Graphics::{
            Dxgi::{
                Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020, CreateDXGIFactory1,
                IDXGIFactory1, IDXGIOutput6,
            },
            Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromWindow},
        },
    },
    core::Interface,
};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Display {
    pub hdr: bool,
    pub peak_nits: f32,
    pub headroom: f32,
    pub name: String,
    pub error: Option<String>,
}
impl Default for Display {
    fn default() -> Self {
        Self {
            hdr: false,
            peak_nits: 0.,
            headroom: 1.,
            name: String::new(),
            error: None,
        }
    }
}
impl Display {
    pub(super) fn reported(hdr: bool, peak_nits: f32, name: String) -> Self {
        let valid_peak = peak_nits.is_finite() && peak_nits > 0.;
        Self {
            hdr: hdr && valid_peak,
            peak_nits: if valid_peak { peak_nits } else { 0. },
            headroom: if hdr && valid_peak {
                (peak_nits / layer_core::color::hdr::REFERENCE_WHITE_NITS).clamp(1., 100.)
            } else {
                1.
            },
            name,
            error: None,
        }
    }
    pub fn encoding(&self, format: wgpu::TextureFormat) -> SdrSurfaceColor {
        if format == wgpu::TextureFormat::Rgba16Float {
            if self.hdr {
                SdrSurfaceColor::WindowsScrgb
            } else {
                SdrSurfaceColor::ExtendedLinearSrgb
            }
        } else {
            SdrSurfaceColor::Srgb
        }
    }
    pub fn available_headroom(&self, format: wgpu::TextureFormat) -> f32 {
        if format == wgpu::TextureFormat::Rgba16Float {
            self.headroom
        } else {
            1.
        }
    }
}
/// A SwapChainPanel composition chain has no HWND association. Enumerate DXGI
/// outputs and match the native window's monitor, including another adapter.
/// Recreating the factory refreshes HDR toggles, docking and display topology.
pub(crate) fn probe(window: usize) -> Display {
    let read = || -> Result<Display, String> {
        if window == 0 {
            return Err("Display window is unavailable".into());
        }
        let monitor =
            unsafe { MonitorFromWindow(HWND(window as *mut _), MONITOR_DEFAULTTONEAREST) };
        let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.map_err(|e| e.to_string())?;
        let mut a = 0;
        while let Ok(adapter) = unsafe { factory.EnumAdapters1(a) } {
            a += 1;
            let mut o = 0;
            while let Ok(output) = unsafe { adapter.EnumOutputs(o) } {
                o += 1;
                let desc = unsafe { output.GetDesc() }.map_err(|e| e.to_string())?;
                if desc.Monitor != monitor {
                    continue;
                }
                let output: IDXGIOutput6 = output.cast().map_err(|e| e.to_string())?;
                let desc = unsafe { output.GetDesc1() }.map_err(|e| e.to_string())?;
                let end = desc
                    .DeviceName
                    .iter()
                    .position(|&v| v == 0)
                    .unwrap_or(desc.DeviceName.len());
                return Ok(Display::reported(
                    desc.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020,
                    desc.MaxLuminance,
                    String::from_utf16_lossy(&desc.DeviceName[..end]),
                ));
            }
        }
        Err("No current DXGI output; showing the saved SDR appearance".into())
    };
    read().unwrap_or_else(|error| Display {
        error: Some(error),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_output_changes_and_invalid_reports_fall_back_without_changing_artwork() {
        for (hdr, peak, expected) in [
            (false, 1000., 1.),
            (true, 1015., 5.),
            (true, 0., 1.),
            (true, f32::NAN, 1.),
            (true, f32::INFINITY, 1.),
        ] {
            let display = Display::reported(hdr, peak, String::new());
            assert_eq!(
                display.available_headroom(wgpu::TextureFormat::Rgba16Float),
                expected
            );
            assert_eq!(
                display.available_headroom(wgpu::TextureFormat::Bgra8Unorm),
                1.
            );
            assert_eq!(
                display.encoding(wgpu::TextureFormat::Bgra8Unorm),
                SdrSurfaceColor::Srgb
            );
        }
        assert_eq!(
            Display::reported(true, 1015., String::new())
                .encoding(wgpu::TextureFormat::Rgba16Float),
            SdrSurfaceColor::WindowsScrgb
        );
        assert_eq!(
            Display::default().encoding(wgpu::TextureFormat::Rgba16Float),
            SdrSurfaceColor::ExtendedLinearSrgb
        );
        assert!(probe(0).error.is_some());
    }
}
