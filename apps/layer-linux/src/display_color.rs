//! Managed SDR and HDR presentation for the app-owned surface and GTK artwork.
//! Monitor conversion belongs to the compositor; document values stay unchanged.
use gtk::{gdk, glib, prelude::*, subclass::prelude::*};
use layer_core::color::{RgbColor, RgbSpace};
use std::{
    cell::{Cell, RefCell},
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStrExt,
};

fn with_gtk_color_management(flags: &OsStr) -> OsString {
    if flags
        .as_bytes()
        .split(|b| b":;, \t".contains(b))
        .any(|flag| flag.eq_ignore_ascii_case(b"color-mgmt"))
    {
        return flags.into();
    }
    let mut flags = flags.to_os_string();
    if !flags.is_empty() {
        flags.push(":");
    }
    flags.push("color-mgmt");
    flags
}

/// GTK 4.22 gates its Wayland color-manager binding behind this flag and has
/// no public setter. Preserve unrelated diagnostic flags. Native test runners
/// set this in their launch environment, never inside the threaded test harness.
/// Revisit when GTK offers a stable API or enables the protocol by default:
/// https://github.com/GNOME/gtk/blob/4.22.4/gdk/wayland/gdkdisplay-wayland.c
///
/// # Safety
/// Call only at the start of the single-threaded application entry point,
/// before initializing GTK or starting any application/library worker threads.
pub unsafe fn enable_gtk_color_management() {
    let flags = with_gtk_color_management(&std::env::var_os("GDK_DEBUG").unwrap_or_default());
    // SAFETY: the caller guarantees exclusive access to the environment.
    unsafe { std::env::set_var("GDK_DEBUG", flags) };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewColor {
    #[default]
    Srgb,
    DisplayP3,
    Mapped { p3: bool, document: RgbSpace, recipe: [u32; 8] },
}
impl ViewColor {
    pub fn with_rendition(self, document: layer_core::color::DocumentColor, rendition: layer_core::color::hdr::SdrRendition) -> Self {
        if !document.depth.is_float() { return self.base(); }
        Self::Mapped { p3: self.base() == Self::DisplayP3, document: document.space,
            recipe: rendition.parameters().map(f32::to_bits) }
    }
    fn base(self) -> Self {
        match self { Self::Mapped { p3, .. } => if p3 { Self::DisplayP3 } else { Self::Srgb }, other => other }
    }
    pub fn space(self) -> RgbSpace {
        match self.base() {
            Self::Mapped { .. } => unreachable!(),
            Self::Srgb => RgbSpace::Srgb,
            Self::DisplayP3 => RgbSpace::DisplayP3,
        }
    }
    pub fn surface(self) -> layer_render_wgpu::SdrSurfaceColor {
        match self.base() {
            Self::Mapped { .. } => unreachable!(),
            Self::Srgb => layer_render_wgpu::SdrSurfaceColor::Srgb,
            Self::DisplayP3 => layer_render_wgpu::SdrSurfaceColor::DisplayP3,
        }
    }
    pub fn format(caps: &wgpu::SurfaceCapabilities) -> Result<wgpu::TextureFormat, String> {
        // Only pass-through lets us own a description with an unambiguous
        // transfer curve. WSI's legacy sRGB/P3 tags can mean gamma 2.2 instead.
        [
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureFormat::Bgra8UnormSrgb,
        ]
        .into_iter()
        .find(|&format| {
            caps.color_spaces(format)
                .contains(wgpu::SurfaceColorSpaces::PASS_THROUGH)
        })
        .ok_or_else(|| "The Wayland canvas requires Vulkan color pass-through".into())
    }
    pub fn description(self) -> &'static str {
        match self.base() {
            Self::Mapped { .. } => unreachable!(),
            Self::Srgb => {
                "sRGB fallback. Colors outside sRGB are clipped for viewing; document values remain intact."
            }
            Self::DisplayP3 => {
                "Managed Display P3. The compositor maps canvas and artwork controls to each monitor. Document values remain intact."
            }
        }
    }
    pub fn state(self) -> gdk::ColorState {
        match self.base() {
            Self::Mapped { .. } => unreachable!(),
            Self::Srgb => gdk::ColorState::srgb(),
            Self::DisplayP3 => {
                thread_local! {
                    static P3: gdk::ColorState = {
                        let params = gdk::CicpParams::new();
                        params.set_color_primaries(12); // P3 primaries, D65 white.
                        params.set_transfer_function(13); // sRGB transfer.
                        params.set_matrix_coefficients(0); // RGB, not YUV.
                        params.set_range(gdk::CicpRange::Full);
                        params.build_color_state().expect("GTK supports Display P3 CICP")
                    };
                }
                P3.with(Clone::clone)
            }
        }
    }
    pub fn texture(
        self,
        extent: [u32; 2],
        format: gdk::MemoryFormat,
        stride: usize,
        bytes: Vec<u8>,
    ) -> gdk::Texture {
        self.texture_bytes(extent, format, stride, glib::Bytes::from_owned(bytes))
    }
    pub fn texture_bytes(
        self,
        extent: [u32; 2],
        format: gdk::MemoryFormat,
        stride: usize,
        bytes: glib::Bytes,
    ) -> gdk::Texture {
        gdk::MemoryTextureBuilder::new()
            .set_width(extent[0] as i32)
            .set_height(extent[1] as i32)
            .set_format(format)
            .set_stride(stride)
            .set_color_state(&self.state())
            .set_bytes(Some(&bytes))
            .build()
    }
    pub fn rgba8(self, extent: [u32; 2], bytes: Vec<u8>) -> gdk::Texture {
        self.texture(
            extent,
            gdk::MemoryFormat::R8g8b8a8,
            extent[0] as usize * 4,
            bytes,
        )
    }
    /// Match the canvas: linear alpha-over-checker, then display encoding/clamp.
    pub fn checker_colors(self, color: RgbColor) -> [[f32; 4]; 2] {
        let linear = if let Self::Mapped { document, recipe, .. } = self {
            let rendition = layer_core::color::hdr::SdrRendition::from_parameters(recipe.map(f32::from_bits)).expect("validated rendition");
            let p = color.linear_in(document).expect("validated artwork color");
            let rgb = rendition.mapper(document, self.space()).map_rgb([p[0], p[1], p[2]]);
            [rgb[0], rgb[1], rgb[2], p[3]]
        } else { color.linear_in(self.space()).expect("validated artwork color") };
        [0.94, 0.80].map(|checker| {
            let mut rgba = [1.; 4];
            for c in 0..3 {
                rgba[c] = (self.space().encode(f64::from(
                    linear[c] * linear[3] + checker * (1. - linear[3]),
                )) as f32).clamp(0., 1.);
            }
            rgba
        })
    }
    pub fn solid(self, rgba: [f32; 4]) -> gdk::Texture {
        self.texture(
            [1, 1],
            // This is a display derivative only. A Float32 swatch can promote
            // GTK's much larger intermediate surfaces to Float32 too. Keep
            // source color definitions and canvas/native samples unchanged.
            gdk::MemoryFormat::R16g16b16a16Float,
            8,
            rgba.into_iter()
                .flat_map(|v| half::f16::from_f32(v).to_bits().to_ne_bytes())
                .collect(),
        )
    }
}

/// Small retained outlines avoid rasterizing a whole wheel for a moving marker.
pub(crate) fn marker_outline(snapshot: &gtk::Snapshot, point: [f32; 2], radius: f32) {
    for (outset, width, color) in [
        (2., 4., gdk::RGBA::new(0., 0., 0., 0.5)),
        (1., 2., gdk::RGBA::WHITE),
    ] {
        let r = radius + outset;
        let border = gtk::gsk::RoundedRect::from_rect(
            gtk::graphene::Rect::new(point[0]-r, point[1]-r, r*2., r*2.), r,
        );
        snapshot.append_border(&border, &[width; 4], &[color; 4]);
    }
}

pub(crate) fn append_solid(snapshot: &gtk::Snapshot, view: ViewColor, rgba: [f32; 4], bounds: &gtk::graphene::Rect) {
    let rgb = view.space().convert(RgbSpace::Srgb, [rgba[0], rgba[1], rgba[2]].map(f64::from));
    if rgb.iter().all(|c| (-0.000001..=1.000001).contains(c)) {
        let [r, g, b] = rgb.map(|c| c.clamp(0., 1.) as f32);
        snapshot.append_color(&gdk::RGBA::new(r, g, b, rgba[3]), bounds);
    } else {
        snapshot.append_texture(&view.solid(rgba), bounds);
    }
}
/// Opaque marker artwork. Colors within sRGB use a native color node, avoiding
/// fresh GPU images. Preserve tagged textures for colors outside sRGB and HDR.
pub(crate) fn append_picker_solid(snapshot: &gtk::Snapshot, view: ViewColor, headroom: f32,
    space: RgbSpace, pixel: [f32; 4], bounds: &gtk::graphene::Rect) {
    if headroom <= 1. {
        let color = RgbColor::from_linear(space, pixel).expect("validated picker color");
        append_solid(snapshot, view, view.checker_colors(color)[0], bounds);
    } else {
        snapshot.append_texture(&picker_texture(view, headroom, space, [1,1], &[pixel]), bounds);
    }
}

/// HDR picker artwork: Float32 evaluation followed by a half-float, tagged
/// display derivative. SDR-only displays use the document's saved rendition.
pub(crate) fn picker_texture(view: ViewColor, headroom: f32, space: RgbSpace,
    extent: [u32; 2], pixels: &[[f32; 4]]) -> gdk::Texture {
    picker_texture_with_gain(view, headroom, space, extent, pixels, 1.)
}
pub(crate) fn picker_texture_with_gain(view: ViewColor, headroom: f32, space: RgbSpace,
    extent: [u32; 2], pixels: &[[f32; 4]], gain: f64) -> gdk::Texture {
    picker_texture_data(view, headroom, space, extent, pixels, gain, None)
}
/// Apply geometric coverage after color mapping, without changing HDR exposure.
pub(crate) fn picker_texture_with_coverage(view: ViewColor, headroom: f32, space: RgbSpace,
    extent: [u32; 2], pixels: &[[f32; 4]], coverage: &[f32]) -> gdk::Texture {
    assert_eq!(pixels.len(), coverage.len());
    picker_texture_data(view, headroom, space, extent, pixels, 1., Some(coverage))
}
fn picker_texture_data(view: ViewColor, headroom: f32, space: RgbSpace,
    extent: [u32; 2], pixels: &[[f32; 4]], gain: f64, coverage: Option<&[f32]>) -> gdk::Texture {
    let document = if let ViewColor::Mapped { document, .. } = view { document } else { space };
    let to_document = space.linear_transform(document);
    let document_pixel = |p: [f32; 4]| {
        let rgb = layer_core::color::rgb::apply(to_document, [p[0],p[1],p[2]].map(f64::from));
        let rgb = rgb.map(|v| (v * gain).clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32);
        [rgb[0],rgb[1],rgb[2],p[3]]
    };
    if headroom > 1. {
        let to_srgb = document.linear_transform(RgbSpace::Srgb);
        let mut bytes = Vec::with_capacity(pixels.len() * 8);
        for (i, &p) in pixels.iter().enumerate() {
            let p = document_pixel(p);
            let rgb = hdr_display_rgb(p, headroom, to_srgb);
            let alpha = p[3] * coverage.map_or(1., |c| c[i]);
            for v in rgb.into_iter().map(|v| v.clamp(0., 10000. / 203.) as f32).chain([alpha]) {
                bytes.extend_from_slice(&half::f16::from_f32(v).to_bits().to_ne_bytes());
            }
        }
        hdr_texture(extent, bytes)
    } else {
        let rendition = if let ViewColor::Mapped { recipe, .. } = view {
            layer_core::color::hdr::SdrRendition::from_parameters(recipe.map(f32::from_bits)).expect("validated rendition")
        } else { Default::default() };
        let mapper = rendition.mapper(document, view.space());
        let mut bytes = Vec::with_capacity(pixels.len() * 4);
        for (i, p) in pixels.iter().enumerate() {
            let p = document_pixel(*p);
            let rgb = mapper.map_rgb([p[0],p[1],p[2]]).map(f64::from);
            bytes.extend(rgb.map(|v| (view.space().encode(v).clamp(0.,1.) * 255.).round() as u8));
            bytes.push((p[3] * coverage.map_or(1., |c| c[i]) * 255.).round() as u8);
        }
        view.rgba8(extent, bytes)
    }
}

fn hdr_display_rgb(p: [f32; 4], headroom: f32, to_srgb: layer_core::color::rgb::Matrix3) -> [f64; 3] {
    let p = layer_core::color::hdr::map_display_premultiplied(p, headroom);
    layer_core::color::rgb::apply(layer_core::color::hdr::srgb_to_bt2020(),
        layer_core::color::rgb::apply(to_srgb, [p[0], p[1], p[2]].map(f64::from)))
}
fn hdr_texture(extent: [u32; 2], bytes: Vec<u8>) -> gdk::Texture {
    gdk::MemoryTextureBuilder::new().set_width(extent[0] as i32).set_height(extent[1] as i32)
        .set_format(gdk::MemoryFormat::R16g16b16a16Float).set_stride(extent[0] as usize * 8)
        .set_color_state(&gdk::ColorState::rec2100_linear())
        .set_bytes(Some(&glib::Bytes::from_owned(bytes))).build()
}
/// Map straight artwork first, then composite it over the neutral checker in
/// linear display light, matching the canvas. Alpha must not change the shoulder.
pub(crate) fn checker_textures(color: RgbColor, view: ViewColor, headroom: f32) -> [gdk::Texture; 2] {
    if let ViewColor::Mapped { document, .. } = view && headroom > 1. {
        let mut p = color.linear_in(document).expect("validated artwork color");
        let alpha = p[3] as f64;
        p[3] = 1.;
        let rgb = hdr_display_rgb(p, headroom, document.linear_transform(RgbSpace::Srgb));
        [0.94, 0.80].map(|checker| {
            let bytes = rgb.into_iter().map(|v| ((v * alpha + checker * (1. - alpha)).clamp(0., 10000. / 203.)) as f32)
                .chain([1.]).flat_map(|v| half::f16::from_f32(v).to_bits().to_ne_bytes()).collect();
            hdr_texture([1, 1], bytes)
        })
    } else { view.checker_colors(color).map(|rgba| view.solid(rgba)) }
}

#[path = "color_pair.rs"]
mod pair;
pub use pair::ColorPair;

pub(crate) fn append_checker(snapshot: &gtk::Snapshot, bounds: gtk::graphene::Rect, radius: f32, textures: &[gdk::Texture; 2]) {
    checker(snapshot, bounds, radius, |index, bounds| snapshot.append_texture(&textures[index], bounds));
}
fn checker(snapshot: &gtk::Snapshot, bounds: gtk::graphene::Rect, radius: f32,
    paint: impl Fn(usize, &gtk::graphene::Rect)) {
    snapshot.push_rounded_clip(&gtk::gsk::RoundedRect::from_rect(bounds, radius));
    // An opaque base avoids alpha seams at fractional checker edges.
    paint(0, &bounds);
    for y in 0..(bounds.height() / 8.).ceil() as i32 {
        for x in 0..(bounds.width() / 8.).ceil() as i32 {
            if (x + y) % 2 == 0 { continue; }
            paint(1, &gtk::graphene::Rect::new(
                bounds.x() + (x * 8) as f32, bounds.y() + (y * 8) as f32, 8., 8.,
            ));
        }
    }
    snapshot.pop();
}

mod patch {
    use super::*;
    #[derive(Default)]
    pub struct Patch {
        pub round: Cell<bool>,
        pub key: Cell<Option<(RgbColor, ViewColor, f32)>>,
        pub textures: RefCell<Option<[gdk::Texture; 2]>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for Patch {
        const NAME: &'static str = "CapyManagedColorPatch";
        type Type = super::ColorPatch;
        type ParentType = gtk::Widget;
    }
    impl ObjectImpl for Patch {}
    impl WidgetImpl for Patch {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let bounds = gtk::graphene::Rect::new(0., 0., obj.width() as f32, obj.height() as f32);
            let Some(textures) = self.textures.borrow().clone() else {
                return;
            };
            let radius = if self.round.get() {
                bounds.width().min(bounds.height()) * 0.5
            } else { 0. };
            if let Some((color, view, headroom)) = self.key.get()
                && headroom <= 1. {
                let colors = view.checker_colors(color);
                checker(snapshot, bounds, radius, |index, bounds| append_solid(snapshot, view, colors[index], bounds));
            } else {
                append_checker(snapshot, bounds, radius, &textures);
            }
        }
    }
}
glib::wrapper! {
    pub struct ColorPatch(ObjectSubclass<patch::Patch>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}
impl ColorPatch {
    pub fn new(round: bool) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().round.set(round);
        obj.set_can_target(false);
        obj
    }
    pub fn set_color(&self, color: RgbColor, view: ViewColor) {
        self.set_display_color(color, view, 1.);
    }
    pub fn set_display_color(&self, color: RgbColor, view: ViewColor, headroom: f32) {
        if self.imp().key.replace(Some((color, view, headroom))) == Some((color, view, headroom)) {
            return;
        }
        let textures = checker_textures(color, view, headroom);
        *self.imp().textures.borrow_mut() = Some(textures);
        self.queue_draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "isolated Wayland display and GTK GPU renderer"]
    fn native_solid_colors_match_tagged_textures() {
        gtk::init().unwrap();
        let window = gtk::Window::new();
        window.set_default_size(64, 64);
        window.present();
        let renderer = window.renderer().unwrap();
        let bounds = gtk::graphene::Rect::new(0., 0., 16., 16.);
        let check = |native: gtk::Snapshot, reference: gdk::Texture, color_node: Option<bool>| {
            let node = native.to_node().unwrap();
            if let Some(expected) = color_node {
                assert_eq!(node.downcast_ref::<gtk::gsk::ColorNode>().is_some(), expected);
            }
            let pair = gtk::Snapshot::new();
            pair.append_node(&node);
            pair.append_texture(&reference, &gtk::graphene::Rect::new(16., 0., 16., 16.));
            let rendered = renderer.render_texture(&pair.to_node().unwrap(), None);
            let mut pixels = vec![0; 32*16*4];
            rendered.download(&mut pixels, 32*4);
            for c in 0..4 {
                let native = pixels[(8*32+8)*4+c];
                let reference = pixels[(8*32+24)*4+c];
                assert!(native.abs_diff(reference) <= 2, "channel {c}: {native} vs {reference}");
            }
        };
        for view in [ViewColor::Srgb, ViewColor::DisplayP3] {
            for (rgba, inside_srgb) in [([0.,1.,0.,1.], false), ([1.,0.05,0.1,1.], false),
                ([0.3,0.4,0.7,0.4], true), ([0.01,0.02,0.03,1.], true)] {
                let native = gtk::Snapshot::new();
                append_solid(&native, view, rgba, &bounds);
                check(native, view.solid(rgba), Some(view == ViewColor::Srgb || inside_srgb));
            }
            let document = layer_core::color::DocumentColor { space: RgbSpace::DisplayP3,
                depth: layer_core::color::SampleDepth::F16, ..Default::default() };
            let view = view.with_rendition(document, Default::default());
            for pixel in [[4.,0.2,0.1,1.], [-0.2,0.3,1.7,1.], [0.03,0.07,0.1,1.]] {
                let native = gtk::Snapshot::new();
                append_picker_solid(&native, view, 1., document.space, pixel, &bounds);
                check(native, picker_texture(view, 1., document.space, [1,1], &[pixel]), None);
            }
        }
        window.destroy();
    }
    #[test]
    fn gtk_color_management_is_a_valid_debug_flag() {
        for (existing, expected) in [
            ("", "color-mgmt"),
            ("no-portals", "no-portals:color-mgmt"),
            ("no-portals;color-mgmt", "no-portals;color-mgmt"),
            ("COLOR-MGMT", "COLOR-MGMT"),
        ] {
            assert_eq!(
                with_gtk_color_management(OsStr::new(existing)),
                OsStr::new(expected)
            );
        }
    }

    #[test]
    fn selection_requires_a_matching_passthrough_format() {
        let caps = |entries| wgpu::SurfaceCapabilities {
            format_capabilities: entries,
            ..Default::default()
        };
        let pair = |format, color_spaces| wgpu::SurfaceFormatCapabilities {
            format,
            color_spaces,
        };
        let pass = wgpu::SurfaceColorSpaces::PASS_THROUGH;
        let srgb = wgpu::SurfaceColorSpaces::SRGB;
        assert!(ViewColor::format(&caps(vec![])).is_err());
        assert!(
            ViewColor::format(&caps(vec![pair(wgpu::TextureFormat::Rgba8Unorm, srgb)])).is_err()
        );
        assert!(
            ViewColor::format(&caps(vec![pair(wgpu::TextureFormat::Rgb10a2Unorm, pass)])).is_err()
        );
        assert_eq!(
            ViewColor::format(&caps(vec![
                pair(wgpu::TextureFormat::Rgba8Unorm, srgb),
                pair(wgpu::TextureFormat::Bgra8UnormSrgb, pass),
            ]))
            .unwrap(),
            wgpu::TextureFormat::Bgra8UnormSrgb
        );
        assert_eq!(
            ViewColor::format(&caps(vec![
                pair(wgpu::TextureFormat::Bgra8UnormSrgb, pass),
                pair(wgpu::TextureFormat::Rgba8Unorm, pass),
            ]))
            .unwrap(),
            wgpu::TextureFormat::Rgba8Unorm
        );
    }
}

/// Surface descriptions stay fixed while Wayland handles monitor transforms.
/// Only the informational native view state changes on a monitor move.
pub(crate) fn bind_monitor_updates(w: &std::rc::Rc<crate::workspace::Workspace>) {
    w.window.connect_realize(glib::clone!(
        #[weak]
        w,
        move |window| {
            let Some(surface) = window.surface() else {
                return;
            };
            let refresh = glib::clone!(
                #[weak]
                w,
                move || {
                    glib::idle_add_local_once(glib::clone!(
                        #[weak]
                        w,
                        move || w.changed(Ok(layer_ui::UiChange {
                            regions: layer_ui::regions::SETTINGS,
                            ..Default::default()
                        }))
                    ));
                }
            );
            surface.connect_enter_monitor({
                let refresh = refresh.clone();
                move |_, _| refresh()
            });
            surface.connect_leave_monitor(move |_, _| refresh());
        }
    ));
}
