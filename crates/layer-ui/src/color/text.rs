use super::*;
use super::editor::ColorForm;

const MAX_TEXT: usize = 256;

#[derive(Clone, Copy)]
struct Number {
    value: f64,
    percent: bool,
}

pub(super) fn parse_color_text(text: &str, document: RgbSpace, form: Option<ColorForm>) -> Result<RgbColor, ColorEditorError> {
    if text.len() > MAX_TEXT {
        return Err(ColorEditorError::ColorSyntax);
    }
    let lower = text.trim().trim_end_matches(';').trim().to_ascii_lowercase();
    if let Some(rgb) = named(&lower).or_else(|| hex(&lower)) {
        return Ok(RgbColor::new(RgbSpace::Srgb, [rgb[0], rgb[1], rgb[2], 1.])?);
    }
    let (function, body) = match lower.find('(') {
        Some(open) if lower.ends_with(')') => (lower[..open].trim(), &lower[open + 1..lower.len() - 1]),
        Some(_) => return Err(ColorEditorError::ColorSyntax),
        None => ("", lower.as_str()),
    };
    let channels = body.split_once('/').map_or(body, |(channels, _)| channels);
    let mut tokens: Vec<&str> = channels
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|token| !token.is_empty())
        .collect();
    let space = if function == "color" {
        if tokens.is_empty() {
            return Err(ColorEditorError::ColorSyntax);
        }
        Some(tokens.remove(0))
    } else {
        None
    };
    if tokens.len() == 4 && !body.contains('/') && matches!(function, "rgb" | "rgba" | "hsl" | "hsla") {
        tokens.pop();
    }
    let values: Vec<Number> = tokens.iter().map(|token| number(token)).collect::<Option<_>>().ok_or(ColorEditorError::ColorSyntax)?;
    let [a, b, c]: [Number; 3] = values.try_into().map_err(|_| ColorEditorError::ColorSyntax)?;
    let percent = |n: Number| if n.percent { n.value / 100. } else { n.value };
    let fraction = |n: Number, full: f64| if n.percent { n.value / 100. } else { n.value / full };
    let srgb = |rgb: [f64; 3]| Ok(RgbColor::new(RgbSpace::Srgb, [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.])?);
    match function {
        "rgb" | "rgba" => srgb([a, b, c].map(|n| fraction(n, 255.).clamp(0., 1.))),
        "hsl" | "hsla" => srgb(from_cylinder(ColorSpace::Hls, a.value, fraction(b, 100.), fraction(c, 100.))),
        "hsb" | "hsv" => srgb(from_cylinder(ColorSpace::Hsv, a.value, fraction(b, 100.), fraction(c, 100.))),
        "oklch" => {
            let chroma = if b.percent { b.value / 100. * 0.4 } else { b.value };
            let hue = c.value.to_radians();
            oklab(document, [lightness(a), chroma * hue.cos(), chroma * hue.sin()])
        }
        "oklab" => {
            let axis = |n: Number| if n.percent { n.value / 100. * 0.4 } else { n.value };
            oklab(document, [lightness(a), axis(b), axis(c)])
        }
        "color" => {
            let rgb = [a, b, c].map(percent);
            let encoded = |space| Ok(RgbColor::new(space, [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.])?);
            match space {
                Some("srgb") => encoded(RgbSpace::Srgb),
                Some("display-p3") => encoded(RgbSpace::DisplayP3),
                Some("a98-rgb") => encoded(RgbSpace::AdobeRgb),
                Some("prophoto-rgb") => encoded(RgbSpace::ProPhoto),
                Some("srgb-linear") => Ok(RgbColor::from_linear(RgbSpace::Srgb, [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.])?),
                _ => Err(ColorEditorError::ColorSyntax),
            }
        }
        "" => {
            let form = form.unwrap_or(if [a, b, c].iter().all(|n| !n.percent && n.value <= 1.) && tokens.iter().any(|t| t.contains('.')) {
                ColorForm::RgbUnit
            } else {
                ColorForm::Rgb
            });
            form.color([a.value, b.value, c.value], document, 1.)
        }
        _ => Err(ColorEditorError::ColorSyntax),
    }
}

fn lightness(n: Number) -> f64 {
    if n.percent || n.value > 1. { n.value / 100. } else { n.value }
}

fn oklab(space: RgbSpace, lab: [f64; 3]) -> Result<RgbColor, ColorEditorError> {
    if !lab.iter().all(|v| v.is_finite()) || lab[0] < 0. {
        return Err(ColorEditorError::ColorSyntax);
    }
    let rgb = gamut::Gamut::get(space).linear_rgb(lab);
    Ok(RgbColor::from_linear(space, [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.])?)
}

pub(super) fn from_cylinder(space: ColorSpace, hue: f64, second: f64, third: f64) -> [f64; 3] {
    let values = match space {
        ColorSpace::Hsv => [hue.rem_euclid(360.), second.clamp(0., 1.) * 100., third.clamp(0., 1.) * 100.],
        ColorSpace::Hls => [hue.rem_euclid(360.), third.clamp(0., 1.) * 100., second.clamp(0., 1.) * 100.],
    };
    let rgba = from_components(values.map(|v| v as f32), space, 1.);
    [rgba[0], rgba[1], rgba[2]].map(f64::from)
}

fn number(token: &str) -> Option<Number> {
    let token = token.trim_end_matches('°').trim_end_matches("deg");
    let percent = token.ends_with('%');
    let value: f64 = token.trim_end_matches('%').parse().ok()?;
    value.is_finite().then_some(Number { value, percent })
}

fn hex(text: &str) -> Option<[f32; 3]> {
    let digits = text.strip_prefix('#').or_else(|| text.strip_prefix("0x")).unwrap_or(text);
    if !matches!(digits.len(), 3 | 4 | 6 | 8) || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |i: usize| -> f32 {
        if digits.len() <= 4 {
            u8::from_str_radix(&digits[i..i + 1], 16).unwrap() as f32 * 17. / 255.
        } else {
            u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).unwrap() as f32 / 255.
        }
    };
    Some([channel(0), channel(1), channel(2)])
}

fn named(text: &str) -> Option<[f32; 3]> {
    let (_, value) = NAMED.iter().find(|(name, _)| *name == text)?;
    Some([16, 8, 0].map(|shift| ((value >> shift) & 0xff) as f32 / 255.))
}

const NAMED: [(&str, u32); 148] = [
    ("aliceblue", 0xf0f8ff), ("antiquewhite", 0xfaebd7), ("aqua", 0x00ffff), ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff), ("beige", 0xf5f5dc), ("bisque", 0xffe4c4), ("black", 0x000000),
    ("blanchedalmond", 0xffebcd), ("blue", 0x0000ff), ("blueviolet", 0x8a2be2), ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887), ("cadetblue", 0x5f9ea0), ("chartreuse", 0x7fff00), ("chocolate", 0xd2691e),
    ("coral", 0xff7f50), ("cornflowerblue", 0x6495ed), ("cornsilk", 0xfff8dc), ("crimson", 0xdc143c),
    ("cyan", 0x00ffff), ("darkblue", 0x00008b), ("darkcyan", 0x008b8b), ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9), ("darkgreen", 0x006400), ("darkgrey", 0xa9a9a9), ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b), ("darkolivegreen", 0x556b2f), ("darkorange", 0xff8c00), ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000), ("darksalmon", 0xe9967a), ("darkseagreen", 0x8fbc8f), ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f), ("darkslategrey", 0x2f4f4f), ("darkturquoise", 0x00ced1), ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493), ("deepskyblue", 0x00bfff), ("dimgray", 0x696969), ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff), ("firebrick", 0xb22222), ("floralwhite", 0xfffaf0), ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff), ("gainsboro", 0xdcdcdc), ("ghostwhite", 0xf8f8ff), ("gold", 0xffd700),
    ("goldenrod", 0xdaa520), ("gray", 0x808080), ("green", 0x008000), ("greenyellow", 0xadff2f),
    ("grey", 0x808080), ("honeydew", 0xf0fff0), ("hotpink", 0xff69b4), ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082), ("ivory", 0xfffff0), ("khaki", 0xf0e68c), ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5), ("lawngreen", 0x7cfc00), ("lemonchiffon", 0xfffacd), ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080), ("lightcyan", 0xe0ffff), ("lightgoldenrodyellow", 0xfafad2), ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90), ("lightgrey", 0xd3d3d3), ("lightpink", 0xffb6c1), ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa), ("lightskyblue", 0x87cefa), ("lightslategray", 0x778899), ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de), ("lightyellow", 0xffffe0), ("lime", 0x00ff00), ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6), ("magenta", 0xff00ff), ("maroon", 0x800000), ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd), ("mediumorchid", 0xba55d3), ("mediumpurple", 0x9370db), ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee), ("mediumspringgreen", 0x00fa9a), ("mediumturquoise", 0x48d1cc), ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970), ("mintcream", 0xf5fffa), ("mistyrose", 0xffe4e1), ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead), ("navy", 0x000080), ("oldlace", 0xfdf5e6), ("olive", 0x808000),
    ("olivedrab", 0x6b8e23), ("orange", 0xffa500), ("orangered", 0xff4500), ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa), ("palegreen", 0x98fb98), ("paleturquoise", 0xafeeee), ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5), ("peachpuff", 0xffdab9), ("peru", 0xcd853f), ("pink", 0xffc0cb),
    ("plum", 0xdda0dd), ("powderblue", 0xb0e0e6), ("purple", 0x800080), ("rebeccapurple", 0x663399),
    ("red", 0xff0000), ("rosybrown", 0xbc8f8f), ("royalblue", 0x4169e1), ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072), ("sandybrown", 0xf4a460), ("seagreen", 0x2e8b57), ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d), ("silver", 0xc0c0c0), ("skyblue", 0x87ceeb), ("slateblue", 0x6a5acd),
    ("slategray", 0x708090), ("slategrey", 0x708090), ("snow", 0xfffafa), ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4), ("tan", 0xd2b48c), ("teal", 0x008080), ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347), ("turquoise", 0x40e0d0), ("violet", 0xee82ee), ("wheat", 0xf5deb3),
    ("white", 0xffffff), ("whitesmoke", 0xf5f5f5), ("yellow", 0xffff00), ("yellowgreen", 0x9acd32),
];

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(text: &str) -> RgbColor { parse_color_text(text, RgbSpace::Srgb, None).unwrap() }
    fn hex_of(color: RgbColor) -> String { ColorLibrary::hex_preview(color) }

    #[test]
    fn css_notations_hex_names_and_bare_numbers_agree() {
        for text in ["#3B7EA1", "3b7ea1", "0x3B7EA1", "#3B7EA1FF", "rgb(59 126 161)", "rgb(59, 126, 161)", "rgba(59,126,161,0.5)",
            "rgb(59 126 161 / 50%)", "color(srgb 0.2314 0.4941 0.6314)", "59 126 161", "59, 126, 161", "0.2314 0.4941 0.6314",
            "hsl(200.59 46.36% 43.14%)", "hsb(200.59 63.35% 63.14%)", "oklch(56.5% 0.087 234)", "oklab(56.5% -0.051 -0.070)"] {
            assert_eq!(hex_of(parse(text)), "#3B7EA1", "{text}");
        }
        assert_eq!(hex_of(parse("#abc")), "#AABBCC");
        assert_eq!(hex_of(parse("RebeccaPurple")), "#663399");
        assert_eq!(hex_of(parse("color(srgb-linear 0.04374 0.20864 0.3564)")), "#3B7EA1");
        assert_eq!(parse("#3B7EA180").rgba[3], 1.);
    }

    #[test]
    fn explicit_spaces_keep_their_meaning_and_bare_numbers_use_the_named_form() {
        let p3 = parse_color_text("color(display-p3 1 0 0)", RgbSpace::Srgb, None).unwrap();
        assert_eq!(p3.space, RgbSpace::DisplayP3);
        assert!(!p3.in_gamut(RgbSpace::Srgb).unwrap());
        let bare = parse_color_text("255 0 0", RgbSpace::DisplayP3, None).unwrap();
        assert_eq!(bare.space, RgbSpace::DisplayP3);
        let hsl = parse_color_text("200.59 46.36 43.14", RgbSpace::Srgb, Some(ColorForm::Hsl)).unwrap();
        assert_eq!(hex_of(hsl), "#3B7EA1");
        let linear = parse_color_text("0.04374 0.20864 0.3564", RgbSpace::Srgb, Some(ColorForm::LinearRgb)).unwrap();
        assert_eq!(hex_of(linear), "#3B7EA1");
        assert_eq!(hex_of(parse_color_text("200.59° 63.35% 63.14%", RgbSpace::Srgb, Some(ColorForm::Hsb)).unwrap()), "#3B7EA1");
    }

    #[test]
    fn malformed_or_unknown_text_is_refused() {
        for text in ["", "#12", "#12345", "rgb(1 2)", "rgb(1 2 3", "lab(50 0 0)", "color(rec2020 1 0 0)", "teal-ish", "1 2 3 4 5",
            "oklch(-5% 0.1 20)", &"9".repeat(300)] {
            assert!(matches!(parse_color_text(text, RgbSpace::Srgb, None), Err(ColorEditorError::ColorSyntax)), "{text}");
        }
    }
}
