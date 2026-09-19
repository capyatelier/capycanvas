use super::container::{BoxView, Container, Reader, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Color {
    pub cicp: [u16; 3],
    pub full_range: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Geometry {
    pub clap: Option<[u32; 8]>,
    pub rotation: u8,
    pub mirror: Option<u8>,
}
impl Geometry {
    pub fn crop(self, extent: [u32; 2]) -> Result<[u32; 4]> {
        let Some(c) = self.clap else {
            return Ok([0, 0, extent[0], extent[1]]);
        };
        if [c[1], c[3], c[5], c[7]].contains(&0) || c[0] % c[1] != 0 || c[2] % c[3] != 0 {
            return Err("Invalid AVIF clean aperture".into());
        }
        let (w, h) = (c[0] / c[1], c[2] / c[3]);
        let offset = |full: u32, crop: u32, numerator: u32, denominator: u32| -> Result<u32> {
            let n = (i128::from(full) - i128::from(crop)) * i128::from(denominator)
                + 2 * i128::from(numerator as i32);
            let d = 2 * i128::from(denominator);
            if n < 0 || n % d != 0 {
                return Err("Invalid AVIF clean aperture offset".into());
            }
            u32::try_from(n / d).map_err(|_| "AVIF clean aperture overflow".into())
        };
        let (x, y) = (
            offset(extent[0], w, c[4], c[5])?,
            offset(extent[1], h, c[6], c[7])?,
        );
        if w == 0
            || h == 0
            || x.checked_add(w).is_none_or(|n| n > extent[0])
            || y.checked_add(h).is_none_or(|n| n > extent[1])
        {
            return Err("AVIF clean aperture exceeds the image".into());
        }
        Ok([x, y, w, h])
    }
    pub fn extent(self, source: [u32; 2]) -> Result<[u32; 2]> {
        let [_, _, w, h] = self.crop(source)?;
        Ok(if self.rotation % 2 == 0 {
            [w, h]
        } else {
            [h, w]
        })
    }
    pub fn source_pixel(self, crop: [u32; 4], x: u32, y: u32) -> [u32; 2] {
        let [cx, cy, w, h] = crop;
        let output = if self.rotation % 2 == 0 {
            [w, h]
        } else {
            [h, w]
        };
        let x = if self.mirror == Some(1) {
            output[0] - 1 - x
        } else {
            x
        };
        let y = if self.mirror == Some(0) {
            output[1] - 1 - y
        } else {
            y
        };
        let [x, y] = match self.rotation {
            0 => [x, y],
            1 => [w - 1 - y, x],
            2 => [w - 1 - x, h - 1 - y],
            3 => [y, h - 1 - x],
            _ => unreachable!(),
        };
        [cx + x, cy + y]
    }
}

#[derive(Default)]
pub(super) struct Properties<'a> {
    pub extent: Option<[u32; 2]>,
    pub config: Option<&'a [u8]>,
    pub icc: Option<&'a [u8]>,
    pub color: Option<Color>,
    pub geometry: Geometry,
    pub aux: Option<&'a [u8]>,
    pub bits: Option<u8>,
    pub channels: Option<u8>,
}
impl<'a> Properties<'a> {
    pub fn read(container: &Container<'a>, id: u32) -> Result<Self> {
        Self::from_views(
            container
                .links(id)
                .iter()
                .map(|&link| container.property(link).map(|view| (view, link.essential))),
        )
    }
    pub fn from_views(
        views: impl IntoIterator<Item = Result<(BoxView<'a>, bool)>>,
    ) -> Result<Self> {
        let mut p = Self::default();
        let mut transforms = 0;
        for view in views {
            let (view, essential) = view?;
            let mut r = Reader::new(view.data);
            match &view.kind {
                b"ispe" => {
                    if r.full()? != (0, 0) || p.extent.is_some() {
                        return Err("Invalid AVIF spatial extent".into());
                    }
                    p.extent = Some([r.u32()?, r.u32()?]);
                    r.end()?;
                }
                b"av1C" => {
                    if p.config.is_some() || r.left() < 4 || r.u8()? != 0x81 {
                        return Err("Invalid AVIF AV1 configuration".into());
                    }
                    r.take(3)?;
                    p.config = Some(view.data);
                }
                b"colr" => match r.take(4)? {
                    b"nclx" => {
                        if p.color.is_some() {
                            return Err("Duplicate AVIF NCLX".into());
                        }
                        let cicp = [r.u16()?, r.u16()?, r.u16()?];
                        let range = r.u8()?;
                        if range & 127 != 0 {
                            return Err("Invalid AVIF NCLX range".into());
                        }
                        p.color = Some(Color {
                            cicp,
                            full_range: range != 0,
                        });
                        r.end()?;
                    }
                    b"prof" | b"rICC" => {
                        if p.icc.is_some() || r.left() == 0 || r.left() > crate::MAX_ICC_BYTES {
                            return Err("Invalid AVIF ICC profile size".into());
                        }
                        p.icc = Some(r.data);
                    }
                    _ if essential => {
                        return Err("Unsupported essential AVIF color property".into());
                    }
                    _ => {}
                },
                b"pixi" => {
                    if r.full()? != (0, 0) || p.bits.is_some() {
                        return Err("Invalid AVIF pixel information".into());
                    }
                    let channels = r.u8()?;
                    if !matches!(channels, 1 | 3 | 4) {
                        return Err("Unsupported AVIF channel count".into());
                    }
                    let bits = r.u8()?;
                    // A tone-map alternate can describe 16/32-bit output.
                    // Coded items are checked against their AV1 precision.
                    if !(1..=32).contains(&bits) {
                        return Err("Unsupported AVIF sample precision".into());
                    }
                    for _ in 1..channels {
                        if r.u8()? != bits {
                            return Err("Mixed AVIF channel precision".into());
                        }
                    }
                    p.bits = Some(bits);
                    p.channels = Some(channels);
                    r.end()?;
                }
                b"auxC" => {
                    if r.full()? != (0, 0) || p.aux.is_some() {
                        return Err("Invalid AVIF auxiliary property".into());
                    }
                    let end = r
                        .data
                        .iter()
                        .position(|&c| c == 0)
                        .ok_or("Invalid AVIF auxiliary type")?;
                    p.aux = Some(&r.data[..end]);
                }
                b"clap" => {
                    if !essential || transforms != 0 || p.geometry.clap.is_some() {
                        return Err("Invalid AVIF clean aperture property".into());
                    }
                    let mut values = [0; 8];
                    for n in &mut values {
                        *n = r.u32()?;
                    }
                    p.geometry.clap = Some(values);
                    transforms = 1;
                    r.end()?;
                }
                b"irot" => {
                    if !essential || transforms > 1 {
                        return Err("Invalid AVIF rotation property".into());
                    }
                    p.geometry.rotation = r.u8()?;
                    if p.geometry.rotation > 3 {
                        return Err("Invalid AVIF rotation".into());
                    }
                    transforms = 2;
                    r.end()?;
                }
                b"imir" => {
                    if !essential || transforms > 2 {
                        return Err("Invalid AVIF mirror property".into());
                    }
                    let axis = r.u8()?;
                    if axis > 1 {
                        return Err("Invalid AVIF mirror".into());
                    }
                    p.geometry.mirror = Some(axis);
                    transforms = 3;
                    r.end()?;
                }
                b"pasp" => {
                    let (x, y) = (r.u32()?, r.u32()?);
                    if x == 0 || x != y {
                        return Err("AVIF pixel aspect ratio requires resampling".into());
                    }
                    r.end()?;
                }
                // Descriptive HDR metadata does not change stored samples.
                b"clli" | b"mdcv" => {}
                _ if essential => {
                    return Err(format!(
                        "Unsupported essential AVIF property {}",
                        String::from_utf8_lossy(&view.kind)
                    ));
                }
                _ => {}
            }
        }
        Ok(p)
    }
}
