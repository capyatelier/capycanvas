#[test]
fn unchanged_worker_revision_cannot_restore_a_hold() {
    let mut chrome = ChromeHandoff::default();
    chrome.ready();
    assert!(chrome.grant(0));
    assert!(chrome.should_hold(true));
    assert_eq!(chrome.publish(true), Some(0));
    assert!(chrome.initial_applied());
    assert!(chrome.request(1, false));
    assert!(!chrome.should_hold(true));
    assert!(chrome.request(1, true));
    assert!(!chrome.should_hold(true), "an unheld requested revision must stay unheld");
    assert_eq!(chrome.publish(true), Some(1));
    assert_eq!(chrome.synchronized, None);
    assert!(chrome.request(2, true));
    assert!(chrome.should_hold(true));
}

#[test]
fn captured_context_publication_shares_source_roots_and_preserves_authored_output() {
    let document = layer_ui::new_drawing(32,24,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let mut capture = layer_host::tasks::capture_document(&document);
    let value = Arc::new(std::sync::OnceLock::new());
    let request = ContextRequest {value:value.clone()};
    let context = EvaluationContext {elapsed:17.,phases:Vec::new().into()};
    request.value.set(Ok(context.clone())).unwrap();
    drop(request);
    ContextCapture(value).install(&mut capture).unwrap();
    assert_eq!(capture.output().context,context);
    assert_eq!(document.output().context.elapsed,0.);
    assert_eq!(capture.artwork.paint,document.artwork.paint);
    assert_eq!(capture.checkpoint.owner,document.owner);
}

#[test]
fn retired_context_capture_resolves_failure_without_changing_sources() {
    let document = layer_ui::new_drawing(32,24,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let mut capture = layer_host::tasks::capture_document(&document);
    let original = capture.artwork.clone();
    let value = Arc::new(std::sync::OnceLock::new());
    drop(ContextRequest {value:value.clone()});
    assert!(ContextCapture(value).install(&mut capture).is_err());
    assert!(Arc::ptr_eq(&capture.artwork,&original));
}

use super::*;
use gtk::prelude::*;

#[test]
fn stop_retires_the_owner_without_waiting_for_its_worker_or_accepting_late_replies() {
    let context = gtk::glib::MainContext::new();
    context.with_thread_default(|| {
        let (mut renderer, commands, _chrome, reply) = RenderWorker::channels(Default::default());
        let status = renderer.chrome_status().clone();
        let (exit, wait) = mpsc::channel();
        let (observed, stopped) = mpsc::channel();
        let acknowledged = Arc::new(AtomicBool::new(false));
        let worker_acknowledged = acknowledged.clone();
        let ui = context.clone();
        renderer.thread = Some(std::thread::spawn(move || {
            assert!(matches!(commands.recv().unwrap(), Command::Stop));
            reply.send(Reply::DisplayHeadroom(9., None)).unwrap();
            observed.send(()).unwrap();
            let source = gtk::glib::idle_source_new(None, gtk::glib::Priority::DEFAULT, move || {
                exit.send(()).unwrap();
                gtk::glib::ControlFlow::Break
            });
            source.attach(Some(&ui));
            wait.recv_timeout(Duration::from_secs(5)).expect("GTK must keep servicing input during retirement");
            worker_acknowledged.store(true, Ordering::Release);
        }));
        let retirement = renderer.stop();
        assert!(!retirement.is_complete());
        assert!(!status.accepting.load(Ordering::Acquire));
        stopped.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(renderer.ready().is_err());
        assert_eq!(renderer.display_headroom, 1.);
        assert!(renderer.send(Command::Wake).is_err());
        assert!(!renderer.worker_is_retired());
        let (replacement, _commands, _chrome, replacement_reply) = RenderWorker::channels(Default::default());
        assert_ne!(renderer.proof_owner, replacement.proof_owner);
        assert!(replacement_reply.send(Reply::DisplayHeadroom(2., None)).is_ok());
        context.block_on(retirement);
        assert!(renderer.worker_is_retired());
        assert!(acknowledged.load(Ordering::Acquire));
        assert_eq!(renderer.display_headroom, 1.);
    }).unwrap();
}

fn overlay_packet(renderer: &mut RenderWorker, document: &layer_core::Document) -> Result<(), BackendError> {
    renderer.geometry = Some(Geometry { position: [0; 2], scale: 1, corner_radius: 0. });
    renderer.submit(FramePacket {
        time_seconds: 0., view: ViewState { width_px: 32, height_px: 24, document_to_surface: [1., 0., 0., 1., 0., 0.] },
        document_extent: [32, 24], scene: document.scene(), inspect_mask: None, selection_overlays: None,
        dabs: &[], dab_batches: &[], restore_rasters: &[], reset_layers: false, commit_rasters: true,
        composite_all: true, blend_space: Default::default(),
    })
}

fn queued_frame(commands: &mpsc::Receiver<Command>) -> Box<Frame> {
    loop {
        match commands.try_recv() {
            Ok(Command::Frame(frame)) => return frame,
            Ok(Command::Wake) => (),
            _ => panic!("expected a Frame without standalone overlay commands"),
        }
    }
}

#[test]
fn nonink_overlay_changes_wait_for_frame_submission() {
    let (mut renderer, commands, _chrome, _reply) = RenderWorker::channels(Default::default());
    let document = layer_ui::new_drawing(32, 24, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let crop = layer_render::CropOverlay { to_crop: layer_core::Affine::IDENTITY, dim: 0.75 };
    renderer.set_crop_overlay(Some(crop));
    assert!(commands.try_recv().is_err(), "an overlay setter can overtake a deferred Frame");
    let selection = layer_core::Selection::full();
    let overlay = layer_render::SelectionOverlay { active: true, editing: None, color: [0.5; 4], protected: false, saved_protected: true };
    renderer.set_selection_overlay(Some(overlay));
    renderer.set_selection_outline(Some(&selection)).unwrap();
    let paper = layer_render::PaperOverlay { paper: document.nearest_paper_binding(None).unwrap(), opacity: 0.6, moving: true };
    renderer.set_paper_overlay(Some(paper.clone()));
    renderer.set_clipping_preview(true, false);
    use layer_core::authored::vector::{Anchor, ItemId, PathGeometry, PathItem, Subpath};
    let path = Arc::new(PathItem::new(ItemId(1), Arc::new(PathGeometry::new(vec![Subpath::new(&[Anchor::corner([2., 3.])], false)]).unwrap()), Arc::default()));
    let owner = document.scene().order()[0];
    renderer.set_vector_preview(Some(layer_render::VectorPreview { owner, path: path.clone(), revision: 7 }));
    renderer.set_vector_motion(Some(layer_render::VectorMotion { owner, items: Arc::new([ItemId(1)].into()), transform: Default::default() }));
    assert!(commands.try_recv().is_err(), "non-ink setters bypassed the Frame");
    overlay_packet(&mut renderer, &document).unwrap();
    let first = queued_frame(&commands);
    assert_eq!(first.overlays.crop, Some(crop));
    assert_eq!(first.overlays.selection, Some(selection));
    assert_eq!(first.overlays.selection_overlay, Some(overlay));
    assert_eq!(first.overlays.paper, Some(paper));
    assert_eq!(first.overlays.paper_generation, 1);
    assert_eq!(first.overlays.clipping, [true, false]);
    assert!(Arc::ptr_eq(&first.overlays.vector_preview.as_ref().unwrap().path, &path));
    assert_eq!(first.overlays.vector_motion.as_ref().unwrap().owner, owner);
    renderer.set_crop_overlay(None);
    renderer.set_selection_overlay(None);
    renderer.set_paper_overlay(None);
    renderer.set_vector_preview(None);
    renderer.set_vector_motion(None);
    renderer.set_clipping_preview(false, true);
    assert!(commands.try_recv().is_err());
    overlay_packet(&mut renderer, &document).unwrap();
    let second = queued_frame(&commands);
    assert_eq!(first.overlays.crop, Some(crop));
    assert_eq!(first.overlays.paper_generation, 1);
    assert!(first.overlays.selection.is_some() && first.overlays.vector_motion.is_some());
    assert_eq!(second.overlays.crop, None);
    assert_eq!(second.overlays.paper_generation, 2);
    assert!(second.overlays.selection.is_none() && second.overlays.selection_overlay.is_none());
    assert!(second.overlays.vector_preview.is_none() && second.overlays.vector_motion.is_none());
    assert_eq!(second.overlays.clipping, [false, true]);
}

#[test]
fn backpressure_preserves_pending_overlays_and_unchanged_frames_share_them() {
    let (mut renderer, commands, _chrome, _reply) = RenderWorker::channels(Default::default());
    let document = layer_ui::new_drawing(32, 24, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    renderer.in_flight.store(2, Ordering::Release);
    let crop = layer_render::CropOverlay { to_crop: layer_core::Affine::IDENTITY, dim: 0.4 };
    renderer.set_crop_overlay(Some(crop));
    assert!(overlay_packet(&mut renderer, &document).is_err());
    assert!(commands.try_recv().is_err());
    renderer.in_flight.store(0, Ordering::Release);
    overlay_packet(&mut renderer, &document).unwrap();
    let first = queued_frame(&commands);
    renderer.set_crop_overlay(Some(crop));
    renderer.set_clipping_preview(false, false);
    renderer.set_vector_preview(None);
    renderer.set_vector_motion(None);
    overlay_packet(&mut renderer, &document).unwrap();
    let second = queued_frame(&commands);
    assert_eq!(second.overlays.crop, Some(crop));
    assert!(Arc::ptr_eq(&first.overlays, &second.overlays));
}

fn visible_chrome() -> ChromeHandoff {
    let mut chrome = ChromeHandoff::default();
    chrome.ready();
    assert!(chrome.grant(0));
    assert_eq!(chrome.publish(true), Some(0));
    assert!(chrome.initial_applied());
    chrome
}

fn chrome_request(revision: u64) -> ChromePresentation {
    ChromePresentation { revision, hold: true, surround: [0.; 4], backdrops: Vec::new(), panels: Vec::new(),
        style: layer_render_wgpu::BackdropBlurStyle {levels:0, offset:0.}, backdrop_hold:false, overviews:Vec::new() }
}

#[test]
fn chrome_requests_give_queued_ink_its_admission_turn_before_representing() {
    let mut input = 0;
    let ink = input_frame_order(&mut input, 1, false);
    let mut requests = ChromeRequests::default();
    requests.request(chrome_request(7), ink);
    assert!(requests.take_ready().is_none());
    requests.considered(ink);
    assert_eq!(requests.take_ready().unwrap().revision, 7);
    assert!(requests.take_ready().is_none());
}

#[test]
fn chrome_requests_protect_zero_dab_terminals_but_bypass_nonink_work() {
    let mut input = 0;
    let mut requests = ChromeRequests::default();
    assert_eq!(input_frame_order(&mut input, 0, false), 0);
    requests.request(chrome_request(1), input);
    assert_eq!(requests.take_ready().unwrap().revision, 1);
    let terminal = input_frame_order(&mut input, 0, true);
    requests.request(chrome_request(2), terminal);
    assert!(requests.take_ready().is_none());
    requests.considered(0);
    assert!(requests.take_ready().is_none());
    requests.considered(terminal);
    assert_eq!(requests.take_ready().unwrap().revision, 2);
}

#[test]
fn chrome_requests_accept_a_frame_considered_before_the_request() {
    let mut requests = ChromeRequests::default();
    requests.considered(2);
    requests.considered(1);
    requests.request(chrome_request(7), 2);
    assert_eq!(requests.take_ready().unwrap().revision, 7);
}

#[test]
fn chrome_requests_join_ready_navigation_and_ink_frames() {
    let mut requests = ChromeRequests::default();
    requests.request(chrome_request(7), 1);
    requests.considered(1);
    assert_eq!(requests.take_ready().unwrap().revision, 7);
    assert!(requests.take_ready().is_none());
}

#[test]
fn ready_navigation_is_admitted_before_receipt_returns() {
    let (mut renderer, commands, _chrome, _reply) = RenderWorker::channels(Default::default());
    let document = layer_ui::new_drawing(32, 24, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    overlay_packet(&mut renderer, &document).unwrap();
    let mut frame = queued_frame(&commands);
    assert_eq!(frame.input_order, 0);
    assert!(frame.present);
    frame.view.document_to_surface[0] = 2.;
    assert!(frame.chrome.revision < 7);
    let expected = (frame.view.document_to_surface, 7);
    let mut requests = ChromeRequests::default();
    requests.request(chrome_request(7), 0);
    let mut chrome = visible_chrome();
    let mut pending = VecDeque::new();
    let mut admitted = Vec::new();
    assert_eq!(admit_frame(Some(frame), &mut pending, |frame| {
        chrome.request(frame.chrome.revision, frame.chrome.hold);
        while let Some(request) = requests.take_ready() {
            chrome.request(request.revision, request.hold);
        }
        chrome.release_for_canvas();
        assert_eq!(chrome.publish(false), Some(7));
        admitted.push((frame.view.document_to_surface, chrome.published));
        Ok::<_, ()>(true)
    }).unwrap(), 1);
    assert_eq!(admitted, [expected]);
    assert!(pending.is_empty());
    assert!(requests.take_ready().is_none());
    assert_eq!(chrome.synchronized, None);
}

#[test]
fn waiting_frame_queues_once_and_is_admitted_when_ready() {
    let (mut renderer, commands, _chrome, _reply) = RenderWorker::channels(Default::default());
    let document = layer_ui::new_drawing(32, 24, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    overlay_packet(&mut renderer, &document).unwrap();
    let frame = queued_frame(&commands);
    let expected = frame.view.document_to_surface;
    let mut pending = VecDeque::new();
    let mut attempts = 0;
    assert_eq!(admit_frame(Some(frame), &mut pending, |_| {
        attempts += 1;
        Ok::<_, ()>(false)
    }).unwrap(), 0);
    assert_eq!(attempts, 1);
    assert_eq!(pending.len(), 1);
    let mut admitted = Vec::new();
    assert_eq!(admit_frame(None, &mut pending, |frame| {
        attempts += 1;
        admitted.push(frame.view.document_to_surface);
        Ok::<_, ()>(true)
    }).unwrap(), 1);
    assert_eq!(attempts, 2);
    assert_eq!(admitted, [expected]);
    assert!(pending.is_empty());
}

#[test]
fn canvas_work_preserves_its_unfinished_predecessor() {
    let mut pending = VecDeque::new();
    assert_eq!(admit_frame(Some(1), &mut pending, |_| Ok::<_, ()>(false)).unwrap(), 0);
    let mut prepared = Vec::new();
    assert_eq!(admit_frame(Some(2), &mut pending, |frame| {
        prepared.push(*frame);
        Ok::<_, ()>(*frame == 2)
    }).unwrap(), 0);
    assert_eq!(prepared, [1]);
    assert_eq!(pending, [1, 2]);
    let mut drawn = Vec::new();
    assert_eq!(admit_frame(Some(3), &mut pending, |frame| {
        drawn.push(*frame);
        Ok::<_, ()>(true)
    }).unwrap(), 3);
    assert_eq!(drawn, [1, 2, 3]);
    assert!(pending.is_empty());
}

#[test]
fn chrome_requests_keep_a_fixed_first_boundary_and_only_the_latest_successor() {
    let mut requests = ChromeRequests::default();
    for revision in 1..=1000 { requests.request(chrome_request(revision), revision); }
    assert!(requests.take_ready().is_none());
    requests.considered(1);
    assert_eq!(requests.take_ready().unwrap().revision, 1);
    assert!(requests.take_ready().is_none());
    requests.considered(1000);
    assert_eq!(requests.take_ready().unwrap().revision, 1000);
    assert!(requests.take_ready().is_none());
}

#[test]
fn chrome_requests_coalesce_the_same_input_boundary_and_ignore_older_requests() {
    let mut requests = ChromeRequests::default();
    requests.request(chrome_request(7), 2);
    requests.request(chrome_request(8), 2);
    requests.request(chrome_request(4), 1);
    requests.considered(2);
    assert_eq!(requests.take_ready().unwrap().revision, 8);
    assert!(requests.take_ready().is_none());
}

#[test]
fn chrome_first_present_uses_the_exact_grant_before_newer_or_deferred_requests() {
    let mut chrome = ChromeHandoff::default();
    chrome.request(3, false);
    assert!(!chrome.can_present());
    chrome.ready();
    assert!(chrome.grant(7));
    assert!(!chrome.request(8, false));
    assert!(!chrome.request(7, false));
    assert!(!chrome.request(4, true));
    assert_eq!(chrome.requested, 7);
    assert!(chrome.should_hold(true));
    assert_eq!(chrome.publish(true), Some(7));
    assert!(!chrome.can_present());
    assert!(!chrome.release(7));
    assert!(chrome.initial_applied());
    assert!(chrome.request(8, true));
    assert_eq!(chrome.publish(true), Some(8));
}

#[test]
fn chrome_initial_revision_zero_still_requires_a_synchronized_first_commit() {
    let mut chrome = ChromeHandoff::default();
    chrome.request(0, false);
    assert!(!chrome.can_present());
    chrome.ready();
    assert!(!chrome.can_present());
    assert!(chrome.grant(0));
    assert!(chrome.should_hold(true));
    assert_eq!(chrome.publish(true), Some(0));
    assert!(!chrome.can_present());
    assert!(!chrome.release(0));
    assert!(chrome.initial_applied());
}

#[test]
fn chrome_status_distinguishes_startup_visibility_and_retirement() {
    use crate::glass::ChromeState;
    let status = ChromeStatus::new(9);
    assert_eq!(status.state(), ChromeState::Preparing(9));
    status.value.store(CHROME_READY, Ordering::Release);
    assert_eq!(status.state(), ChromeState::Ready(9));
    status.publish(23);
    assert_eq!(status.state(), ChromeState::Visible(9, 23));
    status.value.store(CHROME_RETIRED, Ordering::Release);
    assert_eq!(status.state(), ChromeState::Retired(9));
    assert_eq!(ChromeStatus::new(10).state(), ChromeState::Preparing(10));
}

#[test]
fn chrome_requests_cannot_be_replaced_by_an_older_deferred_frame() {
    let mut chrome = ChromeHandoff::default();
    chrome.request(7, true);
    chrome.request(4, false);
    assert_eq!(chrome.requested, 7);
    assert!(chrome.should_hold(true));
}

#[test]
#[ignore = "hardware GPU, no window: workspace presenter startup and replacement"]
fn workspace_presenter_prepares_chrome_before_first_panel_parameters() {
    fn prepared(renderer: &mut WgpuRasterizer, kind: &str) {
        for (format, color) in [
            (wgpu::TextureFormat::Rgba8UnormSrgb, layer_render_wgpu::SdrSurfaceColor::Srgb),
            (wgpu::TextureFormat::Rgba16Float, layer_render_wgpu::SdrSurfaceColor::WindowsScrgb),
        ] {
            let mut presenter = workspace_presenter(renderer, format, color).unwrap();
            assert!(!presenter.chrome_ready(), "{kind}/{format:?}: cold chrome must prepare asynchronously");
            let deadline = std::time::Instant::now() + Duration::from_secs(60);
            while !presenter.prepare_chrome(renderer) {
                assert!(std::time::Instant::now() < deadline, "{kind}/{format:?}: chrome preparation timed out");
                renderer.device().poll(wgpu::PollType::Poll).unwrap();
                renderer.poll_startup().unwrap();
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(presenter.pipelines_ready(renderer) && presenter.chrome_ready());
            presenter.set_backdrop(renderer, &[layer_render_wgpu::BackdropRegion::rounded(
                [96., 24., 144., 160.], [18.; 4], layer_render_wgpu::BackdropRegion::SQUIRCLE,
            )], Default::default(), false);
            presenter.set_panel_surfaces(renderer, &[layer_render_wgpu::PanelSurface {
                sidebar: [54., 8., 36., 208.], sidebar_radii: [18.; 4], plane: 96.,
                bridge_width: 6., direction: 1., scale: 1., color: [0.5, 0.5, 0.5, 1.],
                anchors: vec![[54., 80., 36., 36.]],
                groups: vec![layer_render_wgpu::PanelSurfaceGroup {
                    bounds: [96., 24., 144., 160.], radii: [18.; 4], tab_bar: [96., 24., 144., 30.],
                    active_tab: [96., 24., 36., 30.], tab_clip: [96., 24., 144., 30.],
                    color: [0.5, 0.5, 0.5, 1.], strip_color: [0.4, 0.4, 0.4, 1.],
                }],
            }]);
            assert!(presenter.pipelines_ready(renderer) && presenter.chrome_ready(),
                "{kind}/{format:?}: the first panel and backdrop reuse prepared pipelines and profiles");
            assert!(presenter.prepare_chrome(renderer));
        }
    }
    let reference = WgpuRasterizer::new_native_headless(Default::default()).expect("hardware GPU required");
    assert_ne!(reference.adapter().get_info().device_type, wgpu::DeviceType::Cpu);
    let mut renderer = WgpuRasterizer::from_wgpu_native_staged(reference.adapter().clone(),
        reference.device().clone(), reference.queue().clone(), Default::default()).unwrap();
    prepared(&mut renderer, "initial/HDR replacement");
    let mut candidate = renderer.color_candidate_staged(layer_core::color::DocumentColor {
        space: layer_core::color::RgbSpace::DisplayP3, ..Default::default()
    }).unwrap();
    prepared(&mut candidate, "color candidate");
    drop(candidate);
    drop(renderer);
    drop(reference);
    layer_render_wgpu::finish_shader_compiler_shutdown();
}

#[test]
fn chrome_requests_coalesce_without_blocking_canvas_publication() {
    let mut chrome = visible_chrome();
    chrome.request(1, true);
    assert_eq!(chrome.published, 0);
    assert!(chrome.can_present());
    assert!(chrome.should_hold(true));
    chrome.request(2, true);
    assert_eq!(chrome.published, 0, "an unacquired buffer cannot acknowledge either request");
    assert_eq!(chrome.publish(true), Some(2));
    chrome.request(3, true);
    chrome.request(4, true);
    assert!(chrome.can_present());
    assert_eq!(chrome.published, 2, "new requests must preserve the held buffer's revision");
    assert!(!chrome.release(1));
    assert!(chrome.can_present());
    assert!(chrome.release(2), "release matches the held revision, not the latest request");
    assert!(chrome.can_present());
    assert_eq!(chrome.publish(true), Some(4));
    assert!(!chrome.release(2));
    assert!(chrome.release(4));
    assert_eq!(chrome.publish(true), None);
    assert!(chrome.can_present());
}

#[test]
fn chrome_release_tracks_the_parent_without_blocking_canvas() {
    let mut chrome = visible_chrome();
    chrome.request(1, true);
    assert_eq!(chrome.publish(true), Some(1));
    chrome.request(2, true);
    assert!(chrome.can_present());
    assert!(!chrome.release(0));
    assert!(chrome.can_present());
    assert!(chrome.release(1));
    assert!(chrome.can_present());
    assert_eq!(chrome.publish(true), Some(2));
}

#[test]
fn ready_canvas_desynchronizes_without_waiting_for_a_parent_release() {
    let mut chrome = visible_chrome();
    chrome.request(1, true);
    assert_eq!(chrome.publish(true), Some(1));
    assert_eq!(chrome.synchronized, Some(1));
    assert!(chrome.can_present());
    chrome.request(2, true);
    assert!(chrome.release_for_canvas());
    assert!(!chrome.should_hold(false));
    assert_eq!(chrome.publish(false), Some(2));
    assert_eq!(chrome.synchronized, None);
    assert!(!chrome.release(1));
    assert!(chrome.can_present());
}

#[test]
fn unheld_revisions_never_request_synchronization() {
    let mut chrome = visible_chrome();
    chrome.request(1, true);
    assert_eq!(chrome.publish(true), Some(1));
    chrome.request(2, false);
    assert!(chrome.release_for_canvas());
    assert!(!chrome.should_hold(true));
    assert!(!chrome.should_hold(false));
    assert_eq!(chrome.publish(true), Some(2));
    assert_eq!(chrome.synchronized, None);
    assert!(!chrome.hold);
}

#[test]
fn a_held_revision_publishes_only_once_while_synchronized() {
    let mut chrome = visible_chrome();
    chrome.request(7, true);
    let mut presents = Vec::new();
    let mut synchronizations = 0;
    for _ in 0..3 {
        if chrome.needs_publication() {
            synchronizations += usize::from(chrome.should_hold(true));
            presents.push(chrome.publish(true));
        }
    }
    assert_eq!(presents, [Some(7)]);
    assert_eq!(synchronizations, 1);
    assert_eq!(chrome.synchronized, Some(7));
    assert!(chrome.can_present());
}

#[test]
fn held_chrome_successors_coalesce_without_blocking_canvas() {
    for canvas in [false, true] {
        let mut chrome = visible_chrome();
        chrome.request(1, true);
        assert_eq!(chrome.publish(true), Some(1));
        for revision in 2..=4 {
            chrome.request(revision, true);
            assert!(!chrome.needs_publication());
            assert!(chrome.can_present());
            assert_eq!(chrome.synchronized, Some(1));
            assert_eq!(chrome.published, 1);
        }
        if canvas {
            assert!(chrome.release_for_canvas());
            assert!(!chrome.should_hold(false));
        } else {
            assert!(chrome.release(1));
        }
        assert!(chrome.can_present());
        assert!(chrome.needs_publication());
        assert_eq!(chrome.publish(!canvas), Some(4));
        assert_eq!(chrome.synchronized, if canvas { None } else { Some(4) });
        assert!(!chrome.needs_publication());
    }
}

fn pump_worker_window() {
    let context = gtk::glib::MainContext::default();
    while context.pending() { context.iteration(false); }
    std::thread::sleep(Duration::from_millis(5));
}

fn native_worker_window(id: &str) -> (crate::workspace::tests::NativeTestApp,
    gtk::ApplicationWindow, Parent, gtk::glib::SendWeakRef<gtk::Picture>) {
    let app = crate::workspace::tests::native_test_app(id);
    let area = gtk::Picture::new();
    let window = gtk::ApplicationWindow::builder()
        .application(&*app)
        .child(&area)
        .default_width(320)
        .default_height(240)
        .build();
    window.present();
    while !window.is_mapped() {
        pump_worker_window();
    }
    let parent = Parent::new(&window.surface().unwrap()).unwrap();
    (app, window, parent, area.downgrade().into())
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_hdr_display_hint_before_first_frame() {
    let (_app, window, parent, area) = native_worker_window("art.capycanvas.EarlyHdrDisplayHint");
    let (commands, receiver) = mpsc::channel();
    let (reply, replies) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut worker = Worker::new(
            parent,
            area,
            Arc::default(),
            Arc::new(ChromeStatus::new(0)),
            layer_core::color::DocumentColor {
                depth: layer_core::color::SampleDepth::F16,
                ..Default::default()
            },
            &layer_ui::Localizer::shared(layer_ui::UiLanguage::English),
        )?;
        // Exercise the display-event path before ANY frame/geometry is sent.
        // An idle acquire here used to panic on an unconfigured swapchain.
        worker.display_headroom = 4.;
        worker.update_hdr_view()?;
        worker.run(
            &receiver,
            &mpsc::channel().1,
            &reply,
            &Default::default(),
            &AtomicUsize::new(0),
            &Default::default(),
            Arc::default(),
        )
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !matches!(replies.try_recv(), Ok(Reply::Initialized(..))) {
        assert!(!worker.is_finished(), "GPU owner stopped during initialization");
        assert!(std::time::Instant::now() < deadline, "GPU owner did not initialize");
        pump_worker_window();
    }
    let idle_until = std::time::Instant::now() + Duration::from_millis(200);
    while std::time::Instant::now() < idle_until {
        pump_worker_window();
    }
    let sent = commands.send(Command::Stop);
    let result = worker.join();
    window.destroy();
    assert!(sent.is_ok(), "GPU owner failed while waiting for its first frame");
    result.expect("GPU owner panicked before its first frame").unwrap();
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_late_shader_failure_after_startup() {
    const CAUSE: &str = "late required compiler cause after startup";
    let (_app, window, parent, area) = native_worker_window("art.capycanvas.LateShaderFailure");
    let (commands, receiver) = mpsc::channel();
    let (reply, _replies) = mpsc::channel();
    let (prepared, ready) = mpsc::channel();
    let worker = std::thread::spawn(move || -> Result<(), String> {
        let mut worker = Worker::new(parent, area, Arc::default(), Arc::new(ChromeStatus::new(0)), Default::default(), &layer_ui::Localizer::shared(layer_ui::UiLanguage::English))?;
        let document = layer_ui::new_drawing(32, 24, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English))?;
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        worker.renderer.prepare_startup(&document, &brush, false).map_err(error)?;
        worker.renderer.finish_startup_catalog();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !worker.renderer.poll_startup().map_err(error)?.complete {
            if std::time::Instant::now() >= deadline { return Err("Renderer startup did not complete".into()); }
            std::thread::sleep(Duration::from_millis(1));
        }
        layer_render_wgpu::enqueue_compiler_failure(&worker.renderer, CAUSE)
            .recv_timeout(Duration::from_secs(5)).map_err(error)?;
        prepared.send(()).map_err(error)?;
        worker.run(&receiver, &mpsc::channel().1, &reply, &Default::default(), &AtomicUsize::new(0),
            &Default::default(), Arc::default())
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    let startup_complete = loop {
        if ready.try_recv().is_ok() { break true; }
        if worker.is_finished() || std::time::Instant::now() >= deadline { break false; }
        pump_worker_window();
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while startup_complete && !worker.is_finished() && std::time::Instant::now() < deadline {
        let _ = commands.send(Command::FinishStartupCatalog);
        pump_worker_window();
    }
    let failed_before_stop = worker.is_finished();
    let _ = commands.send(Command::Stop);
    let result = worker.join();
    window.destroy();
    assert!(startup_complete, "Startup preparation failed: {result:?}");
    assert!(failed_before_stop, "A completed startup must still poll late compiler failures");
    let cause = result.expect("GPU owner panicked").expect_err("GPU owner ignored the compiler failure");
    assert!(cause.contains(CAUSE), "GPU owner lost the recorded cause: {cause}");
}
