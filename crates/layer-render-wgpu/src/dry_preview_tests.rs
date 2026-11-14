use super::*;
use crate::{
    layer_tests::{dab, preset_style, page_bytes},
    test_support::{packet, dab_batch, submit_ready},
};
use layer_core::{Document, DefaultBrushPreset, DocumentNames, Point, PortableId, Rect};

fn contact(x: f32, radius: f32) -> Dab {
    Dab {
        center: Point { x, y: 128. },
        radii: [radius; 2],
        previous: [radius, radius, 1., 0.],
        motion: [4., 0.],
        contact: [0.7, 0., 0., 0.],
        previous_contact: [0.7, 0., 0., 0.],
        ..dab([0.1, 0.1, 0.1, 1.])
    }
}

fn document() -> Document {
    Document::new(
        PortableId::random(),
        1024,
        256,
        DocumentNames {
            paint: "Ink".into(),
            paper: "Paper".into(),
        },
    )
}

#[test]
fn dry_preview_end_owns_its_contact_before_main_only_dabs() {
    use layer_core::BrushBlendMode;
    use layer_render::DabPreview;
    for blend in [BrushBlendMode::Normal, BrushBlendMode::Multiply] {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let doc = document();
        for preset in [
            DefaultBrushPreset::Pencil,
            DefaultBrushPreset::Chalk,
            DefaultBrushPreset::PastelBlock,
        ] {
            let mut style = preset_style(preset);
            style.rendering.blend_mode = blend;
            for radius in [3f32, 20.] {
                for angle in [0f32, 0.7] {
                    let (sin, cos) = angle.sin_cos();
                    let mut body = contact(128., radius);
                    body.radii = [radius, 0.7 * radius];
                    body.rotation = [cos, sin];
                    body.previous = [radius, 0.7 * radius, cos, sin];
                    body.motion = [radius * cos, radius * sin];
                    body.flow = 0.1;
                    let end = body.completion([-cos, -sin], [0.; 2], 1.);
                    let mut landing = body;
                    landing.radii = body.radii.map(|v| v * 1.4);
                    landing.previous = [landing.radii[0], landing.radii[1], cos, sin];
                    landing.center.x -= 0.5 * radius * cos;
                    landing.center.y -= 0.5 * radius * sin;
                    landing.motion = [0.; 2];
                    landing.flow = 0.25;
                    landing.contact = [0.9, 0., 0., 0.];
                    landing.previous_contact = landing.contact;
                    let mut main = body;
                    main.radii = body.radii.map(|v| v * 2.);
                    main.previous = [main.radii[0], main.radii[1], cos, sin];
                    main.contact = [1., 0., 0., 0.];
                    main.previous_contact = main.contact;
                    main.flow = 1.;
                    let mut render = |dabs: &[Dab]| {
                        let damage = dabs
                            .iter()
                            .fold(Rect::EMPTY, |damage, dab| damage.union(dab.bounds()));
                        let mut batch =
                            dab_batch(doc.working.target.unwrap(), style.clone(), damage);
                        batch.kind = DabBatchKind::Preview;
                        batch.dab_count = dabs.len() as u32;
                        batch.preview_end = dabs
                            .iter()
                            .rposition(|dab| {
                                dab.is_completion() && dab.previous_contact[2..] == [0.; 2]
                            })
                            .map(|i| i as u32);
                        submit_ready(
                            &mut r,
                            FramePacket {
                                dabs,
                                dab_batches: &[batch],
                                ..packet(doc.scene(), [1024, 256])
                            },
                        )
                        .unwrap();
                        r.preview_pages
                            .iter()
                            .map(|page| {
                                let expected = r.preview_expected(doc.working.target.unwrap(), layer_core::raster::RasterPlane::Color, page.coordinate, scene::PreviewStage::Final).unwrap();
                                let read = page.active().read(expected, None).unwrap();
                                assert_eq!(
                                    read.value().texture.format(),
                                    wgpu::TextureFormat::Rgba32Float
                                );
                                (page.coordinate, page_bytes(&r, &read.value().texture))
                            })
                            .collect::<Vec<_>>()
                    };
                    let frozen = render(&[body, landing, end]);
                    let moving = render(&[
                        body,
                        landing.with_preview_tag(DabPreview::LiftOnly),
                        end,
                        main.with_preview_tag(DabPreview::MainOnly),
                    ]);
                    let ordinary = render(&[body, landing, end, main]);
                    let without_landing =
                        render(&[body, end, main.with_preview_tag(DabPreview::MainOnly)]);
                    let base = render(&[body, end]);
                    let mut contact = [end.into()];
                    dry_material::prepare_geometry(&style, &mut contact);
                    let contact = contact[0];
                    let (
                        mut owned,
                        mut excluded,
                        mut outside,
                        mut landing_inside,
                        mut landing_outside,
                    ) = (0, 0, 0, 0, 0);
                    assert_eq!(frozen.len(), moving.len());
                    assert_eq!(frozen.len(), ordinary.len());
                    assert_eq!(frozen.len(), without_landing.len());
                    assert_eq!(frozen.len(), base.len());
                    for (page, (coordinate, expected)) in frozen.iter().enumerate() {
                        let (actual_coordinate, actual) = &moving[page];
                        let (ordinary_coordinate, unowned) = &ordinary[page];
                        let (without_coordinate, without) = &without_landing[page];
                        let (base_coordinate, base) = &base[page];
                        assert_eq!(coordinate, actual_coordinate);
                        assert_eq!(coordinate, ordinary_coordinate);
                        assert_eq!(coordinate, without_coordinate);
                        assert_eq!(coordinate, base_coordinate);
                        for (index, ((expected, actual), unowned)) in expected
                            .chunks_exact(16)
                            .zip(actual.chunks_exact(16))
                            .zip(unowned.chunks_exact(16))
                            .enumerate()
                        {
                            let without = &without[index * 16..index * 16 + 16];
                            let base = &base[index * 16..index * 16 + 16];
                            let delta = [
                                (coordinate[0] * 256 + (index % 256) as u32) as f32 + 0.5
                                    - end.center.x,
                                (coordinate[1] * 256 + (index / 256) as u32) as f32 + 0.5
                                    - end.center.y,
                            ];
                            let o = [
                                delta[0] * contact.metric[0] + delta[1] * contact.metric[1],
                                delta[0] * contact.metric[2] + delta[1] * contact.metric[3],
                            ];
                            let q = if style.contact.unwrap().dry.unwrap().corner > 0. {
                                let rim = [contact.dab.material[1], contact.dab.material[2]];
                                let edge = [
                                    (o[0].abs() - (1. - rim[0])).max(0.) / rim[0],
                                    (o[1].abs() - (1. - rim[1])).max(0.) / rim[1],
                                ];
                                (edge[0] + edge[1]).powi(2)
                            } else {
                                o[0] * o[0] + o[1] * o[1]
                            };
                            if q <= 1. {
                                owned += 1;
                                assert_eq!(
                                    actual, expected,
                                    "{preset:?} {blend:?} radius={radius} angle={angle}: owned pixel {index}"
                                );
                                excluded += usize::from(unowned != expected);
                                landing_inside += usize::from(expected != base);
                            } else {
                                assert_eq!(
                                    actual, without,
                                    "{preset:?} {blend:?} radius={radius} angle={angle}: LiftOnly excludes unowned pixel {index}"
                                );
                                outside += usize::from(without != base);
                                landing_outside += usize::from(expected != base);
                            }
                        }
                    }
                    assert!(
                        owned > 0 && excluded > 0,
                        "{preset:?} {blend:?}: ordinary Main changes the owned contact"
                    );
                    assert!(
                        outside > 0,
                        "{preset:?} {blend:?}: MainOnly retains its contribution outside the contact"
                    );
                    assert!(
                        landing_inside > 0 && landing_outside > 0,
                        "{preset:?} {blend:?}: unmatched landing body paints both sides without ownership"
                    );
                }
            }
        }
    }
}

#[test]
fn unchanged_dry_preview_keeps_pages_and_encodes_no_gpu_work() {
    for radius in [3., 20.] {
        let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let doc = document();
        let target = doc.working.target.unwrap();
        let body = contact(128., radius);
        let mut persistent = dab_batch(
            target,
            preset_style(DefaultBrushPreset::Pencil),
            body.bounds(),
        );
        persistent.stroke_end = false;
        submit_ready(
            &mut r,
            FramePacket {
                dabs: &[body],
                dab_batches: &[persistent.clone()],
                ..packet(doc.scene(), [1024, 256])
            },
        )
        .unwrap();
        let cap = body.completion([-1., 0.], [0.; 2], 1.);
        let mut preview = persistent.clone();
        preview.kind = DabBatchKind::Preview;
        preview.stroke_start = false;
        preview.damage = cap.bounds();
        preview.preview_end = Some(0);
        let submit = |r: &mut WgpuRasterizer, dabs: &[Dab], batches: &[DabBatch], time| {
            submit_ready(
                r,
                FramePacket {
                    dabs,
                    dab_batches: batches,
                    time_seconds: time,
                    composite_all: false,
                    ..packet(doc.scene(), [1024, 256])
                },
            )
            .unwrap()
        };
        submit(&mut r, &[cap], &[preview.clone()], 0.);
        assert!(r.preview_valid);
        let pages = |r: &WgpuRasterizer| {
            r.preview_pages
                .iter()
                .map(|p| {
                    let expected = r.preview_expected(target, layer_core::raster::RasterPlane::Color, p.coordinate, scene::PreviewStage::Final).unwrap();
                    (p.coordinate, page_bytes(r, &p.active().read(expected, None).unwrap().value().texture))
                })
                .collect::<Vec<_>>()
        };
        let expected = pages(&r);
        let count = r.metrics.command_passes;
        assert!(count > 0);
        for i in 1..=8 {
            submit(&mut r, &[cap], &[preview.clone()], i as f32 * 0.016);
            assert_eq!(
                r.metrics.command_passes, count,
                "radius={radius}, frame={i}"
            );
            assert_eq!(pages(&r), expected);
        }
        let mut changed = cap;
        changed.contact[0] = 0.8;
        submit(&mut r, &[changed], &[preview.clone()], 0.2);
        assert!(r.metrics.command_passes > count);
        let count = r.metrics.command_passes;
        let mut shifted = preview.clone();
        shifted.first_dab = 1;
        submit(&mut r, &[body, cap], &[persistent, shifted], 0.3);
        assert!(r.metrics.command_passes > count);
        let count = r.metrics.command_passes;
        submit(&mut r, &[cap], &[preview.clone()], 0.4);
        assert_eq!(
            r.metrics.command_passes, count,
            "batch offsets do not change retained dabs"
        );
        let mut restyled = preview.clone();
        restyled.style.opacity = 0.5;
        submit(&mut r, &[cap], &[restyled], 0.41);
        assert!(
            r.metrics.command_passes > count,
            "changed styles retire the preview"
        );
        let count = r.metrics.command_passes;
        let mut moved = packet(doc.scene(), [1024, 256]).view;
        moved.document_to_surface[4] = 10.;
        submit_ready(
            &mut r,
            FramePacket {
                view: moved,
                dabs: &[cap],
                dab_batches: &[preview.clone()],
                composite_all: false,
                ..packet(doc.scene(), [1024, 256])
            },
        )
        .unwrap();
        assert!(
            r.metrics.command_passes > count,
            "changed presentation inputs retire the preview"
        );
        submit(&mut r, &[cap], &[preview.clone()], 0.42);
        let count = r.metrics.command_passes;
        r.release_asset(&AssetId("retained-preview-source".into()));
        submit(&mut r, &[cap], &[preview.clone()], 0.43);
        assert!(
            r.metrics.command_passes > count,
            "released brush resources retire retained pages"
        );
        let count = r.metrics.command_passes;
        let mut invalid = preview.clone();
        invalid.dab_count = 2;
        assert!(
            r.submit(FramePacket {
                dabs: &[cap],
                dab_batches: &[invalid],
                ..packet(doc.scene(), [1024, 256])
            })
            .is_err()
        );
        submit(&mut r, &[cap], &[preview.clone()], 0.5);
        assert!(
            r.metrics.command_passes > count,
            "a discarded frame invalidates retained preview state"
        );
        submit(&mut r, &[], &[], 0.6);
        assert!(r.preview_pages.is_empty());
        let count = r.metrics.command_passes;
        submit(&mut r, &[cap], &[preview], 0.7);
        assert!(r.metrics.command_passes > count);
    }
}

#[test]
fn retained_dry_preview_keys_include_captured_effect_phases() {
    let doc = document();
    let scene = doc.scene();
    let effect = scene
        .order()
        .iter()
        .find_map(|&h| scene.effect_handle(h))
        .unwrap();
    let previous = layer_core::EvaluationContext {
        elapsed: 8.,
        phases: vec![(effect, 0.25)].into(),
    };
    let changed = layer_core::EvaluationContext {
        elapsed: 8.,
        phases: vec![(effect, 0.75)].into(),
    };
    let dabs = [contact(64., 3.).completion([-1., 0.], [0.; 2], 1.)];
    let mut preview = dab_batch(
        doc.working.target.unwrap(),
        preset_style(DefaultBrushPreset::Pencil),
        dabs[0].bounds(),
    );
    preview.kind = DabBatchKind::Preview;
    preview.preview_end = Some(0);
    let frame = FramePacket {
        dabs: &dabs,
        dab_batches: std::slice::from_ref(&preview),
        time_seconds: 8.,
        ..packet(scene.with_context(&previous), [1024, 256])
    };
    let cached = artwork::Frame::new(frame, previous.clone());
    assert!(cached.same_preview(frame));
    let updated = FramePacket {
        scene: scene.with_context(&changed),
        ..frame
    };
    assert!(cached.scene.view().same_artwork(updated.scene));
    assert!(
        !cached.same_preview(updated),
        "same elapsed time does not imply the same captured effect inputs"
    );
}

#[test]
fn dry_preview_ownership_uses_the_original_end_across_compacted_pages() {
    use layer_render::DabPreview;
    let mut r = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let doc = document();
    let style = preset_style(DefaultBrushPreset::Pencil);
    let mut real = contact(380., 20.);
    real.motion = [8., 0.];
    real.flow = 0.1;
    let mut landing = real;
    landing.motion = [300., 0.];
    landing.flow = 0.2;
    let end = real.completion([-1., 0.], [0.; 2], 1.);
    let mut main = real;
    main.center.x = 650.;
    main.motion = [270., 0.];
    main.flow = 0.2;
    let far = contact(900., 3.);
    let prefix = [contact(16., 3.), contact(32., 3.)];
    let mut render = |contacts: &[Dab]| {
        let mut dabs = prefix.to_vec();
        dabs.extend_from_slice(contacts);
        let damage = contacts
            .iter()
            .fold(Rect::EMPTY, |damage, dab| damage.union(dab.bounds()));
        let mut batch = dab_batch(doc.working.target.unwrap(), style.clone(), damage);
        batch.kind = DabBatchKind::Preview;
        batch.first_dab = prefix.len() as u32;
        batch.dab_count = contacts.len() as u32;
        batch.preview_end = contacts
            .iter()
            .rposition(|dab| dab.is_completion() && dab.previous_contact[2..] == [0.; 2])
            .map(|i| i as u32);
        submit_ready(
            &mut r,
            FramePacket {
                dabs: &dabs,
                dab_batches: &[batch],
                ..packet(doc.scene(), [1024, 256])
            },
        )
        .unwrap();
        let target=doc.working.target.unwrap();
        let pages = r.restore_preflight.as_ref().unwrap().outputs().iter()
            .filter_map(|(owner,output)|match output {
                crate::raster::staging::Output::PreviewColor(coordinate) if *owner==target=>Some(*coordinate),_=>None,
            })
            .map(|coordinate| {
                let page=r.preview_pages.iter().find(|page|page.coordinate==coordinate).unwrap();
                let expected = r.preview_expected(target, layer_core::raster::RasterPlane::Color, coordinate, scene::PreviewStage::Final).unwrap();
                let read = page.active().read(expected, None).unwrap();
                assert_eq!(
                    read.value().texture.format(),
                    wgpu::TextureFormat::Rgba32Float
                );
                (page.coordinate, page_bytes(&r, &read.value().texture))
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        assert!(!pages.is_empty());
        pages
    };
    let frozen = render(&[real, far, landing, end]);
    let base = render(&[real, far, end]);
    let ordinary = render(&[real, far, end, main]);
    let moving = render(&[
        real,
        far,
        landing.with_preview_tag(DabPreview::LiftOnly),
        end,
        main.with_preview_tag(DabPreview::MainOnly),
    ]);
    let (mut owned_contribution, mut outside_main) = (0, 0);
    for (coordinate, actual) in &moving {
        for (index, actual) in actual.chunks_exact(16).enumerate() {
            let at = index * 16;
            let pixel = |pages: &std::collections::BTreeMap<[u32; 2], Vec<u8>>| {
                pages
                    .get(coordinate)
                    .map_or([0u8; 16], |bytes| bytes[at..at + 16].try_into().unwrap())
            };
            let expected = pixel(&frozen);
            let ordinary = pixel(&ordinary);
            let base = pixel(&base);
            let x = (coordinate[0] * 256 + (index % 256) as u32) as f32 + 0.5;
            let y = (coordinate[1] * 256 + (index / 256) as u32) as f32 + 0.5;
            let owned =
                ((x - end.center.x) / 20.).powi(2) + ((y - end.center.y) / 20.).powi(2) <= 1.;
            if owned {
                assert_eq!(
                    actual, expected,
                    "the original global end owns ({x},{y}) after per-page compaction"
                );
                owned_contribution += usize::from(expected != base);
            } else {
                assert_eq!(
                    actual, ordinary,
                    "unowned pixel ({x},{y}) excludes the long LiftOnly body"
                );
                outside_main += usize::from(ordinary != base);
            }
        }
    }
    assert!(
        owned_contribution > 0,
        "the frozen lift contributes inside the end contact"
    );
    assert!(
        outside_main > 0,
        "the MainOnly tail contributes on pages without the end"
    );
    assert!(
        moving
            .get(&[0, 0])
            .is_none_or(|page| page.iter().all(|&v| v == 0)),
        "the LiftOnly-only page stays transparent outside ownership"
    );
}

#[path = "../tests/support/dry_fixture.rs"]
mod dry_fixture;

type DryEngine = layer_engine::CanvasEngine<WgpuRasterizer>;
type NativePaint = std::collections::BTreeMap<[u32; 2], Vec<u8>>;

fn canonical_paint(
    r: &mut WgpuRasterizer,
    tiles: Vec<([u32; 2], wgpu::Texture)>,
    color: layer_core::color::DocumentColor,
) -> NativePaint {
    use crate::native_tiles::{NativeTileEncoder, NativeTileRequest};
    use layer_core::color::{SampleDepth, AlphaAssociation, LayerColorMode};
    let transfer = r.prepare_native_transfer(color.space).unwrap();
    let encoder = NativeTileEncoder::with_device(&r.device);
    pollster::block_on(r.compilation.prepare_all(encoder.pipelines_for_depth(color.depth,false))).unwrap();
    let format = match color.depth {
        SampleDepth::U8 => wgpu::TextureFormat::Rgba8Uint,
        SampleDepth::U16 | SampleDepth::F16 => wgpu::TextureFormat::Rgba16Uint,
        SampleDepth::F32 => wgpu::TextureFormat::Rgba32Uint,
    };
    tiles
        .into_iter()
        .map(|(coordinate, working)| {
            let output = crate::test_support::page_texture(r, format);
            let canonical = crate::test_support::page_texture(r, wgpu::TextureFormat::Rgba32Float);
            let request = NativeTileRequest {
                working: &working,
                encoded: (&output).into(),
                canonical: &canonical,
                mode: LayerColorMode::FullColor,
                space: color.space,
                transfer: &transfer,
                depth: color.depth,
                alpha: AlphaAssociation::Straight,
                region: [0, 0, 256, 256],
            };
            let batch = encoder
                .prepare(&r.device, &[request], &mut Default::default())
                .unwrap();
            let mut commands = r.device.create_command_encoder(&Default::default());
            {
                let mut pass = commands.begin_compute_pass(&Default::default());
                encoder.encode(&mut pass, &batch);
            }
            r.queue.submit([commands.finish()]);
            (coordinate, page_bytes(r, &output))
        })
        .collect()
}

fn dry_engine_frame(engine: &mut DryEngine, time: u64, requested: u64) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        engine.render_frame_for(time, requested).unwrap();
        engine.backend_mut().wait_idle().unwrap();
        if !engine.has_pending_input() && !engine.has_pending_document_edits() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "dry fixture input and publication settle"
        );
        std::thread::yield_now();
    }
}

fn dry_continuation_coverage(
    preset: DefaultBrushPreset,
    diameter: f32,
    strength: f32,
    mut style: layer_render::DabStyle,
    dabs: Vec<Dab>,
) -> std::collections::BTreeSet<[u32; 2]> {
    if dabs.is_empty() {
        return Default::default();
    }
    let gpu = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let (mut engine, _) = dry_fixture::owned_contact_engine(gpu, preset, diameter, strength);
    let scene = engine.document().scene().snapshot();
    let target = engine.document().working.target.unwrap();
    style.paper = Some(engine.document().paper_binding(target));
    let dabs: Vec<_> = dabs
        .into_iter()
        .map(|dab| dab.with_preview_tag(layer_render::DabPreview::Shared))
        .collect();
    let damage = dabs
        .iter()
        .fold(Rect::EMPTY, |damage, dab| damage.union(dab.bounds()));
    let mut batch = dab_batch(target, style, damage);
    batch.kind = DabBatchKind::Preview;
    batch.dab_count = dabs.len() as u32;
    let renderer = engine.backend_mut();
    submit_ready(
        renderer,
        FramePacket {
            dabs: &dabs,
            dab_batches: &[batch],
            ..packet(scene.view(), [256; 2])
        },
    )
    .unwrap();
    renderer
        .preview_pages
        .iter()
        .flat_map(|page| {
            let expected = renderer.preview_expected(target, layer_core::raster::RasterPlane::Color, page.coordinate, scene::PreviewStage::Final).unwrap();
            let read = page.active().read(expected, None).unwrap();
            assert_eq!(
                read.value().texture.format(),
                wgpu::TextureFormat::Rgba32Float
            );
            page_bytes(renderer, &read.value().texture)
                .chunks_exact(16)
                .enumerate()
                .filter_map(|(index, pixel)| {
                    (f32::from_le_bytes(pixel[12..16].try_into().unwrap()) > 0.).then_some([
                        page.coordinate[0] * 256 + (index % 256) as u32,
                        page.coordinate[1] * 256 + (index / 256) as u32,
                    ])
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

struct CanonicalDryLift {
    before: NativePaint,
    after: NativePaint,
    end: Dab,
    committed_end: Dab,
    continuation_coverage: std::collections::BTreeSet<[u32; 2]>,
    corner: f32,
}

fn canonical_dry_lift(
    preset: DefaultBrushPreset,
    diameter: f32,
    strength: f32,
    paused: bool,
    last: u64,
) -> CanonicalDryLift {
    use dry_fixture::{owned_contact_engine, pen_sample};
    let gpu = WgpuRasterizer::new_native_headless(Default::default()).unwrap();
    let (mut engine, mut input) = owned_contact_engine(gpu, preset, diameter, strength);
    let color = engine.document().composition().color;
    let corner = engine.brush().contact.unwrap().dry.unwrap().corner;
    let target = engine.document().working.target.unwrap();
    let mut points = Vec::new();
    let (mut time, mut head) = (0, Point::default());
    for step in 0..=last {
        time = 1_000_000_000 + step * 4_000_000;
        head = if step <= 32 {
            Point {
                x: 32.5 + 3. * step as f32,
                y: 96.5,
            }
        } else {
            Point {
                x: 128.5,
                y: 96.5 + 3. * (step - 32) as f32,
            }
        };
        let mut event = pen_sample(step, last + 1, time, head);
        event.pressure = 0.7;
        points.push(layer_core::StrokePoint {
            position: event.surface_position,
            pressure: event.pressure,
            tilt: event.tilt_radians,
            twist: event.twist_radians,
            elapsed_micros: ((event.timestamp_ns - 1_000_000_000) / 1_000) as u32,
        });
        input.push(event).unwrap();
        dry_engine_frame(&mut engine, time, time + 8_000_000);
    }
    if paused {
        let mut frozen = None;
        for delay in [120_000_000, 200_000_000, 1_000_000_000, 3_000_000_000] {
            dry_engine_frame(&mut engine, time + delay, time + delay + 8_000_000);
            let held = engine.backend_mut().readback_srgb_rgba8().unwrap();
            if let Some(frozen) = &frozen {
                assert_eq!(
                    frozen, &held,
                    "{preset:?} strength={strength}: sampleless frozen preview stays fixed"
                );
            } else {
                frozen = Some(held);
            }
        }
        time += 3_000_000_000;
    }
    let (end, continuations, style, stroke_id) = {
        let frame = engine
            .backend()
            .artwork_frame
            .as_ref()
            .unwrap()
            .packet([256; 2]);
        let batch = frame
            .dab_batches
            .iter()
            .find(|b| b.kind == DabBatchKind::Preview && b.preview_end.is_some())
            .unwrap();
        let index = batch.preview_end.unwrap();
        assert!(index < batch.dab_count);
        let end = frame.dabs[(batch.first_dab + index) as usize];
        let continuations = frame
            .dab_batches
            .iter()
            .filter(|b| b.kind == DabBatchKind::Preview)
            .flat_map(|b| &frame.dabs[b.first_dab as usize..(b.first_dab + b.dab_count) as usize])
            .filter(|dab| {
                matches!(
                    dab.preview_tag(),
                    layer_render::DabPreview::LiftOnly | layer_render::DabPreview::MainOnly
                )
            })
            .copied()
            .collect();
        (end, continuations, batch.style.clone(), batch.stroke_id)
    };
    assert!(
        !engine.backend().preview_contribution,
        "the native full paint preview includes its persistent base"
    );
    let display_before = engine.backend_mut().readback_srgb_rgba8().unwrap();
    let preview = engine
        .backend()
        .preview_pages
        .iter()
        .map(|page| {
            let expected = engine.backend().preview_expected(target, layer_core::raster::RasterPlane::Color, page.coordinate, scene::PreviewStage::Final).unwrap();
            (page.coordinate, page.active().read(expected, None).unwrap().value().texture.clone())
        })
        .collect();
    let before = canonical_paint(engine.backend_mut(), preview, color);
    assert!(
        before
            .values()
            .any(|bytes| bytes.iter().any(|&value| value != 0)),
        "the canonical preview contains paint"
    );
    let continuation_coverage =
        dry_continuation_coverage(preset, diameter, strength, style, continuations);
    let brush = engine.brush().clone();
    let mut up = pen_sample(last + 1, last + 1, time, head);
    up.pressure = 0.7;
    points.push(layer_core::StrokePoint {
        position: up.surface_position,
        pressure: up.pressure,
        tilt: up.tilt_radians,
        twist: up.twist_radians,
        elapsed_micros: ((up.timestamp_ns - 1_000_000_000) / 1_000) as u32,
    });
    input.push(up).unwrap();
    for frame in 0..=32 {
        let now = time + frame * 16_000_000;
        engine.render_frame_for(now, now).unwrap();
        engine.backend_mut().wait_idle().unwrap();
        if !engine.has_pending_input() && !engine.has_pending_document_edits() {
            break;
        }
        assert!(
            frame < 32,
            "the stored Up completes its real settlement frames"
        );
    }
    assert!(
        !engine.has_active_stroke(),
        "the actual Up commits before native comparison"
    );
    let stroke = layer_core::Stroke::new(
        stroke_id,
        target,
        layer_core::StrokeTool::Brush,
        brush,
        points,
    )
    .unwrap();
    let mut committed = Vec::new();
    layer_engine::DabGenerator::generate(&stroke, color.space, &mut committed);
    let committed_end = committed
        .iter()
        .rev()
        .find(|dab| dab.is_completion() && dab.previous_contact[2..] == [0.; 2])
        .copied()
        .expect("actual authored input replay ends with a completion");
    let display_after = engine.backend_mut().readback_srgb_rgba8().unwrap();
    let stored_versions = engine.backend().raster_versions(engine.document().scene(), target).unwrap();
    let stored = engine
        .backend()
        .paint_layers
        .iter()
        .find(|layer| layer.id == target)
        .unwrap()
        .pages
        .iter()
        .map(|page| (page.coordinate, page.active().read(stored_versions.tile(layer_core::raster::RasterPlane::Color, page.coordinate), None).unwrap().value().texture.clone()))
        .collect();
    let after = canonical_paint(engine.backend_mut(), stored, color);
    let mut maximum = [0u8; 4];
    let mut changed = 0;
    for (before, after) in display_before
        .chunks_exact(4)
        .zip(display_after.chunks_exact(4))
    {
        changed += usize::from(before != after);
        for c in 0..4 {
            maximum[c] = maximum[c].max(before[c].abs_diff(after[c]));
        }
    }
    eprintln!(
        "U8_DISPLAY_RESIDUAL preset={preset:?} diameter={diameter} strength={strength} paused={paused} changed_pixels={changed} maxRGBA={maximum:?}"
    );
    CanonicalDryLift {
        before,
        after,
        end,
        committed_end,
        continuation_coverage,
        corner,
    }
}

#[test]
fn frozen_dry_preview_matches_canonical_stored_paint() {
    for strength in [0., 0.5, 1.] {
        for (preset, diameter) in [
            (DefaultBrushPreset::Pencil, 9.),
            (DefaultBrushPreset::Pencil, 36.),
            (DefaultBrushPreset::Chalk, 36.),
            (DefaultBrushPreset::PastelBlock, 36.),
        ] {
            let lift = canonical_dry_lift(preset, diameter, strength, true, 42);
            assert_eq!(
                lift.before.keys().collect::<Vec<_>>(),
                lift.after.keys().collect::<Vec<_>>()
            );
            for (coordinate, expected) in &lift.before {
                let actual = &lift.after[coordinate];
                let first = expected.iter().zip(actual).position(|(a, b)| a != b);
                assert_eq!(
                    first, None,
                    "{preset:?} diameter={diameter} strength={strength}: exact canonical stored paint page {coordinate:?}"
                );
            }
        }
    }
}

#[test]
fn moving_dry_lift_now_matches_canonical_paint_outside_the_settled_contact_and_continuations() {
    for (strength, last) in [(0., 33), (0.5, 42), (1., 42)] {
        for (preset, diameter) in [
            (DefaultBrushPreset::Pencil, 9.),
            (DefaultBrushPreset::Pencil, 36.),
            (DefaultBrushPreset::Chalk, 36.),
            (DefaultBrushPreset::PastelBlock, 36.),
        ] {
            let lift = canonical_dry_lift(preset, diameter, strength, false, last);
            let delta = 1.5f32.min(0.25 * lift.end.radii[0].min(lift.end.radii[1]));
            let distance = (lift.end.center.x - lift.committed_end.center.x)
                .hypot(lift.end.center.y - lift.committed_end.center.y);
            assert!(
                distance <= delta + 0.0001,
                "settled end remains within delta_c: {distance}>{delta}"
            );
            let mut expanded = lift.end;
            expanded.radii = expanded.radii.map(|r| r + delta);
            assert_eq!(
                lift.before.keys().collect::<Vec<_>>(),
                lift.after.keys().collect::<Vec<_>>()
            );
            let (mut outside, mut outside_paint, mut owned_paint) = (0, 0, 0);
            for (coordinate, before) in &lift.before {
                for (index, (before, after)) in before
                    .chunks_exact(4)
                    .zip(lift.after[coordinate].chunks_exact(4))
                    .enumerate()
                {
                    let (x, y) = (
                        (coordinate[0] * 256 + (index % 256) as u32) as f32 + 0.5,
                        (coordinate[1] * 256 + (index / 256) as u32) as f32 + 0.5,
                    );
                    owned_paint += usize::from(
                        before[3] > 0
                            && dry_fixture::dry_contact_contains(lift.end, lift.corner, x, y),
                    );
                    let continuation = lift.continuation_coverage.contains(&[
                        coordinate[0] * 256 + (index % 256) as u32,
                        coordinate[1] * 256 + (index / 256) as u32,
                    ]);
                    if !continuation
                        && !dry_fixture::dry_contact_contains(expanded, lift.corner, x, y)
                    {
                        outside += 1;
                        outside_paint += usize::from(before[3] > 0);
                        assert_eq!(
                            before, after,
                            "{preset:?} diameter={diameter} strength={strength}: canonical moving lift changes ({x},{y}) outside delta_c and pre-Up continuations"
                        );
                    }
                }
            }
            assert!(
                outside > 0 && outside_paint > 0,
                "the continuation mask leaves painted pixels under exact comparison"
            );
            assert!(owned_paint > 0, "the settled end owns visible paint");
        }
    }
}
