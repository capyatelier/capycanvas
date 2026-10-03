use super::new_photo::{invoke, ready};
use super::*;
use layer_core::{
    Project,
    color::{source::*, *},
};

fn fixture() -> Project {
    let mut p = new_drawing(64, 16, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    p.document.color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    };
    p.document
        .layers
        .iter_mut()
        .find(|l| l.kind == layer_core::LayerKind::Background)
        .unwrap()
        .visible = false;
    let mut source = SourceBuilder::new(
        [64, 16],
        SourceInterpretation {
            channels: SourceChannels::Rgba,
            depth: SampleDepth::U16,
            profile: ColorProfile::Builtin(RgbSpace::DisplayP3),
            profile_assumed: false,
        },
        1024 * 1024,
    )
    .unwrap();
    let pixels: [[u16; 4]; 4] = [
        [0, 0, 0, 65535],
        [20000, 30000, 40000, 32768],
        [65535; 4],
        [60000, 30000, 10000, 0],
    ];
    let row: Vec<u8> = (0..64)
        .flat_map(|x| pixels[x / 16].into_iter().flat_map(u16::to_le_bytes))
        .collect();
    for _ in 0..16 {
        source.push_row(&row).unwrap();
    }
    p.document.layers[0].source = Some(std::sync::Arc::new(source.finish().unwrap()));
    p.document.reference_layers.insert(p.document.layers[0].id);
    let mut mask =
        layer_core::LayerMask::reveal_all(p.document.allocate_layer_id(), Point { x: 0., y: 0. });
    mask.default_coverage = 0.5;
    mask.show_area = true;
    p.document.layers[0].mask = Some(mask);
    p
}
fn completed(w: &Workspace) -> std::sync::Arc<layer_core::color::histogram::Histogram> {
    until(|| state(w).histogram.status.as_ref() == "Exact" && state(w).histogram.data.is_some(), "exact histogram");
    state(w).histogram.data.unwrap()
}

fn histogram_widget<T: IsA<gtk::Widget>>(w: &Workspace, name: &str) -> T {
    widgets(w.window.upcast_ref()).find(|widget| widget.is_mapped() && widget.widget_name() == name)
        .unwrap_or_else(|| panic!("mapped {name}")).downcast().unwrap()
}

pub(super) fn choose(w: &Workspace, input: &mut RemoteInput, name: &str, index: u32) {
    let dropdown = widgets(w.window.upcast_ref()).find(|widget| widget.is_mapped() && widget.widget_name() == name)
        .map(|widget| widget.downcast::<gtk::DropDown>().unwrap()).unwrap_or_else(|| named(w.window.upcast_ref(), name));
    scroll_to(dropdown.upcast_ref());
    input.click(screen_point(dropdown.upcast_ref(), &w.window, [0.5, 0.5]));
    let model = dropdown.model().unwrap();
    let label = model.item(index).unwrap_or_else(|| panic!("native {name} choice {index} of {}", model.n_items())).downcast::<gtk::StringObject>().unwrap().string();
    let visible_option = |widget: &gtk::Widget| widget.is_mapped()
        && widget.native().is_some_and(|native| native.is::<gtk::Popover>())
        && widget.downcast_ref::<gtk::Label>().is_some_and(|l| l.text() == label);
    until(|| widgets(dropdown.upcast_ref()).any(|widget| visible_option(&widget)), &format!("native {name} popup choice {label}"));
    let item = widgets(dropdown.upcast_ref()).find(visible_option).unwrap();
    input.click(screen_point(&item, &w.window, [0.5, 0.5]));
    assert_eq!(dropdown.selected(), index);
}

fn tab(w: &Workspace, input: &mut RemoteInput, panel: Panel) {
    let button = w.groups.borrow().iter().flat_map(|group| &group.tabs)
        .find(|(candidate, button)| *candidate == panel && button.is_mapped()).unwrap().1.clone();
    input.click(screen_point(button.upcast_ref(), &w.window, [0.5, 0.5]));
}

#[test]
#[ignore = "private Wayland display, hardware GPU and native input"]
fn native_composite_histogram_updates_without_changing_the_drawing() {
    let app = native_test_app("art.capycanvas.Histogram");
    let w = Workspace::with_project(&app, Some((fixture(), None)));
    w.window.maximize();w.window.present();ready(&w);
    let width = w.window.width();
    assert!(matches!(width, 640 | 1100), "run private viewport640x800 or1100x800");
    let mut input = RemoteInput::new().settle_ms(150).timeout_secs(30);input.ready();
    for panel in Panel::ALL.into_iter().filter(|panel| !matches!(panel, Panel::Toolbar | Panel::Commands | Panel::Histogram | Panel::Properties)) {
        w.customize(CustomizationAction::SetPanelVisible { panel, visible: false });
    }
    w.dispatch(UiAction::Customize { action: CustomizationAction::CloseExpanded });
    let output = artifact_dir("../../artifacts/photo-editing-color/p16-gtk");
    std::fs::create_dir_all(&output).unwrap();
    let mut before = super::place_source::snapshot(&w);
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        invoke(&w, CommandId::Histogram);
        w.dispatch(UiAction::MovePanel { panel: Panel::Histogram, target: DockTarget::Edge { edge: Edge::Right, outer: false }, viewport: [width as f32, 800.] });
        pump(200);
        let initial = completed(&w);
        assert_eq!((initial.pixels, initial.transparent), (768, 256));
        for channel in &initial.channels { assert_eq!(channel.bins.iter().sum::<u64>(), initial.pixels); }
        assert!(w.histogram.root.is_mapped());
        choose(&w, &mut input, "histogram-channel", 4);
        assert_eq!(state(&w).histogram.channel, 4);
        let logarithmic = histogram_widget::<gtk::CheckButton>(&w, "histogram-log");
        input.click(screen_point(logarithmic.upcast_ref(), &w.window, [0.5, 0.5]));
        assert!(state(&w).histogram.logarithmic);
        assert_eq!(*completed(&w), *initial);
        choose(&w, &mut input, "histogram-source", 1);
        let raw = completed(&w);
        assert_eq!((raw.pixels, raw.transparent), (768, 256));
        assert_eq!(*raw, *initial, "partial mask coverage does not weight histogram counts");
        choose(&w, &mut input, "histogram-source", 2);
        assert_eq!(*completed(&w), *initial, "reference composition keeps its mask");
        choose(&w, &mut input, "histogram-source", 3);
        until(|| state(&w).histogram.status == w.localization().text(layer_ui::MessageId::RESOURCES_HISTOGRAM_UNAVAILABLE), "selection source without selection");
        assert!(state(&w).histogram.data.is_none());
        choose(&w, &mut input, "histogram-source", 0);
        assert_eq!(*completed(&w), *initial);
        assert_eq!(super::place_source::snapshot(&w), before, "histogram controls create no artwork/history edit");
        w.dispatch(UiAction::Invoke { command: CommandId::FitCanvas });pump(200);
        w.dispatch(UiAction::Invoke { command: CommandId::RectangleSelect });
        let a = super::photo_edit::window_point(&w, [16., 1.]);
        let b = super::photo_edit::window_point(&w, [31., 15.]);
        let hit = w.window.pick(a[0] as f64, a[1] as f64, gtk::PickFlags::DEFAULT).unwrap();
        assert!(hit == w.area || hit.is_ancestor(&w.area), "selection point hits canvas, got {}", hit.widget_name());
        input.perform(serde_json::json!([{"point":a,"down":true},{"point":b},{"down":false}]));
        until(|| super::photo_edit::document(&w).selection.is_some(), "native rectangle selection");
        choose(&w, &mut input, "histogram-source", 3);
        let selected = completed(&w);
        assert!(selected.pixels > 0 && selected.pixels < initial.pixels);
        assert_eq!(selected.transparent, 0);
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        choose(&w, &mut input, "histogram-source", 0);
        assert_eq!(*completed(&w), *initial);
        before = super::place_source::snapshot(&w);
        let unmarked = super::photo_edit::shown(&w, [8., 8.]);
        let shadows = histogram_widget::<gtk::CheckButton>(&w, "histogram-shadows");
        input.click(screen_point(shadows.upcast_ref(), &w.window, [0.5, 0.5]));
        assert!(state(&w).histogram.shadows);
        pump(200);
        let marked = super::photo_edit::shown(&w, [8., 8.]);
        assert_ne!(marked, unmarked, "clipping marks are presented over black artwork");
        assert_eq!(*completed(&w), *initial, "clipping never enters histogram readings");
        assert_eq!(super::place_source::snapshot(&w), before, "clipping creates no artwork/history edit");
        crate::snapshot(&w).save_to_png(std::path::Path::new(&output).join(format!("clipping-{width}-{theme:?}.png"))).unwrap();
        input.click(screen_point(shadows.upcast_ref(), &w.window, [0.5, 0.5]));
        pump(200);assert_eq!(super::photo_edit::shown(&w, [8., 8.]), unmarked);
        assert_eq!(super::place_source::snapshot(&w), before, "inspection never edits artwork/history");
        crate::snapshot(&w).save_to_png(std::path::Path::new(&output).join(format!("histogram-{width}-{theme:?}.png"))).unwrap();
        w.customize(CustomizationAction::SetPanelVisible { panel: Panel::Properties, visible: true });
        w.dispatch(UiAction::Customize { action: CustomizationAction::CloseExpanded });
        let group = state(&w).workspace.layout.panel_group(Panel::Properties).unwrap();
        w.dispatch(UiAction::MovePanel { panel: Panel::Histogram, target: DockTarget::Tab { group, index: None }, viewport: [width as f32, 800.] });
        pump(200);tab(&w, &mut input, Panel::Properties);
        crate::snapshot(&w).save_to_png(std::path::Path::new(&output).join(format!("tabs-{width}-{theme:?}.png"))).unwrap();
        assert_eq!(state(&w).workspace.layout.active_panel(Panel::Histogram), Some(Panel::Properties), "native Properties tab is selected");
        until(|| state(&w).histogram.data.is_none(), "inactive histogram tab retires demand");
        tab(&w, &mut input, Panel::Histogram);
        assert_eq!(*completed(&w), *initial);
        w.dispatch(UiAction::Customize { action: CustomizationAction::SetColumnCollapsed { group, collapsed: true } });
        until(|| state(&w).histogram.data.is_none(), "closed collapsed histogram retires demand");
        invoke(&w, CommandId::Histogram);
        assert_eq!(*completed(&w), *initial);
        choose(&w, &mut input, "histogram-channel", 0);
        crate::snapshot(&w).save_to_png(std::path::Path::new(&output).join(format!("drawer-{width}-{theme:?}.png"))).unwrap();
        w.dispatch(UiAction::Customize { action: CustomizationAction::SetColumnCollapsed { group, collapsed: false } });
        w.dispatch(UiAction::Customize { action: CustomizationAction::CloseExpanded });
        let root = w.histogram.root.clone();
        w.dispatch(UiAction::MovePanel { panel: Panel::Histogram,
            target: DockTarget::Float { position: [70., 150.] }, viewport: [width as f32, 800.] });
        pump(200);assert_eq!(*completed(&w), *initial);
        assert_eq!(w.histogram.root, root);
        choose(&w, &mut input, "histogram-channel", 4);
        crate::snapshot(&w).save_to_png(std::path::Path::new(&output).join(format!("floating-{width}-{theme:?}.png"))).unwrap();
        w.dispatch(UiAction::Customize { action: CustomizationAction::SetPanelVisible { panel: Panel::Histogram, visible: false } });
        until(|| state(&w).histogram.data.is_none(), "hidden histogram releases data");
        invoke(&w, CommandId::Histogram);
        assert_eq!(*completed(&w), *initial);
        w.dispatch(UiAction::Histogram { action: layer_ui::HistogramAction::Logarithmic { enabled: false } });
        w.dispatch(UiAction::Histogram { action: layer_ui::HistogramAction::Channel { index: 0 } });
    }
    w.dispatch(UiAction::Customize { action: CustomizationAction::SetPanelVisible { panel: Panel::Histogram, visible: false } });
    until(|| state(&w).histogram.data.is_none(), "last consumer closes");
    assert_eq!(super::place_source::snapshot(&w), before);
    input.finish();w.window.close();pump(100);
}

pub(super) fn scroll_to(widget: &gtk::Widget) {
    let mut parent = widget.parent();
    while let Some(ancestor) = parent {
        if let Some(scroll) = ancestor.downcast_ref::<gtk::ScrolledWindow>() {
            let bounds = widget.compute_bounds(scroll).unwrap();
            let adjustment = scroll.vadjustment();
            adjustment.set_value(adjustment.value() + (bounds.y() as f64).min(0.)
                + ((bounds.y() + bounds.height()) as f64 - scroll.height() as f64).max(0.));
        }
        parent = ancestor.parent();
    }
    pump(100);
}

fn tonal_completed(w: &Workspace) -> std::sync::Arc<layer_core::color::histogram::Histogram> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| until(
        || state(w).tonal_histogram.status.as_ref() == "Exact" && state(w).tonal_histogram.data.is_some(), "exact embedded Curves histogram")));
    if let Err(error) = result {
        let current = state(w);
        let document = super::photo_edit::document(w);
        let effect = document.layer(document.active_layer).and_then(|layer| layer.effect.as_ref());
        eprintln!("embedded statistics timeout: view {:?}, page {:?}, domain {:?}, host_error {:?}, notice {:?}", current.tonal_histogram, current.layer_properties.page, effect.and_then(|effect| effect.value("domain")), current.host_error, current.notice);
        std::panic::resume_unwind(error);
    }
    state(w).tonal_histogram.data.unwrap()
}

#[test]
#[ignore = "private Wayland display, hardware GPU and native input"]
fn native_curves_histogram_preserves_numeric_focus() {
    use layer_core::EffectValue;
    use layer_ui::EffectAction;
    use layer_core::color::histogram::HistogramDomain;
    let app = native_test_app("art.capycanvas.CurvesHistogram");
    let mut project = fixture();project.document.color.depth = SampleDepth::F32;project.document.blend_space = layer_core::BlendSpace::Linear;
    let w = Workspace::with_project(&app, Some((project, None)));
    w.window.maximize();w.window.present();ready(&w);
    let width = w.window.width();assert!(matches!(width, 640 | 1100));
    let mut input = RemoteInput::new().settle_ms(150).timeout_secs(30);input.ready();
    for panel in Panel::ALL.into_iter().filter(|panel| !matches!(panel, Panel::Toolbar | Panel::Commands | Panel::Properties)) {
        w.customize(CustomizationAction::SetPanelVisible { panel, visible: false });
    }
    w.customize(CustomizationAction::SetPanelVisible { panel: Panel::Properties, visible: true });
    w.dispatch(UiAction::Customize { action: CustomizationAction::CloseExpanded });
    w.dispatch(UiAction::MovePanel { panel: Panel::Properties, target: DockTarget::Edge { edge: Edge::Right, outer: false }, viewport: [width as f32, 800.] });
    w.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "curves".into() } });ready(&w);
    let layer = state(&w).layer_properties.layer.unwrap();
    let set = |key: &str, value| w.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: key.into(), value } });
    set("curve_0", EffectValue::Curve(vec![[0., 0.], [0.5, 0.5], [1., 1.]]));
    let output = artifact_dir("../../artifacts/photo-editing-color/p16-gtk");
    std::fs::create_dir_all(&output).unwrap();
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        choose(&w, &mut input, "properties-page", 0);
        choose(&w, &mut input, "property-domain", 0);
        let initial = tonal_completed(&w);assert_eq!(initial.domain, HistogramDomain::Encoded);
        let graph = histogram_widget::<gtk::DrawingArea>(&w, "property-curve_0-graph");
        scroll_to(graph.upcast_ref());input.click(screen_point(graph.upcast_ref(), &w.window, [0.5, 0.5]));
        let field = histogram_widget::<gtk::Widget>(&w, "property-curve_0-output");
        let spin = descendant::<gtk::SpinButton>(&field).unwrap();spin.grab_focus();pump(50);
        let text = spin.text();
        set("curve_1", EffectValue::Curve(vec![[0., 0.], [0.5, 0.25], [1., 1.]]));
        let checkpoint = ui_session(&w).engine().checkpoint();
        until(|| state(&w).tonal_histogram.data.as_ref().is_some_and(|data| **data != *initial), "channel edit publishes new embedded statistics");
        let updated = tonal_completed(&w);
        assert_ne!(*updated, *initial, "RGB page reads the nonneutral channel stages");
        assert_eq!(descendant::<gtk::SpinButton>(&histogram_widget::<gtk::Widget>(&w, "property-curve_0-output")).unwrap(), spin);
        assert_eq!(spin.text(), text);
        assert!(spin.has_focus() || descendant::<gtk::Text>(&spin).unwrap().has_focus(), "statistics publication preserves numeric focus");
        assert_eq!(ui_session(&w).engine().checkpoint(), checkpoint, "statistics do not enter undo history");
        for page in 1..=3 {
            choose(&w, &mut input, "properties-page", page);
            let data = tonal_completed(&w);assert_eq!(state(&w).tonal_histogram.channel, page as u8);
            assert_eq!(data.domain, HistogramDomain::Encoded);
            if page == 1 {assert_ne!(*data, *updated, "red page reads before its own channel stage");}
        }
        choose(&w, &mut input, "properties-page", 0);
        choose(&w, &mut input, "property-domain", 1);
        assert!(matches!(tonal_completed(&w).domain, HistogramDomain::CurveLog { .. }));
        let before = super::place_source::snapshot(&w);
        let shadows = histogram_widget::<gtk::CheckButton>(&w, "curve-shadows");
        scroll_to(shadows.upcast_ref());input.click(screen_point(shadows.upcast_ref(), &w.window, [0.5, 0.5]));
        assert!(state(&w).histogram.shadows);assert_eq!(super::place_source::snapshot(&w), before);
        crate::snapshot(&w).save_to_png(std::path::Path::new(&output).join(format!("curves-log-{width}-{theme:?}.png"))).unwrap();
        input.click(screen_point(shadows.upcast_ref(), &w.window, [0.5, 0.5]));
        let highlights = histogram_widget::<gtk::CheckButton>(&w, "curve-highlights");
        scroll_to(highlights.upcast_ref());input.click(screen_point(highlights.upcast_ref(), &w.window, [0.5, 0.5]));
        assert!(state(&w).histogram.highlights);assert_eq!(super::place_source::snapshot(&w), before);
        input.click(screen_point(highlights.upcast_ref(), &w.window, [0.5, 0.5]));
        w.customize(CustomizationAction::SetPanelVisible { panel: Panel::Properties, visible: false });
        until(|| state(&w).tonal_histogram.data.is_none(), "hidden Properties retires embedded statistics");
        assert_eq!(super::place_source::snapshot(&w), before);
        w.customize(CustomizationAction::SetPanelVisible { panel: Panel::Properties, visible: true });
        w.dispatch(UiAction::Customize { action: CustomizationAction::CloseExpanded });
        tonal_completed(&w);
        choose(&w, &mut input, "property-domain", 0);
        set("curve_1", EffectValue::Curve(vec![[0., 0.], [1., 1.]]));tonal_completed(&w);
        crate::snapshot(&w).save_to_png(std::path::Path::new(&output).join(format!("curves-encoded-{width}-{theme:?}.png"))).unwrap();
    }
    input.finish();w.window.close();pump(100);
}
