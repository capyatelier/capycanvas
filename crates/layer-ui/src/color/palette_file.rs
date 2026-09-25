//! Small, bounded palette interchange. GPL is sRGB; native .capycolor files
//! use JSON to retain HDR, alpha and tagged color spaces. Legacy .json exports
//! remain readable. Parsing never mutates the library.
use super::*;

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
impl ColorLibrary {
    pub const MAX_IMPORT_BYTES: usize = 1024 * 1024;

    pub fn import_file(bytes: &[u8], fallback_name: &str) -> Result<ColorLibraryAction, String> {
        if bytes.len() > Self::MAX_IMPORT_BYTES {
            return Err("Palette files must be at most 1 MB".into());
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|_| "Choose a Capycolor (.capycolor) or GIMP (.gpl) palette")?
            .trim_start_matches('\u{feff}');
        let (name, swatches) =
            if text.trim_start().starts_with('{') {
                let file: PaletteFile =
                    serde_json::from_str(text).map_err(|e| format!("Invalid palette: {e}"))?;
                (
                    file.name,
                    file.colors
                        .into_iter()
                        .map(|c| (c.name, c.color))
                        .collect::<Vec<_>>(),
                )
            } else {
                let mut lines = text.lines();
                if lines.next().map(str::trim) != Some("GIMP Palette") {
                    return Err("Choose a Capycolor (.capycolor) or GIMP (.gpl) palette".into());
                }
                let mut name = fallback_name.to_string();
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
                    let mut rgba = [1.; 4];
                    for channel in &mut rgba[..3] {
                        *channel =
                            f32::from(words.next().and_then(|s| s.parse::<u8>().ok()).ok_or_else(
                                || format!("Invalid RGB value on line {}", index + 2),
                            )?) / 255.;
                    }
                    colors.push((
                        words.collect::<Vec<_>>().join(" "),
                        RgbColor::new(RgbSpace::Srgb, rgba)?,
                    ));
                    if colors.len() > Self::MAX_SWATCHES {
                        return Err("A palette can contain at most 4096 colors".into());
                    }
                }
                (name, colors)
            };
        if swatches.is_empty() {
            return Err("This file contains no colors".into());
        }
        if swatches.len() > Self::MAX_SWATCHES {
            return Err("A palette can contain at most 4096 colors".into());
        }
        for (_, color) in &swatches {
            color.validate()?;
        }
        Ok(ColorLibraryAction::Import { name, swatches })
    }

    pub fn export_palette(&self, id: u64) -> Result<Vec<u8>, String> {
        let palette = self
            .palettes
            .iter()
            .find(|p| p.id == id)
            .ok_or("Palette no longer exists")?;
        serde_json::to_vec_pretty(&PaletteFile {
            name: palette.name.clone(),
            colors: palette
                .swatches
                .iter()
                .map(|s| PaletteColor {
                    name: s.name.clone(),
                    color: s.color,
                })
                .collect(),
        })
        .map_err(|e| e.to_string())
    }
}
