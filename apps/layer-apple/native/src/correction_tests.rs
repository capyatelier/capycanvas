//! Retained photo corrections through the actual Apple owner, worker and Metal.
use super::*;
use layer_core::{
    Document,
    color::{ColorProfile, SampleDepth, RgbSpace, source::*},
};
use std::{
    io::Seek,
    os::fd::AsRawFd,
};

const CORRECTIONS: [(&str, &str, f64, Option<&str>); 6] = [
    ("exposure", "exposure", 0.75, None),
    ("white_balance", "temperature", 25., None),
    ("levels", "gamma", 0.9, None),
    ("curves", "rgb", 0., None),
    ("hue_saturation", "hue", 10., None),
    ("color_balance", "midtones_red", 12., Some("midtones")),
];
fn document(app: &App) -> Document {
    unsafe { &*app.0 }.host.session.engine().document().clone()
}
fn samples(app: &App) -> Vec<[f32; 4]> {
    let s = &unsafe { &*app.0 }.host.session;
    let project = s.capture_artwork().unwrap();
    let mut snapshot = s
        .engine()
        .backend()
        .0
        .as_ref()
        .unwrap()
        .snapshot_gpu()
        .capture(
            project,
            Default::default(),
        )
        .unwrap();
    snapshot.read_region([0, 0, 128, 96]).unwrap()
}
// Neutral color conversions may round by a few Float32 ULPs. History and
// persistence still compare exactly; report one differing sample, not the image.
fn check_samples(actual: Vec<[f32; 4]>, expected: &[[f32; 4]], tolerance: f32, context: &str) {
    assert_eq!(actual.len(), expected.len());
    for (pixel, (a, b)) in actual.iter().zip(expected).enumerate() {
        for channel in 0..4 {
            assert!(
                a[channel].is_finite() && (a[channel] - b[channel]).abs() <= tolerance,
                "{context}: pixel {pixel} channel {channel}: {} != {} (tolerance {tolerance})",
                a[channel],
                b[channel]
            );
        }
    }
}
fn check_document(app: &App, expected: Document) {
    let actual = document(app);
    assert_saved_document(&actual, &expected);
}
fn set(app: &App, layer: u64, key: &str, value: Value) {
    app.action(
        json!({"type":"effect","action":{"op":"set","layer":layer,"key":key,"value":value}}),
    );
    app.draw_until_idle();
}
fn value(index: usize, alternate: bool) -> Value {
    let (_, key, amount, _) = CORRECTIONS[index];
    if key == "rgb" {
        json!({"kind":"curve","value": if alternate { json!([[0.,0.],[0.4,0.7],[1.,1.]]) }
            else { json!([[0.,0.],[0.213,0.13],[0.79,0.9],[1.,1.]]) }})
    } else {
        json!({"kind":"number","value":if !alternate { amount } else if key == "gamma" { 1.2 } else { -amount }})
    }
}
fn select_region(app: &App) {
    app.invoke("lasso");
    app.draw_until_idle();
    let [a, b, c, d, e, f] = unsafe { &*app.0 }
        .host
        .session
        .state()
        .camera
        .document_to_surface();
    let records: Vec<_> = [[8., 8.], [60., 8.], [60., 88.], [8., 88.], [8., 8.]]
        .into_iter()
        .enumerate()
        .flat_map(|(i, [x, y])| {
            [
                f64::from(a * x + c * y + e),
                f64::from(b * x + d * y + f),
                1.,
                0.,
                0.,
                0.,
                0.,
                1_000_000_000. + i as f64 * 10_000_000.,
                if i == 0 {
                    1.
                } else if i == 4 {
                    3.
                } else {
                    2.
                },
            ]
        })
        .collect();
    assert_eq!(
        unsafe {
            capy_apple_pointer(
                app.0,
                1,
                0,
                0,
                records.as_ptr(),
                records.len(),
                0,
                capy_apple_camera_revision(app.0),
            )
        },
        0
    );
    app.draw_until_idle();
    assert!(
        document(app).working.selection.is_some(),
        "Native lasso must create a local mask region"
    );
}

#[test]
fn apple_photo_corrections_masks_and_original_samples_remain_revisable_after_worker_reopen() {
    for platform in [0, 1] {
        for (space, depth) in [
            (RgbSpace::DisplayP3, SampleDepth::U8),
            (RgbSpace::ProPhoto, SampleDepth::U16),
        ] {
            let app = App::new(platform);
            unsafe { &mut *app.0 }.host.session.renderer_mut().0 = Some(native_renderer());
            app.draw_until_idle();
            let new = ProjectJob::new(&app, true);
            let options = CString::new(json!({"extent":[128,96],"color":{"space":space,"depth":depth},"background":"Transparent"}).to_string()).unwrap();
            assert_eq!(unsafe { capy_project_new(new.0, options.as_ptr()) }, 0);
            assert_eq!(
                unsafe {
                    capy_apple_project_adopt(app.0, new.0, c"Retained photo".as_ptr(), c"".as_ptr())
                },
                0
            );
            let mut builder = SourceBuilder::new(
                [257, 129],
                SourceInterpretation {
                    channels: SourceChannels::Rgba,
                    depth: SampleDepth::U16,
                    profile: ColorProfile::Builtin(space),
                    profile_assumed: false,
                },
                4 * 1024 * 1024,
            )
            .unwrap();
            for y in 0..129u32 {
                let row: Vec<_> = (0..257u32)
                    .flat_map(|x| {
                        [
                            8000 + x * 137,
                            11000 + y * 191,
                            18000 + (x * 67 + y * 89) % 26000,
                            if x % 7 == 0 { 32768 } else { 65535 },
                        ]
                        .into_iter()
                        .flat_map(|v| (v as u16).to_le_bytes())
                    })
                    .collect();
                builder.push_row(&row).unwrap();
            }
            unsafe { &mut *app.0 }
                .host
                .session
                .import_layer_source("Original 16-bit photograph", builder.finish().unwrap())
                .unwrap();
            app.draw_until_idle();
            let original = document(&app);
            let original_occurrence = original.working.occurrence.unwrap();
            let original_id = original.artwork.occurrences.id(original_occurrence).unwrap();
            let source = original.scene().paint_source(original_occurrence).unwrap().original.as_ref().unwrap();
            let mut ids = Vec::new();
            for (index, (effect, key, _, page)) in CORRECTIONS.into_iter().enumerate() {
                let before = samples(&app);
                app.action(json!({"type":"effect","action":{"op":"insert","effect":effect}}));
                app.draw_until_idle();
                let effect_document = document(&app);
                let selected = effect_document.working.occurrence.unwrap();
                let layer = layer_ui::occurrence_token(selected);
                ids.push(effect_document.artwork.occurrences.id(selected).unwrap());
                if let Some(page) = page {
                    app.action(json!({"type":"effect","action":{"op":"select_page","layer":layer,"page":page}}));
                }
                let controls = app.state()["layer_properties"]["controls"]
                    .as_array()
                    .unwrap()
                    .clone();
                assert!(controls.iter().any(|c| c["key"] == key));
                let neutral = samples(&app);
                check_samples(
                    neutral.clone(),
                    &before,
                    1e-6,
                    &format!("{effect}: neutral correction"),
                );
                set(&app, layer, key, value(index, false));
                let edited = samples(&app);
                assert!(edited != before, "{effect}: edit must change composite");
                app.invoke("undo");
                app.draw_until_idle();
                check_samples(samples(&app), &neutral, 0., "Undo property");
                app.invoke("redo");
                app.draw_until_idle();
                check_samples(samples(&app), &edited, 0., "Restore correction");
                app.action(
                    json!({"type":"effect","action":{"op":"reset","layer":layer,"key":key}}),
                );
                app.draw_until_idle();
                check_samples(samples(&app), &neutral, 0., &format!("{effect}: reset"));
                app.invoke("undo");
                app.draw_until_idle();
                check_samples(samples(&app), &edited, 0., "Restore correction");
                app.layer_action(json!({"op":"visibility","id":layer,"value":false}));
                app.draw_until_idle();
                check_samples(samples(&app), &before, 0., "Bypass");
                app.layer_action(json!({"op":"visibility","id":layer,"value":true}));
                app.draw_until_idle();
                check_samples(samples(&app), &edited, 0., "Restore correction");
                app.layer_action(json!({"op":"add_mask","id":layer,"replace":false}));
                app.draw_until_idle();
                select_region(&app);
                app.layer_action(json!({"op":"mask_selection","id":layer,"hide":true}));
                app.draw_until_idle();
                assert!(
                    document(&app).working.selection.is_none(),
                    "Mask from selection consumes the selection"
                );
                let masked = samples(&app);
                assert!(masked != edited, "{effect}: local mask");
                assert_eq!(
                    masked[40 * 128 + 100],
                    edited[40 * 128 + 100],
                    "Mask preserves pixels outside the selection"
                );
                app.layer_action(json!({"op":"select","id":layer,"mask":false}));
                app.draw_until_idle();
            }
            let edited = document(&app);
            let before = samples(&app);
            assert_eq!(
                edited.scene().order().iter()
                    .filter(|h| edited.scene().effect(**h).is_some() && edited.scene().mask(**h).is_some())
                    .count(),
                6
            );
            let mut file = fixtures::tempfile();
            let save = ProjectJob::new(&app, false);
            assert_eq!(
                unsafe { capy_project_write(save.0, file.as_raw_fd()) },
                0,
                "{:?}",
                save.error()
            );
            let fresh = App::new(platform);
            unsafe { &mut *fresh.0 }.host.session.renderer_mut().0 = Some(native_renderer());
            fresh.draw_until_idle();
            let open = ProjectJob::new(&fresh, true);
            file.rewind().unwrap();
            assert_eq!(
                unsafe {
                    capy_project_read(open.0, file.as_raw_fd(), c"Corrections.capy".as_ptr())
                },
                0,
                "{:?}",
                open.error()
            );
            assert_eq!(
                unsafe {
                    capy_apple_project_adopt(fresh.0, open.0, c"Corrections".as_ptr(), c"".as_ptr())
                },
                0
            );
            fresh.draw_until_idle();
            check_document(&fresh, edited);
            check_samples(samples(&fresh), &before, 0., "Reopened correction/history");
            for (index, id) in ids.into_iter().enumerate() {
                let layer = layer_ui::occurrence_token(document(&fresh).artwork.occurrences.resolve(id).unwrap());
                fresh.layer_action(json!({"op":"select","id":layer,"mask":false}));
                fresh.draw_until_idle();
                let key = CORRECTIONS[index].1;
                set(&fresh, layer, key, value(index, true));
                assert!(
                    samples(&fresh) != before,
                    "Reopened {} stays editable",
                    CORRECTIONS[index].0
                );
                fresh.invoke("undo");
                fresh.draw_until_idle();
                check_samples(samples(&fresh), &before, 0., "Reopened correction/history");
                fresh.invoke("redo");
                fresh.draw_until_idle();
                assert!(samples(&fresh) != before);
                set(&fresh, layer, key, value(index, false));
                check_samples(samples(&fresh), &before, 0., "Reopened correction/history");
                fresh.layer_action(json!({"op":"invert_mask","id":layer}));
                fresh.draw_until_idle();
                assert!(samples(&fresh) != before);
                fresh.invoke("undo");
                fresh.draw_until_idle();
                check_samples(samples(&fresh), &before, 0., "Reopened correction/history");
            }
            let after = document(&fresh);
            let original_occurrence = after.artwork.occurrences.resolve(original_id).unwrap();
            let paint = after.scene().paint_source(original_occurrence).unwrap();
            let retained = paint.original.as_ref().unwrap();
            assert_source_samples(retained, source);
            assert!(paint.raster.is_empty(), "Corrections must not bake the retained photo");
        }
    }
}
