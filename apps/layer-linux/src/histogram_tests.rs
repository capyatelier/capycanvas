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
    until(|| state(w).histogram.status == w.localization().text(layer_ui::MessageId::RESOURCES_HISTOGRAM_EXACT) && state(w).histogram.data.is_some(), "exact histogram");
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
        || state(w).tonal_histogram.status == w.localization().text(layer_ui::MessageId::RESOURCES_HISTOGRAM_EXACT) && state(w).tonal_histogram.data.is_some(), "exact embedded Curves histogram")));
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

#[test]
#[ignore = "private Wayland display, hardware GPU and retained histogram language controls"]
fn native_histogram_live_language() {
    use layer_ui::{HistogramAction, MessageId, PreferenceAction, PreferenceId, PreferenceValue};
    let output = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap());
    let (application, active) = crate::application("art.capycanvas.HistogramLanguages");
    let app = NativeTestApp(application);
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    let mut project = fixture();
    project.document.color.depth = SampleDepth::F32;
    project.document.blend_space = layer_core::BlendSpace::Linear;
    std::sync::Arc::make_mut(project.document.layers[0].source.as_mut().unwrap()).interpretation.profile =
        ColorProfile::Icc(layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap().into());
    crate::open_workspace(&app, &active, Some((project, None)), None);
    until(|| !active.borrow().is_empty(), "prepared histogram language window");
    let w = active.borrow().last().unwrap().clone();
    w.window.maximize();w.window.present();ready(&w);
    let width = w.window.width();assert!(matches!(width, 640 | 1100), "private Histogram viewport");
    let recording_button=widgets(w.effects.stats.upcast_ref()).find(|widget|widget.widget_name()=="stroke-recording").unwrap().downcast::<gtk::Button>().unwrap();
    let mut input = RemoteInput::new().settle_ms(100).timeout_secs(30);input.ready();
    for panel in Panel::ALL.into_iter().filter(|panel| !matches!(panel, Panel::Toolbar | Panel::Commands | Panel::Histogram)) {
        w.customize(CustomizationAction::SetPanelVisible { panel, visible: false });
    }
    w.dispatch(UiAction::Customize { action:CustomizationAction::CloseExpanded });
    invoke(&w, CommandId::Histogram);
    w.dispatch(UiAction::MovePanel { panel:Panel::Histogram, target:DockTarget::Edge { edge:Edge::Right, outer:false }, viewport:[width as f32,800.] });
    pump(200);completed(&w);
    let source = histogram_widget::<gtk::DropDown>(&w, "histogram-source");
    let channel = histogram_widget::<gtk::DropDown>(&w, "histogram-channel");
    let models = [source.model().unwrap(), channel.model().unwrap()];
    let status = histogram_widget::<gtk::Label>(&w, "histogram-status");
    let details = histogram_widget::<gtk::Expander>(&w,"histogram-details");
    scroll_to(details.upcast_ref());input.click(screen_point(details.upcast_ref(),&w.window,[0.5,0.5]));
    until(||details.is_expanded(),"native histogram Details expansion");
    let description = histogram_widget::<gtk::Label>(&w, "histogram-description");
    let range = histogram_widget::<gtk::Label>(&w, "histogram-range");
    let chart = histogram_widget::<gtk::DrawingArea>(&w, "histogram-chart");
    let options = ["histogram-log", "histogram-shadows", "histogram-highlights"]
        .map(|name| histogram_widget::<gtk::CheckButton>(&w, name));
    let switch = |language| {
        let index = 1 + layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|candidate| *candidate == language).unwrap() as u32;
        w.dispatch(UiAction::Preferences { action:PreferenceAction::Edit { id:PreferenceId::Language, value:PreferenceValue::Choice(index) } });
        until(|| w.localization().language() == language, "retained histogram language");
    };
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme:Some(theme) });ready(&w);
        choose(&w, &mut input, "histogram-source", 3);
        until(|| state(&w).histogram.status == w.localization().text(MessageId::RESOURCES_HISTOGRAM_UNAVAILABLE), "empty selection histogram status");
        let unavailable_checkpoint = ui_session(&w).engine().checkpoint();
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            switch(language);
            assert_eq!(status.text(), w.localization().text(MessageId::RESOURCES_HISTOGRAM_UNAVAILABLE).as_ref());
            assert!(state(&w).histogram.data.is_none());
            assert_eq!(source.selected(), 3);
            assert_eq!(ui_session(&w).engine().checkpoint(), unavailable_checkpoint);
        }
        switch(layer_ui::UiLanguage::English);
        input.click(screen_point(channel.upcast_ref(), &w.window, [0.5,0.5]));
        until(|| widgets(channel.upcast_ref()).any(|widget| widget.is::<gtk::Popover>() && widget.is_mapped()), "retained histogram native choice popup");
        let choice=1+layer_ui::localization::SHIPPED_LANGUAGES.iter().position(|language| *language==layer_ui::UiLanguage::Turkish).unwrap() as u32;
        w.dispatch(UiAction::Preferences {action:PreferenceAction::Edit {id:PreferenceId::Language,value:PreferenceValue::Choice(choice)}});
        pump(120);assert_eq!(w.localization().language(),layer_ui::UiLanguage::English,"native histogram popup defers publication");
        let selected=models[1].item(channel.selected()).unwrap().downcast::<gtk::StringObject>().unwrap().string();
        let option=widgets(channel.upcast_ref()).find(|widget| widget.is_mapped()
            && widget.native().is_some_and(|native| native.is::<gtk::Popover>())
            && widget.downcast_ref::<gtk::Label>().is_some_and(|label| label.text()==selected)).unwrap();
        input.click(screen_point(&option,&w.window,[0.5,0.5]));
        until(|| w.localization().language()==layer_ui::UiLanguage::Turkish,"histogram native choice publication boundary");
        assert_eq!(channel.model().unwrap(),models[1]);
        invoke(&w, CommandId::SelectAll);ready(&w);
        assert!(ui_session(&w).engine().document().selection.is_some());
        for selected_source in 0..=3 {
            choose(&w, &mut input, "histogram-source", selected_source);
            choose(&w, &mut input, "histogram-channel", 4);
            for action in [HistogramAction::Logarithmic {enabled:true}, HistogramAction::Shadows {enabled:true}, HistogramAction::Highlights {enabled:true}] {
                w.dispatch(UiAction::Histogram {action});
            }
            completed(&w);pump(150);
            let captured = state(&w).histogram;
            let data = captured.data.as_ref().unwrap().clone();
            let checkpoint = ui_session(&w).engine().checkpoint();
            let persisted = super::place_source::snapshot(&w);
            for &language in layer_ui::localization::SHIPPED_LANGUAGES {
                switch(language);
                let view = state(&w).histogram;
                assert!(std::sync::Arc::ptr_eq(view.data.as_ref().unwrap(), &data), "language must retain prepared histogram data");
                assert_eq!((view.captured_time,&view.captured_source), (captured.captured_time,&captured.captured_source));
                assert_eq!((view.source,view.channel,view.logarithmic,view.shadows,view.highlights), (selected_source as u8,4,true,true,true));
                assert_eq!(source.model().unwrap(),models[0]);assert_eq!(channel.model().unwrap(),models[1]);
                assert_eq!((source.selected(),channel.selected()), (selected_source,4));
                assert_eq!(histogram_widget::<gtk::DrawingArea>(&w,"histogram-chart"),chart);
                assert_eq!(histogram_widget::<gtk::Label>(&w,"histogram-status"),status);
                assert_eq!(status.text(),w.localization().text(MessageId::RESOURCES_HISTOGRAM_EXACT).as_ref());
                assert_eq!(description.text(),view.description);assert_eq!(range.text(),view.range);
                for (dropdown,labels) in [(&source,&view.sources),(&channel,&view.channels)] {
                    let model=dropdown.model().unwrap().downcast::<gtk::StringList>().unwrap();
                    assert_eq!(model.n_items(),labels.len() as u32);
                    for (index,label) in labels.iter().enumerate() {assert_eq!(model.string(index as u32).unwrap(),label.as_ref());}
                }
                for (index,option) in options.iter().enumerate() {
                    assert!(option.is_active());assert_eq!(option.label().as_deref(),Some(view.labels[index].as_ref()));
                }
                assert_eq!(ui_session(&w).engine().checkpoint(),checkpoint);
                assert_eq!(super::place_source::snapshot(&w),persisted,"source ICC bytes, selection, artwork and undo state survive histogram publication");
                save_snapshot(&w,50,||output.join(format!("histogram-{selected_source}-{}-{theme:?}.png",language.tag())));
            }
        }
        invoke(&w,CommandId::Undo);ready(&w);
        assert!(ui_session(&w).engine().document().selection.is_none());
    }
    w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Histogram,visible:false});
    w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Properties,visible:true});
    w.dispatch(UiAction::Customize {action:CustomizationAction::CloseExpanded});
    w.dispatch(UiAction::MovePanel {panel:Panel::Properties,target:DockTarget::Edge {edge:Edge::Right,outer:false},viewport:[width as f32,800.]});
    w.dispatch(UiAction::Effect {action:layer_ui::EffectAction::Insert {effect:"curves".into()}});ready(&w);
    let layer=state(&w).layer_properties.layer.unwrap();
    w.dispatch(UiAction::Effect {action:layer_ui::EffectAction::Set {layer,key:"curve_0".into(),value:layer_core::EffectValue::Curve(vec![[0.,0.],[0.5,0.5],[1.,1.]])}});
    choose(&w,&mut input,"properties-page",0);choose(&w,&mut input,"property-domain",0);
    let graph=histogram_widget::<gtk::DrawingArea>(&w,"property-curve_0-graph");
    scroll_to(graph.upcast_ref());input.click(screen_point(graph.upcast_ref(),&w.window,[0.5,0.5]));
    let field=histogram_widget::<gtk::Widget>(&w,"property-curve_0-output");
    let spin=descendant::<gtk::SpinButton>(&field).unwrap();
    let actions:Vec<_>=widgets(w.effects.properties.upcast_ref())
        .filter(|widget|widget.is_mapped() && widget.widget_name()=="property-picker")
        .map(|widget|widget.downcast::<gtk::Button>().unwrap()).collect();
    assert!(!actions.is_empty(),"actual Target adjustment action");
    let page=histogram_widget::<gtk::DropDown>(&w,"properties-page");let page_model=page.model().unwrap();
    let domain=histogram_widget::<gtk::DropDown>(&w,"property-domain");let domain_model=domain.model().unwrap();
    let statistics=histogram_widget::<gtk::Label>(&w,"curve-statistics");
    let clipping=["curve-shadows","curve-highlights"].map(|name|histogram_widget::<gtk::CheckButton>(&w,name));
    let scroller=w.effects.properties.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
    let wrapped_available_width=scroller.width();
    let assert_bounds=|| {
        let viewport=scroller.width() as f32;
        assert!(w.effects.properties.measure(gtk::Orientation::Horizontal,-1).0<=wrapped_available_width,"wrapped Properties minimum fits its retained available width");
        let properties=w.effects.properties.compute_bounds(&scroller).unwrap();
        eprintln!("GTK Properties {}: viewport {}, minimum {}, natural {}, bounds {:?}, horizontal adjustment {} / {} / {}",w.localization().language().tag(),viewport,
            w.effects.properties.measure(gtk::Orientation::Horizontal,-1).0,w.effects.properties.measure(gtk::Orientation::Horizontal,-1).1,
            properties,scroller.hadjustment().value(),scroller.hadjustment().upper(),scroller.hadjustment().page_size());
        assert!(properties.x()>=-1. && properties.x()+properties.width()<=viewport+1.,"Properties content fits its native viewport: {properties:?}, {viewport}");
        let controls=actions.iter().map(|button|button.clone().upcast::<gtk::Widget>())
            .chain([graph.clone().upcast(),spin.clone().upcast(),page.clone().upcast(),domain.clone().upcast()])
            .chain(clipping.iter().map(|button|button.clone().upcast()));
        for widget in controls {
            let bounds=widget.compute_bounds(&scroller).unwrap();
            assert!(bounds.x()>=-1. && bounds.x()+bounds.width()<=viewport+1.,"{} fits Properties viewport: {bounds:?}, {viewport}",widget.widget_name());
        }
        for label in widgets(w.effects.properties.upcast_ref()).filter(|widget|widget.is_mapped()).filter_map(|widget|widget.downcast::<gtk::Label>().ok()) {
            let bounds=label.compute_bounds(&scroller).unwrap();
            assert!(bounds.x()>=-1. && bounds.x()+bounds.width()<=viewport+1.,"Properties label fits viewport: {:?}, {bounds:?}, {viewport}",label.text());
            assert!(label.layout().pixel_size().0<=label.width()+1,"Properties glyphs fit label: {:?}, layout {:?}, allocation {}",label.text(),label.layout().pixel_size(),label.width());
        }
    };
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});ready(&w);
        let initial=tonal_completed(&w);
        spin.grab_focus();pump(50);spin.select_region(0,-1);
        let text=spin.text();let selection=spin.selection_bounds();
        let checkpoint=ui_session(&w).engine().checkpoint();let persisted=super::place_source::snapshot(&w);
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            switch(language);
            let view=state(&w).tonal_histogram;
            assert_eq!(widgets(w.effects.stats.upcast_ref()).find(|widget|widget.widget_name()=="stroke-recording").unwrap(),recording_button.clone().upcast::<gtk::Widget>());
            assert_eq!(recording_button.tooltip_text().as_deref(),Some(layer_ui::NativeCopy::new(&w.localization()).color.record_tablet.as_ref()));
            assert!(std::sync::Arc::ptr_eq(view.data.as_ref().unwrap(),&initial));
            assert_eq!(histogram_widget::<gtk::DrawingArea>(&w,"property-curve_0-graph"),graph);
            assert_eq!(descendant::<gtk::SpinButton>(&histogram_widget::<gtk::Widget>(&w,"property-curve_0-output")).unwrap(),spin);
            let current:Vec<_>=widgets(w.effects.properties.upcast_ref())
                .filter(|widget|widget.is_mapped() && widget.widget_name()=="property-picker")
                .map(|widget|widget.downcast::<gtk::Button>().unwrap()).collect();
            assert_eq!(current,actions);assert_eq!(current.len(),state(&w).layer_properties.actions.len());
            for (button,action) in current.iter().zip(&state(&w).layer_properties.actions) {
                assert_eq!(button.label().as_deref(),Some(action.label.as_str()));
                assert_eq!(button.tooltip_text().as_deref(),Some(action.label.as_str()));
            }
            assert_eq!(page.model().unwrap(),page_model);assert_eq!(domain.model().unwrap(),domain_model);
            assert_eq!((page.selected(),domain.selected()),(0,0));
            for dropdown in [&page,&domain] {
                let selected=dropdown.model().and_downcast::<gtk::StringList>().unwrap().string(dropdown.selected()).unwrap();
                let label=widgets(dropdown.upcast_ref()).filter(|widget|widget.is_mapped()).filter_map(|widget|widget.downcast::<gtk::Label>().ok()).find(|label|label.text()==selected).unwrap();
                assert_eq!(label.tooltip_text().as_deref(),Some(selected.as_str()));
            }
            assert_eq!(spin.text(),text);assert_eq!(spin.selection_bounds(),selection);
            assert!(spin.has_focus() || descendant::<gtk::Text>(&spin).unwrap().has_focus());
            assert_eq!(statistics.text(),w.localization().text(MessageId::RESOURCES_HISTOGRAM_EXACT).as_ref());
            for (index,button) in clipping.iter().enumerate() {assert_eq!(button.label().as_deref(),Some(state(&w).histogram.labels[index+1].as_ref()));}
            assert_eq!(ui_session(&w).engine().checkpoint(),checkpoint);assert_eq!(super::place_source::snapshot(&w),persisted);
            if language==UiLanguage::Russian {
                assert_bounds();
                let wrapped_viewport=scroller.width();
                for button in &actions {button.child().and_downcast::<gtk::Label>().unwrap().set_wrap(false);}
                pump(60);
                let minimum=w.effects.properties.measure(gtk::Orientation::Horizontal,-1).0;
                let bounds=w.effects.properties.compute_bounds(&scroller).unwrap();
                eprintln!("GTK Properties nowrap counterfactual: minimum {minimum}, retained wrapped viewport {wrapped_viewport}, actual viewport {}, bounds {:?}, horizontal adjustment {}",scroller.width(),bounds,scroller.hadjustment().value());
                assert!(minimum>wrapped_viewport,"old native nowrap labels exceed the retained available width");
                assert!(scroller.width()>wrapped_viewport || bounds.x()< -1. || bounds.x()+bounds.width()>wrapped_viewport as f32+1.,"actual nowrap labels grow the panel or exceed its retained viewport");
                save_snapshot(&w,50,||output.join(format!("properties-nowrap-counterfactual-{}-{theme:?}.png",language.tag())));
                for button in &actions {button.child().and_downcast::<gtk::Label>().unwrap().set_wrap(true);}
            }
            pump(60);assert_bounds();
            save_snapshot(&w,50,||output.join(format!("curves-histogram-{}-{theme:?}.png",language.tag())));
            if matches!(language,UiLanguage::French|UiLanguage::German) {
                let settings=gtk::Settings::default().unwrap();let font=settings.gtk_font_name();let size=w.window.default_size();
                let maximized=w.window.is_maximized();let initial_width=w.window.width();
                let target_width=initial_width.min(744);
                if maximized {w.window.unmaximize();}
                settings.set_property("gtk-font-name","Sans 16");w.window.set_default_size(target_width,780);
                until(||!w.window.is_maximized() && w.window.width()<=target_width+20 && if initial_width>744 {w.window.width()<initial_width} else {w.window.width()<=initial_width},"actual narrow Properties window");
                pump(200);assert_bounds();
                assert!((target_width-20..=target_width+20).contains(&w.window.width()) && (760..=800).contains(&w.window.height()),"actual narrow Properties allocation {} × {}",w.window.width(),w.window.height());
                assert_eq!(spin.text(),text);assert_eq!(spin.selection_bounds(),selection);
                save_snapshot(&w,50,||output.join(format!("curves-histogram-large-narrow-{}-{theme:?}.png",language.tag())));
                eprintln!("GTK Properties {} {theme:?}: actual {} × {}, Sans 16",language.tag(),w.window.width(),w.window.height());
                settings.set_property("gtk-font-name",font);w.window.set_default_size(size.0,size.1);
                if maximized {w.window.maximize();}
                pump(120);
            }
        }
    }
    input.finish();w.window.close();pump(100);
}
