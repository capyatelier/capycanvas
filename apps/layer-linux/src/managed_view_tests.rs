//! Native color-state transport, canvas/control agreement and fallback checks.
use super::new_photo::{capture_ui, ready};
use super::*;
use crate::display_color::{ColorPatch, ViewColor};
use layer_core::color::{ColorProfile, DocumentColor, SampleDepth, RgbColor, RgbSpace, source::*};

fn download(texture: &gdk::Texture, state: &gdk::ColorState) -> Vec<[f32; 4]> {
    let mut downloader = gdk::TextureDownloader::new(texture);
    downloader.set_color_state(state);
    downloader.set_format(gdk::MemoryFormat::R32g32b32a32Float);
    let (bytes, stride) = downloader.download_bytes();
    let mut output = vec![];
    for y in 0..texture.height() as usize {
        for pixel in bytes[y * stride..][..texture.width() as usize * 16].chunks_exact(16) {
            output.push(std::array::from_fn(|c| {
                f32::from_ne_bytes(pixel[c * 4..c * 4 + 4].try_into().unwrap())
            }));
        }
    }
    output
}
#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_managed_canvas_and_gtk_artwork_agree() {
    glib::set_prgname(Some("capy-canvas-test"));
    let app = native_test_app("art.capycanvas.ManagedView");
    for space in RgbSpace::ALL {
        for rgba in [
            [0.85, 0.35, 0.2, 1.],
            [0.2, 0.7, 0.3, 0.5],
            [0., 0., 0., 0.],
            [1.; 4],
            [0.50024, 0.04044, 0.99975, 0.0001],
        ] {
            let color = RgbColor::new(space, rgba).unwrap();
            let p3 = color.encoded_in(RgbSpace::DisplayP3).unwrap();
            if p3[..3].iter().any(|v| !(0. ..=1.).contains(v)) {
                continue;
            }
            let texture = ViewColor::DisplayP3.solid(p3);
            let pixels = download(&texture, &gdk::ColorState::srgb_linear());
            let expected = color.linear_in(RgbSpace::Srgb).unwrap();
            for c in 0..4 {
                assert!(
                    // Display-only half-float codes have at most 2^-12 encoded
                    // error in [0, 1]. The sRGB derivative and P3-to-sRGB
                    // matrix amplify this to < 0.001 in linear RGB. Document
                    // sample precision is tested independently in the renderer;
                    // the canvas/control display comparison below remains two
                    // U8 codes, including a patch outside the sRGB gamut.
                    (pixels[0][c] - expected[c]).abs() < 0.001,
                    "GDK {space:?} {rgba:?}: {:?} expected {expected:?}",
                    pixels[0]
                );
            }
        }
    }
    // An in-sRGB patch cannot detect an accidental sRGB8 intermediate in GSK.
    let color = RgbColor::new(RgbSpace::DisplayP3, [1., 0.1, 0.02, 1.]).unwrap();
    let mut project = new_drawing(64, 64).unwrap();
    project.document.color = DocumentColor {
        space: color.space,
        depth: SampleDepth::U16,
    };
    let mut source = SourceBuilder::new(
        [64, 64],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(color.space),
            profile_assumed: false,
        },
        1024 * 1024,
    )
    .unwrap();
    let codes = color.rgba.map(|v| (v * 65535.).round() as u16);
    let row: Vec<_> = (0..64)
        .flat_map(|_| codes.into_iter().flat_map(u16::to_le_bytes))
        .collect();
    for _ in 0..64 {
        source.push_row(&row).unwrap();
    }
    project.document.layers[0].source = Some(std::sync::Arc::new(source.finish().unwrap()));
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.present();
    ready(&w);
    let renderer = w.window.renderer().unwrap().type_().name().to_string();
    eprintln!("GTK artwork renderer: {renderer}");
    let expected_renderer = match std::env::var("GSK_RENDERER").as_deref() {
        Ok("vulkan") => Some("GskVulkanRenderer"),
        Ok("gl" | "ngl") => Some("GskGLRenderer"),
        Ok("cairo") => Some("GskCairoRenderer"),
        _ => None,
    };
    if let Some(expected) = expected_renderer { assert_eq!(renderer, expected); }
    let view = w.view_color();
    assert_eq!(
        view,
        if std::env::var_os("LAYER_TEST_VIEW_SRGB").is_some() {
            ViewColor::Srgb
        } else {
            ViewColor::DisplayP3
        }
    );
    let original = super::place_source::snapshot(&w);
    // GTK 4.22.4's Cairo render_texture() always draws into sRGB ARGB32;
    // its on-screen renderer instead uses the surface's color state. Compare
    // this snapshot in its actual domain, without claiming a P3 screen check.
    // https://github.com/GNOME/gtk/blob/4.22.4/gsk/gskcairorenderer.c
    let capture_view = if renderer == "GskCairoRenderer" {
        eprintln!("Cairo snapshot checks sRGB only; on-screen P3 remains unqualified");
        ViewColor::Srgb
    } else {
        view
    };
    w.dispatch(UiAction::Color {
        action: ColorAction::SetSlot {
            slot: ColorSlot::Foreground,
            color,
        },
    });
    pump(200);
    let capture = w
        .gpu
        .borrow()
        .as_ref()
        .unwrap()
        .session
        .engine()
        .backend()
        .capture_in(capture_view)
        .unwrap();
    let m = state(&w).camera.document_to_surface();
    let x = (m[0] * 32. + m[2] * 32. + m[4]).round() as usize;
    let y = (m[1] * 32. + m[3] * 32. + m[5]).round() as usize;
    let canvas = &capture.bytes[y * capture.stride as usize + x * 4..][..4];
    let expected = color
        .encoded_in(capture_view.space())
        .unwrap()
        .map(|v| v.clamp(0., 1.));
    for c in 0..4 {
        assert!(
            (canvas[c] as f32 / 255. - expected[c]).abs() <= 1.5 / 255.,
            "canvas {canvas:?}"
        );
    }

    // Render a real GTK artwork widget through GSK, then download in the
    // explicit capture state instead of interpreting untagged screenshot bytes.
    let patch = ColorPatch::new(false);
    patch.set_size_request(40, 40);
    patch.set_color(color, view);
    let window = gtk::Window::builder()
        .application(&*app)
        .child(&patch)
        .build();
    window.present();
    pump(600);
    let snapshot = gtk::Snapshot::new();
    gtk::WidgetPaintable::new(Some(&patch)).snapshot(&snapshot, 40., 40.);
    let node = snapshot.to_node().unwrap();
    let texture = window
        .renderer()
        .unwrap()
        .render_texture(&node, Some(&gtk::graphene::Rect::new(0., 0., 40., 40.)));
    if renderer == "GskCairoRenderer" {
        assert_eq!(texture.color_state(), gdk::ColorState::srgb());
    }
    let pixels = download(&texture, &capture_view.state());
    for c in 0..4 {
        assert!(
            (pixels[20 * 40 + 20][c] - canvas[c] as f32 / 255.).abs() <= 2. / 255.,
            "GTK {:?}, canvas {canvas:?}",
            pixels[20 * 40 + 20]
        );
    }
    window.destroy();
    let output = std::path::PathBuf::from(format!(
        "../../artifacts/color-m2/managed-view-ui/{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&output).unwrap();
    capture_ui(&w, &output, "p3-canvas-and-picker.png");
    assert_eq!(super::place_source::snapshot(&w), original);
    w.window.destroy();
    pump(100);
}
