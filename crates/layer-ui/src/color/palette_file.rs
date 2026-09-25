//! Bounded palette interchange. Parsing never mutates the library.
use super::*;
#[path = "palette_adobe.rs"]
mod adobe;
#[path = "palette_apps.rs"]
mod apps;
#[path = "palette_archive.rs"]
mod archive;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PaletteFile {
    name: String,
    colors: Vec<PaletteColor>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PaletteColor {
    #[serde(default)]
    name: String,
    color: RgbColor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaletteFormat {
    Capycolor,
    Aco,
    Swatches,
    Ase,
    Gpl,
}
impl PaletteFormat {
    pub const ALL: [Self; 5] = [
        Self::Capycolor,
        Self::Aco,
        Self::Swatches,
        Self::Ase,
        Self::Gpl,
    ];
    pub const IMPORT_EXTENSIONS: [&'static str; 9] = [
        "capycolor",
        "aco",
        "cls",
        "swatches",
        "ase",
        "afpalette",
        "gpl",
        "kpl",
        "json",
    ];
    pub fn extension(self) -> &'static str {
        match self {
            Self::Capycolor => "capycolor",
            Self::Aco => "aco",
            Self::Swatches => "swatches",
            Self::Ase => "ase",
            Self::Gpl => "gpl",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Capycolor => "Capycolor (.capycolor)",
            Self::Aco => "Clip Studio Paint, Photoshop (.aco)",
            Self::Swatches => "Procreate (.swatches)",
            Self::Ase => "Affinity, Adobe (.ase)",
            Self::Gpl => "Krita, GIMP (.gpl)",
        }
    }
    pub fn from_file_name(name: &str) -> Option<Self> {
        let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
        Self::ALL.into_iter().find(|f| f.extension() == extension)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PaletteExport {
    pub file_name: String,
    #[serde(skip)]
    pub bytes: Vec<u8>,
    pub notice: Option<String>,
}

pub(crate) type Imported = (String, RgbColor);

fn too_many() -> String {
    format!(
        "A palette can contain at most {} colors",
        ColorLibrary::MAX_SWATCHES
    )
}
pub(crate) fn push_color(
    colors: &mut Vec<Imported>,
    name: String,
    color: RgbColor,
) -> Result<(), String> {
    if colors.len() == ColorLibrary::MAX_SWATCHES {
        return Err(too_many());
    }
    colors.push((name, color));
    Ok(())
}

pub(crate) fn srgb(rgb: [f64; 3]) -> Result<RgbColor, String> {
    if !rgb.iter().all(|v| v.is_finite()) {
        return Err("Palette color values must be finite".into());
    }
    RgbColor::new(
        RgbSpace::Srgb,
        [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.],
    )
}
pub(crate) fn hsb(h: f64, s: f64, b: f64) -> [f64; 3] {
    let h = h.rem_euclid(1.) * 6.;
    let f = |n: f64| {
        let k = (n + h) % 6.;
        b - b * s * k.min(4. - k).clamp(0., 1.)
    };
    [f(5.), f(3.), f(1.)]
}
pub(crate) fn cmyk(c: f64, m: f64, y: f64, k: f64) -> [f64; 3] {
    [c, m, y].map(|v| (1. - v.clamp(0., 1.)) * (1. - k.clamp(0., 1.)))
}
pub(crate) fn lab(l: f64, a: f64, b: f64) -> Result<RgbColor, String> {
    let fy = (l + 16.) / 116.;
    let inverse = |t: f64| {
        if t > 6. / 29. {
            t * t * t
        } else {
            3. * (6f64 / 29.).powi(2) * (t - 4. / 29.)
        }
    };
    let white = [0.3457 / 0.3585, 1., (1. - 0.3457 - 0.3585) / 0.3585];
    xyz([
        inverse(fy + a / 500.) * white[0],
        inverse(fy),
        inverse(fy - b / 200.) * white[2],
    ])
}
pub(crate) fn xyz(xyz: [f64; 3]) -> Result<RgbColor, String> {
    let linear = layer_core::color::rgb::apply(
        layer_core::color::rgb::inverse(RgbSpace::ProPhoto.to_xyz()),
        xyz,
    );
    if !linear.iter().all(|v| v.is_finite()) {
        return Err("Palette color values must be finite".into());
    }
    let color = RgbColor::from_linear(
        RgbSpace::ProPhoto,
        [linear[0] as f32, linear[1] as f32, linear[2] as f32, 1.],
    )?;
    for space in [RgbSpace::Srgb, RgbSpace::DisplayP3] {
        let rgba = color.encoded_in(space)?;
        if rgba[..3].iter().all(|v| (-1e-5..=1.00001).contains(v)) {
            return RgbColor::new(space, rgba.map(|v| v.clamp(0., 1.)));
        }
    }
    Ok(color)
}

impl ColorLibrary {
    pub const MAX_IMPORT_BYTES: usize = 1024 * 1024;

    pub fn import_file(bytes: &[u8], fallback_name: &str) -> Result<ColorLibraryAction, String> {
        if bytes.len() > Self::MAX_IMPORT_BYTES {
            return Err("Palette files must be at most 1 MB".into());
        }
        let (name, swatches) = if bytes.starts_with(b"ASEF") {
            adobe::read_ase(bytes)?
        } else if bytes.starts_with(b"SLCC") {
            apps::read_cls(bytes)?
        } else if bytes.starts_with(b"\0\xffKA") {
            apps::read_afpalette(bytes)?
        } else if bytes.starts_with(b"PK\x03\x04") {
            archive::read(bytes)?
        } else if let Some(colors) = adobe::read_aco(bytes)? {
            (String::new(), colors)
        } else {
            let text = std::str::from_utf8(bytes)
                .map_err(|_| unsupported())?
                .trim_start_matches('\u{feff}');
            if text.trim_start().starts_with('{') {
                let file: PaletteFile =
                    serde_json::from_str(text).map_err(|e| format!("Invalid palette: {e}"))?;
                if file.colors.len() > Self::MAX_SWATCHES {
                    return Err(too_many());
                }
                (
                    file.name,
                    file.colors.into_iter().map(|c| (c.name, c.color)).collect(),
                )
            } else {
                read_gpl(text)?
            }
        };
        if swatches.is_empty() {
            return Err("This file contains no colors".into());
        }
        for (_, color) in &swatches {
            color.validate()?;
        }
        let name = if name.trim().is_empty() {
            fallback_name.to_string()
        } else {
            name
        };
        let clip = |value: String| {
            value
                .chars()
                .filter(|c| !c.is_control())
                .take(64)
                .collect::<String>()
        };
        Ok(ColorLibraryAction::Import {
            name: clip(name),
            swatches: swatches.into_iter().map(|(n, c)| (clip(n), c)).collect(),
        })
    }

    pub fn export_palette(&self, id: u64, format: PaletteFormat) -> Result<PaletteExport, String> {
        self.palettes
            .iter()
            .find(|p| p.id == id)
            .ok_or("Palette no longer exists")?
            .export(format)
    }
}

impl ColorPalette {
    pub fn export(&self, format: PaletteFormat) -> Result<PaletteExport, String> {
        let (bytes, notice) = match format {
            PaletteFormat::Capycolor => (
                serde_json::to_vec_pretty(&PaletteFile {
                    name: self.name.clone(),
                    colors: self
                        .swatches
                        .iter()
                        .map(|s| PaletteColor {
                            name: s.name.clone(),
                            color: s.color,
                        })
                        .collect(),
                })
                .map_err(|e| e.to_string())?,
                None,
            ),
            PaletteFormat::Aco
            | PaletteFormat::Swatches
            | PaletteFormat::Ase
            | PaletteFormat::Gpl => {
                let (mut clipped, mut transparent) = (0, 0);
                let mut colors = Vec::with_capacity(self.swatches.len());
                for swatch in &self.swatches {
                    let rgba = swatch.color.encoded_in(RgbSpace::Srgb)?;
                    clipped +=
                        usize::from(rgba[..3].iter().any(|v| !(-1e-6..=1.000001).contains(v)));
                    transparent += usize::from(rgba[3] < 1.);
                    colors.push((swatch.name.as_str(), rgba.map(|v| v.clamp(0., 1.))));
                }
                let bytes = match format {
                    PaletteFormat::Aco => adobe::write_aco(&colors)?,
                    PaletteFormat::Swatches => archive::write_swatches(&self.name, &colors)?,
                    PaletteFormat::Ase => adobe::write_ase(&self.name, &colors)?,
                    _ => write_gpl(&self.name, &colors),
                };
                let omitted = if format == PaletteFormat::Swatches {
                    colors.len().saturating_sub(archive::PROCREATE_SLOTS)
                } else {
                    0
                };
                let changes: Vec<_> = [
                    (
                        clipped,
                        "color outside sRGB was clipped",
                        "colors outside sRGB were clipped",
                    ),
                    (transparent, "color became opaque", "colors became opaque"),
                    (
                        omitted,
                        "color after Procreate's 30 was left out",
                        "colors after Procreate's 30 were left out",
                    ),
                ]
                .into_iter()
                .filter(|(count, ..)| *count > 0)
                .map(|(count, one, many)| {
                    format!("{count} {}", if count == 1 { one } else { many })
                })
                .collect();
                let notice = (!changes.is_empty())
                    .then(|| format!("{}. Capycolor keeps exact colors.", changes.join("; ")));
                (bytes, notice)
            }
        };
        Ok(PaletteExport {
            file_name: file_name(&self.name, format),
            bytes,
            notice,
        })
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PaletteFileRequest {
    Limits,
    Import {
        file_name: String,
    },
    Export {
        palette: ColorPalette,
        format: PaletteFormat,
    },
}
pub fn palette_file(
    request: PaletteFileRequest,
    bytes: &[u8],
) -> Result<(serde_json::Value, Vec<u8>), String> {
    Ok(match request {
        PaletteFileRequest::Limits => (
            serde_json::json!({
                "read_bytes": ColorLibrary::MAX_IMPORT_BYTES + 1,
                "extensions": PaletteFormat::IMPORT_EXTENSIONS,
            }),
            Vec::new(),
        ),
        PaletteFileRequest::Import { file_name } => {
            let stem = file_name
                .rsplit_once('.')
                .map_or(file_name.as_str(), |(stem, _)| stem);
            let action = ColorLibrary::import_file(
                bytes,
                if stem.trim().is_empty() {
                    "Imported palette"
                } else {
                    stem
                },
            )?;
            (serde_json::json!({ "action": action }), Vec::new())
        }
        PaletteFileRequest::Export { palette, format } => {
            let export = palette.export(format)?;
            (
                serde_json::to_value(&export).map_err(|e| e.to_string())?,
                export.bytes,
            )
        }
    })
}

fn file_name(name: &str, format: PaletteFormat) -> String {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_control() || r#"/\:*?"<>|"#.contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    format!("{}.{}", stem.trim_matches(['.', ' ']), format.extension())
}

fn unsupported() -> String {
    "Choose a Capycolor, Clip Studio Paint, Photoshop, Procreate, Affinity, Adobe, Krita or GIMP palette".into()
}

fn read_gpl(text: &str) -> Result<(String, Vec<Imported>), String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("GIMP Palette") {
        return Err(unsupported());
    }
    let mut name = String::new();
    let mut colors = Vec::new();
    for (index, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("Columns:") {
            continue;
        }
        if let Some(value) = line.strip_prefix("Name:") {
            name = value.trim().into();
            continue;
        }
        let mut words = line.split_whitespace();
        let mut rgb = [0.; 3];
        for channel in &mut rgb {
            *channel = f64::from(
                words
                    .next()
                    .and_then(|s| s.parse::<u8>().ok())
                    .ok_or_else(|| format!("Invalid RGB value on line {}", index + 2))?,
            ) / 255.;
        }
        let name = words.collect::<Vec<_>>().join(" ");
        let name = if name == "Untitled" {
            String::new()
        } else {
            name
        };
        push_color(&mut colors, name, srgb(rgb)?)?;
    }
    Ok((name, colors))
}

fn write_gpl(name: &str, colors: &[(&str, [f32; 4])]) -> Vec<u8> {
    let mut text = format!("GIMP Palette\nName: {name}\nColumns: 0\n#\n");
    for (name, rgba) in colors {
        let [r, g, b] = [rgba[0], rgba[1], rgba[2]].map(|v| (v * 255.).round() as u8);
        text.push_str(&format!("{r:3} {g:3} {b:3}\t{name}\n"));
    }
    text.into_bytes()
}

#[cfg(test)]
#[path = "palette_file_tests.rs"]
mod tests;
