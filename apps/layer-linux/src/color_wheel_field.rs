//! Native preview raster work: one in-flight job and one replaceable request.
//! Color evaluation stays in the shared color model; GTK only installs textures.
use super::*;
use layer_core::color::RgbSpace;

pub(super) struct FieldPath {
    pub size: f32,
    pub path: gtk::gsk::Path,
}
impl FieldPath {
    pub fn new(size: f32, geometry: &ColorWheelGeometry) -> Self {
        let path = gtk::gsk::PathBuilder::new();
        path.move_to(geometry.triangle[0][0], geometry.triangle[0][1]);
        for p in &geometry.triangle[1..] { path.line_to(p[0], p[1]); }
        path.close();
        Self { size, path: path.to_path() }
    }
}

pub(super) type LinearField = (u32, f32, ColorShape, RgbSpace, Vec<[f32; 4]>);
pub(super) type Guide = (u32, ColorShape, RgbSpace, ViewColor, gtk::gdk::Texture);
pub(super) type Request = (Key, ColorState, Option<LinearField>, Option<Guide>);
type Disc = (
    u32,
    f32,
    ColorShape,
    RgbSpace,
    ViewColor,
    f32,
    f32,
    gtk::gdk::Texture,
);

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Key {
    side: u32,
    hue: f32,
    shape: ColorShape,
    space: RgbSpace,
    view: ViewColor,
    intensity: f32,
    headroom: f32,
}
impl Key {
    pub fn new(side: u32, state: &ColorState, view: ViewColor, headroom: f32) -> Self {
        Self {
            side,
            hue: state.wheel_components()[0],
            shape: state.shape,
            space: state.rgb_space(),
            view,
            intensity: state.hdr_intensity(),
            headroom,
        }
    }
    pub fn tuple(self) -> (u32, f32, ColorShape, RgbSpace, ViewColor, f32, f32) {
        (
            self.side,
            self.hue,
            self.shape,
            self.space,
            self.view,
            self.intensity,
            self.headroom,
        )
    }
    pub fn disc(self, texture: gtk::gdk::Texture) -> Disc {
        (
            self.side,
            self.hue,
            self.shape,
            self.space,
            self.view,
            self.intensity,
            self.headroom,
            texture,
        )
    }
    fn compatible(self, other: Self) -> bool {
        self.shape == other.shape
            && self.space == other.space
            && self.view == other.view
            && self.headroom == other.headroom
    }
}

fn render(
    key: Key,
    state: &ColorState,
    linear: &mut Option<LinearField>,
) -> gtk::gdk::Texture {
    let Key {
        side,
        hue,
        shape,
        space,
        view,
        intensity,
        headroom,
    } = key;
    if matches!(view, ViewColor::Mapped { .. }) {
        if linear
            .as_ref()
            .is_none_or(|(s, h, p, c, _)| (*s, *h, *p, *c) != (side, hue, shape, space))
        {
            let mut pixels = vec![[0.; 4]; side as usize * side as usize];
            state.render_field_base_linear(side, &mut pixels);
            *linear = Some((side, hue, shape, space, pixels));
        }
        crate::display_color::picker_texture_with_gain(
            view,
            headroom,
            space,
            [side, side],
            &linear.as_ref().unwrap().4,
            f64::from(intensity).exp2(),
        )
    } else {
        *linear = None;
        let mut pixels = vec![0; side as usize * side as usize * 4];
        state.render_field_in(side, view.space(), &mut pixels);
        view.rgba8([side, side], pixels)
    }
}

impl ColorWheel {
    pub(super) fn request_field(&self, key: Key, state: &ColorState, ready: bool) {
        self.imp().field.request(
            self,
            key,
            || {
                (!ready).then(|| {
                    (
                        key,
                        state.clone(),
                        self.imp().linear_field.borrow_mut().take(),
                        self.imp().ring.borrow().clone(),
                    )
                })
            },
            |(key, state, mut linear, mut ring)| {
                let started = std::time::Instant::now();
                let texture = render(key, &state, &mut linear);
                if ring.as_ref().is_none_or(|(side, shape, space, view, _)|
                    (*side, *shape, *space, *view) != (key.side, key.shape, key.space, key.view)) {
                    let mut pixels = vec![0; key.side as usize * key.side as usize * 8];
                    layer_ui::render_hue_ring_f16_in(key.side, key.shape, key.space, key.view.space(), &mut pixels);
                    let guide = key.view.texture([key.side, key.side], gtk::gdk::MemoryFormat::R16g16b16a16Float,
                        key.side as usize * 8, pixels);
                    ring = Some((key.side, key.shape, key.space, key.view, guide));
                }
                (texture, linear, ring, started.elapsed())
            },
            |wheel, key, (texture, linear, ring, _elapsed)| {
                #[cfg(test)]
                wheel
                    .imp()
                    .field_render_ms
                    .borrow_mut()
                    .push(_elapsed.as_secs_f64() * 1000.);
                *wheel.imp().linear_field.borrow_mut() = linear;
                *wheel.imp().disc.borrow_mut() = Some(key.disc(texture));
                *wheel.imp().ring.borrow_mut() = ring;
            },
            Key::compatible,
        );
    }
}
