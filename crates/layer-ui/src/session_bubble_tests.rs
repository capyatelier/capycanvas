use super::{test_support::*, *};
use layer_core::DefaultBrushPreset;

const VIEW: [f32; 2] = [1200., 900.];
const FAR: [f32; 2] = [640., 520.];

fn desk() -> UiSession<Recorder> {
    let mut s = session(Platform::Gtk);
    s.set_viewport(VIEW, [1200, 900]).unwrap();
    s
}

fn act(s: &mut UiSession<Recorder>, action: BubbleAction) -> Result<UiChange, String> {
    s.dispatch(UiAction::Bubble { action })
}

fn contact(s: &mut UiSession<Recorder>, grab: BubbleGrab, phase: ContactPhase, position: [f32; 2]) -> Result<UiChange, String> {
    act(s, BubbleAction::Drag { grab, phase, position, viewport: VIEW })
}

fn drag(s: &mut UiSession<Recorder>, grab: BubbleGrab, from: [f32; 2], to: [f32; 2]) {
    contact(s, grab, ContactPhase::Down, from).unwrap();
    contact(s, grab, ContactPhase::Move, to).unwrap();
    contact(s, grab, ContactPhase::Up, to).unwrap();
}

fn take(s: &mut UiSession<Recorder>, grab: BubbleGrab, at: [f32; 2]) -> u32 {
    drag(s, grab, at, at);
    s.state.workspace.layout.bubbles.bubbles.last().unwrap().id
}

fn brush(id: DefaultBrushPreset) -> BubbleGrab {
    BubbleGrab::Variant { variant: ToolVariant::BrushPreset { id: id as u32 } }
}

#[test]
fn pulling_a_bubble_closes_the_tool_drawer_on_every_platform() {
    for platform in Platform::ALL {
        for source in 0..3 {
            for finish in [ContactPhase::Up, ContactPhase::Cancel] {
                let mut s = session(platform);
                s.set_viewport(VIEW, [1200, 900]).unwrap();
                s.state.workspace.layout = WorkspacePreset::Painter.layout(platform);
                let control = if source == 2 { ToolbarControl::Command { command: CommandId::DrawingBrush } } else { ToolbarControl::Color };
                let id = s.state.workspace.layout.header.entries().find(|e| e.item == HeaderItem::Tool { control }).unwrap().id;
                s.dispatch(UiAction::MeasureHeader { height: 60., items: vec![HeaderItemBounds {
                    id, bounds: Bounds { x: 800., y: 0., width: 40., height: 60. },
                }] }).unwrap();
                s.dispatch(UiAction::ActivateHeaderItem { id }).unwrap();
                if s.state.customization.drawer.is_none() { s.dispatch(UiAction::ActivateHeaderItem { id }).unwrap(); }
                assert!(s.state.customization.drawer.is_some());
                let grab = match source {
                    0 => BubbleGrab::Header { id },
                    1 => { let (panel, tile) = toolbar_tile(&mut s, control); BubbleGrab::Tile { panel, tile } },
                    _ => brush(DefaultBrushPreset::PointyPencil),
                };
                let change = contact(&mut s, grab, ContactPhase::Down, FAR).unwrap();
                assert!(s.state.customization.drawer.is_none(), "{platform:?}/{grab:?}");
                assert_ne!(change.regions & regions::CUSTOMIZATION, 0, "hosts receive the closed drawer");
                contact(&mut s, grab, ContactPhase::Move, [700., 550.]).unwrap();
                contact(&mut s, grab, finish, [700., 550.]).unwrap();
                assert!(s.state.customization.drawer.is_none());
                assert_eq!(s.state.workspace.layout.bubbles.bubbles.len(), usize::from(finish == ContactPhase::Up));
                if finish == ContactPhase::Up {
                    let bubble = s.state.workspace.layout.bubbles.bubbles[0].id;
                    s.state.customization.drawer = Some(ContentDrawer::for_header(&s.state.workspace.layout, id).unwrap());
                    contact(&mut s, BubbleGrab::Bubble { id: bubble }, ContactPhase::Down, FAR).unwrap();
                    assert!(s.state.customization.drawer.is_some(), "moving an existing bubble keeps unrelated drawers");
                    contact(&mut s, BubbleGrab::Bubble { id: bubble }, ContactPhase::Cancel, FAR).unwrap();
                }
            }
        }
    }
}

#[test]
fn pulling_a_toolbar_bubble_closes_its_column_drawer() {
    let mut s = desk();
    s.state.workspace.layout.column_stack_mut(4).drawers = true;
    s.dispatch(UiAction::MovePanel { panel: Panel::Toolbar, target: DockTarget::Tab { group: 5, index: None }, viewport: VIEW }).unwrap();
    s.dispatch(UiAction::DoubleClickPanelHandle { group: 5, viewport: VIEW }).unwrap();
    s.dispatch(UiAction::Customize { action: CustomizationAction::ToggleColumnDrawer { group: 5, panel: Panel::Toolbar } }).unwrap();
    assert_eq!(s.state.customization.column_drawers.len(), 1);
    let tile = s.state.workspace.layout.panel(Panel::Toolbar).unwrap().tiles()[0].id;
    let grab = BubbleGrab::Tile { panel: Panel::Toolbar, tile };
    let change = contact(&mut s, grab, ContactPhase::Down, FAR).unwrap();
    assert!(s.state.customization.column_drawers.is_empty());
    assert_ne!(change.regions & regions::CUSTOMIZATION, 0);
    contact(&mut s, grab, ContactPhase::Up, FAR).unwrap();
    assert_eq!(s.state.workspace.layout.bubbles.bubbles.len(), 1);
}

#[test]
fn bubble_pickup_queries_use_shared_sources_and_drop_clearance() {
    let mut s = desk();
    for (control, plain, held) in [
        (ToolbarControl::Color, true, true),
        (ToolbarControl::ColorPicker, true, true),
        (ToolbarControl::BrushSizeSlider, false, false),
        (ToolbarControl::Command { command: CommandId::Undo }, true, true),
    ] {
        let (panel, tile) = toolbar_tile(&mut s, control);
        let grab = BubbleGrab::Tile { panel, tile };
        assert_eq!(s.bubble_grab(grab, false).is_some(), plain);
        assert_eq!(s.bubble_grab(grab, true).is_some(), held);
        let bounds = s.layout(VIEW).groups.iter().find(|g| g.panels.contains(&panel)).unwrap().bounds;
        assert!(!s.can_take_bubble(grab, [bounds.x + 2., bounds.y + 2.], VIEW));
        assert_eq!(s.can_take_bubble(grab, FAR, VIEW), held);
        assert!(!s.can_take_bubble(grab, [f32::NAN, FAR[1]], VIEW));
        assert!(!s.can_take_bubble(grab, FAR, [0., 900.]));
    }
    assert!(s.bubble_grab(BubbleGrab::Bubble { id: u32::MAX }, false).is_none());
}

#[test]
fn bubble_menu_publication_marks_headings_and_persistent_options() {
    let mut s = desk();
    let id = take_ring(&mut s, FAR);
    let menu = s.context_menu(ContextTarget::Bubble { id }).unwrap();
    let json = serde_json::to_value(&menu).unwrap();
    assert_eq!(json["sections"][0][0]["is_heading"], true);
    assert_eq!(json["sections"][0][0]["keep_open"], false);
    for (section, serialized) in menu.sections.iter().zip(json["sections"].as_array().unwrap()) {
        for (item, serialized) in section.iter().zip(serialized.as_array().unwrap()) {
            assert_eq!(serialized["keep_open"], matches!(item.action, Some(UiAction::Bubble { action: BubbleAction::Option { .. } })));
        }
    }
}

fn toolbar_tile(s: &mut UiSession<Recorder>, control: ToolbarControl) -> (Panel, u32) {
    let panel = s.layout(VIEW).groups.iter().flat_map(|g| g.panels.clone()).find(|p| p.kind() == PanelKind::Tiles).unwrap();
    let layout = &mut s.state.workspace.layout;
    layout.insert_tools(panel, None, &[control]).unwrap();
    (panel, *tile_ids(layout, panel).last().unwrap())
}

fn take_control(s: &mut UiSession<Recorder>, control: ToolbarControl, at: [f32; 2]) -> u32 {
    let (panel, tile) = toolbar_tile(s, control);
    take(s, BubbleGrab::Tile { panel, tile }, at)
}

fn take_source(s: &mut UiSession<Recorder>, source: BubbleSource) -> u32 {
    match source {
        BubbleSource::Medium { group } => take(s, BubbleGrab::Variant { variant: ToolVariant::BrushGroup { group } }, FAR),
        BubbleSource::Control { control } => take_control(s, control, FAR),
    }
}

fn show_sliders(s: &UiSession<Recorder>, id: u32) -> ContextMenuItem {
    s.context_menu(ContextTarget::Bubble { id }).unwrap().sections.into_iter().flatten()
        .find(|item| matches!(item.action, Some(UiAction::Bubble { action: BubbleAction::ToggleSliders { .. } }))).unwrap()
}

fn take_ring(s: &mut UiSession<Recorder>, at: [f32; 2]) -> u32 {
    let id = take_control(s, ToolbarControl::Color, at);
    let options = BubbleOptions { recent: true, palette: true, pair: true, ..view(s, id).options };
    act(s, BubbleAction::SetOptions { id, options }).unwrap();
    id
}

fn wheel_contact(s: &mut UiSession<Recorder>, id: u32, phase: ContactPhase, part: ColorWheelPart, position: [f32; 2]) -> Result<UiChange, String> {
    act(s, BubbleAction::Wheel { id, phase, part, position })
}

fn hdr_desk(depth: layer_core::color::SampleDepth, platform: Platform) -> UiSession<Recorder> {
    let mut document = layer_core::Document::new(layer_core::PortableId::random(), 32, 32,
        layer_core::DocumentNames { paint: "Paint".into(), paper: "Paper".into() });
    document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth = depth;
    let mut s = UiSession::new(Recorder { color: document.composition().color, ..Default::default() }, document, [32, 32], platform).unwrap();
    s.set_viewport(VIEW, [1200, 900]).unwrap();
    s
}

#[test]
fn hdr_bubble_layout_only_appears_on_hosts_with_the_intensity_control() {
    for (depth, platform) in [layer_core::color::SampleDepth::F16, layer_core::color::SampleDepth::F32].into_iter()
        .flat_map(|depth| Platform::ALL.into_iter().map(move |platform| (depth, platform))) {
        let mut s = hdr_desk(depth, platform);
        let id = take_ring(&mut s, FAR);
        let layout = view(&s, id).color.unwrap();
        let enabled = matches!(platform, Platform::Gtk | Platform::Web | Platform::Android | Platform::Windows | Platform::Mac | Platform::Ios);
        assert_eq!(layout.intensity.is_some(), enabled, "{platform:?}");
        assert_eq!(act(&mut s, BubbleAction::SetIntensity { id, stops: 1. }).is_ok(), enabled, "{platform:?}");
        if !enabled { assert_eq!(layout.bounds().height, layout.diameter); }
    }
}

#[test]
fn hdr_bubble_eyedropper_preview_overrides_only_preview_presentations() {
    use layer_core::color::{RgbColor, RgbSpace, SampleDepth};
    for depth in [SampleDepth::F16, SampleDepth::F32] {
        let mut s = hdr_desk(depth, Platform::Windows);
        let id = take_ring(&mut s, FAR);
        let committed = s.state.colors.clone();
        let checkpoint = s.engine.checkpoint();
        let before = serde_json::to_value(s.bubble_presentations()).unwrap();
        let mut sampled = committed.clone();
        sampled.apply(ColorAction::Definition { color: RgbColor::new(RgbSpace::Srgb, [0.2, 1., 0.4, 0.37]).unwrap() }).unwrap();
        sampled.apply(ColorAction::HdrIntensity { stops: 10. }).unwrap();
        s.state.color_picker.preview = Some(sampled.definition());
        let expected = s.state.preview_colors().into_owned();
        {
            let preview = s.color_preview();
            assert_eq!(preview.bubble_colors.len(), 1);
            let (owner, color) = &preview.bubble_colors[0];
            assert_eq!(*owner, id);
            let intensity = &color.intensity;
            assert!(intensity.stops > 6.);
            assert_eq!(intensity.stops, expected.hdr_intensity());
            assert_eq!(intensity.range, HdrIntensityArc::range(expected.hdr_intensity(), depth));
            assert_eq!(intensity.zero, intensity.geometry.zero_tick(intensity.range));
            assert_eq!(intensity.ramp, expected.intensity_ramp(s.effective_sdr_rendition(), intensity.range));
            let mapped = expected.mapped_swatch(expected.definition(), s.effective_sdr_rendition());
            assert_eq!(intensity.marker, [mapped[0], mapped[1], mapped[2], 1.]);
            assert_eq!(preview.colors.definition().linear_in(RgbSpace::Srgb).unwrap()[3], 0.37);
            assert_eq!(intensity.point, intensity.geometry.point((intensity.stops-intensity.range[0])/(intensity.range[1]-intensity.range[0])));
            let serialized = serde_json::to_value(&preview).unwrap();
            assert_eq!(serialized["bubble_colors"][0][0], id);
            assert_eq!(serialized["bubble_colors"][0][1], serde_json::to_value(color).unwrap());
        }
        assert_eq!(s.state.colors, committed);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_eq!(serde_json::to_value(s.bubble_presentations()).unwrap(), before);
        for editor in [true, false] {
            s.state.color_picker.editor = editor;
            s.state.color_picker.calibrating = !editor;
            assert!(s.color_preview().bubble_colors.is_empty());
        }
        s.state.color_picker.calibrating = false;
        s.state.color_picker.preview = None;
        let preview = s.color_preview();
        assert!(preview.bubble_colors.is_empty());
        assert_eq!(*preview.colors, committed);
        assert_eq!(serde_json::to_value(s.bubble_presentations()).unwrap(), before);
        invoke(&mut s, CommandId::QuickMask);
        s.state.color_picker.preview = Some(sampled.definition());
        assert!(s.color_preview().bubble_colors.is_empty());
        assert_eq!(s.engine.checkpoint(), checkpoint);
    }
}

#[test]
fn hdr_bubble_intensity_keeps_its_range_until_release() {
    use layer_core::color::SampleDepth;
    for depth in [SampleDepth::F16, SampleDepth::F32] {
        for platform in Platform::ALL {
            for initial in [-12., 12.] {
                for ending in [ContactPhase::Up, ContactPhase::Cancel] {
                    let mut s = hdr_desk(depth, platform);
                    let id = take_ring(&mut s, FAR);
                    act(&mut s, BubbleAction::SetIntensity { id, stops: initial }).unwrap();
                    let before = s.state.colors.clone();
                    let checkpoint = s.engine.checkpoint();
                    let range = HdrIntensityArc::range(initial, depth);
                    let arc = view(&s, id).color.unwrap().intensity.unwrap();
                    let position = arc.point(0.625);
                    let expected = arc.value(position, range);
                    for phase in [ContactPhase::Down, ContactPhase::Move, ContactPhase::Move, ContactPhase::Move] {
                        act(&mut s, BubbleAction::Intensity { id, phase, position }).unwrap();
                        assert!((s.state.colors.hdr_intensity() - expected).abs() < 0.01, "{depth:?} {platform:?} {initial} {phase:?}");
                        let packet = s.bubble_presentations().into_iter().find(|packet| packet.view.id == id).unwrap().color.unwrap();
                        assert_eq!(packet.intensity.range, range);
                        assert_eq!(packet.intensity.zero, arc.zero_tick(range));
                        assert!((packet.intensity.point[0] - position[0]).hypot(packet.intensity.point[1] - position[1]) < 0.01);
                    }
                    act(&mut s, BubbleAction::Intensity { id, phase: ending, position }).unwrap();
                    let stops = if ending == ContactPhase::Cancel { initial } else { expected };
                    assert!((s.state.colors.hdr_intensity() - stops).abs() < 0.01);
                    let packet = s.bubble_presentations().into_iter().find(|packet| packet.view.id == id).unwrap().color.unwrap();
                    assert_eq!(packet.intensity.range, HdrIntensityArc::range(stops, depth));
                    if ending == ContactPhase::Cancel { assert_eq!(s.state.colors, before); }
                    assert_eq!(s.engine.checkpoint(), checkpoint);
                }
            }
        }
    }
}

#[test]
fn hdr_bubble_intensity_commits_cancels_and_retires_with_its_color_context() {
    use layer_core::color::{RgbColor, RgbSpace, SampleDepth};
    for (depth, platform) in [SampleDepth::F16, SampleDepth::F32].into_iter()
        .flat_map(|depth| Platform::ALL.into_iter().map(move |platform| (depth, platform))) {
        let mut s = hdr_desk(depth, platform);
        s.dispatch(UiAction::Color { action: ColorAction::Definition { color: RgbColor::new(RgbSpace::Srgb, [0.2, 0.8, 0.4, 0.37]).unwrap() } }).unwrap();
        s.dispatch(UiAction::Color { action: ColorAction::HdrIntensity { stops: -1. } }).unwrap();
        let original = s.state.colors.clone();
        let checkpoint = s.engine.checkpoint();
        let id = take_ring(&mut s, FAR);
        let bubble = view(&s, id);
        let arc = bubble.color.unwrap().intensity.unwrap();
        let color = s.bubble_presentations().into_iter().find(|packet| packet.view.id == id).unwrap().color.unwrap();
        let mapped = original.mapped_swatch(original.definition(), s.effective_sdr_rendition());
        assert_eq!(color.intensity.marker, [mapped[0], mapped[1], mapped[2], 1.]);
        assert_eq!(s.state.colors.definition().linear_in(RgbSpace::Srgb).unwrap()[3], 0.37);
        let transported: BubbleHitGeometry = serde_json::from_value(serde_json::to_value(&bubble).unwrap()).unwrap();
        for fraction in [0., 0.5, 1.] {
            assert_eq!(bubble.hit(arc.point(fraction)), Some(BubbleHit::Intensity));
            assert_eq!(transported.hit(arc.point(fraction)), bubble.hit(arc.point(fraction)));
        }
        let change = |s: &mut UiSession<Recorder>, phase, stops: f32| act(s, BubbleAction::Intensity { id, phase, position: arc.point((stops + 2.) / 8.) }).unwrap();
        change(&mut s, ContactPhase::Down, 2.);
        change(&mut s, ContactPhase::Move, 4.);
        assert_eq!(s.state.colors.hdr_intensity(), 4.);
        assert_eq!(s.state.colors.picker_base(), original.picker_base());
        change(&mut s, ContactPhase::Cancel, 4.);
        assert_eq!(s.state.colors.definition(), original.definition());
        assert_eq!(s.state.colors.hdr_intensity(), -1.);
        change(&mut s, ContactPhase::Down, 3.);
        change(&mut s, ContactPhase::Up, 3.);
        wheel_contact(&mut s, id, ContactPhase::Down, ColorWheelPart::Field, [20., -10.]).unwrap();
        wheel_contact(&mut s, id, ContactPhase::Up, ColorWheelPart::Field, [20., -10.]).unwrap();
        assert_eq!(s.state.colors.hdr_intensity(), 3.);
        change(&mut s, ContactPhase::Down, 4.);
        s.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Background } }).unwrap();
        let background = s.state.colors.clone();
        change(&mut s, ContactPhase::Move, 6.);
        change(&mut s, ContactPhase::Cancel, 6.);
        assert_eq!(s.state.colors, background);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        act(&mut s, BubbleAction::SetIntensity { id, stops: 12. }).unwrap();
        let [low, high] = HdrIntensityArc::range(12., depth);
        let presentation = s.bubble_presentations().into_iter().find(|packet| packet.view.id == id).unwrap().color.unwrap();
        assert_eq!(presentation.intensity.range, [low, high]);
        assert_eq!(presentation.intensity.point, arc.point(1.));
        assert_eq!(presentation.intensity.zero, arc.zero_tick([low, high]));
        assert_eq!(presentation.intensity.path.len(), presentation.intensity.ramp.len());
        assert_eq!(presentation.intensity.path[0], arc.point(0.));
        assert_eq!(*presentation.intensity.path.last().unwrap(), arc.point(1.));
        for (actual, expected) in presentation.intensity.ramp.last().unwrap()[..3].iter().zip(presentation.intensity.marker) {
            assert!((actual - expected).abs() < 0.001, "expanded ramp endpoint matches the selected highlight");
        }
        act(&mut s, BubbleAction::Intensity { id, phase: ContactPhase::Down, position: arc.point((10. - low) / (high - low)) }).unwrap();
        assert!((s.state.colors.hdr_intensity() - 10.).abs() < 0.01, "sampled highlights retain their expanded intensity range");
        act(&mut s, BubbleAction::Intensity { id, phase: ContactPhase::Cancel, position: [0.; 2] }).unwrap();
        assert_eq!(s.state.colors.hdr_intensity(), 12.);
        s.dispatch(UiAction::Color { action: ColorAction::Select { slot: ColorSlot::Transparent } }).unwrap();
        let transparent = s.state.colors.clone();
        for phase in [ContactPhase::Down, ContactPhase::Move, ContactPhase::Up, ContactPhase::Cancel] {
            act(&mut s, BubbleAction::Intensity { id, phase, position: arc.point(0.5) }).unwrap();
            assert_eq!(s.state.colors, transparent, "{depth:?} {platform:?} {phase:?}");
            assert_eq!(s.engine.checkpoint(), checkpoint);
        }
        act(&mut s, BubbleAction::SetIntensity { id, stops: 0. }).unwrap();
        assert_eq!(s.state.colors, transparent, "{depth:?} {platform:?} transparent reset");
        assert_eq!(s.engine.checkpoint(), checkpoint);
        invoke(&mut s, CommandId::QuickMask);
        assert!(view(&s, id).color.unwrap().intensity.is_none());
        assert!(s.bubble_presentations().into_iter().find(|packet| packet.view.id == id).unwrap().color.is_none());
    }
}

#[test]
fn bubble_wheel_commits_the_pressed_part_and_cancel_restores_its_start() {
    let mut s = desk();
    let id = take_ring(&mut s, FAR);
    let original = s.state.colors.clone();
    let checkpoint = s.engine.checkpoint();
    let size = view(&s, id).diameter - 2. * RING_RIM;
    let mut expected = original.clone();
    for (phase, point) in [(ContactPhase::Down,[10.,-20.]),(ContactPhase::Move,[-15.,10.]),(ContactPhase::Up,[20.,15.])] {
        if phase != ContactPhase::Up {
            expected.apply(ColorAction::PickWheel { part: ColorWheelPart::Field, point: point.map(|v|v+size/2.), size }).unwrap();
        }
        wheel_contact(&mut s,id,phase,if phase == ContactPhase::Down { ColorWheelPart::Field } else { ColorWheelPart::Hue },point).unwrap();
        assert_eq!(s.state.colors.definition(), expected.definition(), "Move retains the pressed part; Up keeps its last sampled color");
    }
    assert_ne!(s.state.colors.definition(),original.definition());
    let committed = s.state.colors.definition();
    wheel_contact(&mut s,id,ContactPhase::Cancel,ColorWheelPart::Field,[0.;2]).unwrap();
    assert_eq!(s.state.colors.definition(),committed,"A late cancellation cannot undo a completed choice");
    wheel_contact(&mut s,id,ContactPhase::Down,ColorWheelPart::Field,[-20.,-15.]).unwrap();
    assert_ne!(s.state.colors.definition(),committed);
    wheel_contact(&mut s,id+100,ContactPhase::Cancel,ColorWheelPart::Field,[0.;2]).unwrap();
    assert_ne!(s.state.colors.definition(),committed,"Another owner's cancellation is ignored");
    wheel_contact(&mut s,id,ContactPhase::Cancel,ColorWheelPart::Hue,[0.;2]).unwrap();
    assert_eq!(s.state.colors.definition(),committed);
    assert_eq!(s.engine.checkpoint(),checkpoint,"Paint color choices do not edit artwork history");
}

#[test]
fn returning_a_wheel_retires_its_contact_before_layout_undo_reuses_the_id() {
    use layer_core::color::{RgbColor,RgbSpace};
    for all in [false,true] {
        let mut s=desk();
        let id=take_ring(&mut s,FAR);
        wheel_contact(&mut s,id,ContactPhase::Down,ColorWheelPart::Field,[20.,-10.]).unwrap();
        act(&mut s,if all {BubbleAction::PutBackAll} else {BubbleAction::PutBack {id}}).unwrap();
        invoke(&mut s,CommandId::UndoWorkspace);
        assert!(s.state.workspace.layout.bubbles.get(id).is_some());
        let color=RgbColor::new(RgbSpace::Srgb,[0.7,0.2,0.4,1.]).unwrap();
        s.dispatch(UiAction::Color {action:ColorAction::Definition {color}}).unwrap();
        wheel_contact(&mut s,id,ContactPhase::Cancel,ColorWheelPart::Field,[0.;2]).unwrap();
        assert_eq!(s.state.colors.definition(),color,"Returning the owner retires cancellation even when Undo reuses its ID; all={all}");
        wheel_contact(&mut s,id,ContactPhase::Down,ColorWheelPart::Field,[-10.,20.]).unwrap();
        wheel_contact(&mut s,id,ContactPhase::Cancel,ColorWheelPart::Field,[0.;2]).unwrap();
        assert_eq!(s.state.colors.definition(),color,"The restored owner accepts a fresh transaction");
    }
}

#[test]
fn bubble_wheel_replacement_cannot_restore_a_previous_context_color() {
    use layer_core::color::{RgbColor,RgbSpace};
    for replacement in ["slot","shape","document","workspace"] {
        let mut s = desk();
        let id = take_ring(&mut s,FAR);
        wheel_contact(&mut s,id,ContactPhase::Down,ColorWheelPart::Field,[20.,-10.]).unwrap();
        match replacement {
            "slot" => {s.dispatch(UiAction::Color {action:ColorAction::Select {slot:ColorSlot::Background}}).unwrap();}
            "shape" => {s.dispatch(UiAction::Color {action:ColorAction::Shape {shape:ColorShape::Square}}).unwrap();}
            "document" => {let mut next=desk();next.inherit_window_state(&s).unwrap();s=next;}
            "workspace" => {s.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(WorkspaceState::default())}).unwrap();}
            _ => unreachable!(),
        }
        let color=RgbColor::new(RgbSpace::Srgb,[0.15,0.7,0.3,1.]).unwrap();
        s.dispatch(UiAction::Color {action:ColorAction::Definition {color}}).unwrap();
        for phase in [ContactPhase::Move,ContactPhase::Cancel,ContactPhase::Up] {
            wheel_contact(&mut s,id,phase,ColorWheelPart::Field,[-10.,20.]).unwrap();
            assert_eq!(s.state.colors.definition(),color,"{replacement}: {phase:?} cannot retarget the old contact");
        }
    }
}

#[test]
fn bubble_color_continuations_retire_while_drawings_activate() {
    use layer_core::color::SampleDepth;
    for (depth, platform, intensity) in [SampleDepth::F16, SampleDepth::F32].into_iter()
        .flat_map(|depth| Platform::ALL.into_iter().flat_map(move |platform| [false, true].map(move |intensity| (depth, platform, intensity)))) {
        let mut source = hdr_desk(depth, platform);
        let id = take_ring(&mut source, FAR);
        source.frame(0, 0).unwrap();
        let position = if intensity { view(&source, id).color.unwrap().intensity.unwrap().point(0.75) } else { [20., -10.] };
        let send = |s: &mut UiSession<Recorder>, id, phase| act(s, if intensity {
            BubbleAction::Intensity { id, phase, position }
        } else { BubbleAction::Wheel { id, phase, part: ColorWheelPart::Field, position } });
        send(&mut source, id, ContactPhase::Down).unwrap();
        let source_color = source.state.colors.clone();
        let source_checkpoint = source.engine.checkpoint();
        let mut destination = desk();
        destination.frame(0, 0).unwrap();
        destination.park_document().unwrap();
        destination.inherit_window_state(&source).unwrap();
        source.park_document().unwrap();
        let destination_color = destination.state.colors.clone();
        let destination_checkpoint = destination.engine.checkpoint();
        let destination_layout = destination.state.workspace.layout.clone();
        for phase in [ContactPhase::Move, ContactPhase::Up, ContactPhase::Cancel] {
            send(&mut destination, id, phase).unwrap();
            assert_eq!(destination.state.colors, destination_color, "{depth:?} {platform:?} intensity={intensity} {phase:?}");
            assert_eq!(destination.engine.checkpoint(), destination_checkpoint);
            assert_eq!(destination.state.workspace.layout, destination_layout);
        }
        assert!(send(&mut destination, id, ContactPhase::Down).is_err());
        assert!(act(&mut destination, BubbleAction::SetIntensity { id, stops: 2. }).is_err());
        destination.replace_renderer(Recorder::default()).unwrap();
        source.inherit_window_state(&destination).unwrap();
        for phase in [ContactPhase::Move, ContactPhase::Up, ContactPhase::Cancel] {
            send(&mut source, id, phase).unwrap();
            assert_eq!(source.state.colors, source_color);
            assert_eq!(source.engine.checkpoint(), source_checkpoint);
        }
        assert!(send(&mut source, id, ContactPhase::Down).is_err());
        assert!(act(&mut source, BubbleAction::SetIntensity { id, stops: 2. }).is_err());
        source.replace_renderer(Recorder { color: source.engine.document().composition().color, ..Default::default() }).unwrap();
        source.dispatch(UiAction::Color { action: ColorAction::Definition { color:
            layer_core::color::RgbColor::new(layer_core::color::RgbSpace::Srgb, [0.15, 0.7, 0.3, 0.37]).unwrap() } }).unwrap();
        let fresh_color = source.state.colors.clone();
        send(&mut source, id, ContactPhase::Down).unwrap();
        assert_ne!(source.state.colors, fresh_color);
        send(&mut source, id + 1, ContactPhase::Cancel).unwrap();
        assert_ne!(source.state.colors, fresh_color);
        send(&mut source, id, ContactPhase::Cancel).unwrap();
        assert_eq!(source.state.colors, fresh_color);
        assert_eq!(source.engine.checkpoint(), source_checkpoint);
    }
}

#[test]
fn bubble_color_continuations_cannot_edit_an_unavailable_owner() {
    for intensity in [false, true] {
        for unavailable in ["renderer", "read_only", "transition"] {
            let mut s = hdr_desk(layer_core::color::SampleDepth::F32, Platform::Windows);
            let id = take_ring(&mut s, FAR);
            s.frame(0, 0).unwrap();
            let send = |s: &mut UiSession<Recorder>, phase| act(s, if intensity {
                BubbleAction::Intensity { id, phase, position: [40., 100.] }
            } else { BubbleAction::Wheel { id, phase, part: ColorWheelPart::Field, position: [20., -10.] } });
            send(&mut s, ContactPhase::Down).unwrap();
            match unavailable {
                "renderer" => { s.park_document().unwrap(); }
                "read_only" => s.workspace_read_only = true,
                "transition" => s.workspace_transition = true,
                _ => unreachable!(),
            }
            let colors = s.state.colors.clone();
            let checkpoint = s.engine.checkpoint();
            let layout = s.state.workspace.layout.clone();
            assert!(send(&mut s, ContactPhase::Move).is_err(), "{unavailable} intensity={intensity}");
            assert!(send(&mut s, ContactPhase::Down).is_err());
            assert!(act(&mut s, BubbleAction::SetIntensity { id, stops: 2. }).is_err());
            send(&mut s, ContactPhase::Up).unwrap();
            send(&mut s, ContactPhase::Cancel).unwrap();
            assert_eq!(s.state.colors, colors);
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert_eq!(s.state.workspace.layout, layout);
        }
    }
}

#[test]
fn bubble_wheel_rejects_invalid_contacts_and_preserves_transparent_and_mask_paint() {
    let mut s=desk();
    let id=take_ring(&mut s,FAR);
    let controls=take_control(&mut s,ToolbarControl::ColorPicker,[900.,450.]);
    let before=s.state.colors.definition();
    assert!(wheel_contact(&mut s,controls,ContactPhase::Down,ColorWheelPart::Field,[0.;2]).is_err());
    assert!(wheel_contact(&mut s,id,ContactPhase::Down,ColorWheelPart::Field,[f32::NAN,0.]).is_err());
    assert_eq!(s.state.colors.definition(),before);
    for mask in [false,true] {
        if mask {invoke(&mut s,CommandId::QuickMask);}
        let artwork=s.state.colors.clone();
        s.dispatch(UiAction::Color {action:ColorAction::Select {slot:ColorSlot::Transparent}}).unwrap();
        let original=s.state.display_colors().definition();
        wheel_contact(&mut s,id,ContactPhase::Down,ColorWheelPart::Field,[20.,-10.]).unwrap();
        assert_ne!(s.state.display_colors().definition(),original);
        assert!(wheel_contact(&mut s,id,ContactPhase::Down,ColorWheelPart::Hue,[0.;2]).is_err());
        assert!(wheel_contact(&mut s,id,ContactPhase::Move,ColorWheelPart::Field,[0.,f32::INFINITY]).is_err());
        wheel_contact(&mut s,id,ContactPhase::Cancel,ColorWheelPart::Field,[0.;2]).unwrap();
        assert_eq!(s.state.display_colors().definition(),original,"mask={mask}");
        if mask {assert_eq!(s.state.colors,artwork,"Mask wheel contacts leave artwork colors alone");}
    }
}

fn view(s: &UiSession<Recorder>, id: u32) -> BubbleView {
    s.bubbles().into_iter().find(|b| b.id == id).unwrap()
}

fn tap(s: &mut UiSession<Recorder>, id: u32) {
    act(s, BubbleAction::Tap { id, position: [0.; 2] }).unwrap();
}

fn open_ring(s: &mut UiSession<Recorder>, id: u32) {
    invoke(s, CommandId::Lasso);
    for _ in 0..2 {
        tap(s, id);
    }
    assert!(!view(s, id).chooser.is_empty());
}

fn saved(s: &UiSession<Recorder>, id: u32) -> [f32; 2] {
    s.state.workspace.layout.bubbles.get(id).unwrap().center(VIEW)
}

fn choose(s: &mut UiSession<Recorder>, id: u32, phase: ContactPhase, position: [f32; 2]) {
    act(s, BubbleAction::Choose { id, phase, position }).unwrap();
}

fn beads(s: &mut UiSession<Recorder>, id: u32, phase: ContactPhase, position: [f32; 2]) -> UiChange {
    act(s, BubbleAction::Beads { id, phase, position }).unwrap()
}

fn bead(view: &BubbleView, bead: Bead) -> [f32; 2] {
    view.beads.iter().find(|b| b.placement.bead == bead).unwrap().placement.center
}

fn tap_bead(s: &mut UiSession<Recorder>, id: u32, which: Bead) {
    let at = bead(&view(s, id), which);
    beads(s, id, ContactPhase::Down, at);
    beads(s, id, ContactPhase::Up, at);
}

fn items(view: &BubbleView, kind: ChooserKind) -> Vec<ChooserItem> {
    view.chooser.iter().filter(|i| i.kind == kind).cloned().collect()
}

fn big_palette(s: &mut UiSession<Recorder>) {
    let space = s.state.colors.rgb_space();
    let swatches = (0..40).map(|i| (format!("{i}"), layer_core::color::RgbColor { linear_rgb: None, space, rgba: [i as f32 / 40., 0.5, 0.2, 1.] })).collect();
    s.dispatch(UiAction::Color { action: ColorAction::Library { action: ColorLibraryAction::Import { name: "Forty".into(), swatches } } }).unwrap();
    let id = s.state.color_library.palettes.iter().find(|p| p.name == "Forty").unwrap().id;
    s.dispatch(UiAction::Color { action: ColorAction::Library { action: ColorLibraryAction::SelectPalette { id } } }).unwrap();
}

fn stroke(s: &mut UiSession<Recorder>, sequence: u64, phase: PenPhase, at: [f32; 2]) {
    let mut e = event(s, sequence, phase, 1.);
    e.surface_position.x = at[0];
    e.surface_position.y = at[1];
    s.pen(e);
}

fn several_brushes(s: &UiSession<Recorder>, item: &ChooserItem) -> bool {
    ToolGroup::ALL.into_iter().any(|g| *g.localized_label(s.localization()) == *item.label && tools::group_presets(g).nth(1).is_some())
}

fn record_colors(s: &mut UiSession<Recorder>, count: usize) {
    for i in 0..count {
        let color = layer_core::color::RgbColor { linear_rgb: None, space: s.state.colors.rgb_space(), rgba: [i as f32 / count as f32, 0.2, 0.4, 1.] };
        s.dispatch(UiAction::Color { action: ColorAction::Definition { color } }).unwrap();
        s.state.color_library.record_use(color);
    }
}

#[test]
fn dragging_a_tool_past_the_line_takes_one_bubble_in_one_undo_step() {
    let mut s = desk();
    let control = ToolbarControl::ToolSlot { slot: ToolSlotId::Drawing };
    let (panel, tile) = toolbar_tile(&mut s, control);
    let grab = BubbleGrab::Tile { panel, tile };
    let group = find_group(&s, VIEW, |g| g.panels.contains(&panel)).bounds;
    let close = [(group.x + group.width + 40.).min(VIEW[0] - 10.), group.y + 40.];
    let close = if group.contains(close[0], close[1]) { [group.x - 40., group.y + 40.] } else { close };
    drag(&mut s, grab, close, close);
    assert!(s.state.workspace.layout.bubbles.bubbles.is_empty(), "within the line the tool stays in its toolbar");
    let id = take(&mut s, grab, FAR);
    let bubble = s.state.workspace.layout.bubbles.get(id).unwrap().clone();
    assert_eq!(bubble.source, BubbleSource::Control { control });
    assert_eq!(saved(&s, id), FAR);
    assert!(s.state.workspace.layout.panel(panel).unwrap().tiles().iter().any(|t| t.id == tile), "the tile stays");
    drag(&mut s, grab, [500., 400.], [500., 400.]);
    assert_eq!(s.state.workspace.layout.bubbles.bubbles.len(), 1, "one bubble per source");
    assert_eq!(saved(&s, id), [500., 400.]);
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(saved(&s, id), FAR);
    invoke(&mut s, CommandId::UndoWorkspace);
    assert!(s.state.workspace.layout.bubbles.bubbles.is_empty());
    invoke(&mut s, CommandId::RedoWorkspace);
    assert_eq!(s.state.workspace.layout.bubbles.get(id), Some(&bubble));
    s.state.workspace.layout.validate().unwrap();
}

#[test]
fn dropping_a_bubble_on_a_panel_or_the_title_bar_puts_it_back() {
    let mut s = desk();
    let id = take(&mut s, brush(DefaultBrushPreset::PointyPencil), FAR);
    let group = s.layout(VIEW).groups[0].bounds;
    let grab = BubbleGrab::Bubble { id };
    contact(&mut s, grab, ContactPhase::Down, FAR).unwrap();
    contact(&mut s, grab, ContactPhase::Move, [group.x + 4., group.y + 4.]).unwrap();
    assert!(s.bubble_placements()[0].homing, "the lifted bubble shows it will go back");
    contact(&mut s, grab, ContactPhase::Up, [group.x + 4., group.y + 4.]).unwrap();
    assert!(s.state.workspace.layout.bubbles.bubbles.is_empty());
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(saved(&s, id), FAR);
    drag(&mut s, grab, FAR, [600., 8.]);
    assert!(s.state.workspace.layout.bubbles.bubbles.is_empty());
    let id = take(&mut s, brush(DefaultBrushPreset::PointyPencil), FAR);
    act(&mut s, BubbleAction::PutBack { id }).unwrap();
    assert!(s.state.workspace.layout.bubbles.bubbles.is_empty());
}

#[test]
fn put_back_all_empties_the_desk_in_one_step() {
    let mut s = desk();
    assert!(act(&mut s, BubbleAction::PutBackAll).is_err(), "nothing is out");
    assert!(!s.command(CommandId::PutBackBubbles).enabled);
    take(&mut s, brush(DefaultBrushPreset::PointyPencil), FAR);
    let color = take_ring(&mut s, [300., 500.]);
    tap_bead(&mut s, color, Bead::Palettes);
    assert!(s.command(CommandId::PutBackBubbles).enabled);
    invoke(&mut s, CommandId::PutBackBubbles);
    assert!(s.bubbles().is_empty() && s.bubble_placements().is_empty());
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(s.state.workspace.layout.bubbles.bubbles.len(), 2);
    assert!(view(&s, color).pill.is_none(), "put back closes open parts");
    assert!(VIEW_MENU.sections.iter().any(|section| section.contains(&CommandId::PutBackBubbles)));
}

#[test]
fn cancelled_and_competing_drags_leave_the_layout_unchanged() {
    let mut s = desk();
    let before = s.state.workspace.layout.clone();
    let grab = brush(DefaultBrushPreset::PointyPencil);
    contact(&mut s, grab, ContactPhase::Down, FAR).unwrap();
    assert!(contact(&mut s, brush(DefaultBrushPreset::Charcoal), ContactPhase::Down, [200., 300.]).is_err());
    contact(&mut s, grab, ContactPhase::Cancel, FAR).unwrap();
    assert_eq!(s.state.workspace.layout, before);
    assert!(s.bubble_placements().is_empty());
    assert!(contact(&mut s, grab, ContactPhase::Up, FAR).unwrap().regions == 0, "a late release is ignored");
}

#[test]
fn brush_bubbles_come_out_selected_with_their_halo_and_taps_select_and_toggle_it() {
    let mut s = desk();
    let pencil = DefaultBrushPreset::PointyPencil as u32;
    let id = take(&mut s, brush(DefaultBrushPreset::PointyPencil), FAR);
    assert_eq!(s.state.brush.preset, pencil, "a brush comes out selected");
    assert!(view(&s, id).open && !view(&s, id).rings.is_empty(), "with its settings around it");
    tap(&mut s, id);
    assert!(!view(&s, id).open);
    drag(&mut s, BubbleGrab::Bubble { id }, FAR, [500., 400.]);
    assert!(!view(&s, id).open, "moving a bubble on the desk leaves it as it was");
    s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 }).unwrap();
    tap(&mut s, id);
    assert_eq!(s.state.brush.preset, pencil);
    let open = view(&s, id);
    let defaults = tool_settings::ring_defaults(s.state.brush.preset, s.engine.configured_brush(), &s.state.tool_settings);
    assert!(open.open && !defaults.is_empty());
    assert_eq!(open.halo, Some(halo_disc(defaults.len())));
    assert_eq!(open.rings.iter().map(|r| r.setting).collect::<Vec<_>>(), defaults);
    assert_eq!(open.rings.iter().map(|r| r.arc).collect::<Vec<_>>(), halo_arcs(defaults.len()));
    tap(&mut s, id);
    assert!(!view(&s, id).open, "the next tap closes it");
    s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 }).unwrap();
    tap(&mut s, id);
    assert!(view(&s, id).open, "selecting the brush opens its settings again");
    s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 }).unwrap();
    s.dispatch(UiAction::SelectBrush { id: pencil }).unwrap();
    tap(&mut s, id);
    assert!(view(&s, id).open, "a selected brush shows its settings on the first tap");
}

#[test]
fn dial_taps_select_with_the_halo_then_show_the_media_ring() {
    let mut s = desk();
    let id = take_control(&mut s, ToolbarControl::Command { command: CommandId::DrawingBrush }, FAR);
    invoke(&mut s, CommandId::Lasso);
    assert!(!view(&s, id).selected);
    tap(&mut s, id);
    assert!(view(&s, id).selected && view(&s, id).chooser.is_empty());
    assert!(!view(&s, id).rings.is_empty() && view(&s, id).chooser.is_empty());
    tap(&mut s, id);
    let open = view(&s, id);
    assert!(open.rings.is_empty(), "the ring replaces the halo");
    let media = items(&open, ChooserKind::Medium);
    assert!(media.len() > 2);
    assert!(media.iter().all(|m| (m.center[0].hypot(m.center[1]) - CHOOSER_RING).abs() < 1. || m.center[0].hypot(m.center[1]) > CHOOSER_RING));
    let current = tools::group(s.state.brush.preset);
    let shown: Vec<_> = media.iter().filter(|m| m.shown).collect();
    assert_eq!(shown.len(), 1, "the current medium is shown with its label");
    assert!(shown[0].selected);
    let brushes = items(&open, ChooserKind::Brush);
    assert_eq!(brushes.len(), tools::group_presets(current).count(), "its brushes curve beside it");
    assert!(brushes.iter().all(|b| b.preview.is_some()));
    assert_eq!(brushes.iter().filter(|b| b.selected).count(), 1);
    tap(&mut s, id);
    assert!(!view(&s, id).open, "the next tap closes it");
}

#[test]
fn clicking_a_tool_bubble_selects_and_opens_its_sliders_on_every_platform() {
    for platform in Platform::ALL {
        for source in [
            BubbleSource::Medium { group: ToolGroup::Pastel },
            BubbleSource::Control { control: ToolbarControl::Brush { id: DefaultBrushPreset::PointyPencil as u32 } },
            BubbleSource::Control { control: ToolbarControl::ToolSlot { slot: ToolSlotId::Drawing } },
            BubbleSource::Control { control: ToolbarControl::Command { command: CommandId::DrawingBrush } },
        ] {
            let mut s = session(platform);
            s.set_viewport(VIEW, [1200, 900]).unwrap();
            s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::PointyPencil as u32 }).unwrap();
            let id = take_source(&mut s, source);
            invoke(&mut s, CommandId::Lasso);
            assert!(!view(&s, id).selected);
            let change = act(&mut s, BubbleAction::Tap { id, position: [0.; 2] }).unwrap();
            let selected = view(&s, id);
            let defaults = tool_settings::ring_defaults(s.state.brush.preset, s.engine.configured_brush(), &s.state.tool_settings);
            assert!(selected.selected && selected.chooser.is_empty());
            assert!(!defaults.is_empty());
            assert_eq!(selected.rings.iter().map(|ring| ring.setting).collect::<Vec<_>>(), defaults, "{platform:?}/{source:?}");
            assert_ne!(change.regions & regions::BUBBLES, 0);
        }
    }
}

#[test]
fn every_paint_tool_command_creates_and_restores_a_selectable_bubble_with_rings() {
    for platform in Platform::ALL {
        for tool in Tool::ALL {
            let mut s = session(platform);
            s.set_viewport(VIEW, [1200, 900]).unwrap();
            let control = ToolbarControl::Command { command: tool.command() };
            let (panel, tile) = toolbar_tile(&mut s, control);
            let grab = BubbleGrab::Tile { panel, tile };
            assert_eq!(s.bubble_grab(grab, false), Some(grab), "{platform:?}/{tool:?}");
            assert!(s.can_take_bubble(grab, FAR, VIEW), "{platform:?}/{tool:?}");
            let id = take(&mut s, grab, FAR);
            let source = BubbleSource::Control { control };
            assert_eq!(s.state.workspace.layout.bubbles.get(id).unwrap().source, source);
            source.validate().unwrap();
            let capture = serde_json::from_str(&serde_json::to_string(&s.capture_workspace().unwrap()).unwrap()).unwrap();
            let mut restored = session(platform);
            restored.set_viewport(VIEW, [1200, 900]).unwrap();
            restored.restore_editing(s.editing_state()).unwrap();
            restored.adopt_workspace(PreparedWorkspace::new(capture).unwrap()).unwrap();
            assert_eq!(restored.state.workspace.layout.bubbles.get(id).unwrap().source, source);
            let artwork = restored.engine.document().artwork.clone();
            invoke(&mut restored, CommandId::Lasso);
            assert!(!view(&restored, id).selected);
            tap(&mut restored, id);
            assert!(restored.command(tool.command()).selected);
            let selected = view(&restored, id);
            let defaults = tool_settings::ring_defaults(restored.state.brush.preset, restored.engine.configured_brush(), &restored.state.tool_settings);
            assert!(selected.selected && selected.chooser.is_empty(), "{platform:?}/{tool:?}");
            assert!(!defaults.is_empty(), "{platform:?}/{tool:?}");
            assert_eq!(selected.rings.iter().map(|ring| ring.setting).collect::<Vec<_>>(), defaults, "{platform:?}/{tool:?}");
            assert_eq!(restored.engine.document().artwork, artwork);
        }
    }
}

#[test]
fn the_media_ring_shows_a_mediums_brushes_and_a_choice_closes_it() {
    let mut s = desk();
    let id = take_control(&mut s, ToolbarControl::Command { command: CommandId::DrawingBrush }, [480., 520.]);
    open_ring(&mut s, id);
    let open = view(&s, id);
    let other = items(&open, ChooserKind::Medium).into_iter().find(|m| !m.shown && m.enabled && several_brushes(&s, m)).unwrap();
    choose(&mut s, id, ContactPhase::Down, other.center);
    choose(&mut s, id, ContactPhase::Up, other.center);
    let switched = view(&s, id);
    assert!(items(&switched, ChooserKind::Medium).iter().any(|m| m.shown && m.label == other.label));
    let column = items(&switched, ChooserKind::Brush);
    let side = column[0].center[0].signum();
    assert_eq!(side, other.center[0].signum(), "brushes open on the medium's side");
    choose(&mut s, id, ContactPhase::Down, [0., 0.]);
    choose(&mut s, id, ContactPhase::Up, [0., 0.]);
    assert!(!view(&s, id).chooser.is_empty(), "letting go on nothing keeps the ring");
    let pick = column.iter().find(|b| !b.selected).unwrap().clone();
    choose(&mut s, id, ContactPhase::Down, pick.center);
    choose(&mut s, id, ContactPhase::Up, pick.center);
    assert_eq!(tools::brush_label_localized(s.state.brush.preset, s.localization()).unwrap().to_string(), pick.label);
    assert!(view(&s, id).chooser.is_empty(), "picking a brush closes the ring");
}

#[test]
fn a_medium_with_one_brush_still_fans_it() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        s.set_viewport(VIEW, [1200, 900]).unwrap();
        let id = take_source(&mut s, BubbleSource::Medium { group: ToolGroup::Eraser });
        open_ring(&mut s, id);
        let brushes = items(&view(&s, id), ChooserKind::Brush);
        assert_eq!(brushes.len(), 1);
        assert_eq!(brushes[0].preview, Some(DefaultBrushPreset::Eraser as u32));
        assert!(!view(&s, id).chooser.is_empty(), "its one brush remains available until chosen");
        choose(&mut s, id, ContactPhase::Down, brushes[0].center);
        choose(&mut s, id, ContactPhase::Up, brushes[0].center);
        assert_eq!(s.state.brush.preset, DefaultBrushPreset::Eraser as u32);
        assert!(view(&s, id).chooser.is_empty());
    }
}

#[test]
fn a_press_can_slide_through_a_medium_to_its_brush() {
    let mut s = desk();
    let id = take_control(&mut s, ToolbarControl::Command { command: CommandId::DrawingBrush }, FAR);
    open_ring(&mut s, id);
    let media = items(&view(&s, id), ChooserKind::Medium);
    let [from, through] = [media.iter().find(|m| m.shown).unwrap(), media.iter().find(|m| !m.shown && m.enabled && several_brushes(&s, m)).unwrap()];
    choose(&mut s, id, ContactPhase::Down, from.center);
    choose(&mut s, id, ContactPhase::Move, through.center);
    let column = items(&view(&s, id), ChooserKind::Brush);
    assert!(items(&view(&s, id), ChooserKind::Medium).iter().any(|m| m.shown && m.label == through.label), "sliding onto a medium fans it");
    let target = column.last().unwrap();
    choose(&mut s, id, ContactPhase::Move, target.center);
    choose(&mut s, id, ContactPhase::Up, target.center);
    assert_eq!(s.state.brush.preset, target.preview.unwrap());
    assert!(view(&s, id).chooser.is_empty());
}

#[test]
fn a_selection_dial_rings_its_tools_directly() {
    let mut s = desk();
    let id = take_control(&mut s, ToolbarControl::ToolSlot { slot: ToolSlotId::Marquee }, FAR);
    assert!(view(&s, id).selected && !view(&s, id).open, "a tool without settings comes out selected and closed");
    tap(&mut s, id);
    let open = view(&s, id);
    let tools = items(&open, ChooserKind::Tool);
    assert!(tools.len() >= 2 && open.chooser.len() == tools.len(), "tools without settings go straight to the ring");
    let pick = tools.iter().find(|t| !t.selected).unwrap().clone();
    choose(&mut s, id, ContactPhase::Down, pick.center);
    choose(&mut s, id, ContactPhase::Up, pick.center);
    assert!(view(&s, id).chooser.is_empty());
    assert_eq!(view(&s, id).label, pick.label);
}

#[test]
fn the_ring_folds_toward_open_space_near_a_corner() {
    let mut s = desk();
    let work = s.layout(VIEW).work_area;
    let corner = [work.x + 40., work.y + work.height - 40.];
    let id = take_control(&mut s, ToolbarControl::Command { command: CommandId::DrawingBrush }, corner);
    open_ring(&mut s, id);
    let center = s.bubble_placements()[0].center;
    for item in view(&s, id).chooser {
        let [w, h] = item.size();
        let at = [center[0] + item.center[0], center[1] + item.center[1]];
        assert!(at[0] - w * 0.5 >= work.x && at[1] + h * 0.5 <= work.y + work.height, "{} at {at:?}", item.label);
    }
}

#[test]
fn edge_choosers_settle_without_reflowing_against_their_own_motion() {
    for reduced in [false, true] {
        for fraction in [[0.05, 0.05], [0.95, 0.05], [0.05, 0.95], [0.95, 0.95], [0.95, 0.2]] {
            let mut s = desk();
            let work = s.layout(VIEW).work_area;
            let at = [work.x + work.width * fraction[0], work.y + work.height * fraction[1]];
            let id = take_control(&mut s, ToolbarControl::Command { command: CommandId::DrawingBrush }, FAR);
            s.state.workspace.layout.bubbles.get_mut(id).unwrap().place(at, VIEW);
            open_ring(&mut s, id);
            s.reduce_bubble_motion(reduced);
            for frame in 0..300 {
                s.step_bubbles(frame * 16_666_667);
            }
            let geometry = view(&s, id).chooser;
            let center = s.bubble_placements()[0].center;
            for frame in 300..320 {
                assert!(!s.step_bubbles(frame * 16_666_667), "edge chooser keeps moving at {fraction:?}, reduced={reduced}");
                assert_eq!(view(&s, id).chooser, geometry, "settled chooser changes its layout");
                assert_eq!(s.bubble_placements()[0].center, center, "settled chooser changes its center");
            }
        }
    }
}

#[test]
fn medium_bubbles_select_their_brushes_and_fan_only_them() {
    let mut s = desk();
    let id = take(&mut s, BubbleGrab::Variant { variant: ToolVariant::BrushGroup { group: ToolGroup::Pastel } }, FAR);
    let brushes = tools::group_presets(ToolGroup::Pastel).count();
    assert_eq!(view(&s, id).dots.len(), brushes);
    assert_eq!(tools::group(s.state.brush.preset), ToolGroup::Pastel, "a medium comes out selected");
    assert_eq!(view(&s, id).dots.iter().filter(|d| **d).count(), 1);
    assert!(!view(&s, id).rings.is_empty());
    tap(&mut s, id);
    let fan = view(&s, id).chooser;
    assert_eq!(fan.len(), brushes);
    assert!(fan.iter().all(|i| i.kind == ChooserKind::Brush));
    let pick = fan.iter().find(|i| !i.selected).unwrap().clone();
    choose(&mut s, id, ContactPhase::Down, pick.center);
    choose(&mut s, id, ContactPhase::Up, pick.center);
    assert_eq!(Some(s.state.brush.preset), pick.preview);
    assert!(view(&s, id).chooser.is_empty() && !view(&s, id).rings.is_empty());
}

#[test]
fn watercolor_pills_leave_the_material_face_available_on_every_platform() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        s.set_viewport(VIEW, [1200, 900]).unwrap();
        let id = take_source(&mut s, BubbleSource::Medium { group: ToolGroup::Watercolor });
        tap(&mut s, id);
        let open = view(&s, id);
        assert_eq!(open.chooser.len(), tools::group_presets(ToolGroup::Watercolor).count());
        for y in -13..=13 {
            for x in -13..=13 {
                let point = [x as f32 * 2., y as f32 * 2.];
                if point[0].hypot(point[1]) < TOOL_BUBBLE * 0.5 {
                    assert_eq!(open.hit(point), Some(BubbleHit::Face), "{platform:?}: a Watercolor pill covers the material face at {point:?}");
                }
            }
        }
    }
}

#[test]
fn chooser_picks_open_the_selected_tool_halo_on_every_platform() {
    for platform in Platform::ALL {
        for source in [
            BubbleSource::Medium { group: ToolGroup::Pastel },
            BubbleSource::Control { control: ToolbarControl::ToolSlot { slot: ToolSlotId::Drawing } },
            BubbleSource::Control { control: ToolbarControl::Command { command: CommandId::DrawingBrush } },
        ] {
            for same_tool in [false, true] {
                let mut s = session(platform);
                s.set_viewport(VIEW, [1200, 900]).unwrap();
                s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::PointyPencil as u32 }).unwrap();
                let id = take_source(&mut s, source);
                open_ring(&mut s, id);
                let pick = view(&s, id).chooser.into_iter().find(|item| matches!(item.kind, ChooserKind::Brush | ChooserKind::Tool) && item.selected == same_tool)
                    .unwrap_or_else(|| panic!("{platform:?}/{source:?}/{same_tool}: chooser needs a matching tool"));
                choose(&mut s, id, ContactPhase::Down, pick.center);
                let change = act(&mut s, BubbleAction::Choose { id, phase: ContactPhase::Up, position: pick.center }).unwrap();
                let selected = view(&s, id);
                let defaults = tool_settings::ring_defaults(s.state.brush.preset, s.engine.configured_brush(), &s.state.tool_settings);
                if let Some(brush) = pick.preview {
                    assert_eq!(s.state.brush.preset, brush);
                } else {
                    assert_eq!(selected.label, pick.label);
                }
                assert!(selected.selected && selected.chooser.is_empty());
                assert!(!defaults.is_empty());
                assert_eq!(selected.rings.iter().map(|ring| ring.setting).collect::<Vec<_>>(), defaults, "{platform:?}/{source:?}/{same_tool}");
                assert_ne!(change.regions & regions::BUBBLES, 0);
                s.bubble_stroke(PenPhase::Down, [FAR[0], FAR[1] + 80.]);
                assert!(s.bubble_fade().1.contains(&id), "the new halo participates in stroke fading immediately");
                s.bubble_stroke(PenPhase::Up, [FAR[0], FAR[1] + 80.]);
                let ring = selected.rings[0].clone();
                for phase in [ContactPhase::Down, ContactPhase::Up] {
                    act(&mut s, BubbleAction::Ring { id, phase, position: ring.arc.point(0.7) }).unwrap();
                }
                assert!(!view(&s, id).rings.is_empty());
                s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 }).unwrap();
                assert!(view(&s, id).rings.is_empty());
            }
        }
    }
}

#[test]
fn every_tool_group_and_medium_bubble_shares_the_workspace_show_sliders_state() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        assert!(s.state.workspace.layout.bubbles.show_sliders);
        let sources: Vec<_> = tools::brush_ids().map(|id| BubbleSource::Control { control: ToolbarControl::Brush { id } })
            .chain(ToolGroup::ALL.into_iter().map(|group| BubbleSource::Medium { group }))
            .chain(ToolSlotId::ALL.into_iter().map(|slot| BubbleSource::Control { control: ToolbarControl::ToolSlot { slot } }))
            .chain([BubbleSource::Control { control: ToolbarControl::ColorPicker }]).collect();
        for show in [true, false] {
            s.state.workspace.layout.bubbles.show_sliders = show;
            for &source in &sources {
                let id = s.state.workspace.layout.allocate().unwrap();
                s.state.workspace.layout.bubbles.bubbles.push(ToolBubble::new(id, source, FAR, VIEW));
                assert_eq!(show_sliders(&s, id).selected, Some(show), "{platform:?}/{source:?}");
                s.state.workspace.layout.bubbles.bubbles.pop();
            }
        }
    }
}

#[test]
fn show_sliders_controls_clicks_choices_and_drag_out_with_workspace_history_on_every_platform() {
    for platform in Platform::ALL {
        for source in [
            BubbleSource::Medium { group: ToolGroup::Pastel },
            BubbleSource::Control { control: ToolbarControl::Brush { id: DefaultBrushPreset::PointyPencil as u32 } },
            BubbleSource::Control { control: ToolbarControl::ToolSlot { slot: ToolSlotId::Drawing } },
            BubbleSource::Control { control: ToolbarControl::Command { command: CommandId::DrawingBrush } },
        ] {
            let mut s = session(platform);
            s.set_viewport(VIEW, [1200, 900]).unwrap();
            s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::PointyPencil as u32 }).unwrap();
            let id = take_source(&mut s, source);
            let option = show_sliders(&s, id);
            assert_eq!(option.selected, Some(true));
            assert_eq!(serde_json::to_value(&option).unwrap()["keep_open"], true);
            s.dispatch(option.action.unwrap()).unwrap();
            assert!(view(&s, id).rings.is_empty());
            assert_eq!(show_sliders(&s, id).selected, Some(false));
            invoke(&mut s, CommandId::UndoWorkspace);
            assert_eq!(show_sliders(&s, id).selected, Some(true));
            invoke(&mut s, CommandId::RedoWorkspace);
            assert_eq!(show_sliders(&s, id).selected, Some(false));
            let other = take(&mut s, brush(DefaultBrushPreset::GPen), [850., 500.]);
            assert_eq!(show_sliders(&s, other).selected, Some(false));
            assert!(view(&s, other).rings.is_empty());
            act(&mut s, BubbleAction::PutBack { id: other }).unwrap();
            invoke(&mut s, CommandId::Lasso);
            tap(&mut s, id);
            assert!(view(&s, id).selected && view(&s, id).rings.is_empty());
            if source.kind() != BubbleKind::Brush {
                tap(&mut s, id);
                let pick = view(&s, id).chooser.into_iter().find(|item| matches!(item.kind, ChooserKind::Brush | ChooserKind::Tool) && item.enabled).unwrap();
                choose(&mut s, id, ContactPhase::Down, pick.center);
                choose(&mut s, id, ContactPhase::Up, pick.center);
                assert!(view(&s, id).selected && view(&s, id).chooser.is_empty() && view(&s, id).rings.is_empty());
            }
            let active = serde_json::from_str(&serde_json::to_string(&s.state.workspace).unwrap()).unwrap();
            s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(active) }).unwrap();
            assert_eq!(show_sliders(&s, id).selected, Some(false));
            act(&mut s, BubbleAction::PutBack { id }).unwrap();
            let parked = serde_json::from_str(&serde_json::to_string(&s.state.workspace).unwrap()).unwrap();
            s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(parked) }).unwrap();
            let id = take_source(&mut s, source);
            assert!(view(&s, id).selected && view(&s, id).rings.is_empty());
            tap(&mut s, id);
            assert!(view(&s, id).rings.is_empty());
            s.dispatch(show_sliders(&s, id).action.unwrap()).unwrap();
            assert_eq!(show_sliders(&s, id).selected, Some(true));
            assert!(!view(&s, id).rings.is_empty() && view(&s, id).chooser.is_empty());
            act(&mut s, BubbleAction::PutBack { id }).unwrap();
            let id = take_source(&mut s, source);
            assert!(!view(&s, id).rings.is_empty());
        }
    }
}

#[test]
fn show_sliders_changes_all_existing_bubbles_and_survives_an_empty_workspace() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        s.set_viewport(VIEW, [1200, 900]).unwrap();
        let brush = take_source(&mut s, BubbleSource::Control { control: ToolbarControl::Brush { id: DefaultBrushPreset::PointyPencil as u32 } });
        let group = take_source(&mut s, BubbleSource::Control { control: ToolbarControl::Command { command: CommandId::DrawingBrush } });
        let medium = take_source(&mut s, BubbleSource::Medium { group: ToolGroup::Pastel });
        let controls = take_control(&mut s, ToolbarControl::ColorPicker, [850., 500.]);
        s.dispatch(show_sliders(&s, group).action.unwrap()).unwrap();
        for id in [brush, group, medium, controls] {
            assert_eq!(show_sliders(&s, id).selected, Some(false));
            assert!(view(&s, id).rings.is_empty());
        }
        let other_options = BubbleOptions { swap: true, ..view(&s, controls).options };
        act(&mut s, BubbleAction::SetOptions { id: controls, options: other_options }).unwrap();
        assert!(!s.state.workspace.layout.bubbles.show_sliders);
        s.dispatch(show_sliders(&s, brush).action.unwrap()).unwrap();
        for id in [brush, group, medium, controls] {
            assert_eq!(show_sliders(&s, id).selected, Some(true));
        }
        let selected = if view(&s, group).rings.is_empty() { medium } else { group };
        assert!(!view(&s, selected).rings.is_empty(), "enabling from another tool's menu reopens the selected tool's sliders");
        assert!(!view(&s, controls).rings.is_empty());
        invoke(&mut s, CommandId::UndoWorkspace);
        assert!(!s.state.workspace.layout.bubbles.show_sliders);
        assert!(view(&s, selected).rings.is_empty() && view(&s, controls).rings.is_empty());
        invoke(&mut s, CommandId::RedoWorkspace);
        assert!(s.state.workspace.layout.bubbles.show_sliders);
        s.dispatch(show_sliders(&s, controls).action.unwrap()).unwrap();
        act(&mut s, BubbleAction::SetOptions { id: controls, options: BubbleOptions::default() }).unwrap();
        act(&mut s, BubbleAction::PutBackAll).unwrap();
        assert!(s.state.workspace.layout.bubbles.bubbles.is_empty());
        assert!(s.state.workspace.layout.bubbles.kept.is_empty());
        assert!(!s.state.workspace.layout.bubbles.is_empty());
        let restored = serde_json::from_str(&serde_json::to_string(&s.state.workspace).unwrap()).unwrap();
        s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(restored) }).unwrap();
        let id = take_source(&mut s, BubbleSource::Medium { group: ToolGroup::Oil });
        assert_eq!(show_sliders(&s, id).selected, Some(false));
        assert!(view(&s, id).rings.is_empty());
    }
}

#[test]
fn the_color_picker_show_sliders_option_updates_its_geometry_and_survives_put_back() {
    let mut s = desk();
    let id = take_control(&mut s, ToolbarControl::ColorPicker, FAR);
    assert_eq!(show_sliders(&s, id).selected, Some(true));
    let point = view(&s, id).rings[0].arc.point(0.4);
    act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Down, position: point }).unwrap();
    assert!(view(&s, id).stamp.is_some());
    s.dispatch(show_sliders(&s, id).action.unwrap()).unwrap();
    assert_eq!(view(&s, id).diameter, TOOL_BUBBLE);
    assert!(view(&s, id).rings.is_empty() && view(&s, id).stamp.is_none());
    act(&mut s, BubbleAction::PutBack { id }).unwrap();
    let id = take_control(&mut s, ToolbarControl::ColorPicker, FAR);
    assert_eq!(show_sliders(&s, id).selected, Some(false));
    assert!(view(&s, id).rings.is_empty());
    s.dispatch(show_sliders(&s, id).action.unwrap()).unwrap();
    assert_eq!(view(&s, id).diameter, halo_disc(CONTROL_RINGS.len()));
    assert_eq!(view(&s, id).rings.len(), CONTROL_RINGS.len());
}

#[test]
fn halo_contacts_set_values_along_their_arc() {
    let mut s = desk();
    let id = take(&mut s, brush(DefaultBrushPreset::PointyPencil), FAR);
    let ring = view(&s, id).rings[0].clone();
    let value = |s: &UiSession<Recorder>| s.state.tool_settings.iter().find(|c| c.id == ring.setting).unwrap().value;
    let numeric = s.state.tool_settings.iter().find(|c| c.id == ring.setting).unwrap().numeric.clone();
    let at = |f: f64| numeric.resolve(0., NumericOperation::Position { position: f }).unwrap().value as f32;
    act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Down, position: ring.arc.point(0.2) }).unwrap();
    let dragging = view(&s, id);
    assert!(dragging.rings[0].active);
    assert!(dragging.readout.as_ref().is_some_and(|r| r.contains(ring.label.as_ref())), "the name and value show while dragging");
    assert!((value(&s) - at(0.2)).abs() < 1e-3);
    act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Move, position: [ring.arc.point(0.9)[0] * 3., ring.arc.point(0.9)[1] * 3.] }).unwrap();
    assert!((value(&s) - at(0.9)).abs() < 1e-3, "the contact stays with its arc");
    act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Up, position: ring.arc.point(0.9) }).unwrap();
    assert!(!view(&s, id).rings[0].active && view(&s, id).readout.is_none());
    assert!(act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Down, position: [0., 0.] }).is_err());
    act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Down, position: ring.arc.point(0.3) }).unwrap();
    act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Move, position: ring.arc.point(0.5) }).unwrap();
    act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Cancel, position: [0., 0.] }).unwrap();
    assert!((value(&s) - at(0.9)).abs() < 1e-3, "a cancelled contact, such as a second finger or a hold, puts the value back");
    assert!(!view(&s, id).rings[0].active);
    act(&mut s, BubbleAction::ResetRing { id, position: ring.arc.point(0.5) }).unwrap();
    let reset = value(&s);
    assert!((reset - at(0.9)).abs() > 1e-3, "a double click on an arc resets its setting");
    s.dispatch(UiAction::ResetToolSetting { id: ring.setting.into() }).unwrap();
    assert_eq!(value(&s), reset, "to its default");
    assert!(act(&mut s, BubbleAction::ResetRing { id, position: [0., 0.] }).is_err());
}

#[test]
fn the_color_picker_comes_out_with_size_and_strength_always_open() {
    let mut s = desk();
    let id = take_control(&mut s, ToolbarControl::ColorPicker, FAR);
    let bubble = view(&s, id);
    assert_eq!((bubble.kind, bubble.face), (BubbleKind::Controls, [TOOL_BUBBLE; 2]));
    assert_eq!(bubble.rings.len(), 2, "both sliders are open without a tap");
    assert_eq!(bubble.rings[1].setting, "size", "Size is on the right");
    assert!(bubble.rings[1].arc.point(0.5)[0] > 0.);
    let size = |s: &UiSession<Recorder>| ToolbarNumericBinding::BrushSize.field(&s.state).unwrap().value;
    let before = size(&s);
    let ring = bubble.rings[1].clone();
    let pencil = take(&mut s, brush(DefaultBrushPreset::PointyPencil), [300., 300.]);
    act(&mut s, BubbleAction::ToggleRingSetting { brush: DefaultBrushPreset::PointyPencil as u32, setting: "size".into() }).unwrap();
    assert!(view(&s, pencil).rings.iter().any(|r| r.setting == "size"), "a brush halo shows Size too");
    act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Down, position: ring.arc.point(0.8) }).unwrap();
    assert_ne!(size(&s), before);
    let halo = view(&s, pencil);
    assert!(halo.readout.is_none() && halo.rings.iter().all(|r| !r.active), "only the bubble being dragged shows its value");
    let stamp = view(&s, id).stamp.expect("a dab in the current color follows the contact");
    assert_eq!(stamp.center, ring.arc.point(0.8));
    assert_eq!(stamp.color, s.state.colors.definition());
    assert!((stamp.diameter - size(&s) * s.state.camera.zoom).abs() < 1e-3, "{stamp:?}");
    act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Move, position: ring.arc.point(0.6) }).unwrap();
    assert_eq!(view(&s, id).stamp.unwrap().center, ring.arc.point(0.6));
    act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Up, position: ring.arc.point(0.6) }).unwrap();
    assert!(view(&s, id).stamp.is_none(), "the dab goes with the contact");
    assert_eq!(view(&s, id).rings.len(), 2, "the sliders stay open");
    let strength = bubble.rings[0].clone();
    act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Down, position: strength.arc.point(0.25) }).unwrap();
    act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Up, position: strength.arc.point(0.25) }).unwrap();
    let value = ToolbarNumericBinding::BrushStrength.field(&s.state).unwrap().value;
    assert!((value - 0.25).abs() < 0.011, "{value}");
    act(&mut s, BubbleAction::Tap { id, position: [0., TOOL_BUBBLE] }).unwrap();
    assert!(!view(&s, id).selected, "taps beside the center change nothing");
    act(&mut s, BubbleAction::Tap { id, position: [0., 0.] }).unwrap();
    assert!(view(&s, id).selected, "the center starts the color picker");
    let picking = view(&s, id).rings;
    assert_eq!(picking.iter().map(|r| (r.setting, r.enabled)).collect::<Vec<_>>(), [("flow", false), ("size", false)], "the sliders stay, unavailable while picking");
    assert!(act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Down, position: ring.arc.point(0.5) }).is_err());
    let toggle = s.context_menu(ContextTarget::Bubble { id }).unwrap().sections.into_iter().flatten().find(|i| i.label == "Swap sliders").unwrap();
    assert_eq!(toggle.selected, Some(false));
    s.dispatch(toggle.action.unwrap()).unwrap();
    assert_eq!(view(&s, id).rings[0].setting, "size", "the menu swaps the sliders");
    act(&mut s, BubbleAction::Tap { id, position: [4., -4.] }).unwrap();
    assert!(!view(&s, id).selected, "and stops it");
}

#[test]
fn only_the_drawn_parts_of_a_bubble_take_input() {
    let mut s = desk();
    let around = |radius: f32| (0..360).map(move |a| (a as f32).to_radians()).map(move |a| [radius * a.cos(), radius * a.sin()]);
    let id = take_ring(&mut s, [400., 450.]);
    let ring = view(&s, id);
    let radius = ring.diameter * 0.5;
    assert_eq!(ring.hit([0.; 2]), Some(BubbleHit::Wheel));
    assert!(around(radius - 2.).all(|p| ring.hit(p) == Some(BubbleHit::Face)), "the plate's rim moves the ring");
    assert_eq!(ring.hit([radius * 0.75, radius * 0.75]), None, "the corners around the plate are canvas");
    let shown: Vec<_> = ring.beads.iter().filter(|b| b.placement.alpha > 0.6).collect();
    assert!(!shown.is_empty() && shown.iter().all(|b| ring.hit(b.placement.center) == Some(BubbleHit::Bead)));
    let clear = |p: [f32; 2]| shown.iter().all(|b| (p[0] - b.placement.center[0]).hypot(p[1] - b.placement.center[1]) > b.placement.size * 0.5 + 2.);
    assert!(around(radius + 3.).filter(|p| clear(*p)).all(|p| ring.hit(p).is_none()), "between the plate and its beads is canvas");
    let id = take_control(&mut s, ToolbarControl::ColorPicker, [900., 450.]);
    let controls = view(&s, id);
    assert_eq!(controls.hit([0.; 2]), Some(BubbleHit::Face));
    assert_eq!(controls.hit([TOOL_BUBBLE * 0.5 + 4., 0.]), None, "between the center and the sliders is canvas");
    assert_eq!(controls.hit([0., -HALO_RINGS[0]]), None, "so is the gap between the sliders");
    assert!(controls.rings.iter().all(|r| controls.hit(r.arc.point(0.5)) == Some(BubbleHit::Ring) && controls.hit(r.arc.icon_point()) == Some(BubbleHit::Ring)));
    let id = take_control(&mut s, ToolbarControl::Command { command: CommandId::Undo }, [600., 200.]);
    let history = view(&s, id);
    let [w, h] = HISTORY_BUBBLE;
    assert_eq!(history.hit([-w * 0.5 + 2., -h * 0.5 + 2.]), None, "the capsule's corners are canvas");
    assert_eq!((history.hit([-20., 0.]), history.hit([20., 0.])), (Some(BubbleHit::Half(0)), Some(BubbleHit::Half(1))));
    assert_eq!(history.hit([0., h * 0.5 - 2.]), Some(BubbleHit::Face), "between the two button circles the capsule still moves the bubble");
    let id = take_control(&mut s, ToolbarControl::Panel { panel: Panel::Layers }, [600., 600.]);
    let layers = view(&s, id);
    let card = layers.card.as_ref().unwrap().size;
    assert_eq!(layers.hit([card[0] * 0.5 - 2., card[1] * 0.5 - 2.]), Some(BubbleHit::Card));
    assert_eq!(layers.hit([card[0] * 0.5 + 2., 0.]), None);
}

#[test]
fn undo_and_redo_share_one_bubble() {
    let mut s = desk();
    let id = take_control(&mut s, ToolbarControl::Command { command: CommandId::Redo }, FAR);
    let bubble = view(&s, id);
    assert_eq!((bubble.kind, bubble.face), (BubbleKind::History, HISTORY_BUBBLE));
    assert_eq!(bubble.buttons.iter().map(|b| b.enabled).collect::<Vec<_>>(), [false, false]);
    let again = take_control(&mut s, ToolbarControl::Command { command: CommandId::Undo }, [300., 300.]);
    assert_eq!((again, s.bubbles().len()), (id, 1), "taking undo moves the same bubble");
    let layers = s.state.layers.len();
    s.dispatch(UiAction::Layer { action: LayerAction::New { group: false, clipped: false } }).unwrap();
    assert_eq!(view(&s, id).buttons.iter().map(|b| b.enabled).collect::<Vec<_>>(), [true, false]);
    act(&mut s, BubbleAction::Tap { id, position: [12., 0.] }).unwrap();
    assert_eq!(s.state.layers.len(), layers + 1, "a disabled redo does nothing");
    act(&mut s, BubbleAction::Tap { id, position: [-12., 0.] }).unwrap();
    assert_eq!(s.state.layers.len(), layers, "the left half undoes");
    assert_eq!(view(&s, id).buttons.iter().map(|b| b.enabled).collect::<Vec<_>>(), [false, true]);
    act(&mut s, BubbleAction::Tap { id, position: [12., 0.] }).unwrap();
    assert_eq!(s.state.layers.len(), layers + 1, "the right half redoes");
}

#[test]
fn the_hold_menu_chooses_ring_settings_up_to_six_with_undo() {
    let mut s = desk();
    let pencil = DefaultBrushPreset::PointyPencil as u32;
    let id = take(&mut s, brush(DefaultBrushPreset::PointyPencil), FAR);
    let menu = s.context_menu(ContextTarget::Bubble { id }).unwrap();
    let rows = |menu: &ContextMenu| menu.sections[0][1..].to_vec();
    assert!(menu.sections[0][0].is_heading() && menu.sections[0][0].label == "Pointy Pencil", "the brush's name heads its menu");
    assert!(rows(&menu).iter().all(|r| r.selected.is_some() && r.sections.is_empty()), "its ring settings are right in the menu");
    assert_eq!(menu.sections.last().unwrap()[0].label, "Put back");
    let all = rows(&menu);
    assert!(all.len() > MAX_RING_SETTINGS);
    let shown = all.iter().filter(|r| r.selected == Some(true)).count();
    for row in all.iter().filter(|r| r.selected == Some(false)) {
        let menu = s.context_menu(ContextTarget::Bubble { id }).unwrap();
        let current = rows(&menu).into_iter().find(|r| r.action == row.action).unwrap();
        let count = rows(&menu).iter().filter(|r| r.selected == Some(true)).count();
        assert_eq!(current.enabled, count < MAX_RING_SETTINGS);
        if current.enabled {
            s.dispatch(current.action.unwrap()).unwrap();
        } else {
            assert!(s.dispatch(current.action.unwrap()).is_err());
        }
    }
    assert_eq!(s.state.workspace.layout.bubbles.ring(pencil).unwrap().len(), MAX_RING_SETTINGS);
    assert_eq!(view(&s, id).rings.len(), MAX_RING_SETTINGS);
    assert_eq!(view(&s, id).halo, Some(halo_disc(MAX_RING_SETTINGS)));
    for _ in shown..MAX_RING_SETTINGS {
        invoke(&mut s, CommandId::UndoWorkspace);
    }
    assert_eq!(s.state.workspace.layout.bubbles.ring(pencil), None);
    let pastel = take(&mut s, BubbleGrab::Variant { variant: ToolVariant::BrushGroup { group: ToolGroup::Pastel } }, [300., 300.]);
    let dial = take_control(&mut s, ToolbarControl::Command { command: CommandId::DrawingBrush }, [500., 650.]);
    let heading = |s: &UiSession<Recorder>, id: u32| s.context_menu(ContextTarget::Bubble { id }).unwrap().sections[0][0].label.clone();
    let current = tools::brush_label_localized(s.state.brush.preset, s.localization()).unwrap().to_string();
    assert_eq!((heading(&s, pastel), heading(&s, dial)), (current.clone(), current), "a medium or dial names its current tool, not itself");
}

#[test]
fn halos_stay_until_tapped_or_the_brush_changes_while_choosers_close_on_contacts_elsewhere() {
    let mut s = desk();
    let id = take(&mut s, brush(DefaultBrushPreset::PointyPencil), FAR);
    let dial = take_control(&mut s, ToolbarControl::ToolSlot { slot: ToolSlotId::Marquee }, [300., 300.]);
    tap(&mut s, id);
    chrome(&mut s, ChromeEvent::Contact { position: [100., 700.], canvas: true }, ChromeFacts::default());
    assert!(view(&s, id).open, "the halo stays");
    s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::GPen as u32 }).unwrap();
    assert!(!view(&s, id).open, "another brush closes it");
    s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::PointyPencil as u32 }).unwrap();
    assert!(!view(&s, id).open, "and it stays closed");
    tap(&mut s, dial);
    tap(&mut s, dial);
    let item = view(&s, dial).chooser[0].center;
    chrome(&mut s, ChromeEvent::Contact { position: [300. + item[0], 300. + item[1]], canvas: false }, ChromeFacts::default());
    assert!(!view(&s, dial).chooser.is_empty(), "contacts on the ring keep it");
    let reply = chrome(&mut s, ChromeEvent::Contact { position: [100., 700.], canvas: true }, ChromeFacts::default());
    assert!(view(&s, dial).chooser.is_empty());
    assert_ne!(reply.change.regions & regions::BUBBLES, 0);
}

#[test]
fn painting_fades_nearby_bubbles_and_closes_choosers_but_not_the_halo() {
    let mut s = desk();
    let pencil = take(&mut s, brush(DefaultBrushPreset::PointyPencil), [300., 300.]);
    let dial = take_control(&mut s, ToolbarControl::ToolSlot { slot: ToolSlotId::Marquee }, [900., 300.]);
    let layers = take_control(&mut s, ToolbarControl::Panel { panel: Panel::Layers }, [900., 700.]);
    tap(&mut s, pencil);
    tap(&mut s, layers);
    assert!(view(&s, layers).card.is_some() && view(&s, pencil).open, "opening a card keeps the halo");
    stroke(&mut s, 1, PenPhase::Down, [300., 380.]);
    assert!(view(&s, pencil).open, "the halo stays while painting");
    assert!(view(&s, layers).card.is_none(), "painting folds the card");
    let (down, faded) = s.bubble_fade();
    assert!(down && faded == [pencil], "only the bubble near the stroke fades: {faded:?}");
    stroke(&mut s, 1, PenPhase::Move, [900., 360.]);
    assert_eq!(s.bubble_fade().1, [pencil, dial]);
    stroke(&mut s, 1, PenPhase::Up, [900., 360.]);
    let (down, faded) = s.bubble_fade();
    assert!(!down && faded.len() == 2, "hosts restore faded bubbles after the stroke ends");
    stroke(&mut s, 2, PenPhase::Down, [600., 800.]);
    assert!(s.bubble_fade().1.is_empty(), "a new stroke starts fresh");
    stroke(&mut s, 2, PenPhase::Up, [600., 800.]);
}

#[test]
fn the_layers_card_unfolds_flips_to_properties_and_folds() {
    let mut s = desk();
    let color = take_control(&mut s, ToolbarControl::Color, [300., 500.]);
    let (panel, tile) = toolbar_tile(&mut s, ToolbarControl::Panel { panel: Panel::Layers });
    contact(&mut s, BubbleGrab::Tile { panel, tile }, ContactPhase::Down, [800., 500.]).unwrap();
    let layers = s.state.workspace.layout.bubbles.bubbles.last().unwrap().id;
    assert!(view(&s, layers).card.is_some(), "Layers opens as soon as it leaves its toolbar");
    contact(&mut s, BubbleGrab::Tile { panel, tile }, ContactPhase::Up, [800., 500.]).unwrap();
    let menu = |s: &UiSession<Recorder>| s.context_menu(ContextTarget::Bubble { id: layers }).unwrap();
    let item = |s: &UiSession<Recorder>, label: &str| menu(s).sections.into_iter().flatten().find(|i| i.label == label).and_then(|i| i.action);
    assert!(view(&s, layers).card.is_some(), "Layers comes out open");
    act(&mut s, BubbleAction::Tap { id: layers, position: [0., 40.] }).unwrap();
    assert!(view(&s, layers).card.is_some(), "a tap inside the open card keeps it");
    s.dispatch(item(&s, "Close").unwrap()).unwrap();
    let folded = view(&s, layers);
    assert!(folded.card.is_none() && item(&s, "Close").is_none(), "Close folds it");
    assert_eq!(folded.layers, s.state.layers.len());
    assert_eq!(folded.layer, s.state.layers.iter().find(|l| l.selected).map(|l| l.id), "the folded card shows the current layer");
    tap(&mut s, layers);
    let card = view(&s, layers).card.unwrap();
    assert_eq!(card, BubbleCard { size: layers_card(VIEW, s.state.layers.len(), None, false), full: false, properties: false, disclosure: "Properties".into() });
    act(&mut s, BubbleAction::Properties { id: layers }).unwrap();
    assert!(view(&s, layers).card.unwrap().properties);
    assert_ne!(view(&s, layers).card.unwrap().disclosure, "Properties", "the footer then goes back to all layers");
    act(&mut s, BubbleAction::Properties { id: layers }).unwrap();
    assert!(!view(&s, layers).card.unwrap().properties, "All layers goes back");
    chrome(&mut s, ChromeEvent::Contact { position: [800., 500. + card.size[1] * 0.5 - 10.], canvas: false }, ChromeFacts::default());
    assert!(view(&s, layers).card.is_some(), "contacts on the card keep it");
    chrome(&mut s, ChromeEvent::Contact { position: [100., 120.], canvas: true }, ChromeFacts::default());
    assert!(view(&s, layers).card.is_none(), "a contact elsewhere folds it");
    assert!(act(&mut s, BubbleAction::Properties { id: layers }).is_err());
    s.dispatch(item(&s, "Full").unwrap()).unwrap();
    s.dispatch(item(&s, "Fold when not in use").unwrap()).unwrap();
    assert!(view(&s, layers).options.full && !view(&s, layers).options.fold);
    tap(&mut s, layers);
    assert_eq!(view(&s, layers).card.unwrap().size, layers_card(VIEW, s.state.layers.len(), None, false), "Full keeps the drawer's size");
    act(&mut s, BubbleAction::MeasureCard { id: layers, heights: [333., 444.] }).unwrap();
    assert_eq!(view(&s, layers).card.unwrap().size[1], 333., "the host's measurement sizes the card");
    assert!(act(&mut s, BubbleAction::MeasureCard { id: layers, heights: [f32::NAN, 1.] }).is_err());
    chrome(&mut s, ChromeEvent::Contact { position: [100., 120.], canvas: true }, ChromeFacts::default());
    assert!(view(&s, layers).card.is_some(), "a card that doesn't fold stays open");
    s.dispatch(item(&s, "Close").unwrap()).unwrap();
    assert!(view(&s, layers).card.is_none());
    let menu = s.context_menu(ContextTarget::Bubble { id: color }).unwrap();
    assert!(menu.sections[0][0].is_heading());
    assert_eq!(menu.sections[0][1..].iter().map(|i| i.enabled).collect::<Vec<_>>(), [false; 2], "nothing to undo or redo yet");
    assert_eq!(menu.sections[1].iter().map(|i| i.selected).collect::<Vec<_>>(), [Some(false); 3], "a new ring is the circle alone");
    s.dispatch(menu.sections[2][2].action.clone().unwrap()).unwrap();
    assert_eq!(view(&s, color).diameter, COLOR_RING_SIZES[2]);
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(view(&s, color).diameter, COLOR_RING_SIZES[1]);
}

#[test]
fn an_open_card_pushes_neighbours_which_return_when_it_folds() {
    let mut s = desk();
    let layers = take_control(&mut s, ToolbarControl::Panel { panel: Panel::Layers }, [600., 450.]);
    let pencil = take(&mut s, brush(DefaultBrushPreset::PointyPencil), [700., 450.]);
    let settle = |s: &mut UiSession<Recorder>, from: u64| {
        let mut now = from;
        while s.step_bubbles(now) && now < from + 10_000_000_000 {
            now += 16_666_667;
        }
        now
    };
    let now = settle(&mut s, 0);
    tap(&mut s, layers);
    let now = settle(&mut s, now);
    let card = layers_card(VIEW, s.state.layers.len(), None, false);
    let place = |s: &mut UiSession<Recorder>, id: u32| s.bubble_placements().into_iter().find(|p| p.id == id).unwrap().center;
    let pushed = place(&mut s, pencil);
    let card_center = place(&mut s, layers);
    assert!(pushed[0] - TOOL_BUBBLE * 0.5 >= card_center[0] + card[0] * 0.5 + WORKSPACE_SPACING - 0.5, "{pushed:?}");
    tap(&mut s, layers);
    settle(&mut s, now);
    assert_eq!(place(&mut s, pencil), [700., 450.]);
    assert_eq!(saved(&s, pencil), [700., 450.], "growth never moves saved places");
}

#[test]
fn an_open_chooser_bumps_neighbours_off_its_items_until_it_closes() {
    let mut s = desk();
    let id = take_control(&mut s, ToolbarControl::Command { command: CommandId::DrawingBrush }, [500., 450.]);
    let neighbour = take(&mut s, brush(DefaultBrushPreset::Charcoal), [500., 450. - CHOOSER_RING]);
    let settle = |s: &mut UiSession<Recorder>, from: u64| {
        let mut now = from;
        while s.step_bubbles(now) && now < from + 10_000_000_000 {
            now += 16_666_667;
        }
        now
    };
    let now = settle(&mut s, 0);
    open_ring(&mut s, id);
    let now = settle(&mut s, now);
    let placements = s.bubble_placements();
    let [dial, other] = [id, neighbour].map(|n| placements.iter().find(|p| p.id == n).unwrap().center);
    for item in view(&s, id).chooser {
        let [w, h] = item.size();
        let at = [dial[0] + item.center[0], dial[1] + item.center[1]];
        let gap = if item.kind == ChooserKind::Brush {
            [((at[0] - other[0]).abs() - w * 0.5).max(0.), ((at[1] - other[1]).abs() - h * 0.5).max(0.)]
        } else {
            [((at[0] - other[0]).hypot(at[1] - other[1]) - w * 0.5).max(0.), 0.]
        };
        assert!(gap[0].hypot(gap[1]) >= TOOL_BUBBLE * 0.5 + WORKSPACE_SPACING - 0.5, "{} at {at:?} overlaps the neighbour at {other:?} (dial {dial:?})", item.label);
    }
    tap(&mut s, id);
    settle(&mut s, now);
    assert_eq!(s.bubble_placements().into_iter().find(|p| p.id == neighbour).unwrap().center, [500., 450. - CHOOSER_RING]);
}

fn overlaps(s: &mut UiSession<Recorder>) -> Vec<(BubbleKind, BubbleKind, [f32; 2])> {
    let views = s.bubbles();
    let placements = s.bubble_placements();
    let placed: Vec<_> = views.iter().map(|v| (v, placements.iter().find(|p| p.id == v.id).unwrap().center)).collect();
    let mut found = Vec::new();
    for (i, (a, ca)) in placed.iter().enumerate() {
        for (b, cb) in &placed[i + 1..] {
            let point = (0..VIEW[1] as u32 / 3).flat_map(|y| (0..VIEW[0] as u32 / 3).map(move |x| [x as f32 * 3., y as f32 * 3.])).find(|p| {
                a.hit([p[0] - ca[0], p[1] - ca[1]]).is_some() && b.hit([p[0] - cb[0], p[1] - cb[1]]).is_some()
            });
            found.extend(point.map(|p| (a.kind, b.kind, p)));
        }
    }
    found
}

fn settle(s: &mut UiSession<Recorder>, from: u64) -> u64 {
    let mut now = from;
    while s.step_bubbles(now) && now < from + 10_000_000_000 {
        now += 16_666_667;
    }
    now
}

#[test]
fn cancelling_a_bubble_drag_restores_settled_positions_after_motion_and_collisions() {
    for frames in [1, 4, 16] {
        let mut s = desk();
        take_control(&mut s, ToolbarControl::Command { command: CommandId::DrawingBrush }, [760., 340.]);
        let id = take_control(&mut s, ToolbarControl::Panel { panel: Panel::Layers }, [800., 420.]);
        let mut now = settle(&mut s, 0);
        assert!(!s.step_bubbles(now));
        let authored = s.state.workspace.clone();
        let before = s.bubble_placements();
        let origin = before.iter().find(|p| p.id == id).unwrap().center;
        let grab = BubbleGrab::Bubble { id };
        contact(&mut s, grab, ContactPhase::Down, origin).unwrap();
        contact(&mut s, grab, ContactPhase::Move, [120., 30.]).unwrap();
        for _ in 0..frames {
            now += 16_666_667;
            s.step_bubbles(now);
        }
        contact(&mut s, grab, ContactPhase::Cancel, [120., 30.]).unwrap();
        settle(&mut s, now);
        assert_eq!(s.state.workspace, authored, "cancellation preserves authored layout");
        let after = s.bubble_placements();
        for original in before {
            let restored = after.iter().find(|p| p.id == original.id).unwrap();
            assert!(!restored.lifted && !restored.homing);
            assert!((restored.center[0] - original.center[0]).hypot(restored.center[1] - original.center[1]) < 0.1,
                "{frames} frames, bubble {}: {:?} -> {:?}", original.id, original.center, restored.center);
        }
    }
}

#[test]
fn opening_the_palette_pill_keeps_its_whole_surface_inside_the_work_area() {
    for right in [false,true] {
        let mut s=desk();
        let work=s.layout(VIEW).work_area;
        let at=[if right {work.x+work.width-60.} else {work.x+work.width*0.5},work.y+work.height-60.];
        let id=take_ring(&mut s,at);
        let now=settle(&mut s,0);
        let saved=s.state.workspace.layout.bubbles.get(id).unwrap().clone();
        tap_bead(&mut s,id,Bead::Palettes);
        settle(&mut s,now);
        let view=view(&s,id);
        assert!(view.pill.is_some());
        let center=s.bubble_placements().into_iter().find(|p|p.id==id).unwrap().center;
        let pill=palette_pill(view.diameter);
        let bounds=[center[0]+pill.x,center[1]+pill.y,center[0]+pill.x+pill.width,center[1]+pill.y+pill.height];
        assert!(bounds[0]>=work.x+8.-0.5 && bounds[1]>=work.y+8.-0.5
            && bounds[2]<=work.x+work.width-8.+0.5 && bounds[3]<=work.y+work.height-8.+0.5,
            "The open palette pill must fit the inset work area: right={right}, pill={bounds:?}, work={work:?}, center={center:?}");
        assert_eq!(s.state.workspace.layout.bubbles.get(id).unwrap(),&saved,"Opening a pill moves the desk projection, not its authored place");
    }
}

#[test]
fn dragging_an_open_palette_closes_it_and_reopening_at_the_edge_stays_inside() {
    for viewport in [VIEW,[2200./1.75,1440./1.75]] {
    let mut s=desk();
    s.set_viewport(viewport,[2200,1440]).unwrap();
    let work=s.layout(viewport).work_area;
    let id=take_ring(&mut s,[600.,350.]);
    act(&mut s,BubbleAction::Resize {id,scale:10.}).unwrap();
    let mut now=settle(&mut s,0);
    tap_bead(&mut s,id,Bead::Palettes);
    now=settle(&mut s,now);
    assert!(view(&s,id).pill.is_some());
    let before=s.state.workspace.clone();
    let from=s.bubble_placements().into_iter().find(|p|p.id==id).unwrap().center;
    let grab=BubbleGrab::Bubble {id};
    let drag=|s:&mut UiSession<Recorder>,phase,position| act(s,BubbleAction::Drag {grab,phase,position,viewport}).unwrap();
    drag(&mut s,ContactPhase::Down,from);
    assert!(view(&s,id).pill.is_none(),"Dragging closes the palette; its always-published pill rectangle is not a visible surface");
    let edge=viewport.map(|v|v-4.);
    drag(&mut s,ContactPhase::Move,edge);
    drag(&mut s,ContactPhase::Up,edge);
    now=settle(&mut s,now);
    let moved=s.state.workspace.clone();
    assert_ne!(moved,before,"The completed drag authors the new resting place");
    invoke(&mut s,CommandId::UndoWorkspace);
    assert_eq!(s.state.workspace,before);
    invoke(&mut s,CommandId::RedoWorkspace);
    assert_eq!(s.state.workspace,moved);
    now=settle(&mut s,now);
    tap_bead(&mut s,id,Bead::Palettes);
    settle(&mut s,now);
    let view=view(&s,id);
    assert!(view.pill.is_some());
    let center=s.bubble_placements().into_iter().find(|p|p.id==id).unwrap().center;
    let pill=palette_pill(view.diameter);
    let bounds=[center[0]+pill.x,center[1]+pill.y,center[0]+pill.x+pill.width,center[1]+pill.y+pill.height];
    assert!(bounds[0]>=work.x+8.-0.5 && bounds[1]>=work.y+8.-0.5
        && bounds[2]<=work.x+work.width-8.+0.5 && bounds[3]<=work.y+work.height-8.+0.5,
        "Reopening the largest wheel's palette after an edge drop must fit: pill={bounds:?}, work={work:?}");
    assert_eq!(s.state.workspace,moved,"Reopening changes only the desk projection");
    }
}

#[test]
fn opening_the_palette_pill_pushes_a_nearby_bubble_clear() {
    let mut s=desk();
    let id=take_ring(&mut s,[600.,350.]);
    let now=settle(&mut s,0);
    let center=s.bubble_placements().into_iter().find(|p|p.id==id).unwrap().center;
    let pill=palette_pill(view(&s,id).diameter);
    let other=take_control(&mut s,ToolbarControl::Command {command:CommandId::Undo},[center[0],center[1]+pill.y+pill.height*0.5]);
    let now=settle(&mut s,now);
    assert_eq!(overlaps(&mut s),[],"Closed parts start apart");
    tap_bead(&mut s,id,Bead::Palettes);
    settle(&mut s,now);
    assert!(view(&s,id).pill.is_some());
    assert_eq!(overlaps(&mut s),[],"The pill pushes the neighboring history capsule clear");
    let places=s.bubble_placements();
    let a=places.iter().find(|p|p.id==id).unwrap().center;
    let b=places.iter().find(|p|p.id==other).unwrap().center;
    let pill=palette_pill(view(&s,id).diameter);
    let gap=[(b[0]-(a[0]+pill.x+pill.width*0.5)).abs()-pill.width*0.5,
        (b[1]-(a[1]+pill.y+pill.height*0.5)).abs()-pill.height*0.5];
    assert!(gap[0]>0. || gap[1]>0.,"The neighbor center cannot remain underneath the pill");
}

#[test]
fn open_rings_halos_and_cards_push_every_kind_of_bubble_clear() {
    let mut s = desk();
    let center = [600., 450.];
    let dial = take_control(&mut s, ToolbarControl::Command { command: CommandId::DrawingBrush }, center);
    let spots = [[600., 450. - CHOOSER_RING], [600. + CHOOSER_RING, 450.], [600., 450. + CHOOSER_RING], [600. - CHOOSER_RING, 450.]];
    take_control(&mut s, ToolbarControl::Command { command: CommandId::Undo }, spots[0]);
    take_control(&mut s, ToolbarControl::Panel { panel: Panel::Layers }, spots[1]);
    let layers = s.bubbles().iter().find(|v| v.kind == BubbleKind::Layers).unwrap().id;
    act(&mut s, BubbleAction::Close { id: layers }).unwrap();
    take(&mut s, brush(DefaultBrushPreset::Charcoal), spots[2]);
    take_control(&mut s, ToolbarControl::ColorPicker, spots[3]);
    s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::PointyPencil as u32 }).unwrap();
    let mut now = settle(&mut s, 0);
    assert_eq!(overlaps(&mut s), [], "closed bubbles rest apart");
    tap(&mut s, dial);
    now = settle(&mut s, now);
    assert!(view(&s, dial).halo.is_some());
    assert_eq!(overlaps(&mut s), [], "the dial's halo pushes its neighbours");
    tap(&mut s, dial);
    now = settle(&mut s, now);
    assert!(!view(&s, dial).chooser.is_empty());
    assert_eq!(overlaps(&mut s), [], "the dial's ring pushes its neighbours");
    tap(&mut s, dial);
    settle(&mut s, now);
    assert_eq!(overlaps(&mut s), [], "and they rest apart again once it closes");
}

#[test]
fn an_open_ring_near_a_wall_still_clears_its_neighbours() {
    let work = desk().layout(VIEW).work_area;
    for (corner, offsets) in [
        ([work.x + 60., work.y + work.height - 60.], [[CHOOSER_RING, 0.], [0., -CHOOSER_RING], [CHOOSER_RING * 0.7, -CHOOSER_RING * 0.7]]),
        ([work.x + work.width * 0.5, work.y + work.height - 60.], [[CHOOSER_RING, 0.], [-CHOOSER_RING, 0.], [0., -CHOOSER_RING]]),
    ] {
        let neighbours = [
            ToolbarControl::Command { command: CommandId::Undo },
            ToolbarControl::Panel { panel: Panel::Layers },
            ToolbarControl::ColorPicker,
            ToolbarControl::Brush { id: DefaultBrushPreset::Charcoal as u32 },
            ToolbarControl::Color,
        ];
        for (offset, control) in offsets.into_iter().flat_map(|o| neighbours.map(|c| (o, c))) {
            let mut s = desk();
            let dial = take_control(&mut s, ToolbarControl::Command { command: CommandId::DrawingBrush }, corner);
            let other = take_control(&mut s, control, [corner[0] + offset[0], (corner[1] + offset[1]).min(work.y + work.height - 36.)]);
            act(&mut s, BubbleAction::Close { id: other }).unwrap();
            s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::PointyPencil as u32 }).unwrap();
            let mut now = settle(&mut s, 0);
            for open in ["halo", "ring"] {
                tap(&mut s, dial);
                now = settle(&mut s, now);
                assert_eq!(overlaps(&mut s), [], "{open} at {corner:?} with {control:?} at {offset:?}");
            }
        }
    }
}

#[test]
fn bubbles_pushed_during_a_long_drag_keep_their_new_place() {
    let mut s = desk();
    let mover = take(&mut s, brush(DefaultBrushPreset::PointyPencil), [400., 450.]);
    let pushed = take(&mut s, brush(DefaultBrushPreset::Charcoal), [470., 450.]);
    let grab = BubbleGrab::Bubble { id: mover };
    contact(&mut s, grab, ContactPhase::Down, [400., 450.]).unwrap();
    let mut now = 0;
    for step in 0..=20 {
        contact(&mut s, grab, ContactPhase::Move, [400. + step as f32 * 3., 450.]).unwrap();
        now += 16_666_667;
        s.step_bubbles(now);
    }
    contact(&mut s, grab, ContactPhase::Move, [300., 450.]).unwrap();
    for _ in 0..240 {
        now += 16_666_667;
        s.step_bubbles(now);
    }
    let place = |s: &mut UiSession<Recorder>| s.bubble_placements().into_iter().find(|p| p.id == pushed).unwrap().center;
    let held = place(&mut s);
    assert!(held[0] >= 460. + TOOL_BUBBLE + WORKSPACE_SPACING - 0.5, "it stays pushed while the drag is held: {held:?}");
    contact(&mut s, grab, ContactPhase::Up, [300., 450.]).unwrap();
    let saved = saved(&s, pushed);
    assert!((saved[0] - held[0]).abs() < 0.5 && (saved[1] - held[1]).abs() < 0.5, "{saved:?} {held:?}");
    for _ in 0..240 {
        now += 16_666_667;
        s.step_bubbles(now);
    }
    assert!((place(&mut s)[0] - saved[0]).abs() < 0.5);
}

#[test]
fn putting_back_an_open_bubble_closes_its_halo_and_chooser() {
    let mut s = desk();
    let id = take(&mut s, BubbleGrab::Variant { variant: ToolVariant::BrushGroup { group: ToolGroup::Pastel } }, FAR);
    tap(&mut s, id);
    assert!(!view(&s, id).chooser.is_empty());
    act(&mut s, BubbleAction::PutBack { id }).unwrap();
    assert!(s.bubbles().is_empty() && s.bubble_placements().is_empty());
    assert!(act(&mut s, BubbleAction::Tap { id, position: [0.; 2] }).is_err());
    assert!(act(&mut s, BubbleAction::Choose { id, phase: ContactPhase::Up, position: [0., 0.] }).unwrap().regions == 0);
}

#[test]
fn a_smaller_window_keeps_bubbles_inside_and_corner_bubbles_follow_their_corner() {
    let mut s = desk();
    let work = s.layout(VIEW).work_area;
    let corner = [work.x + work.width - 40., work.y + work.height - 40.];
    let id = take(&mut s, brush(DefaultBrushPreset::PointyPencil), corner);
    let middle = take(&mut s, brush(DefaultBrushPreset::Charcoal), [560., 420.]);
    tap(&mut s, middle);
    let saved_before = s.state.workspace.layout.bubbles.clone();
    s.set_viewport([700., 560.], [700, 560]).unwrap();
    let mut now = 0;
    while s.step_bubbles(now) {
        now += 16_666_667;
    }
    let area = s.state.workspace.layout.free_area([700., 560.]);
    let place = |s: &mut UiSession<Recorder>, id: u32| s.bubble_placements().into_iter().find(|p| p.id == id).unwrap().center;
    let at = place(&mut s, id);
    assert_eq!(at, [corner[0] - 500., corner[1] - 340.], "kept the same distance to the bottom-right corner");
    let inside = place(&mut s, middle);
    assert!(inside[0] + TOOL_BUBBLE * 0.5 <= area.x + area.width && inside[1] + TOOL_BUBBLE * 0.5 <= area.y + area.height);
    assert_eq!(s.state.workspace.layout.bubbles, saved_before, "resizing never rewrites saved places");
    s.set_viewport(VIEW, [1200, 900]).unwrap();
    while s.step_bubbles(now) {
        now += 16_666_667;
    }
    assert_eq!(place(&mut s, id), corner);
}

#[test]
fn narrow_windows_open_color_rings_small() {
    let mut s = desk();
    let color = take_control(&mut s, ToolbarControl::Color, [500., 400.]);
    assert_eq!(view(&s, color).diameter, COLOR_RING_SIZES[1]);
    s.set_viewport([560., 800.], [560, 800]).unwrap();
    assert_eq!(view(&s, color).diameter, COLOR_RING_SIZES[0]);
    assert_eq!(view(&s, color).options.size, COLOR_RING_SIZES[1], "the chosen size returns in a wider window");
}

#[test]
fn bubbles_round_trip_with_the_workspace() {
    let mut s = desk();
    let id = take(&mut s, brush(DefaultBrushPreset::PointyPencil), FAR);
    let json = serde_json::to_string(&s.state.workspace).unwrap();
    let restored: WorkspaceState = serde_json::from_str(&json).unwrap();
    restored.layout.validate().unwrap();
    assert_eq!(restored.layout.bubbles.get(id), s.state.workspace.layout.bubbles.get(id));
}

#[test]
fn color_beads_pick_select_and_switch_palettes() {
    let mut s = desk();
    let id = take_ring(&mut s, [500., 380.]);
    record_colors(&mut s, 20);
    let open = view(&s, id);
    for kind in [Bead::Foreground, Bead::Background, Bead::Transparent, Bead::Swatch { index: 0 }, Bead::Recent { index: 0 }, Bead::Palettes] {
        assert!(open.beads.iter().any(|b| b.placement.bead == kind), "{kind:?}");
    }
    let target = s.state.color_library.history[2];
    tap_bead(&mut s, id, Bead::Recent { index: 2 });
    assert_eq!(s.state.colors.definition(), target);
    let (foreground, background) = (s.state.colors.foreground, s.state.colors.background);
    let overlap = |s: &UiSession<Recorder>| {
        let [f, b] = [bead(&view(s, id), Bead::Foreground), bead(&view(s, id), Bead::Background)];
        [(f[0] + b[0]) * 0.5, (f[1] + b[1]) * 0.5]
    };
    let beads_of = |s: &UiSession<Recorder>| view(s, id).beads.iter().map(|b| b.placement).collect::<Vec<_>>();
    assert_eq!(bead_at(&beads_of(&s), overlap(&s)), Some(Bead::Foreground));
    let exposed = {
        let [f, b] = [bead(&view(&s, id), Bead::Foreground), bead(&view(&s, id), Bead::Background)];
        let away = [b[0] - f[0], b[1] - f[1]];
        let length = away[0].hypot(away[1]);
        [b[0] + away[0] / length * 8., b[1] + away[1] / length * 8.]
    };
    beads(&mut s, id, ContactPhase::Down, exposed);
    beads(&mut s, id, ContactPhase::Up, exposed);
    assert_eq!((s.state.colors.foreground, s.state.colors.background), (foreground, background), "nothing swaps");
    assert_eq!(s.state.colors.slot, ColorSlot::Background, "the background is chosen");
    assert_eq!(bead_at(&beads_of(&s), overlap(&s)), Some(Bead::Background), "and comes to the front");
    assert!(view(&s, id).beads.iter().any(|b| b.placement.bead == Bead::Background && b.selected));
    tap_bead(&mut s, id, Bead::Transparent);
    assert_eq!(s.state.colors.slot, ColorSlot::Transparent);
    tap_bead(&mut s, id, Bead::Foreground);
    assert_eq!(s.state.colors.slot, ColorSlot::Foreground);
    let swatch = s.state.color_library.active_palette().swatches[1].color;
    tap_bead(&mut s, id, Bead::Swatch { index: 1 });
    assert_eq!(s.state.colors.definition(), swatch);
    assert!(view(&s, id).pill.is_none());
    tap_bead(&mut s, id, Bead::Palettes);
    let name = view(&s, id).pill.unwrap();
    assert_eq!(name, s.state.color_library.active_palette().name);
    assert!(view(&s, id).beads.iter().any(|b| b.placement.bead == Bead::Palettes && b.selected));
    let palettes = s.state.color_library.palettes.len();
    act(&mut s, BubbleAction::Palette { id, step: 1 }).unwrap();
    if palettes > 1 {
        assert_ne!(view(&s, id).pill.unwrap(), name);
    }
    act(&mut s, BubbleAction::Palette { id, step: -1 }).unwrap();
    assert_eq!(view(&s, id).pill.unwrap(), name);
    let menu = s.context_menu(ContextTarget::BubblePalettes { id }).unwrap();
    assert_eq!(menu.sections[0].len(), palettes + 1);
    assert_eq!(menu.sections[0].iter().filter(|i| i.selected == Some(true)).count(), 1);
    assert_eq!(menu.sections[0].last().unwrap().label, "Edit palettes…");
    tap_bead(&mut s, id, Bead::Swatch { index: 0 });
    assert!(view(&s, id).pill.is_none(), "picking a color closes the name");
    tap_bead(&mut s, id, Bead::Palettes);
    chrome(&mut s, ChromeEvent::Contact { position: [100., 120.], canvas: true }, ChromeFacts::default());
    assert!(view(&s, id).pill.is_none(), "so does a contact elsewhere");
    let options = BubbleOptions { palette: false, ..view(&s, id).options };
    act(&mut s, BubbleAction::SetOptions { id, options }).unwrap();
    assert!(!view(&s, id).beads.iter().any(|b| matches!(b.placement.bead, Bead::Swatch { .. } | Bead::Palettes)));
}

#[test]
fn sliding_along_beads_turns_them_and_settles_on_whole_columns() {
    let mut s = desk();
    let id = take_ring(&mut s, [500., 420.]);
    record_colors(&mut s, 20);
    big_palette(&mut s);
    let before = s.state.colors.definition();
    let start = view(&s, id);
    let second = bead(&start, Bead::Recent { index: 1 });
    let third = bead(&start, Bead::Recent { index: 2 });
    beads(&mut s, id, ContactPhase::Down, third);
    let turned = beads(&mut s, id, ContactPhase::Move, second);
    assert_ne!(turned.regions & regions::BUBBLES, 0);
    let moved = bead(&view(&s, id), Bead::Recent { index: 3 });
    assert!((moved[0] - third[0]).hypot(moved[1] - third[1]) < 1., "beads travel with the finger");
    let halfway = [(second[0] + third[0]) * 0.5, (second[1] + third[1]) * 0.5 - 2.];
    beads(&mut s, id, ContactPhase::Move, halfway);
    beads(&mut s, id, ContactPhase::Up, halfway);
    let settled = view(&s, id);
    assert!(settled.beads.iter().filter(|b| matches!(b.placement.bead, Bead::Recent { .. })).all(|b| b.placement.alpha == 1. || b.placement.alpha <= 0.3 + 1e-3));
    assert_eq!(s.state.colors.definition(), before, "a slide picks nothing");
    let first = bead(&start, Bead::Swatch { index: 0 });
    let further = bead(&start, Bead::Swatch { index: 4 });
    beads(&mut s, id, ContactPhase::Down, further);
    for step in 1..=20 {
        let f = step as f32 / 20.;
        beads(&mut s, id, ContactPhase::Move, [further[0] + (first[0] - further[0]) * f, further[1] + (first[1] - further[1]) * f]);
    }
    beads(&mut s, id, ContactPhase::Up, first);
    let moved = view(&s, id);
    assert!((bead(&moved, Bead::Swatch { index: 5 })[0] - bead(&start, Bead::Swatch { index: 1 })[0]).abs() < 1., "both rows turn together");
    assert_eq!(bead(&moved, Bead::Palettes), bead(&start, Bead::Palettes));
}

#[test]
fn reset_layout_puts_bubbles_back_and_keeps_ring_choices() {
    let mut s = desk();
    let pencil = DefaultBrushPreset::PointyPencil as u32;
    take(&mut s, brush(DefaultBrushPreset::PointyPencil), FAR);
    act(&mut s, BubbleAction::ToggleRingSetting { brush: pencil, setting: "size".into() }).unwrap();
    s.state.workspace.layout.reset_docking(Platform::Gtk).unwrap();
    assert!(s.state.workspace.layout.bubbles.bubbles.is_empty());
    assert!(s.state.workspace.layout.bubbles.ring(pencil).is_some_and(|ring| ring.iter().any(|id| id == "size")));
    assert!(s.bubble_placements().is_empty());
}

#[test]
fn a_bubble_that_returns_starts_with_fresh_scrolling_and_no_open_parts() {
    let mut s = desk();
    let (panel, tile) = toolbar_tile(&mut s, ToolbarControl::Color);
    let before = s.state.workspace.clone();
    big_palette(&mut s);
    let everything = |s: &mut UiSession<Recorder>, id: u32| {
        let options = BubbleOptions { recent: true, palette: true, pair: true, ..view(s, id).options };
        act(s, BubbleAction::SetOptions { id, options }).unwrap();
    };
    let id = take(&mut s, BubbleGrab::Tile { panel, tile }, [500., 380.]);
    everything(&mut s, id);
    let start = view(&s, id);
    let [from, to] = [bead(&start, Bead::Swatch { index: 4 }), bead(&start, Bead::Swatch { index: 0 })];
    beads(&mut s, id, ContactPhase::Down, from);
    beads(&mut s, id, ContactPhase::Move, to);
    beads(&mut s, id, ContactPhase::Up, to);
    tap_bead(&mut s, id, Bead::Palettes);
    assert!(view(&s, id).pill.is_some());
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(before) }).unwrap();
    s.bubble_placements();
    let again = take(&mut s, BubbleGrab::Tile { panel, tile }, [500., 380.]);
    assert_eq!(again, id, "a restored layout reuses the identity");
    everything(&mut s, again);
    let fresh = view(&s, again);
    assert!(fresh.pill.is_none());
    assert_eq!(bead(&fresh, Bead::Swatch { index: 0 }), bead(&start, Bead::Swatch { index: 0 }));
}

#[test]
fn a_color_ring_starts_as_the_medium_circle_and_comes_back_as_it_was_left() {
    let mut s = desk();
    let (panel, tile) = toolbar_tile(&mut s, ToolbarControl::Color);
    let id = take(&mut s, BubbleGrab::Tile { panel, tile }, [500., 380.]);
    let first = view(&s, id);
    assert_eq!(first.diameter, COLOR_RING_SIZES[1]);
    assert!(first.beads.is_empty(), "the circle alone");
    let options = BubbleOptions { size: COLOR_RING_SIZES[2], palette: true, ..first.options };
    act(&mut s, BubbleAction::SetOptions { id, options }).unwrap();
    act(&mut s, BubbleAction::PutBack { id }).unwrap();
    s.set_viewport([1400., 1000.], [1400, 1000]).unwrap();
    let again = take(&mut s, BubbleGrab::Tile { panel, tile }, [500., 380.]);
    assert_eq!(view(&s, again).options, options, "its items and size are remembered");
    act(&mut s, BubbleAction::PutBackAll).unwrap();
    let json = serde_json::to_string(&s.state.workspace).unwrap();
    let restored: WorkspaceState = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.layout.bubbles.kept, s.state.workspace.layout.bubbles.kept, "and saved with the workspace");
    let layers = take_control(&mut s, ToolbarControl::Panel { panel: Panel::Layers }, [800., 500.]);
    assert_eq!(view(&s, layers).options, BubbleOptions::default());
}

#[test]
fn a_drag_out_of_the_wheel_grabs_the_nearest_rim_and_catches_up_smoothly() {
    let mut s = desk();
    let id = take_control(&mut s, ToolbarControl::Color, [500., 400.]);
    let radius = view(&s, id).diameter * 0.5;
    let mut now = 0;
    while s.step_bubbles(now) && now < 2_000_000_000 {
        now += 16_666_667;
    }
    let grab = BubbleGrab::Rim { id };
    let finger = [500. + radius * 2. + 1., 400.];
    contact(&mut s, grab, ContactPhase::Down, finger).unwrap();
    let place = |s: &mut UiSession<Recorder>| s.bubble_placements().into_iter().find(|p| p.id == id).unwrap().center;
    assert_eq!(place(&mut s), [500., 400.], "nothing jumps when the drag starts");
    let mut path = Vec::new();
    for _ in 0..30 {
        now += 8_333_333;
        s.step_bubbles(now);
        path.push(place(&mut s)[0]);
    }
    assert!(path.windows(2).all(|w| w[1] >= w[0] && w[1] - w[0] < 20.), "it glides toward the finger: {path:?}");
    let caught = place(&mut s);
    assert!((caught[0] + radius - finger[0]).abs() < 1., "the nearest rim point ends under the finger: {caught:?}");
    contact(&mut s, grab, ContactPhase::Move, [finger[0] + 40., 400.]).unwrap();
    now += 8_333_333;
    s.step_bubbles(now);
    assert!((place(&mut s)[0] - (caught[0] + 40.)).abs() < 1., "then it follows directly");
    contact(&mut s, grab, ContactPhase::Up, [finger[0] + 40., 400.]).unwrap();
    assert!(saved(&s, id)[0] >= caught[0] + 39., "and keeps the place it was let go, gliding on");
}

#[test]
fn toolbars_can_stay_in_zen_mode() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        s.set_viewport(VIEW, [1200, 900]).unwrap();
        let bar = s.state.workspace.layout.panels.iter().find(|p| p.id.kind() == PanelKind::Tiles && s.state.workspace.layout.panel_group(p.id).is_some()).unwrap().id;
        let group = s.state.workspace.layout.panel_group(bar).unwrap();
        let item = |s: &UiSession<Recorder>| {
            s.context_menu(ContextTarget::Ribbon { panel: bar }).unwrap().sections.into_iter().flatten().find(|i| i.label == "Show in Zen mode").unwrap()
        };
        assert_eq!(item(&s).selected, Some(false), "toolbars hide in Zen by default");
        s.dispatch(item(&s).action.unwrap()).unwrap();
        assert!(s.state.workspace.layout.shows_in_zen(group));
        assert!(find_group(&s, VIEW, |g| g.id == group).zen_kept);
        assert_eq!(item(&s).selected, Some(true));
        invoke(&mut s, CommandId::ZenMode);
        s.interaction.hidden = true;
        let bounds = find_group(&s, VIEW, |g| g.id == group).bounds;
        let inside = [bounds.x + bounds.width * 0.5, bounds.y + bounds.height * 0.5];
        let id = take(&mut s, brush(DefaultBrushPreset::PointyPencil), FAR);
        drag(&mut s, BubbleGrab::Bubble { id }, FAR, inside);
        assert!(s.state.workspace.layout.bubbles.bubbles.is_empty(), "a toolbar shown in Zen takes bubbles back");
        invoke(&mut s, CommandId::UndoWorkspace);
        assert_eq!(s.state.workspace.layout.bubbles.bubbles.len(), 1);
        invoke(&mut s, CommandId::UndoWorkspace);
        assert!(s.state.workspace.layout.shows_in_zen(group));
        invoke(&mut s, CommandId::UndoWorkspace);
        assert!(!s.state.workspace.layout.shows_in_zen(group), "the choice is one layout step");
    }
}

#[test]
fn an_edge_swipe_in_zen_peeks_that_edges_toolbars_until_a_contact_elsewhere() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        s.set_viewport(VIEW, [1200, 900]).unwrap();
        let layout = s.layout(VIEW);
        let group = layout.groups.iter().find(|g| !g.floating && s.state.workspace.layout.group_edge(g.id).is_some()).unwrap().clone();
        let edge = s.state.workspace.layout.group_edge(group.id).unwrap();
        invoke(&mut s, CommandId::ZenMode);
        chrome(&mut s, ChromeEvent::Refresh, ChromeFacts::default());
        assert!(s.interaction.hidden && !s.zen_keeps(group.id));
        let (start, inward) = match edge {
            Edge::Left => ([4., 450.], [1., 0.]),
            Edge::Right => ([VIEW[0] - 4., 450.], [-1., 0.]),
            Edge::Top => ([600., 4.], [0., 1.]),
            Edge::Bottom => ([600., VIEW[1] - 4.], [0., -1.]),
        };
        let at = |d: f32| [start[0] + inward[0] * d, start[1] + inward[1] * d];
        let touch = |s: &mut UiSession<Recorder>, phase, position| {
            s.input(UiInput::Pointer { id: 9, phase, kind: PointerKind::Touch, button: PointerButton::Primary, position, time_ns: 0 }).unwrap()
        };
        touch(&mut s, ContactPhase::Down, start);
        touch(&mut s, ContactPhase::Move, at(20.));
        assert!(!s.zen_keeps(group.id), "a short swipe keeps chrome hidden");
        let peek = touch(&mut s, ContactPhase::Move, at(60.));
        assert_ne!(peek.change.regions & regions::LAYOUT, 0);
        assert!(find_group(&s, VIEW, |g| g.id == group.id).zen_kept);
        assert!(s.zen_keeps(group.id), "swiping in from the edge shows its toolbar");
        touch(&mut s, ContactPhase::Up, at(60.));
        let inside = [group.bounds.x + group.bounds.width * 0.5, group.bounds.y + group.bounds.height * 0.5];
        chrome(&mut s, ChromeEvent::Contact { position: inside, canvas: false }, ChromeFacts::default());
        assert!(s.zen_keeps(group.id), "using the peeked toolbar keeps it");
        let hidden = chrome(&mut s, ChromeEvent::Contact { position: [600., 450.], canvas: true }, ChromeFacts::default());
        assert_ne!(hidden.change.regions & regions::LAYOUT, 0);
        assert!(!find_group(&s, VIEW, |g| g.id == group.id).zen_kept);
        assert!(!s.zen_keeps(group.id), "a contact elsewhere hides it again");
        touch(&mut s, ContactPhase::Down, [600., 450.]);
        touch(&mut s, ContactPhase::Move, [700., 450.]);
        assert!(!s.zen_keeps(group.id), "swipes away from the edges do nothing");
    }
}

#[test]
fn bubbles_open_in_zen_without_bringing_the_docks_back() {
    let mut s = desk();
    let layers = take_control(&mut s, ToolbarControl::Panel { panel: Panel::Layers }, FAR);
    invoke(&mut s, CommandId::ZenMode);
    chrome(&mut s, ChromeEvent::Refresh, ChromeFacts::default());
    assert!(s.interaction.hidden);
    tap(&mut s, layers);
    let reply = chrome(&mut s, ChromeEvent::Refresh, ChromeFacts::default());
    assert!(reply.chrome_hidden && view(&s, layers).card.is_some());
}

#[test]
fn pinching_the_color_ring_resizes_it_within_its_range() {
    let mut s = desk();
    let id = take_control(&mut s, ToolbarControl::Color, [500., 380.]);
    act(&mut s, BubbleAction::Resize { id, scale: 1.25 }).unwrap();
    assert_eq!(view(&s, id).diameter, COLOR_RING_SIZES[1] * 1.25);
    act(&mut s, BubbleAction::Resize { id, scale: 10. }).unwrap();
    assert_eq!(view(&s, id).diameter, COLOR_RING_RANGE[1]);
    act(&mut s, BubbleAction::Resize { id, scale: 0.01 }).unwrap();
    assert_eq!(view(&s, id).diameter, COLOR_RING_RANGE[0]);
    invoke(&mut s, CommandId::UndoWorkspace);
    assert_eq!(view(&s, id).diameter, COLOR_RING_RANGE[1]);
    let brush = take(&mut s, brush(DefaultBrushPreset::PointyPencil), [900., 380.]);
    assert!(act(&mut s, BubbleAction::Resize { id: brush, scale: 2. }).is_err());
    assert!(act(&mut s, BubbleAction::Resize { id, scale: f32::NAN }).is_err());
}

#[test]
fn an_open_layers_card_moves_without_folding() {
    let mut s = desk();
    let layers = take_control(&mut s, ToolbarControl::Panel { panel: Panel::Layers }, FAR);
    tap(&mut s, layers);
    let card = view(&s, layers).card.expect("the card unfolds");
    let at = s.bubble_placements().into_iter().find(|p| p.id == layers).unwrap().center;
    let grip = [at[0], at[1] + card.size[1] * 0.5 - 20.];
    drag(&mut s, BubbleGrab::Bubble { id: layers }, grip, [grip[0] - 60., grip[1] - 30.]);
    assert!(view(&s, layers).card.is_some(), "dragging the open card keeps it open");
}

#[test]
fn bubble_motion_retains_commands_until_editor_state_changes() {
    let mut s = desk();
    let id = take_control(&mut s, ToolbarControl::Command { command: CommandId::Undo }, FAR);
    let grab = BubbleGrab::Bubble { id };
    contact(&mut s, grab, ContactPhase::Down, FAR).unwrap();
    s.frame(0, 0).unwrap();
    let before = COMMAND_REFRESHES.with(|count| count.get());
    for frame in 1..=60 {
        contact(&mut s, grab, ContactPhase::Move, [FAR[0] + frame as f32, FAR[1]]).unwrap();
        s.step_bubbles(frame * 16_000_000);
        s.frame(frame * 16_000_000, frame * 16_000_000).unwrap();
    }
    assert_eq!(COMMAND_REFRESHES.with(|count| count.get()), before);
    contact(&mut s, grab, ContactPhase::Up, [FAR[0] + 60., FAR[1]]).unwrap();
    invoke(&mut s, CommandId::AddLayer);
    assert!(COMMAND_REFRESHES.with(|count| count.get()) > before);
    for command in &s.state.commands { assert_eq!(*command, s.command(command.id)); }
}

#[test]
fn halo_arcs_that_shape_the_dab_show_it_under_the_contact() {
    let mut s = desk();
    let id = take(&mut s, brush(DefaultBrushPreset::Paintbrush), FAR);
    tap(&mut s, id);
    tap(&mut s, id);
    for setting in ["hardness", "spacing"] {
        if !view(&s, id).rings.iter().any(|r| r.setting == setting) {
            act(&mut s, BubbleAction::ToggleRingSetting { brush: DefaultBrushPreset::Paintbrush as u32, setting: setting.into() }).unwrap();
        }
    }
    for (setting, shows) in [("hardness", true), ("spacing", false)] {
        let ring = view(&s, id).rings.into_iter().find(|r| r.setting == setting).unwrap();
        act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Down, position: ring.arc.point(0.4) }).unwrap();
        assert_eq!(view(&s, id).stamp.is_some(), shows, "{setting}");
        act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Up, position: ring.arc.point(0.4) }).unwrap();
        assert!(view(&s, id).stamp.is_none());
    }
}

#[test]
fn serialized_bubble_hit_geometry_preserves_native_contact_regions() {
    let mut s = desk();
    let dial = take_control(&mut s, ToolbarControl::Command { command: CommandId::DrawingBrush }, FAR);
    let halo = view(&s, dial);
    let ring_point = halo.rings[0].arc.point(0.5);
    assert_eq!(halo.hit(ring_point), Some(BubbleHit::Ring));
    let gap = (0..360).map(|degrees| {
        let angle = (degrees as f32).to_radians();
        [angle.cos() * 55., angle.sin() * 55.]
    }).find(|&point| halo.hit(point).is_none()).expect("the halo leaves canvas gaps");
    open_ring(&mut s, dial);
    let chooser = view(&s, dial);
    let brush_index = chooser.chooser.iter().position(|item| item.kind == ChooserKind::Brush).unwrap();
    assert_eq!(chooser.hit(chooser.chooser[brush_index].center), Some(BubbleHit::Item(brush_index)));
    let color = take_ring(&mut s, [350., 500.]);
    tap_bead(&mut s, color, Bead::Palettes);
    let wheel = view(&s, color);
    assert_eq!(wheel.hit([0.; 2]), Some(BubbleHit::Wheel));
    let history = take_control(&mut s, ToolbarControl::Command { command: CommandId::Undo }, [600., 200.]);
    let history = view(&s, history);
    assert_eq!(history.hit([-20., 0.]), Some(BubbleHit::Half(0)));
    assert_eq!(history.hit([20., 0.]), Some(BubbleHit::Half(1)));
    let layers = take_control(&mut s, ToolbarControl::Panel { panel: Panel::Layers }, [900., 600.]);
    let card = view(&s, layers);
    assert_eq!(card.hit([0.; 2]), Some(BubbleHit::Card));
    let mut disabled = halo.clone();
    disabled.rings.iter_mut().for_each(|ring| ring.enabled = false);
    assert_eq!(disabled.hit(ring_point), None);
    let controls = take_control(&mut s, ToolbarControl::ColorPicker, [500., 350.]);
    let brush = take(&mut s, brush(DefaultBrushPreset::Charcoal), [750., 350.]);
    let mut regions = Vec::new();
    for value in [halo, chooser, wheel, history, card, disabled, view(&s, controls), view(&s, brush)] {
        let encoded = serde_json::to_vec(&value).unwrap();
        let geometry: BubbleHitGeometry = serde_json::from_slice(&encoded).unwrap();
        let mut points = vec![[0.; 2], gap, ring_point, [-20., 0.], [20., 0.]];
        points.extend(value.chooser.iter().flat_map(|item| {
            let size = item.size();
            [[item.center[0], item.center[1]],
                [item.center[0] + size[0] * 0.5 - 0.01, item.center[1]],
                [item.center[0] + size[0] * 0.5 + 0.01, item.center[1]],
                [item.center[0] + size[0] * 0.5 - 0.01, item.center[1] + size[1] * 0.5 - 0.01]]
        }));
        if let Some(card) = &value.card {
            points.extend([[card.size[0] * 0.5 - 0.01, card.size[1] * 0.5 - 0.01], [card.size[0] * 0.5 + 0.01, 0.]]);
        }
        points.extend((-400..=400).step_by(8).flat_map(|x| (-400..=400).step_by(8).map(move |y| [x as f32, y as f32])));
        for point in points {
            let expected = value.hit(point);
            assert_eq!(geometry.hit(point), expected, "{:?} {} at {point:?}", value.kind, value.label);
            if !regions.contains(&expected) { regions.push(expected); }
        }
    }
    for expected in [None, Some(BubbleHit::Face), Some(BubbleHit::Ring), Some(BubbleHit::Bead), Some(BubbleHit::Pill),
        Some(BubbleHit::Wheel), Some(BubbleHit::Card), Some(BubbleHit::Half(0)), Some(BubbleHit::Half(1))] {
        assert!(regions.contains(&expected), "sampled region {expected:?}");
    }
    assert!(regions.iter().any(|hit| matches!(hit, Some(BubbleHit::Item(_)))));
}

#[test]
fn restoring_workspace_cancels_bubble_contacts_before_reusing_their_ids() {
    let mut s = desk();
    let empty = s.state.workspace.clone();
    let controls = take_control(&mut s, ToolbarControl::ColorPicker, FAR);
    let size = |s: &UiSession<Recorder>| ToolbarNumericBinding::BrushSize.field(&s.state).unwrap().value;
    let before = size(&s);
    let ring = view(&s, controls).rings.into_iter().find(|ring| ring.setting == "size").unwrap();
    act(&mut s, BubbleAction::Ring { id: controls, phase: ContactPhase::Down, position: ring.arc.point(0.8) }).unwrap();
    assert_ne!(size(&s), before);
    assert!(view(&s, controls).stamp.is_some());
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(empty.clone()) }).unwrap();
    assert_eq!(size(&s), before, "replacing the workspace cancels the numeric gesture");
    let layers = take_control(&mut s, ToolbarControl::Panel { panel: Panel::Layers }, FAR);
    assert_eq!(layers, controls, "the replacement workspace reuses its first free ID");
    let card = view(&s, layers);
    assert!(card.stamp.is_none() && card.readout.is_none() && card.rings.iter().all(|ring| !ring.active));
    let placed = s.state.workspace.clone();
    let original_center = saved(&s, layers);
    contact(&mut s, BubbleGrab::Bubble { id: layers }, ContactPhase::Down, FAR).unwrap();
    contact(&mut s, BubbleGrab::Bubble { id: layers }, ContactPhase::Move, [700., 500.]).unwrap();
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(placed) }).unwrap();
    contact(&mut s, BubbleGrab::Bubble { id: layers }, ContactPhase::Up, [700., 500.]).unwrap();
    assert_eq!(saved(&s, layers), original_center, "a late release cannot move the restored layout");
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(empty) }).unwrap();
    let color = take_ring(&mut s, FAR);
    let placed = s.state.workspace.clone();
    let palette = bead(&view(&s, color), Bead::Palettes);
    beads(&mut s, color, ContactPhase::Down, palette);
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(placed) }).unwrap();
    beads(&mut s, color, ContactPhase::Up, palette);
    assert!(view(&s, color).pill.is_none(), "a replaced bead contact cannot open the restored palette pill");
}

#[test]
fn restoring_workspace_preserves_the_canvas_stroke_that_controls_bubble_fading() {
    let mut s = desk();
    let id = take(&mut s, brush(DefaultBrushPreset::PointyPencil), FAR);
    let workspace = s.state.workspace.clone();
    let near = [FAR[0], FAR[1] + 60.];
    stroke(&mut s, 1, PenPhase::Down, near);
    assert!(s.bubble_fade().0);
    s.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) }).unwrap();
    assert!(s.bubble_fade().0, "restoring UI does not release the ongoing canvas contact");
    s.bubble_placements();
    stroke(&mut s, 1, PenPhase::Move, near);
    assert!(s.bubble_fade().1.contains(&id), "the ongoing stroke fades the restored bubble");
    stroke(&mut s, 1, PenPhase::Up, near);
    assert!(!s.bubble_fade().0);
}

#[test]
fn native_source_pickup_flags_match_the_original_header_and_tile_controls() {
    let mut plain = 0;
    let mut blocked = 0;
    for platform in Platform::ALL {
        let mut s = session(platform);
        s.set_viewport(VIEW, [1200, 900]).unwrap();
        for command in [CommandId::DrawingBrush, CommandId::Lasso, CommandId::Eraser, CommandId::Liquify] {
            invoke(&mut s, command);
            for item in s.header_view().items {
                let grab = BubbleGrab::Header { id: item.id };
                let expected = s.bubble_grab(grab, false).is_some();
                assert_eq!(item.tears_bubble, expected, "{platform:?}/{command:?}/header {}", item.id);
                plain += usize::from(expected);
                blocked += usize::from(!expected);
            }
            for panel in &s.state.workspace.layout.panels {
                for tile in s.panel_view(panel.id).unwrap().tiles {
                    let grab = BubbleGrab::Tile { panel: panel.id, tile: tile.id };
                    let expected = s.bubble_grab(grab, false).is_some();
                    assert_eq!(tile.tears_bubble, expected, "{platform:?}/{command:?}/{:?}/tile {}", panel.id, tile.id);
                    plain += usize::from(expected);
                    blocked += usize::from(!expected);
                }
            }
        }
    }
    assert!(plain > 0 && blocked > 0, "both direct sources and controls that cannot tear are projected");
}

#[test]
fn dither_and_scatter_cell_size_is_a_localized_whole_pixel_log_slider() {
    for (preset, cell_id, default_spread, default_cell) in [(DefaultBrushPreset::DitherBlender, "dither_cell_size", 0.08, 4.), (DefaultBrushPreset::ScatterBlender, "scatter_cell_size", 0.2, 3.)] {
        let brush = layer_core::default_brush(preset);
        let english = Localizer::shared(UiLanguage::English);
        let controls = tool_settings::controls(preset as u32, &brush, &english);
        let cell = controls.iter().find(|c| c.id == cell_id).unwrap();
        let smooth = controls.iter().find(|c| c.id == "smoothing_strength").unwrap();
        let spread_index = controls.iter().position(|c| c.id == "spread").unwrap();
        let cell_index = controls.iter().position(|c| c.id == cell.id).unwrap();
        let spread = &controls[spread_index];
        assert_eq!(cell_index, spread_index + 1);
        assert_eq!(cell.group_id, spread.group_id);
        assert_eq!(spread.numeric.mapping, NumericMapping::Log);
        assert_eq!(spread.numeric.log_offset, smooth.numeric.log_offset);
        let other = tool_settings::controls(DefaultBrushPreset::CrystalBlender as u32, &layer_core::default_brush(DefaultBrushPreset::CrystalBlender), &english);
        assert_eq!(other.iter().find(|c| c.id == "spread").unwrap().numeric.mapping, NumericMapping::Linear);
        for (position, expected) in [(0., 0.), (0.5, 0.09), (1., 1.)] {
            let resolved = spread.numeric.resolve(0., NumericOperation::Position { position }).unwrap().value;
            assert!((resolved - expected).abs() < 0.011);
        }
        assert_eq!(spread.value, default_spread);
        assert_eq!(cell.value, default_cell);
        assert_eq!(cell.numeric.kind, NumericKind::Slider);
        assert_eq!(cell.numeric.mapping, NumericMapping::Log);
        assert_eq!((cell.numeric.min, cell.numeric.max), (1., 50.));
        assert_eq!((cell.numeric.resolution, cell.numeric.step, cell.numeric.digits), (1., 1., 0));
        assert_eq!(cell.numeric.unit, "px");
        assert!((cell.numeric.log_offset - (49. * smooth.numeric.log_offset - 1.)).abs() < 1e-12);
        for position in [0., 0.1, 0.25, 0.5, 0.75, 0.9, 1.] {
            let value = cell.numeric.resolve(1., NumericOperation::Position { position }).unwrap().value;
            assert_eq!(value.fract(), 0.);
            assert!((1. ..=50.).contains(&value));
            if position == 0.5 { assert!(value < 8., "small cells occupy most of the slider"); }
        }
        for value in 1..=layer_core::BLENDER_CELL_SIZE_LIMIT {
            let edited = tool_settings::edit(&brush, cell.id, value as f32, &english).unwrap();
            assert_eq!(edited.blender.cell_size, value as f32);
            let restored: layer_core::BrushSnapshot = serde_json::from_str(&serde_json::to_string(&edited).unwrap()).unwrap();
            assert_eq!(restored, edited);
            restored.validate().unwrap();
        }
        for value in [0., 51., 1.5, f32::NAN, f32::INFINITY] {
            assert!(tool_settings::edit(&brush, cell.id, value, &english).is_err());
            let mut invalid = brush.clone();
            invalid.blender.cell_size = value;
            assert!(invalid.validate().is_err());
        }
        let other_cell_id = if cell_id == "dither_cell_size" { "scatter_cell_size" } else { "dither_cell_size" };
        assert!(tool_settings::edit(&brush, other_cell_id, 2., &english).is_err());
        let crystal = layer_core::default_brush(DefaultBrushPreset::CrystalBlender);
        assert!(tool_settings::edit(&crystal, cell.id, 2., &english).is_err());
        assert_eq!(tool_settings::edit(&crystal, "cell_size", 0.3, &english).unwrap().blender.scale, 0.3);
        for language in UiLanguage::ALL {
            let localized = tool_settings::controls(preset as u32, &brush, &Localizer::shared(language));
            let localized = localized.iter().find(|c| c.id == cell.id).unwrap();
            assert_eq!(localized.numeric, cell.numeric);
            assert_eq!(localized.value, cell.value);
            assert!(!localized.label.is_empty());
        }
    }
}

#[test]
fn dither_and_scatter_bubbles_expose_cell_size_and_preserve_edits_on_every_platform() {
    for (preset, cell_id, default_spread, default_cell) in [(DefaultBrushPreset::DitherBlender, "dither_cell_size", 0.08, 4.), (DefaultBrushPreset::ScatterBlender, "scatter_cell_size", 0.2, 3.)] {
        for platform in Platform::ALL {
            let mut s = session(platform);
            s.set_viewport(VIEW, [1200, 900]).unwrap();
            let id = take(&mut s, brush(preset), FAR);
            let bubble = view(&s, id);
            assert!(bubble.open);
            assert_eq!(s.engine.configured_brush().blender.reach, default_spread);
            assert_eq!(bubble.rings.iter().map(|r| r.setting).collect::<Vec<_>>(), if preset == DefaultBrushPreset::ScatterBlender { vec!["spread", cell_id, "grit"] } else { vec!["spread", cell_id] });
            let spread = bubble.rings.iter().find(|r| r.setting == "spread").unwrap();
            if preset == DefaultBrushPreset::DitherBlender { assert!(spread.arc.point(0.5)[0] < 0.); }
            let ring = bubble.rings.iter().find(|r| r.setting == cell_id).unwrap().clone();
            if preset == DefaultBrushPreset::DitherBlender { assert!(ring.arc.point(0.5)[0] > 0.); }
            for (fraction, expected) in [(0., 0.), (0.5, 0.09), (1., 1.)] {
                let position = spread.arc.point(fraction);
                act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Down, position }).unwrap();
                act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Up, position }).unwrap();
                assert!((s.engine.configured_brush().blender.reach - expected).abs() < 0.011);
            }
            let value = |s: &UiSession<Recorder>| s.engine.configured_brush().blender.cell_size;
            assert_eq!(value(&s), default_cell);
            for (fraction, expected) in [(1., 50.), (0., 1.)] {
                let position = ring.arc.point(fraction);
                act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Down, position }).unwrap();
                act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Up, position }).unwrap();
                assert_eq!(value(&s), expected, "{platform:?}");
            }
            s.dispatch(UiAction::SetToolSetting { id: ring.setting.into(), value: 12. }).unwrap();
            act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Down, position: ring.arc.point(0.5) }).unwrap();
            assert_ne!(value(&s), 12.);
            act(&mut s, BubbleAction::Ring { id, phase: ContactPhase::Cancel, position: [0.; 2] }).unwrap();
            assert_eq!(value(&s), 12.);
            s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::CrystalBlender as u32 }).unwrap();
            s.dispatch(UiAction::SelectBrush { id: preset as u32 }).unwrap();
            assert_eq!(value(&s), 12.);
            let workspace: WorkspaceState = serde_json::from_str(&serde_json::to_string(&s.state.workspace).unwrap()).unwrap();
            workspace.layout.validate().unwrap();
            let saved = serde_json::to_string(&s.editing_state()).unwrap();
            let mut restored = session(platform);
            restored.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) }).unwrap();
            restored.restore_editing(serde_json::from_str(&saved).unwrap()).unwrap();
            restored.dispatch(UiAction::SelectBrush { id: preset as u32 }).unwrap();
            assert_eq!(value(&restored), 12., "{platform:?}: saved editing state retains the cell size");
            if view(&s, id).rings.is_empty() { tap(&mut s, id); }
            act(&mut s, BubbleAction::ResetRing { id, position: ring.arc.point(0.5) }).unwrap();
            assert_eq!(value(&s), default_cell);
            act(&mut s, BubbleAction::ResetRing { id, position: spread.arc.point(0.5) }).unwrap();
            assert_eq!(s.engine.configured_brush().blender.reach, default_spread);
            s.dispatch(UiAction::SetToolSetting { id: "spread".into(), value: 0.7 }).unwrap();
            s.dispatch(UiAction::SetToolSetting { id: cell_id.into(), value: 8. }).unwrap();
            s.dispatch(UiAction::ResetToolSetting { id: "spread".into() }).unwrap();
            s.dispatch(UiAction::ResetToolSetting { id: cell_id.into() }).unwrap();
            assert_eq!(s.engine.configured_brush().blender.reach, default_spread);
            assert_eq!(value(&s), default_cell);
        }
    }
}

fn blender_profiles() -> Vec<(DefaultBrushPreset, &'static [&'static str])> {
    use DefaultBrushPreset::*;
    vec![
        (Smudge, &["pull", "spread"]), (NaturalBlender, &["pull", "spread"]),
        (SoftBlender, &["pull", "spread"]), (BlendingStump, &["pull", "spread"]), (Tissue, &["pull", "spread"]),
        (Blur, &["spread"]), (GrainyBlender, &["spread", "grain_depth"]),
        (BristleBlender, &["pull", "streaks", "spread"]), (BlendingKnife, &["pull", "streaks"]),
        (TextureBlender, &["texture_kept", "detail_size", "spread"]), (HueBlender, &["blend_lightness", "spread"]),
        (WetBlender, &["pull", "dilution", "average_pickup"]), (ColorMixer, &["pull", "average_pickup", "persistence"]),
        (PainterlyBlender, &["spread", "stretch"]), (CombBlender, &["edge_keeping", "spread"]),
        (ScatterBlender, &["spread", "scatter_cell_size", "grit"]), (Sharpen, &["sharpness", "spread"]),
        (Marbling, &["swirl", "swirl_size"]), (CrystalBlender, &["cell_size", "stretch", "spread"]),
        (DitherBlender, &["spread", "dither_cell_size"]),
    ]
}

#[test]
fn blender_profiles_match_primary_controls_and_default_bubbles_on_every_platform() {
    for platform in Platform::ALL {
        for (preset, expected) in blender_profiles() {
            let mut s = session(platform);
            s.set_viewport(VIEW, [1200, 900]).unwrap();
            let id = take(&mut s, brush(preset), FAR);
            let primary: Vec<_> = s.state.tool_settings.iter().filter(|c| c.group_id.is_empty() && !matches!(c.id, "size" | "flow" | "strength" | "smoothing_strength")).map(|c| c.id).collect();
            assert_eq!(primary, expected, "{platform:?}/{preset:?}");
            assert_eq!(view(&s, id).rings.iter().map(|r| r.setting).collect::<Vec<_>>(), expected, "{platform:?}/{preset:?}");
            let grouped: Vec<_> = s.state.tool_settings.iter().filter(|c| !c.group_id.is_empty()).map(|c| c.group_id).collect();
            if let (Some(blender), Some(tip)) = (grouped.iter().position(|id| *id == "tool-setting-group-blender"), grouped.iter().position(|id| *id == "tool-setting-group-tip")) {
                assert!(blender < tip, "{platform:?}/{preset:?}");
            }
            if s.engine.configured_brush().execution == layer_core::BrushExecution::Smudge { assert!(!s.state.tool_settings.iter().any(|c| c.id == "paint")); }
            for setting in expected {
                act(&mut s, BubbleAction::ToggleRingSetting { brush: preset as u32, setting: (*setting).into() }).unwrap();
            }
            assert!(view(&s, id).rings.is_empty(), "explicit empty membership is retained");
            let workspace = s.state.workspace.clone();
            let mut restored = session(platform);
            restored.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) }).unwrap();
            restored.dispatch(UiAction::SelectBrush { id: preset as u32 }).unwrap();
            if !view(&restored, id).open { tap(&mut restored, id); }
            assert!(view(&restored, id).rings.is_empty(), "{platform:?}/{preset:?}");
            act(&mut s, BubbleAction::ToggleRingSetting { brush: preset as u32, setting: "size".into() }).unwrap();
            restored.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(s.state.workspace.clone()) }).unwrap();
            restored.dispatch(UiAction::SelectBrush { id: preset as u32 }).unwrap();
            if !view(&restored, id).open { tap(&mut restored, id); }
            assert_eq!(view(&restored, id).rings.iter().map(|r| r.setting).collect::<Vec<_>>(), ["size"], "{platform:?}/{preset:?}: custom membership is retained");
        }
    }
}

#[test]
fn blender_hidden_pickup_and_persistence_values_survive_profile_projection_and_restore() {
    for platform in Platform::ALL {
        let mut s = session(platform);
        s.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::WetBlender as u32 }).unwrap();
        for (id, value) in [("average_pickup", 0.5), ("persistence", 2.), ("average_pickup", 0.)] {
            s.dispatch(UiAction::SetToolSetting { id: id.into(), value }).unwrap();
        }
        assert!(!s.state.tool_settings.iter().any(|c| c.id == "persistence"));
        assert_eq!(s.engine.configured_brush().wet_mix.stretch, 2.);
        let saved = serde_json::to_string(&s.editing_state()).unwrap();
        let mut restored = session(platform);
        restored.restore_editing(serde_json::from_str(&saved).unwrap()).unwrap();
        restored.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::WetBlender as u32 }).unwrap();
        assert_eq!(restored.engine.configured_brush().wet_mix.stretch, 2.);
        restored.dispatch(UiAction::SetToolSetting { id: "average_pickup".into(), value: 0.5 }).unwrap();
        assert_eq!(restored.state.tool_settings.iter().find(|c| c.id == "persistence").unwrap().value, 2.);
        restored.set_viewport(VIEW, [1200, 900]).unwrap();
        let id = take(&mut restored, brush(DefaultBrushPreset::WetBlender), FAR);
        for setting in ["persistence", "size", "hardness"] {
            act(&mut restored, BubbleAction::ToggleRingSetting { brush: DefaultBrushPreset::WetBlender as u32, setting: setting.into() }).unwrap();
        }
        restored.dispatch(UiAction::SetToolSetting { id: "average_pickup".into(), value: 0. }).unwrap();
        assert!(act(&mut restored, BubbleAction::ToggleRingSetting { brush: DefaultBrushPreset::WetBlender as u32, setting: "angle".into() }).is_err(), "hidden stored memberships count toward six arcs");
        act(&mut restored, BubbleAction::ToggleRingSetting { brush: DefaultBrushPreset::WetBlender as u32, setting: "hardness".into() }).unwrap();
        restored.dispatch(UiAction::SetToolSetting { id: "average_pickup".into(), value: 0.5 }).unwrap();
        if view(&restored, id).rings.is_empty() { tap(&mut restored, id); }
        assert_eq!(view(&restored, id).rings.iter().map(|r| r.setting).collect::<Vec<_>>(), ["pull", "dilution", "average_pickup", "persistence", "size"], "hidden membership survives editing another arc in its original order");
        assert_eq!(restored.engine.configured_brush().wet_mix.stretch, 2.);
        restored.dispatch(UiAction::ResetToolSetting { id: "persistence".into() }).unwrap();
        assert_eq!(restored.engine.configured_brush().wet_mix.stretch, layer_core::default_brush(DefaultBrushPreset::WetBlender).wet_mix.stretch);
        let mut hidden = restored.editing_state();
        hidden.tools.overrides.insert(DefaultBrushPreset::Blur as u32, [("pull".into(), 0.37), ("paint".into(), 0.23), ("dilution".into(), 0.41)].into());
        restored.restore_editing(hidden).unwrap();
        restored.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::Blur as u32 }).unwrap();
        assert!(!restored.state.tool_settings.iter().any(|c| matches!(c.id, "pull" | "paint" | "dilution")));
        assert_eq!(restored.engine.configured_brush().wet_mix.pull, 0.37);
        assert_eq!(restored.engine.configured_brush().wet_mix.amount_of_paint, 0.23);
        assert_eq!(restored.engine.configured_brush().wet_mix.dilution, 0.41);
        restored.dispatch(UiAction::SelectBrush { id: DefaultBrushPreset::MixingBrush as u32 }).unwrap();
        let primary: Vec<_> = restored.state.tool_settings.iter().filter(|c| c.group_id.is_empty() && !matches!(c.id, "size" | "flow" | "strength" | "smoothing_strength")).map(|c| c.id).collect();
        assert_eq!(primary, ["paint", "pull"]);
    }
}
