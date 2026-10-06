use super::new_photo::{invoke, ready};
use super::*;
use layer_core::{
    Document,
    color::{source::*, *},
};

fn fixture() -> Document {
    let mut p = new_drawing(64, 16, &layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap();
    composition_mut(&mut p).color = DocumentColor {
        space: RgbSpace::DisplayP3,
        depth: SampleDepth::U16,
    };
    let paper = *p.scene().order().last().unwrap();
    p.artwork.occurrences.get_mut(paper).unwrap().visible = false;
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
    paint_at_mut(&mut p, 0).base = Some(layer_core::PaintBase::new((std::sync::Arc::new(source.finish().unwrap())).into()));
    let owner = p.scene().order()[0];
    let coverage = p.artwork.coverage.next_handle();
    let mut mask = layer_core::CoverageSnapshot::reveal_all(coverage, [64, 16], Point::default());
    mask.source.default_coverage = 0.5;
    p.artwork.coverage.insert(layer_core::authored::PortableId::random(), mask.source).unwrap();
    let occurrence = p.artwork.occurrences.get_mut(owner).unwrap();
    occurrence.reference = true;
    occurrence.mask = Some(mask.use_);
    let working = p.working.clone();
    p = layer_core::Document::from_artwork(p.artwork).unwrap();
    p.working = working;
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
    let mut point=screen_point(&item,&w.window,[0.5,0.5]);
    let mut parent=item.native().and_downcast::<gtk::Popover>().and_then(|popup|popup.surface()).and_downcast::<gtk::gdk::Popup>().and_then(|popup|popup.parent());
    while let Some(popup)=parent.and_downcast::<gtk::gdk::Popup>() {
        point[0]+=popup.position_x() as f32;point[1]+=popup.position_y() as f32;parent=popup.parent();
    }
    input.click(point);
    assert_eq!(dropdown.selected(), index, "native {name} choice {label}; histogram source/channel {}/{}, waveform source/channel {}/{}", state(w).histogram.source, state(w).histogram.channel, state(w).waveform.source, state(w).waveform.channel);
}

fn precision_label(w:&Workspace,output:&std::path::Path,prefix:&str,width:i32,theme:Theme) {
    pump(100);let label=histogram_widget::<gtk::Label>(w,&format!("{prefix}-status"));
    let (_,natural,_,_)=label.measure(gtk::Orientation::Horizontal,-1);let layout=label.layout();
    let preview=w.localization().text(layer_ui::MessageId::RESOURCES_HISTOGRAM_PREVIEW);let preview_width=label.create_pango_layout(Some(&preview)).pixel_size().0;
    let footer=label.parent().unwrap();let children=widgets(&footer).map(|widget| {let (minimum,natural,_,_)=widget.measure(gtk::Orientation::Horizontal,-1);let bounds=widget.compute_bounds(&footer).unwrap();serde_json::json!({"name":widget.widget_name().to_string(),"type":widget.type_().name(),"css":widget.css_classes().iter().map(|class|class.to_string()).collect::<Vec<_>>(),"bounds":[bounds.x(),bounds.y(),bounds.width(),bounds.height()],"minimum_width":minimum,"natural_width":natural})}).collect::<Vec<_>>();
    let measurement=serde_json::json!({"text":label.text().to_string(),"allocated_width":label.width(),"natural_width":natural,"layout_pixels":layout.pixel_size(),"ellipsized":layout.is_ellipsized(),"preview_text":preview,"preview_width":preview_width,"footer_width":footer.width(),"widgets":children});
    std::fs::write(output.join(format!("{prefix}-precision-{width}-{theme:?}.json")),serde_json::to_vec_pretty(&measurement).unwrap()).unwrap();
    assert!(label.is_mapped() && !layout.is_ellipsized(),"precision status must be fully readable: {measurement}");
    assert!(label.width()>=preview_width,"Preview precision must fit the same status allocation: {measurement}");
    let panel=histogram_widget::<gtk::Box>(w,&format!("{prefix}-panel"));let bounds=footer.compute_bounds(&panel).unwrap();assert!(bounds.x()>=-1. && bounds.x()+bounds.width()<=panel.width() as f32+1.,"footer remains inside panel: {measurement}");let bounds=panel.compute_bounds(&w.window).unwrap();assert!(bounds.x()>=-1. && bounds.x()+bounds.width()<=w.window.width() as f32+1.,"precision minimum does not overflow the private window: {measurement}");
    let kind=if prefix=="waveform" {Panel::Waveform} else {Panel::Histogram};let group=w.groups.borrow().iter().find(|group|group.panels.contains(&kind) && group.root.is_mapped()).unwrap().root.clone();let bounds=label.compute_bounds(&group).unwrap();assert!(bounds.x()>=-1. && bounds.y()>=-1. && bounds.x()+bounds.width()<=group.width() as f32+1. && bounds.y()+bounds.height()<=group.height() as f32+1.,"precision status is not clipped by its dock group: {bounds:?} in {}x{}; {measurement}",group.width(),group.height());
    let view=state(w);let expected=if prefix=="waveform" {view.waveform.status.as_ref()} else {view.histogram.status.as_ref()};assert_eq!(label.text().as_str(),expected);
}

pub(super) fn monitor_bounds(w: &Workspace, panel: Panel) {
    pump(50);
    let prefix = if panel == Panel::Waveform {"waveform"} else {"histogram"};
    let root = histogram_widget::<gtk::Box>(w, &format!("{prefix}-panel"));
    let group = w.groups.borrow().iter().find(|group| group.panels.contains(&panel)
        && group.root.is_mapped()).expect("mapped monitor group").root.clone();
    for widget in widgets(root.upcast_ref()).filter(|widget| widget.is_mapped()) {
        let bounds = widget.compute_bounds(&group).unwrap();
        assert!(bounds.x() >= -1. && bounds.y() >= -1.
            && bounds.x() + bounds.width() <= group.width() as f32 + 1.
            && bounds.y() + bounds.height() <= group.height() as f32 + 1.,
            "{} {} fits {}x{} monitor group in {}: {bounds:?}", prefix, widget.widget_name(),
            group.width(), group.height(), w.localization().language().tag());
        if let Some(label) = widget.downcast_ref::<gtk::Label>() {
            assert!(label.layout().pixel_size().0 <= label.width() + 1,
                "{} glyphs fit allocated label in {}: {:?}, layout {:?}, allocation {}",
                prefix, w.localization().language().tag(), label.text(), label.layout().pixel_size(), label.width());
        }
    }
    let chart = histogram_widget::<gtk::DrawingArea>(w, &format!("{prefix}-chart"));
    let bounds = chart.compute_bounds(&root).unwrap();
    assert!(bounds.x() >= -1. && bounds.x() + bounds.width() <= root.width() as f32 + 1.,
        "complete {prefix} plot including shadow bins fits native panel: {bounds:?}, {}", root.width());
    let status = histogram_widget::<gtk::Label>(w, &format!("{prefix}-status"));
    if [layer_ui::MessageId::RESOURCES_HISTOGRAM_EXACT, layer_ui::MessageId::RESOURCES_HISTOGRAM_PREVIEW]
        .into_iter().any(|message| status.text().as_str() == w.localization().text(message).as_ref()) {
        assert!(!status.layout().is_ellipsized(), "{prefix} precision is readable: {:?}", status.text());
    }
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
    let output = artifact_dir("../../artifacts/photo-editing-color/compact-ui/gtk");
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
        let chart = histogram_widget::<gtk::DrawingArea>(&w, "histogram-chart");assert!(chart.height() >= 120 && chart.width() as f32 >= w.histogram.root.width() as f32 * 0.8);
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
        until(|| super::photo_edit::document(&w).working.selection.is_some(), "native rectangle selection");
        choose(&w, &mut input, "histogram-source", 3);
        let selected = completed(&w);
        assert!(selected.pixels > 0 && selected.pixels < initial.pixels);
        assert_eq!(selected.transparent, 0);
        w.dispatch(UiAction::Invoke { command: CommandId::Undo });
        choose(&w, &mut input, "histogram-source", 0);
        assert_eq!(*completed(&w), *initial);
        before = super::place_source::snapshot(&w);
        let unmarked = super::photo_edit::shown(&w, [8., 8.]);
        let shadows = histogram_widget::<gtk::ToggleButton>(&w, "histogram-shadows");
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
        let effect = document.working.occurrence.and_then(|h| document.scene().effect(h));
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
    let mut project = fixture();composition_mut(&mut project).color.depth = SampleDepth::F32;composition_mut(&mut project).blend = layer_core::BlendSpace::Linear;
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
    set("rgb", EffectValue::Curve(vec![[0., 0.], [0.5, 0.5], [1., 1.]]));
    let output = artifact_dir("../../artifacts/photo-editing-color/compact-ui/gtk");
    std::fs::create_dir_all(&output).unwrap();
    for theme in [Theme::Light, Theme::Dark] {
        w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        choose(&w, &mut input, "properties-page", 0);
        choose(&w, &mut input, "property-domain", 0);
        let initial = tonal_completed(&w);assert_eq!(initial.domain, HistogramDomain::Encoded);
        let graph = histogram_widget::<gtk::DrawingArea>(&w, "property-rgb-graph");
        scroll_to(graph.upcast_ref());input.click(screen_point(graph.upcast_ref(), &w.window, [0.5, 0.5]));
        let field = histogram_widget::<gtk::Widget>(&w, "property-rgb-output");
        let control = field.clone().downcast::<crate::number_control::NumberControl>().unwrap();
        let display = find_css(control.upcast_ref(), "number-value").unwrap();input.click(screen_point(&display, &w.window, [0.5, 0.5]));
        let entry = descendant::<gtk::Entry>(&control).unwrap();let scroll=widgets(w.effects.properties.upcast_ref()).find_map(|widget|widget.downcast::<gtk::ScrolledWindow>().ok());let horizontal=scroll.as_ref().map(|scroll|scroll.hadjustment().value());entry.set_text("0.12345678901234567890123456789");let text = entry.text();pump(150);
        let focus = gtk::prelude::RootExt::focus(&w.window);
        for widget in widgets(w.effects.properties.upcast_ref()).filter(|widget|widget.is_mapped()) {
            let bounds=widget.compute_bounds(&w.window).unwrap();let (minimum,natural,_,_)=widget.measure(gtk::Orientation::Horizontal,-1);
            if minimum>100 || widget==control || widget==entry {eprintln!("HDR layout type={} name={} css={:?} min={} natural={} bounds={:?} entry={:?}",widget.type_().name(),widget.widget_name(),widget.css_classes(),minimum,natural,bounds,widget.downcast_ref::<gtk::Entry>().map(|entry|(entry.width_chars(),entry.max_width_chars())));}
        }
        crate::snapshot(&w).save_to_png(std::path::Path::new(output).join(format!("curves-long-draft-{}-{theme:?}.png",w.window.width()))).unwrap();
        assert!(graph.width() >= 64 && graph.height() >= 200, "compact curve keeps its drawable graph visible");
        let plot=graph.parent().unwrap();let plot_bounds=graph.compute_bounds(&plot).unwrap();
        assert!((plot_bounds.x()+plot_bounds.width()-plot.width() as f32).abs()<=1.,"curve fills its available plot column: {plot_bounds:?}, {}",plot.width());
        let group=w.groups.borrow().iter().find(|group|group.panels.contains(&Panel::Properties) && group.root.is_mapped()).unwrap().root.clone();
        let bounds=plot.compute_bounds(&group).unwrap();assert!(bounds.x()>=-1. && bounds.x()+bounds.width()<=group.width() as f32+1.,"curve axes and plot fit visible Properties: {bounds:?}, {}",group.width());
        let graph_bounds=graph.compute_bounds(&w.window).unwrap();assert!(graph_bounds.x()>=0. && graph_bounds.x()+graph_bounds.width()<=w.window.width() as f32,"HDR draft cannot overflow the graph");if let Some(scroll)=scroll {assert_eq!(Some(scroll.hadjustment().value()),horizontal,"long exact draft scrolls within its editor, not the panel");}
        set("red", EffectValue::Curve(vec![[0., 0.], [0.5, 0.25], [1., 1.]]));
        let checkpoint = ui_session(&w).engine().checkpoint();
        until(|| state(&w).tonal_histogram.data.as_ref().is_some_and(|data| **data != *initial), "channel edit publishes new embedded statistics");
        let updated = tonal_completed(&w);
        assert_ne!(*updated, *initial, "RGB page reads the nonneutral channel stages");
        assert_eq!(histogram_widget::<gtk::Widget>(&w, "property-rgb-output"), field);
        assert_eq!(entry.text(), text);
        assert_eq!(gtk::prelude::RootExt::focus(&w.window), focus, "statistics publication preserves numeric focus");
        input.key(0xff1b);
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
        let controls=state(&w).layer_properties.controls;let control=controls.iter().find(|control|control.key=="rgb").unwrap();let curve=control.curve.as_ref().unwrap();let EffectValue::Curve(points)=&control.value else {panic!("curve value")};let point=points[1];let graph=histogram_widget::<gtk::DrawingArea>(&w,"property-rgb-graph");scroll_to(graph.upcast_ref());input.click(screen_point(graph.upcast_ref(),&w.window,[curve.domain.encode(point[0] as f64) as f32,1.-curve.domain.encode(point[1] as f64) as f32]));pump(150);
        assert!(state(&w).layer_properties.controls.iter().find(|control|control.key=="rgb").unwrap().curve.as_ref().unwrap().output.as_ref().unwrap().ev.is_some());

        for widget in widgets(w.effects.properties.upcast_ref()).filter(|widget|widget.is_mapped()) {
            let (minimum,natural,_,_)=widget.measure(gtk::Orientation::Horizontal,-1);
            if minimum>100 || widget.downcast_ref::<gtk::Label>().is_some() {eprintln!("HDR Log type={} name={} min={} natural={} bounds={:?} text={:?}",widget.type_().name(),widget.widget_name(),minimum,natural,widget.compute_bounds(&w.window),widget.downcast_ref::<gtk::Label>().map(|label|label.text()));}
        }

        let before = super::place_source::snapshot(&w);
        let shadows = histogram_widget::<gtk::ToggleButton>(&w, "curve-shadows");
        scroll_to(shadows.upcast_ref());input.click(screen_point(shadows.upcast_ref(), &w.window, [0.5, 0.5]));
        assert!(state(&w).histogram.shadows);assert_eq!(super::place_source::snapshot(&w), before);
        crate::snapshot(&w).save_to_png(std::path::Path::new(&output).join(format!("curves-log-{width}-{theme:?}.png"))).unwrap();
        input.click(screen_point(shadows.upcast_ref(), &w.window, [0.5, 0.5]));
        let highlights = histogram_widget::<gtk::ToggleButton>(&w, "curve-highlights");
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
        set("red", EffectValue::Curve(vec![[0., 0.], [1., 1.]]));tonal_completed(&w);
        crate::snapshot(&w).save_to_png(std::path::Path::new(&output).join(format!("curves-encoded-{width}-{theme:?}.png"))).unwrap();
    }
    input.finish();w.window.close();pump(100);
}

#[allow(deprecated)]
pub(super) fn photo_workspace(app: &NativeTestApp) -> Rc<Workspace> {
    let w=Workspace::with_project(app,Some((new_drawing(512,512,&layer_ui::Localizer::shared(layer_ui::UiLanguage::English)).unwrap(),None)));
    w.window.maximize();w.window.present();ready(&w);
    let weak=Rc::downgrade(&w);
    *w.open_document.borrow_mut()=Some(Rc::new(move |project,location| {if let Some(w)=weak.upgrade() {w.documents.enqueue(&w,(project,location));}}));
    invoke(&w,CommandId::OpenDocument);let dialog=super::new_photo::chooser();
    let photo=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/photo-editing-color/g2-inputs/portrait.png");
    dialog.set_file(&gtk::gio::File::for_path(photo)).unwrap();pump(200);dialog.response(gtk::ResponseType::Accept);
    super::new_photo::finish(&w);ready(&w);
    assert!(super::photo_edit::document(&w).artwork.paint.iter().any(|(_,_,paint)|paint.base.is_some()));w
}

#[test]
#[ignore = "private compositor, hardware GPU and native input"]
fn native_compact_graphs_photo_review() {
    let app=native_test_app("art.capycanvas.CompactGraphs");let w=photo_workspace(&app);
    let width=w.window.width();assert!(matches!(width,640|1100));
    let output=std::path::PathBuf::from(artifact_dir("../../artifacts/photo-editing-color/compact-ui/gtk"));
    let mut input=RemoteInput::new().settle_ms(150).timeout_secs(30);input.ready();
    for panel in Panel::ALL {w.customize(CustomizationAction::SetPanelVisible {panel,visible:matches!(panel,Panel::Toolbar|Panel::Commands|Panel::Properties)});}
    w.customize(CustomizationAction::CloseExpanded);w.dispatch(UiAction::MovePanel {panel:Panel::Properties,target:DockTarget::Edge {edge:Edge::Right,outer:false},viewport:[width as f32,800.]});
    invoke(&w,CommandId::FitCanvas);let original=super::photo_edit::document(&w);
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});ready(&w);
        for effect in ["levels","curves","white_balance","color_lookup"] {
            w.dispatch(UiAction::Effect {action:layer_ui::EffectAction::Insert {effect:effect.into()}});ready(&w);pump(200);
            if effect=="curves" {
                let graph=histogram_widget::<gtk::DrawingArea>(&w,"property-rgb-graph");let bounds=graph.compute_bounds(&w.window).unwrap();assert!(bounds.width()>=200. && bounds.height()>=200.);assert!(bounds.y()>=0. && bounds.y()+bounds.height()<=800.);assert!(bounds.x()>=0. && bounds.x()+bounds.width()<=width as f32,"curve chart remains inside the private viewport");for axis in ["input","output"] {let field=histogram_widget::<gtk::Widget>(&w,&format!("property-rgb-{axis}"));let bounds=field.compute_bounds(&w.window).unwrap();assert!(bounds.x()>=0. && bounds.x()+bounds.width()<=width as f32,"paired coordinate {axis} remains visible");}
                input.perform(serde_json::json!([{"point":screen_point(graph.upcast_ref(),&w.window,[0.5,0.5]),"down":true},{"point":screen_point(graph.upcast_ref(),&w.window,[0.5,0.65])},{"down":false}]));ready(&w);
            }
            if effect=="color_lookup" {
                let cube=output.join("photo-review.cube");std::fs::write(&cube,super::pointwise::lookup_cube("Inverse gradient",false)).unwrap();super::pointwise::import_lookup(&w,Some(&cube),&mut input);let choice=named::<gtk::DropDown>(w.effects.properties.upcast_ref(),"property-resource-choice");assert_eq!(choice.selected_item().unwrap().downcast::<gtk::StringObject>().unwrap().string(),"Inverse gradient");crate::snapshot(&w).save_to_png(output.join(format!("photo-lookup-loaded-{width}-{theme:?}.png"))).unwrap();
                let title="夕空の色彩調整と深い青の階調".repeat(8);std::fs::write(&cube,super::pointwise::lookup_cube(&title,true)).unwrap();super::pointwise::import_lookup(&w,Some(&cube),&mut input);assert_eq!(named::<gtk::DropDown>(w.effects.properties.upcast_ref(),"property-resource-choice").selected_item().unwrap().downcast::<gtk::StringObject>().unwrap().string(),title);
            }
            if matches!(effect,"levels"|"curves") {
                crate::snapshot(&w).save_to_png(output.join(format!("photo-{effect}-{width}-{theme:?}.png"))).unwrap();
                let menu=named::<gtk::MenuButton>(w.effects.properties.upcast_ref(),"property-picker-menu");assert!(menu.is_mapped());let actions=state(&w).layer_properties.actions;let before_focus=gtk::prelude::RootExt::focus(&w.window);let bounds=menu.compute_bounds(&w.window).unwrap();assert!(bounds.x()>=0. && bounds.x()+bounds.width()<=width as f32,"grouped picker is fully inside the panel");let point=screen_point(menu.upcast_ref(),&w.window,[0.5,0.5]);let hit=w.window.pick(point[0] as f64,point[1] as f64,gtk::PickFlags::DEFAULT).unwrap();assert!(hit==menu || hit.is_ancestor(&menu),"native grouped picker contact hits {hit:?}");input.click(screen_point(menu.upcast_ref(),&w.window,[0.5,0.5]));assert_eq!(named::<gtk::MenuButton>(w.effects.properties.upcast_ref(),"property-picker-menu"),menu,"graph focus-leave retains grouped picker widget");assert!(menu.root().is_some());
                let opened=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||until(||menu.popover().is_some_and(|p|p.is_mapped()),"compact calibration choices")));if let Err(error)=opened {crate::snapshot(&w).save_to_png(output.join(format!("menu-failed-{effect}-{width}-{theme:?}.png"))).unwrap();eprintln!("compact menu timeout: old mapped {} parent {:?} focus before {:?} after {:?}, actions before {:?} after {:?}",menu.is_mapped(),menu.parent(),before_focus,gtk::prelude::RootExt::focus(&w.window),actions,state(&w).layer_properties.actions);std::panic::resume_unwind(error);}
                assert_eq!(widgets(menu.upcast_ref()).filter(|widget|widget.is_mapped() && widget.widget_name()=="property-picker").count(),3);input.key(0xff1b);
            }
            crate::snapshot(&w).save_to_png(output.join(format!("photo-{effect}-{width}-{theme:?}.png"))).unwrap();
            let edited=super::photo_edit::document(&w);for (_,id,paint) in original.artwork.paint.iter() {let current=edited.artwork.paint.get(edited.artwork.paint.resolve(id).unwrap()).unwrap();assert_eq!(current.base,paint.base);assert_eq!(current.raster,paint.raster);}
            w.dispatch(UiAction::Invoke {command:CommandId::DeleteLayer});ready(&w);assert_live_artwork_eq(&super::photo_edit::document(&w),&original);
        }
    }
    input.finish();w.window.destroy();pump(100);
}

#[test]
#[ignore = "private compositor, hardware GPU and native input"]
fn native_waveform_photo_sources_channels_and_layout() {
    let app=native_test_app("art.capycanvas.WaveformReview");let w=photo_workspace(&app);let width=w.window.width();assert!(matches!(width,640|1100));
    let output=std::path::PathBuf::from(artifact_dir("../../artifacts/photo-editing-color/compact-ui/gtk"));let mut input=RemoteInput::new().settle_ms(150).timeout_secs(30);input.ready();
    let before=super::place_source::snapshot(&w);
    let done=|| {until(||state(&w).waveform.status.as_ref()=="Exact" && state(&w).waveform.data.as_ref().is_some_and(|data|data.waveform.is_some()),"exact spatial waveform");state(&w).waveform.data.unwrap()};
    for theme in [Theme::Light,Theme::Dark] {
        w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(layer_ui::WorkspaceState {layout:layer_ui::WorkspacePreset::Photographer.layout(Platform::Gtk),..Default::default()})});
        w.dispatch(UiAction::SetTheme {theme:Some(theme)});ready(&w);invoke(&w,CommandId::FitCanvas);
        let layout=state(&w).workspace.layout;let group=layout.panel_group(Panel::Histogram).unwrap();assert_eq!(group,14);assert_eq!(layout.panel_group(Panel::Waveform),Some(group));
        assert!(w.groups.borrow().iter().any(|group|group.panels==[Panel::Histogram,Panel::Waveform]));assert!(named::<gtk::DrawingArea>(w.window.upcast_ref(),"histogram-chart").is_mapped());completed(&w);
        precision_label(&w,&output,"histogram",width,theme);
        crate::snapshot(&w).save_to_png(output.join(format!("photo-default-scopes-{width}-{theme:?}.png"))).unwrap();
        std::fs::write(output.join(format!("photo-default-measurements-{width}-{theme:?}.json")),serde_json::to_vec_pretty(&serde_json::json!({"layout":state(&w).workspace.layout,"measurements":state(&w).workspace.layout.measurements})).unwrap()).unwrap();

        let menu=named::<gtk::PopoverMenu>(w.window.upcast_ref(),"workspace-menu");menu.popup();pump(150);let model=ui_session(&w).application_menu(layer_ui::ApplicationMenu::Window);let label=model.sections.iter().flatten().find(|item|matches!(item.action,Some(UiAction::Customize {action:CustomizationAction::SetPanelVisible {panel:Panel::Waveform,..}}))).unwrap().label.clone();assert!(menu_action(&menu.menu_model().unwrap(),&label).is_some());menu.popdown();pump(100);
        tab(&w,&mut input,Panel::Waveform);assert_eq!(state(&w).waveform.channel,0);
        let initial=done();precision_label(&w,&output,"waveform",width,theme);assert!(initial.pixels>0);let chart=histogram_widget::<gtk::DrawingArea>(&w,"waveform-chart");assert!((160..=240).contains(&chart.height()));assert!(chart.width()>=histogram_widget::<gtk::Box>(&w,"waveform-panel").width()-2);
        let colors=state(&w).palette.histogram_colors().map(|color|color.0);let mut timings=Vec::new();
        for channel in [0,4] {for logarithmic in [false,true] {
            let mut view=state(&w).waveform;view.channel=channel;view.logarithmic=logarithmic;
            for _ in 0..3 {std::hint::black_box(view.waveform_premultiplied_rgba(colors).unwrap());}
            let mut samples=Vec::new();let mut dimensions=[0,0];
            for _ in 0..20 {let started=std::time::Instant::now();let rgba=std::hint::black_box(view.waveform_premultiplied_rgba(colors).unwrap());samples.push(started.elapsed().as_secs_f64()*1000.);dimensions=rgba.0;}
            samples.sort_by(f64::total_cmp);timings.push(serde_json::json!({"channel":channel,"logarithmic":logarithmic,"dimensions":dimensions,"warmup_iterations":3,"sample_count":20,"min_ms":samples[0],"median_ms":(samples[9]+samples[10])/2.,"p99_ms":samples[19],"samples_ms":samples}));
        }}
        std::fs::write(output.join(format!("waveform-rgba-timing-{width}-{theme:?}.json")),serde_json::to_vec_pretty(&serde_json::json!({"source_pixels":initial.pixels,"reference_tier_qualification":false,"timings":timings})).unwrap()).unwrap();
        choose(&w,&mut input,"waveform-channel",4);pump(200);crate::snapshot(&w).save_to_png(output.join(format!("waveform-luma-{width}-{theme:?}.png"))).unwrap();
        for channel in [0,1,2,3,4] {choose(&w,&mut input,"waveform-channel",channel);assert_eq!(state(&w).waveform.channel,channel as u8);assert_eq!(*done(),*initial);}
        choose(&w,&mut input,"waveform-source",1);assert_eq!(state(&w).waveform.source,1);done();choose(&w,&mut input,"waveform-source",0);
        let log=histogram_widget::<gtk::CheckButton>(&w,"waveform-log");let logarithmic=state(&w).waveform.logarithmic;input.click(screen_point(log.upcast_ref(),&w.window,[0.5,0.5]));assert_eq!(state(&w).waveform.logarithmic,!logarithmic);
        input.click(screen_point(log.upcast_ref(),&w.window,[0.5,0.5]));assert_eq!(state(&w).waveform.logarithmic,logarithmic);
        choose(&w,&mut input,"waveform-channel",0);pump(200);let bounds=chart.compute_bounds(&w.window).unwrap();let texture=crate::snapshot(&w);let mut pixels=vec![0;texture.width() as usize*texture.height() as usize*4];texture.download(&mut pixels,texture.width() as usize*4);let sx=texture.width() as f32/w.window.width() as f32;let sy=texture.height() as f32/w.window.height() as f32;
        let colored=(bounds.y() as u32..(bounds.y()+bounds.height()) as u32).any(|y|(bounds.x() as u32..(bounds.x()+bounds.width()) as u32).any(|x| {let at=((y as f32*sy) as usize*texture.width() as usize+(x as f32*sx) as usize)*4;let pixel=&pixels[at..at+3];pixel.iter().max().unwrap()-pixel.iter().min().unwrap()>20}));assert!(colored,"native RGB waveform plot contains drawn channel traces");
        texture.save_to_png(output.join(format!("waveform-photo-{width}-{theme:?}.png"))).unwrap();std::fs::write(output.join(format!("waveform-layout-{width}-{theme:?}.json")),serde_json::to_vec_pretty(&serde_json::json!({"window":[w.window.width(),w.window.height()],"chart":[bounds.x(),bounds.y(),bounds.width(),bounds.height()],"pixels":initial.pixels})).unwrap()).unwrap();
        w.dispatch(UiAction::MovePanel {panel:Panel::Waveform,target:DockTarget::Float {position:[70.,150.]},viewport:[width as f32,800.]});done();pump(200);
        let group=w.groups.borrow().iter().find(|group|group.floating && group.panels.contains(&Panel::Waveform)).unwrap().root.clone();
        crate::snapshot(&w).save_to_png(output.join(format!("waveform-floating-{width}-{theme:?}.png"))).unwrap();
        for name in ["waveform-source","waveform-channel","waveform-log","waveform-chart","waveform-status","waveform-shadows","waveform-highlights"] {
            let widget=histogram_widget::<gtk::Widget>(&w,name);let bounds=widget.compute_bounds(&group).unwrap();
            assert!(bounds.x()>=-1. && bounds.y()>=-1. && bounds.x()+bounds.width()<=group.width() as f32+1. && bounds.y()+bounds.height()<=group.height() as f32+1.,"floating {name} bounds {bounds:?} within {}x{}",group.width(),group.height());
        }
        let clipping=histogram_widget::<gtk::ToggleButton>(&w,"waveform-shadows");clipping.grab_focus();let focus=gtk::prelude::RootExt::focus(&w.window);let active=state(&w).histogram.shadows;input.key(0x20);assert_eq!(state(&w).histogram.shadows,!active);assert_eq!(gtk::prelude::RootExt::focus(&w.window),focus);input.key(0x20);assert_eq!(state(&w).histogram.shadows,active);
        invoke(&w,CommandId::Histogram);completed(&w);pump(200);crate::snapshot(&w).save_to_png(output.join(format!("histogram-photo-{width}-{theme:?}.png"))).unwrap();let group=state(&w).workspace.layout.panel_group(Panel::Histogram).unwrap();w.dispatch(UiAction::MovePanel {panel:Panel::Waveform,target:DockTarget::Tab {group,index:None},viewport:[width as f32,800.]});pump(200);tab(&w,&mut input,Panel::Histogram);until(||state(&w).waveform.data.is_none(),"inactive Waveform tab releases its plot");assert!(completed(&w).pixels>0);tab(&w,&mut input,Panel::Waveform);done();w.dispatch(UiAction::Histogram {action:layer_ui::HistogramAction::Channel {index:4}});assert_eq!(state(&w).waveform.channel,0,"monitor channels are independent");
        choose(&w,&mut input,"waveform-source",1);choose(&w,&mut input,"waveform-channel",4);let retained=done();
        w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Histogram,visible:false});w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Waveform,visible:false});until(||state(&w).waveform.data.is_none(),"hidden Waveform releases demand");assert_eq!(super::place_source::snapshot(&w),before);
        invoke(&w,CommandId::Waveform);assert_eq!(*done(),*retained);pump(200);
        assert_eq!((state(&w).waveform.source,state(&w).waveform.channel),(1,4));
        let group=w.groups.borrow().iter().find(|group|group.panels.contains(&Panel::Waveform) && group.root.is_mapped()).unwrap().root.clone();
        let bounds=group.compute_bounds(&w.window).unwrap();assert!(group.width() as f32>=Panel::Waveform.default_width(),"reopened Waveform remains usable: {bounds:?}");
        assert!(bounds.x()>=-1. && bounds.y()>=-1. && bounds.x()+bounds.width()<=w.window.width() as f32+1. && bounds.y()+bounds.height()<=w.window.height() as f32+1.,"reopened Waveform stays visible: {bounds:?} in {}x{}",w.window.width(),w.window.height());
        for name in ["waveform-source","waveform-channel","waveform-log","waveform-chart","waveform-status","waveform-shadows","waveform-highlights"] {
            let widget=histogram_widget::<gtk::Widget>(&w,name);let bounds=widget.compute_bounds(&group).unwrap();
            assert!(widget.is_sensitive() && bounds.x()>=-1. && bounds.y()>=-1. && bounds.x()+bounds.width()<=group.width() as f32+1. && bounds.y()+bounds.height()<=group.height() as f32+1.,"reopened {name} remains usable: {bounds:?} in {}x{}",group.width(),group.height());
        }
        precision_label(&w,&output,"waveform",width,theme);crate::snapshot(&w).save_to_png(output.join(format!("waveform-reopened-{width}-{theme:?}.png"))).unwrap();
        choose(&w,&mut input,"waveform-channel",2);assert_eq!(state(&w).waveform.channel,2);choose(&w,&mut input,"waveform-channel",4);
        choose(&w,&mut input,"waveform-source",0);assert_eq!(*done(),*initial);choose(&w,&mut input,"waveform-source",1);assert_eq!(*done(),*retained);
        assert_eq!(super::place_source::snapshot(&w),before);
        w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Waveform,visible:false});until(||state(&w).waveform.data.is_none(),"Waveform closes");
        w.dispatch(UiAction::Histogram {action:layer_ui::HistogramAction::Source {index:0}});w.dispatch(UiAction::Histogram {action:layer_ui::HistogramAction::WaveformChannel {index:0}});
        w.dispatch(UiAction::Histogram {action:layer_ui::HistogramAction::WaveformLogarithmic {enabled:true}});
    }
    input.finish();w.window.destroy();pump(100);
}

#[test]
#[ignore = "private Wayland display and hardware GPU"]
fn native_localized_photo_histogram_bounds() {
    let app = native_test_app("art.capycanvas.PhotoHistogramBounds");
    let output = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let input = RemoteInput::new();input.ready();
    let reported = [layer_ui::UiLanguage::French, layer_ui::UiLanguage::German, layer_ui::UiLanguage::Russian];
    for theme in [Theme::Light, Theme::Dark] {
        for &language in reported.iter().chain(layer_ui::localization::SHIPPED_LANGUAGES.iter().filter(|language| !reported.contains(language))) {
            let w = Workspace::with_project_localized(&app, Some((fixture(), None)), layer_ui::Localizer::shared(language));
            w.dispatch(UiAction::RestoreSettings {settings:layer_ui::Settings {
                theme:Some(theme), language:layer_ui::LanguagePreference::Explicit(language), ..Default::default()
            }});
            w.window.maximize();w.window.present();ready(&w);
            assert_eq!(w.localization().language(), language);
            assert!(matches!(w.window.width(), 640 | 1100), "private Histogram viewport");
            w.dispatch(UiAction::SetTheme {theme:Some(theme)});
            w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(layer_ui::WorkspaceState {
                layout:layer_ui::WorkspacePreset::Photographer.layout(Platform::Gtk), ..Default::default()
            })});
            for panel in [Panel::Histogram, Panel::Waveform] {
                let layout = state(&w).workspace.layout;
                if layout.active_panel(panel) != Some(panel) {
                    let group = layout.panel_group(panel).unwrap();
                    w.dispatch(UiAction::SelectPanelTab {group, panel});
                }
                w.dispatch(UiAction::Customize {action:CustomizationAction::CloseExpanded});ready(&w);
                until(|| {
                    let view = state(&w);
                    let monitor = if panel == Panel::Histogram {view.histogram} else {view.waveform};
                    monitor.data.is_some() && monitor.status == w.localization().text(layer_ui::MessageId::RESOURCES_HISTOGRAM_EXACT)
                }, "exact Photo monitor");
                let view = state(&w);
                let monitor = if panel == Panel::Histogram {view.histogram} else {view.waveform};
                assert!(monitor.data.as_ref().unwrap().pixels > 0);
                if panel == Panel::Histogram {
                    let plot = monitor.histogram_plot();assert!(!plot.is_empty());
                    assert_eq!(plot.iter().flat_map(|(_, bins)| bins.iter()).copied().fold(0_f32, f32::max), 1.);
                } else {
                    let (_, pixels) = monitor.waveform_premultiplied_rgba(view.palette.histogram_colors().map(|color| color.0)).unwrap();
                    assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
                }
                let prefix = if panel == Panel::Histogram {"histogram"} else {"waveform"};
                let logarithmic = histogram_widget::<gtk::CheckButton>(&w, &format!("{prefix}-log"));
                let caption = logarithmic.label().unwrap();
                logarithmic.set_label(Some(""));logarithmic.set_label(Some(&caption));
                monitor_bounds(&w, panel);
                save_snapshot(&w, 50, || output.join(format!("photo-{panel:?}-{}-{theme:?}.png", language.tag())));
            }
            w.window.destroy();pump(100);
        }
    }
    input.finish();
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
    composition_mut(&mut project).color.depth = SampleDepth::F32;
    composition_mut(&mut project).blend = layer_core::BlendSpace::Linear;
    let base=paint_at_mut(&mut project,0).base.as_mut().unwrap();
    let mut image=(*base.image).clone();
    image.interpretation.profile=ColorProfile::Icc(layer_color::profile_bytes(&ColorProfile::Builtin(RgbSpace::DisplayP3)).unwrap().into());
    base.image=std::sync::Arc::new(image).into();
    crate::open_workspace(&app, &active, Some((project, None)));
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
    let chart = histogram_widget::<gtk::DrawingArea>(&w, "histogram-chart");
    let logarithmic = histogram_widget::<gtk::CheckButton>(&w, "histogram-log");
    let histogram_clipping = ["histogram-shadows", "histogram-highlights"]
        .map(|name| histogram_widget::<gtk::ToggleButton>(&w, name));
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
            monitor_bounds(&w, Panel::Histogram);
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
        assert!(ui_session(&w).engine().document().working.selection.is_some());
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
                monitor_bounds(&w, Panel::Histogram);
                let view = state(&w).histogram;
                assert!(std::sync::Arc::ptr_eq(view.data.as_ref().unwrap(), &data), "language must retain prepared histogram data");
                assert_eq!((view.captured_time,&view.captured_source), (captured.captured_time,&captured.captured_source));
                assert_eq!((view.source,view.channel,view.logarithmic,view.shadows,view.highlights), (selected_source as u8,4,true,true,true));
                assert_eq!(source.model().unwrap(),models[0]);assert_eq!(channel.model().unwrap(),models[1]);
                assert_eq!((source.selected(),channel.selected()), (selected_source,4));
                assert_eq!(histogram_widget::<gtk::DrawingArea>(&w,"histogram-chart"),chart);
                assert_eq!(histogram_widget::<gtk::Label>(&w,"histogram-status"),status);
                assert_eq!(status.text(),w.localization().text(MessageId::RESOURCES_HISTOGRAM_EXACT).as_ref());
                assert_eq!(chart.tooltip_text().as_deref(),Some(format!("{}\n{}",view.description,view.range).as_str()));
                assert_eq!(status.tooltip_text().as_deref(),Some(view.status.as_ref()));
                assert_eq!(histogram_widget::<gtk::DropDown>(&w,"histogram-source"),source);
                assert_eq!(histogram_widget::<gtk::DropDown>(&w,"histogram-channel"),channel);
                for (dropdown,labels) in [(&source,&view.sources),(&channel,&view.channels)] {
                    let model=dropdown.model().unwrap().downcast::<gtk::StringList>().unwrap();
                    assert_eq!(model.n_items(),labels.len() as u32);
                    for (index,label) in labels.iter().enumerate() {assert_eq!(model.string(index as u32).unwrap(),label.as_ref());}
                }
                assert_eq!(histogram_widget::<gtk::CheckButton>(&w,"histogram-log"),logarithmic);
                assert!(logarithmic.is_active());assert_eq!(logarithmic.label().as_deref(),Some(view.labels[0].as_ref()));
                for (index,button) in histogram_clipping.iter().enumerate() {
                    assert_eq!(histogram_widget::<gtk::ToggleButton>(&w,if index==0 {"histogram-shadows"}else{"histogram-highlights"}),*button);
                    assert!(button.is_active());assert_eq!(button.tooltip_text().as_deref(),Some(view.labels[index+1].as_ref()));
                }
                assert_eq!(ui_session(&w).engine().checkpoint(),checkpoint);
                assert_eq!(super::place_source::snapshot(&w),persisted,"source ICC bytes, selection, artwork and undo state survive histogram publication");
                save_snapshot(&w,50,||output.join(format!("histogram-{selected_source}-{}-{theme:?}.png",language.tag())));
            }
        }
        invoke(&w,CommandId::Undo);ready(&w);
        assert!(ui_session(&w).engine().document().working.selection.is_none());
    }
    w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Histogram,visible:false});
    w.customize(CustomizationAction::SetPanelVisible {panel:Panel::Properties,visible:true});
    w.dispatch(UiAction::Customize {action:CustomizationAction::CloseExpanded});
    w.dispatch(UiAction::MovePanel {panel:Panel::Properties,target:DockTarget::Edge {edge:Edge::Right,outer:false},viewport:[width as f32,800.]});
    w.dispatch(UiAction::Effect {action:layer_ui::EffectAction::Insert {effect:"curves".into()}});ready(&w);
    let layer=state(&w).layer_properties.layer.unwrap();
    w.dispatch(UiAction::Effect {action:layer_ui::EffectAction::Set {layer,key:"rgb".into(),value:layer_core::EffectValue::Curve(vec![[0.,0.],[0.5,0.5],[1.,1.]])}});
    choose(&w,&mut input,"properties-page",0);choose(&w,&mut input,"property-domain",0);
    let graph=histogram_widget::<gtk::DrawingArea>(&w,"property-rgb-graph");
    scroll_to(graph.upcast_ref());input.click(screen_point(graph.upcast_ref(),&w.window,[0.5,0.5]));
    let field=histogram_widget::<gtk::Widget>(&w,"property-rgb-output");
    let control=field.clone().downcast::<crate::number_control::NumberControl>().unwrap();
    let display=find_css(control.upcast_ref(),"number-value").unwrap();input.click(screen_point(&display,&w.window,[0.5,0.5]));
    let entry=descendant::<gtk::Entry>(&control).unwrap();
    let picker=histogram_widget::<gtk::MenuButton>(&w,"property-picker-menu");let choices=picker.popover().unwrap();
    let target=histogram_widget::<gtk::Button>(&w,"property-picker");
    let actions:Vec<_>=widgets(choices.upcast_ref())
        .filter(|widget|widget.widget_name()=="property-picker")
        .map(|widget|widget.downcast::<gtk::Button>().unwrap()).collect();
    assert!(!actions.is_empty(),"actual Target adjustment action");
    let page=histogram_widget::<gtk::DropDown>(&w,"properties-page");let page_model=page.model().unwrap();
    let domain=histogram_widget::<gtk::DropDown>(&w,"property-domain");let domain_model=domain.model().unwrap();
    let statistics=histogram_widget::<gtk::Label>(&w,"curve-status");
    let clipping=["curve-shadows","curve-highlights"].map(|name|histogram_widget::<gtk::ToggleButton>(&w,name));
    let scroller=w.effects.properties.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
    let assert_bounds=|| {
        let group=w.groups.borrow().iter().find(|group|group.panels.contains(&Panel::Properties) && group.root.is_mapped()).unwrap().root.clone();
        let outer=scroller.compute_bounds(&group).unwrap();
        eprintln!("GTK Properties visible {}: group {}x{}, scroller {outer:?}",w.localization().language().tag(),group.width(),group.height());
        assert!(outer.x()>=-1. && outer.x()+outer.width()<=group.width() as f32+1.,"Properties scroller fits its visible dock group: {outer:?}, {}",group.width());
        let viewport=scroller.width() as f32;
        assert!(w.effects.properties.measure(gtk::Orientation::Horizontal,-1).0<=scroller.width(),"compact Properties minimum fits its native viewport");
        let properties=w.effects.properties.compute_bounds(&scroller).unwrap();
        eprintln!("GTK Properties {}: viewport {}, minimum {}, natural {}, bounds {:?}, horizontal adjustment {} / {} / {}",w.localization().language().tag(),viewport,
            w.effects.properties.measure(gtk::Orientation::Horizontal,-1).0,w.effects.properties.measure(gtk::Orientation::Horizontal,-1).1,
            properties,scroller.hadjustment().value(),scroller.hadjustment().upper(),scroller.hadjustment().page_size());
        assert!(properties.x()>=-1. && properties.x()+properties.width()<=viewport+1.,"Properties content fits its native viewport: {properties:?}, {viewport}");
        let controls=[graph.clone().upcast::<gtk::Widget>(),control.clone().upcast(),entry.clone().upcast(),page.clone().upcast(),domain.clone().upcast(),picker.clone().upcast(),target.clone().upcast(),statistics.clone().upcast()]
            .into_iter().chain(clipping.iter().map(|button|button.clone().upcast()));
        for widget in controls {
            let bounds=widget.compute_bounds(&scroller).unwrap();
            assert!(bounds.x()>=-1. && bounds.x()+bounds.width()<=viewport+1.,"{} fits Properties viewport: {bounds:?}, {viewport}",widget.widget_name());
            let bounds=widget.compute_bounds(&group).unwrap();
            assert!(bounds.x()>=-1. && bounds.x()+bounds.width()<=group.width() as f32+1.,"{} fits visible Properties dock: {bounds:?}, {}",widget.widget_name(),group.width());
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
        entry.grab_focus();entry.set_text("0.543210987654321");pump(50);entry.select_region(0,-1);
        let text=entry.text();let selection=entry.selection_bounds();let focus=gtk::prelude::RootExt::focus(&w.window);
        let checkpoint=ui_session(&w).engine().checkpoint();let persisted=super::place_source::snapshot(&w);
        for &language in layer_ui::localization::SHIPPED_LANGUAGES {
            switch(language);
            let view=state(&w).tonal_histogram;
            assert_eq!(widgets(w.effects.stats.upcast_ref()).find(|widget|widget.widget_name()=="stroke-recording").unwrap(),recording_button.clone().upcast::<gtk::Widget>());
            assert_eq!(recording_button.tooltip_text().as_deref(),Some(layer_ui::NativeCopy::new(&w.localization()).color.record_tablet.as_ref()));
            assert!(std::sync::Arc::ptr_eq(view.data.as_ref().unwrap(),&initial));
            assert_eq!(histogram_widget::<gtk::DrawingArea>(&w,"property-rgb-graph"),graph);
            assert_eq!(histogram_widget::<gtk::Widget>(&w,"property-rgb-output"),field);
            assert_eq!(descendant::<gtk::Entry>(&control).unwrap(),entry);
            assert_eq!(histogram_widget::<gtk::MenuButton>(&w,"property-picker-menu"),picker);
            assert_eq!(picker.popover().unwrap(),choices);
            let current:Vec<_>=widgets(choices.upcast_ref())
                .filter(|widget|widget.widget_name()=="property-picker")
                .map(|widget|widget.downcast::<gtk::Button>().unwrap()).collect();
            let properties=state(&w).layer_properties;
            let calibration:Vec<_>=properties.actions.iter().filter(|action|action.group.as_ref().is_some_and(|group|group.id=="calibration")).collect();
            assert_eq!(current,actions);assert_eq!(current.len(),calibration.len());
            for (button,action) in current.iter().zip(&calibration) {
                assert_eq!(button.label().as_deref(),Some(action.label.as_str()));
            }
            let targeted=properties.actions.iter().find(|action|matches!(action.action,layer_ui::EffectAction::TargetCurve {..})).unwrap();
            assert_eq!(histogram_widget::<gtk::Button>(&w,"property-picker"),target);
            assert_eq!(target.tooltip_text().as_deref(),Some(targeted.label.as_str()));
            let group=calibration.first().unwrap().group.as_ref().unwrap();
            assert_eq!(picker.tooltip_text().as_deref(),Some(group.label.as_str()));
            assert_eq!(histogram_widget::<gtk::DropDown>(&w,"properties-page"),page);
            assert_eq!(histogram_widget::<gtk::DropDown>(&w,"property-domain"),domain);
            assert_eq!(page.model().unwrap(),page_model);assert_eq!(domain.model().unwrap(),domain_model);
            assert_eq!((page.selected(),domain.selected()),(0,0));
            for dropdown in [&page,&domain] {
                let selected=dropdown.model().and_downcast::<gtk::StringList>().unwrap().string(dropdown.selected()).unwrap();
                let label=widgets(dropdown.upcast_ref()).filter(|widget|widget.is_mapped()).filter_map(|widget|widget.downcast::<gtk::Label>().ok()).find(|label|label.text()==selected).unwrap();
                assert_eq!(label.tooltip_text().as_deref(),Some(selected.as_str()));
            }
            assert_eq!(entry.text(),text);assert_eq!(entry.selection_bounds(),selection);
            assert_eq!(gtk::prelude::RootExt::focus(&w.window),focus,"language publication retains exact numeric draft focus");
            assert_eq!(statistics.text(),w.localization().text(MessageId::RESOURCES_HISTOGRAM_EXACT).as_ref());
            assert_eq!(histogram_widget::<gtk::Label>(&w,"curve-status"),statistics);
            assert_eq!(statistics.tooltip_text().as_deref(),Some(view.status.as_ref()));
            for (index,button) in clipping.iter().enumerate() {
                assert_eq!(histogram_widget::<gtk::ToggleButton>(&w,if index==0 {"curve-shadows"}else{"curve-highlights"}),*button);
                assert_eq!(button.is_active(),if index==0 {state(&w).histogram.shadows}else{state(&w).histogram.highlights});
                assert_eq!(button.tooltip_text().as_deref(),Some(state(&w).histogram.labels[index+1].as_ref()));
            }
            assert_eq!(ui_session(&w).engine().checkpoint(),checkpoint);assert_eq!(super::place_source::snapshot(&w),persisted);
            pump(60);assert_bounds();
            save_snapshot(&w,50,||output.join(format!("curves-histogram-{}-{theme:?}.png",language.tag())));
            if matches!(language,UiLanguage::French|UiLanguage::German) {
                let settings=gtk::Settings::default().unwrap();let font=settings.gtk_font_name();let size=w.window.default_size();
                let maximized=w.window.is_maximized();let initial_width=w.window.width();
                let target_width=initial_width.min(744);
                if maximized {w.window.unmaximize();}
                until(||!w.window.is_maximized(),"unmaximized Properties window");pump(120);
                settings.set_property("gtk-font-name","Sans 16");w.window.set_default_size(target_width,780);
                until(||!w.window.is_maximized() && (target_width-20..=target_width+20).contains(&w.window.width()) && (760..=800).contains(&w.window.height()),"actual narrow Properties window");
                pump(200);assert_bounds();
                assert!((target_width-20..=target_width+20).contains(&w.window.width()) && (760..=800).contains(&w.window.height()),"actual narrow Properties allocation {} × {}",w.window.width(),w.window.height());
                assert_eq!(entry.text(),text);assert_eq!(entry.selection_bounds(),selection);
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
