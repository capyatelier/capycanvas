//! Display/output coordinates, independent of document depth and editing values.
use layer_core::color::RgbSpace;

/// Supported SDR surface encodings. Extended linear sRGB carries wide-gamut SDR
/// colors to the compositor without a premature sRGB gamut clamp. It does not
/// enable HDR editing or change document reference white.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SdrSurfaceColor {
    #[default]
    Srgb,
    DisplayP3,
    ExtendedLinearSrgb,
    /// Wayland Windows-scRGB: linear sRGB with RGB 1 = 80 cd/m².
    WindowsScrgb,
}
impl SdrSurfaceColor {
    pub fn surface_color_space(self) -> wgpu::SurfaceColorSpace {
        match self {
            Self::Srgb => wgpu::SurfaceColorSpace::Srgb,
            Self::DisplayP3 => wgpu::SurfaceColorSpace::DisplayP3,
            Self::ExtendedLinearSrgb | Self::WindowsScrgb => wgpu::SurfaceColorSpace::ExtendedSrgbLinear,
        }
    }
    pub(crate) fn primaries(self) -> RgbSpace {
        if self == Self::DisplayP3 {
            RgbSpace::DisplayP3
        } else {
            RgbSpace::Srgb
        }
    }
    pub(crate) fn shader_encoding(
        self,
        format: wgpu::TextureFormat,
    ) -> Result<bool, crate::GpuRasterError> {
        if matches!(self, Self::ExtendedLinearSrgb | Self::WindowsScrgb) {
            if !matches!(
                format,
                wgpu::TextureFormat::Rgba16Float | wgpu::TextureFormat::Rgba32Float
            ) {
                return Err(crate::GpuRasterError::Color(
                    "Extended linear viewing requires a floating-point surface".into(),
                ));
            }
            Ok(false)
        } else {
            Ok(!format.is_srgb())
        }
    }
}

pub(crate) fn transform(name: &str, source: RgbSpace, destination: RgbSpace) -> String {
    if source == destination {
        return format!("fn {name}(rgb:vec3<f32>)->vec3<f32>{{return rgb;}}\n");
    }
    let matrix = source.linear_transform(destination);
    let row = |i: usize| {
        format!(
            "vec3<f32>({:.12},{:.12},{:.12})",
            matrix[i][0], matrix[i][1], matrix[i][2]
        )
    };
    format!(
        "fn {name}(rgb:vec3<f32>)->vec3<f32>{{return vec3<f32>(dot({},rgb),dot({},rgb),dot({},rgb));}}\n",
        row(0),
        row(1),
        row(2)
    )
}

pub(crate) fn shader(source: RgbSpace, destination: RgbSpace) -> String {
    let mut result = transform("view_working_rgb", source, destination);
    result.push_str(&transform("view_ui_rgb", RgbSpace::Srgb, destination));
    result.push_str("fn view_straight(c:vec4<f32>)->vec3<f32>{if c.a>0. {return view_working_rgb(c.rgb/c.a);}return vec3<f32>(0.);}\n");
    result
}
