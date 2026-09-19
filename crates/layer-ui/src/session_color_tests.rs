// Included in session::tests, using the protocol recorder (no simulated pixels).
#[test]
fn print_panel_first_use_has_no_target_and_off_rejects_late_publication() {
    use crate::proof_workflow::{proof_form,ProofPreparation};
    use layer_core::color::{ColorProfile,ProofRecipe,RgbSpace};
    let mut s=session();
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
    assert!(s.engine.document().proof.is_none());
}

#[test]
fn portable_proof_workflow_preserves_original_before_history_and_rejects_stale_jobs() {
    use crate::proof_workflow::{ProofPreparation, ProofView};
    use layer_core::color::{ColorProfile, ProofRecipe, RgbSpace};
    for platform in [Platform::Web, Platform::Android, Platform::Mac, Platform::Ios, Platform::Windows] {
        let mut s = session(); s.set_platform(platform);
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
        assert_eq!(s.engine.document().proof, Some(original.clone()));
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
        assert_eq!(s.engine.document().proof, Some(replacement));
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
        assert_eq!(s.engine.document().proof, Some(original));
        assert!(!s.state.document_file.modified);
        assert!(prepare.validate(&s).is_err());
        // First use is enabled on these ports and leaves toggles off until Apply.
        s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
        assert!(s.engine.document().proof.is_none());
        s.dispatch(UiAction::Invoke { command: CommandId::SoftProof }).unwrap();
        assert!(!s.state.soft_proof);
        assert!(matches!(s.state.requests.last().unwrap().kind, HostRequestKind::SoftProofSetup));
    }
}

#[test]
fn proof_colors_first_use_opens_setup_without_enabling_or_editing() {
    let mut s = session();
    s.set_platform(Platform::Gtk);
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
    let mut s = session();
    s.set_platform(Platform::Gtk);
    let original = s.engine.document().clone();
    assert!(s.command(CommandId::SoftProof).enabled);
    let recipe = ProofRecipe::new("Lab paper".into(), ColorProfile::default());
    s.set_proof_recipe(Some(recipe.clone())).unwrap();
    assert!(s.state.soft_proof);
    assert!(s.state.document_file.modified);
    assert_eq!(s.engine.document().layers, original.layers);
    s.files.saved_checkpoint = s.engine.checkpoint();
    s.refresh_file_state();
    let saved = s.engine.document().clone();
    for command in [CommandId::SoftProof, CommandId::GamutWarning, CommandId::SoftProof, CommandId::GamutWarning] {
        s.dispatch(UiAction::Invoke { command }).unwrap();
        assert_eq!(s.engine.document(), &saved);
        assert!(!s.state.document_file.modified);
    }
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    assert!(s.engine.document().proof.is_none());
    assert!(!s.state.soft_proof && !s.state.gamut_warning);
    assert_eq!(s.engine.document().layers, original.layers);
    s.dispatch(UiAction::Invoke { command: CommandId::Redo }).unwrap();
    assert_eq!(s.engine.document().proof, Some(recipe.clone()));
    assert!(!s.state.document_file.modified);
    assert!(!s.state.soft_proof, "restoring a recipe does not enable a temporary view");
    s.set_platform(Platform::Windows);
    assert!(s.command(CommandId::SoftProofSetup).enabled);
    assert!(s.set_proof_recipe(Some(recipe)).is_ok());
}

#[test]
fn color_transitions_update_picker_coordinates_and_route_exact_history_through_the_host() {
    use layer_core::{ColorTransition, color::{DocumentColor, SampleDepth, RgbColor, RgbSpace}};
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios] {
    let mut s = session();
    s.set_platform(platform);
    let definition = RgbColor::new(RgbSpace::DisplayP3, [0.8, 0.3, 0.1, 1.]).unwrap();
    s.dispatch(UiAction::Color { action: ColorAction::Definition { color: definition } }).unwrap();
    s.frame(1, 1).unwrap();
    let before = s.engine.document().clone();
    let color = DocumentColor { space: RgbSpace::ProPhoto, depth: SampleDepth::U16 };
    let prepare = |s: &UiSession<Recorder>| s.prepare_document_color_transition(ColorTransition::Apply {
        color, layers: s.engine.document().layers.clone(),
    }).unwrap().0;
    let prepared = prepare(&s);
    assert!(s.commit_document_color_transition(prepared).unwrap_err().contains("not ready"));
    assert_eq!(s.engine.document(), &before);
    assert_eq!(s.state.colors.rgb_space(), before.color.space);
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
        assert_eq!(project.document.color, expected.color);
        s.renderer_mut().prepared_color = Some(expected.color);
        s.commit_document_color_transition(prepared).unwrap();
        s.complete_document_request(id, Ok(true)).unwrap();
        assert_eq!(s.engine.document().layers, expected.layers);
        assert_eq!(s.engine.document().color, expected.color);
        assert_eq!(s.state.colors.rgb_space(), expected.color.space);
        assert_eq!(s.state.colors.definition(), definition);
        assert!(!s.state.document_file.busy);
    }
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
        let mut document = Document::new("wide", 1000, 1000);
        document.color = color;
        UiSession::new(
            Recorder {
                color,
                ..Default::default()
            },
            document,
            [1000; 2],
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
        let candidate = Box::new(make(space));
        let epoch = s.state.document_file.epoch;
        let revision = s.engine.document().revision;
        s.adopt_project(candidate, epoch, revision, None)
            .map_err(|(e, _)| e)
            .unwrap();
        assert_eq!(s.state.colors.definition(), definition);
        assert_eq!(s.state.colors.rgb_space(), space);
        assert_eq!(
            s.engine.configured_brush().color_rgba_linear,
            definition.linear_in(space).unwrap()
        );
        assert_eq!(s.state.brush.opacity, 0.23);
        let prepared = PreparedWorkspace::new(serde_json::from_slice(&encoded).unwrap()).unwrap();
        s.adopt_workspace(prepared).unwrap();
        assert_eq!(s.state.colors.rgb_space(), space);
        assert_eq!(s.state.colors.definition(), definition);
        assert_eq!(
            s.engine.configured_brush().color_rgba_linear,
            definition.linear_in(space).unwrap()
        );
        for preset in [
            DefaultBrushPreset::GPen,
            DefaultBrushPreset::WetRound,
            DefaultBrushPreset::GPen,
        ] {
            s.select_brush(preset as u32).unwrap();
            assert_eq!(
                s.engine.configured_brush().color_rgba_linear,
                definition.linear_in(space).unwrap()
            );
            let secondary = default_brush(preset)
                .color_dynamics
                .secondary_color_rgba_linear;
            let expected = RgbColor::from_linear(RgbSpace::Srgb, secondary)
                .unwrap()
                .linear_in(space)
                .unwrap();
            for (a, b) in s
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
        let opacity = s.state.brush.opacity;
        s.eyedropper.queue(ColorSampleSource::Composite, [32, 32]);
        s.frame(1, 1).unwrap();
        let request = *s.renderer_mut().sample_requests.last().unwrap();
        s.renderer_mut().sample_reply = Some(ColorSample {
            request_id: request.request_id,
            rgba: [-0.1, 1.2, 0.4, 0.00001],
        });
        s.frame(2, 2).unwrap();
        assert_eq!(s.state.colors.definition().space, space);
        for (a, b) in s
            .engine
            .configured_brush()
            .color_rgba_linear
            .into_iter()
            .zip([-0.1, 1.2, 0.4, 1.])
        {
            assert!((a - b).abs() < 2e-7, "{space:?}: {a} != {b}");
        }
        assert_eq!(s.state.brush.opacity, opacity);
        assert_eq!(s.engine.document().revision, revision);
        s.dispatch(UiAction::Color {
            action: ColorAction::Definition { color: definition },
        })
        .unwrap();
    }
}

#[test]
fn figures_and_gradients_convert_both_portable_paints() {
    use layer_core::color::{DocumentColor, SampleDepth, RgbColor, RgbSpace};
    let color = DocumentColor {
        space: RgbSpace::ProPhoto,
        depth: SampleDepth::U16,
    };
    let mut document = Document::new("wide", 1000, 1000);
    document.color = color;
    let mut s = UiSession::new(
        Recorder {
            color,
            ..Default::default()
        },
        document,
        [1000; 2],
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
    s.dispatch(UiAction::Layer {
        action: LayerAction::Tool {
            tool: LayerCanvasTool::Gradient {
                radial: false,
                transparent: false,
            },
        },
    })
    .unwrap();
    let mut down = event(&s, 1, PenPhase::Down, 1.);
    down.surface_position = Point { x: 50., y: 50. };
    s.pen(down).unwrap();
    let mut up = event(&s, 2, PenPhase::Up, 1.);
    up.surface_position = Point { x: 250., y: 250. };
    s.pen(up).unwrap();
    s.frame(10, 10).unwrap();
    let operations = &s.renderer_mut().pending_operations;
    let actual = operations
        .iter()
        .find_map(|(_, operation)| {
            if let layer_core::LayerOperationKind::Gradient { colors, .. } = &operation.kind {
                Some(*colors)
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
    for platform in [Platform::Gtk, Platform::Web, Platform::Android, Platform::Mac, Platform::Ios] {
        let mut s = session();
        s.set_platform(platform);
        s.frame(1, 1).unwrap();
        invoke(&mut s, CommandId::AssignProfile);
        let id = s.state.requests.first().unwrap().id;
        let mut workflow = ColorWorkflow::begin(&s, id).unwrap();
        assert!(workflow.select(Some(C::Depth { depth: SampleDepth::U16, dither: layer_core::color::OutputDither::None }), false).is_err());
        assert!(workflow.select(Some(C::Assign(RgbSpace::DisplayP3)), true).is_err());
        let ColorPreparation::Edit(change) = workflow.select(Some(C::Assign(RgbSpace::DisplayP3)), false).unwrap() else { panic!() };
        workflow.candidate = Some(layer_color::prepare_document_color(&workflow.original, change, 1024 * 1024, || false).unwrap().project);
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
        s.renderer_mut().prepared_color = Some(workflow.candidate.as_ref().unwrap().document.color);
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
            s.renderer_mut().prepared_color = Some(expected.color);
            let prepared = history.prepare_commit(&s, false, true).unwrap();
            assert!(history.prepare_commit(&s, false, true).is_err(), "a consumed Undo/Redo candidate cannot become a new edit");
            s.commit_document_color_candidate(prepared, |_| {}).unwrap();
            s.complete_document_request(id, Ok(true)).unwrap();
            assert_eq!(s.engine.document().layers, expected.layers);
            assert_eq!(s.engine.document().color, expected.color);
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
}

#[test]
fn hdr_appearance_draft_is_transient_and_preview_follows_display_capability() {
    use layer_core::color::{SampleDepth, hdr::SdrRendition};
    let mut document = Document::new("HDR", 32, 32);
    document.color.depth = SampleDepth::F16;
    let renderer = Recorder { color: document.color, ..Default::default() };
    let mut s = UiSession::new(renderer, document, [32, 32]).unwrap();
    s.set_platform(Platform::Gtk);
    let original = s.engine.document().clone();
    let checkpoint = s.engine.checkpoint();
    assert!(!s.command(CommandId::PreviewSdr).enabled);
    s.set_hdr_display_available(true);
    assert!(s.command(CommandId::PreviewSdr).enabled);
    let recipe = SdrRendition { exposure: -2., ..Default::default() };
    s.preview_sdr_appearance(Some(recipe)).unwrap();
    assert_eq!(s.effective_sdr_rendition(), recipe);
    assert_eq!(s.engine.document(), &original);
    assert_eq!(s.capture_project_recovery().unwrap().document.sdr_rendition, original.sdr_rendition);
    assert_eq!(s.engine.checkpoint(), checkpoint);
    assert!(!s.command(CommandId::PreviewSdr).enabled);
    s.preview_sdr_appearance(None).unwrap();
    assert_eq!(s.effective_sdr_rendition(), original.sdr_rendition);
    assert!(s.command(CommandId::PreviewSdr).enabled);
    s.set_sdr_rendition(recipe).unwrap();
    s.dispatch(UiAction::Invoke { command: CommandId::Undo }).unwrap();
    assert_eq!(s.engine.document().sdr_rendition, original.sdr_rendition);
    s.dispatch(UiAction::Invoke { command: CommandId::Redo }).unwrap();
    assert_eq!(s.engine.document().sdr_rendition, recipe);
    s.state.soft_proof = true; s.refresh_commands();
    assert!(!s.command(CommandId::PreviewSdr).enabled);
    s.state.soft_proof = false; s.set_hdr_display_available(false);
    assert!(!s.command(CommandId::PreviewSdr).enabled);
}

#[test]
fn proof_panel_preview_can_compare_master_and_saved_without_touching_history() {
    use layer_core::color::{SampleDepth, hdr::SdrRendition};
    let mut document=Document::new("HDR",32,32);document.color.depth=SampleDepth::F16;
    let renderer=Recorder {color:document.color,..Default::default()};
    let mut s=UiSession::new(renderer,document,[32,32]).unwrap();s.set_platform(Platform::Gtk);
    let original=s.engine.document().clone();let checkpoint=s.engine.checkpoint();
    let draft=SdrRendition { exposure:-1., contrast:1.2, headroom:3., ..Default::default() };
    s.state.soft_proof=true;s.state.gamut_warning=true;
    s.set_sdr_view(Some(draft),true).unwrap();
    assert!(s.state.preview_sdr);assert!(!s.state.soft_proof);assert!(!s.state.gamut_warning);
    assert_eq!(s.effective_sdr_rendition(),draft);
    s.set_sdr_view(Some(draft),false).unwrap();
    assert!(!s.state.preview_sdr);assert_eq!(s.state.sdr_appearance_preview,None);
    s.set_sdr_view(None,true).unwrap();
    assert_eq!(s.effective_sdr_rendition(),original.sdr_rendition);
    assert_eq!(s.engine.document(),&original);assert_eq!(s.engine.checkpoint(),checkpoint);
    assert_eq!(s.capture_project_recovery().unwrap().document.sdr_rendition,original.sdr_rendition);
    assert!(s.set_sdr_view(Some(SdrRendition {exposure:f32::NAN,..draft}),true).is_err());
    assert_eq!(s.state.sdr_appearance_preview,None);
}

#[test]
fn live_sdr_panel_gesture_commits_once_and_cancels_without_losing_redo() {
    use layer_core::color::{SampleDepth,hdr::SdrRendition};
    let mut document=Document::new("HDR",32,32);document.color.depth=SampleDepth::F16;
    let renderer=Recorder{color:document.color,..Default::default()};
    let mut s=UiSession::new(renderer,document,[32,32]).unwrap();s.set_platform(Platform::Gtk);
    let original=s.engine.document().sdr_rendition;
    let changed=SdrRendition{exposure:1.5,highlight_color:0.65,..original};
    s.set_proof_mode(ProofMode::Sdr).unwrap();
    let checkpoint=s.engine.checkpoint();
    s.edit_sdr_rendition(ContactPhase::Down,original).unwrap();
    for exposure in [0.2,0.8,1.5]{s.edit_sdr_rendition(ContactPhase::Move,SdrRendition{exposure,..changed}).unwrap();}
    assert_eq!(s.engine.document().sdr_rendition,changed);
    assert_eq!(s.engine.checkpoint(),checkpoint);
    assert!(s.capture_project_recovery().is_err(),"Recovery must not capture an unfinished contact");
    assert!(!s.command(CommandId::ExportDocument).enabled);
    s.edit_sdr_rendition(ContactPhase::Up,changed).unwrap();
    assert!(s.state.document_file.modified);
    assert_eq!(s.capture_project_recovery().unwrap().document.sdr_rendition,changed);
    s.dispatch(UiAction::Invoke{command:CommandId::Undo}).unwrap();
    assert_eq!(s.engine.document().sdr_rendition,original);
    s.edit_sdr_rendition(ContactPhase::Down,original).unwrap();
    s.edit_sdr_rendition(ContactPhase::Move,changed).unwrap();
    s.edit_sdr_rendition(ContactPhase::Cancel,changed).unwrap();
    assert_eq!(s.engine.document().sdr_rendition,original);
    assert_eq!(s.engine.checkpoint(),checkpoint);
    s.dispatch(UiAction::Invoke{command:CommandId::Redo}).unwrap();
    assert_eq!(s.engine.document().sdr_rendition,changed);
    let checkpoint=s.engine.checkpoint();
    s.set_proof_mode(ProofMode::Off).unwrap();
    assert_eq!(s.engine.checkpoint(),checkpoint);
    assert_eq!(s.engine.document().sdr_rendition,changed);
    assert!(!s.state.preview_sdr && !s.state.soft_proof);
}

#[test]
fn proof_modes_share_view_state_and_preserve_both_saved_recipes() {
    use layer_core::color::{SampleDepth,ProofRecipe,ColorProfile,RgbSpace};
    let mut document=Document::new("HDR",32,32);document.color.depth=SampleDepth::F16;
    let renderer=Recorder{color:document.color,..Default::default()};
    let mut s=UiSession::new(renderer,document,[32,32]).unwrap();s.set_platform(Platform::Gtk);
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
    let mut document = Document::new("HDR", 32, 32);
    document.color.depth = SampleDepth::F16;
    let renderer = Recorder { color: document.color, ..Default::default() };
    let mut s = UiSession::new(renderer, document, [32, 32]).unwrap();
    s.set_platform(Platform::Gtk);
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
    toggle(&mut s);
    assert_eq!(s.proof_panel_mode(), ProofMode::Off, "Off cancels first-profile setup too");
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

    let mut s = session();
    s.set_platform(Platform::Gtk);
    toggle(&mut s);
    assert_eq!(s.proof_panel_mode(), ProofMode::Print, "SDR artwork opens Print setup");
    assert_eq!(s.proof_mode(), ProofMode::Off);
}

#[test]
fn float32_bundled_effect_ranges_preserve_history_and_embedded_programs() {
    use layer_core::{EffectInstance, EffectValue, Layer, LayerKind};
    use std::sync::Arc;
    use layer_core::color::SampleDepth;
    for (name, key, value) in [("exposure", "exposure", 30.), ("curves", "hdr_stops", 40.)] {
        // An older embedded program keeps its original range when promoted.
        let mut document = Document::new("Float32", 32, 32);
        document.color.depth = SampleDepth::F32;
        let id = document.allocate_layer_id();
        let mut layer = Layer::paint(id, name);
        layer.kind = LayerKind::Effect;
        layer.effect = Some(Arc::new(EffectInstance::new(layer_core::bundled_effect_catalog().get(name).unwrap().program())));
        document.layers.insert(0, layer);
        document.active_layer = id;
        let renderer = Recorder { color: document.color, ..Default::default() };
        let mut s = UiSession::new(renderer, document, [32,32]).unwrap();
        s.set_platform(Platform::Gtk);
        let before = s.engine.document().clone();
        let set = |value| UiAction::Effect { action: EffectAction::Set { layer: id.0, key: key.into(), value: EffectValue::Number(value) } };
        s.dispatch(set(value)).unwrap();
        assert_eq!(s.engine.document().layer(id).unwrap().effect.as_ref().unwrap().value(key), Some(&EffectValue::Number(value)));
        let edited = s.engine.document().clone();
        assert!(s.dispatch(set(200.)).is_err());
        assert_eq!(s.engine.document().layers, edited.layers);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().layers, before.layers);
        invoke(&mut s, CommandId::Redo);
        assert_eq!(s.engine.document().layers, edited.layers);
        s.capture_project_recovery().unwrap().validate(Default::default()).unwrap();
    }
}


#[test]
fn proof_reveal_preserves_placement_and_opens_a_collapsed_drawer_idempotently() {
    for platform in [Platform::Gtk,Platform::Web,Platform::Android] {
        let mut s=session();s.set_platform(platform);
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
