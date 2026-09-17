//! Complete-stack color comparison, worker cancellation and atomic GTK adoption.
use super::*;
use layer_color::DocumentColorChange;
use layer_core::color::*;
use layer_render_wgpu::snapshot::CaptureControl;
use std::{
    cell::{Cell, RefCell},
    time::Duration,
};
mod flatten;

const LIMIT: usize = 512 * 1024 * 1024;

#[derive(Clone, Copy)]
struct Choice {
    change: DocumentColorChange,
    flattened: bool,
}
struct Conversion {
    workspace: std::rc::Weak<Workspace>,
    gpu: layer_render_wgpu::snapshot::SnapshotGpu,
    original: Project,
    workflow: RefCell<ColorWorkflow>,
    background: [f32; 4],
    time: f32,
    comparison: Rc<super::preview::Comparison>,
    detail: gtk::Label,
    pending: Cell<Option<Choice>>,
    active: RefCell<Option<CaptureControl>>,
    running: Cell<bool>,
    closed: Cell<bool>,
}
impl Conversion {
    fn request(self: &Rc<Self>, choice: Choice) {
        self.workflow.borrow_mut().candidate = None;
        self.comparison
            .invalidate("Preparing the complete comparison…");
        self.pending.set(Some(choice));
        if let Some(active) = self.active.borrow().as_ref() {
            active.cancel();
        }
        if self.running.get() {
            return;
        }
        self.running.set(true);
        glib::MainContext::default().spawn_local(glib::clone!(#[strong(rename_to = state)] self, async move {
            while let Some(choice) = state.pending.take().filter(|_| !state.closed.get()) {
                if !choice.flattened && choice.change.target(state.original.document.color) == state.original.document.color {
                    state.comparison.invalidate("Choose a different profile or bit depth to compare.");
                    continue;
                }
                let plan = match state.workflow.borrow_mut().select(Some(choice.change), choice.flattened) {
                    Ok(plan) => plan,
                    Err(error) => { state.comparison.invalidate(&error); continue; }
                };
                let control = CaptureControl::default();
                *state.active.borrow_mut() = Some(control.clone());
                let source = state.original.clone();
                let background = state.background;
                let time = state.time;
                let worker_control = control.clone();
                let gpu = state.gpu.clone();
                let result = gio::spawn_blocking(move || {
                    match plan {
                        ColorPreparation::Flatten { color, options } => flatten::prepare(gpu, source, color, options, background, time, worker_control),
                        ColorPreparation::Edit(change) => layer_color::prepare_document_color(&source, change, LIMIT, || worker_control.is_cancelled()),
                        ColorPreparation::History => unreachable!(),
                    }
                }).await.map_err(|_| "Color conversion worker failed".to_string()).and_then(|r| r);
                state.active.borrow_mut().take();
                if control.is_cancelled() || state.closed.get() { continue; }
                let result = result.and_then(|prepared| {
                    let w = state.workspace.upgrade().ok_or("Canvas closed")?;
                    let gpu = w.gpu.borrow();
                    let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
                    state.workflow.borrow().identity.validate(session, control.is_cancelled(), state.gpu.same_device(&w.snapshot_gpu()?))?;
                    let project = prepared.project;
                    state.detail.set_label(if prepared.statistics.clipped_channels > 0 {
                        "Some colors exceed the destination gamut and will be clipped. Compare the complete result before applying."
                    } else if choice.flattened { "The layered original will stay open." }
                    else { "Compare the complete result before applying." });
                    let mut brush = session.engine().configured_brush().clone();
                    let mut view = session.engine().view();
                    layer_render::remap_document_colors(state.original.document.color.space, project.document.color.space, &mut brush, &mut view);
                    state.workflow.borrow_mut().candidate = Some(project.clone());
                    state.comparison.request(project, view.background_rgba_linear, state.time);
                    Ok(())
                });
                if let Err(error) = result { state.comparison.invalidate(&error); }
            }
            state.running.set(false);
        }));
    }
    async fn finish(&self) {
        self.closed.set(true);
        self.pending.set(None);
        if let Some(active) = self.active.borrow().as_ref() {
            active.cancel();
        }
        self.comparison.close();
        while self.running.get() {
            glib::timeout_future(Duration::from_millis(5)).await;
        }
        self.comparison.finish().await;
    }
}

fn choice(title: &str, name: &str, labels: &[&str], subtitle: bool) -> adw::ComboRow {
    let row = adw::ComboRow::builder()
        .title(title)
        .use_subtitle(subtitle)
        .build();
    row.set_expression(Some(gtk::PropertyExpression::new(
        gtk::StringObject::static_type(),
        None::<gtk::Expression>,
        "string",
    )));
    row.set_model(Some(&gtk::StringList::new(labels)));
    row.set_widget_name(name);
    row
}

pub(super) async fn run(
    w: &Rc<Workspace>,
    id: u32,
    operation: DocumentColorOperation,
) -> Result<bool, String> {
    let (workflow, background, time) = {
        let gpu = w.gpu.borrow();
        let session = &gpu.as_ref().ok_or("Canvas unavailable")?.session;
        (
            ColorWorkflow::begin(session, id)?,
            session.engine().view().background_rgba_linear,
            session.engine().animation_time(),
        )
    };
    let project = workflow.original.clone();
    let color = project.document.color;
    let comparison = super::preview::Comparison::new(w.snapshot_gpu()?, project.clone(), w.view_color());
    let detail = gtk::Label::builder()
        .wrap(true)
        .xalign(0.)
        .label("Choose the destination to compare.")
        .build();
    detail.set_widget_name("document-color-detail");
    let group = adw::PreferencesGroup::new();
    let space = choice(
        "Profile",
        "document-color-space",
        &RgbSpace::ALL.map(|s| s.name()),
        false,
    );
    space.set_selected(
        RgbSpace::ALL
            .iter()
            .position(|s| *s == color.space)
            .unwrap() as u32,
    );
    space.set_visible(operation != DocumentColorOperation::Depth);
    group.add(&space);
    let depth = choice(
        "Bit depth",
        "document-color-depth",
        &["8-bit SDR", "16-bit SDR", "16-bit float HDR"],
        false,
    );
    depth.set_selected(if color.depth.is_float() { 2 } else { u32::from(color.depth == SampleDepth::U8) });
    depth.set_visible(operation == DocumentColorOperation::Depth);
    group.add(&depth);
    let result = choice(
        "Result",
        "document-color-result",
        &["Convert editable layers", "Create flattened copy"],
        true,
    );
    result.set_visible(operation == DocumentColorOperation::Convert);
    group.add(&result);
    let intent = choice(
        "Rendering intent",
        "document-color-intent",
        &[
            "Relative colorimetric",
            "Perceptual",
            "Saturation",
            "Absolute colorimetric",
        ],
        true,
    );
    intent.set_visible(operation == DocumentColorOperation::Convert);
    group.add(&intent);
    let bpc = adw::SwitchRow::builder()
        .title("Black point compensation")
        .subtitle("Currently unavailable")
        .active(false)
        .sensitive(false)
        .visible(operation == DocumentColorOperation::Convert)
        .build();
    bpc.set_widget_name("document-color-bpc");
    group.add(&bpc);
    let dither = adw::SwitchRow::builder()
        .title("Reduce banding")
        .subtitle("Dither 8-bit gradients")
        .visible(operation == DocumentColorOperation::Depth)
        .build();
    dither.set_widget_name("document-color-dither");
    group.add(&dither);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 12);
    content.append(&group);
    content.append(&detail);
    content.append(&comparison.widget);
    let request = DocumentRequest::ChangeColor { operation };
    let dialog = adw::AlertDialog::builder().heading(request.title()).body(match operation {
        DocumentColorOperation::Assign => "Keep RGB numbers and change how committed pixels are interpreted. Appearance may change. Retained originals keep their own profiles.",
        DocumentColorOperation::Convert => "Editable layers may change blending and adjustments. A flattened copy preserves their combined appearance as far as gamut and precision allow and keeps the layered original.",
        DocumentColorOperation::Depth => "Change editing precision independently of the profile. Compare reductions before applying. Undo restores the exact original state.",
    }).extra_child(&content).prefer_wide_layout(true).content_width(520).build();
    dialog.set_widget_name("document-color-dialog");
    dialog.add_responses(&[("cancel", "Cancel"), ("apply", "Apply")]);
    dialog.set_close_response("cancel");
    dialog.set_default_response(Some("cancel"));
    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
    dialog.set_response_enabled("apply", false);
    *comparison.changed.borrow_mut() = Some(Box::new(glib::clone!(
        #[weak]
        dialog,
        move |ready| dialog.set_response_enabled("apply", ready)
    )));
    let state = Rc::new(Conversion {
        gpu: w.snapshot_gpu()?,
        workspace: Rc::downgrade(w),
        original: project,
        workflow: RefCell::new(workflow),
        background,
        time,
        comparison,
        detail,
        pending: Cell::new(None),
        active: RefCell::new(None),
        running: Cell::new(false),
        closed: Cell::new(false),
    });
    let refresh: Rc<dyn Fn()> = Rc::new(glib::clone!(
        #[strong]
        state,
        #[weak]
        space,
        #[weak]
        depth,
        #[weak]
        result,
        #[weak]
        intent,
        #[weak]
        bpc,
        #[weak]
        dither,
        #[weak]
        dialog,
        move || {
            let depth = [SampleDepth::U8, SampleDepth::U16, SampleDepth::F16][depth.selected() as usize];
            let intent = [
                RenderingIntent::RelativeColorimetric,
                RenderingIntent::Perceptual,
                RenderingIntent::Saturation,
                RenderingIntent::AbsoluteColorimetric,
            ][intent.selected() as usize];
            bpc.set_sensitive(false);
            dither.set_sensitive(depth == SampleDepth::U8);
            let flattened = operation == DocumentColorOperation::Convert && result.selected() == 1;
            dialog.set_response_label("apply", if flattened { "Create Copy" } else { "Apply" });
            let space = RgbSpace::ALL[space.selected() as usize];
            state.request(Choice {
                flattened,
                change: match operation {
                    DocumentColorOperation::Assign => DocumentColorChange::Assign(space),
                    DocumentColorOperation::Convert => DocumentColorChange::Convert {
                        space,
                        options: ConversionOptions {
                            intent,
                            black_point_compensation: bpc.is_active()
                                && intent != RenderingIntent::AbsoluteColorimetric,
                        },
                    },
                    DocumentColorOperation::Depth => DocumentColorChange::Depth {
                        depth,
                        dither: if depth == SampleDepth::U8 && dither.is_active() {
                            OutputDither::Stochastic8
                        } else {
                            OutputDither::None
                        },
                    },
                },
            });
        }
    ));
    for row in [&space, &depth, &result, &intent] {
        row.connect_selected_notify({
            let refresh = refresh.clone();
            move |_| refresh()
        });
    }
    for row in [&bpc, &dither] {
        row.connect_active_notify({
            let refresh = refresh.clone();
            move |_| refresh()
        });
    }
    refresh();
    let response = crate::alert::choose(dialog, &w.window).await;
    let compared = state.comparison.ready.get();
    state.finish().await;
    if response != "apply" {
        return Ok(false);
    }
    if !compared { return Err("No completed color comparison".into()); }
    state.workflow.borrow_mut().comparison_completed()?;
    if state.workflow.borrow().is_copy() {
        let project = state.workflow.borrow().copy_project(false)?.clone();
        w.open_document.borrow().as_ref().ok_or("New drawing window is unavailable")?(project, None, None);
        Ok(true)
    } else {
        adopt(w, &mut state.workflow.borrow_mut(), &state.gpu).await
    }
}

pub(super) async fn history(w: &Rc<Workspace>, id: u32) -> Result<bool, String> {
    let mut workflow = {
        let gpu = w.gpu.borrow();
        ColorWorkflow::begin(&gpu.as_ref().ok_or("Canvas unavailable")?.session, id)?
    };
    workflow.select(None, false)?;
    adopt(w, &mut workflow, &w.snapshot_gpu()?).await
}

async fn adopt(
    w: &Rc<Workspace>,
    workflow: &mut ColorWorkflow,
    original_gpu: &layer_render_wgpu::snapshot::SnapshotGpu,
) -> Result<bool, String> {
    {
        let mut gpu = w.gpu.borrow_mut();
        let session = &mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
        workflow.identity.validate(session, false, original_gpu.same_device(&session.engine().backend().snapshot_gpu()?))?;
        let project = workflow.candidate.as_ref().ok_or("Color candidate is missing")?.clone();
        let mut brush = session.engine().configured_brush().clone();
        let mut view = session.engine().view();
        layer_render::remap_document_colors(workflow.original.document.color.space, project.document.color.space, &mut brush, &mut view);
        let time = session.engine().animation_time();
        session
            .renderer_mut()
            .prepare_color(project, brush, view, time)?;
    }
    let dialog = adw::AlertDialog::builder()
        .heading("Preparing color change")
        .body(
            "Preparing the complete canvas. Your current drawing is unchanged until this finishes.",
        )
        .build();
    dialog.set_widget_name("document-color-progress");
    dialog.add_response("cancel", "Cancel");
    dialog.set_close_response("cancel");
    let cancelled = Rc::new(Cell::new(false));
    dialog.connect_response(None, {
        let cancelled = cancelled.clone();
        move |_, _| cancelled.set(true)
    });
    dialog.present(Some(&w.window));
    let result = loop {
        if cancelled.get() {
            break Ok(false);
        }
        let result = w
            .gpu
            .borrow_mut()
            .as_mut()
            .ok_or("Canvas unavailable")?
            .session
            .renderer_mut()
            .poll_prepared_color();
        if let Some(result) = result {
            break result.map(|_| true);
        }
        glib::timeout_future(Duration::from_millis(5)).await;
    };
    // Close before dropping the cancellation choice; no await separates commit
    // from its final check, so Cancel cannot race model publication.
    let should_commit = matches!(result, Ok(true)) && !cancelled.get();
    dialog.force_close();
    let result = if should_commit {
        let mut gpu = w.gpu.borrow_mut();
        let session = &mut gpu.as_mut().ok_or("Canvas unavailable")?.session;
        let prepared = workflow.prepare_commit(session, false, original_gpu.same_device(&session.engine().backend().snapshot_gpu()?));
        let change = prepared.and_then(|p| session.commit_document_color_candidate(p, |_| {}));
        drop(gpu);
        match change {
            Ok(change) => { w.changed(Ok(change)); Ok(true) }
            Err(error) => Err(error),
        }
    } else {
        result
    };
    let acknowledgement = w
        .gpu
        .borrow_mut()
        .as_mut()
        .ok_or("Canvas unavailable")?
        .session
        .renderer_mut()
        .discard_prepared_color()?;
    while matches!(
        acknowledgement.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ) {
        glib::timeout_future(Duration::from_millis(5)).await;
    }
    result
}
