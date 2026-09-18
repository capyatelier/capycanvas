//! Display/output coordinates, independent of document depth and editing values.
use layer_core::color::RgbSpace;

/// Surface encodings, independent of document storage. Extended linear sRGB
/// carries wide-gamut SDR; scRGB and PQ additionally support negotiated HDR.
/// Surface selection never changes the document's reference white.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SdrSurfaceColor {
    #[default]
    Srgb,
    DisplayP3,
    ExtendedLinearSrgb,
    /// Wayland Windows-scRGB: linear sRGB with RGB 1 = 80 cd/m².
    WindowsScrgb,
    /// Full-range BT.2020 PQ; RGB 1 in the artwork remains 203 cd/m².
    Bt2100Pq,
}
impl SdrSurfaceColor {
    pub fn surface_color_space(self) -> wgpu::SurfaceColorSpace {
        match self {
            Self::Srgb => wgpu::SurfaceColorSpace::Srgb,
            Self::DisplayP3 => wgpu::SurfaceColorSpace::DisplayP3,
            Self::ExtendedLinearSrgb | Self::WindowsScrgb => wgpu::SurfaceColorSpace::ExtendedSrgbLinear,
            Self::Bt2100Pq => wgpu::SurfaceColorSpace::Bt2100Pq,
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
        if matches!(self, Self::ExtendedLinearSrgb | Self::WindowsScrgb | Self::Bt2100Pq) {
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
    matrix_shader(name, matrix)
}

pub(crate) fn matrix_shader(name: &str, matrix: layer_core::color::rgb::Matrix3) -> String {
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
