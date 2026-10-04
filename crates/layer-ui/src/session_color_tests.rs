// Included in session::tests, using the protocol recorder (no simulated pixels).
#[test]
fn tone_preview_survives_edits_but_not_document_replacement() {
    use crate::proof_workflow::ToneKey;
    use layer_core::color::{SampleDepth,hdr::SdrRendition};
    let mut document=Document::new(layer_core::PortableId::random(),32,32, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() }); document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth=SampleDepth::F32;
    let make = || UiSession::new(Recorder {color:document.composition().color,..Default::default()},document.clone(),[32,32], Platform::Gtk).unwrap();
    let mut s=make();
    let original=ToneKey::current(&s).unwrap();
    s.dispatch(UiAction::Invoke {command:CommandId::AddLayer}).unwrap();
    let edited=ToneKey::current(&s).unwrap();
    assert!(edited != original && original.can_preview(&edited));
    s.edit_sdr_rendition(ContactPhase::Down,SdrRendition {exposure:2.,..Default::default()}).unwrap();
    assert!(ToneKey::current(&s).unwrap() == edited,"appearance changes reuse exact analysis");
    s.cancel_sdr_gesture().unwrap();
    let mut replacement=make();
    replacement.inherit_window_state(&s).unwrap();
    let replacement=ToneKey::current(&replacement).unwrap();
    assert!(!original.can_preview(&replacement),"same-size replacement must reject old illumination");
}

#[test]
fn print_panel_first_use_has_no_target_and_off_rejects_late_publication() {
    use crate::proof_workflow::{proof_form,ProofPreparation};
    use layer_core::color::{ColorProfile,ProofRecipe,RgbSpace};
    let mut s=session(Platform::Gtk);
    let before=s.engine.checkpoint();
    let form=proof_form(&s);
    assert!(form["print_settings"]["profile"].is_null());
    assert_eq!(form["print_settings"]["simulation"],"black_ink");
    assert_eq!(form["print_controls"].as_array().unwrap().iter().map(|v|v["label"].as_str().unwrap()).collect::<Vec<_>>(),["Profile","Simulate","Intent","Black point compensation","Gamut warning"]);
    s.select_proof_mode(ProofMode::Print).unwrap();
    let job=ProofPreparation::panel(&s,ProofRecipe::new("sRGB".into(),ColorProfile::Builtin(RgbSpace::Srgb))).unwrap();
    job.validate(&s).unwrap();
    s.select_proof_mode(ProofMode::Off).unwrap();
    assert!(job.apply(&mut s,true).is_err());
    assert_eq!(s.engine.checkpoint(),before);
    assert!(s.engine.document().output().proof.is_none());
}

#[test]
fn portable_proof_workflow_preserves_original_before_history_and_rejects_stale_jobs() {
    use crate::proof_workflow::{ProofPreparation, ProofView};
    use layer_core::color::{ColorProfile, ProofRecipe, RgbSpace};
    for platform in [Platform::Web, Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
        let mut s = session(platform);
        let bytes = layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap();
        let original = ProofRecipe::new("Embedded P3".into(), ColorProfile::Icc(bytes.clone().into()));
        s.set_proof_recipe(Some(original.clone())).unwrap();
        s.files.saved_checkpoint = s.engine.checkpoint();
        s.refresh_file_state();
        s.dispatch(UiAction::Invoke { command: CommandId::SoftProofSetup }).unwrap();
        let id = s.state.requests.last().unwrap().id;
        let replacement = ProofRecipe::new("sRGB".into(), ColorProfile::Builtin(RgbSpace::Srgb));
        let job = ProofPreparation::begin(&s, Some(id), Some(replacement.clone())).unwrap();
        assert_eq!(job.preservation(), Some(bytes.as_slice()));
        let checkpoint = s.engine.checkpoint();
        assert!(job.apply(&mut s, false).is_err());
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_eq!(s.engine.document().output().proof, Some(original.clone()));
        let form = crate::proof_workflow::proof_form(&s);
        assert_eq!(form["document_profile"]["name"], "Embedded P3");
        // Cancel rejects a late result and has not mutated the original.
        s.dispatch(UiAction::CompleteRequest { id, error: None }).unwrap();
        assert!(job.apply(&mut s, true).is_err());
        assert_eq!(s.engine.checkpoint(), checkpoint);
        s.dispatch(UiAction::Invoke { command: CommandId::SoftProofSetup }).unwrap();
        let id = s.state.requests.last().unwrap().id;
        let job = ProofPreparation::begin(&s, Some(id), Some(replacement.clone())).unwrap();
        job.apply(&mut s, true).unwrap();
        assert_eq!(s.engine.document().output().proof, Some(replacement));
        assert!(s.state.soft_proof && s.state.document_file.modified);
        assert!(job.validate(&s).is_err());
        let mut view = ProofView::default();
        assert!(view.observe(&s).needed);
        let prepare = ProofPreparation::begin(&s, None, None).unwrap();
        view.fail(&s, &prepare, "Unavailable profile".into());
        assert!(!view.observe(&s).needed);
        assert_eq!(view.observe(&s).text, "Proof unavailable");
        s.dispatch(UiAction::Invoke { command: CommandId::SoftProof }).unwrap();
        assert_eq!(view.observe(&s).text, "");
        s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
        assert_eq!(s.engine.document().output().proof, Some(original));
        assert!(!s.state.document_file.modified);
        assert!(prepare.validate(&s).is_err());
        // First use is enabled on these ports and leaves toggles off until Apply.
        s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
        assert!(s.engine.document().output().proof.is_none());
        s.dispatch(UiAction::Invoke { command: CommandId::SoftProof }).unwrap();
        assert!(!s.state.soft_proof);
        if platform == Platform::Windows {
            assert!(s.state.requests.is_empty());
            assert_eq!(s.proof_panel_mode(), crate::ProofMode::Print);
        } else {
            assert!(matches!(s.state.requests.last().unwrap().kind, HostRequestKind::SoftProofSetup));
        }
    }
}

#[test]
fn proof_colors_first_use_opens_setup_without_enabling_or_editing() {
    let mut s = session(Platform::Gtk);
    let original = s.engine.document().clone();
    let checkpoint = s.engine.checkpoint();
    assert!(s.command(CommandId::SoftProof).enabled);
    assert!(!s.command(CommandId::GamutWarning).enabled);

    // Opening another document blocks first-use setup just like explicit setup.
    s.dispatch(UiAction::Invoke { command: CommandId::OpenDocument }).unwrap();
    assert!(!s.command(CommandId::SoftProof).enabled);
    assert!(s.dispatch(UiAction::Invoke { command: CommandId::SoftProof }).is_err());
    let id = s.state.requests.first().unwrap().id;
    s.complete_document_request(id, Ok(false)).unwrap();

    // Leaving the setup page with Off allows trying again; completing the
    // reveal request alone must not dismiss a nonmodal panel's selection.
    for _ in 0..2 {
        s.dispatch(UiAction::Invoke { command: CommandId::SoftProof }).unwrap();
        assert_eq!(s.state.requests.len(), 1);
        let request = s.state.requests.first().unwrap();
        assert!(matches!(request.kind, HostRequestKind::SoftProofSetup));
        let id = request.id;
        assert!(!s.state.soft_proof && !s.state.gamut_warning);
        assert!(!s.command(CommandId::SoftProof).selected);
        s.dispatch(UiAction::CompleteRequest { id, error: None }).unwrap();
        assert_eq!(s.proof_panel_mode(), ProofMode::Print);
        s.select_proof_mode(ProofMode::Off).unwrap();
        assert_eq!(s.engine.document(), &original);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert!(s.command(CommandId::SoftProof).enabled);
    }
}

#[test]
fn proof_recipe_history_is_separate_from_comparison_and_delivery() {
    use layer_core::color::{ColorProfile, ProofRecipe};
    let mut s = session(Platform::Gtk);
    let original = s.engine.document().clone();
    assert!(s.command(CommandId::SoftProof).enabled);
    let recipe = ProofRecipe::new("Lab paper".into(), ColorProfile::default());
    s.set_proof_recipe(Some(recipe.clone())).unwrap();
    assert!(s.state.soft_proof);
    assert!(s.state.document_file.modified);
    let mut expected = original.clone(); expected.artwork.outputs = s.engine.document().artwork.outputs.clone();
    assert_live_artwork_eq(s.engine.document(), &expected);
    s.files.saved_checkpoint = s.engine.checkpoint();
    s.refresh_file_state();
    let saved = s.engine.document().clone();
    for command in [CommandId::SoftProof, CommandId::GamutWarning, CommandId::SoftProof, CommandId::GamutWarning] {
        s.dispatch(UiAction::Invoke { command }).unwrap();
        assert_eq!(s.engine.document(), &saved);
        assert!(!s.state.document_file.modified);
    }
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    assert!(s.engine.document().output().proof.is_none());
    assert!(!s.state.soft_proof && !s.state.gamut_warning);
    let mut expected = original.clone(); expected.artwork.outputs = s.engine.document().artwork.outputs.clone();
    assert_live_artwork_eq(s.engine.document(), &expected);
    s.dispatch(UiAction::Invoke { command: CommandId::Redo }).unwrap();
    assert_eq!(s.engine.document().output().proof, Some(recipe.clone()));
    assert!(!s.state.document_file.modified);
    assert!(!s.state.soft_proof, "restoring a recipe does not enable a temporary view");
    s.set_platform(Platform::Windows);
    assert!(s.command(CommandId::SoftProofSetup).enabled);
    assert!(s.set_proof_recipe(Some(recipe)).is_ok());
}

#[test]
fn color_transitions_update_picker_coordinates_and_route_exact_history_through_the_host() {
    use layer_core::{ColorTransition, color::{DocumentColor, SampleDepth, RgbColor, RgbSpace}};
    let mut s = session(Platform::Gtk);
    let definition = RgbColor::new(RgbSpace::DisplayP3, [0.8, 0.3, 0.1, 1.]).unwrap();
    s.dispatch(UiAction::Color { action: ColorAction::Definition { color: definition } }).unwrap();
    s.frame(1, 1).unwrap();
    let before = s.engine.document().clone();
    let color = DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 };
    let prepare = |s: &UiSession<Recorder>| {
        let document = s.engine.document();
        let paint = document.artwork.paint.iter().map(|(handle, _, source)| layer_core::RecordChange::replace(&document.artwork.paint, handle, Some(source.clone())).unwrap()).collect();
        let coverage = document.artwork.coverage.iter().map(|(handle, _, source)| layer_core::RecordChange::replace(&document.artwork.coverage, handle, Some(source.clone())).unwrap()).collect();
        let edit = document.color_edit(color, paint, coverage).unwrap();
        s.prepare_document_color_transition(ColorTransition::Apply { edit: Box::new(edit) }).unwrap().0
    };
    let prepared = prepare(&s);
    assert!(s.commit_document_color_transition(prepared).unwrap_err().contains("not ready"));
    assert_eq!(s.engine.document(), &before);
    assert_eq!(s.state.colors.rgb_space(), before.composition().color.space);
    s.renderer_mut().prepared_color = Some(color);
    s.commit_document_color_transition(prepare(&s)).unwrap();
    assert_eq!(s.state.colors.rgb_space(), color.space);
    assert_eq!(s.state.colors.definition(), definition);
    for (a, b) in s.engine.configured_brush().color_rgba_linear.into_iter().zip(definition.linear_in(color.space).unwrap()) {
        assert!((a - b).abs() < 2e-7);
    }
    let after = s.engine.document().clone();
    for (redo, expected) in [(false, &before), (true, &after)] {
        s.dispatch(UiAction::Invoke { command: if redo { CommandId::Redo } else { CommandId::Undo } }).unwrap();
        let request = s.state.requests.first().unwrap();
        let id = request.id;
        assert!(matches!(request.kind, HostRequestKind::Document { request: DocumentRequest::ColorHistory { redo: value } } if value == redo));
        let (prepared, project) = s.prepare_document_color_transition(if redo { ColorTransition::Redo } else { ColorTransition::Undo }).unwrap();
        assert_eq!(project.composition().color, expected.composition().color);
        s.renderer_mut().prepared_color = Some(expected.composition().color);
        s.commit_document_color_transition(prepared).unwrap();
        s.complete_document_request(id, Ok(true)).unwrap();
        assert_live_artwork_eq(s.engine.document(), expected);
        assert_eq!(s.engine.document().composition().color, expected.composition().color);
        assert_eq!(s.state.colors.rgb_space(), expected.composition().color.space);
        assert_eq!(s.state.colors.definition(), definition);
        assert!(!s.state.document_file.busy);
    }
}

#[test]
fn portable_colors_follow_documents_workspaces_brushes_and_samples() {
    use layer_core::color::{DocumentColor, SampleDepth, RgbColor, RgbSpace};
    use layer_render::{ColorSample, ColorSampleSource};
    let definition = RgbColor::new(RgbSpace::DisplayP3, [1., 0., 0., 0.37]).unwrap();
    let make = |space| {
        let color = DocumentColor {
            space,
            depth: SampleDepth::U16,
        };
        let mut document = Document::new(layer_core::PortableId::random(), 1000, 1000, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        document.artwork.compositions.get_mut(document.artwork.root).unwrap().color = color;
        UiSession::new(
            Recorder {
                color,
                ..Default::default()
            },
            document,
            [1000; 2],
            Platform::Gtk,
        )
        .unwrap()
    };
    let mut s = make(RgbSpace::DisplayP3);
    s.dispatch(UiAction::Color {
        action: ColorAction::Definition { color: definition },
    })
    .unwrap();
    s.dispatch(UiAction::SetBrushOpacity { value: 0.23 })
        .unwrap();
    let capture = s.capture_workspace().unwrap();
    let encoded = serde_json::to_vec(&capture).unwrap();
    for space in RgbSpace::ALL {
        let mut c = make(space);
        c.inherit_initial_drawing_tools(&s).unwrap();
        let revision = c.engine.document().revision;
        assert_eq!(c.state.colors.definition(), definition);
        assert_eq!(c.state.colors.rgb_space(), space);
        assert_eq!(
            c.engine.configured_brush().color_rgba_linear,
            definition.linear_in(space).unwrap()
        );
        assert_eq!(c.state.brush.opacity, 0.23);
        let prepared = PreparedWorkspace::new(serde_json::from_slice(&encoded).unwrap()).unwrap();
        c.adopt_workspace(prepared).unwrap();
        assert_eq!(c.state.colors.rgb_space(), space);
        assert_eq!(c.state.colors.definition(), definition);
        assert_eq!(
            c.engine.configured_brush().color_rgba_linear,
            definition.linear_in(space).unwrap()
        );
        for preset in [
            DefaultBrushPreset::GPen,
            DefaultBrushPreset::WetRound,
            DefaultBrushPreset::GPen,
        ] {
            c.select_brush(preset as u32).unwrap();
            assert_eq!(
                c.engine.configured_brush().color_rgba_linear,
                definition.linear_in(space).unwrap()
            );
            let secondary = default_brush(preset)
                .color_dynamics
                .secondary_color_rgba_linear;
            let expected = RgbColor::from_linear(RgbSpace::Srgb, secondary)
                .unwrap()
                .linear_in(space)
                .unwrap();
            for (a, b) in c
                .engine
                .configured_brush()
                .color_dynamics
                .secondary_color_rgba_linear
                .into_iter()
                .zip(expected)
            {
                assert!((a - b).abs() < 1e-7);
            }
        }
        // Sampling preserves extended straight document RGB; alpha controls
        // whether a sample exists, while paint opacity remains independent.
        let opacity = c.state.brush.opacity;
        c.eyedropper.queue(ColorSampleSource::Composite, [32, 32]);
        c.frame(1, 1).unwrap();
        let request = *c.renderer_mut().sample_requests.last().unwrap();
        c.renderer_mut().sample_reply = Some(ColorSample {
            request_id: request.request_id,
            rgba: [-0.1, 1.2, 0.4, 0.00001],
        });
        c.frame(2, 2).unwrap();
        assert_eq!(c.state.colors.definition().space, space);
        for (a, b) in c
            .engine
            .configured_brush()
            .color_rgba_linear
            .into_iter()
            .zip([-0.1, 1.2, 0.4, 1.])
        {
            assert!((a - b).abs() < 2e-7, "{space:?}: {a} != {b}");
        }
        assert_eq!(c.state.brush.opacity, opacity);
        assert_eq!(c.engine.document().revision, revision);
    }
}

#[test]
fn figures_and_gradients_convert_both_portable_paints() {
    use layer_core::color::{DocumentColor, SampleDepth, RgbColor, RgbSpace};
    let color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    let mut document = Document::new(layer_core::PortableId::random(), 1000, 1000, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color = color;
    let mut s = UiSession::new(
        Recorder {
            color,
            ..Default::default()
        },
        document,
        [1000; 2],
        Platform::Gtk,
    )
    .unwrap();
    let foreground = RgbColor::new(RgbSpace::DisplayP3, [1., 0.1, 0.3, 0.37]).unwrap();
    let background = RgbColor::new(RgbSpace::AdobeRgb, [0.1, 1., 0.3, 0.73]).unwrap();
    s.state.colors.foreground = foreground;
    s.state.colors.background = background;
    s.state.brush.opacity = 0.23;
    s.layer_interaction.tool = LayerCanvasTool::Figure {
        shape: FigureShape::Rectangle,
        paint: FigurePaint::Both,
    };
    s.layer_interaction.path = vec![Point { x: 10., y: 10. }, Point { x: 60., y: 60. }];
    let figure = s.current_figure().unwrap();
    let expected = [foreground, background].map(|c| {
        let mut rgba = c.linear_in(color.space).unwrap();
        rgba[3] *= 0.23;
        rgba
    });
    assert_eq!(figure.colors, expected);
    // Exercise the gradient through its public canvas-tool and contact path.
    s.cancel_layer_gesture().unwrap();
    layer(&mut s, LayerAction::Tool { tool: LayerCanvasTool::Gradient {shape:layer_core::GradientShape::Linear}, });
    let mut down = event(&s, 1, PenPhase::Down, 1.);
    down.surface_position = Point { x: 50., y: 50. };
    s.pen(down).unwrap();
    let mut up = event(&s, 2, PenPhase::Up, 1.);
    up.surface_position = Point { x: 250., y: 250. };
    s.pen(up).unwrap();
    s.frame(10, 10).unwrap();
    let color_space_for_gradient=s.engine.document().composition().color.space;
    let operations = &s.renderer_mut().pending_operations;
    let actual = operations
        .iter()
        .find_map(|(_, operation)| {
            if let layer_core::RasterOperationKind::Gradient { gradient, opacity, .. } = &operation.kind {
                Some([gradient.stops[0].color,gradient.stops[1].color].map(|color|{let mut rgba=color.linear_in(color_space_for_gradient).unwrap();rgba[3]*=*opacity;rgba}))
            } else {
                None
            }
        })
        .expect("gradient operation");
    assert_eq!(actual, expected);
}

#[test]
fn color_workflow_validates_choices_comparison_identity_and_rolls_back_renderer() {
    use crate::{ColorWorkflow, ColorPreparation};
    use layer_color::DocumentColorChange as C;
    use layer_core::color::{RgbSpace, SampleDepth};
    let mut s = session(Platform::Gtk);
    s.frame(1, 1).unwrap();
    invoke(&mut s, CommandId::AssignProfile);
    let id = s.state.requests.first().unwrap().id;
    let mut workflow = ColorWorkflow::begin(&s, id).unwrap();
    assert!(workflow.select(Some(C::Depth { depth: SampleDepth::U16, dither: layer_core::color::OutputDither::None }), false).is_err());
    assert!(workflow.select(Some(C::Assign(RgbSpace::DisplayP3)), true).is_err());
    let ColorPreparation::Edit(change) = workflow.select(Some(C::Assign(RgbSpace::DisplayP3)), false).unwrap() else { panic!() };
    workflow.candidate = Some(layer_color::prepare_document_color(&workflow.original, change, 1024 * 1024, || false).unwrap().document);
    assert!(workflow.prepare_commit(&s, false, true).err().unwrap().contains("Preview"));
    workflow.comparison_completed().unwrap();
    assert!(workflow.prepare_commit(&s, true, true).is_err());
    assert!(workflow.prepare_commit(&s, false, false).is_err());
    let revision = s.engine.document().revision;
    let before = s.engine.document().clone();
    let prepared = workflow.prepare_commit(&s, false, true).unwrap();
    let mut swaps = 0;
    assert!(s.commit_document_color_candidate(prepared, |_| swaps += 1).is_err());
    assert_eq!(swaps, 2, "failed history must restore the original renderer");
    assert_eq!(s.engine.document(), &before);
    assert_eq!(s.engine.document().revision, revision);
    let prepared = workflow.prepare_commit(&s, false, true).unwrap();
    s.renderer_mut().prepared_color = Some(workflow.candidate.as_ref().unwrap().composition().color);
    s.commit_document_color_candidate(prepared, |_| {}).unwrap();
    assert!(workflow.identity.validate(&s, false, true).is_err(), "revision advanced");
    s.complete_document_request(id, Ok(true)).unwrap();
    assert!(workflow.identity.validate(&s, false, true).is_err());
    let after = s.engine.document().clone();
    for (command, expected) in [(CommandId::Undo, &before), (CommandId::Redo, &after)] {
        invoke(&mut s, command);
        let id = s.state.requests.first().unwrap().id;
        let mut history = ColorWorkflow::begin(&s, id).unwrap();
        assert!(history.select(Some(C::Assign(RgbSpace::Srgb)), false).is_err());
        assert!(history.select(None, true).is_err());
        history.select(None, false).unwrap();
        s.renderer_mut().prepared_color = Some(expected.composition().color);
        let prepared = history.prepare_commit(&s, false, true).unwrap();
        assert!(history.prepare_commit(&s, false, true).is_err(), "a consumed Undo/Redo candidate cannot become a new edit");
        s.commit_document_color_candidate(prepared, |_| {}).unwrap();
        s.complete_document_request(id, Ok(true)).unwrap();
        assert_live_artwork_eq(s.engine.document(), expected);
        assert_eq!(s.engine.document().composition().color, expected.composition().color);
    }
    invoke(&mut s, CommandId::ConvertColorSpace);
    let id = s.state.requests.first().unwrap().id;
    let mut copy = ColorWorkflow::begin(&s, id).unwrap();
    copy.select(Some(C::Convert { space: RgbSpace::ProPhoto, options: Default::default() }), true).unwrap();
    copy.candidate = Some(copy.original.clone());
    assert!(copy.copy_project(false).is_err());
    copy.comparison_completed().unwrap();
    assert!(copy.copy_project(true).is_err());
    assert!(copy.copy_project(false).is_ok());
    assert!(copy.prepare_commit(&s, false, true).is_err());
    s.complete_document_request(id, Ok(false)).unwrap();
    assert!(copy.identity.validate(&s, false, true).is_err(), "closed request must stay closed");
}

#[test]
fn sdr_preview_follows_display_capability_and_rendition_edits_undo() {
    use layer_core::color::{SampleDepth, hdr::SdrRendition};
    let mut document = Document::new(layer_core::PortableId::random(), 32, 32, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth = SampleDepth::F16;
    let renderer = Recorder { color: document.composition().color, ..Default::default() };
    let mut s = UiSession::new(renderer, document, [32, 32], Platform::Gtk).unwrap();
    let original = s.engine.document().clone();
    assert!(!s.command(CommandId::PreviewSdr).enabled);
    let published = s.workspace_update().model_revision;
    s.set_hdr_display_available(true);
    assert!(s.command(CommandId::PreviewSdr).enabled);
    assert!(s.workspace_update().model_revision > published, "Display capability republishes command availability");
    let recipe = SdrRendition { exposure: -2., ..Default::default() };
    s.set_sdr_rendition(recipe).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    assert_eq!(s.engine.document().output().sdr, original.output().sdr);
    s.dispatch(UiAction::Invoke { command: CommandId::Redo }).unwrap();
    assert_eq!(s.engine.document().output().sdr, recipe);
    s.state.soft_proof = true; s.refresh_commands();
    assert!(!s.command(CommandId::PreviewSdr).enabled);
    s.state.soft_proof = false; s.set_hdr_display_available(false);
    assert!(!s.command(CommandId::PreviewSdr).enabled);
}

#[test]
fn live_sdr_panel_gesture_commits_once_and_cancels_without_losing_redo() {
    use layer_core::color::{SampleDepth,hdr::SdrRendition};
    let mut document=Document::new(layer_core::PortableId::random(),32,32, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth=SampleDepth::F16;
    let renderer=Recorder{color:document.composition().color,..Default::default()};
    let mut s=UiSession::new(renderer,document,[32,32], Platform::Gtk).unwrap();
    let original=s.engine.document().output().sdr;
    let changed=SdrRendition{exposure:1.5,highlight_color:0.65,..original};
    s.set_proof_mode(ProofMode::Sdr).unwrap();
    let checkpoint=s.engine.checkpoint();
    s.edit_sdr_rendition(ContactPhase::Down,original).unwrap();
    for exposure in [0.2,0.8,1.5]{s.edit_sdr_rendition(ContactPhase::Move,SdrRendition{exposure,..changed}).unwrap();}
    assert_eq!(s.engine.document().output().sdr,changed);
    assert_eq!(s.engine.checkpoint(),checkpoint);
    assert!(s.capture_artwork().is_err(),"Recovery must not capture an unfinished contact");
    assert!(!s.command(CommandId::ExportDocument).enabled);
    s.edit_sdr_rendition(ContactPhase::Up,changed).unwrap();
    assert!(s.state.document_file.modified);
    assert_eq!(s.capture_artwork().unwrap().output().sdr,changed);
    s.dispatch(UiAction::Invoke{command:CommandId::Undo}).unwrap();
    assert_eq!(s.engine.document().output().sdr,original);
    s.edit_sdr_rendition(ContactPhase::Down,original).unwrap();
    s.edit_sdr_rendition(ContactPhase::Move,changed).unwrap();
    s.edit_sdr_rendition(ContactPhase::Cancel,changed).unwrap();
    assert_eq!(s.engine.document().output().sdr,original);
    assert_eq!(s.engine.checkpoint(),checkpoint);
    s.dispatch(UiAction::Invoke{command:CommandId::Redo}).unwrap();
    assert_eq!(s.engine.document().output().sdr,changed);
    let checkpoint=s.engine.checkpoint();
    s.set_proof_mode(ProofMode::Off).unwrap();
    assert_eq!(s.engine.checkpoint(),checkpoint);
    assert_eq!(s.engine.document().output().sdr,changed);
    assert!(!s.state.preview_sdr && !s.state.soft_proof);
}

#[test]
fn proof_modes_share_view_state_and_preserve_both_saved_recipes() {
    use layer_core::color::{SampleDepth,ProofRecipe,ColorProfile,RgbSpace};
    let mut document=Document::new(layer_core::PortableId::random(),32,32, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth=SampleDepth::F16;
    let renderer=Recorder{color:document.composition().color,..Default::default()};
    let mut s=UiSession::new(renderer,document,[32,32], Platform::Gtk).unwrap();
    assert!(s.set_proof_mode(ProofMode::Print).is_err());
    s.set_proof_recipe(Some(ProofRecipe::new("Printer".into(),ColorProfile::Builtin(RgbSpace::Srgb)))).unwrap();
    let original=s.engine.document().clone();let checkpoint=s.engine.checkpoint();
    for mode in [ProofMode::Sdr,ProofMode::Print,ProofMode::Off]{
        s.set_proof_mode(mode).unwrap();assert_eq!(s.proof_mode(),mode);
        assert_eq!(s.engine.document(),&original);assert_eq!(s.engine.checkpoint(),checkpoint);
    }
    let menu=s.application_menu(ApplicationMenu::View);
    let labels=menu.sections.iter().flatten().map(|i|i.label.as_str()).collect::<Vec<_>>();
    assert!(labels.contains(&"Proof"));
    assert!(!labels.contains(&"Proof…"));
    assert!(!labels.contains(&"Preview SDR"));assert!(!labels.contains(&"Soft Proof"));
}

#[test]
fn proof_toggle_remembers_mode_and_keeps_pending_setup_separate_from_rendering() {
    use layer_core::color::{SampleDepth, ProofRecipe, ColorProfile};
    let mut document = Document::new(layer_core::PortableId::random(), 32, 32, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth = SampleDepth::F16;
    let renderer = Recorder { color: document.composition().color, ..Default::default() };
    let mut s = UiSession::new(renderer, document, [32, 32], Platform::Gtk).unwrap();
    let toggle = |s: &mut UiSession<Recorder>| {
        s.dispatch(UiAction::Invoke { command: CommandId::SoftProof }).unwrap();
        // Only enabling asks the host to reveal the panel, including setup.
        let reveals = s.state.requests.iter().filter(|r| matches!(r.kind, HostRequestKind::SoftProofSetup)).count();
        assert_eq!(reveals, usize::from(s.proof_panel_mode() != ProofMode::Off));
        for request in s.state.requests.clone() {
            s.dispatch(UiAction::CompleteRequest { id: request.id, error: None }).unwrap();
        }
        let menu = s.application_menu(ApplicationMenu::View);
        let item = menu.sections.iter().flatten().find(|i| i.label == "Proof").unwrap();
        assert_eq!(item.selected, Some(s.proof_mode() != ProofMode::Off));
        assert!(!item.hint.is_empty());
    };
    let checkpoint = s.engine.checkpoint();
    for expected in [ProofMode::Sdr, ProofMode::Off, ProofMode::Sdr] {
        toggle(&mut s);
        assert_eq!(s.proof_mode(), expected);
        assert_eq!(s.engine.checkpoint(), checkpoint);
    }
    s.select_proof_mode(ProofMode::Print).unwrap();
    assert_eq!(s.proof_panel_mode(), ProofMode::Print);
    assert_eq!(s.proof_mode(), ProofMode::Off, "no profile means no rendered print proof");
    assert!(!s.hdr_presentation_allowed(), "a pending Print page shows SDR");
    toggle(&mut s);
    assert_eq!(s.proof_panel_mode(), ProofMode::Off, "Off cancels first-profile setup too");
    assert!(s.hdr_presentation_allowed());
    toggle(&mut s);
    assert_eq!(s.proof_panel_mode(), ProofMode::Print);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    s.set_proof_recipe(Some(ProofRecipe::new("Printer".into(), ColorProfile::default()))).unwrap();
    let saved = s.engine.document().clone();
    let checkpoint = s.engine.checkpoint();
    for expected in [ProofMode::Off, ProofMode::Print, ProofMode::Off] {
        toggle(&mut s);
        assert_eq!(s.proof_mode(), expected);
    }
    // Selection in the panel becomes the next menu toggle's remembered mode.
    s.select_proof_mode(ProofMode::Sdr).unwrap();
    s.select_proof_mode(ProofMode::Off).unwrap();
    toggle(&mut s);
    assert_eq!(s.proof_mode(), ProofMode::Sdr);
    assert_eq!(s.engine.document(), &saved);
    assert_eq!(s.engine.checkpoint(), checkpoint);

    let mut s = session(Platform::Gtk);
    toggle(&mut s);
    assert_eq!(s.proof_panel_mode(), ProofMode::Print, "SDR artwork opens Print setup");
    assert_eq!(s.proof_mode(), ProofMode::Off);
}

#[test]
fn float32_bundled_effect_ranges_preserve_history_and_embedded_programs() {
    use layer_core::{EffectInstance, EffectValue};
    use layer_core::color::SampleDepth;
    for (name, key, value) in [("exposure", "exposure", 30.), ("curves", "hdr_stops", 40.)] {
        // An older embedded program keeps its original range when promoted.
        let mut document = Document::new(layer_core::PortableId::random(), 32, 32, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
        document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth = SampleDepth::F32;
        let (id, edit) = effect_insertion(&document, EffectInstance::new(layer_core::bundled_effect_catalog().get(name).unwrap().program()), name);
        document.apply(edit).unwrap();
        let mut working = document.working.clone(); working.occurrence = Some(id); working.target = None;
        document.apply(layer_core::Edit::Working(working)).unwrap();
        let renderer = Recorder { color: document.composition().color, ..Default::default() };
        let mut s = UiSession::new(renderer, document, [32,32], Platform::Gtk).unwrap();
        let before = s.engine.document().clone();
        let set = |value| UiAction::Effect { action: EffectAction::Set { layer: occurrence_token(id), key: key.into(), value: EffectValue::Number(value) } };
        s.dispatch(set(value)).unwrap();
        assert_eq!(s.engine.document().scene().effect(id).unwrap().value(key), Some(&EffectValue::Number(value)));
        let edited = s.engine.document().clone();
        assert!(s.dispatch(set(200.)).is_err());
        assert_live_artwork_eq(s.engine.document(), &edited);
        invoke(&mut s, CommandId::Undo);
        assert_live_artwork_eq(s.engine.document(), &before);
        invoke(&mut s, CommandId::Redo);
        assert_live_artwork_eq(s.engine.document(), &edited);
        Document::from_artwork((*s.capture_artwork().unwrap().artwork).clone()).unwrap().validate(Default::default()).unwrap();
    }
}

#[test]
fn proof_reveal_preserves_placement_and_opens_a_collapsed_drawer_idempotently() {
    for platform in [Platform::Gtk,Platform::Web,Platform::Android,Platform::Windows] {
        let mut s = session(platform);
        s.dispatch(UiAction::Customize {action:CustomizationAction::SetPanelVisible {panel:Panel::Color,visible:true}}).unwrap();
        let before=s.engine.document().clone();
        crate::proof_panel::reveal(&mut s).unwrap();
        let layout=&s.state.workspace.layout;
        let group=layout.panel_group(Panel::Proof).unwrap();
        assert_eq!(layout.panel_group(Panel::Color),Some(group));
        assert_eq!(layout.active_panel(Panel::Proof),Some(Panel::Proof));
        crate::proof_panel::reveal(&mut s).unwrap();
        assert!(s.state.customization.expanded.is_none(),"Reopening must not toggle expanded controls");
        s.dispatch(UiAction::Customize {action:CustomizationAction::SetColumnCollapsed {group,collapsed:true}}).unwrap();
        let column=s.state.workspace.layout.collapsed_column_for_group(group).unwrap();
        s.dispatch(UiAction::Customize {action:CustomizationAction::SetColumnDrawers {column,drawers:true}}).unwrap();
        crate::proof_panel::reveal(&mut s).unwrap();
        assert_eq!(s.state.customization.column_drawers.len(),1);
        let layout=s.state.workspace.layout.clone();
        crate::proof_panel::reveal(&mut s).unwrap();
        assert_eq!(s.state.customization.column_drawers.len(),1);
        assert_eq!(s.state.workspace.layout,layout);
        assert_eq!(s.engine.document(),&before);
    }
}

#[test]
fn proof_dial_and_queued_numeric_edits_share_cancellation_and_one_step_history() {
    use crate::proof_panel::{apply, ProofAction};
    let mut document=Document::new(layer_core::PortableId::random(),32,32, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth=layer_core::color::SampleDepth::F32;
    let mut s=UiSession::new(Recorder{color:document.composition().color,..Default::default()},document,[32,32], Platform::Windows).unwrap();
    let original=s.engine.document().output().sdr;
    let checkpoint=s.engine.checkpoint();
    // No full-recipe snapshots: queued fields resolve against the current recipe.
    apply(&mut s,ProofAction::Number{key:"exposure".into(),value:-0.4,phase:None}).unwrap();
    apply(&mut s,ProofAction::Number{key:"highlight_color".into(),value:0.73,phase:None}).unwrap();
    let numbers=s.engine.document().output().sdr;
    assert!((numbers.exposure+0.4).abs()<1e-6 && (numbers.highlight_color-0.73).abs()<1e-6);
    s.dispatch(UiAction::Invoke{command:CommandId::Undo}).unwrap();
    assert!((s.engine.document().output().sdr.exposure+0.4).abs()<1e-6);
    s.dispatch(UiAction::Invoke{command:CommandId::Undo}).unwrap();assert_eq!(s.engine.checkpoint(),checkpoint);
    let g=crate::parameter_pad::ParameterDialGeometry::new(256.).unwrap();let origin=g.arcs[0].point(0.5);let point=g.arcs[0].point(0.25);
    let action=|phase,point|ProofAction::Dial{phase,size:256.,origin,point};
    apply(&mut s,action(ContactPhase::Down,origin)).unwrap();apply(&mut s,action(ContactPhase::Move,point)).unwrap();
    assert!(s.capture_artwork().is_err());
    apply(&mut s,action(ContactPhase::Cancel,point)).unwrap();assert_eq!(s.engine.checkpoint(),checkpoint);assert_eq!(s.engine.document().output().sdr,original);
    apply(&mut s,action(ContactPhase::Down,origin)).unwrap();apply(&mut s,action(ContactPhase::Move,point)).unwrap();apply(&mut s,action(ContactPhase::Up,point)).unwrap();
    let dial=s.engine.document().output().sdr;assert!((dial.exposure+1.).abs()<1e-6);assert_eq!(dial.headroom,original.headroom);
    s.dispatch(UiAction::Invoke{command:CommandId::Undo}).unwrap();assert_eq!(s.engine.checkpoint(),checkpoint);
    s.dispatch(UiAction::Invoke{command:CommandId::Redo}).unwrap();assert_eq!(s.engine.document().output().sdr,dial);
    s.dispatch(UiAction::Invoke{command:CommandId::SoftProof}).unwrap();assert_eq!(s.proof_panel_mode(),ProofMode::Sdr);assert!(s.state.requests.is_empty());
    s.dispatch(UiAction::Invoke{command:CommandId::SdrRendition}).unwrap();assert!(s.state.requests.is_empty());assert_eq!(s.engine.document().output().sdr,dial);
}

#[test]
fn hdr_curves_default_to_log_domain_with_reference_white_on_the_axis() {
    use layer_core::EffectValue;
    use layer_core::color::SampleDepth;
    let mut document = Document::new(layer_core::PortableId::random(), 32, 32, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth = SampleDepth::F16;
    let renderer = Recorder { color: document.composition().color, ..Default::default() };
    let mut s = UiSession::new(renderer, document, [32, 32], Platform::Gtk).unwrap();
    s.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "curves".into() } }).unwrap();
    let id = s.state.layer_properties.layer.unwrap();
    let effect = s.engine.document().scene().effect(occurrence_handle(id).unwrap()).unwrap();
    assert_eq!(effect.choice("domain"), Some("Log HDR"));
    assert_eq!(s.state.layer_properties.curve_max, Some(16.));
    assert_eq!(s.state.layer_properties.curve_white, Some(8. / 12.));
    let set = |key: &str, value| UiAction::Effect { action: EffectAction::Set { layer: id, key: key.into(), value } };
    assert!(s.dispatch(set("domain", EffectValue::Choice(2))).is_err());
    s.dispatch(set("domain", EffectValue::Choice(0))).unwrap();
    assert_eq!(s.state.layer_properties.curve_max, None);
    assert_eq!(s.state.layer_properties.curve_white, None);
}

#[test]
fn phased_proof_controls_commit_once() {
    use crate::proof_panel::{apply, ProofAction, SdrControlEdit};
    let mut document=Document::new(layer_core::PortableId::random(),32,32, layer_core::DocumentNames { paint: "Current ink".into(), paper: "Paper".into() });document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth=layer_core::color::SampleDepth::F32;
    let mut s=UiSession::new(Recorder{color:document.composition().color,..Default::default()},document,[32,32], Platform::Gtk).unwrap();
    let checkpoint=s.engine.checkpoint();
    for (phase,steps) in [(ContactPhase::Down,1.),(ContactPhase::Move,1.),(ContactPhase::Up,0.)] {
        apply(&mut s,ProofAction::Control{part:1,edit:SdrControlEdit::Step{axis:0,steps},phase:Some(phase)}).unwrap();
    }
    assert!((s.engine.document().output().sdr.exposure-0.08).abs()<1e-6);
    s.dispatch(UiAction::Invoke{command:CommandId::Undo}).unwrap();
    assert_eq!(s.engine.checkpoint(),checkpoint);
    apply(&mut s,ProofAction::Number{key:"exposure".into(),value:1.,phase:Some(ContactPhase::Down)}).unwrap();
    apply(&mut s,ProofAction::Number{key:"exposure".into(),value:1.,phase:Some(ContactPhase::Cancel)}).unwrap();
    assert_eq!(s.engine.checkpoint(),checkpoint);
    assert_eq!(s.engine.document().output().sdr.exposure,0.);
}

#[test]
fn bristle_streaks_carry_the_color_that_is_not_painting() {
    use layer_core::color::{RgbColor,RgbSpace};
    let mut s=session(Platform::Gtk);
    s.select_brush(DefaultBrushPreset::BristlePaintbrush as u32).unwrap();
    let olive=RgbColor::new(RgbSpace::Srgb,[0.5,0.5,0.1,1.]).unwrap();
    let red=RgbColor::new(RgbSpace::Srgb,[0.7,0.1,0.1,1.]).unwrap();
    for (slot,color) in [(ColorSlot::Foreground,olive),(ColorSlot::Background,red)] {
        s.dispatch(UiAction::Color {action:ColorAction::SetSlot {slot,color}}).unwrap();
    }
    let space=s.engine.document().composition().color.space;
    let paints=|s:&UiSession<Recorder>| {
        let brush=s.engine.configured_brush();
        (brush.color_rgba_linear,brush.contact.unwrap().bristles.unwrap().streak_rgba_linear)
    };
    s.dispatch(UiAction::Color {action:ColorAction::Select {slot:ColorSlot::Foreground}}).unwrap();
    assert_eq!(paints(&s),(olive.linear_in(space).unwrap(),red.linear_in(space).unwrap()));
    s.dispatch(UiAction::Color {action:ColorAction::Select {slot:ColorSlot::Background}}).unwrap();
    assert_eq!(paints(&s),(red.linear_in(space).unwrap(),olive.linear_in(space).unwrap()),"painting with the second color streaks with the first");
    s.dispatch(UiAction::Color {action:ColorAction::Swap}).unwrap();
    assert_eq!(paints(&s).1,red.linear_in(space).unwrap());
}

fn p28_gradient_session(tool:bool)->UiSession<Recorder>{
    let mut app=session(Platform::Gtk);
    if tool{invoke(&mut app,CommandId::Gradient);}else{app.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"gradient_map".into()}}).unwrap();}
    app.reveal_panel(Panel::Properties).unwrap();app.frame(1,1).unwrap();app
}
fn p28_destination(app:&UiSession<Recorder>,tool:bool)->crate::GradientDestination{
    if tool{crate::GradientDestination::Tool{epoch:app.state.document_file.epoch}}
    else{crate::GradientDestination::Effect{layer:app.state.layer_properties.layer.unwrap(),key:"gradient".into(),epoch:app.state.layer_properties.epoch}}
}
fn p28_control(app:&UiSession<Recorder>)->&crate::PropertyControl{
    app.state.tool_extra.iter().find_map(|option|match option {crate::ToolOption::Gradient(control)=>Some(control.as_ref()),_=>None})
        .unwrap_or_else(||app.state.layer_properties.controls.iter().find(|c|c.key=="gradient").unwrap())
}
fn p28_definition(app:&UiSession<Recorder>)->layer_core::GradientDefinition{
    let control=p28_control(app);
    let layer_core::EffectValue::Gradient(value)=&control.value else{panic!("gradient control")};value.clone()
}
fn p28_gradient_edit(app:&mut UiSession<Recorder>,target:crate::GradientDestination,edit:crate::GradientEdit){
    app.dispatch(UiAction::Effect{action:EffectAction::Gradient{target,edit}}).unwrap();
}
#[test]
fn shared_gradient_destinations_bound_stops_and_preserve_exact_close_neighbors(){
    use crate::GradientEdit as G;
    for tool in [false,true]{
        let mut app=p28_gradient_session(tool);
        for position in [0.5,f32::from_bits(0.5_f32.to_bits()+1)]{
            let target=p28_destination(&app,tool);p28_gradient_edit(&mut app,target,G::Stop{index:None,position,color:None,remove:false});
        }
        let close=p28_definition(&app);assert_eq!(close.stops.len(),4);assert_eq!(close.stops[2].position.to_bits(),0.5_f32.to_bits()+1);
        let target=p28_destination(&app,tool);p28_gradient_edit(&mut app,target,G::Stop{index:Some(1),position:0.6,color:None,remove:false});
        assert_eq!(p28_definition(&app).stops[1].position,0.5);
        for i in 1..40{let target=p28_destination(&app,tool);p28_gradient_edit(&mut app,target,G::Stop{index:None,position:i as f32/41.,color:None,remove:false});}
        assert_eq!(p28_definition(&app).stops.len(),32);
        let original=p28_definition(&app);
        let target=p28_destination(&app,tool);p28_gradient_edit(&mut app,target,G::Stop{index:Some(0),position:0.4,color:None,remove:true});assert_eq!(p28_definition(&app),original);
        while p28_definition(&app).stops.len()>2{let target=p28_destination(&app,tool);p28_gradient_edit(&mut app,target,G::Stop{index:Some(1),position:0.,color:None,remove:true});}
        let target=p28_destination(&app,tool);p28_gradient_edit(&mut app,target,G::Stop{index:Some(1),position:0.4,color:None,remove:true});assert_eq!(p28_definition(&app).stops.len(),2);
    }
}
#[test]
fn shared_gradient_gestures_cancel_and_commit_one_destination_appropriate_history(){
    use crate::GradientEdit as G;
    for tool in [false,true]{for cancel in [false,true]{
        let mut app=p28_gradient_session(tool);let original=p28_definition(&app);let checkpoint=app.engine.checkpoint();let target=p28_destination(&app,tool);
        for (phase,position) in [(ContactPhase::Down,0.25),(ContactPhase::Move,0.4),(if cancel{ContactPhase::Cancel}else{ContactPhase::Up},0.6)]{
            app.dispatch(UiAction::Effect{action:EffectAction::Gesture{phase,action:Box::new(EffectAction::Gradient{target:target.clone(),edit:G::Stop{index:if phase==ContactPhase::Down{None}else{Some(1)},position,color:None,remove:false}})}}).unwrap();
        }
        if cancel{assert_eq!(p28_definition(&app),original);assert_eq!(app.engine.checkpoint(),checkpoint);}
        else{assert_eq!(p28_definition(&app).stops.len(),3);assert_eq!(p28_definition(&app).stops[1].position,0.6);
            if tool{assert_eq!(app.engine.checkpoint(),checkpoint);}else{invoke(&mut app,CommandId::Undo);assert_eq!(p28_definition(&app),original);assert_eq!(app.engine.checkpoint(),checkpoint);invoke(&mut app,CommandId::Redo);assert_eq!(p28_definition(&app).stops[1].position,0.6);}}
    }}
}
#[test]
fn shared_gradient_tool_retains_settings_and_stale_destinations_do_not_edit(){
    use crate::{GradientEdit as G,GradientDestination as D};
    let mut app=p28_gradient_session(true);let checkpoint=app.engine.checkpoint();
    for edit in [G::Interpolation{value:layer_core::ColorMixSpace::LinearRgb},G::Reverse]{let target=p28_destination(&app,true);p28_gradient_edit(&mut app,target,edit);}
    let expected=p28_definition(&app);invoke(&mut app,CommandId::Hand);
    assert!(!app.state.tool_extra.iter().any(|option|matches!(option,crate::ToolOption::Gradient(_))),"leaving Gradient retires its tool option");
    invoke(&mut app,CommandId::Gradient);
    assert_eq!(p28_definition(&app),expected);
    let target=D::Tool{epoch:app.state.document_file.epoch.wrapping_sub(1)};p28_gradient_edit(&mut app,target,G::Reset);assert_eq!(p28_definition(&app),expected);assert_eq!(app.engine.checkpoint(),checkpoint);
    let mut app=p28_gradient_session(false);let original=p28_definition(&app);let checkpoint=app.engine.checkpoint();let target=D::Effect{layer:app.state.layer_properties.layer.unwrap(),key:"gradient".into(),epoch:app.state.layer_properties.epoch.wrapping_sub(1)};
    p28_gradient_edit(&mut app,target,G::Interpolation{value:layer_core::ColorMixSpace::Classic});assert_eq!(p28_definition(&app),original);assert_eq!(app.engine.checkpoint(),checkpoint);
}

#[test]
fn shared_gradient_stale_gesture_down_cannot_acquire_tool_or_effect_owner(){
    use crate::{GradientDestination as D,GradientEdit as G};
    for tool in [false,true]{
        let mut app=p28_gradient_session(tool);let original=p28_definition(&app);let checkpoint=app.engine.checkpoint();
        let target=if tool{D::Tool{epoch:app.state.document_file.epoch.wrapping_sub(1)}}else{D::Effect{layer:app.state.layer_properties.layer.unwrap(),key:"gradient".into(),epoch:app.state.layer_properties.epoch.wrapping_sub(1)}};
        app.dispatch(UiAction::Effect{action:EffectAction::Gesture{phase:ContactPhase::Down,action:Box::new(EffectAction::Gradient{target,edit:G::Stop{index:None,position:0.5,color:None,remove:false}})}}).unwrap();
        assert_eq!(p28_definition(&app),original);assert_eq!(app.engine.checkpoint(),checkpoint);
        assert!(app.effect_gesture.is_none(),"stale effect destination owns gesture");
        assert!(app.layer_interaction.gradient_before.is_none(),"stale tool destination owns gesture");
    }
}

#[test]
fn shared_gradient_tool_option_preserves_layer_properties_and_panel_placement(){
    let mut app=p28_gradient_session(false);
    let properties=serde_json::to_value(&app.state.layer_properties).unwrap();
    let layout=serde_json::to_value(&app.state.workspace.layout).unwrap();
    invoke(&mut app,CommandId::Gradient);
    assert_eq!(serde_json::to_value(&app.state.layer_properties).unwrap(),properties);
    assert_eq!(serde_json::to_value(&app.state.workspace.layout).unwrap(),layout);
    assert!(matches!(p28_control(&app).gradient.as_ref().unwrap().destination,crate::GradientDestination::Tool{..}));
    let editor=app.state.tool_extra.iter().position(|option|matches!(option,crate::ToolOption::Gradient(_))).unwrap();
    assert!(editor>0);
    let crate::ToolOption::Choice{id,segmented,items,..}=&app.state.tool_extra[editor-1] else{panic!("shape must precede editor")};
    assert_eq!(*id,"gradient-shape");assert!(*segmented);assert!(!items.is_empty());
    let serialized=serde_json::to_value(p28_control(&app).gradient.as_ref().unwrap()).unwrap();
    for removed in ["shape","shapes","dither_label"]{assert!(serialized.get(removed).is_none());}

    let target=p28_destination(&app,true);p28_gradient_edit(&mut app,target,crate::GradientEdit::Interpolation{value:layer_core::ColorMixSpace::Classic});
    assert_eq!(serde_json::to_value(&app.state.layer_properties).unwrap(),properties);
}

#[test]
fn shared_gradient_shapes_are_independent_of_workspace_tool_groups(){
    for platform in [Platform::Gtk,Platform::Web,Platform::Android] {
        let mut app=session(platform);
        for preset in [WorkspacePreset::Painter,WorkspacePreset::Illustrator,WorkspacePreset::Photographer] {
            app.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(WorkspaceState {
                layout:preset.layout(platform),..WorkspaceState::default()
            })}).unwrap();
            invoke(&mut app,CommandId::Gradient);
            let shapes=app.state.tool_extra.iter().find_map(|option|match option {
                crate::ToolOption::Choice {id:"gradient-shape",items,..}=>Some(items.clone()),_=>None
            }).unwrap();
            assert_eq!(shapes.len(),3,"{platform:?} {preset:?}");
            for (item,shape) in shapes.into_iter().zip(layer_core::GradientShape::ALL) {
                app.dispatch(UiAction::ToolbarEdit {context:app.state.toolbar_context(),action:Box::new(item.action)}).unwrap();
                assert_eq!(app.state.layer_tools.tool,LayerCanvasTool::Gradient {shape});
                let crate::ToolOption::Choice {items,..}=&app.state.tool_extra[0] else {panic!("shape first")};
                assert_eq!(items.iter().filter(|item|item.selected).count(),1);
            }
        }
    }
}

#[test]
fn shared_gradient_tool_edits_work_in_quick_and_saved_selection_masks() {
    use crate::GradientEdit as G;
    for saved in [false,true] {
        let mut app=p28_gradient_session(true);
        invoke(&mut app,CommandId::QuickMask);
        if saved {invoke(&mut app,CommandId::SaveSelectionLayer);}
        invoke(&mut app,CommandId::Gradient);
        assert!(app.selection_masks.target().is_some());
        let checkpoint=app.engine.checkpoint();let document=app.engine.document().clone();
        for edit in [G::Interpolation{value:layer_core::ColorMixSpace::LinearRgb},G::Stop{index:None,position:0.4,color:None,remove:false}] {
            let target=p28_destination(&app,true);p28_gradient_edit(&mut app,target,edit);
        }
        let expected=p28_definition(&app);assert_eq!(expected.interpolation,layer_core::ColorMixSpace::LinearRgb);assert_eq!(expected.stops.len(),3);
        let target=p28_destination(&app,true);
        for (phase,position) in [(ContactPhase::Down,0.5),(ContactPhase::Move,0.6),(ContactPhase::Cancel,0.6)] {
            app.dispatch(UiAction::Effect{action:EffectAction::Gesture{phase,action:Box::new(EffectAction::Gradient{target:target.clone(),edit:G::Stop{index:Some(1),position,color:None,remove:false}})}}).unwrap();
        }
        assert_eq!(p28_definition(&app),expected);assert!(app.layer_interaction.gradient_before.is_none());
        assert_eq!(app.engine.checkpoint(),checkpoint);assert_eq!(app.engine.document(),&document);
        let effect=crate::GradientDestination::Effect{layer:occurrence_token(document.working.occurrence.unwrap()),key:"gradient".into(),epoch:app.state.layer_properties.epoch};
        assert!(app.effect_action(EffectAction::Gradient{target:effect,edit:G::Reset}).is_err());
        assert_eq!(p28_definition(&app),expected);assert_eq!(app.engine.document(),&document);
    }
}

#[test]
fn shared_gradient_reverse_reorders_colors_and_keeps_uneven_adjacent_knots_valid(){
    use crate::GradientEdit as G;
    for tool in [false,true]{
        let mut app=p28_gradient_session(tool);
        for (i,position) in [0.2,0.5,f32::from_bits(0.5_f32.to_bits()+1)].into_iter().enumerate(){
            let color=layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb,[i as f32/3.,0.2,0.7,0.4]).unwrap();
            let target=p28_destination(&app,tool);p28_gradient_edit(&mut app,target,G::Stop{index:None,position,color:Some(color),remove:false});
        }
        let before=p28_definition(&app);let checkpoint=app.engine.checkpoint();
        let target=p28_destination(&app,tool);p28_gradient_edit(&mut app,target,G::Reverse);
        let reversed=p28_definition(&app);reversed.validate().unwrap();
        assert_eq!(reversed.stops.iter().map(|s|s.color).collect::<Vec<_>>(),before.stops.iter().rev().map(|s|s.color).collect::<Vec<_>>());
        assert_eq!(reversed.stops[3].position,0.8);
        assert!(reversed.stops[1].position<reversed.stops[2].position);
        assert_eq!(reversed.interpolation,before.interpolation);
        if tool{assert_eq!(app.engine.checkpoint(),checkpoint);}else{invoke(&mut app,CommandId::Undo);assert_eq!(p28_definition(&app),before);}
    }
}

#[test]
fn shared_gradient_bucket_uses_selected_paint_in_artwork_and_masks(){
    use crate::GradientEdit as G;
    for mask in [false,true]{
        let mut app=p28_gradient_session(true);
        if mask{invoke(&mut app,CommandId::QuickMask);invoke(&mut app,CommandId::Gradient);}
        let selected=layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb,[0.1,0.7,0.3,0.6]).unwrap();
        if mask{app.selection_masks.colors.background=selected;}else{app.state.colors.background=selected;}
        app.dispatch(UiAction::Color{action:ColorAction::Select{slot:ColorSlot::Background}}).unwrap();
        let checkpoint=app.engine.checkpoint();let target=p28_destination(&app,true);
        p28_gradient_edit(&mut app,target,G::UseCurrentColor{index:1});
        assert_eq!(p28_definition(&app).stops[1].color,selected,"mask={mask}");
        assert_eq!(app.engine.checkpoint(),checkpoint);
    }
}

#[test]
fn shared_gradient_toolbar_admits_only_its_published_tool_destination(){
    use crate::{GradientDestination as D,GradientEdit as G};
    let mut app=p28_gradient_session(false);
    let effect=p28_destination(&app,false);
    let stale_context=app.state.toolbar_context();
    invoke(&mut app,CommandId::Gradient);
    let context=app.state.toolbar_context();let target=p28_destination(&app,true);
    let original=p28_definition(&app);let checkpoint=app.engine.checkpoint();
    let dispatch=|app:&mut UiSession<Recorder>,context,target,mode|app.dispatch(UiAction::ToolbarEdit{context,action:Box::new(UiAction::Effect{action:EffectAction::Gradient{target,edit:G::Interpolation{value:mode}}})});
    assert!(dispatch(&mut app,stale_context,target.clone(),layer_core::ColorMixSpace::Classic).is_err());
    assert_eq!(p28_definition(&app),original);
    assert!(dispatch(&mut app,context.clone(),effect,layer_core::ColorMixSpace::Classic).is_err());
    assert_eq!(p28_definition(&app),original);
    let stale_epoch=app.state.document_file.epoch.wrapping_sub(1);
    assert!(dispatch(&mut app,context.clone(),D::Tool{epoch:stale_epoch},layer_core::ColorMixSpace::Classic).is_err());
    assert_eq!(p28_definition(&app),original);
    dispatch(&mut app,context,target,layer_core::ColorMixSpace::Classic).unwrap();
    assert_eq!(p28_definition(&app).interpolation,layer_core::ColorMixSpace::Classic);
    assert_eq!(app.engine.checkpoint(),checkpoint);
}

#[test]
fn shared_gradient_keyboard_release_preserves_precise_position_and_one_history_edit() {
    use crate::{GradientEdit as G, NumericOperation};
    for tool in [false, true] {
        for narrow in [false, true] {
            let mut app = p28_gradient_session(tool);
            let position = f32::from_bits(0.405_f32.to_bits() + 7);
            let neighbor = f32::from_bits(position.to_bits() + 2);
            for value in [position, if narrow { neighbor } else { 0.8 }] {
                let target = p28_destination(&app, tool);
                p28_gradient_edit(&mut app, target, G::Stop { index: None, position: value, color: None, remove: false });
            }
            let original = p28_definition(&app);
            let checkpoint = app.engine.checkpoint();
            let target = p28_destination(&app, tool);
            for phase in [ContactPhase::Down, ContactPhase::Move] {
                app.dispatch(UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(
                    EffectAction::Gradient { target: target.clone(), edit: G::Position { index: 1, operation: NumericOperation::Step { steps: 1. } } }
                ) } }).unwrap();
            }
            let before_release = p28_definition(&app);
            assert_ne!(before_release, original);
            if narrow { assert_eq!(before_release.stops[1].position.to_bits(), neighbor.to_bits() - 1); }
            app.dispatch(UiAction::Effect { action: EffectAction::Gesture { phase: ContactPhase::Up, action: Box::new(
                EffectAction::Gradient { target: target.clone(), edit: G::Position { index: 1, operation: NumericOperation::Step { steps: 0. } } }
            ) } }).unwrap();
            assert_eq!(p28_definition(&app), before_release, "release must preserve the full f32 knot position");
            if tool { assert_eq!(app.engine.checkpoint(), checkpoint); }
            else {
                invoke(&mut app, CommandId::Undo);
                assert_eq!(p28_definition(&app), original);
                assert_eq!(app.engine.checkpoint(), checkpoint);
                invoke(&mut app, CommandId::Redo);
                assert_eq!(p28_definition(&app), before_release);
            }
        }
    }
}
