#[test]
fn unchanged_worker_revision_cannot_restore_a_hold() {
    let mut chrome = ChromeHandoff::default();
    chrome.ready();
    assert!(chrome.grant(0));
    assert!(chrome.should_hold());
    assert_eq!(chrome.publish(), Some(0));
    assert!(chrome.initial_applied());
    assert!(chrome.request(1, false));
    assert!(!chrome.should_hold());
    assert!(chrome.request(1, true));
    assert!(!chrome.should_hold(), "an unheld requested revision must stay unheld");
    assert_eq!(chrome.publish(), Some(1));
    assert_eq!(chrome.synchronized, None);
    assert!(chrome.request(2, true));
    assert!(chrome.should_hold());
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
fn canvas_admits_wayland_surfaces_with_mailbox_premultiplied_alpha_and_an_eight_bit_format() {
    use wgpu::{SurfaceColorSpaces as Spaces, TextureFormat as Format};
    let caps = |color_spaces: Spaces, formats: &[Format], present_modes: &[wgpu::PresentMode], alpha_modes: &[wgpu::CompositeAlphaMode]| wgpu::SurfaceCapabilities {
        format_capabilities: formats.iter().map(|&format| wgpu::SurfaceFormatCapabilities { format, color_spaces }).collect(),
        present_modes: present_modes.to_vec(),
        alpha_modes: alpha_modes.to_vec(),
        ..Default::default()
    };
    let formats = [Format::Bgra8UnormSrgb, Format::Rgba8UnormSrgb, Format::Rgba16Float, Format::Rgb10a2Unorm, Format::Bgra8Unorm, Format::Rgba8Unorm];
    let modes = [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Fifo];
    let alphas = [wgpu::CompositeAlphaMode::Opaque, wgpu::CompositeAlphaMode::PreMultiplied];
    let refusal = |spaces, formats: &[Format], modes: &[wgpu::PresentMode], alphas: &[wgpu::CompositeAlphaMode]| canvas_refusal(&caps(spaces, formats, modes, alphas));
    let mesa = Spaces::SRGB | Spaces::DISPLAY_P3 | Spaces::BT2100_PQ | Spaces::PASS_THROUGH;
    assert_eq!(refusal(mesa, &formats, &modes, &alphas), None);
    assert_eq!(refusal(Spaces::SRGB, &formats, &modes, &alphas), None);
    assert_eq!(refusal(mesa, &[Format::Rgba16Float, Format::Rgb10a2Unorm], &modes, &alphas).as_deref(), Some(EIGHT_BIT_SURFACE));
    assert!(refusal(Spaces::SRGB, &formats, &[wgpu::PresentMode::Fifo], &alphas).is_some_and(|cause| cause.contains("mailbox")));
    assert!(refusal(Spaces::SRGB, &formats, &modes, &[wgpu::CompositeAlphaMode::Opaque]).is_some_and(|cause| cause.contains("premultiplied")));
}

struct ReadyPreparation {
    remaining: usize,
    steps: usize,
}
impl ReadyPreparation {
    fn ready(&self) -> bool { self.remaining != 0 }
    fn step(&mut self) {
        assert!(self.ready());
        self.remaining -= 1;
        self.steps += 1;
    }
}

#[test]
fn ready_preparation_finishes_each_cpu_step_without_a_receiver_wait() {
    const STEPS: usize = 37;
    let (_commands, receiver) = mpsc::channel::<()>();
    let mut renderer = ReadyPreparation { remaining: STEPS, steps: 0 };
    for _ in 0..STEPS {
        assert!(matches!(preparation_command(&receiver, renderer.ready()), PreparationCommand::Ready));
        renderer.step();
    }
    assert_eq!(renderer.steps, STEPS);
    assert!(matches!(preparation_command(&receiver, renderer.ready()), PreparationCommand::Wait));
}

#[test]
fn input_queued_during_ready_preparation_waits_at_most_one_cpu_step() {
    const STEPS: usize = 37;
    const INPUT_AT: usize = 11;
    let (commands, receiver) = mpsc::channel();
    let mut renderer = ReadyPreparation { remaining: STEPS, steps: 0 };
    let mut handled_at = None;
    for _ in 0..STEPS {
        if renderer.steps == INPUT_AT { commands.send(7).unwrap(); }
        renderer.step();
        match preparation_command(&receiver, renderer.ready()) {
            PreparationCommand::Command(7) => handled_at = Some(renderer.steps),
            PreparationCommand::Ready => assert!(renderer.ready()),
            PreparationCommand::Wait => assert!(!renderer.ready()),
            _ => panic!("preparation lost its input channel or command"),
        }
    }
    assert_eq!(handled_at, Some(INPUT_AT + 1));
    assert_eq!(renderer.steps, STEPS);
    assert!(matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty)));
}

#[test]
fn preparation_waits_without_ready_work_and_prioritizes_queued_commands() {
    let (commands, receiver) = mpsc::channel();
    assert!(matches!(preparation_command(&receiver, false), PreparationCommand::Wait));
    commands.send(7).unwrap();
    assert!(matches!(preparation_command(&receiver, false), PreparationCommand::Command(7)));
    commands.send(9).unwrap();
    drop(commands);
    assert!(matches!(preparation_command(&receiver, true), PreparationCommand::Command(9)));
    assert!(matches!(preparation_command(&receiver, true), PreparationCommand::Closed));
    assert!(matches!(preparation_command(&receiver, false), PreparationCommand::Closed));
}

#[test]
fn a_settled_worker_sleeps_on_its_receiver_until_a_wake_and_work_never_blocks() {
    let (commands, receiver) = mpsc::channel();
    let soon = std::time::Instant::now() + Duration::from_millis(5);
    assert_eq!(FrameDemand::Wait.sooner(FrameDemand::Wait), FrameDemand::Wait);
    assert_eq!(FrameDemand::WaitUntil(soon + Duration::from_secs(1)).sooner(FrameDemand::WaitUntil(soon)), FrameDemand::WaitUntil(soon));
    assert_eq!(FrameDemand::WaitUntil(soon).sooner(FrameDemand::Work), FrameDemand::Work);
    assert!(matches!(receive::<u8>(&receiver, FrameDemand::Work), Err(mpsc::RecvTimeoutError::Timeout)));
    assert!(matches!(receive::<u8>(&receiver, FrameDemand::WaitUntil(soon)), Err(mpsc::RecvTimeoutError::Timeout)));
    let started = std::time::Instant::now();
    let wake = std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(300)); commands.send(7u8).unwrap(); });
    assert_eq!(receive(&receiver, FrameDemand::Wait).unwrap(), 7);
    assert!(started.elapsed() >= Duration::from_millis(300));
    wake.join().unwrap();
    assert!(matches!(receive::<u8>(&receiver, FrameDemand::Wait), Err(mpsc::RecvTimeoutError::Disconnected)));
}

#[test]
fn preparation_notifications_coalesce_until_the_next_completion() {
    let (commands, receiver) = mpsc::channel();
    let signal = PreparationSignal {
        commands,
        queued: AtomicBool::new(false),
    };
    for _ in 0..128 { signal.notify(); }
    assert!(matches!(receiver.try_recv(), Ok(Command::PreparationReady)));
    assert!(matches!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty)));
    signal.queued.store(false, Ordering::Release);
    signal.notify();
    assert!(matches!(receiver.try_recv(), Ok(Command::PreparationReady)));
    signal.queued.store(false, Ordering::Release);
    signal.notify();
    assert!(matches!(receiver.try_recv(), Ok(Command::PreparationReady)));

}

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
            Ok(Command::Frame(frame, _)) => return frame,
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

#[test]
fn pending_presentation_keeps_picker_changes_even_when_the_view_and_cursor_are_unchanged() {
    let view = ViewState { width_px: 320, height_px: 240, document_to_surface: [1., 0., 0., 1., 0., 0.] };
    let geometry = Geometry { position: [0, 0], scale: 1, corner_radius: 0. };
    let mut previous = Presentation { view, geometry, cursor: Vec::new(), picker: None };
    let mut picker = layer_render::ColorPickerOverlay { sample_color_only: false, center: [160., 120.],
        sample: [160., 180.], scale: 2., classic: false, layer: false, original: [0., 0., 0., 1.], candidate: [1.; 4] };
    assert!(!previous.matches(view, geometry, &[], Some(picker)));
    previous.picker = Some(picker);
    assert!(previous.matches(view, geometry, &[], Some(picker)));
    picker.candidate = [0.3, 0.5, 0.8, 1.];
    assert!(!previous.matches(view, geometry, &[], Some(picker)));
    previous.picker = Some(picker);
    picker.center[0] += 10.;
    assert!(!previous.matches(view, geometry, &[], Some(picker)));
    assert!(!previous.matches(view, geometry, &[], None));
    previous.picker = None;
    assert!(previous.matches(view, geometry, &[], None));
    assert!(!previous.matches(view, Geometry { scale: 2, ..geometry }, &[], None));
}

fn presenting_frame(presentation: Presentation) -> Command {
    let (mut renderer, commands, _chrome, _reply) = RenderWorker::channels(Default::default());
    let document = layer_ui::new_drawing(32, 24, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    overlay_packet(&mut renderer, &document).unwrap();
    let mut frame = queued_frame(&commands);
    frame.view = presentation.view;
    Command::Frame(frame, Some(presentation))
}

#[test]
fn deferred_artwork_cannot_replay_an_older_picker_after_a_newer_presentation() {
    let view = ViewState { width_px: 320, height_px: 240, document_to_surface: [1., 0., 0., 1., 0., 0.] };
    let picker = layer_render::ColorPickerOverlay { sample_color_only: false, center: [160., 120.], sample: [160., 180.],
        scale: 2., classic: false, layer: false, original: [0., 0., 0., 1.], candidate: [1.; 4] };
    let presentation = |shown: bool| Presentation { view, geometry: Geometry { position: [0, 0], scale: 1, corner_radius: 0. },
        cursor: Vec::new(), picker: shown.then_some(picker) };
    for shown in [false, true] {
        let mut artwork = presenting_frame(presentation(!shown));
        let received = artwork.take_presentation().unwrap();
        assert_eq!(received.picker.is_some(), !shown);
        let mut pending = VecDeque::from([artwork]);
        let mut newer = Command::Present(Some(presentation(shown)));
        let latest = newer.take_presentation().unwrap();
        assert_eq!(latest.picker.is_some(), shown);
        let mut resumed = pending.pop_front().unwrap();
        assert!(resumed.take_presentation().is_none());
        assert!(newer.take_presentation().is_none());
        assert!(matches!(resumed, Command::Frame(frame, None) if frame.view == view));
    }
}

#[test]
fn deferred_presenting_frame_after_hdr_invalidation_waits_for_a_new_view() {
    let view = ViewState { width_px: 320, height_px: 240, document_to_surface: [1., 0., 0., 1., 0., 0.] };
    let mut artwork = presenting_frame(Presentation { view, geometry: Geometry { position: [0, 0], scale: 1, corner_radius: 0. },
        cursor: Vec::new(), picker: None });
    let mut last_view = artwork.take_presentation().map(|presentation| presentation.view);
    let mut pending = VecDeque::from([artwork]);
    assert!(presentation_needed(last_view, true, |_| false));
    last_view = None;
    let ready = pending.pop_front().unwrap();
    assert!(matches!(ready, Command::Frame(frame, None) if frame.present));
    assert!(!presentation_needed(last_view, true, |_| panic!("HDR invalidated the view before artwork became ready")));
    assert!(!presentation_needed(last_view, false, |_| panic!("the old frame must not restore its paint-space view")));
    assert!(presentation_needed(Some(view), false, |_| true));
}

fn visible_chrome() -> ChromeHandoff {
    let mut chrome = ChromeHandoff::default();
    chrome.ready();
    assert!(chrome.grant(0));
    assert_eq!(chrome.publish(), Some(0));
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
        chrome.release_unheld();
        assert_eq!(chrome.publish(), Some(7));
        admitted.push((frame.view.document_to_surface, chrome.published));
        Ok::<_, ()>(true)
    }).unwrap(), 1);
    assert_eq!(admitted, [expected]);
    assert!(pending.is_empty());
    assert!(requests.take_ready().is_none());
    assert_eq!(chrome.synchronized, Some(7));
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
fn chrome_progress_keeps_unready_frames_pending_and_ready_frames_first() {
    let mut pending = VecDeque::new();
    assert_eq!(admit_frame(Some(2), &mut pending, |_| Ok::<_, ()>(false)).unwrap(), 0);
    let mut requests = ChromeRequests::default();
    requests.considered(0);
    requests.request(chrome_request(7), 0);
    let mut chrome = visible_chrome();
    let request = requests.take_ready().unwrap();
    assert!(chrome.request(request.revision, request.hold));
    assert_eq!(chrome.publish(), Some(7));
    let mut presents = vec![(1, chrome.published)];
    assert_eq!(pending, [2]);
    assert_eq!(admit_frame(None, &mut pending, |camera| {
        assert!(!chrome.release_unheld());
        assert_eq!(chrome.publish(), None);
        presents.push((*camera, chrome.published));
        Ok::<_, ()>(true)
    }).unwrap(), 1);
    assert_eq!(presents, [(1, 7), (2, 7)]);
    assert!(pending.is_empty());
    assert!(requests.take_ready().is_none());
    assert_eq!(chrome.synchronized, Some(7));
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
    assert!(chrome.should_hold());
    assert_eq!(chrome.publish(), Some(7));
    assert!(!chrome.can_present());
    assert!(!chrome.release(7));
    assert!(chrome.initial_applied());
    assert!(chrome.request(8, true));
    assert_eq!(chrome.publish(), Some(8));
}

#[test]
fn chrome_initial_revision_zero_still_requires_a_synchronized_first_commit() {
    let mut chrome = ChromeHandoff::default();
    chrome.request(0, false);
    assert!(!chrome.can_present());
    chrome.ready();
    assert!(!chrome.can_present());
    assert!(chrome.grant(0));
    assert!(chrome.should_hold());
    assert_eq!(chrome.publish(), Some(0));
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
    assert!(chrome.should_hold());
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
            presenter.set_panel_surfaces(renderer, &[layer_render_wgpu::PanelSurface { opacity:1.,
                sidebar: [54., 8., 36., 208.], sidebar_radii: [18.; 4], plane: 96.,
                bridge_width: 6., direction: 1., scale: 1., color: [0.5, 0.5, 0.5, 1.],
                anchors: vec![[54., 80., 36., 36.]],
                groups: vec![layer_render_wgpu::PanelSurfaceGroup { opacity:1.,
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
    assert!(chrome.should_hold());
    chrome.request(2, true);
    assert_eq!(chrome.published, 0, "an unacquired buffer cannot acknowledge either request");
    assert_eq!(chrome.publish(), Some(2));
    chrome.request(3, true);
    chrome.request(4, true);
    assert!(chrome.can_present());
    assert_eq!(chrome.published, 2, "new requests must preserve the held buffer's revision");
    assert!(!chrome.release(1));
    assert!(chrome.can_present());
    assert!(chrome.release(2), "release matches the held revision, not the latest request");
    assert!(chrome.can_present());
    assert_eq!(chrome.publish(), Some(4));
    assert!(!chrome.release(2));
    assert!(chrome.release(4));
    assert_eq!(chrome.publish(), None);
    assert!(chrome.can_present());
}

#[test]
fn chrome_release_tracks_the_parent_without_blocking_canvas() {
    let mut chrome = visible_chrome();
    chrome.request(1, true);
    assert_eq!(chrome.publish(), Some(1));
    chrome.request(2, true);
    assert!(chrome.can_present());
    assert!(!chrome.release(0));
    assert!(chrome.can_present());
    assert!(chrome.release(1));
    assert!(chrome.can_present());
    assert_eq!(chrome.publish(), Some(2));
}

#[test]
fn chrome_canvas_preserves_a_held_revision_until_the_parent_release() {
    let mut chrome = visible_chrome();
    chrome.request(1, true);
    assert_eq!(chrome.publish(), Some(1));
    assert_eq!(chrome.synchronized, Some(1));
    assert!(chrome.can_present());
    chrome.request(2, true);
    assert!(!chrome.release_unheld());
    assert!(chrome.should_hold());
    assert_eq!(chrome.publish(), Some(2));
    assert_eq!(chrome.synchronized, Some(2));
    assert!(!chrome.release(1));
    assert!(chrome.release(2));
    assert!(chrome.can_present());
}

#[test]
fn unheld_revisions_never_request_synchronization() {
    let mut chrome = visible_chrome();
    chrome.request(1, true);
    assert_eq!(chrome.publish(), Some(1));
    chrome.request(2, false);
    assert!(chrome.release_unheld());
    assert!(!chrome.should_hold());
    assert_eq!(chrome.publish(), Some(2));
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
            synchronizations += usize::from(chrome.should_hold());
            presents.push(chrome.publish());
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
        assert_eq!(chrome.publish(), Some(1));
        for revision in 2..=4 {
            chrome.request(revision, true);
            assert!(!chrome.needs_publication());
            assert!(chrome.can_present());
            assert_eq!(chrome.synchronized, Some(1));
            assert_eq!(chrome.published, 1);
        }
        if canvas {
            assert!(!chrome.release_unheld());
            assert!(chrome.should_hold());
        } else {
            assert!(chrome.release(1));
        }
        assert!(chrome.can_present());
        assert_eq!(chrome.needs_publication(), !canvas);
        assert_eq!(chrome.publish(), Some(4));
        assert_eq!(chrome.synchronized, Some(4));
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

#[test]
fn a_worker_reply_wakes_the_canvas_that_waits_for_it() {
    let status = Arc::new(ChromeStatus::new(0));
    let (sender, replies) = mpsc::channel();
    let mailbox = Replies { sender, area: gtk::glib::WeakRef::<gtk::Picture>::new().into(), status: status.clone() };
    let seen = status.wakes();
    let mut waiting = std::pin::pin!(status.clone().wake_after(seen));
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(waiting.as_mut().poll(&mut context).is_pending());
    mailbox.send(Reply::ThumbnailCancelled(3)).unwrap();
    assert!(matches!(replies.try_recv(), Ok(Reply::ThumbnailCancelled(3))));
    assert!(waiting.as_mut().poll(&mut context).is_ready(), "the reply wakes the waiting canvas");
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_effect_validation_reply_wakes_its_canvas() {
    let (_app, window, parent, area) = native_worker_window("art.capycanvas.EffectValidationWake");
    let (commands, receiver) = mpsc::channel();
    let (reply, replies) = mpsc::channel();
    let (prepared, ready) = mpsc::channel();
    let status = Arc::new(ChromeStatus::new(0));
    let worker_status = status.clone();
    let worker = std::thread::spawn(move || -> Result<(), String> {
        let mut worker = Worker::new(parent, area, Arc::default(), worker_status, Default::default(), &layer_ui::Localizer::shared(layer_ui::UiLanguage::English))?;
        let document = layer_ui::new_drawing(32, 24, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English))?;
        let brush = layer_core::default_brush(layer_core::DefaultBrushPreset::GPen);
        worker.renderer.prepare_startup(&document, &brush, false).map_err(error)?;
        worker.renderer.finish_startup_catalog();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !worker.renderer.poll_startup().map_err(error)?.complete {
            if std::time::Instant::now() >= deadline { return Err("Renderer startup did not complete".into()); }
            std::thread::sleep(Duration::from_millis(1));
        }
        prepared.send(()).map_err(error)?;
        worker.run(&receiver, &mpsc::channel().1, &reply, &Default::default(), &AtomicUsize::new(0),
            &Default::default(), Arc::default())
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    while ready.try_recv().is_err() {
        assert!(!worker.is_finished() && std::time::Instant::now() < deadline, "the worker prepares its startup");
        pump_worker_window();
    }
    let (mut seen, mut since) = (status.wakes(), std::time::Instant::now());
    while since.elapsed() < Duration::from_secs(1) {
        assert!(std::time::Instant::now() < deadline, "the idle worker stops waking its canvas");
        pump_worker_window();
        if status.wakes() != seen { (seen, since) = (status.wakes(), std::time::Instant::now()); }
    }
    let program = layer_core::bundled_effect_catalog().get("gaussian_blur").expect("bundled blur").program();
    commands.send(Command::EffectValidation(layer_render::EffectValidationRequest {
        request_id: 7, retained_programs: vec![program.clone()], programs: vec![program] })).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let validated = loop {
        if let Ok(Reply::EffectValidation(result)) = replies.try_recv() { break Some(result); }
        if worker.is_finished() || std::time::Instant::now() >= deadline { break None; }
        pump_worker_window();
    };
    let answered = std::time::Instant::now();
    while status.wakes() == seen && answered.elapsed() < Duration::from_secs(1) { pump_worker_window(); }
    let woken = status.wakes() != seen;
    let _ = commands.send(Command::Stop);
    let result = worker.join();
    window.destroy();
    assert!(result.is_ok(), "GPU owner panicked");
    assert_eq!(validated.map(|result| result.request_id), Some(7), "the worker answers the validation request");
    assert!(woken, "a validation reply must wake the canvas that waits for it");
}

#[test]
fn switch_transition_failure_is_ready_before_its_wake() {
    let transition = CanvasTransition::default();
    let (commands, receiver) = mpsc::channel();
    transition.listen(commands);
    assert!(transition.take().is_none());
    drop(TransitionReply(transition.clone(), None));
    assert!(matches!(receiver.try_recv(), Ok(Command::Wake)));
    assert!(matches!(transition.take(), Some(None)));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn switch_transition_completion_before_attach_wakes_the_incoming_worker() {
    let transition = CanvasTransition::default();
    TransitionReply(transition.clone(), None).finish(None);
    let (commands, receiver) = mpsc::channel();
    transition.listen(commands);
    assert!(matches!(receiver.try_recv(), Ok(Command::Wake)));
    assert!(matches!(transition.take(), Some(None)));
}

#[test]
fn switch_transition_forwards_an_unfinished_switch_without_waiting() {
    let first = CanvasTransition::default();
    let next = CanvasTransition::default();
    let (commands, receiver) = mpsc::channel();
    next.listen(commands);
    let old_parent = Arc::new(ParentReleased::default());
    let next_parent = Arc::new(ParentReleased::default());
    first.state.lock().unwrap().forward = Some(TransitionReply(next.clone(), Some(ParentRelease(next_parent.clone()))));
    assert!(first.take().is_none());
    assert!(next.take().is_none());
    assert!(!old_parent.done.load(Ordering::Acquire));
    assert!(!next_parent.done.load(Ordering::Acquire));
    drop(TransitionReply(first, Some(ParentRelease(old_parent.clone()))));
    assert!(old_parent.done.load(Ordering::Acquire));
    assert!(next_parent.done.load(Ordering::Acquire));
    assert!(matches!(receiver.try_recv(), Ok(Command::Wake)));
    assert!(matches!(next.take(), Some(None)));
    assert!(receiver.try_recv().is_err());
}

#[test]
fn switch_transition_receipt_does_not_wait_for_the_producer_lock() {
    let transition = CanvasTransition::default();
    let state = transition.state.lock().unwrap();
    assert!(transition.take().is_none());
    drop(state);
    TransitionReply(transition.clone(), None).finish(None);
    assert!(matches!(transition.take(), Some(None)));
}

#[test]
fn resized_switch_invalidates_the_native_handoff_without_waiting_for_its_producer() {
    let transition = CanvasTransition::new([2560, 1600]);
    let state = transition.state.lock().unwrap();
    transition.resize([2560, 1600]);
    assert!(!transition.invalidated.load(Ordering::Acquire));
    transition.resize([1100, 750]);
    transition.resize([2560, 1600]);
    assert!(transition.invalidated.load(Ordering::Acquire));
    drop(state);
    TransitionReply(transition.clone(), None).finish(None);
    assert!(matches!(transition.take(), Some(None)));
}

#[test]
fn switch_transition_parent_release_wakes_its_async_retirement() {
    use std::{future::Future, task::{Context, Poll, Wake, Waker}};
    struct WakeCount(AtomicUsize);
    impl Wake for WakeCount { fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::Release); } }
    let released = Arc::new(ParentReleased::default());
    let parent = ParentRelease(released.clone());
    let wake = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = Waker::from(wake.clone());
    let mut context = Context::from_waker(&waker);
    let mut wait = Box::pin(released.wait());
    assert_eq!(wait.as_mut().poll(&mut context), Poll::Pending);
    drop(parent);
    assert_eq!(wake.0.load(Ordering::Acquire), 1);
    assert_eq!(wait.as_mut().poll(&mut context), Poll::Ready(()));
}

#[test]
fn reused_child_initial_publication_commits_parent_without_geometry_change() {
    let mut chrome = ChromeHandoff::default();
    chrome.ready();
    assert!(!chrome.needs_parent_commit(false));
    assert!(chrome.grant(0));
    assert!(chrome.should_hold());
    assert!(chrome.needs_parent_commit(false));
    assert_eq!(chrome.publish(), Some(0));
    assert!(!chrome.needs_parent_commit(false));
    assert!(chrome.initial_applied());
    assert!(!chrome.needs_parent_commit(false));
    assert!(chrome.needs_parent_commit(true));
}

#[test]
fn launch_frame_requires_its_gpu_completion_and_parent_commit() {
    for gpu_first in [false, true] {
        let mut frame = LaunchFrame::default();
        frame.request(Some(LaunchRequest::new(7, 3)));
        assert!(frame.submit(Some(4), true).is_none());
        assert!(frame.submit(Some(3), false).is_none());
        let completion = frame.submit(Some(3), true).unwrap();
        assert!(frame.take_presented().is_none());
        if gpu_first { completion.gpu_completed(); } else { completion.parent_committed(); }
        assert!(frame.take_presented().is_none());
        if gpu_first { completion.parent_committed(); } else { completion.gpu_completed(); }
        assert_eq!(frame.take_presented().map(|receipt| receipt.frame), Some(7));
        assert!(frame.take_presented().is_none());
    }
}

#[test]
fn retired_launch_submission_cannot_complete_its_replacement() {
    let mut frame = LaunchFrame::default();
    let previous = LaunchRequest::new(7, 3);
    frame.request(Some(previous.clone()));
    frame.submit(Some(3), true).unwrap();
    let current = LaunchRequest::new(2, 4);
    frame.request(Some(current.clone()));
    frame.submit(Some(4), true).unwrap();
    previous.completion.gpu_completed();
    previous.completion.parent_committed();
    assert!(!current.matches(&previous));
    assert!(frame.take_presented().is_none());
    current.completion.parent_committed();
    assert!(frame.take_presented().is_none());
    current.completion.gpu_completed();
    let receipt = frame.take_presented().unwrap();
    assert_eq!(receipt.frame, 2);
    assert!(current.matches(&receipt));
    assert!(!previous.matches(&receipt));
}

#[test]
fn cancelled_launch_ticket_needs_a_new_submission_even_when_its_digest_is_unchanged() {
    let mut frame = LaunchFrame::default();
    let previous = LaunchRequest::new(7, 3);
    frame.request(Some(previous.clone()));
    frame.submit(Some(3), true).unwrap();
    frame.request(None);
    previous.completion.gpu_completed();
    previous.completion.parent_committed();
    assert!(frame.take_presented().is_none());
    let current = LaunchRequest::new(7, 3);
    assert!(!current.matches(&previous));
    frame.request(Some(current.clone()));
    assert!(frame.take_presented().is_none());
    frame.submit(Some(3), true).unwrap();
    current.completion.gpu_completed();
    assert!(frame.take_presented().is_none());
    current.completion.parent_committed();
    let receipt = frame.take_presented().unwrap();
    assert!(current.matches(&receipt));
    assert!(!previous.matches(&receipt));
}

#[test]
fn launch_receipt_waits_for_the_new_view_even_when_its_frame_dependencies_are_cold() {
    let request = LaunchRequest::new(7, 3);
    let command = Command::LaunchFrame(Some(request.clone()));
    let mut pending = VecDeque::new();
    let mut view = 1;
    assert_eq!(admit_frame(Some(2), &mut pending, |_| Ok::<_, ()>(false)).unwrap(), 0);
    assert!(!command.ready_after_frames(!pending.is_empty()));
    assert!(!pending.is_empty());
    assert_eq!(view, 1);
    let mut launch = LaunchFrame::default();
    assert!(launch.submit(Some(3), true).is_none());
    assert_eq!(admit_frame(None, &mut pending, |next| {
        view = *next;
        Ok::<_, ()>(true)
    }).unwrap(), 1);
    assert!(pending.is_empty());
    assert!(command.ready_after_frames(!pending.is_empty()));
    launch.request(Some(request));
    let completion = launch.submit(Some(3), true).unwrap();
    completion.gpu_completed();
    completion.parent_committed();
    assert_eq!(view, 2);
    assert_eq!(launch.take_presented().unwrap().frame, 7);
    assert!(Command::LaunchFrame(None).ready_after_frames(true));
}

#[test]
fn shared_preparation_keeps_the_gpu_worker_frame_queued_without_submitting() {
    #[derive(Default)]
    struct PreparedRenderer { ready: bool, submissions: usize }
    impl CanvasRenderer for PreparedRenderer {
        type Error = BackendError;
        fn raster_dependencies_ready(&mut self, _: FramePacket<'_>, _: Option<&layer_render::TransformPreview>) -> bool { self.ready }
        fn resize_surface(&mut self, _: u32, _: u32) -> Result<(), Self::Error> { Ok(()) }
        fn prepare_asset(&mut self, _: &AssetId, _: HostImage<'_>) -> Result<(), Self::Error> { Ok(()) }
        fn release_asset(&mut self, _: &AssetId) {}
        fn submit(&mut self, packet: FramePacket<'_>) -> Result<(), Self::Error> {
            assert_eq!(packet.time_seconds, 7.);
            assert!(!packet.restore_rasters.is_empty());
            self.submissions += 1;
            Ok(())
        }
    }
    let (mut worker, commands, _chrome, _reply) = RenderWorker::channels(Default::default());
    let document = layer_ui::new_drawing(32, 24, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    overlay_packet(&mut worker, &document).unwrap();
    let mut frame = queued_frame(&commands);frame.time_seconds = 7.;
    frame.restore_rasters = document.scene().targets().filter_map(|target|document.scene().raster(target).map(|root|(target, root.clone()))).collect();
    let scene = frame.scene.clone();let mut renderer = PreparedRenderer::default();let mut pending = VecDeque::new();
    let draw = |renderer: &mut PreparedRenderer, frame: &mut Box<Frame>| match renderer.submit_when_ready(frame.packet(), frame.transform_preview.as_ref()) {
        std::task::Poll::Pending => Ok(false),
        std::task::Poll::Ready(result) => result.map(|_|true),
    };
    assert_eq!(admit_frame(Some(frame), &mut pending, |frame|draw(&mut renderer, frame)).unwrap(), 0);
    assert_eq!(renderer.submissions, 0);assert_eq!(pending.len(), 1);
    assert!(Arc::ptr_eq(&pending.front().unwrap().scene, &scene));
    renderer.ready = true;
    assert_eq!(admit_frame(None, &mut pending, |frame|draw(&mut renderer, frame)).unwrap(), 1);
    assert_eq!(renderer.submissions, 1);assert!(pending.is_empty());
}


#[derive(Default)]
struct TimedRenderer { ready: bool, readiness: Duration, work: Duration, submissions: usize }
impl CanvasRenderer for TimedRenderer {
    type Error = BackendError;
    fn raster_dependencies_ready(&mut self, _: FramePacket<'_>, _: Option<&layer_render::TransformPreview>) -> bool {
        std::thread::sleep(self.readiness);
        self.ready
    }
    fn resize_surface(&mut self, _: u32, _: u32) -> Result<(), Self::Error> { Ok(()) }
    fn prepare_asset(&mut self, _: &AssetId, _: HostImage<'_>) -> Result<(), Self::Error> { Ok(()) }
    fn release_asset(&mut self, _: &AssetId) {}
    fn submit(&mut self, _: FramePacket<'_>) -> Result<(), Self::Error> {
        std::thread::sleep(self.work);
        self.submissions += 1;
        Ok(())
    }
}
fn timed_packet(document: &layer_core::Document) -> FramePacket<'_> {
    FramePacket {
        time_seconds: 0., view: ViewState { width_px: 32, height_px: 24, document_to_surface: [1., 0., 0., 1., 0., 0.] },
        document_extent: [32, 24], scene: document.scene(), inspect_mask: None, selection_overlays: None,
        dabs: &[], dab_batches: &[], restore_rasters: &[], reset_layers: false, commit_rasters: true,
        composite_all: false, blend_space: Default::default(),
    }
}

#[test]
fn a_ready_frame_observes_its_submission_but_not_its_readiness() {
    let document = layer_ui::new_drawing(32, 24, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let mut renderer = TimedRenderer { ready: true, readiness: Duration::from_millis(40), work: Duration::from_millis(5), ..Default::default() };
    let mut draw_start = std::time::Instant::now();
    assert_eq!(submit_ready_frame(&mut renderer, timed_packet(&document), None, &mut draw_start), std::task::Poll::Ready(Ok(())));
    let observed = draw_start.elapsed();
    assert_eq!(renderer.submissions, 1);
    assert!(observed >= Duration::from_millis(5) && observed < Duration::from_millis(40), "observed draw work {observed:?}");
}

#[test]
fn a_pending_frame_submits_nothing_and_keeps_its_draw_start() {
    let document = layer_ui::new_drawing(32, 24, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    let mut renderer = TimedRenderer { readiness: Duration::from_millis(5), ..Default::default() };
    let started = std::time::Instant::now();
    let mut draw_start = started;
    assert_eq!(submit_ready_frame(&mut renderer, timed_packet(&document), None, &mut draw_start), std::task::Poll::Pending);
    assert_eq!(renderer.submissions, 0);
    assert_eq!(draw_start, started);
}


fn first_preview_at_the_gpu_worker_boundary(pattern: bool, fit: bool) {
    let extent = [1100, 700];
    let mut document = layer_core::Document::new(layer_core::authored::PortableId::random(), extent[0], extent[1],
        layer_core::DocumentNames {paint:"First stroke".into(),paper:"Paper".into()});
    let target = document.working.target.unwrap();
    if pattern {
        let SourceTarget::Paint(handle) = target else {unreachable!()};
        document.artwork.paint.get_mut(handle).unwrap().base = Some(layer_core::authored::PaintBase::new(
            layer_core::color::source::rgba8_source(extent, |x,y| [
                if (x/5+y/7)%2==0 {40}else {220}, if (x/13+y/17)%2==0 {40}else {220},
                if (x/2+y/3)%2==0 {40}else {220}, 255]).into()));
    }
    let view = if fit {ViewState {width_px:330,height_px:210,document_to_surface:[0.2976,0.,0.,0.2976,1.,1.]}}
        else {ViewState {width_px:extent[0],height_px:extent[1],document_to_surface:[1.,0.,0.,1.,0.,0.]}};
    let (mut worker, commands, _chrome, _reply) = RenderWorker::channels(document.composition().color);
    let in_flight=worker.in_flight.clone();
    let mut next_frame=|reset| {
        overlay_packet(&mut worker,&document).unwrap();
        let mut frame=queued_frame(&commands);frame.extent=extent;frame.view=view;frame.reset=reset;frame.composite=reset;frame
    };
    let mut renderer = WgpuRasterizer::new_native_headless(document.composition().color).unwrap();
    renderer.resize_surface(view.width_px,view.height_px).unwrap();
    let execute = |renderer:&mut WgpuRasterizer, frame:Box<Frame>| {
        let deadline=std::time::Instant::now()+layer_core::raster::CAPTURE_TIMEOUT;
        let mut frame=Some(frame);let mut pending=VecDeque::new();
        loop {
            renderer.poll_pending(view).unwrap();
            let admitted=admit_frame(frame.take(),&mut pending,|frame| {
                if !renderer.can_submit() {return Ok(false);}
                match renderer.submit_when_ready(frame.packet(),frame.transform_preview.as_ref()) {
                    std::task::Poll::Pending=>Ok(false),
                    std::task::Poll::Ready(result)=>result.map(|_|true),
                }
            }).unwrap();
            if admitted!=0 {in_flight.fetch_sub(admitted,Ordering::Release);}
            renderer.wait_idle().unwrap();
            if pending.is_empty() {return;}
            assert!(std::time::Instant::now()<deadline,"GPU worker frame dependencies must become ready");
            std::thread::yield_now();
        }
    };
    execute(&mut renderer,next_frame(true));
    let deadline=std::time::Instant::now()+layer_core::raster::CAPTURE_TIMEOUT;
    while !renderer.settled() {
        execute(&mut renderer,next_frame(false));
        assert!(std::time::Instant::now()<deadline,"initial worker picture must settle");
    }
    let before=renderer.readback_srgb_rgba8().unwrap();
    let dab=Dab {center:layer_core::Point {x:200.,y:300.},radii:[60.;2],rotation:[1.,0.],motion:[0.;2],
        color_rgba_linear:[0.9,0.02,0.1,0.8],flow:1.,hardness:1.,texture_sign:[1.;2],material:[0.,0.,1.,0.],
        previous:[0.;4],contact:[0.;4],previous_contact:[0.;4]};
    let style=layer_render::DabStyle::for_brush(&layer_core::default_brush(layer_core::DefaultBrushPreset::GPen),layer_core::StrokeTool::Brush);
    for kind in [layer_render::DabBatchKind::Preview,layer_render::DabBatchKind::Persistent] {
        let mut frame=next_frame(false);frame.dabs=vec![dab];
        frame.batches=vec![DabBatch {preview_end:None,material_update:0,stroke_id:layer_core::StrokeId(1),target,kind,
            stroke_start:true,stroke_end:true,first_dab:0,dab_count:1,style:style.clone(),damage:dab.bounds()}];
        execute(&mut renderer,frame);
    }
    assert_ne!(renderer.readback_srgb_rgba8().unwrap(),before,"the admitted first stroke must change the worker's artwork");
}

#[test]
fn gpu_worker_first_preview_stroke_blank_native() {first_preview_at_the_gpu_worker_boundary(false,false);}
#[test]
fn gpu_worker_first_preview_stroke_blank_fit() {first_preview_at_the_gpu_worker_boundary(false,true);}
#[test]
fn gpu_worker_first_preview_stroke_pattern_native() {first_preview_at_the_gpu_worker_boundary(true,false);}
#[test]
fn gpu_worker_first_preview_stroke_pattern_fit() {first_preview_at_the_gpu_worker_boundary(true,true);}
