//! Actual GTK widgets and Mutter-delivered input for the window-bar builder.
use super::*;
#[path = "command_bar_tests.rs"]
mod command_bar_tests;

#[path = "canvas_pen_button_tests.rs"]
mod canvas_pen_buttons;
#[path = "color_picker_tests.rs"]
mod color_picker_tests;
#[path = "accent_preferences_tests.rs"]
mod accent_preferences_tests;
#[path = "palette_tests.rs"]
mod palette_tests;
#[path = "callback_boundary_tests.rs"]
mod callback_boundary_tests;

fn wait_for_drawer_close(w: &Workspace) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !w.drawer.is_closed() {
        assert!(Instant::now() < deadline, "drawer close animation");
        pump(10);
    }
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_sketch_layer_properties_drawer"]
fn native_sketch_layer_properties_drawer() {
    let mut d = Driver::new("art.capycanvas.SketchLayerProperties");
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from).unwrap_or_else(|| d.input.dir.clone());
    std::fs::create_dir_all(&output).unwrap();
    let mut reports = Vec::new();
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        d.w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(WorkspaceState {
            layout:WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default()
        })});
        d.w.dispatch(UiAction::SelectLayer {id:1});
        pump(250);
        let opener = d.header_tool(ToolbarControl::Panel {panel:Panel::Layers});
        d.click_name(&opener);
        let mut footer = d.named("layer-properties-disclosure");
        let mut label = d.named("layer-properties-disclosure-label").downcast::<gtk::Label>().unwrap();
        let mut summary = d.named("layer-properties-disclosure-summary").downcast::<gtk::Label>().unwrap();
        assert_eq!(label.text(), "Layer properties");
        assert!(!summary.is_visible());
        assert!(!footer.has_css_class("heading"));
        let mut stack = d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
        layer_page_settled(&d,&stack,"initial Layers drawer");
        layer_footer_alignment(&d);
        let bounds = d.w.drawer.placement().unwrap().bounds;
        let checkpoint = ui_session(&d.w).engine().checkpoint();
        let _warm = crate::snapshot(&d.w); pump(120);
        crate::snapshot(&d.w).save_to_png(output.join(format!("properties-plain-{theme:?}.png"))).unwrap();
        for open in [true, false] {
            let point = d.point(&footer);
            d.input.perform(serde_json::json!([{"touch":"down","point":point},{"touch":"up"}]));
            layer_page_settled(&d,&stack,"touch Properties toggle");
            assert_eq!(state(&d.w).customization.drawer.unwrap().layer_properties, Some(open), "touch footer contact");
            assert_eq!(label.text(), if open { "All layers" } else { "Layer properties" });
            assert!(!summary.is_visible());
        }
        for close in ["footer", "escape", "outside"] {
            let measurement=d.w.drawer.layers().unwrap().scroll_measurement(bounds.width as i32);
            let properties=stack.child_by_name("properties").unwrap().first_child().unwrap().downcast::<gtk::ScrolledWindow>().unwrap().child().unwrap();
            let expected=layer_ui::LayerDrawerSizing::pages(measurement,state(&d.w).layers.len(),d.w.surface.height() as f32,properties.measure(gtk::Orientation::Vertical,bounds.width as i32).1 as f32)[1];
            d.click(&footer);
            layer_page_settled(&d,&stack,"Properties before dismissal");
            assert_eq!(state(&d.w).customization.drawer.unwrap().layer_properties, Some(true));
            assert_eq!(stack.visible_child_name().as_deref(), Some("properties"));
            assert_eq!(label.text(), "All layers");
            assert!(!summary.is_visible());
            assert!((d.w.drawer.placement().unwrap().bounds.height-expected).abs()<=1.,"native Properties follows shared page measurement");
            assert!(d.named("drawer-panel-Properties").is_mapped());
            assert!(!stack.child_by_name("layers").unwrap().is_child_visible());
            match close {
                "footer" => d.click(&footer),
                "escape" => d.input.key(0xff1b),
                _ => {
                    let p = [d.w.surface.width() as f32 * 0.45, d.w.surface.height() as f32 * 0.75];
                    assert!(!bounds.contains(p[0], p[1]));
                    d.input.click(screen_point(d.w.surface.upcast_ref(), &d.w.window, [0.45, 0.75]));
                }
            }
            assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint, "{theme:?}/{close}: dismissal does not paint");
            if close == "outside" {
                assert!(state(&d.w).customization.drawer.is_none());
                d.click_name(&opener);
                footer = d.named("layer-properties-disclosure");
                label = d.named("layer-properties-disclosure-label").downcast::<gtk::Label>().unwrap();
                summary = d.named("layer-properties-disclosure-summary").downcast::<gtk::Label>().unwrap();
                stack = d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
            }
            assert_eq!(state(&d.w).customization.drawer.unwrap().layer_properties, Some(false));
            assert_eq!(stack.visible_child_name().as_deref(), Some("layers"));
        }
        for _ in 0..32 {
            d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::New {group:false, clipped:false}});
        }
        pump(300);
        let list = find_css(stack.child_by_name("layers").unwrap().upcast_ref(), "layer-list").unwrap();
        let scroll = list.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
        assert!(scroll.vadjustment().upper() > scroll.vadjustment().page_size());
        d.input.perform(serde_json::json!([{"point":d.point(&list)}, {"wheel":[0,5]}]));
        let position = scroll.vadjustment().value();
        assert!(position > 0.);
        let anchor = layer_list_anchor(&list, &scroll);
        d.click(&footer);
        layer_page_settled(&d,&stack,"plain Properties scroll retention");
        d.click(&footer);
        layer_page_settled(&d,&stack,"Layers scroll retention");
        let retained = find_named(&list, &anchor.0).unwrap();
        assert!(retained.is_mapped());
        assert!((retained.compute_bounds(&scroll).unwrap().y()-anchor.1).abs()<1., "visible layer survives page switching");
        let workspace = ui_session_mut(&d.w).capture_workspace();
        let add_filter = find_named(stack.child_by_name("layers").unwrap().upcast_ref(), "layer-add-filter").unwrap();
        assert!(add_filter.is_sensitive());
        let _warm = crate::snapshot(&d.w); pump(120);
        crate::snapshot(&d.w).save_to_png(output.join(format!("properties-add-filter-{theme:?}.png"))).unwrap();
        let point = d.point(&add_filter);
        let picked = d.w.window.pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT).unwrap();
        assert!(picked == add_filter || picked.is_ancestor(&add_filter), "Add Filter hit target: {} at {point:?}", picked.widget_name());
        let menu = add_filter.downcast_ref::<gtk::MenuButton>().unwrap().popover().unwrap();
        d.click(&add_filter);
        assert!(menu.is_mapped(), "native Add Filter opens its menu");
        let blur = widgets(menu.upcast_ref()).find(|node| node.is_mapped() && node.type_().name() == "GtkModelButton" && node.property::<String>("text") == "Blur").unwrap();
        let submenu = blur.property::<Option<gtk::PopoverMenu>>("popover").unwrap();
        let point = d.point(&blur);
        d.input.perform(serde_json::json!([{"point":point},{"wait_ms":200}]));
        if !submenu.is_mapped() { d.input.click(point); }
        until(|| submenu.is_mapped(), "Blur submenu opens");
        let gaussian = widgets(submenu.upcast_ref()).find(|node| node.is_mapped() && node.type_().name() == "GtkModelButton" && node.property::<String>("text") == "Gaussian Blur").unwrap();
        d.click(&gaussian);
        assert_eq!(state(&d.w).customization.drawer.unwrap().layer_properties, Some(true));
        assert!(!find_named(stack.child_by_name("properties").unwrap().upcast_ref(),"properties-add-filter").unwrap().is_visible());
        assert_eq!(ui_session_mut(&d.w).capture_workspace(), workspace);
        d.click(&footer);
        let id = state(&d.w).layer_properties.layer.unwrap();
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::Rename {id, name:"Soft light".into()}});
        pump(250);
        assert_eq!(label.text(), "Gaussian Blur");
        assert!(summary.is_visible());
        layer_footer_alignment(&d);
        let original = summary.text().to_string();
        assert!(original.chars().any(|c| c.is_ascii_digit()));
        d.click(&footer);
        assert!(!summary.is_visible());
        let sigma = find_named(stack.child_by_name("properties").unwrap().upcast_ref(), "property-sigma").unwrap();
        d.number(&sigma, "7");
        d.click(&footer);
        assert!(summary.text().contains('7'));
        d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x7a,"down":true},{"key":0x7a,"down":false},{"key":0xffe3,"down":false}]));
        assert_eq!(summary.text().as_str(), original);
        let bounds = d.w.drawer.placement().unwrap().bounds;
        d.click(&footer);
        layer_page_settled(&d,&stack,"Properties page animation");
        pump(100);
        let properties_bounds = d.w.drawer.placement().unwrap().bounds;
        assert!(properties_bounds.height<bounds.height);
        d.click(&footer);
        layer_page_settled(&d,&stack,"Layers page animation");
        for repeat in 0..4 {
            let window = Instant::now();
            while window.elapsed() < Duration::from_secs(5) {
              for open in [true, false] {
                let timings = Rc::new(RefCell::new(Vec::new()));
                let native = d.named("tool-drawer");
                let height = Rc::new(Cell::new(native.height()));
                let list = find_css(&stack.child_by_name("layers").unwrap(),"layer-list").unwrap();
                let scroll = list.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
                let position = Rc::new(Cell::new(scroll.vadjustment().value()));
                let clock = d.w.surface.frame_clock().unwrap();
                let capture = clock.connect_after_paint(glib::clone!(#[strong] timings, #[strong] height, #[strong] position, #[weak] scroll, #[weak] list, #[weak] native, #[weak] stack, move |clock| {
                    let moved = height.replace(native.height()) != native.height();
                    let scrolled = position.replace(scroll.vadjustment().value()) != scroll.vadjustment().value() && list.is_mapped();
                    if (stack.is_transition_running() || moved || scrolled) && let Some(timing) = clock.current_timings() {
                        timings.borrow_mut().push(timing);
                    }
                }));
                d.click(&footer);
                pump(100);
                clock.disconnect(capture);
                assert_eq!(d.w.drawer.placement().unwrap().bounds,if open {properties_bounds} else {bounds});
                let mut times:Vec<_> = timings.borrow().iter().filter(|t| t.is_complete() && t.presentation_time() > 0).map(|t|t.presentation_time()).collect();
                times.sort_unstable(); times.dedup();
                let intervals:Vec<_> = times.windows(2).map(|pair| pair[1]-pair[0]).collect();
                reports.push(serde_json::json!({"theme":format!("{theme:?}"),"repeat":repeat,"open":open,"presentations":times,"intervals_us":intervals,"viewport":[d.w.surface.width(),d.w.surface.height()],"canvas":super::photo_edit::document(&d.w).composition().size}));
              }
            }
        }
        let _warm = crate::snapshot(&d.w); pump(120);
        crate::snapshot(&d.w).save_to_png(output.join(format!("properties-footer-{theme:?}.png"))).unwrap();
        d.click(&footer);
        let _warm = crate::snapshot(&d.w); pump(120);
        crate::snapshot(&d.w).save_to_png(output.join(format!("properties-page-{theme:?}.png"))).unwrap();
        d.click_name(&opener);

    }
    std::fs::write(output.join("properties-animation.json"), serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
    d.input.finish(); d.w.window.destroy(); pump(100);
}

#[allow(deprecated)]
fn panel_background(widget:&gtk::Widget) -> Option<gtk::gsk::RenderNode> {
    let snapshot=gtk::Snapshot::new();
    snapshot.render_background(&widget.style_context(),0.,0.,widget.width() as f64,widget.height() as f64);
    snapshot.to_node()
}

fn panel_shadow(node:&gtk::gsk::RenderNode) -> bool {
    if node.downcast_ref::<gtk::gsk::OutsetShadowNode>().is_some() {return true;}
    if let Some(container)=node.downcast_ref::<gtk::gsk::ContainerNode>() {
        return (0..container.n_children()).any(|index|panel_shadow(&container.child(index)));
    }
    false
}

fn surface_content(widget:&gtk::Widget) -> gtk::Widget {
    let child=widget.downcast_ref::<gtk::ScrolledWindow>().and_then(|scroll|scroll.child())
        .or_else(||widget.downcast_ref::<gtk::Viewport>().and_then(|viewport|viewport.child()));
    child.map_or_else(||widget.clone(),|child|surface_content(&child))
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_workspace_panel_surfaces_light"]
fn native_workspace_panel_surfaces_light() {panel_surfaces(Theme::Light);}

#[test]
#[ignore = "isolated native-input.js --native-test=native_workspace_panel_surfaces_dark"]
fn native_workspace_panel_surfaces_dark() {panel_surfaces(Theme::Dark);}

fn panel_surfaces(theme:Theme) {
    let mut d=Driver::new("art.capycanvas.PanelSurfaces");
    let output=std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from).unwrap_or_else(||d.input.dir.clone());
    std::fs::create_dir_all(&output).unwrap();
    let panels=[Panel::Properties,Panel::Adjustments,Panel::ToolSettings,Panel::Color,Panel::Sizes,Panel::Histogram,Panel::Waveform,Panel::Navigator,Panel::Proof,Panel::Brushes,Panel::BrushSets,Panel::SculptSets,Panel::Tools,Panel::Palettes,Panel::Layers,Panel::FilterTypes,Panel::Stats];
    d.w.dispatch(UiAction::Effect {action:layer_ui::EffectAction::Insert {effect:"curves".into()}}); pump(200);
    let viewport=[d.w.surface.width() as f32,d.w.surface.height() as f32];
    let mut reports=Vec::new();
      d.w.dispatch(UiAction::SetTheme {theme:Some(theme)});
      for transparency in [layer_ui::Transparency::Off,layer_ui::Transparency::High] {
        super::use_transparency(&d.w,transparency);
        for panel in panels {
          for scope in ["docked","tabbed","floating","header","toolbar","column","configuration"] {
            let mut layout=WorkspacePreset::Painter.layout(Platform::Gtk);
            for hidden in panels {layout.set_panel_visible(hidden,false).unwrap();}
            layout.set_panel_visible(panel,true).unwrap();
            layout.move_panel(viewport,panel,DockTarget::Edge {edge:Edge::Right,outer:false}).unwrap();
            let group=layout.panel_group(panel).unwrap();
            let control=ToolbarControl::Panel {panel};
            let mut toolbar=None;
            match scope {
                "tabbed" => {
                    let other=if panel==Panel::Layers {Panel::Properties} else {Panel::Layers};
                    layout.add_panel_to_group(other,group).unwrap(); layout.select_tab(group,panel).unwrap();
                }
                "floating" => {layout.move_panel(viewport,panel,DockTarget::Float {position:[360.,140.]}).unwrap();}
                "header" => {
                    layout.set_panel_visible(panel,false).unwrap(); layout.header.zones=Default::default();
                    layout.header.add(HeaderZone::Left,None,&[HeaderItem::Menu,HeaderItem::Tool {control}]).unwrap();
                }
                "toolbar" => {
                    layout.set_panel_visible(panel,false).unwrap(); toolbar=Some(layout.add_toolbar(None,"Surface audit",&[control]).unwrap());
                }
                "column" => {
                    layout.set_column_collapsed(group,true,viewport).unwrap();
                    let column=layout.collapsed_column_for_group(group).unwrap();
                    let mut settings=layout.column_stack(column);settings.drawers=true;
                    layout.column_stacks.retain(|stack|stack.column!=settings.column);
                    layout.column_stacks.push(settings);
                }
                _ => (),
            }
            d.w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(WorkspaceState {layout,..Default::default()})}); pump(180);
            let opener=match scope {
                "header" => Some(d.named(&d.header_tool(control))),
                "toolbar" => {
                    let id=ui_session(&d.w).panel_view(toolbar.unwrap()).unwrap().tiles[0].id;
                    Some(widgets(d.w.surface.upcast_ref()).find(|widget|widget.is_mapped() && widget.widget_name()==format!("tile-{id}")).unwrap())
                }
                "column" => {
                    let column=state(&d.w).workspace.layout.collapsed_column_for_group(group).unwrap();
                    Some(d.w.columns.button(column,panel).unwrap().clone().upcast())
                }
                _ => None,
            };
            if let Some(opener)=&opener {d.click(opener);pump(100);}
            if scope=="configuration" {
                d.w.dispatch(UiAction::Customize {action:CustomizationAction::ShowAllControls {panel}}); pump(300);
                assert!(ui_session(&d.w).panel_view(panel).unwrap().expanded,"configuration model opens {panel:?}");
            }
            let drawer=matches!(scope,"header"|"toolbar"|"column");
            let (content,owner)=if drawer {
                let owner_name=if scope=="column" {
                    let state=state(&d.w);
                    assert!(state.customization.column_drawers.iter().any(|drawer|drawer.panels().any(|id|id==panel)),"column popup adopts {panel:?}");
                    format!("column-drawer-{}",state.workspace.layout.collapsed_column_for_group(group).unwrap())
                } else {"tool-drawer".into()};
                let owner=widgets(d.w.surface.upcast_ref()).find(|widget|widget.is_mapped() && widget.widget_name()==owner_name).unwrap();
                let name=format!("drawer-panel-{panel:?}");
                let body=widgets(&owner).find(|widget|widget.is_mapped() && widget.widget_name()==name).unwrap_or_else(||panic!("mapped {name}: {scope}"));
                (surface_content(&body),owner)
            } else {
                let content=surface_content(&d.w.panel_widget(panel));
                let owner=d.w.groups.borrow().iter().find(|view|view.panels.contains(&panel)).unwrap().root.clone().upcast::<gtk::Widget>();
                (content,owner)
            };
            assert!(content.is_mapped(),"{panel:?}/{scope} content mapped");
            assert_eq!(content.pango_context().font_description(),owner.pango_context().font_description(),"{panel:?}/{scope} inherits owner typography");
            let insets=|widget:&gtk::Widget|[widget.margin_start(),widget.margin_end(),widget.margin_top(),widget.margin_bottom()];
            if matches!(panel,Panel::Histogram|Panel::Waveform) {
                assert_eq!(insets(&content),[layer_ui::PANEL_CONTENT_INSET as i32;4],"standalone monitor composition owns its inset");
            }
            if let Some(histogram)=find_named(&content,"levels-histogram") {
                assert_eq!(insets(&histogram),[0;4],"nested tonal histogram has no second content inset");
            }
            let _warm=crate::snapshot(&d.w); pump(80);
            if matches!(panel,Panel::Properties|Panel::Color|Panel::Navigator) {
                crate::snapshot(&d.w).save_to_png(output.join(format!("{panel:?}-{scope}-{theme:?}-{transparency:?}.png"))).unwrap();
            }
            assert!(panel_background(&content).is_none(),"reusable {panel:?}/{scope}/{theme:?}/{transparency:?} content has no surface");
            for widget in widgets(&content) {
                assert!(!widget.has_css_class("dock-panel"),"reusable {panel:?}/{scope} contains surface owner {}",widget.widget_name());
            }
            let expanded=scope=="configuration" && owner.has_css_class("expanded-panel");
            if scope=="configuration" {assert!(expanded,"native configuration opens {panel:?}");}
            if expanded {
                assert!(panel_background(&owner).is_none(),"expanded owner delegates preview/configuration surfaces");
                let preview=find_css(&owner,"panel-preview").unwrap();
                assert!(panel_background(&preview).is_some(),"expanded preview retains surface");
                let configuration=find_css(&owner,"panel-configuration").unwrap();
                assert!(panel_background(&configuration).is_some(),"configuration retains surface");
            } else {
                if owner.has_css_class("tabbed") && transparency.enabled() {
                    if drawer {assert!(panel_background(&owner).is_none(),"glass tabbed drawer root delegates its backdrop");}
                    assert!(panel_background(&find_css(&owner,"panel-body").unwrap()).is_some(),"glass tabbed body chrome retains backdrop");
                    assert!(panel_background(&find_css(&owner,"dock-tabs").unwrap()).is_some(),"glass tab strip retains backdrop");
                } else {
                    assert!(panel_background(&owner).is_some(),"{panel:?}/{scope} chrome retains background or shadow");
                }
                if !drawer {assert!(panel_shadow(&panel_background(&owner).unwrap()),"normal/tabbed/floating owner retains its external shadow");}
                if let Some(preview)=find_css(&owner,"panel-preview") {
                    assert!(panel_background(&preview).is_none(),"normal preview has no duplicate surface");
                }
            }
            if drawer {
                let shadow=widgets(d.w.surface.upcast_ref()).find(|widget|widget.is_mapped() && widget.widget_name()=="drawer-shadow").unwrap();
                assert!(panel_shadow(&panel_background(&shadow).unwrap()),"drawer outer shadow retained");
            }
            let metrics:Vec<_>=widgets(&content).filter(|widget|widget.is_mapped() && !widget.widget_name().is_empty() && (widget.is::<gtk::Button>() || widget.is::<gtk::Entry>() || widget.is::<gtk::DropDown>())).map(|widget|serde_json::json!({"name":widget.widget_name().as_str(),"height":widget.measure(gtk::Orientation::Vertical,-1).1})).collect();
            reports.push(serde_json::json!({"panel":format!("{panel:?}"),"scope":scope,"theme":format!("{theme:?}"),"transparency":format!("{transparency:?}"),"expanded":expanded,"control_metrics":metrics}));
            std::fs::write(output.join("panel-surfaces.json"),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
            if let Some(opener)=opener {d.click(&opener);pump(200);}
          }
        }
      }
    assert_eq!(reports.len(),17*7*2);
    d.finish();
}

fn layer_footer_alignment(d:&Driver) {
    let footer=d.named("layer-properties-disclosure");
    let stack=d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
    let layers=stack.child_by_name("layers").unwrap();
    let leading=d.named("layer-properties-icon-slot");
    let trailing=d.named("layer-properties-chevron-slot");
    let label=d.named("layer-properties-disclosure-label");
    let summary=d.named("layer-properties-disclosure-summary");
    let bounds=|widget:&gtk::Widget|widget.compute_bounds(&d.w.surface).unwrap();
    let center=|rect:gtk::graphene::Rect|[rect.x()+rect.width()*0.5,rect.y()+rect.height()*0.5];
    let left=bounds(&leading); let right=bounds(&trailing);
    let control=layer_ui::PANEL_CONTROL_METRICS.control_height as i32;
    assert_eq!([leading.width(),leading.height(),trailing.width(),trailing.height()],[control;4]);
    for slot in [&leading,&trailing] {
        let image=slot.first_child().unwrap();
        assert_eq!(image.measure(gtk::Orientation::Horizontal,-1).1,16,"footer glyph uses compact icon size");
        assert!((center(bounds(slot))[1]-center(bounds(&footer))[1]).abs()<=1.,"footer icon vertically centered");
    }
    assert!((bounds(&label).x()-left.x()-left.width()-layer_ui::PANEL_GROUP_SPACING).abs()<=1.,"shared compact icon/text gap");
    if summary.is_visible() {assert!((bounds(&label).x()-bounds(&summary).x()).abs()<0.5);}
    if stack.visible_child_name().as_deref()==Some("layers") {
        let first=find_named(&layers,"layer-add-layer-symbolic").unwrap();
        let last=find_named(&layers,"layer-actions").unwrap();
        if (center(left)[0]-center(bounds(&first))[0]).abs()>1.
            && let Some(output)=std::env::var_os("LAYER_TEST_ARTIFACTS") {
                crate::snapshot(&d.w).save_to_png(std::path::PathBuf::from(output).join("footer-alignment-failure.png")).unwrap();
            }
        assert!((center(left)[0]-center(bounds(&first))[0]).abs()<=1.,"disclosure leading icon aligns with Layers action bar: slot={left:?} action={:?} footer={:?}",bounds(&first),bounds(&footer));
        assert!((center(right)[0]-center(bounds(&last))[0]).abs()<=1.,"disclosure chevron aligns with Layers action bar: slot={right:?} action={:?} footer={:?}",bounds(&last),bounds(&footer));
    }
    let text=label.parent().unwrap();
    assert!((center(bounds(&text))[1]-center(bounds(&footer))[1]).abs()<=1.,"plain/summary text vertically centered");
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_layer_drawer_any_toolbar"]
fn native_layer_drawer_any_toolbar() {
    let mut d=Driver::new("art.capycanvas.LayerDrawerAnyToolbar");
    let output=std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from).unwrap_or_else(||d.input.dir.clone());
    std::fs::create_dir_all(&output).unwrap();
    let layers=ToolbarControl::Panel {panel:Panel::Layers};
    for theme in [Theme::Light,Theme::Dark] {
      d.w.dispatch(UiAction::SetTheme {theme:Some(theme)});
      for preset in [WorkspacePreset::Painter,WorkspacePreset::Illustrator,WorkspacePreset::Photographer] {
        let mut layout=preset.layout(Platform::Gtk);
        layout.header.zones=Default::default();
        layout.header.add(HeaderZone::Left,None,&[HeaderItem::Menu,HeaderItem::Tool {control:layers}]).unwrap();
        let toolbar=layout.add_toolbar(None,"Drawer audit",&[layers,ToolbarControl::Color,ToolbarControl::Panel {panel:Panel::Color}]).unwrap();
        d.w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(WorkspaceState {layout,..Default::default()})});
        pump(250);
        let header=d.named(&d.header_tool(layers));
        let tile=ui_session(&d.w).panel_view(toolbar).unwrap().tiles.iter().find(|tile|tile.choice.control==layers).unwrap().id;
        let tile=widgets(d.w.surface.upcast_ref()).find(|widget|widget.is_mapped() && widget.widget_name()==format!("tile-{tile}")).unwrap();
        for (origin,opener) in [("header",header),("toolbar",tile)] {
            d.click(&opener);
            let stack=d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
            layer_page_settled(&d,&stack,"Layers combo from any native toolbar");
            assert_eq!(state(&d.w).customization.drawer.unwrap().layer_properties,Some(false));
            layer_footer_alignment(&d);
            let footer=d.named("layer-properties-disclosure");
            d.click(&footer); layer_page_settled(&d,&stack,"any-workspace Properties page");
            assert_eq!(stack.visible_child_name().as_deref(),Some("properties"));
            assert_eq!(d.named("layer-properties-disclosure-label").downcast::<gtk::Label>().unwrap().text(),"All layers");
            layer_footer_alignment(&d);
            d.input.key(0xff1b); layer_page_settled(&d,&stack,"Escape returns every-workspace Layers");
            assert_eq!(stack.visible_child_name().as_deref(),Some("layers"));
            layer_footer_alignment(&d);
            let _warm=crate::snapshot(&d.w); pump(100);
            crate::snapshot(&d.w).save_to_png(output.join(format!("footer-{theme:?}-{preset:?}-{origin}.png"))).unwrap();
            d.click(&opener); wait_for_drawer_close(&d.w);
        }
        for control in [ToolbarControl::Color,ToolbarControl::Panel {panel:Panel::Color}] {
            let id=ui_session(&d.w).panel_view(toolbar).unwrap().tiles.iter().find(|tile|tile.choice.control==control).unwrap().id;
            let tile=widgets(d.w.surface.upcast_ref()).find(|widget|widget.is_mapped() && widget.widget_name()==format!("tile-{id}")).unwrap();
            d.click(&tile);
            let drawer=state(&d.w).customization.drawer.unwrap();
            assert!(drawer.columns.iter().flatten().any(|panel|*panel==Panel::Color));
            assert!(drawer.columns.iter().flatten().any(|panel|*panel==Panel::Palettes));
            assert!(d.named("drawer-panel-Palettes").is_mapped());
            d.click(&tile); wait_for_drawer_close(&d.w);
        }
      }
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_sketch_layer_properties_footer_pen --tablet"]
fn native_sketch_layer_properties_footer_pen() {
    assert!(std::env::var("WAYLAND_DISPLAY").is_ok_and(|display|display.ends_with("-tablet")));
    let mut d = Driver::new("art.capycanvas.SketchLayerPropertiesPen");
    sketch_layer_properties_pen(&mut d);
    d.input.finish(); d.w.window.destroy(); pump(100);
}

fn sketch_layer_properties_pen(d: &mut Driver) {
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        d.w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(WorkspaceState {
            layout:WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default()
        })});
        pump(250);
        let opener = d.header_tool(ToolbarControl::Panel {panel:Panel::Layers});
        d.click_name(&opener);
        let footer = d.named("layer-properties-disclosure");
        let stack = d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
        layer_page_settled(d,&stack,"pen Layers drawer");
        let bounds = d.w.drawer.placement().unwrap().bounds;
        let checkpoint = ui_session(&d.w).engine().checkpoint();
        for open in [true, false, true, false] {
            d.input.perform(serde_json::json!([{"pen":"down","point":d.point(&footer)},{"pen":"up"}]));
            layer_page_settled(d,&stack,"pen Properties toggle");
            assert_eq!(state(&d.w).customization.drawer.unwrap().layer_properties, Some(open));
            assert_eq!(d.w.drawer.placement().unwrap().bounds, bounds);
            assert_eq!(ui_session(&d.w).engine().checkpoint(), checkpoint);
        }
        d.input.perform(serde_json::json!([{"pen":"leave"}]));
        d.click_name(&opener);
    }
}

fn layer_list_anchor(list: &gtk::Widget, scroll: &gtk::ScrolledWindow) -> (String, f32) {
    widgets(list).filter_map(|row| {
        if !row.is_mapped() {return None;}
        let name = row.widget_name();
        if !name.starts_with("art-layer-") {return None;}
        let bounds = row.compute_bounds(scroll)?;
        (bounds.y()+bounds.height()>1. && bounds.y()<scroll.height() as f32)
            .then(||(name.to_string(),bounds.y()))
    }).min_by(|a,b|a.1.total_cmp(&b.1)).unwrap()
}

fn layer_page_settled(d: &Driver, stack: &gtk::Stack, label: &str) {
    until(||!stack.is_transition_running(),label);
    let deadline = Instant::now()+Duration::from_secs(3);
    let mut bounds = d.w.drawer.placement().unwrap().bounds;
    let mut stable = Instant::now();
    while stable.elapsed()<Duration::from_millis(60) {
        assert!(Instant::now()<deadline,"{label}: native drawer placement settles");
        pump(20);
        let current = d.w.drawer.placement().unwrap().bounds;
        if current!=bounds {bounds=current; stable=Instant::now();}
    }
}

fn layer_mutation_frame(d: &mut Driver, rows: usize, action: impl FnOnce(&mut Driver)) -> Bounds {
    let first = Rc::new(Cell::new(None));
    let native = d.named("tool-drawer");
    let clock = d.w.surface.frame_clock().unwrap();
    let capture = clock.connect_after_paint(glib::clone!(#[strong] first, #[strong] native, #[weak(rename_to=w)] d.w, move |_| {
        if first.get().is_none() && ui_session(&w).state().layers.len()==rows {
            first.set(w.drawer.placement().map(|placement|(placement.bounds,native.height())));
        }
    }));
    action(d);
    until(||first.get().is_some(), "layer mutation allocated frame");
    clock.disconnect(capture);
    let (bounds,height) = first.get().unwrap();
    assert!((bounds.height-height as f32).abs()<=1.,"first native allocation {height} matches placement {bounds:?}");
    pump(250);
    let settled = d.w.drawer.placement().unwrap().bounds;
    assert!((bounds.height-settled.height).abs()<=1.,"first mutation frame {bounds:?} matches settled height {settled:?}");
    bounds
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_sketch_layer_drawer_immediate_audit"]
fn native_sketch_layer_drawer_immediate_audit() {
    let mut d = Driver::new("art.capycanvas.LayerDrawerImmediate");
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from).unwrap_or_else(||d.input.dir.clone());
    std::fs::create_dir_all(&output).unwrap();
    for theme in [Theme::Light,Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        let epoch = state(&d.w).document_file.epoch;
        d.w.documents.enqueue(&d.w,(new_drawing(2048,1536,&d.w.localization()).unwrap(),None));
        until(||state(&d.w).document_file.epoch!=epoch,"fresh audit drawing");
        d.w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(WorkspaceState {layout:WorkspacePreset::Painter.layout(Platform::Gtk),..Default::default()})});
        if state(&d.w).customization.drawer.as_ref().is_some_and(|drawer|drawer.layer_properties==Some(true)) {
            d.click_name("layer-properties-disclosure");
        }
        for _ in 0..4 {d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::New {group:false,clipped:false}});}
        let opener = d.header_tool(ToolbarControl::Panel {panel:Panel::Layers});
        if state(&d.w).customization.drawer.is_none() {d.click_name(&opener);}
        pump(100);
        let stack = d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
        let page = stack.child_by_name("layers").unwrap();
        let minimum = d.w.drawer.placement().unwrap().bounds.height;
        assert_eq!(state(&d.w).layers.len(),6,"audit starts with six listed rows");
        let added = layer_mutation_frame(&mut d,7,|d|d.click(&find_named(&page,"layer-add-layer-symbolic").unwrap()));
        assert!(added.height>minimum+20.,"Add grows on the first frame: minimum {minimum}, added {added:?}, settled {:?}",d.w.drawer.placement().unwrap().bounds);
        let deleted = layer_mutation_frame(&mut d,6,|d|d.click(&find_named(&page,"delete-selected-layers").unwrap()));
        assert_eq!(deleted.height,minimum);
        let restored = layer_mutation_frame(&mut d,7,|d|d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x7a,"down":true},{"key":0x7a,"down":false},{"key":0xffe3,"down":false}])));
        assert_eq!(restored.height,added.height);
        let redone = layer_mutation_frame(&mut d,6,|d|d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x79,"down":true},{"key":0x79,"down":false},{"key":0xffe3,"down":false}])));
        assert_eq!(redone.height,minimum);
        let rename_id = state(&d.w).layer_properties.layer.unwrap();
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::BeginRename {id:rename_id}});
        let row = find_named(&page,&format!("art-layer-{rename_id}")).unwrap();
        until(||descendant::<gtk::Entry>(&row).is_some_and(|entry|entry.is_mapped()),"layer rename entry mapped");
        d.input.key(0xff1b);
        until(||descendant::<gtk::Entry>(&row).is_some_and(|entry|!entry.is_mapped()),"native Escape cancels rename");
        assert_eq!(d.w.drawer.placement().unwrap().bounds.height,minimum);
        let provider = gtk::CssProvider::new();
        provider.load_from_string("window.capy-workspace { --ui-text-size: 18pt; }");
        gtk::style_context_add_provider_for_display(&d.w.surface.display(),&provider,gtk::STYLE_PROVIDER_PRIORITY_APPLICATION+1);
        pump(180);
        assert!(d.w.drawer.placement().unwrap().bounds.height>minimum,"GTK text size invalidates row metrics");
        gtk::style_context_remove_provider_for_display(&d.w.surface.display(),&provider); pump(180);
        assert_eq!(d.w.drawer.placement().unwrap().bounds.height,minimum);
        for _ in 6..32 {d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::New {group:false,clipped:false}});}
        let list = find_css(&page,"layer-list").unwrap();
        let scroll = list.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
        d.input.perform(serde_json::json!([{"point":d.point(&list)},{"wheel":[0,12]}]));
        let capped = d.w.drawer.placement().unwrap().bounds;
        let added = layer_mutation_frame(&mut d,33,|d|d.click(&find_named(&page,"layer-add-layer-symbolic").unwrap()));
        assert_eq!(added.height,capped.height);
        let selected_visible = |d:&Driver| {
            let id = state(&d.w).layer_properties.layer.unwrap();
            let row = find_named(&list,&format!("art-layer-{id}")).unwrap();
            let b = row.compute_bounds(&scroll).unwrap();
            assert!(b.y()>=-1. && b.y()+b.height()<=scroll.height() as f32+1.,"changed target is visible at the viewport cap: {b:?}");
        };
        selected_visible(&d);
        layer_mutation_frame(&mut d,32,|d|d.click(&find_named(&page,"delete-selected-layers").unwrap()));
        layer_mutation_frame(&mut d,33,|d|d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x7a,"down":true},{"key":0x7a,"down":false},{"key":0xffe3,"down":false}])));
        selected_visible(&d);
        layer_mutation_frame(&mut d,32,|d|d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x79,"down":true},{"key":0x79,"down":false},{"key":0xffe3,"down":false}])));
        d.click_name("layer-properties-disclosure");
        layer_page_settled(&d,&stack,"Properties before hidden-list Add");
        d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0xffe1,"down":true},{"key":0x6e,"down":true},{"key":0x6e,"down":false},{"key":0xffe1,"down":false},{"key":0xffe3,"down":false}]));
        until(||state(&d.w).layers.len()==33,"native Add while Properties is open");
        d.click_name("layer-properties-disclosure");
        layer_page_settled(&d,&stack,"Layers after hidden-list Add");
        selected_visible(&d);
        layer_mutation_frame(&mut d,32,|d|d.click(&find_named(&page,"delete-selected-layers").unwrap()));
        d.click_name("layer-properties-disclosure");
        layer_page_settled(&d,&stack,"Properties before hidden-list Undo");
        d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x7a,"down":true},{"key":0x7a,"down":false},{"key":0xffe3,"down":false}]));
        until(||state(&d.w).layers.len()==33,"native Undo while Properties is open");
        d.click_name("layer-properties-disclosure");
        layer_page_settled(&d,&stack,"Layers after hidden-list Undo");
        selected_visible(&d);
        layer_mutation_frame(&mut d,32,|d|d.click(&find_named(&page,"delete-selected-layers").unwrap()));
        for size in [HeaderSize::Medium,HeaderSize::Large] {
            d.w.dispatch(HeaderAction::SetSize {size}.action()); pump(100);
            for transparency in [layer_ui::Transparency::Low,layer_ui::Transparency::High] {
                super::use_transparency(&d.w,transparency); pump(100);
                let placement = d.w.drawer.placement().unwrap();
                let bridge = placement.connection().unwrap();
                assert!(bridge.bounds.height>6.5,"centered titlebar gap remains connected");
                let button = find_css(&d.named(&opener),"header-tool").unwrap();
                assert!(button.has_css_class("drawer-open"));
                let checkpoint = ui_session(&d.w).engine().checkpoint();
                let p = [bridge.bounds.x+bridge.bounds.width*0.5,bridge.bounds.y+bridge.bounds.height*0.5];
                let p = d.w.surface.compute_point(&d.w.window,&gtk::graphene::Point::new(p[0],p[1])).unwrap();
                d.input.click([p.x(),p.y()]);
                assert!(state(&d.w).customization.drawer.is_some());
                assert_eq!(ui_session(&d.w).engine().checkpoint(),checkpoint);
                let footer = d.named("layer-properties-disclosure");
                d.input.perform(serde_json::json!([{"point":d.point(&footer)}]));
                let _warm = crate::snapshot(&d.w); pump(120);
                let texture = crate::snapshot(&d.w);
                texture.save_to_png(output.join(format!("bridge-footer-{theme:?}-{size:?}-{transparency:?}.png"))).unwrap();
                let mut download = gdk::TextureDownloader::new(&texture); download.set_format(gdk::MemoryFormat::R8g8b8a8);
                let (pixels,stride) = download.download_bytes();
                let scale = texture.width() as f32/d.w.surface.width() as f32;
                let sample = |x:f32,y:f32| {let at=(y*scale) as usize*stride+(x*scale) as usize*4;pixels[at..at+3].to_vec()};
                let join = sample(bridge.bounds.x+bridge.bounds.width*0.5,bridge.bounds.y+bridge.bounds.height*0.5);
                let body = sample(placement.bounds.x+placement.bounds.width*0.5,placement.bounds.y+2.);
                assert!(join.iter().zip(body).all(|(a,b)|a.abs_diff(b)<=4),"{theme:?}/{transparency:?}: bridge matches body {join:?}");
                let b = footer.compute_bounds(&d.w.surface).unwrap();
                let corner = sample(b.x()+1.,b.y()+1.);
                let top = sample(b.x()+b.width()*0.5,b.y()+1.);
                assert!(corner.iter().zip(top).all(|(a,b)|a.abs_diff(b)<=3),"hovered footer top corners stay square");
            }
        }
        d.w.dispatch(HeaderAction::SetSize {size:HeaderSize::Medium}.action());
        let footer = d.named("layer-properties-disclosure");
        d.click(&footer); layer_page_settled(&d,&stack,"audit Properties animation");
        let properties = stack.child_by_name("properties").unwrap();
        assert!(find_named(&properties,"properties-add-filter").unwrap().is_visible());
        let raster = state(&d.w).layer_properties.layer.unwrap();
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::AddMask {id:raster,replace:false}});
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::Select {id:raster,mask:true}}); pump(100);
        assert!(!find_named(&properties,"properties-add-filter").unwrap().is_visible());
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::Select {id:raster,mask:false}});
        assert!(find_named(&properties,"properties-add-filter").unwrap().is_visible());
        d.w.dispatch(UiAction::SelectLayer {id:2});
        assert!(!find_named(&properties,"properties-add-filter").unwrap().is_visible());
        d.w.dispatch(UiAction::SelectLayer {id:raster});
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::New {group:true,clipped:false}});
        let group = state(&d.w).layer_properties.layer.unwrap();
        assert!(!find_named(&properties,"properties-add-filter").unwrap().is_visible());
        assert_eq!(d.named("layer-properties-disclosure-label").downcast::<gtk::Label>().unwrap().text(),"All layers");
        d.click(&footer); layer_page_settled(&d,&stack,"group Layers footer");
        assert_eq!(d.named("layer-properties-disclosure-label").downcast::<gtk::Label>().unwrap().text(),"Group properties");
        assert!(!d.named("layer-properties-disclosure-summary").is_visible());
        d.click(&footer); layer_page_settled(&d,&stack,"group Properties footer");
        assert_eq!(d.named("layer-properties-disclosure-label").downcast::<gtk::Label>().unwrap().text(),"All layers");
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::Delete {id:group}});
        d.w.dispatch(UiAction::SelectLayer {id:raster});
        d.w.dispatch(UiAction::Effect {action:layer_ui::EffectAction::Insert {effect:"curves".into()}}); pump(100);
        assert!(!find_named(&properties,"properties-add-filter").unwrap().is_visible());
        let long = d.w.drawer.placement().unwrap().bounds.height;
        assert!(long>minimum,"long Properties grows beyond its default height");
        d.w.window.unmaximize(); d.w.window.set_default_size(900,420);
        until(||!d.w.window.is_maximized() && d.w.surface.height()<600,"short Properties window"); pump(100);
        assert!(d.w.drawer.placement().unwrap().bounds.height<=d.w.surface.height() as f32*0.85+1.);
        d.w.window.maximize(); until(||d.w.window.is_maximized(),"Properties window regrows"); pump(100);
        assert!((d.w.drawer.placement().unwrap().bounds.height-long).abs()<2.);
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::SelectAllLayers {selected:true}});
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::DeleteSelected}); pump(100);
        assert!(d.named("layer-properties-disclosure").is_sensitive(),"empty Properties can still close");
        d.click(&d.named("layer-properties-disclosure"));
        assert_eq!(state(&d.w).customization.drawer.unwrap().layer_properties,Some(false));
        let epoch = state(&d.w).document_file.epoch;
        let mut object = layer_core::Document::from_artwork(layer_core::authored::Artwork::new([2048,1536]).unwrap()).unwrap();
        let image = layer_core::color::source::rgba8_source([96,64],|_,_|[75,174,158,255]);
        let (owner,edit) = object.create_object_layer_edit("Object",layer_core::authored::ImageObject::new(image.into()),None,0).unwrap();
        object.apply(edit).unwrap();
        d.w.documents.enqueue(&d.w,(object,None)); until(||state(&d.w).document_file.epoch!=epoch,"object drawing replaces the inspected document");
        d.w.dispatch(UiAction::SelectLayer {id:layer_ui::occurrence_token(owner)});
        if state(&d.w).customization.drawer.is_none() {d.click_name(&opener);}
        if !state(&d.w).customization.drawer.as_ref().is_some_and(|drawer|drawer.layer_properties==Some(true)) {d.click_name("layer-properties-disclosure");}
        let stack = d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
        layer_page_settled(&d,&stack,"object Properties animation");
        assert!(find_named(&stack.child_by_name("properties").unwrap(),"properties-add-filter").unwrap().is_visible());
        let _warm = crate::snapshot(&d.w); pump(120);
        crate::snapshot(&d.w).save_to_png(output.join(format!("object-properties-{theme:?}.png"))).unwrap();
    }
    d.input.finish(); d.w.window.destroy(); pump(100);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_sketch_layer_drawer_auto_height"]
fn native_sketch_layer_drawer_auto_height() {
    let mut d = Driver::new("art.capycanvas.LayerDrawerGrowth");
    d.input = d.input.timeout_secs(20);
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from).unwrap_or_else(||d.input.dir.clone());
    std::fs::create_dir_all(&output).unwrap();
    let opener = d.header_tool(ToolbarControl::Panel {panel:Panel::Layers});
    d.click_name(&opener);
    let mut reports = Vec::new();
    let mut heights = Vec::new();
    let touch_motion = std::env::var("LAYER_DRAWER_SCROLL_INPUT").is_ok_and(|input|input=="touch");
    for count in [2, 6, 12, 32, 1000] {
        {
            let mut session = ui_session_mut(&d.w);
            while session.state().layers.len() < count {
                session.dispatch(UiAction::Layer {action:layer_ui::LayerAction::New {group:false,clipped:false}}).unwrap();
            }
        }
        d.w.refresh_document_view(); pump(450);
        until(||!ui_session(&d.w).engine().has_pending_document_edits(), "layer fixture edits settle");
        assert_eq!(state(&d.w).layers.len(), count);
        for theme in [Theme::Light, Theme::Dark] {
            d.w.dispatch(UiAction::SetTheme {theme:Some(theme)}); pump(250);
            let stack = d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
            let page = stack.child_by_name("layers").unwrap();
            let list = find_css(&page, "layer-list").unwrap();
            let scroll = list.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
            let footer = d.named("layer-properties-disclosure");
            let actions = find_css(&page, "layer-footer").unwrap();
            let bounds = d.w.drawer.placement().unwrap().bounds;
            let footer_bounds = footer.compute_bounds(&d.w.surface).unwrap();
            let action_bounds = actions.compute_bounds(&d.w.surface).unwrap();
            assert!(bounds.height <= d.w.window.height() as f32 * 0.85 + 2., "{theme:?}/{count}: drawer height {bounds:?}");
            assert!(bounds.y >= 0. && bounds.y + bounds.height <= d.w.surface.height() as f32 + 1.);
            assert!((footer_bounds.y() + footer_bounds.height() - bounds.y - bounds.height).abs() < 2.);
            assert!(action_bounds.y() + action_bounds.height() <= footer_bounds.y() + 1.);
            let realized = widgets(&list).filter(|widget|widget.widget_name().starts_with("art-layer-")).count();
            assert!(realized < 256, "{count} listed layers realized {realized} row widgets");
            if count >= 32 {
                assert!(scroll.vadjustment().upper() - scroll.vadjustment().page_size() > 40.);
                d.input.perform(serde_json::json!([{"point":d.point(&list)},{"wheel":[0,5]}]));
                assert!(scroll.vadjustment().value() > 0.);
                let before = scroll.vadjustment().value();
                let point = d.point(scroll.upcast_ref());
                let direction = if before+100. < scroll.vadjustment().upper()-scroll.vadjustment().page_size() {-1.} else {1.};
                let mut touch = vec![serde_json::json!({"touch":"down","point":point})];
                touch.extend((1..=36).map(|step|serde_json::json!({"touch":"move","point":[point[0],point[1]+direction*step as f32*2.]})));
                touch.push(serde_json::json!({"touch":"up"}));
                d.input.perform(touch.into());
                assert!((scroll.vadjustment().value()-before).abs()>10., "{theme:?}/{count}: native touch scrolls the list");
                let deadline = Instant::now()+Duration::from_secs(5);
                let mut stable = Instant::now();
                let mut previous = scroll.vadjustment().value();
                while stable.elapsed()<Duration::from_millis(150) {
                    assert!(Instant::now()<deadline, "native touch scroll settles");
                    pump(30);
                    let current = scroll.vadjustment().value();
                    if current!=previous {previous=current; stable=Instant::now();}
                }
            }
            let position = scroll.vadjustment().value();
            let anchor = layer_list_anchor(&list,&scroll);
            d.click(&footer);
            assert_eq!(stack.visible_child_name().as_deref(),Some("properties"));
            layer_page_settled(&d,&stack,"Properties page animation");
            let properties_bounds = d.w.drawer.placement().unwrap().bounds;
            let minimum = heights.first().copied().unwrap_or(bounds.height);
            assert!((properties_bounds.height-minimum).abs()<=1., "plain Properties uses the six-row minimum");
            d.click(&footer);
            layer_page_settled(&d,&stack,"Layers page animation");
            assert_eq!(d.w.drawer.placement().unwrap().bounds, bounds);
            let retained_row = widgets(&list).find(|row|row.widget_name()==anchor.0 && row.is_mapped()).unwrap();
            let retained = retained_row.compute_bounds(&scroll).unwrap().y();
            let mut offsets = vec![retained];
            assert!((retained-anchor.1).abs()<=2.,"{theme:?}/{count}: {} offset {} -> {}, adjustment {} -> {}",anchor.0,anchor.1,retained,position,scroll.vadjustment().value());
            for _ in 0..3 {
                d.click(&footer); layer_page_settled(&d,&stack,"repeated Properties page");
                d.click(&footer); layer_page_settled(&d,&stack,"repeated Layers page");
                assert_eq!(d.w.drawer.placement().unwrap().bounds,bounds);
                let offset = widgets(&list).find(|row|row.widget_name()==anchor.0 && row.is_mapped()).unwrap().compute_bounds(&scroll).unwrap().y();
                offsets.push(offset);
                assert!((offset-anchor.1).abs()<=2.,"{theme:?}/{count}: original {} offset {} -> {offsets:?} does not drift",anchor.0,anchor.1);
            }
            std::fs::write(output.join(format!("retention-{count}-{theme:?}.json")),serde_json::to_vec_pretty(&serde_json::json!({"rows":count,"theme":format!("{theme:?}"),"anchor":anchor.0,"anchor_mapped":retained_row.is_mapped(),"offset_before":anchor.1,"offset_after":retained,"offsets_after_roundtrips":offsets,"offset_delta":retained-anchor.1,"adjustment_before":position,"adjustment_after":scroll.vadjustment().value(),"drawer_height":bounds.height,"viewport_height":d.w.surface.height()})).unwrap()).unwrap();
            d.click(&footer);
            assert_eq!(stack.visible_child_name().as_deref(),Some("properties"));
            layer_page_settled(&d,&stack,"Properties page animation");
            assert_eq!(d.w.drawer.placement().unwrap().bounds, properties_bounds);
            let opacity = find_named(stack.child_by_name("properties").unwrap().upcast_ref(), "property-opacity").unwrap();
            assert!(opacity.is_sensitive(), "{theme:?}/{count}: Properties opacity is editable");
            let _warm = crate::snapshot(&d.w); pump(120);
            crate::snapshot(&d.w).save_to_png(output.join(format!("properties-{count}-{theme:?}.png"))).unwrap();
            let display = find_css(&opacity, "number-value").unwrap();
            let point = d.point(&display);
            let picked = d.w.window.pick(point[0] as f64, point[1] as f64, gtk::PickFlags::DEFAULT).unwrap();
            assert!(picked==display || picked.is_ancestor(&display), "{theme:?}/{count}: numeric value hit {} at {point:?}",picked.widget_name());
            d.number(&opacity, "75");
            assert_eq!(d.w.drawer.placement().unwrap().bounds, properties_bounds);
            d.click(&footer);
            layer_page_settled(&d,&stack,"Layers page animation");
            assert_eq!(d.w.drawer.placement().unwrap().bounds, bounds);
            if count == 6 {
                let added = layer_mutation_frame(&mut d,7,|d|d.click(&find_named(&page,"layer-add-layer-symbolic").unwrap()));
                assert!(added.height>bounds.height+20.);
                let deleted = layer_mutation_frame(&mut d,6,|d|d.click(&find_named(&page,"delete-selected-layers").unwrap()));
                assert!((deleted.height-bounds.height).abs()<2.);
                let undone = layer_mutation_frame(&mut d,7,|d|d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x7a,"down":true},{"key":0x7a,"down":false},{"key":0xffe3,"down":false}])));
                assert_eq!(undone.height,added.height);
                let redone = layer_mutation_frame(&mut d,6,|d|d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x79,"down":true},{"key":0x79,"down":false},{"key":0xffe3,"down":false}])));
                assert_eq!(redone.height,deleted.height);
            }
            if count == 12 {
                let ids:Vec<_> = state(&d.w).layers.iter().take(2).map(|row|row.id).collect();
                d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::SelectAllLayers {selected:false}});
                for id in ids {d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::ToggleSelection {id}});}
                d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::GroupSelected}); pump(300);
                let group = state(&d.w).layer_properties.layer.unwrap();
                let expanded = state(&d.w).layers.len();
                let group_row = find_named(&page, &format!("art-layer-{group}")).unwrap();
                let thumbnail = find_css(&group_row, "layer-thumbnail").unwrap();
                let expanded_height = d.w.drawer.placement().unwrap().bounds.height;
                let collapsed = layer_mutation_frame(&mut d,expanded-2,|d|d.click(&thumbnail));
                assert!(collapsed.height<expanded_height-20.);
                let restored = layer_mutation_frame(&mut d,expanded,|d|d.click(&find_css(&find_named(&page,&format!("art-layer-{group}")).unwrap(),"layer-thumbnail").unwrap()));
                assert_eq!(restored.height,expanded_height);
                d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x7a,"down":true},{"key":0x7a,"down":false},{"key":0xffe3,"down":false}]));
                assert_eq!(state(&d.w).layers.len(), count);
            }
            if count == 1000 {
                let clock = d.w.surface.frame_clock().unwrap();
                let point = d.point(scroll.upcast_ref());
                d.input.perform(serde_json::json!([{"point":point},{"wheel":[0,5]}]));
                pump(250);
                for run in 0..3 {
                    d.input.perform(serde_json::json!([{"point":point}]));
                    pump(100);
                    let timings = Rc::new(RefCell::new(Vec::new()));
                    let callbacks = Rc::new(Cell::new(0));
                    let value = Rc::new(Cell::new(scroll.vadjustment().value()));
                    let adjustment = scroll.vadjustment();
                    let scroll_start = adjustment.value();
                    let scroll_limit = adjustment.upper()-adjustment.page_size();
                    let first_direction = if scroll_start < scroll_limit*0.5 {1} else {-1};
                    let capture = clock.connect_after_paint(glib::clone!(#[strong] timings, #[strong] callbacks, #[strong] value, #[strong] adjustment, move |clock| {
                        callbacks.set(callbacks.get()+1);
                        if value.replace(adjustment.value()) != adjustment.value() && let Some(timing) = clock.current_timings() {
                            timings.borrow_mut().push(timing);
                        }
                    }));
                    let mut events = Vec::new();
                    if touch_motion {events.push(serde_json::json!({"touch":"down","point":point}));}
                    events.extend((0..1500).map(|step| {
                        if touch_motion {
                            let phase = (step as f32/1500.*12.)%2.;
                            let offset = if phase<1. {phase} else {2.-phase};
                            serde_json::json!({"touch":"move","point":[point[0],point[1]-offset*140.]})
                        } else {serde_json::json!({"wheel":[0,if (step/125)%2==0 {first_direction} else {-first_direction}]})}
                    }));
                    if touch_motion {events.push(serde_json::json!({"touch":"up"}));}
                    d.input.perform(events.into()); clock.disconnect(capture);
                    pump(150);
                    let mut times:Vec<_> = timings.borrow().iter().filter(|timing|timing.is_complete() && timing.presentation_time()>0).map(|timing|timing.presentation_time()).collect();
                    times.sort_unstable(); times.dedup();
                    std::fs::write(output.join(format!("scroll-timings-{theme:?}-{run}.json")),serde_json::to_vec_pretty(&serde_json::json!({"callbacks":callbacks.get(),"moving_timings":timings.borrow().iter().map(|timing|serde_json::json!({"frame_time":timing.frame_time(),"presentation_time":timing.presentation_time(),"complete":timing.is_complete()})).collect::<Vec<_>>()})).unwrap()).unwrap();
                    assert!(times.len()>100, "{theme:?}: {} moving large-list presentations at adjustment {} of {}",times.len(),scroll.vadjustment().value(),scroll.vadjustment().upper());
                    let seconds = (times.last().unwrap()-times[0]) as f64/1_000_000.;
                    assert!(seconds >= 5.);
                    assert_eq!(d.w.drawer.placement().unwrap().bounds, bounds);
                    assert_eq!(footer.compute_bounds(&d.w.surface).unwrap(),footer_bounds);
                    assert_eq!(actions.compute_bounds(&d.w.surface).unwrap(),action_bounds);
                    assert!(widgets(&list).filter(|widget|widget.widget_name().starts_with("art-layer-")).count()<256);
                    let extra:Vec<_> = d.w.drawers().iter().filter_map(|drawer|drawer.layers()).collect();
                    let realized = widgets(&list).filter(|widget|widget.widget_name().starts_with("art-layer-")).count();
                    reports.push(serde_json::json!({"theme":format!("{theme:?}"),"input":if touch_motion {"touch"} else {"mouse-wheel"},"run":run,"rows":count,"scroll_start":scroll_start,"scroll_end":adjustment.value(),"scroll_limit":scroll_limit,"first_direction":first_direction,"realized_rows":realized,"thumbnail_requests":d.w.layer_panel.preview_requests(),"thumbnail_work":d.w.layer_panel.preview_debug(&extra),"moving_presentations":times,"seconds":seconds,"viewport":[d.w.surface.width(),d.w.surface.height()],"scale":d.w.surface.scale_factor()}));
                    std::fs::write(output.join("large-list-scroll.json"),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
                }
            }
            let _warm = crate::snapshot(&d.w); pump(120);
            crate::snapshot(&d.w).save_to_png(output.join(format!("layers-{count}-{theme:?}.png"))).unwrap();
            if theme == Theme::Dark {heights.push(bounds.height);}
        }
    }
    assert!(heights[2] > heights[1] + 20., "drawer grows beyond six rows: {heights:?}");
    assert!((heights[3]-heights[4]).abs()<2., "large lists share the viewport cap: {heights:?}");
    std::fs::write(output.join("large-list-scroll.json"),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
    d.input.finish(); d.w.window.destroy(); pump(100);
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_sketch_layer_drawer_viewport_input"]
fn native_sketch_layer_drawer_viewport_input() {
    let mut d = Driver::new("art.capycanvas.LayerDrawerViewport");
    let output = std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from).unwrap_or_else(||d.input.dir.clone());
    std::fs::create_dir_all(&output).unwrap();
    for _ in 2..32 {d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::New {group:false,clipped:false}});}
    let opener = d.header_tool(ToolbarControl::Panel {panel:Panel::Layers});
    d.click_name(&opener);
    for theme in [Theme::Light,Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        let stack = d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
        layer_page_settled(&d,&stack,"full Layers viewport");
        let full = d.w.drawer.placement().unwrap().bounds;
        layer_drawer_viewport_input(&mut d,&output,theme,full);
    }
    d.input.finish(); d.w.window.destroy(); pump(100);
}

fn layer_drawer_viewport_input(d: &mut Driver, output: &std::path::Path, theme: Theme, full: Bounds) {
    let original_viewport = [d.w.surface.width(),d.w.surface.height()];
    let stack = d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
    let list = find_css(&stack.child_by_name("layers").unwrap(),"layer-list").unwrap();
    let scroll = list.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
    let before_anchor = layer_list_anchor(&list,&scroll);
    d.input.perform(serde_json::json!([{"point":d.point(&list)},{"wheel":[0,10000]}]));
    until(||(scroll.vadjustment().value()-(scroll.vadjustment().upper()-scroll.vadjustment().page_size())).abs()<=1.,"full viewport reaches end");
    layer_scroll_painted(d,&scroll);
    let after_anchor = layer_list_anchor(&list,&scroll);
    let point = screen_point(scroll.upcast_ref(),&d.w.window,[0.5,0.03]);
    let mut hit = d.w.window.pick(point[0] as f64,point[1] as f64,gtk::PickFlags::DEFAULT).unwrap();
    while !hit.widget_name().starts_with("art-layer-") {hit=hit.parent().unwrap();}
    let hit_id = hit.widget_name().to_string();
    std::fs::write(output.join(format!("wheel-anchor-{theme:?}.json")),serde_json::to_vec_pretty(&serde_json::json!({"before":before_anchor,"after":after_anchor,"native_top_hit":hit_id,"adjustment":scroll.vadjustment().value(),"limit":scroll.vadjustment().upper()-scroll.vadjustment().page_size()})).unwrap()).unwrap();
    let _warm = crate::snapshot(&d.w); pump(120);
    crate::snapshot(&d.w).save_to_png(output.join(format!("layers-wheel-end-before-properties-{theme:?}.png"))).unwrap();
    assert_ne!(hit_id,before_anchor.0,"native top hit changes when wheel reaches end");
    d.click_name("layer-properties-disclosure"); layer_page_settled(d,&stack,"Properties before viewport shrink");
    assert_eq!(stack.visible_child_name().as_deref(),Some("properties"));
    d.w.window.unmaximize(); d.w.window.set_default_size(900, 640);
    until(||!d.w.window.is_maximized() && d.w.surface.height() < 800, "short drawing window");
    pump(300);
    let footer = d.named("layer-properties-disclosure");
    layer_footer_key(d,&footer,&stack,"layers","Layers after Properties viewport shrink");
    let short = d.w.drawer.placement().unwrap().bounds;
    assert!(short.height <= d.w.window.height() as f32 * 0.85 + 2.);
    assert!(short.y >= 0. && short.y+short.height <= d.w.surface.height() as f32+1.);
    assert!(short.height < full.height, "drawer follows a shorter window");
    let footer = d.named("layer-properties-disclosure");
    let stack = d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
    let page = stack.child_by_name("layers").unwrap();
    let list = find_css(&page,"layer-list").unwrap();
    let scroll = list.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
    d.input.perform(serde_json::json!([{"point":d.point(&list)},{"wheel":[0,10000]}]));
    until(||(scroll.vadjustment().value()-(scroll.vadjustment().upper()-scroll.vadjustment().page_size())).abs()<=1.,"short viewport scroll reaches end");
    layer_scroll_painted(d,&scroll);
    layer_footer_key(d,&footer,&stack,"properties","short Properties animation");
    assert!(d.w.drawer.placement().unwrap().bounds.height<=short.height);
    layer_footer_key(d,&footer,&stack,"layers","short Layers animation");
    assert_eq!(d.w.drawer.placement().unwrap().bounds,short);
    let _warm = crate::snapshot(&d.w); pump(120);
    crate::snapshot(&d.w).save_to_png(output.join(format!("layers-near-end-short-{theme:?}.png"))).unwrap();
    d.input.perform(serde_json::json!([{"point":d.point(&list)},{"wheel":[0,10000]}]));
    until(||(scroll.vadjustment().value()-(scroll.vadjustment().upper()-scroll.vadjustment().page_size())).abs()<=1.,"near-end viewport growth starts at end");
    layer_scroll_painted(d,&scroll);
    let original_anchor = layer_list_anchor(&list,&scroll);
    let original_position = scroll.vadjustment().value();
    layer_footer_key(d,&footer,&stack,"properties","near-end Properties before viewport growth");
    d.w.window.maximize();
    until(||d.w.window.is_maximized() && [d.w.surface.width(),d.w.surface.height()]==original_viewport, "drawing viewport maximized"); pump(300);
    let footer = widgets(d.w.window.upcast_ref()).find(|widget|widget.widget_name()=="layer-properties-disclosure" && widget.is_mapped()).unwrap();
    let stack = widgets(d.w.window.upcast_ref()).find(|widget|widget.widget_name()=="layer-properties-stack" && widget.is_mapped()).unwrap().downcast::<gtk::Stack>().unwrap();
    let point = d.point(&footer);
    let picked = d.w.window.pick(point[0] as f64,point[1] as f64,gtk::PickFlags::DEFAULT).unwrap();
    assert!(picked==footer || picked.is_ancestor(&footer),"maximized Properties footer receives native contact: {picked:?}");
    d.click(&footer); layer_page_settled(d,&stack,"near-end Layers after viewport growth");
    assert_eq!(stack.visible_child_name().as_deref(),Some("layers"));
    let list = find_css(&stack.child_by_name("layers").unwrap(),"layer-list").unwrap();
    let scroll = list.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
    assert!((d.w.drawer.placement().unwrap().bounds.height-full.height).abs()<2.,"near-end Layers height {:?}, expected {}, viewport {}x{}, page {:?}, state {:?}",d.w.drawer.placement().unwrap().bounds,full.height,d.w.surface.width(),d.w.surface.height(),stack.visible_child_name(),state(&d.w).customization.drawer.as_ref().map(|drawer|drawer.layer_properties));
    let end = scroll.vadjustment().value();
    let limit = scroll.vadjustment().upper()-scroll.vadjustment().page_size();
    let returned = widgets(&list).find(|row|row.widget_name()==original_anchor.0 && row.is_mapped()).unwrap().compute_bounds(&scroll).unwrap().y();
    let wanted = end+(returned-original_anchor.1) as f64;
    std::fs::write(output.join(format!("viewport-anchor-{theme:?}.json")),serde_json::to_vec_pretty(&serde_json::json!({"anchor":original_anchor.0,"offset_before":original_anchor.1,"offset_after":returned,"position_before":original_position,"position_after":end,"wanted_position":wanted,"limit":limit,"viewport":[d.w.surface.width(),d.w.surface.height()]})).unwrap()).unwrap();
    assert!(end>=scroll.vadjustment().lower() && end<=limit,"resized scroll position stays within its bounds");
    if wanted>=scroll.vadjustment().lower() && wanted<=limit {
        assert!((returned-original_anchor.1).abs()<=2.,"feasible viewport anchor is preserved: {} -> {returned}",original_anchor.1);
    } else {
        assert!((end-wanted.clamp(scroll.vadjustment().lower(),limit)).abs()<=2.,"impossible viewport anchor is clamped: value {end}, wanted {wanted}, limit {limit}");
    }
    d.input.perform(serde_json::json!([{"point":d.point(&list)},{"wheel":[0,-3]}]));
    assert!(scroll.vadjustment().value()<end-20.,"list remains scrollable after an impossible old anchor is clamped");
}

fn layer_footer_key(d: &mut Driver, footer: &gtk::Widget, stack: &gtk::Stack, page: &str, label: &str) {
    assert!(footer.grab_focus());
    assert_eq!(gtk::prelude::GtkWindowExt::focus(&d.w.window),Some(footer.clone()));
    until(||d.w.window.is_active(),"native footer window active");
    d.input.key(0xff0d);
    until(||stack.visible_child_name().as_deref()==Some(page),label);
    layer_page_settled(d,stack,label);
}

fn layer_scroll_painted(d: &Driver, scroll: &gtk::ScrolledWindow) {
    let painted = Rc::new(Cell::new(false));
    let clock = d.w.surface.frame_clock().unwrap();
    let previous = clock.frame_counter();
    let capture = clock.connect_after_paint(glib::clone!(#[strong] painted, #[weak] scroll, move |clock| {
        if clock.frame_counter()>previous && (scroll.vadjustment().value()-(scroll.vadjustment().upper()-scroll.vadjustment().page_size())).abs()<=1. {painted.set(true);}
    }));
    d.w.window.queue_draw();
    until(||painted.get(),"native end-scroll after-paint frame");
    clock.disconnect(capture);
}

fn assert_shared_icons(widget: &gtk::Widget) {
    if let Some(image) = widget.downcast_ref::<gtk::Image>()
        && let Some(name) = crate::icons::name(image).filter(|name| name.starts_with("layer-"))
    {
        assert!(
            image.paintable().is_some_and(|p| p.is::<gtk::Svg>()),
            "{name} must use the shared SVG renderer, not theme-symbolic loading"
        );
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        child = widget.next_sibling();
        assert_shared_icons(&widget);
    }
}

pub(super) struct Driver {
    pub(super) w: Rc<Workspace>,
    _app: NativeTestApp,
    pub(super) input: RemoteInput,
}
impl Driver {
    fn managed(name: &str) -> Self {
        assert!(crate::storage::workspaces().is_some());
        let app = native_test_app(name);
        let w = Workspace::new(&app);
        w.window.maximize();
        w.window.present();
        wait_workspaces(&w);
        pump(500);
        Self::start(w, app)
    }
    pub(super) fn start(w: Rc<Workspace>, app: NativeTestApp) -> Self {
        let input = RemoteInput::new().settle_ms(180).timeout_secs(8);
        input.ready();
        pump(500);
        Self {
            w,
            _app: app,
            input,
        }
    }
    fn capture_canvas(&self, name: &str) {
        // As in native_header_canvas_visual, settle the screenshot-only
        // paintable before inspecting GTK's cached whole-window scene.
        let _warm = crate::snapshot(&self.w);
        pump(120);
        let snapshot = crate::snapshot(&self.w);
        assert!(
            canvas_paper(&self.w, &snapshot) > 100_000,
            "paper remains visible"
        );
        snapshot.save_to_png(self.input.dir.join(name)).unwrap();
    }
    fn header_tool(&self, control: ToolbarControl) -> String {
        let id = state(&self.w)
            .workspace
            .layout
            .header
            .entries()
            .find(|e| e.item == HeaderItem::Tool { control })
            .unwrap()
            .id;
        format!("header-item-{id}")
    }
    fn header_icon(&self, command: CommandId, icon: &str) -> gtk::Widget {
        let root = self.named(&self.header_tool(ToolbarControl::Command { command }));
        find_named(&root, &format!("layer-{icon}-symbolic"))
            .unwrap_or_else(|| panic!("{command:?} should display {icon}"))
    }
    fn number(&mut self, root: &gtk::Widget, text: &str) {
        self.click(&find_css(root, "number-value").unwrap());
        let entry = find_css(root, "number-entry")
            .unwrap()
            .downcast::<gtk::Entry>()
            .unwrap();
        assert!(entry.is_mapped());
        self.input.perform(serde_json::json!([{ "key": 0xffe3, "down": true }, { "key": 0x61, "down": true }, { "key": 0x61, "down": false }, { "key": 0xffe3, "down": false }]));
        for c in text.chars() {
            self.input.key(c as u32);
        }
        self.input.key(0xff0d);
    }
    pub(super) fn new(name: &str) -> Self {
        let app = native_test_app(name);
        let w = fixture_workspace(&app);
        w.window.maximize();
        w.window.present();
        wait_workspaces(&w);
        w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(WorkspaceState {
                layout: WorkspacePreset::Painter.layout(Platform::Gtk),
                ..WorkspaceState::default()
            }),
        });
        pump(500);
        Self::start(w, app)
    }
    pub(super) fn point(&self, widget: &gtk::Widget) -> [f32; 2] {
        assert!(widget.is_mapped(), "{} is not mapped", widget.widget_name());
        screen_point(widget, &self.w.window, [0.5, 0.5])
    }
    pub(super) fn named(&self, name: &str) -> gtk::Widget {
        find_named(self.w.window.upcast_ref(), name).unwrap_or_else(|| panic!("missing {name}"))
    }
    pub(super) fn click(&mut self, widget: &gtk::Widget) {
        let p = self.point(widget);
        self.input.click(p);
    }
    fn click_name(&mut self, name: &str) {
        self.click(&self.named(name));
    }
    fn label(&self, text: &str) -> gtk::Widget {
        mapped_label(self.w.window.upcast_ref(), text).unwrap_or_else(|| panic!("no visible label {text}"))
    }
    fn click_label(&mut self, text: &str) {
        // Native menu pages animate and can scroll at short window heights.
        // A mapped label is not necessarily inside its visible viewport.
        pump(250);
        let label = self.label(text);
        let point = self.point(&label);
        if point[1] < 0. || point[1] > self.w.window.height() as f32 {
            let mut row = label.clone();
            while !row.is_focusable() {
                row = row.parent().unwrap();
            }
            assert!(row.grab_focus());
            pump(250);
        }
        self.click(&label);
    }
    fn edit(&mut self) {
        // Exercise a discoverable entry, not a test-only action or shortcut.
        let item = state(&self.w)
            .workspace
            .layout
            .header
            .entries()
            .find(|e| self.named(&format!("header-item-{}", e.id)).is_mapped())
            .cloned();
        if let Some(item) = item {
            let p = self.point(&self.named(&format!("header-item-{}", item.id)));
            self.input.perform(serde_json::json!([{"point":p},{"button":273,"down":true},{"button":273,"down":false}]));
        } else {
            self.click_name("header-recovery");
            self.click_label("Window");
        }
        self.click_label("Customize Title Bar…");
        if !state(&self.w).customization.header_editing {
            crate::capture(
                &self.w,
                self.input.dir.join("failed-entry.png").to_str().unwrap(),
            );
            eprintln!(
                "Editor entry focus: {:?}",
                gtk::prelude::GtkWindowExt::focus(&self.w.window)
            );
        }
        assert!(state(&self.w).customization.header_editing);
    }
    fn drop_component(&mut self, name: &str, zone: HeaderZone) {
        let geometry = self.w.header.geometry_for_test();
        let b = geometry.zones[zone.index()];
        let point = [b.x + b.width - 2., b.y + b.height / 2.];
        self.drop_component_at(name, point, false);
    }
    fn drop_component_at(&mut self, name: &str, point: [f32; 2], touch: bool) {
        let device = if touch { "touch" } else { "mouse" };
        let start = self.point(&self.named(&format!("header-add-{name}")));
        self.input.perform(serde_json::json!([
            contact(device, "down", start),
            contact(device, "move", point)
        ]));
        assert!(self.w.dragging.get(), "immediate whole-body pickup: {name}");
        assert!(
            self.w.header.drag_for_test().unwrap().action.is_some(),
            "valid drop: {name}"
        );
        self.input
            .perform(serde_json::json!([contact(device, "up", point)]));
    }
    fn drop_component_before(&mut self, name: &str, before: u32, touch: bool) {
        let bounds = self
            .named(&format!("header-item-{before}"))
            .compute_bounds(&self.w.surface)
            .unwrap();
        self.drop_component_at(
            name,
            [bounds.x() + 2., bounds.y() + bounds.height() / 2.],
            touch,
        );
    }
    pub(super) fn finish(self) {
        self.input.finish();
        self.w.window.close();
        pump(200);
    }
}

#[test]
#[ignore = "private display and native pointer menu heading navigation"]
fn native_header_menu_hover_input() {
    let mut d = Driver::new("art.capycanvas.HeaderMenuHover");
    let mut workspace = state(&d.w).workspace;
    workspace.layout.header = Default::default();
    d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(workspace) });
    pump(250);
    let menus = ApplicationMenu::ALL.map(|id| (id, named::<gtk::MenuButton>(d.w.window.upcast_ref(), &format!("application-menu-{id:?}"))));
    let opened = || menus.iter().filter(|(_, button)| button.popover().unwrap().is_mapped()).map(|(id, _)| *id).collect::<Vec<_>>();
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        for id in [ApplicationMenu::File, ApplicationMenu::Edit, ApplicationMenu::View] {
            let button = &menus.iter().find(|(candidate, _)| *candidate == id).unwrap().1;
            let point = d.point(button.upcast_ref());
            d.input.perform(serde_json::json!([{"point": point}]));
            assert!(opened().is_empty(), "{theme:?}: closed menus stay closed over {id:?}");
        }
        let file = &menus[0].1;
        d.click(file.upcast_ref());
        assert_eq!(opened(), vec![ApplicationMenu::File], "{theme:?}: File opens by click");
        for id in [ApplicationMenu::Edit, ApplicationMenu::View, ApplicationMenu::File] {
            let button = &menus.iter().find(|(candidate, _)| *candidate == id).unwrap().1;
            let point = d.point(button.upcast_ref());
            d.input.perform(serde_json::json!([{"point": point}]));
            assert_eq!(opened(), vec![id], "{theme:?}: hover switches to exactly one {id:?} popup");
            if id == ApplicationMenu::View && std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some() {
                d.input.perform(serde_json::json!([{"capture": format!("menu-hover-{}", format!("{theme:?}").to_lowercase())}]));
            }
        }
        d.input.key(0xff1b);
        assert!(opened().is_empty(), "{theme:?}: Escape closes the popup");
        let point = d.point(menus[1].1.upcast_ref());
        d.input.perform(serde_json::json!([{"point": point}]));
        assert!(opened().is_empty(), "{theme:?}: hover after Escape stays closed");
        d.click(file.upcast_ref());
        assert_eq!(opened(), vec![ApplicationMenu::File], "{theme:?}: click reopens File");
        d.click(file.upcast_ref());
        assert!(opened().is_empty(), "{theme:?}: second click toggles File closed");
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_managed_input --native-storage"]
fn native_header_managed_input() {
    let mut d = Driver::managed("art.capycanvas.HeaderStorage");
    assert_eq!(
        d.w.workspaces
            .manager()
            .unwrap()
            .active_name()
            .as_deref(),
        Some("Paint")
    );
    for name in ["Sketch", "Paint", "Photo"] {
        d.label(name);
    }
    d.capture_canvas("paint-default.png");
    d.click_name("workspace-switch-painter");
    wait_workspaces(&d.w);
    pump(400);
    assert_eq!(state(&d.w).workspace.layout.header, HeaderLayout::painter_for_platform(Platform::Gtk));
    assert_eq!(
        d.w.workspaces
            .manager()
            .unwrap()
            .active_name()
            .as_deref(),
        Some("Sketch")
    );
    d.capture_canvas("sketch-default.png");
    let switcher =
        d.w.workspaces
            .switcher
            .compute_bounds(&d.w.surface)
            .unwrap();
    assert!(
        (switcher.x() + switcher.width() / 2. - d.w.surface.width() as f32 / 2.).abs() < 1.,
        "visible workspace choices, not just their container, must be centered"
    );
    d.edit();
    d.click_name("header-size-2");
    pump(120);
    let capy = state(&d.w)
        .workspace
        .layout
        .header
        .entries()
        .find(|e| e.item == HeaderItem::Capy)
        .unwrap()
        .id;
    d.named(&format!("header-item-{capy}")).grab_focus();
    d.input.key(0xffff);
    assert!(state(&d.w).workspace.layout.header.entry(capy).is_err());
    assert!(
        gtk::prelude::GtkWindowExt::focus(&d.w.window)
            .is_some_and(|f| f.widget_name().starts_with("header-item-")),
        "keyboard position survives removal"
    );
    d.click_name("header-canvas-info");
    d.drop_component("tools", HeaderZone::Right);
    d.named("tool-search")
        .downcast::<gtk::SearchEntry>()
        .unwrap()
        .set_text("Brush size");
    pump(300);
    d.click_name(&format!(
        "tool-choice-{}",
        serde_json::to_string(&ToolbarControl::Panel { panel: Panel::Sizes }).unwrap()
    ));
    d.click_name("confirm-tools");
    assert!(state(&d.w).customization.header_editing);
    d.click_name("header-edit-done");
    let saved = durable_layout(&state(&d.w).workspace.layout);
    for name in ["illustrator", "painter"] {
        d.click_name(&format!("workspace-switch-{name}"));
        wait_workspaces(&d.w);
        pump(400);
    }
    assert_eq!(durable_layout(&state(&d.w).workspace.layout), saved);
    d.edit();
    d.click_name("header-size-0");
    pump(150);
    assert_ne!(state(&d.w).workspace.layout.header, saved.header);
    // Closing without Done must persist the committed workspace, not the preview.
    d.w.window.close();
    until(|| !d.w.window.is_visible(), "window close");
    d.w = Workspace::new(&d._app);
    d.w.window.maximize();
    d.w.window.present();
    wait_workspaces(&d.w);
    pump(500);
    assert_eq!(
        d.w.workspaces
            .manager()
            .unwrap()
            .active_name()
            .as_deref(),
        Some("Sketch")
    );
    assert_eq!(
        durable_layout(&state(&d.w).workspace.layout),
        saved,
        "restart preserves header and canvas info"
    );
    d.click_name(&d.header_tool(ToolbarControl::Color));
    assert!(state(&d.w).customization.drawer.is_some());
    d.w.dispatch(UiAction::Customize {
        action: CustomizationAction::CloseExpanded,
    });
    pump(100);
    d.capture_canvas("managed-painter.png");
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_workspace_ownership_input --native-storage"]
fn native_workspace_ownership_input() {
    let mut d = Driver::managed("art.capycanvas.WorkspaceOwnership");
    let database = crate::storage::workspaces().unwrap().to_path_buf()
        .join("workspaces.sqlite3");
    let original =
        d.w.workspaces
            .manager()
            .unwrap()
            .active_id()
            .unwrap();
    let target = layer_workspace::DEFAULT_WORKSPACES
        .iter()
        .find(|(id, _)| *id != original)
        .unwrap();
    let target_id = target.0;
    let target_button = match target.1 {
        WorkspacePreset::Painter => "workspace-switch-painter",
        WorkspacePreset::Illustrator => "workspace-switch-illustrator",
        WorkspacePreset::Photographer => "workspace-switch-photographer",
    };
    let mut external = layer_workspace::SqliteStore::open(&database).unwrap();
    let owner = layer_workspace::Owner::fresh();
    let saved = external.claim(target_id, owner.clone()).unwrap();
    // A live but non-renewing owner remains protected after its timer expires.
    let output = std::process::Command::new("sqlite3")
        .arg(&database)
        .arg(format!(
            "UPDATE items SET lease_until='0' WHERE id='{target_id}'"
        ))
        .output()
        .unwrap();
    assert!(output.status.success());
    d.click_name(target_button);
    wait_workspaces(&d.w);
    assert_eq!(
        d.w.workspaces
            .manager()
            .unwrap()
            .active_id()
            .as_deref(),
        Some(original.as_str())
    );
    assert_eq!(
        external.load(target_id).unwrap().claim.unwrap().owner,
        owner
    );
    // Keep the GTK process/worker alive while this independent owner exits.
    drop(external);
    d.click_name(target_button);
    wait_workspaces(&d.w);
    let manager = d.w.workspaces.manager().unwrap();
    assert_eq!(manager.active_id().as_deref(), Some(target_id));
    assert!(manager.error().is_none());
    let adopted = saved.entity.working.unwrap();
    assert_eq!(manager.current().unwrap().working, Some(adopted));

    // Context menus created while an item was occupied use SwitchToWindow.
    // That stale action must now reclaim the item too, rather than get stuck.
    let mut external = layer_workspace::SqliteStore::open(&database).unwrap();
    external
        .claim(&original, layer_workspace::Owner::fresh())
        .unwrap();
    glib::MainContext::default()
        .block_on(manager.refresh())
        .unwrap();
    drop(external);
    perform(
        &d.w,
        layer_workspace::ManagerAction::SwitchToWindow(original.clone()),
    );
    wait_workspaces(&d.w);
    assert_eq!(manager.active_id().as_deref(), Some(original.as_str()));

    // An actual second GTK window is focused, not stolen. Closing it makes
    // its workspace immediately available to the first window.
    let second = Workspace::new(&d._app);
    second.window.present();
    wait_workspaces(&second);
    let second_id = second
        .workspaces
        .manager()
        .unwrap()
        .active_id()
        .unwrap();
    assert_ne!(second_id, original);
    perform(
        &d.w,
        layer_workspace::ManagerAction::Switch(second_id.clone()),
    );
    pump(500);
    assert_eq!(manager.active_id().as_deref(), Some(original.as_str()));
    assert!(
        second.window.is_active(),
        "switching to a live owner focuses its GTK window"
    );
    second.window.close();
    until(|| !second.window.is_visible(), "second window close");
    d.w.window.present();
    perform(
        &d.w,
        layer_workspace::ManagerAction::Switch(second_id.clone()),
    );
    wait_workspaces(&d.w);
    assert_eq!(manager.active_id().as_deref(), Some(second_id.as_str()));
    assert!(manager.error().is_none());
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_unreadable_workspace_storage_input --native-storage"]
fn native_unreadable_workspace_storage_input() {
    let mut d = Driver::managed("art.capycanvas.UnreadableWorkspaceStorage");
    let database = crate::storage::workspaces().unwrap().to_path_buf()
        .join("workspaces.sqlite3");
    let sql = |query: &str| {
        let output = std::process::Command::new("sqlite3")
            .arg(&database)
            .arg(query)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    d.w.window.close();
    until(|| !d.w.window.is_visible(), "workspace close");
    assert_eq!(
        sql("UPDATE items SET working=json_set(working,'$.zen_mode',json('{}')); SELECT changes()"),
        "3"
    );
    let settings = crate::storage::roots().unwrap().settings();
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(
        &settings,
        r#"{"pan_speed":2.0,"tip_lock":true,"prediction_algorithm":"kalman","zoom_speed":"fast"}"#,
    )
    .unwrap();
    std::fs::write(
        crate::storage::roots().unwrap().export_presets(),
        r#"{"destinations":[null,null,null,null],"named":[]}"#,
    )
    .unwrap();
    d.w = Workspace::new(&d._app);
    d.w.window.maximize();
    d.w.window.present();
    wait_workspaces(&d.w);
    pump(500);
    let manager = d.w.workspaces.manager().unwrap();
    assert!(manager.error().is_none());
    assert!(!manager.in_memory());
    assert_eq!(
        state(&d.w).notice.map(|notice| notice.text),
        Some("Saved workspaces couldn't be opened, so they were reset.".into())
    );
    assert!(d.w.area.is_mapped());
    assert_eq!(state(&d.w).settings.pan_speed, 2.0);
    assert_eq!(state(&d.w).settings.zoom_speed, 1.0);
    assert_eq!(
        sql("SELECT count(*) FROM items WHERE json_type(working,'$.zen_mode')='object'"),
        "0"
    );
    let kept = std::process::Command::new("sqlite3")
        .arg(database.with_file_name("workspaces.unreadable.sqlite3"))
        .arg("SELECT count(*) FROM items WHERE json_type(working,'$.zen_mode')='object'")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(kept.stdout).unwrap().trim(), "3");
    d.w.dispatch(UiAction::Invoke {
        command: CommandId::ExportDocument,
    });
    until(
        || {
            d.w.window
                .visible_dialog()
                .is_some_and(|dialog| dialog.widget_name() == "export-options")
        },
        "export dialog with unreadable presets",
    );
    assert!(state(&d.w).host_error.is_none(), "{:?}", state(&d.w).host_error);
    d.w.window.visible_dialog().unwrap().close();
    until(|| d.w.window.visible_dialog().is_none(), "export dialog close");
    d.click_name("workspace-switch-painter");
    wait_workspaces(&d.w);
    pump(400);
    assert_eq!(
        state(&d.w).workspace.layout.header,
        HeaderLayout::painter_for_platform(Platform::Gtk)
    );
    assert!(manager.error().is_none());
    d.click_name(&d.header_tool(ToolbarControl::Color));
    assert!(state(&d.w).customization.drawer.is_some());
    crate::capture(
        &d.w,
        d.input.dir.join("replaced-storage.png").to_str().unwrap(),
    );
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_drag_only_bank_input"]
fn native_header_drag_only_bank_input() {
    let mut d = Driver::new("art.capycanvas.HeaderDragOnly");
    let original = state(&d.w).workspace;
    d.edit();
    fn inert(root: &gtk::Widget) {
        for widget in widgets(root) {
            assert!(!widget.is::<gtk::Button>() && !widget.is::<gtk::MenuButton>());
            assert!(!widget.is_focusable(), "bank components are not activation controls");
        }
    }
    // Neither a tap, a hold nor sub-slop motion adds anything or opens Tools.
    for name in [
        "tools",
        "clock",
        "battery",
        "document-title",
        "menu-labels",
        "space",
    ] {
        let chip = d.named(&format!("header-component-{name}"));
        inert(&chip);
        let p = d.point(&chip);
        for touch in [false, true] {
            let device = if touch { "touch" } else { "mouse" };
            let moved = [p[0] + 1., p[1]];
            d.input.perform(serde_json::json!([
                contact(device, "down", p),
                contact(device, "move", moved),
                contact(device, "up", moved)
            ]));
            assert_eq!(state(&d.w).workspace.layout.header, original.layout.header);
            assert!(d.w.window.visible_dialog().is_none());
            assert!(!d.w.dragging.get() && d.w.workspace_drag.borrow().is_none());
        }
    }
    for touch in [false, true] {
        let device = if touch { "touch" } else { "mouse" };
        let p = d.point(&d.named("header-component-tools"));
        let mut events = vec![contact(device, "down", p)];
        events.extend((0..12).map(|_| serde_json::json!({})));
        events.push(contact(device, "up", p));
        d.input.perform(serde_json::Value::Array(events));
        assert!(d.w.window.visible_dialog().is_none());
        assert_eq!(state(&d.w).workspace.layout.header, original.layout.header);
    }
    // Every visible part belongs to the same source, not just its old grip.
    for touch in [false, true] {
        let device = if touch { "touch" } else { "mouse" };
        for hit in 0..4 {
            let chip = d.named("header-component-clock");
            let b = chip.compute_bounds(&d.w.surface).unwrap();
            let body = d.named("header-add-clock");
            let p = match hit {
                0 => [b.x() + 2., b.y() + b.height() / 2.],
                1 => d.point(&body.first_child().unwrap()),
                2 => d.point(&body.last_child().unwrap()),
                _ => [b.x() + b.width() - 2., b.y() + b.height() - 2.],
            };
            let (source, target) = d.w.drag_source_at(p).unwrap();
            assert_eq!(source, chip);
            assert!(matches!(
                target,
                DragTarget::Header(HeaderDragSource::Component(HeaderItem::Clock))
            ));
            let to = [d.w.surface.width() as f32 / 2., 25.];
            d.input.perform(serde_json::json!([
                contact(device, "down", p),
                contact(device, "move", to)
            ]));
            assert!(d.w.dragging.get(), "touch={touch}, hit={hit}");
            assert!(!d.w.workspace_drag.borrow().as_ref().unwrap().wait_for_hold);
            assert_eq!(
                state(&d.w).workspace.layout.header,
                original.layout.header,
                "motion stays a preview"
            );
            d.input
                .perform(serde_json::json!([contact(device, "up", to)]));
            let model = state(&d.w).workspace.layout.header;
            let clock = model
                .entries()
                .find(|e| e.item == HeaderItem::Clock)
                .unwrap()
                .id;
            assert_eq!(model.location(clock).unwrap().0, HeaderZone::Center);
            assert!(!chip.is_mapped());
            d.click_name("header-edit-cancel");
            assert_eq!(state(&d.w).workspace.layout.header, original.layout.header);
            d.edit();
        }
    }
    crate::capture(
        &d.w,
        d.input.dir.join("drag-only-bank.png").to_str().unwrap(),
    );
    d.click_name("header-edit-cancel");
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_catalog_preview_input"]
fn native_header_catalog_preview_input() {
    let mut d = Driver::new("art.capycanvas.HeaderCatalog");
    let original = state(&d.w).workspace.layout.header;
    let capture =
        ui_session_mut(&d.w)
            .capture_workspace()
            .unwrap();
    for (touch, grip, held) in [
        (false, false, false),
        (false, false, true),
        (false, true, false),
        (true, false, false),
        (true, false, true),
        (true, true, false),
    ] {
        let device = if touch { "touch" } else { "mouse" };
        d.edit();
        let name = if grip {
            "header-add-grip-clock"
        } else {
            "header-add-clock"
        };
        let source = d.point(&d.named(name));
        let destination = [d.w.surface.width() as f32 / 2., 30.];
        let mut events = vec![contact(device, "down", source)];
        if held {
            events.extend((0..12).map(|_| serde_json::json!({})));
        }
        events.extend([
            contact(device, "move", [source[0], source[1] - 20.]),
            contact(device, "move", destination),
            contact(device, "up", destination),
        ]);
        d.input.perform(serde_json::Value::Array(events));
        let layout = state(&d.w).workspace.layout.header;
        assert_eq!(
            layout.entries().count(),
            original.entries().count() + 1,
            "touch={touch} grip={grip} held={held}"
        );
        let entry = layout
            .entries()
            .find(|e| e.item == HeaderItem::Clock)
            .unwrap();
        assert_eq!(layout.location(entry.id).unwrap().0, HeaderZone::Center);
        assert!(!d.w.dragging.get() && d.w.workspace_drag.borrow().is_none());
        assert_eq!(
            ui_session_mut(&d.w)
                .capture_workspace()
                .unwrap()
                .history,
            capture.history,
            "preview must not be autosaved"
        );
        d.click_name("header-edit-cancel");
        assert_eq!(state(&d.w).workspace.layout.header, original);
    }
    d.edit();
    // Cancel outside, Escape, blur and replacement without ever creating a tile.
    for cancellation in 0..4 {
        let source = d.point(&d.named("header-add-grip-clock"));
        let destination = [d.w.surface.width() as f32 / 2., 30.];
        d.input
            .perform(serde_json::json!([{"point":source},{"down":true},{"point":destination}]));
        assert!(d.w.dragging.get());
        match cancellation {
            0 => d.input.perform(serde_json::json!([{"point":[900.,700.]}])),
            1 => d.input.key(0xff1b),
            2 => {
                d.w.interact(UiInput::Blur);
            }
            _ => {
                d.w.dispatch(
                    HeaderAction::SetSize {
                        size: HeaderSize::Large,
                    }
                    .action(),
                );
                d.w.dispatch(
                    HeaderAction::SetSize {
                        size: original.size,
                    }
                    .action(),
                );
                pump(250);
            }
        }
        d.input.perform(serde_json::json!([{"down":false}]));
        assert!(!d.w.dragging.get() && d.w.workspace_drag.borrow().is_none());
        assert_eq!(state(&d.w).workspace.layout.header, original);
        assert!(state(&d.w).customization.header_editing);
    }
    // The ordinary tool picker adds into this preview, not directly to storage.
    d.drop_component("tools", HeaderZone::Right);
    d.named("tool-search")
        .downcast::<gtk::SearchEntry>()
        .unwrap()
        .set_text("Brush size");
    pump(250);
    d.click_name(&format!(
        "tool-choice-{}",
        serde_json::to_string(&ToolbarControl::Panel { panel: Panel::Sizes }).unwrap()
    ));
    d.click_name("confirm-tools");
    assert!(state(&d.w).customization.header_editing);
    let preview = state(&d.w).workspace.layout.header;
    d.click_name("header-edit-done");
    assert_eq!(state(&d.w).workspace.layout.header, preview);
    let saved =
        ui_session_mut(&d.w)
            .capture_workspace()
            .unwrap();
    assert_eq!(saved.history.generation, capture.history.generation + 1);
    d.edit();
    d.click_name("header-size-2");
    pump(120);
    d.click_name("header-edit-cancel");
    assert!(!state(&d.w).customization.header_editing);
    assert_eq!(state(&d.w).workspace.layout.header, preview);
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_picker_journey"]
fn native_header_picker_journey() {
    let mut d = Driver::new("art.capycanvas.HeaderPickerJourney");
    let original = state(&d.w).workspace.layout.header;
    let opacity = format!(
        "tool-choice-{}",
        serde_json::to_string(&ToolbarControl::Panel { panel: Panel::Sizes }).unwrap()
    );
    let color = format!(
        "tool-choice-{}",
        serde_json::to_string(&ToolbarControl::Color).unwrap()
    );
    let search = |d: &Driver, query| {
        d.named("tool-search")
            .downcast::<gtk::SearchEntry>()
            .unwrap()
            .set_text(query);
        pump(300);
    };
    fn choose(d: &mut Driver, name: &str) {
        let choice = d.named(name);
        let results = choice.ancestor(gtk::ScrolledWindow::static_type()).unwrap();
        assert!(choice.grab_focus());
        let deadline = Instant::now() + Duration::from_secs(2);
        while !choice
            .compute_bounds(&results)
            .is_some_and(|b| b.y() >= 0. && b.y() + b.height() <= results.height() as f32)
        {
            assert!(Instant::now() < deadline, "{name} scrolls into view");
            pump(10);
        }
        d.click(&choice);
    }
    for touch in [false, true] {
        d.edit();
        let before = original.zones[2][0].id;
        let item = d.named(&format!("header-item-{before}"));
        let b = item.compute_bounds(&d.w.window).unwrap();
        let point = [b.x() + 25., b.y() + b.height() / 2.]; // Body, before its midpoint; not its grip.
        d.input.perform(if touch {
            serde_json::json!([{"touch":"down","point":point},{"touch":"up"}])
        } else {
            serde_json::json!([{"point":point},{"down":true},{"down":false}])
        });
        assert!(item.has_css_class("editing-selection"));
        d.drop_component_before("tools", before, touch);
        assert_shared_icons(d.w.window.visible_dialog().unwrap().upcast_ref());
        assert!(d.w.window.visible_dialog().is_some());
        assert!(!d.named("toolbar-name").is_mapped());
        assert!(!d.named("confirm-tools").is_sensitive());
        search(&d, "Brush size");
        choose(&mut d, &opacity);
        search(&d, "Color");
        choose(&mut d, &color);
        search(&d, "No tool matches this query");
        assert!(d.label("No matching tools").is_mapped());
        assert_eq!(
            state(&d.w)
                .customization
                .picker
                .as_ref()
                .unwrap()
                .selected
                .len(),
            2
        );
        assert!(
            d.named("confirm-tools").is_sensitive(),
            "Filtering retains selected tools"
        );
        crate::capture(
            &d.w,
            d.input
                .dir
                .join(format!("picker-empty-{touch}.png"))
                .to_str()
                .unwrap(),
        );
        if touch {
            d.click_name("cancel-tools");
        } else {
            d.input.key(0xff1b);
        }
        assert!(
            state(&d.w).customization.header_editing,
            "Cancel/Escape closes the picker, not its parent preview"
        );
        assert!(state(&d.w).customization.picker.is_none());
        assert_eq!(state(&d.w).workspace.layout.header, original);
        d.drop_component_before("tools", before, touch);
        assert_eq!(
            d.named("tool-search")
                .downcast::<gtk::SearchEntry>()
                .unwrap()
                .text(),
            ""
        );
        assert!(!d.named("confirm-tools").is_sensitive());
        search(&d, "Brush size");
        choose(&mut d, &opacity);
        search(&d, "Color");
        choose(&mut d, &color);
        crate::capture(
            &d.w,
            d.input
                .dir
                .join(format!("picker-selected-{touch}.png"))
                .to_str()
                .unwrap(),
        );
        d.click_name("confirm-tools");
        let added = state(&d.w).workspace.layout.header;
        assert_eq!(
            added.zones[2][0].item,
            HeaderItem::Tool {
                control: ToolbarControl::Panel { panel: Panel::Sizes }
            }
        );
        assert_eq!(
            added.zones[2][1].item,
            HeaderItem::Tool {
                control: ToolbarControl::Color
            }
        );
        assert_eq!(added.zones[2][2].id, before);
        assert!(state(&d.w).customization.header_editing);
        d.drop_component("clock", HeaderZone::Center);
        let clock = state(&d.w)
            .workspace
            .layout
            .header
            .entries()
            .find(|e| e.item == HeaderItem::Clock)
            .unwrap()
            .id;
        assert_eq!(
            state(&d.w)
                .workspace
                .layout
                .header
                .location(clock)
                .unwrap()
                .0,
            HeaderZone::Center
        );
        assert!(!d.named("header-add-clock").is_mapped());
        d.click_name(&format!("header-item-{clock}"));
        d.input.key(0xff08); // Backspace removes the clicked item.
        assert!(d.named("header-add-clock").is_mapped());
        // Focus/Enter does not choose an insertion slot; the actual drop does.
        let target = d.named(&format!("header-item-{before}"));
        target.grab_focus();
        d.input.key(0xff0d);
        d.drop_component_before("space", before, touch);
        let current = state(&d.w).workspace.layout.header;
        let (_, index) = current.location(before).unwrap();
        assert_eq!(current.zones[2][index - 1].item, HeaderItem::Space);
        crate::capture(
            &d.w,
            d.input
                .dir
                .join(format!("editor-insertion-{touch}.png"))
                .to_str()
                .unwrap(),
        );
        d.click_name("header-edit-cancel");
        assert_eq!(state(&d.w).workspace.layout.header, original);
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_spacing_visual"]
fn native_header_spacing_visual() {
    let mut d = Driver::managed("art.capycanvas.HeaderSpacing");
    let scale = d.w.window.surface().unwrap().scale();
    let expected = std::env::var("LAYER_MOTION_SCALE").ok().map(|s| s.parse::<f64>().unwrap()).unwrap_or(1.);
    assert!((scale - expected).abs() < 0.01, "GTK surface scale {scale}, expected {expected}");
    eprintln!("GTK surface scale {scale}; logical viewport {}x{}", d.w.surface.width(), d.w.surface.height());
    d.click_name("workspace-switch-painter");
    wait_workspaces(&d.w);
    pump(250);
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for size in HeaderSize::ALL {
            d.w.dispatch(HeaderAction::SetSize { size }.action());
            pump(250);
            let gap = match size { HeaderSize::Small => 6., HeaderSize::Medium => 8., HeaderSize::Large => 10. };
            assert_eq!(d.w.header.height(), size.tile() + 2. * gap);
            assert_shared_icons(d.w.header.root.upcast_ref());
            let close = find_css(d.w.header.root.upcast_ref(), "close").unwrap();
            let b = close.compute_bounds(&d.w.surface).unwrap();
            assert!(
                (b.width() - b.height()).abs() < 1.,
                "{size:?}: close not square: {b:?}"
            );
            assert!(
                (b.y() - gap).abs() < 1.
                    && (d.w.header.height() - b.y() - b.height() - gap).abs() < 1.,
                "close vertical padding: {b:?}"
            );
            let image = close.first_child().unwrap().compute_bounds(&close).unwrap();
            assert!((image.x() - (close.width() as f32 - image.x() - image.width())).abs() < 1.);
            assert!((image.y() - (close.height() as f32 - image.y() - image.height())).abs() < 1.);
            let h = state(&d.w).workspace.layout.header;
            let last = h.zones[2].last().unwrap();
            let item = d.named(&format!("header-item-{}", last.id)).compute_bounds(&d.w.surface).unwrap();
            assert_eq!(b.x() - item.x() - item.width(), gap, "last tile clears native close by one gap");
            assert_eq!(d.w.surface.width() as f32 - b.x() - b.width(), gap, "native close outer clearance");
            for entry in h.entries() {
                let item = if entry.item == HeaderItem::Capy { d.w.zen_capy.clone().upcast() }
                    else { d.named(&format!("header-item-{}", entry.id)) };
                assert!(item.is_mapped(), "header item {} must be mapped", entry.id);
                let b = item.compute_bounds(&d.w.surface).unwrap();
                let picked =
                    d.w.surface
                        .pick(
                            (b.x() + b.width() / 2.).into(),
                            (b.y() + b.height() / 2.).into(),
                            gtk::PickFlags::DEFAULT,
                        )
                        .expect("hit target");
                assert!(
                    picked == item || picked.is_ancestor(&item),
                    "header item {} is obscured by {}",
                    entry.id,
                    picked.widget_name()
                );
            }
            for pair in h.zones[2].windows(2) {
                let a = d
                    .named(&format!("header-item-{}", pair[0].id))
                    .compute_bounds(&d.w.surface)
                    .unwrap();
                let b = d
                    .named(&format!("header-item-{}", pair[1].id))
                    .compute_bounds(&d.w.surface)
                    .unwrap();
                let gap = if pair[0].item.joins_bar() && pair[1].item.joins_bar() {
                    size.gap()
                } else {
                    size.item_gap()
                };
                assert!(
                    (b.x() - a.x() - a.width() - gap).abs() < 1.,
                    "tile gap {a:?} {b:?}"
                );
            }
            let switcher =
                d.w.workspaces
                    .switcher
                    .compute_bounds(&d.w.surface)
                    .unwrap();
            assert!(
                (switcher.x() + switcher.width() / 2. - d.w.surface.width() as f32 / 2.).abs() < 1.
            );
            assert!(switcher.height() <= 36., "selector stretched vertically");
            crate::capture(
                &d.w,
                d.input
                    .dir
                    .join(format!("bar-{theme:?}-{size:?}.png"))
                    .to_str()
                    .unwrap(),
            );
            let tool = d.named(&d.header_tool(ToolbarControl::Color));
            d.click(&tool);
            let button = find_css(&tool, "header-tool").unwrap();
            assert!(button.has_css_class("drawer-origin-bottom"));
            assert!(button.has_css_class("drawer-open"));
            assert!(!button.has_css_class("selected-tool"));
            crate::capture(
                &d.w,
                d.input
                    .dir
                    .join(format!("drawer-{theme:?}-{size:?}.png"))
                    .to_str()
                    .unwrap(),
            );
            d.click(&tool);
            wait_for_drawer_close(&d.w);
            assert!(!button.has_css_class("drawer-origin-bottom"));
            d.edit();
            assert_shared_icons(d.w.header.root.upcast_ref());
            let capy = h.entries().find(|e| e.item == HeaderItem::Capy).unwrap().id;
            let item = d.named(&format!("header-item-{capy}"));
            let grip = d
                .named(&format!("header-grip-{capy}"))
                .compute_bounds(&item)
                .unwrap();
            assert!(
                (grip.y() + grip.height() / 2. - item.height() as f32 / 2.).abs() < 1.,
                "grip alignment"
            );
            d.drop_component("space", HeaderZone::Left);
            let model = state(&d.w).workspace.layout.header;
            let space = model
                .entries()
                .find(|e| e.item == HeaderItem::Space)
                .unwrap()
                .id;
            let space = d.named(&format!("header-item-{space}"));
            assert_eq!(
                space.width(),
                size.tile() as i32 + 20,
                "Space is one tile plus its editing grip"
            );
            crate::capture(
                &d.w,
                d.input
                    .dir
                    .join(format!("editor-{theme:?}-{size:?}.png"))
                    .to_str()
                    .unwrap(),
            );
            d.click_name("header-edit-cancel");
        }
    }
    d.click_name("workspace-switch-illustrator");
    wait_workspaces(&d.w);
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for size in HeaderSize::ALL {
            d.w.dispatch(HeaderAction::SetSize { size }.action());
            pump(200);
            let h = state(&d.w).workspace.layout.header;
            let settings = h.entries().find(|e| e.item == HeaderItem::Settings).unwrap();
            let settings = d.named(&format!("header-item-{}", settings.id)).compute_bounds(&d.w.surface).unwrap();
            let close = find_css(d.w.header.root.upcast_ref(), "close").unwrap().compute_bounds(&d.w.surface).unwrap();
            assert_eq!(close.x() - settings.x() - settings.width(), size.item_gap(), "Settings clears native close by one gap");
            crate::capture(&d.w, d.input.dir.join(format!("settings-{theme:?}-{size:?}.png")).to_str().unwrap());
        }
    }
    for size in HeaderSize::ALL {
        d.w.dispatch(HeaderAction::SetSize { size }.action());
        pump(200);
        let label = d.label("File");
        let mut button = label.clone();
        while !button.is::<gtk::Button>() {
            button = button.parent().unwrap();
        }
        let button_bounds = button.compute_bounds(&d.w.surface).unwrap();
        let mut bar = button.clone();
        while !bar.has_css_class("header-menu-labels") {
            bar = bar.parent().unwrap();
        }
        let bar_bounds = bar.compute_bounds(&d.w.surface).unwrap();
        assert_eq!(
            (bar_bounds.height(), button_bounds.height()),
            (36., 26.),
            "menu labels share the workspace switcher track at {size:?}"
        );
        let label_bounds = label.compute_bounds(&d.w.surface).unwrap();
        assert!(
            label_bounds.x() - button_bounds.x() >= 8.
                && button_bounds.x() + button_bounds.width()
                    - label_bounds.x()
                    - label_bounds.width()
                    >= 8.
        );
        let p = d.point(&label);
        d.input.perform(serde_json::json!([{"point":p}]));
        crate::capture(
            &d.w,
            d.input
                .dir
                .join(format!("menu-hover-{size:?}.png"))
                .to_str()
                .unwrap(),
        );
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_drawer_dismissal_input"]
fn native_drawer_dismissal_input() {
    fn contact(d: &mut Driver, point: [f32; 2], touch: bool) {
        d.input.perform(if touch {
            serde_json::json!([{"touch":"down","point":point},{"touch":"up"}])
        } else {
            serde_json::json!([{"point":point},{"down":true},{"down":false}])
        });
    }
    fn tap(d: &mut Driver, widget: &gtk::Widget, touch: bool) {
        contact(d, d.point(widget), touch);
    }
    let mut d = Driver::new("art.capycanvas.DrawerDismissal");
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for touch in [false, true] {
            d.w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(WorkspaceState {
                    layout: WorkspacePreset::Painter.layout(Platform::Gtk),
                    ..WorkspaceState::default()
                }),
            });
            pump(250);
            let color = d.named(&d.header_tool(ToolbarControl::Color));
            let color_button = find_css(&color, "header-tool").unwrap();
            let brush = d.named(&d.header_tool(ToolbarControl::Command {
                command: CommandId::DrawingBrush,
            }));
            let menu = d.named("header-item-2");
            for outside in ["gap", "menu", "disabled", "canvas"] {
                tap(&mut d, &color, touch);
                assert!(state(&d.w).customization.drawer.is_some());
                assert!(!color_button.has_css_class("selected-tool"));
                assert!(color_button.has_css_class("drawer-open"));
                match outside {
                    "gap" => contact(&mut d, [420., 25.], touch),
                    "menu" => tap(&mut d, &menu, touch),
                    "disabled" => {
                        let transform = d.named(&d.header_tool(ToolbarControl::Command {
                            command: CommandId::ScaleRotate,
                        }));
                        tap(&mut d, &transform, touch);
                    }
                    _ => contact(&mut d, [800., 700.], touch),
                }
                assert!(
                    state(&d.w).customization.drawer.is_none(),
                    "{theme:?}/{touch}/{outside}"
                );
                wait_for_drawer_close(&d.w);
                assert!(!color_button.has_css_class("drawer-open"));
                if outside == "menu" {
                    assert!(
                        d.w.popovers
                            .borrow()
                            .iter()
                            .filter_map(|p| p.upgrade())
                            .any(|p| p.is_visible()),
                        "The same click must open the menu, not just dismiss the drawer"
                    );
                    d.input.key(0xff1b);
                }
                assert!(
                    !state(&d.w)
                        .commands
                        .iter()
                        .find(|c| c.id == CommandId::Undo)
                        .unwrap()
                        .enabled,
                    "Dismissing on the canvas must not leave a mark"
                );
            }
            tap(&mut d, &color, touch);
            tap(&mut d, &brush, touch);
            assert!(
                find_css(&brush, "header-tool")
                    .unwrap()
                    .has_css_class("selected-tool")
            );
            assert!(
                state(&d.w).customization.drawer.is_some(),
                "Switch drawer and select in one click"
            );
            tap(&mut d, &brush, touch);
            assert!(
                state(&d.w).customization.drawer.is_none(),
                "Current opener toggles closed"
            );
            // Docked and floating toolbar bodies also dismiss without consuming
            // their normal grip/title-bar click or a menu's activation.
            for floating in [false, true] {
                let mut layout = DockLayout::default();
                layout
                    .move_panel(
                        [1600., 1000.],
                        Panel::Toolbar,
                        if floating {
                            DockTarget::Float {
                                position: [420., 300.],
                            }
                        } else {
                            DockTarget::Edge {
                                edge: Edge::Top,
                                outer: true,
                            }
                        },
                    )
                    .unwrap();
                let color_id = layout
                    .panel(Panel::Toolbar)
                    .unwrap()
                    .tiles()
                    .iter()
                    .find(|t| t.control == ToolbarControl::Color)
                    .unwrap()
                    .id;
                d.w.dispatch(UiAction::RestoreWorkspace {
                    workspace: Box::new(WorkspaceState {
                        layout,
                        ..WorkspaceState::default()
                    }),
                });
                pump(250);
                let color =
                    d.w.customization
                        .drawer_button(TileAnchor {
                            panel: Panel::Toolbar,
                            tile: color_id,
                        })
                        .unwrap();
                tap(&mut d, color.upcast_ref(), touch);
                assert!(state(&d.w).customization.drawer.is_some());
                assert!(
                    !color.has_css_class("selected-tool") && color.has_css_class("drawer-open")
                );
                let group =
                    d.w.resolved()
                        .groups
                        .into_iter()
                        .find(|g| g.panels.contains(&Panel::Toolbar))
                        .unwrap();
                let handle = {
                    let groups = d.w.groups.borrow();
                    let view = groups.iter().find(|g| g.id == group.id).unwrap();
                    find_css(view.root.upcast_ref(), "panel-grip").unwrap()
                };
                let grip = handle.compute_bounds(&d.w.window).unwrap();
                let exposed = [grip.x() + 6., grip.y() + grip.height() / 2.];
                assert!(
                    !d.w.drawer.placement().unwrap().bounds.contains(exposed[0], exposed[1]),
                    "toolbar handle {floating}/{touch}: the drawer covers the tapped grip"
                );
                contact(&mut d, exposed, touch);
                assert!(
                    state(&d.w).customization.drawer.is_none(),
                    "toolbar handle {floating}/{touch}"
                );
                tap(&mut d, color.upcast_ref(), touch);
                contact(&mut d, [500., 18.], touch);
                assert!(
                    state(&d.w).customization.drawer.is_none(),
                    "toolbar to window bar {floating}/{touch}"
                );
            }
        }
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_cancel_caption_input"]
fn native_header_cancel_caption_input() {
    let mut d = Driver::new("art.capycanvas.HeaderCancellation");
    let original = state(&d.w).workspace.layout.header;
    // Native caption double-click remains owned by the window manager.
    d.input.perform(serde_json::json!([{ "point": [400., 20.] }, { "down": true }, { "down": false }, { "down": true }, { "down": false }]));
    assert!(!d.w.window.is_maximized());
    d.w.window.maximize();
    pump(400);
    d.edit();
    let id = original.zones[0][0].id;
    d.input.key(0xff09);
    assert!(
        state(&d.w).customization.header_editing && !state(&d.w).workspace.zen_mode,
        "Tab navigates the editor instead of hiding it"
    );
    for blur in [false, true] {
        let start = d.point(&d.named(&format!("header-grip-{id}")));
        d.input.perform(
            serde_json::json!([{ "point": start }, { "down": true }, { "point": [800.,start[1]] }]),
        );
        assert!(d.w.dragging.get());
        if blur {
            d.w.interact(UiInput::Blur);
        } else {
            d.input.key(0xff1b);
        }
        d.input.perform(serde_json::json!([{ "down": false }]));
        assert!(!d.w.dragging.get() && d.w.workspace_drag.borrow().is_none());
        assert_eq!(state(&d.w).workspace.layout.header, original);
        assert!(state(&d.w).customization.header_editing);
    }
    let start = d.point(&d.named(&format!("header-grip-{id}")));
    d.input.perform(
        serde_json::json!([{ "point": start }, { "down": true }, { "point": [800.,start[1]] }]),
    );
    d.w.dispatch(HeaderAction::Remove { id }.action());
    pump(120);
    assert!(
        !d.w.dragging.get() && d.w.workspace_drag.borrow().is_none(),
        "source replacement retires the contact"
    );
    d.input.perform(serde_json::json!([{ "down": false }]));
    d.w.dispatch(HeaderAction::Cancel.action());
    d.w.dispatch(HeaderAction::Edit { editing: true }.action());
    pump(120);
    assert_eq!(state(&d.w).workspace.layout.header, original);
    d.click_name("header-edit-done");
    let menu = original
        .entries()
        .find(|e| e.item == HeaderItem::Menu)
        .unwrap()
        .id;
    d.click_name(&format!("header-item-{menu}"));
    d.click_label("Window");
    d.click_label("Customize Title Bar…");
    d.drop_component("menu-labels", HeaderZone::Left);
    d.click_name("header-edit-done");
    assert!(
        state(&d.w)
            .workspace
            .layout
            .header
            .entries()
            .any(|e| e.item == HeaderItem::MenuLabels)
    );
    // Structural publication must not be dropped when settings change in the
    // same frame (native refreshes used to overwrite a queued layout update).
    d.w.dispatch(
        HeaderAction::SetSize {
            size: HeaderSize::Large,
        }
        .action(),
    );
    d.w.dispatch(UiAction::SetTheme {
        theme: Some(Theme::Light),
    });
    pump(250);
    assert_eq!(d.w.header.root.height(), HeaderSize::Large.height() as i32);
    assert_eq!(
        state(&d.w).workspace.layout.header_presentation.height,
        HeaderSize::Large.height()
    );
    d.w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(WorkspaceState {
            layout: WorkspacePreset::Painter.layout(Platform::Gtk),
            ..WorkspaceState::default()
        }),
    });
    pump(250);
    let capy = d.w.zen_capy.clone().upcast::<gtk::Widget>();
    d.click(&capy);
    d.input
        .perform(serde_json::json!([{ "point": [600.,400.] }]));
    assert!(state(&d.w).workspace.zen_mode && !d.w.header.root.can_target());
    d.input.perform(serde_json::json!([{ "point": [6.,6.] }]));
    assert!(
        !d.w.header.root.can_target(),
        "Edge contacts keep chrome hidden"
    );
    d.click(&capy);
    assert!(!state(&d.w).workspace.zen_mode);
    // Close is native and cannot be removed by customization.
    let close = find_css(d.w.header.root.upcast_ref(), "close").unwrap();
    d.click(&close);
    assert!(!d.w.window.is_visible());
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_brush_drawer_input"]
fn native_brush_drawer_input() {
    let mut d = Driver::new("art.capycanvas.BrushDrawer");
    let brush_icon = d.header_icon(CommandId::DrawingBrush, "pen");
    let brush = ToolbarControl::Command { command: CommandId::DrawingBrush };
    let opener = d.header_tool(brush);
    d.click_name(&opener);
    let drawer = d.named("tool-drawer");
    let sets = find_named(&drawer, "drawer-panel-BrushSets").unwrap();
    let tools = find_named(&drawer, "drawer-panel-Tools").unwrap();
    let settings = find_named(&drawer, "drawer-panel-ToolSettings").unwrap();
    assert!(sets.width() < tools.width());
    assert!(tools.width() < settings.width());
    assert!(find_css(&tools, "tool-groups").is_some_and(|w| !w.is_visible()));
    for (touch, icon, group) in [
        (false, "pencil", layer_ui::ToolGroup::Pencil),
        (true, "pastel", layer_ui::ToolGroup::Pastel),
        (false, "paint", layer_ui::ToolGroup::Paint),
    ] {
        let button = find_named(&sets, &format!("brush-set-{icon}")).unwrap();
        assert!(button.height() >= 44, "touchable brush set row");
        let p = d.point(&button);
        if touch {
            d.input.perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));
        } else { d.click(&button); }
        assert!(state(&d.w).customization.drawer.is_some());
        assert_eq!(state(&d.w).brush.tool, group.tool());
        assert_eq!(d.header_icon(CommandId::DrawingBrush, icon), brush_icon, "retain the header image while changing tools");
        assert!(state(&d.w).tool_panels.brush_sets.groups.iter().any(|g| g.selected && g.label == group.label()));
        assert_eq!(d.named("drawer-panel-BrushSets"), sets, "set list remains retained");
        let choices = state(&d.w).tool_set.subtools;
        let last = choices.last().unwrap();
        d.click_name(&format!("brush-{}", last.preview.unwrap()));
        assert_eq!(state(&d.w).brush.preset, last.preview.unwrap());
        assert!(state(&d.w).customization.drawer.is_some());
    }
    d.number(&find_named(&drawer, "tool-setting-size").unwrap(), "37");
    let remembered = state(&d.w).brush.preset;
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS")
        .unwrap_or_else(|_| d.input.dir.to_string_lossy().into()));
    std::fs::create_dir_all(&output).unwrap();
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        let _warm = crate::snapshot(&d.w);
        pump(120);
        crate::snapshot(&d.w).save_to_png(output.join(format!("brush-drawer-{theme:?}.png"))).unwrap();
    }
    assert_eq!(state(&d.w).tool_panels.brush_sets.groups.len(), 10);
    let sculpt = d.header_tool(ToolbarControl::Command { command: CommandId::Sculpt });
    d.click_name(&sculpt);
    let sculpt_sets = d.named("drawer-panel-SculptSets");
    assert_eq!(
        state(&d.w).tool_panels.sculpt_sets.groups.iter().map(|s| s.label.as_ref()).collect::<Vec<_>>(),
        ["Blend", "Liquify", "Clone", "Heal", "Spot Heal"]
    );
    let liquify = find_named(&sculpt_sets, "sculpt-set-liquify").unwrap();
    let p = d.point(&liquify);
    d.input
        .perform(serde_json::json!([{"touch":"down","point":p},{"touch":"up"}]));
    assert_eq!(state(&d.w).brush.tool, layer_ui::Tool::Liquify);
    d.header_icon(CommandId::Sculpt, "liquify");
    assert_eq!(d.header_icon(CommandId::DrawingBrush, "paint"), brush_icon);
    assert!(state(&d.w).customization.drawer.is_some());
    d.number(&find_named(&d.named("tool-drawer"), "tool-setting-size").unwrap(), "79");
    let sculpt_preset = state(&d.w).brush.preset;
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        crate::snapshot(&d.w).save_to_png(output.join(format!("sculpt-drawer-{theme:?}.png"))).unwrap();
    }
    d.click_name(&opener);
    assert_eq!((state(&d.w).brush.preset, state(&d.w).brush.diameter), (remembered, 37.));
    d.header_icon(CommandId::Sculpt, "liquify");
    d.click_name(&sculpt);
    assert_eq!((state(&d.w).brush.preset, state(&d.w).brush.diameter), (sculpt_preset, 79.));
    d.click_name(&opener);
    d.click_name(&opener);
    assert!(state(&d.w).customization.drawer.is_none());
    d.w.dispatch(UiAction::Invoke { command: CommandId::Lasso });
    d.click_name(&opener);
    assert!(state(&d.w).customization.drawer.is_none(), "first click restores Brush");
    assert_eq!(state(&d.w).brush.preset, remembered);
    assert_eq!(state(&d.w).brush.diameter, 37.);
    d.click_name(&opener);
    assert!(state(&d.w).customization.drawer.is_some());
    assert_eq!(ui_session(&d.w).command(CommandId::Brush).label.as_ref(), "Paint Brush");
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_filter_drawer_input"]
fn native_filter_drawer_input() {
    let mut d = Driver::new("art.capycanvas.FilterDrawer");
    let opener = d.header_tool(ToolbarControl::Panel { panel: Panel::Adjustments });
    d.click_name(&opener);
    let types = d.named("drawer-panel-FilterTypes");
    let choices = d.named("drawer-panel-Adjustments");
    assert!(types.width() < choices.width());
    assert!(find_css(&choices, "filter-picker-header").is_some_and(|w| !w.is_visible()));
    d.click_name("adjustment-brightness_contrast");
    let id = state(&d.w).layer_properties.layer.unwrap();
    assert!(state(&d.w).layers.iter().any(|l| l.id == 1 && l.drawing));
    d.number(&d.named("property-brightness"), "20");
    assert!(state(&d.w).customization.drawer.is_some());
    d.click_name(&opener);
    d.click_name(&opener);
    assert_eq!(state(&d.w).layer_properties.layer, Some(id));
    d.click_name("adjustment-curves");
    assert_eq!(state(&d.w).layer_properties.layer, Some(id));
    assert_eq!(state(&d.w).layers.len(), 3);
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        let _warm = crate::snapshot(&d.w); pump(120);
        crate::snapshot(&d.w).save_to_png(output.join(format!("filters-{theme:?}.png"))).unwrap();
    }
    d.click_name("cancel-filter");
    assert_eq!(state(&d.w).layers.len(), 2);
    assert!(state(&d.w).customization.drawer.is_none());
    d.click_name(&opener);
    d.w.dispatch(UiAction::SelectLayer { id: 2 });
    d.w.dispatch(UiAction::SetColor { rgba: [0., 1., 0., 1.] });
    pump(100);
    d.click_name("color-bucket");
    assert!(state(&d.w).layer_properties.controls[0].color_action.is_some());
    let title = d.w.effects.properties.first_child().unwrap().downcast::<gtk::Label>().unwrap();
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for name in ["Paper", "Solid Color", "Paper"] {
            d.w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Rename { id: 2, name: name.into() } });
            pump(120);
            assert_eq!(title.text().as_str(), name);
            assert_eq!(named::<gtk::Label>(d.w.effects.properties.upcast_ref(), "properties-layer-type").text(), "Solid Color");
        }
        let _warm = crate::snapshot(&d.w); pump(120);
        crate::snapshot(&d.w).save_to_png(output.join(format!("paper-properties-{theme:?}.png"))).unwrap();
    }
    // Native hover exercises the GPU's prohibited cursor path.
    d.click_name(&opener);
    d.input.perform(serde_json::json!([{"point":[600.,400.]}]));
    assert_eq!(ui_session_mut(&d.w).canvas_cursor().unwrap().segments[0].marker, 6.);
    let layers = d.header_tool(ToolbarControl::Panel { panel: Panel::Layers });
    d.click_name(&layers);
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        super::place_source::wait_layer_thumbnail(&d.w, 2);
        pump(250);
        let _warm = crate::snapshot(&d.w); pump(120);
        let shot = crate::snapshot(&d.w);
        let row = d.named("art-layer-2");
        let symbol = find_css(&row, "layer-type-symbol").unwrap().downcast::<gtk::Image>().unwrap();
        assert!(symbol.is_visible());
        assert_eq!(crate::icons::name(&symbol).as_deref(), Some("layer-fill-symbolic"));
        assert!(find_css(&d.named("art-layer-1"), "layer-type-symbol").is_none_or(|symbol| !symbol.is_visible()));
        let thumb = find_css(&row, "layer-thumbnail").unwrap();
        let p = thumb.compute_bounds(&d.w.surface).unwrap();
        let mut download = gdk::TextureDownloader::new(&shot);
        download.set_format(gdk::MemoryFormat::R8g8b8a8);
        let (bytes, stride) = download.download_bytes();
        let scale = shot.width() as f32 / d.w.surface.width() as f32;
        let x = ((p.x() + p.width() * 0.5) * scale) as usize;
        let y = ((p.y() + p.height() * 0.35) * scale) as usize;
        let pixel = &bytes[y * stride + x * 4..][..4];
        assert!(pixel[1] > 240 && pixel[0] < 10 && pixel[2] < 10, "{theme:?}: fill thumbnail {pixel:?}");
        shot.save_to_png(output.join(format!("fill-thumbnail-{theme:?}.png"))).unwrap();
    }
    d.click_name(&opener);
    d.click_name("filter-type-fill");
    d.click_name("adjustment-gradient_fill");
    d.number(&d.named("property-angle"), "0");
    d.click_name(&layers);
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        let mut original = None;
        for (step, reverse) in [false, true, false].into_iter().enumerate() {
            if reverse {
                let target = state(&d.w).layer_properties.controls.iter().find_map(|control| control.gradient.as_ref().map(|gradient| gradient.destination.clone())).unwrap();
                d.w.dispatch(UiAction::Effect { action: layer_ui::EffectAction::Gradient { target, edit: layer_ui::GradientEdit::Reverse } });
            } else if step == 2 {
                d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
            }
            super::place_source::wait_layer_thumbnail(&d.w, 2);
            pump(350);
            let shot = crate::snapshot(&d.w);
            let row = d.named("art-layer-2");
            let symbol = find_css(&row, "layer-type-symbol").unwrap().downcast::<gtk::Image>().unwrap();
            assert!(symbol.is_visible());
            assert_eq!(crate::icons::name(&symbol).as_deref(), Some("layer-gradient-symbolic"));
            let thumb = find_css(&row, "layer-thumbnail").unwrap();
            let p = thumb.compute_bounds(&d.w.surface).unwrap();
            let mut download = gdk::TextureDownloader::new(&shot);
            download.set_format(gdk::MemoryFormat::R8g8b8a8);
            let (bytes, stride) = download.download_bytes();
            let scale = shot.width() as f32 / d.w.surface.width() as f32;
            let sample = |fraction| {
                let x = ((p.x() + p.width() * fraction) * scale) as usize;
                let y = ((p.y() + p.height() * 0.35) * scale) as usize;
                bytes[y * stride + x * 4]
            };
            let (left, right) = (sample(0.2), sample(0.8));
            let contrast = i16::from(right) - i16::from(left);
            assert!(if reverse { contrast < -100 } else { contrast > 100 }, "{theme:?} reverse={reverse}: {left}, {right}");
            if step == 0 { original = Some((left, right)); }
            if step == 2 { assert_eq!(Some((left, right)), original, "Undo restores the gradient thumbnail"); }
            shot.save_to_png(output.join(format!("gradient-thumbnail-{theme:?}-{reverse}.png"))).unwrap();
        }
    }
    d.click_name(&layers);
    d.w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::New { group: false, clipped: false } });
    let removable = state(&d.w).layer_properties.layer.unwrap();
    let layers = d.header_tool(ToolbarControl::Panel { panel: Panel::Layers });
    d.click_name(&layers);
    let row = d.named(&format!("art-layer-{removable}"));
    let swipe = row.parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
    let before = row.compute_bounds(&d.w.window).unwrap().x();
    let p = d.point(&row);
    d.input.perform(serde_json::json!([{"touch":"down","point":p},
        {"touch":"move","point":[p[0]-18.,p[1]]}, {"touch":"move","point":[p[0]-48.,p[1]]}]));
    assert!(swipe.is_open());
    let moved = before - row.compute_bounds(&d.w.window).unwrap().x();
    assert!((moved - 48.).abs() < 2., "row follows the finger: {moved}");
    d.input.perform(serde_json::json!([{"touch":"up"}]));
    let _warm = crate::snapshot(&d.w); pump(120);
    crate::snapshot(&d.w).save_to_png(output.join("layer-delete.png")).unwrap();
    let p = d.point(&row);
    d.input.perform(serde_json::json!([{"touch":"down","point":p},
        {"touch":"move","point":[p[0]+18.,p[1]]}, {"touch":"move","point":[p[0]+65.,p[1]]}, {"touch":"up"}]));
    assert!(!swipe.is_open(), "reverse swipe closes Delete");
    for delete in [false, true] {
        let row = d.named(&format!("art-layer-{removable}"));
        let swipe = row.parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
        let p = d.point(&row);
        d.input.perform(serde_json::json!([{"touch":"down","point":p},
            {"touch":"move","point":[p[0]-18.,p[1]]}, {"touch":"move","point":[p[0]-65.,p[1]]}, {"touch":"up"}]));
        assert!(swipe.is_open());
        if delete { d.click(swipe.delete_button().upcast_ref()); }
        else {
            d.input.click([600., 400.]);
            assert!(!swipe.is_open(), "outside click closes Delete");
            if state(&d.w).customization.drawer.is_none() { d.click_name(&layers); }
        }
    }
    assert!(!state(&d.w).layers.iter().any(|l| l.id == removable));
    d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
    assert!(state(&d.w).layers.iter().any(|l| l.id == removable));
    d.w.window.close();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_layer_preview_selection"]
fn native_layer_preview_selection() {
    let mut d = Driver::new("art.capycanvas.LayerPreviewSelection");
    d.w.dispatch(UiAction::SetColor { rgba: [0., 1., 0., 1.] });
    d.w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
    pump(200);
    d.w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::FillSelection });
    pump(300);
    d.w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
    let opener = d.header_tool(ToolbarControl::Panel { panel: Panel::Layers });
    d.click_name(&opener);
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").unwrap());
    super::place_source::wait_layer_thumbnail(&d.w, 1);
    for id in [2, 1, 2, 1] {
        let row = d.named(&format!("art-layer-{id}"));
        d.click(&find_css(&row, "layer-thumbnail").unwrap());
        if state(&d.w).layers.iter().find(|row| row.id == id).is_some_and(|row| row.fill_color.is_some()) {
            crate::color_editor::tests::editor(&d.w);
            super::new_photo::response(&d.w, "cancel");
            until(|| d.w.color_editors.borrow().is_empty(), "the Paper color closes");
        }
        super::place_source::wait_layer_thumbnail(&d.w, 1);
        let _warm = crate::snapshot(&d.w); pump(120);
        let shot = crate::snapshot(&d.w);
        let row = d.named("art-layer-1");
        let thumb = find_css(&row, "layer-thumbnail").unwrap();
        let p = thumb.compute_bounds(&d.w.surface).unwrap();
        let mut bytes = vec![0; shot.width() as usize * shot.height() as usize * 4];
        shot.download(&mut bytes, shot.width() as usize * 4);
        let x = (p.x()+p.width()*0.5) as usize;
        let y = (p.y()+p.height()*0.5) as usize;
        let pixel = &bytes[(y*shot.width() as usize+x)*4..][..4];
        shot.save_to_png(output.join(format!("thumbnail-selected-{id}.png"))).unwrap();
        assert!(pixel[1] > 180 && pixel[0] < 80 && pixel[2] < 80, "selected {id}, thumbnail center {pixel:?}");
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_space_pans_after_a_clicked_button"]
fn native_space_pans_after_a_clicked_button() {
    let mut d = Driver::new("art.capycanvas.SpaceFocus");
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(200);
        let root = d.named(&d.header_tool(ToolbarControl::Command { command: CommandId::Eraser }));
        let button = widgets(&root).find_map(|w| w.downcast::<gtk::Button>().ok()).unwrap();
        let clicks = Rc::new(Cell::new(0));
        let counter = button.connect_clicked(glib::clone!(
            #[strong]
            clicks,
            move |_| clicks.set(clicks.get() + 1)
        ));
        d.click(button.upcast_ref());
        assert!(button.has_focus(), "a clicked header button keeps the focus");
        assert_eq!(clicks.get(), 1);
        let before = state(&d.w).camera.translation;
        let start = screen_point(d.w.area.upcast_ref(), &d.w.window, [0.5, 0.5]);
        d.input.perform(serde_json::json!([{ "key": 0x20, "down": true }, { "point": start }, { "down": true },
            { "point": [start[0] + 30., start[1]] }, { "point": [start[0] + 60., start[1]] }, { "down": false }, { "key": 0x20, "down": false }]));
        pump(400);
        let dpi = d.w.area.scale_factor() as f32;
        let after = state(&d.w).camera.translation;
        assert_eq!(clicks.get(), 1, "Space after clicking a button pans instead of pressing it again");
        assert!((after[0] - before[0] - 60. * dpi).abs() <= dpi && (after[1] - before[1]).abs() <= dpi, "Space-drag pans: {before:?} -> {after:?}");
        d.input.key(0xff1b);
        gtk::prelude::GtkWindowExt::set_focus(&d.w.window, gtk::Widget::NONE);
        assert!(button.grab_focus());
        d.input.key(0x20);
        until(|| clicks.get() == 2, "Space presses a button reached with the keyboard");
        button.disconnect(counter);
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_panel_pen_input --tablet"]
fn native_panel_pen_input() {
    let mut d = Driver::new("art.capycanvas.PanelPen");
    let opener = d.header_tool(ToolbarControl::Command { command: CommandId::DrawingBrush });
    d.click_name(&opener);
    let sets = state(&d.w).tool_panels.brush_sets.groups;
    let mut longest = (0, sets[0].action.clone());
    for set in sets {
        d.w.dispatch(set.action.clone());
        let count = state(&d.w).tool_set.subtools.len();
        if count > longest.0 { longest = (count, set.action); }
    }
    d.w.dispatch(longest.1);
    pump(200);
    let tools = d.named("drawer-panel-Tools");
    let scroll = tools.ancestor(gtk::ScrolledWindow::static_type()).unwrap().downcast::<gtk::ScrolledWindow>().unwrap();
    // Constrain this native viewport to make every device exercise overflow.
    scroll.set_max_content_height(180);
    scroll.set_propagate_natural_height(false);
    scroll.set_height_request(180);
    scroll.set_valign(gtk::Align::Start);
    pump(200);
    let overflow = scroll.vadjustment().upper() - scroll.vadjustment().page_size();
    assert!(overflow > 20., "fixture has scrollable tools");
    let bounds = scroll.compute_bounds(&d.w.window).unwrap();
    let p = [bounds.x() + bounds.width() * 0.5, bounds.y() + bounds.height().min(160.) - 15.];
    let input_tools = Rc::new(Cell::new(0));
    let events = gtk::EventControllerLegacy::new();
    events.set_propagation_phase(gtk::PropagationPhase::Capture);
    events.connect_event(glib::clone!(#[strong] input_tools, move |_, event| {
        if event.device_tool().is_some() { input_tools.set(input_tools.get() + 1); }
        glib::Propagation::Proceed
    }));
    tools.add_controller(events);
    for device in ["mouse", "touch", "pen"] {
        scroll.vadjustment().set_value(0.);
        pump(80);
        let mut input = Vec::new();
        if device == "mouse" { input.extend([serde_json::json!({"point":p}), serde_json::json!({"down":true})]); }
        else { input.push(serde_json::json!({device:"down","point":p})); }
        for delta in [18., 40., 65., 95.] {
            let q = [p[0],p[1]-delta];
            input.push(if device == "mouse" { serde_json::json!({"point":q}) } else { serde_json::json!({device:"move","point":q}) });
        }
        input.push(if device == "mouse" { serde_json::json!({"down":false}) } else { serde_json::json!({device:"up"}) });
        d.input.perform(input.into());
        if device == "mouse" { assert_eq!(scroll.vadjustment().value(), 0., "mouse choices do not pan"); }
        else { assert!(scroll.vadjustment().value() > 40_f64.min(overflow * 0.8), "{device} scrolls choices: {}", scroll.vadjustment().value()); }
    }
    assert!(input_tools.get() > 0, "Wayland tablet-v2 produced real GDK tablet events");
    d.input.perform(serde_json::json!([{"pen":"leave"}]));
    d.w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::New { group: false, clipped: false } });
    let id = state(&d.w).layer_properties.layer.unwrap();
    let layers = d.header_tool(ToolbarControl::Panel { panel: Panel::Layers });
    d.click_name(&layers);
    let row = d.named(&format!("art-layer-{id}"));
    let swipe = row.parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
    let p = d.point(&row);
    d.input.perform(serde_json::json!([{"pen":"down","point":p}, {"pen":"move","point":[p[0]-20.,p[1]]},
        {"pen":"move","point":[p[0]-64.,p[1]]},{"pen":"up"}]));
    assert!(swipe.is_open(), "pen swipes expose Delete");
    let p = d.point(swipe.delete_button().upcast_ref());
    d.input.perform(serde_json::json!([{"pen":"down","point":p},{"pen":"up"},{"pen":"leave"}]));
    assert!(!state(&d.w).layers.iter().any(|l| l.id == id));
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_drawer_controls_input"]
fn native_header_drawer_controls_input() {
    let mut d = Driver::new("art.capycanvas.HeaderDrawers");
    let brush = ToolbarControl::Command {
        command: CommandId::DrawingBrush,
    };
    d.click_name(&d.header_tool(brush));
    if state(&d.w).customization.drawer.is_none() {
        d.click_name(&d.header_tool(brush));
    }
    let drawer = d.named("tool-drawer");
    d.number(&find_named(&drawer, "tool-setting-size").unwrap(), "37");
    assert_eq!(state(&d.w).brush.diameter, 37.);
    // The first click on another header tool switches the existing drawer.
    d.click_name(&d.header_tool(ToolbarControl::Command {
        command: CommandId::Sculpt,
    }));
    assert!(state(&d.w).customization.drawer.is_some());
    assert!(
        tool_state(
            &state(&d.w),
            ToolbarControl::Command {
                command: CommandId::Sculpt
            }
        )
        .1
    );
    d.click_name(&d.header_tool(ToolbarControl::Color));
    let before = state(&d.w).colors;
    let drawer = d.named("tool-drawer");
    d.click(&find_named(&drawer, "color-swap").unwrap());
    assert_eq!(state(&d.w).colors.foreground, before.background);
    d.click_name(&d.header_tool(ToolbarControl::Panel {
        panel: Panel::Layers,
    }));
    let layers = d.w.drawer.layers().unwrap();
    d.click(&layers.footer.last_child().unwrap());
    assert!(
        d.w.popovers
            .borrow()
            .iter()
            .filter_map(|p| p.upgrade())
            .any(|p| p.is_mapped() && p.parent().as_ref() == Some(layers.root.upcast_ref())),
        "Layers context menu uses its header drawer projection"
    );
    d.input.key(0xff1b);
    // Every content-panel family also works from an individual header item.
    let controls = Panel::ALL
        .into_iter()
        .filter(|p| p.kind() == PanelKind::Content)
        .map(|panel| ToolbarControl::Panel { panel })
        .chain([
            ToolbarControl::Brush {
                id: layer_core::DefaultBrushPreset::GPen as u32,
            },
            ToolbarControl::Size { tenths: 300 },
        ]);
    for control in controls {
        let mut workspace = WorkspaceState {
            layout: WorkspacePreset::Painter.layout(Platform::Gtk),
            ..WorkspaceState::default()
        };
        workspace.layout.header.zones = Default::default();
        workspace
            .layout
            .header
            .add(
                HeaderZone::Left,
                None,
                &[HeaderItem::Menu, HeaderItem::Tool { control }],
            )
            .unwrap();
        d.w.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(workspace),
        });
        pump(160);
        let name = d.header_tool(control);
        d.click_name(&name);
        if !matches!(control, ToolbarControl::Size { .. }) {
            if state(&d.w).customization.drawer.is_none() {
                d.click_name(&name);
            }
            assert!(d.named("tool-drawer").is_mapped(), "{control:?}");
            if control == (ToolbarControl::Panel { panel: Panel::Sizes }) {
                assert!(!d.named("drawer-control-brush-size").is_mapped());
                d.click_name("size-preset-1.5"); assert_eq!(state(&d.w).brush.diameter, 1.5);
            }
            let bounds = d.named("tool-drawer").compute_bounds(&d.w.surface).unwrap();
            assert!(bounds.y() >= d.w.header.height() && bounds.x() >= 0., "{control:?}: {bounds:?}, header {}px", d.w.header.height());
            d.click_name(&name);
            assert!(
                state(&d.w).customization.drawer.is_none(),
                "repeat click closes {control:?}"
            );
        } else if let ToolbarControl::Size { tenths } = control {
            assert_eq!(state(&d.w).brush.diameter, tenths as f32 / 10.);
        }
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_hold_context_input"]
fn native_header_hold_context_input() {
    let mut d = Driver::new("art.capycanvas.HeaderHolds");
    let original = state(&d.w).workspace;
    let id = original.layout.header.zones[0][1].id;
    d.edit();
    for touch in [false, true] {
        let device = if touch { "touch" } else { "mouse" };
        let point = d.point(&d.named(&format!("header-item-{id}")));
        let mut events = vec![contact(device, "down", point)];
        events.extend((0..12).map(|_| serde_json::json!({})));
        events.push(contact(device, "up", point));
        d.input.perform(serde_json::Value::Array(events));
        assert_eq!(
            d.w.popovers
                .borrow()
                .iter()
                .filter_map(|p| p.upgrade())
                .any(|p| p.is_visible()),
            touch,
            "only a touch/pen hold opens a menu; mouse holds only arm pickup"
        );
        assert_eq!(state(&d.w).workspace.layout.header, original.layout.header);
        if touch {
            d.input.key(0xff1b);
        }
    }
    for touch in [false, true] {
        let device = if touch { "touch" } else { "mouse" };
        for grip in [false, true] {
            for held in [false, true] {
                if grip && held {
                    continue;
                }
                d.w.dispatch(UiAction::RestoreWorkspace {
                    workspace: Box::new(original.clone()),
                });
                pump(160);
                d.w.dispatch(HeaderAction::Edit { editing: true }.action());
                pump(160);
                let from = d.point(&d.named(&format!(
                    "header-{}-{id}",
                    if grip { "grip" } else { "item" }
                )));
                let to = [d.w.surface.width() as f32 / 2., from[1]];
                let mut events = vec![contact(device, "down", from)];
                if held {
                    events.extend((0..12).map(|_| serde_json::json!({})));
                }
                events.extend([
                    contact(device, "move", [from[0] + 20., from[1]]),
                    contact(device, "move", to),
                    contact(device, "up", to),
                ]);
                d.input.perform(serde_json::Value::Array(events));
                assert_eq!(
                    state(&d.w).workspace.layout.header.location(id).unwrap().0,
                    HeaderZone::Center,
                    "touch={touch} grip={grip} held={held}"
                );
                d.w.dispatch(HeaderAction::Cancel.action());
                pump(160);
                assert_eq!(state(&d.w).workspace.layout.header, original.layout.header);
                assert!(
                    !d.w.popovers
                        .borrow()
                        .iter()
                        .filter_map(|p| p.upgrade())
                        .any(|p| p.is_visible()),
                    "drag must dismiss the held menu"
                );
            }
        }
    }
    d.w.dispatch(HeaderAction::Edit { editing: false }.action());
    pump(160);
    let p = d.point(&d.named(&format!("header-item-{id}")));
    d.input.perform(serde_json::json!([{ "point": p }, { "button": 273, "down": true }, { "button": 273, "down": false }]));
    d.click_label("Customize Title Bar…");
    let p = d.point(&d.named(&format!("header-item-{id}")));
    d.input.perform(serde_json::json!([{ "point": p }, { "button": 273, "down": true }, { "button": 273, "down": false }]));
    d.click_label("Move to Center");
    assert_eq!(
        state(&d.w).workspace.layout.header.location(id).unwrap().0,
        HeaderZone::Center
    );
    let item = d.named(&format!("header-item-{id}"));
    item.grab_focus();
    d.input.perform(serde_json::json!([{ "key": 0xffe1, "down": true }, { "key": 0xffc7, "down": true }, { "key": 0xffc7, "down": false }, { "key": 0xffe1, "down": false }]));
    d.click_label("Remove from Title Bar");
    assert!(state(&d.w).workspace.layout.header.entry(id).is_err());
    d.w.dispatch(HeaderAction::Cancel.action());
    pump(160);
    assert!(!state(&d.w).customization.header_editing);
    assert!(state(&d.w).workspace.layout.header.entry(id).is_ok());
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_editor_controls_input"]
fn native_header_editor_controls_input() {
    let mut d = Driver::new("art.capycanvas.HeaderControls");
    d.edit();
    for size in HeaderSize::ALL {
        d.click_name(&format!("header-size-{}", size as usize));
        pump(220);
        assert_eq!(state(&d.w).workspace.layout.header.size, size);
        let editor = d.named("header-editor");
        assert!(
            editor.height() <= 56 && editor.width() == d.w.surface.width() - 12,
            "The editor is one full-width strip at {size:?}: {}x{}",
            editor.width(),
            editor.height()
        );
        let palette = d.named("header-add-tools").compute_bounds(&editor).unwrap();
        let done = d.named("header-edit-done").compute_bounds(&editor).unwrap();
        assert!((palette.y() + palette.height() / 2. - done.y() - done.height() / 2.).abs() < 1.);
        assert!(done.x() + done.width() >= editor.width() as f32 - 13.);
        assert!(d.w.header.root.height() <= size.height() as i32 + 68);
        assert_eq!(d.w.area.height(), d.w.surface.height());
    }
    for name in ["clock", "menu-labels"] {
        d.drop_component(name, HeaderZone::Left);
    }
    d.drop_component("tools", HeaderZone::Left);
    d.named("tool-search")
        .downcast::<gtk::SearchEntry>()
        .unwrap()
        .set_text("Brush size");
    pump(250);
    d.click_name(&format!(
        "tool-choice-{}",
        serde_json::to_string(&ToolbarControl::Panel { panel: Panel::Sizes }).unwrap()
    ));
    d.click_name("confirm-tools");
    for label in ["Brush size", "Clock", "Menu Labels"] {
        assert!(
            state(&d.w)
                .workspace
                .layout
                .header
                .entries()
                .any(|e| e.item.canonical_label() == label)
        );
    }
    assert!(
        !d.named("header-add-clock").is_mapped(),
        "Existing singleton components do not clutter the palette"
    );
    d.click_name("header-canvas-info");
    assert_eq!(
        d.named("header-canvas-info")
            .downcast::<gtk::CheckButton>()
            .unwrap()
            .label()
            .as_deref(),
        Some("Show footer")
    );
    assert!(state(&d.w).workspace.layout.canvas_info.visible && d.w.view_info.root.is_visible());
    assert!(d.w.resolved().status.y > d.w.surface.height() as f32 / 2.);
    assert_eq!(d.w.view_info.root.halign(), gtk::Align::End);
    d.click_name("header-edit-cancel");
    assert_eq!(state(&d.w).workspace.layout.header, HeaderLayout::painter_for_platform(Platform::Gtk));
    d.edit();
    // Removing every editable navigation item cannot strand touch users.
    let ids = state(&d.w)
        .workspace
        .layout
        .header
        .entries()
        .map(|e| e.id)
        .collect::<Vec<_>>();
    for id in ids {
        d.w.dispatch(HeaderAction::Remove { id }.action());
    }
    pump(250);
    d.click_name("header-edit-done");
    assert!(d.named("header-recovery").is_mapped());
    let recovery = d.named("header-recovery").compute_bounds(&d.w.surface).unwrap();
    let close = find_css(d.w.header.root.upcast_ref(), "close").unwrap().compute_bounds(&d.w.surface).unwrap();
    assert_eq!(close.x() - recovery.x() - recovery.width(), state(&d.w).workspace.layout.header.size.item_gap(), "recovery menu clears native close by one gap");
    d.click_name("header-recovery");
    d.click_label("Window");
    d.click_label("Customize Title Bar…");
    assert!(state(&d.w).customization.header_editing);
    crate::capture(
        &d.w,
        d.input.dir.join("empty-bar-palette.png").to_str().unwrap(),
    );
    for (i, item) in HeaderItem::COMPONENTS
        .into_iter()
        .filter(|item| item.available_on(Platform::Gtk))
        .enumerate()
    {
        let zone = HeaderZone::ALL[i % 3];
        let name = item.id();
        d.drop_component(name, zone);
        let model = state(&d.w).workspace.layout.header;
        let id = model.entries().find(|e| e.item == item).unwrap().id;
        assert_eq!(model.location(id).unwrap().0, zone);
        assert_eq!(
            d.named(&format!("header-add-{name}")).is_mapped(),
            !item.singleton()
        );
    }
    crate::capture(
        &d.w,
        d.input
            .dir
            .join("all-components-added.png")
            .to_str()
            .unwrap(),
    );
    d.click_name("header-edit-done");
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_editor_keyboard_input"]
fn native_header_editor_keyboard_input() {
    let mut d = Driver::new("art.capycanvas.HeaderKeyboard");
    let original = state(&d.w).workspace.layout;
    d.edit();
    assert_eq!(
        gtk::prelude::GtkWindowExt::focus(&d.w.window)
            .unwrap()
            .widget_name(),
        "header-size-1"
    );
    let id = original.header.zones[0][1].id;
    d.click_name(&format!("header-item-{id}"));
    for forward in [
        true, true, true, true, true, true, false, false, false, false, false, false,
    ] {
        let previous = state(&d.w).workspace.layout.header;
        let expected = previous.step(id, forward).unwrap();
        let HeaderAction::Move { zone, before, .. } = expected else {
            panic!()
        };
        let mut next = previous.clone();
        next.move_item(id, zone, before).unwrap();
        d.input.key(if forward { 0xff53 } else { 0xff51 });
        assert_eq!(state(&d.w).workspace.layout.header, next);
        assert_eq!(
            gtk::prelude::GtkWindowExt::focus(&d.w.window)
                .unwrap()
                .widget_name(),
            format!("header-item-{id}")
        );
    }
    assert_eq!(state(&d.w).workspace.layout.header, original.header);
    // Existing native keyboard context action and focus return survive rebuild.
    d.input.key(0xff67);
    d.click_label("Move to Center");
    assert_eq!(
        state(&d.w).workspace.layout.header.location(id).unwrap().0,
        HeaderZone::Center
    );
    d.input.key(0xff08);
    assert!(state(&d.w).workspace.layout.header.entry(id).is_err());
    let next = gtk::prelude::GtkWindowExt::focus(&d.w.window)
        .unwrap()
        .widget_name()
        .strip_prefix("header-item-")
        .unwrap()
        .parse::<u32>()
        .unwrap();
    d.input.key(0xffff);
    assert!(state(&d.w).workspace.layout.header.entry(next).is_err());
    assert!(d.named("header-add-main-menu").is_mapped());
    // Native Tab traverses controls and never fires canvas shortcuts.
    let mut sized = false;
    for _ in 0..36 {
        d.input.key(0xff09);
        let focus = gtk::prelude::GtkWindowExt::focus(&d.w.window).unwrap();
        assert!(focus == d.w.header.root || focus.is_ancestor(&d.w.header.root));
        assert!(!state(&d.w).workspace.zen_mode);
        if focus.widget_name() == "header-size-2" {
            d.input.key(0x20);
            assert_eq!(state(&d.w).workspace.layout.header.size, HeaderSize::Large);
            sized = true;
        }
    }
    assert!(sized);
    d.click_name("header-edit-cancel");
    assert_eq!(state(&d.w).workspace.layout.header, original.header);
    assert_eq!(
        state(&d.w).workspace.layout.canvas_info,
        original.canvas_info
    );
    d.edit();
    let root = d.w.header.root.clone();
    let x = (12..root.width() - 72)
        .step_by(12)
        .find(|x| {
            root.pick(*x as f64, 20., gtk::PickFlags::DEFAULT).as_ref() == Some(root.upcast_ref())
        })
        .unwrap();
    d.input.perform(serde_json::json!([{"point":[x,20]},{"button":273,"down":true},{"button":273,"down":false}]));
    d.click_label("Cancel Changes");
    assert!(!state(&d.w).customization.header_editing);
    d.finish();
}

#[test]
#[ignore = "640x480 isolated compositor, --native-test=native_header_editor_short_window_input"]
fn native_header_editor_short_window_input() {
    let mut d = Driver::new("art.capycanvas.HeaderShortWindow");
    assert!(
        d.w.surface.height() == 480,
        "run at the app’s minimum size with LAYER_MOTION_VIEWPORT=640x480"
    );
    d.edit();
    d.w.dispatch(
        HeaderAction::SetSize {
            size: HeaderSize::Large,
        }
        .action(),
    );
    let ids = state(&d.w)
        .workspace
        .layout
        .header
        .entries()
        .map(|e| e.id)
        .collect::<Vec<_>>();
    for id in ids {
        d.w.dispatch(HeaderAction::Remove { id }.action());
    }
    pump(250);
    let panel = d
        .named("header-editor")
        .downcast::<gtk::ScrolledWindow>()
        .unwrap();
    let bounds = panel.compute_bounds(&d.w.surface).unwrap();
    assert!(bounds.y() + bounds.height() <= d.w.surface.height() as f32);
    assert_eq!(panel.width(), d.w.surface.width() - 12);
    assert!(
        panel.height() <= 200,
        "full palette uses the available width"
    );
    // The simplified palette can fit without scrolling even at the minimum
    // size. Either way, native focus must reveal the footer, never clip it.
    assert!(panel.height() <= 480 - HeaderSize::Large.height() as i32 - 12);
    for _ in 0..40 {
        if gtk::prelude::GtkWindowExt::focus(&d.w.window)
            .unwrap()
            .widget_name()
            == "header-edit-done"
        {
            break;
        }
        d.input.key(0xff09);
    }
    let focus = gtk::prelude::GtkWindowExt::focus(&d.w.window).unwrap();
    assert_eq!(focus.widget_name(), "header-edit-done");
    let done = focus.compute_bounds(&d.w.surface).unwrap();
    assert!(done.y() >= bounds.y() && done.y() + done.height() <= bounds.y() + bounds.height());
    crate::capture(
        &d.w,
        d.input.dir.join("short-editor-done.png").to_str().unwrap(),
    );
    d.input.key(0xff0d);
    assert!(!state(&d.w).customization.header_editing);
    d.edit();
    d.click_name("header-edit-cancel");
    assert!(!state(&d.w).customization.header_editing);
    d.finish();
}

#[test]
#[ignore = "640px isolated compositor, --native-test=native_header_compact_switcher_input --native-storage"]
fn native_header_compact_switcher_input() {
    let mut d = Driver::managed("art.capycanvas.HeaderCompactSwitcher");
    assert!(
        d.w.surface.width() <= 800,
        "use LAYER_MOTION_VIEWPORT=640x600"
    );
    let manager = d.w.workspaces.manager().unwrap();
    let settle = |input| {
        let prompt = matches!(&input, layer_workspace::WorkspaceInput::Form { action: layer_workspace::ManagerAction::New });
        d.w.workspaces.send(&d.w, input);
        until(
            || {
                let view = d.w.workspaces.view();
                !view.busy && !view.switcher_busy && view.prompt.is_some() == prompt
            },
            "workspace edit",
        );
    };
    settle(layer_workspace::WorkspaceInput::Form {
        action: layer_workspace::ManagerAction::New,
    });
    settle(layer_workspace::WorkspaceInput::Submit {
        name: "Wide workspace".into(),
        description: None,
        choice: None,
    });
    wait_workspaces(&d.w);
    let ids = manager.switcher_display_ids();
    assert_eq!(ids.len(), 4);
    settle(layer_workspace::WorkspaceInput::EditSwitcher {
        edit: layer_workspace::SwitcherEdit::Move {
            id: ids[2].clone(),
            before: Some(ids[0].clone()),
        },
    });

    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        for (input, size) in HeaderSize::ALL.into_iter().enumerate() {
            let mut workspace = state(&d.w).workspace;
            workspace.layout.header = HeaderLayout::painter();
            workspace.layout.header.size = size;
            d.w.dispatch(UiAction::RestoreWorkspace {
                workspace: Box::new(workspace),
            });
            pump(250);
            let selector = d.named("header-workspace-selector");
            assert!(selector.is_mapped(), "compact selector at {size:?}");
            assert_eq!(
                selector.downcast_ref::<gtk::MenuButton>().unwrap().label(),
                manager.active_name().map(Into::into)
            );
            let active = manager.active_id();
            let point = d.point(&selector);
            d.input.perform(serde_json::json!([{"point":point},{"button":273,"down":true},{"button":273,"down":false}]));
            let context = d.named("workspace-switcher-options-context").downcast::<gtk::PopoverMenu>().unwrap();
            assert!(context.is_visible());
            assert_eq!(manager.active_id(), active);
            let checklist = context.menu_model().unwrap().item_link(0, gtk::gio::MENU_LINK_SECTION).unwrap();
            assert_eq!(checklist.n_items() as usize, manager.items().len());
            d.input.key(0xff1b);
            assert!(!context.is_visible());
            let mut overflow_count = 0;
            for zone in HeaderZone::ALL {
                let button = d.named(&format!("header-overflow-{}", zone.index()));
                if button.is_mapped() {
                    overflow_count += 1;
                    let image = button
                        .downcast_ref::<gtk::MenuButton>()
                        .unwrap()
                        .child()
                        .and_downcast::<gtk::Image>()
                        .unwrap();
                    assert_eq!(image.pixel_size(), size.icon());
                    assert_eq!(
                        image.measure(gtk::Orientation::Horizontal, -1).1,
                        size.icon()
                    );
                    assert_eq!(image.measure(gtk::Orientation::Vertical, -1).1, size.icon());
                    let bounds = image.compute_bounds(&button).unwrap();
                    assert!(bounds.width() >= size.icon() as f32);
                    assert!(bounds.height() >= size.icon() as f32);
                    assert!(
                        (bounds.x() + bounds.width() / 2. - button.width() as f32 / 2.).abs() < 1.
                    );
                    assert!(
                        (bounds.y() + bounds.height() / 2. - button.height() as f32 / 2.).abs()
                            < 1.
                    );
                }
            }
            assert!(overflow_count > 0);
            d.click(&selector);
            let popup = d.named("workspace-switcher-popup");
            let model = popup
                .downcast_ref::<gtk::PopoverMenu>()
                .unwrap()
                .menu_model()
                .unwrap();
            let choices = manager.switcher_display_ids();
            assert_eq!(model.n_items(), 3);
            let visibility = model.item_link(1, gtk::gio::MENU_LINK_SECTION).unwrap();
            assert_eq!(visibility.item_attribute_value(0, "label", None).unwrap().get::<String>().as_deref(), Some("Show in top bar"));
            assert_eq!(visibility.item_link(0, gtk::gio::MENU_LINK_SUBMENU).unwrap().item_link(0, gtk::gio::MENU_LINK_SECTION).unwrap().n_items() as usize, manager.items().len());
            let switching = model.item_link(0, gtk::gio::MENU_LINK_SECTION).unwrap();
            assert_eq!(switching.n_items() as usize, choices.len());
            for (index, choice) in d.w.workspaces.view().switcher_display.iter().enumerate() {
                assert_eq!(
                    switching
                        .item_attribute_value(index as i32, "label", None)
                        .unwrap()
                        .get::<String>()
                        .as_ref(),
                    Some(&choice.title),
                    "dropdown follows the pill's configured order"
                );
            }
            crate::capture(
                &d.w,
                d.input
                    .dir
                    .join(format!("compact-{theme:?}-{size:?}.png"))
                    .to_str()
                    .unwrap(),
            );
            capture_popover(
                popup
                    .downcast_ref::<gtk::PopoverMenu>()
                    .unwrap()
                    .upcast_ref(),
                d.input
                    .dir
                    .join(format!("choices-{theme:?}-{size:?}.png"))
                    .to_str()
                    .unwrap(),
            );
            let target = choices
                .iter()
                .find(|id| Some(*id) != manager.active_id().as_ref())
                .unwrap()
                .clone();
            let name = manager
                .items()
                .into_iter()
                .find(|item| item.id == target)
                .unwrap()
                .metadata
                .name;
            let label = d.label(&name);
            match input {
                0 => d.click(&label),
                1 => {
                    let point = d.point(&label);
                    d.input.perform(serde_json::json!([
                        { "touch": "down", "point": point }, { "touch": "up" }
                    ]));
                }
                _ => {
                    let mut row = label;
                    while !row.is_focusable() {
                        row = row.parent().unwrap();
                    }
                    assert!(row.grab_focus());
                    d.input.key(0xff0d);
                }
            }
            wait_workspaces(&d.w);
            assert_eq!(manager.active_id().as_ref(), Some(&target));
            assert!(!popup.is_visible());
        }
    }

    // A switcher moved into a crowded zone uses the same choices from overflow.
    let mut workspace = state(&d.w).workspace;
    workspace.layout.header = HeaderLayout::painter();
    workspace.layout.header.size = HeaderSize::Large;
    d.w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(workspace),
    });
    let id = state(&d.w)
        .workspace
        .layout
        .header
        .entries()
        .find(|entry| entry.item == HeaderItem::Workspaces)
        .unwrap()
        .id;
    d.w.dispatch(
        HeaderAction::Move {
            id,
            zone: HeaderZone::Right,
            before: None,
        }
        .action(),
    );
    pump(250);
    assert!(!d.named(&format!("header-item-{id}")).is_mapped());
    d.click_name("header-overflow-2");
    d.click_name(&format!("header-overflow-item-{id}"));
    assert!(d.named("workspace-switcher-popup").is_mapped());
    let target = manager
        .switcher_display_ids()
        .into_iter()
        .find(|id| Some(id) != manager.active_id().as_ref())
        .unwrap();
    let name = manager
        .items()
        .into_iter()
        .find(|item| item.id == target)
        .unwrap()
        .metadata
        .name;
    d.click_label(&name);
    wait_workspaces(&d.w);
    assert_eq!(manager.active_id().as_ref(), Some(&target));
    d.finish();
}

#[test]
#[ignore = "640px isolated compositor, --native-test=native_header_overflow_input"]
fn native_header_overflow_input() {
    let mut d = Driver::new("art.capycanvas.HeaderOverflow");
    assert!(
        d.w.surface.width() <= 800,
        "run with LAYER_MOTION_VIEWPORT=640x600"
    );
    let baseline = state(&d.w).workspace.layout.header;
    for size in HeaderSize::ALL {
        d.w.dispatch(HeaderAction::SetSize { size }.action());
        pump(240);
        let model = state(&d.w).workspace.layout.header;
        for zone in HeaderZone::ALL {
            for entry in &model.zones[zone.index()] {
                if !d.named(&format!("header-item-{}", entry.id)).is_mapped() {
                    let overflow = d.named(&format!("header-overflow-{}", zone.index()));
                    d.click(&overflow);
                    if find_named(
                        d.w.window.upcast_ref(),
                        &format!("header-overflow-item-{}", entry.id),
                    )
                    .is_none()
                    {
                        crate::capture(
                            &d.w,
                            d.input.dir.join("missing-overflow.png").to_str().unwrap(),
                        );
                        eprintln!(
                            "Missing overflow {:?} at {:?}; dialog {:?} fullscreen {}",
                            entry.item,
                            size,
                            d.w.window.visible_dialog(),
                            d.w.window.is_fullscreen()
                        );
                    }
                    let row = d.named(&format!("header-overflow-item-{}", entry.id));
                    if !row.is_mapped() {
                        crate::capture(
                            &d.w,
                            d.input.dir.join("overflow-failure.png").to_str().unwrap(),
                        );
                    }
                    assert!(
                        row.is_mapped(),
                        "hidden item {} must be reachable at {size:?}",
                        entry.item.canonical_label()
                    );
                    if row.is_sensitive() {
                        d.click(&row);
                        if matches!(
                            entry.item,
                            HeaderItem::Tool {
                                control: ToolbarControl::Color | ToolbarControl::Panel { .. }
                            }
                        ) {
                            assert!(
                                state(&d.w).customization.drawer.is_some(),
                                "overflow drawer for {:?}",
                                entry.item
                            );
                            pump(400);
                            assert!(
                                state(&d.w).customization.drawer.is_some(),
                                "overflow origin must survive a new allocation"
                            );
                            assert!(
                                find_named(d.w.window.upcast_ref(), "tool-drawer")
                                    .unwrap()
                                    .is_mapped()
                            );
                        }
                    }
                    d.w.dispatch(UiAction::Customize {
                        action: CustomizationAction::CloseExpanded,
                    });
                    if entry.item == HeaderItem::Settings {
                        // On a narrow NavigationSplitView, Escape navigates
                        // back to the sidebar first. Use the native close control.
                        let content =
                            find_named(d.w.preferences.dialog.upcast_ref(), "preferences-content")
                                .unwrap();
                        d.click(&find_css(&content, "close").unwrap());
                    } else {
                        d.input.key(0xff1b);
                    }
                    pump(400);
                    if entry.item == HeaderItem::Fullscreen && d.w.window.is_fullscreen() {
                        // Keep each overflow case in the same window geometry.
                        d.input.key(0xffc8); // F11, native Full Screen shortcut.
                        pump(400);
                    }
                }
            }
        }
        d.w.dispatch(HeaderAction::Edit { editing: true }.action());
        pump(160);
        assert!(d.named("header-edit-done").is_mapped());
        crate::capture(
            &d.w,
            d.input
                .dir
                .join(format!("narrow-editor-{size:?}.png"))
                .to_str()
                .unwrap(),
        );
        if let Some(entry) = model
            .entries()
            .find(|e| !d.named(&format!("header-item-{}", e.id)).is_mapped())
        {
            let zone = model.location(entry.id).unwrap().0;
            d.click_name(&format!("header-overflow-{}", zone.index()));
            d.click_name(&format!("header-overflow-item-{}", entry.id));
            d.drop_component("tools", HeaderZone::Center);
            assert!(d.w.window.visible_dialog().is_some());
            crate::capture(
                &d.w,
                d.input
                    .dir
                    .join(format!("narrow-picker-{size:?}.png"))
                    .to_str()
                    .unwrap(),
            );
            d.input.key(0xff1b);
            assert!(state(&d.w).customization.header_editing);
            assert!(state(&d.w).customization.picker.is_none());
            assert!(d.w.window.visible_dialog().is_none());
        }
        d.click_name("header-edit-done");
        assert!(!state(&d.w).customization.header_editing);
    }
    assert_eq!(
        state(&d.w).workspace.layout.header.zones,
        baseline.zones,
        "overflow never changes the stored item arrangement"
    );
    crate::capture(&d.w, d.input.dir.join("narrow.png").to_str().unwrap());
    d.finish();
}

#[test]
#[ignore = "isolated compositor, --native-test=native_header_canvas_visual"]
fn native_header_canvas_visual() {
    let d = Driver::new("art.capycanvas.HeaderCanvas");
    for _ in 0..8 {
        d.w.dispatch(UiAction::Invoke {
            command: CommandId::ZoomIn,
        });
    }
    pump(500);
    for theme in [Theme::Dark, Theme::Light] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(250);
        let _warm = crate::snapshot(&d.w);
        pump(120);
        let snapshot = crate::snapshot(&d.w);
        assert_eq!(snapshot.width(), d.w.surface.width());
        assert_eq!(snapshot.height(), d.w.surface.height());
        let mut bytes = vec![0u8; snapshot.width() as usize * snapshot.height() as usize * 4];
        snapshot.download(&mut bytes, snapshot.width() as usize * 4);
        let offset = (20 * snapshot.width() as usize + 400) * 4;
        assert!(
            bytes[offset..offset + 3].iter().all(|v| *v > 245),
            "white artwork must remain visible beneath the window bar"
        );
        snapshot
            .save_to_png(d.input.dir.join(format!("canvas-behind-{theme:?}.png")))
            .unwrap();
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_slide_remove_input"]
fn native_header_slide_remove_input() {
    let mut d = Driver::new("art.capycanvas.HeaderSlideRemove");
    let original = state(&d.w).workspace.layout.header;
    for touch in [false, true] {
        let device = if touch { "touch" } else { "mouse" };
        for tool in [false, true] {
            d.edit();
            let id = original.zones[0][if tool { 2 } else { 1 }].id;
            let neighbor = original.zones[0][if tool { 3 } else { 2 }].id;
            let widget = d.named(&format!("header-item-{id}"));
            let rect = widget.compute_bounds(&d.w.surface).unwrap();
            let start = [rect.x() + rect.width() - 8., rect.y() + rect.height() / 2.];
            let offset = [start[0] - rect.x(), start[1] - rect.y()];
            let initial = d.w.header.geometry_for_test();
            let move_to = |d: &mut Driver, point: [f32; 2]| {
                d.input
                    .perform(serde_json::json!([contact(device, "move", point)]))
            };
            d.input
                .perform(serde_json::json!([contact(device, "down", start)]));
            move_to(&mut d, [start[0] + 50., start[1] + 4.]);
            let preview = d.w.header.drag_for_test().expect("native drag visual");
            assert!(!preview.detached);
            assert!((preview.held.x - (rect.x() + 50.)).abs() < 0.1);
            assert_eq!(preview.held.y, state(&d.w).workspace.layout.header.size.item_gap());
            let x =
                |g: &HeaderGeometry| g.items.iter().find(|m| m.id == neighbor).unwrap().bounds.x;
            assert!(
                x(&preview.geometry) < x(&initial),
                "neighbors shift before release"
            );
            assert_eq!(
                state(&d.w).workspace.layout.header,
                original,
                "motion is only a preview"
            );
            crate::capture(
                &d.w,
                d.input
                    .dir
                    .join(format!("sliding-{touch}-{tool}.png"))
                    .to_str()
                    .unwrap(),
            );
            // Backtracking uses grab-time slots, not the animated rectangles.
            move_to(&mut d, start);
            assert_eq!(
                d.w.header.drag_for_test().unwrap().geometry.items,
                initial.items
            );
            let outside = [start[0] + 170., 240.];
            move_to(&mut d, outside);
            let preview = d.w.header.drag_for_test().unwrap();
            assert!(preview.detached && preview.target.is_none());
            assert_eq!(preview.held.x, outside[0] - offset[0]);
            assert_eq!(preview.held.y, outside[1] - offset[1]);
            assert!(
                matches!(preview.action, Some(HeaderAction::Remove { id:removed }) if removed == id)
            );
            crate::capture(
                &d.w,
                d.input
                    .dir
                    .join(format!("detached-{touch}-{tool}.png"))
                    .to_str()
                    .unwrap(),
            );
            // Re-entering attaches the same contact without applying an edit.
            let center = [d.w.surface.width() as f32 / 2., start[1]];
            move_to(&mut d, center);
            assert!(!d.w.header.drag_for_test().unwrap().detached);
            assert_eq!(
                d.w.header.drag_for_test().unwrap().target.unwrap().0,
                HeaderZone::Center
            );
            move_to(&mut d, outside);
            d.input
                .perform(serde_json::json!([contact(device, "up", outside)]));
            assert!(state(&d.w).workspace.layout.header.entry(id).is_err());
            assert!(d.w.header.drag_for_test().is_none());
            for entry in state(&d.w).workspace.layout.header.entries() {
                assert_eq!(d.named(&format!("header-item-{}", entry.id)).opacity(), 1.);
            }
            assert_eq!(
                d.named("header-add-main-menu").is_mapped(),
                !tool,
                "only removed singleton components return to the palette"
            );
            d.click_name("header-edit-cancel");
            assert_eq!(state(&d.w).workspace.layout.header, original);
        }
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_tools_drop_input"]
fn native_header_tools_drop_input() {
    let mut d = Driver::new("art.capycanvas.HeaderToolsDrop");
    let original = state(&d.w).workspace.layout.header;
    for touch in [false, true] {
        let device = if touch { "touch" } else { "mouse" };
        d.edit();
        let start = d.point(&d.named("header-add-tools"));
        let target = [d.w.surface.width() as f32 / 2., 25.];
        d.input.perform(serde_json::json!([
            contact(device, "down", start),
            contact(device, "move", target)
        ]));
        assert!(matches!(
            d.w.header.drag_for_test().unwrap().action,
            Some(HeaderAction::InsertTools {
                zone: HeaderZone::Center,
                ..
            })
        ));
        assert!(
            d.w.window.visible_dialog().is_none(),
            "picker opens on drop, not during drag"
        );
        crate::capture(
            &d.w,
            d.input
                .dir
                .join(format!("tools-drop-{touch}.png"))
                .to_str()
                .unwrap(),
        );
        d.input
            .perform(serde_json::json!([contact(device, "up", target)]));
        assert!(d.w.window.visible_dialog().is_some());
        assert!(d.w.header.drag_for_test().is_none());
        assert_eq!(
            state(&d.w).workspace.layout.header,
            original,
            "a Tools placeholder is never stored"
        );
        // Native editing keys belong to search, not to the selected bar item.
        d.input.key(0x61);
        d.input.key(0xff08);
        d.input.key(0xffff);
        assert_eq!(state(&d.w).workspace.layout.header, original);
        d.named("tool-search")
            .downcast::<gtk::SearchEntry>()
            .unwrap()
            .set_text("Brush size");
        pump(300);
        d.click_name(&format!(
            "tool-choice-{}",
            serde_json::to_string(&ToolbarControl::Panel { panel: Panel::Sizes }).unwrap()
        ));
        d.click_name("confirm-tools");
        let model = state(&d.w).workspace.layout.header;
        let entry = model
            .entries()
            .find(|e| {
                e.item
                    == HeaderItem::Tool {
                        control: ToolbarControl::Panel { panel: Panel::Sizes },
                    }
            })
            .unwrap();
        assert_eq!(model.location(entry.id).unwrap().0, HeaderZone::Center);
        d.click_name("header-edit-cancel");
        assert_eq!(state(&d.w).workspace.layout.header, original);
    }
    d.finish();
}

#[test]
#[ignore = "640px isolated compositor, --native-test=native_header_overflow_drag_input"]
fn native_header_overflow_drag_input() {
    let mut d = Driver::new("art.capycanvas.HeaderOverflowDrag");
    assert!(d.w.surface.width() <= 800, "run at 640x600");
    for size in HeaderSize::ALL {
        d.w.dispatch(HeaderAction::SetSize { size }.action());
        pump(250);
        for touch in [false, true] {
            let device = if touch { "touch" } else { "mouse" };
            d.edit();
            let original = state(&d.w).workspace.layout.header;
            let geometry = d.w.header.geometry_for_test();
            assert!(geometry.overflow.iter().any(Option::is_some));
            let id = geometry.items[0].id;
            let start = d.point(&d.named(&format!("header-grip-{id}")));
            let outside = [start[0] + 30., size.height() + 180.];
            d.input.perform(serde_json::json!([
                contact(device, "down", start),
                contact(device, "move", outside)
            ]));
            let preview = d.w.header.drag_for_test().expect("overflow drag preview");
            assert!(preview.detached);
            assert_eq!(state(&d.w).workspace.layout.header, original);
            crate::capture(
                &d.w,
                d.input
                    .dir
                    .join(format!("overflow-drag-{size:?}-{touch}.png"))
                    .to_str()
                    .unwrap(),
            );
            d.input
                .perform(serde_json::json!([contact(device, "up", outside)]));
            assert!(d.w.header.drag_for_test().is_none());
            let model = state(&d.w).workspace.layout.header;
            assert_eq!(model.entries().count(), original.entries().count() - 1);
            assert!(model.entry(id).is_err());
            assert_eq!(d.w.header.geometry_for_test().items, preview.geometry.items);
            for entry in model.entries() {
                assert_eq!(d.named(&format!("header-item-{}", entry.id)).opacity(), 1.);
            }
            // The editor transaction restores hidden entries as well as visible
            // ones. No temporary capture allocation becomes authoritative.
            d.click_name("header-edit-cancel");
            assert_eq!(state(&d.w).workspace.layout.header, original);
        }
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_empty_center_input"]
fn native_header_empty_center_input() {
    let mut d = Driver::new("art.capycanvas.HeaderEmptyCenter");
    for size in HeaderSize::ALL {
        d.w.dispatch(HeaderAction::SetSize { size }.action());
        pump(250);
        for touch in [false, true] {
            let device = if touch { "touch" } else { "mouse" };
            for edge in [-1., 1.] {
                d.edit();
                let original = state(&d.w).workspace.layout.header;
                let id = original.zones[1][0].id;
                d.click_name(&format!("header-item-{id}"));
                d.input.key(0xffff);
                assert!(state(&d.w).workspace.layout.header.zones[1].is_empty());
                let b = d.w.header.geometry_for_test().zones[1];
                assert!(b.width >= 160., "empty center remains easy to target");
                let point = [
                    b.x + b.width / 2. + edge * (b.width / 2. - 10.),
                    b.y + b.height / 2.,
                ];
                assert!((point[0] - d.w.surface.width() as f32 / 2.).abs() > 40.);
                let start = d.point(&d.named("header-add-grip-workspace-switcher"));
                d.input.perform(serde_json::json!([
                    contact(device, "down", start),
                    contact(device, "move", point)
                ]));
                assert_eq!(
                    d.w.header.drag_for_test().unwrap().target,
                    Some((HeaderZone::Center, None))
                );
                crate::capture(
                    &d.w,
                    d.input
                        .dir
                        .join(format!("empty-center-{size:?}-{touch}-{edge}.png"))
                        .to_str()
                        .unwrap(),
                );
                d.input
                    .perform(serde_json::json!([contact(device, "up", point)]));
                let model = state(&d.w).workspace.layout.header;
                assert_eq!(model.zones[1].len(), 1);
                assert_eq!(model.zones[1][0].item, HeaderItem::Workspaces);
                assert_eq!(model.zones[0], original.zones[0]);
                assert_eq!(model.zones[2], original.zones[2]);
                d.click_name("header-edit-cancel");
                assert_eq!(state(&d.w).workspace.layout.header, original);
            }
        }
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_header_window_actions_input"]
fn native_header_window_actions_input() {
    let mut d = Driver::new("art.capycanvas.HeaderWindowActions");
    assert!(
        !state(&d.w)
            .workspace
            .layout
            .header
            .entries()
            .any(|e| e.item == HeaderItem::Fullscreen)
    );
    d.w.dispatch(UiAction::RestoreWorkspace {
        workspace: Box::new(WorkspaceState {
            layout: WorkspacePreset::Illustrator.layout(Platform::Gtk),
            ..WorkspaceState::default()
        }),
    });
    pump(500);
    let model = state(&d.w).workspace.layout.header;
    let settings = model
        .entries()
        .find(|e| e.item == HeaderItem::Settings)
        .unwrap()
        .id;
    assert!(!model.entries().any(|e| e.item == HeaderItem::Fullscreen));
    // Portable/saved Web components must not reintroduce a native fullscreen
    // button, reserve a gap, or appear in the overflow menu/editor palette.
    d.w.dispatch(
        HeaderAction::Add {
            zone: HeaderZone::Right,
            before: Some(settings),
            item: HeaderItem::Fullscreen,
        }
        .action(),
    );
    let model = state(&d.w).workspace.layout.header;
    let ids: Vec<_> = [
        HeaderItem::Clock,
        HeaderItem::Battery,
        HeaderItem::Fullscreen,
    ]
    .map(|item| model.entries().find(|e| e.item == item).unwrap().id)
    .into();
    d.w.system_status
        .show_battery(Some(crate::system_status::Battery {
            percent: 73,
            charging: false,
            low: false,
        }));
    for size in HeaderSize::ALL {
        d.w.dispatch(HeaderAction::SetSize { size }.action());
        pump(250);
        let geometry = d.w.header.geometry_for_test();
        for id in &ids {
            assert!(!geometry.items.iter().any(|item| item.id == *id));
        }
        assert!(!d.w.system_status.clock.is_visible());
        assert!(!d.w.system_status.battery.is_visible());
        d.edit();
        for id in &ids[..2] {
            assert!(
                d.w.header
                    .geometry_for_test()
                    .items
                    .iter()
                    .any(|item| item.id == *id),
                "status placeholders are editable while windowed"
            );
        }
        assert!(
            !d.w.header
                .geometry_for_test()
                .items
                .iter()
                .any(|item| item.id == ids[2])
        );
        assert!(find_named(d.w.header.root.upcast_ref(), "header-add-full-screen").is_none());
        d.click_name("header-edit-done");
        d.click_name(&format!("header-item-{settings}"));
        assert!(state(&d.w).settings_open && d.w.window.visible_dialog().is_some());
        crate::capture(
            &d.w,
            d.input
                .dir
                .join(format!("settings-open-{size:?}.png"))
                .to_str()
                .unwrap(),
        );
        d.input.key(0xff1b);
        pump(400); // Adwaita emits closed after its closing animation.
        if state(&d.w).settings_open {
            crate::capture(
                &d.w,
                d.input
                    .dir
                    .join("settings-not-closed.png")
                    .to_str()
                    .unwrap(),
            );
            eprintln!(
                "Settings focus: {:?}, can close {}",
                gtk::prelude::GtkWindowExt::focus(&d.w.window),
                d.w.preferences.dialog.can_close()
            );
        }
        assert!(!state(&d.w).settings_open);
        for enabled in [true, false] {
            d.input.key(0xffc8); // Native fullscreen remains available through F11/menu.
            pump(500);
            assert_eq!(d.w.window.is_fullscreen(), enabled);
            assert_eq!(state(&d.w).fullscreen, enabled);
            assert_eq!(d.w.system_status.clock.is_visible(), enabled);
            assert_eq!(d.w.system_status.battery.is_visible(), enabled);
            let geometry = d.w.header.geometry_for_test();
            for id in &ids[..2] {
                assert_eq!(geometry.items.iter().any(|item| item.id == *id), enabled);
            }
            assert!(!geometry.items.iter().any(|item| item.id == ids[2]));
            crate::capture(
                &d.w,
                d.input
                    .dir
                    .join(format!("status-{size:?}-{enabled}.png"))
                    .to_str()
                    .unwrap(),
            );
        }
    }
    d.finish();
}

#[path = "selection_tests.rs"]
mod selection_tools;
#[path = "vector_layer_tests.rs"]
mod vector_layers;
#[path = "vector_edge_tests.rs"]
mod vector_edges;
#[path = "toolbar_component_tests.rs"]
mod toolbar_components;
#[path = "color_mixing_tests.rs"]
mod color_mixing;
#[path = "blender_tests.rs"]
mod blenders;

#[test]
#[ignore = "isolated native-input.js --native-test=native_layer_swipe_alpha_lock"]
fn native_layer_swipe_alpha_lock() { layer_swipes(&["touch", "pen"]); }

#[test]
#[ignore = "isolated native-input.js --native-test=native_sketch_layer_swipe_actions"]
fn native_sketch_layer_swipe_actions() { sketch_layer_swipe_actions("touch"); }

#[test]
#[ignore = "isolated native-input.js --tablet --native-test=native_sketch_layer_swipe_actions_pen"]
fn native_sketch_layer_swipe_actions_pen() { sketch_layer_swipe_actions("pen"); }

fn layer_swipe_view_matches(row:&crate::swipe_row::SwipeRow, model:&layer_ui::LayerState) {
    let actions=row.delete_button().parent().unwrap();
    let mut names=Vec::new();
    let mut child=actions.first_child();
    while let Some(widget)=child {
        if widget.is_visible() {names.push(widget.widget_name().to_string());}
        child=widget.next_sibling();
    }
    assert_eq!(names,model.swipe_actions.iter().map(|action|format!("layer-swipe-{}",action.id)).collect::<Vec<_>>(),"shared swipe action order");
    for action in &model.swipe_actions {
        let button=match action.id {"lock"=>row.lock_button(),"duplicate"=>row.duplicate_button(),"delete"=>row.delete_button(),_=>panic!("unknown native swipe action")};
        assert_eq!(button.is_sensitive(),action.enabled);
        assert_eq!(button.child().unwrap().downcast::<gtk::Label>().unwrap().text().as_str(),action.label.as_ref());
        assert_eq!(button.tooltip_text().as_deref(),Some(action.label.as_ref()));
    }
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_sketch_layer_swipe_motion"]
fn native_sketch_layer_swipe_motion() {
    let mut d=Driver::new("art.capycanvas.SketchSwipeMotion");
    let output=std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from).unwrap_or_else(||d.input.dir.clone());
    std::fs::create_dir_all(&output).unwrap();
    let mut reports=Vec::new();
    for theme in [Theme::Light,Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        d.w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(WorkspaceState {
            layout:WorkspacePreset::Painter.layout(Platform::Gtk),..WorkspaceState::default()
        })});
        d.w.dispatch(UiAction::SelectLayer {id:1}); pump(250);
        let opener=d.header_tool(ToolbarControl::Panel {panel:Panel::Layers});
        d.click_name(&opener);
        let stack=d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
        layer_page_settled(&d,&stack,"swipe motion Layers ready");
        for repeat in 0..3 {
            let window=Instant::now();
            while window.elapsed()<Duration::from_secs(6) {
                for open in [true,false] {
                    let content=d.named("art-layer-1");
                    let row=content.parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
                    let x=Rc::new(Cell::new(content.compute_bounds(&row).unwrap().x()));
                    let timings=Rc::new(RefCell::new(Vec::new()));
                    let clock=d.w.surface.frame_clock().unwrap();
                    let capture=clock.connect_after_paint(glib::clone!(#[strong] timings,#[strong] x,#[weak] content,#[weak] row,move |clock| {
                        if let Some(bounds)=content.compute_bounds(&row)
                            && x.replace(bounds.x())!=bounds.x() && let Some(timing)=clock.current_timings() {timings.borrow_mut().push(timing);}
                    }));
                    let p=d.point(row.upcast_ref());
                    let distance=if open {-240.} else {72.};
                    let mut events=vec![contact("touch","down",p)];
                    for step in 1..=18 {events.push(contact("touch","move",[p[0]+distance*step as f32/18.,p[1]]));}
                    events.push(contact("touch","up",[p[0]+distance,p[1]]));
                    d.input.perform(serde_json::Value::Array(events)); pump(100);
                    clock.disconnect(capture);
                    assert_eq!(row.is_open(),open);
                    let mut times:Vec<_>=timings.borrow().iter().filter(|t|t.is_complete() && t.presentation_time()>0).map(|t|t.presentation_time()).collect();
                    times.sort_unstable(); times.dedup();
                    assert!(times.len()>2,"moving swipe presentations");
                    let intervals:Vec<_>=times.windows(2).map(|pair|pair[1]-pair[0]).collect();
                    reports.push(serde_json::json!({"theme":format!("{theme:?}"),"repeat":repeat,"open":open,"presentations":times,"intervals_us":intervals,"viewport":[d.w.surface.width(),d.w.surface.height()]}));
                }
            }
        }
        let _warm=crate::snapshot(&d.w); pump(120);
        crate::snapshot(&d.w).save_to_png(output.join(format!("swipe-closed-{theme:?}.png"))).unwrap();
        d.click_name(&opener); wait_for_drawer_close(&d.w);
    }
    std::fs::write(output.join("swipe-animation.json"),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
    d.finish();
}

fn sketch_layer_swipe_actions(device: &str) {
    let mut d = Driver::new("art.capycanvas.SketchLayerSwipeActions");
    for theme in [Theme::Light,Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        d.w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(WorkspaceState {
            layout:WorkspacePreset::Painter.layout(Platform::Gtk),..WorkspaceState::default()
        })});
        d.w.dispatch(UiAction::SelectLayer {id:1});
        pump(250);
        let opener = d.header_tool(ToolbarControl::Panel {panel:Panel::Layers});
        d.click_name(&opener);
        let stack = d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
        layer_page_settled(&d,&stack,"swipe Layers ready");
        let swipe = || d.named("art-layer-1").parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
        let row = swipe();
        let start = d.point(&d.named("art-layer-1"));
        d.input.perform(serde_json::json!([contact(device,"down",start),contact(device,"move",[start[0]-18.,start[1]]),contact(device,"up",[start[0]-18.,start[1]])]));
        pump(180); assert!(!row.is_open(),"short left swipe remains closed");
        let open = |d:&mut Driver| {
            let row=d.named("art-layer-1").parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
            if row.is_open() {return;}
            let p=d.point(row.upcast_ref());
            d.input.perform(serde_json::json!([contact(device,"down",p),contact(device,"move",[p[0]-30.,p[1]]),contact(device,"move",[p[0]-240.,p[1]]),contact(device,"up",[p[0]-240.,p[1]])]));
            pump(180);
        };
        open(&mut d);
        let row=d.named("art-layer-1").parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
        assert!(row.is_open());
        layer_swipe_view_matches(&row,state(&d.w).layers.iter().find(|layer|layer.id==1).unwrap());
        let output=std::env::var_os("LAYER_TEST_ARTIFACTS").map(std::path::PathBuf::from).unwrap_or_else(||d.input.dir.clone());
        std::fs::create_dir_all(&output).unwrap();
        let _warm=crate::snapshot(&d.w); pump(120);
        crate::snapshot(&d.w).save_to_png(output.join(format!("swipe-actions-{device}-{theme:?}.png"))).unwrap();
        for button in [row.lock_button(),row.duplicate_button(),row.delete_button()] {
            assert!(button.is_sensitive());
            let label=button.child().unwrap().downcast::<gtk::Label>().unwrap();
            assert!(!label.layout().is_ellipsized(),"English swipe action fits: {}",label.text());
        }
        let p=d.point(row.lock_button().upcast_ref());
        d.input.perform(serde_json::json!([contact(device,"down",p),contact(device,"up",p)]));
        assert!(state(&d.w).layers.iter().find(|l|l.id==1).unwrap().locked);
        pump(150); assert!(!row.is_open(),"Lock closes action strip");
        open(&mut d);
        let row=d.named("art-layer-1").parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
        assert!(row.lock_button().is_sensitive());
        assert_eq!(row.lock_button().child().unwrap().downcast::<gtk::Label>().unwrap().text(),"Unlock");
        assert!(row.duplicate_button().is_sensitive()); assert!(!row.delete_button().is_sensitive());
        let p=d.point(row.lock_button().upcast_ref());
        d.input.perform(serde_json::json!([contact(device,"down",p),contact(device,"up",p)]));
        assert!(!state(&d.w).layers.iter().find(|l|l.id==1).unwrap().locked);
        pump(150); assert!(!row.is_open(),"Unlock closes action strip");
        assert_eq!(row.lock_button().child().unwrap().downcast::<gtk::Label>().unwrap().text(),"Lock");
        let count=state(&d.w).layers.len();
        open(&mut d);
        let row=d.named("art-layer-1").parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
        let p=d.point(row.duplicate_button().upcast_ref());
        d.input.perform(serde_json::json!([contact(device,"down",p),contact(device,"up",p)]));
        assert_eq!(state(&d.w).layers.len(),count+1);
        pump(150); assert!(!row.is_open(),"Duplicate closes action strip");
        d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x7a,"down":true},{"key":0x7a,"down":false},{"key":0xffe3,"down":false}]));
        assert_eq!(state(&d.w).layers.len(),count);
        open(&mut d);
        let row=d.named("art-layer-1").parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
        let p=d.point(row.delete_button().upcast_ref());
        d.input.perform(serde_json::json!([contact(device,"down",p),contact(device,"up",p)]));
        assert!(!state(&d.w).layers.iter().any(|l|l.id==1));
        d.input.perform(serde_json::json!([{"key":0xffe3,"down":true},{"key":0x7a,"down":true},{"key":0x7a,"down":false},{"key":0xffe3,"down":false}]));
        assert!(state(&d.w).layers.iter().any(|l|l.id==1));
        open(&mut d);
        let row=d.named("art-layer-1").parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
        let p=d.point(row.upcast_ref());
        d.input.perform(serde_json::json!([contact(device,"down",p),contact(device,"move",[p[0]+30.,p[1]]),contact(device,"move",[p[0]+260.,p[1]]),contact(device,"up",[p[0]+260.,p[1]])]));
        pump(180); assert!(!row.is_open(),"reverse swipe closes action strip");
        assert!(!state(&d.w).layers.iter().find(|l|l.id==1).unwrap().alpha_locked,"closing actions does not invoke right swipe");
        let p=d.point(&d.named("art-layer-1"));
        d.input.perform(serde_json::json!([contact(device,"down",p),contact(device,"move",[p[0]-240.,p[1]])]));
        let controllers=row.observe_controllers();
        for i in 0..controllers.n_items() {
            if let Some(gesture)=controllers.item(i).and_downcast::<gtk::GestureDrag>() {gesture.set_state(gtk::EventSequenceState::Denied);}
        }
        d.input.perform(serde_json::json!([contact(device,"up",[p[0]-240.,p[1]])]));
        pump(180); assert!(!row.is_open(),"canceled contact closes actions");
        d.w.dispatch(UiAction::Invoke {command:CommandId::QuickMask}); pump(100);
        let row=d.named("art-layer-0").parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
        layer_swipe_view_matches(&row,state(&d.w).layers.iter().find(|layer|layer.id==0).unwrap());
        assert!(!row.lock_button().is_visible()); assert!(!row.duplicate_button().is_visible());
        assert!(row.delete_button().is_sensitive());
        d.w.dispatch(UiAction::Invoke {command:CommandId::QuickMask});
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::New {group:true,clipped:false}});
        let group=state(&d.w).layer_properties.layer.unwrap();
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::New {group:false,clipped:false}});
        let child=state(&d.w).layer_properties.layer.unwrap();
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::Lock {id:group,value:true}}); pump(100);
        let row=d.named(&format!("art-layer-{child}")).parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
        layer_swipe_view_matches(&row,state(&d.w).layers.iter().find(|layer|layer.id==child).unwrap());
        assert!(!row.lock_button().is_sensitive()); assert!(!row.duplicate_button().is_sensitive());
        assert!(!row.delete_button().is_sensitive());
        let p=d.point(&d.named(&format!("art-layer-{child}")));
        d.input.perform(serde_json::json!([contact(device,"down",p),contact(device,"move",[p[0]-240.,p[1]]),contact(device,"up",[p[0]-240.,p[1]])]));
        assert!(!row.is_open(),"locked parent denies unavailable actions");
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::Lock {id:group,value:false}});
        d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::Delete {id:group}});
        d.click_name(&opener); wait_for_drawer_close(&d.w);
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_sketch_layer_properties_transition"]
fn native_sketch_layer_properties_transition() {
    let mut d=Driver::new("art.capycanvas.SketchPropertiesTransition");
    for theme in [Theme::Light,Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme {theme:Some(theme)});
        d.w.dispatch(UiAction::RestoreWorkspace {workspace:Box::new(WorkspaceState {
            layout:WorkspacePreset::Painter.layout(Platform::Gtk),..WorkspaceState::default()
        })});
        for _ in 0..32 {d.w.dispatch(UiAction::Layer {action:layer_ui::LayerAction::New {group:false,clipped:false}});}
        let opener=d.header_tool(ToolbarControl::Panel {panel:Panel::Layers});
        d.click_name(&opener);
        let stack=d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
        layer_page_settled(&d,&stack,"transition Layers ready");
        let native=d.named("tool-drawer");
        let footer=d.named("layer-properties-disclosure");
        let start=native.height();
        let overhead=start-stack.height();
        let frames=Rc::new(RefCell::new(Vec::new()));
        let clock=d.w.surface.frame_clock().unwrap();
        let capture=clock.connect_after_paint(glib::clone!(#[strong] frames,#[weak] stack,#[weak] native,move |_| {
            let slide=stack.visible_child().and_then(|child|child.compute_bounds(&stack)).map(|b|1.-b.y().abs()/stack.height().max(1) as f32);
            frames.borrow_mut().push((native.height(),stack.height(),stack.is_transition_running(),slide));
        }));
        d.click(&footer);
        layer_page_settled(&d,&stack,"synchronized Properties opening");
        let end=native.height();
        clock.disconnect(capture);
        assert!(end<start);
        let frames=frames.borrow();
        assert!(frames.iter().any(|&(h,_,running,_)|running && h> end+2 && h<start-2),"native intermediate height during slide");
        for &(h,s,running,slide) in frames.iter() {
            assert!((h-s-overhead).abs()<=2,"stack/container synchronized: {h}/{s}/{overhead}");
            if running && h>end+2 && h<start-2 {
                let height_progress=(h-start) as f32/(end-start) as f32;
                let slide=slide.unwrap();
                let tolerance=2./(start-end).abs() as f32+2./s.max(1) as f32;
                assert!((height_progress-slide).abs()<=tolerance,"height/slide progress synchronized: {height_progress}/{slide}, tolerance {tolerance}");
            }
        }
        drop(frames);
        let frames=Rc::new(RefCell::new(Vec::new()));
        let capture=clock.connect_after_paint(glib::clone!(#[strong] frames,#[weak] stack,#[weak] native,move |_| {
            let slide=stack.visible_child().and_then(|child|child.compute_bounds(&stack)).map(|b|1.-b.y().abs()/stack.height().max(1) as f32);
            frames.borrow_mut().push((native.height(),stack.height(),stack.is_transition_running(),slide));
        }));
        d.click(&footer); layer_page_settled(&d,&stack,"synchronized Layers return");
        clock.disconnect(capture);
        assert!((native.height()-start).abs()<=1);
        assert!(frames.borrow().iter().any(|&(h,_,running,_)|running && h>end+2 && h<start-2));
        for &(h,s,running,slide) in frames.borrow().iter() {
            assert!((h-s-overhead).abs()<=2);
            if running && h>end+2 && h<start-2 {
                let height_progress=(h-end) as f32/(start-end) as f32;
                let slide=slide.unwrap();
                let tolerance=2./(start-end).abs() as f32+2./s.max(1) as f32;
                assert!((height_progress-slide).abs()<=tolerance,"return height/slide progress synchronized: {height_progress}/{slide}, tolerance {tolerance}");
            }
        }
        d.click(&footer); layer_page_settled(&d,&stack,"Properties before rapid reversal");
        d.input.settle_ms=0;
        for (index,open) in [false,true,false].into_iter().enumerate() {
            if index>0 {assert!(stack.is_transition_running(),"reverse during active native transition");}
            let p=d.point(&footer);
            let picked=d.w.window.pick(p[0] as f64,p[1] as f64,gtk::PickFlags::DEFAULT).unwrap();
            assert!(picked==footer || picked.is_ancestor(&footer),"rapid footer native hit at {p:?}: {}",picked.widget_name());
            d.input.perform(serde_json::json!([contact("touch","down",p),contact("touch","up",p)]));
            until(||state(&d.w).customization.drawer.unwrap().layer_properties==Some(open),"rapid native touch adopted");
            pump(100);
        }
        d.input.settle_ms=180;
        layer_page_settled(&d,&stack,"rapid reversal returns Layers");
        assert_eq!(stack.visible_child_name().as_deref(),Some("layers"));
        assert!((native.height()-start).abs()<=1);
        let settings=gtk::Settings::for_display(&gtk::prelude::WidgetExt::display(&d.w.window));
        let animations=settings.is_gtk_enable_animations();
        settings.set_gtk_enable_animations(false);
        d.click(&footer); layer_page_settled(&d,&stack,"animations disabled Properties");
        assert!(!stack.is_transition_running()); assert!((native.height()-end).abs()<=1);
        d.click(&footer); layer_page_settled(&d,&stack,"animations disabled Layers");
        assert!((native.height()-start).abs()<=1);
        settings.set_gtk_enable_animations(animations);
        d.click_name(&opener); wait_for_drawer_close(&d.w);
    }
    d.finish();
}

#[test]
#[ignore = "isolated native-input.js --native-test=native_layer_swipe_mouse"]
fn native_layer_swipe_mouse() { layer_swipes(&["mouse"]); }

#[test]
#[ignore = "isolated native-input.js --native-test=native_layer_multitouch_actions --tablet"]
fn native_layer_multitouch_actions() {
    let mut d = Driver::new("art.capycanvas.LayerMultitouchActions");
    d.w.dispatch(UiAction::SetColor { rgba: [0., 1., 0., 1.] });
    d.w.dispatch(UiAction::Invoke { command: CommandId::SelectAll });
    d.w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::FillSelection });
    pump(300);
    d.w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
    for theme in [Theme::Light, Theme::Dark] {
        for simple in [false, true] {
            d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            if simple {
                d.w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(WorkspaceState {
                    layout: WorkspacePreset::Painter.layout(Platform::Gtk), ..WorkspaceState::default()
                }) });
                pump(250);
                let opener = d.header_tool(ToolbarControl::Panel { panel: Panel::Layers });
                d.click_name(&opener);
                let stack = d.named("layer-properties-stack").downcast::<gtk::Stack>().unwrap();
                layer_page_settled(&d, &stack, "multitouch simple Layers ready");
            } else {
                d.w.dispatch(UiAction::Invoke { command: CommandId::ResetLayout });
                pump(300);
            }
            let row = d.named("art-layer-1");
            let p = d.point(&row);
            let pair = [[p[0] - 25., p[1]], [p[0] + 25., p[1]]];
            let down = |d: &mut Driver| d.input.perform(serde_json::json!([
                {"touch":"down","slot":0,"point":pair[0]}, {"touch":"down","slot":1,"point":pair[1]}
            ]));
            let up = |d: &mut Driver| d.input.perform(serde_json::json!([
                {"touch":"up","slot":0}, {"touch":"up","slot":1}
            ]));
            down(&mut d); up(&mut d);
            until(|| state(&d.w).layer_tools.opacity_popup == Some(1), "two fingers open opacity");
            let editor = d.named("layer-gesture-opacity");
            assert!(editor.is_mapped(), "{theme:?}/{simple}: opacity editor is native");
            d.number(&editor, "42");
            assert!((state(&d.w).layers.iter().find(|l| l.id == 1).unwrap().opacity - 0.42).abs() < 0.001);
            d.input.key(0xff1b);
            d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
            pump(150);
            assert_eq!(state(&d.w).layers.iter().find(|l| l.id == 1).unwrap().opacity, 1.);
            down(&mut d);
            pump(700);
            until(|| ui_session(&d.w).engine().document().working.selection.is_some(), "held fingers select layer alpha");
            up(&mut d);
            assert!(state(&d.w).layer_tools.opacity_popup.is_none(), "hold cannot open opacity on release");
            d.w.dispatch(UiAction::Invoke { command: CommandId::Deselect });
            let selected = state(&d.w).layers.iter().find(|l| l.id == 1).unwrap().selected;
            for expected in [!selected, selected] {
                down(&mut d);
                d.input.perform(serde_json::json!([
                    {"touch":"move","slot":0,"point":[pair[0][0] + 60., pair[0][1]]},
                    {"touch":"move","slot":1,"point":[pair[1][0] + 60., pair[1][1]]}
                ]));
                up(&mut d);
                assert_eq!(state(&d.w).layers.iter().find(|l| l.id == 1).unwrap().selected, expected, "{theme:?}/{simple}: two fingers toggle selection");
                assert!(!state(&d.w).layers.iter().find(|l| l.id == 1).unwrap().alpha_locked);
            }
            d.input.perform(serde_json::json!([
                contact("touch", "down", p), contact("touch", "move", [p[0] + 18., p[1]]),
                contact("touch", "move", [p[0] + 60., p[1]]), contact("touch", "up", [p[0] + 60., p[1]])
            ]));
            assert!(state(&d.w).layers.iter().find(|l| l.id == 1).unwrap().alpha_locked, "one finger retains Alpha Lock");
            d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
            for _ in 0..2 { d.w.dispatch(UiAction::Invoke { command: CommandId::AddLayer }); }
            pump(200);
            let top = state(&d.w).layers.iter().find(|l| l.editing).unwrap().id;
            let a = d.point(&d.named(&format!("art-layer-{top}")));
            let b = d.point(&d.named("art-layer-1"));
            let count = state(&d.w).layers.len();
            d.input.perform(serde_json::json!([
                {"touch":"down","slot":0,"point":a}, {"touch":"down","slot":1,"point":b},
                {"touch":"move","slot":0,"point":[a[0], a[1] + (b[1] - a[1]) * 0.35]},
                {"touch":"move","slot":1,"point":[b[0], b[1] + (a[1] - b[1]) * 0.35]},
                {"touch":"up","slot":0}, {"touch":"up","slot":1}
            ]));
            until(|| state(&d.w).layers.len() == count - 2, "pinch merges inclusive three-row range");
            d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
            pump(150);
            assert_eq!(state(&d.w).layers.len(), count, "one Undo restores range");
            for _ in 0..2 { d.w.dispatch(UiAction::Invoke { command: CommandId::Undo }); }
            pump(150);
            d.w.dispatch(UiAction::Layer { action: layer_ui::LayerAction::Select { id: 1, mask: false } });
        }
    }
    d.finish();
}

fn layer_swipes(devices: &[&str]) {
    let mut d = Driver::new("art.capycanvas.LayerSwipeAlphaLock");
    d.w.dispatch(UiAction::Invoke { command: CommandId::ResetLayout });
    pump(300);
    let alpha = || state(&d.w).layers.iter().find(|l| l.id == 1).unwrap().alpha_locked;
    for theme in [Theme::Light, Theme::Dark] {
        d.w.dispatch(UiAction::SetTheme { theme: Some(theme) });
        pump(200);
        for &device in devices {
            for (distance, cancel, expected) in [(18., false, false), (60., true, false), (60., false, device != "mouse")] {
                let row = d.named("art-layer-1");
                let p = d.point(&row);
                let end = [p[0]+distance, p[1]];
                d.input.perform(serde_json::json!([
                    contact(device, "down", p), contact(device, "move", [p[0]+12.,p[1]]),
                    contact(device, "move", end)
                ]));
                if cancel {
                    let controllers = row.parent().unwrap().observe_controllers();
                    for i in 0..controllers.n_items() {
                        if let Some(gesture) = controllers.item(i).and_downcast::<gtk::GestureDrag>() {
                            gesture.set_state(gtk::EventSequenceState::Denied);
                        }
                    }
                }
                d.input.perform(serde_json::json!([contact(device, "up", end)]));
                assert_eq!(alpha(), expected, "{device}, {distance}, cancelled={cancel}");
                if expected {
                    d.w.dispatch(UiAction::Invoke { command: CommandId::Undo });
                    assert!(!alpha());
                    d.w.dispatch(UiAction::Invoke { command: CommandId::Redo });
                    assert!(alpha());
                    let row = d.named("art-layer-1");
                    let p = d.point(&row);
                    d.input.perform(serde_json::json!([contact(device,"down",p),
                        contact(device,"move",[p[0]+18.,p[1]]), contact(device,"move",[p[0]+60.,p[1]]),
                        contact(device,"up",[p[0]+60.,p[1]])]));
                    assert!(!alpha());
                }
            }
            if device == "mouse" { continue; }
            for distance in [-60., 90.] {
                let row = d.named("art-layer-1");
                let swipe = row.parent().unwrap().downcast::<crate::swipe_row::SwipeRow>().unwrap();
                let p = if swipe.is_open() {d.point(swipe.upcast_ref())} else {d.point(&row)};
                d.input.perform(serde_json::json!([contact(device,"down",p),
                    contact(device,"move",[p[0]+distance/3.,p[1]]), contact(device,"move",[p[0]+distance,p[1]]),
                    contact(device,"up",[p[0]+distance,p[1]])]));
                assert_eq!(swipe.is_open(), distance < 0.);
                assert!(!alpha(), "closing Delete does not toggle alpha lock");
            }
        }
    }
    d.w.window.close();
}

#[test]
#[ignore = "private Wayland display; gtk-raster.sh"]
fn native_caption_toolbar_constructor_before_realize() {
    let app = native_test_app("art.capycanvas.CaptionConstructor");
    let w = Workspace::new_localized(&app, Localizer::shared(UiLanguage::Japanese));
    assert!(w.gpu.borrow().is_none());
    let layout = DockLayout::default();
    let resolved = w.resolved();
    for _ in 0..2 {
        assert!(!w.customization.reconcile_toolbars(&w, &layout, &resolved));
        assert!(w.toolbar.first_child().is_none());
    }
    w.window.present();
    let deadline = Instant::now() + Duration::from_secs(30);
    while w.gpu.borrow().is_none() {
        assert!(Instant::now() < deadline, "canvas startup");
        pump(10);
    }
    wait_workspaces(&w);
    if let Ok(theme) = std::env::var("CAPY_NATIVE_TEST_THEME") {
        w.dispatch(UiAction::SetTheme { theme: Some(match theme.as_str() { "light" => Theme::Light, "dark" => Theme::Dark, _ => panic!("unknown theme") }) });
    }
    let state = ui_session(&w).state().clone();
    assert!(w.customization.reconcile_toolbars(&w, &state.workspace.layout, &w.resolved()));
    let view = ui_session(&w).panel_view(Panel::Toolbar).unwrap();
    assert!(!view.tiles.is_empty());
    for tile in &view.tiles {
        let button = find_named(w.toolbar.upcast_ref(), &format!("tile-{}", tile.id)).unwrap().downcast::<gtk::Button>().unwrap();
        let tooltip = if matches!(tile.choice.control, ToolbarControl::ColorPicker | ToolbarControl::Command { command: CommandId::Eyedropper }) {
            ui_session(&w).color_picker_button_label(tile.choice.control).to_string()
        } else { tile.tooltip.clone() };
        assert_eq!(button.tooltip_text().as_deref(), Some(tooltip.as_str()));
    }
    let control = ToolbarControl::Command { command: CommandId::MergeDown };
    for action in [
        CustomizationAction::InsertTools { panel: Panel::Toolbar, before: None },
        CustomizationAction::PickerSelect { control, selected: true },
        CustomizationAction::ConfirmTools,
        CustomizationAction::SetTileStyle { panel: Panel::Toolbar, style: TileStyle::Labeled },
    ] { w.dispatch(UiAction::Customize { action }); }
    let view = ui_session(&w).panel_view(Panel::Toolbar).unwrap();
    let tile = view.tiles.iter().find(|tile| tile.choice.control == control).unwrap();
    let name = format!("tile-{}", tile.id);
    let button = find_named(w.toolbar.upcast_ref(), &name).unwrap().downcast::<gtk::Button>().unwrap();
    let caption = button.child().and_downcast::<gtk::Box>().unwrap().last_child().and_downcast::<gtk::Label>().unwrap();
    let before = caption.text();
    w.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "exposure".into() } });
    let expected = ui_session(&w).command(CommandId::MergeDown).label;
    assert_ne!(before.as_str(), expected.as_ref());
    assert_eq!(caption.text().as_str(), expected.as_ref());
    assert_eq!(find_named(w.toolbar.upcast_ref(), &name).unwrap(), button.clone().upcast::<gtk::Widget>());
    for size in [8., 17.] {
        w.dispatch(UiAction::SetBrushSize { value: size });
        w.dispatch(UiAction::SetZoom { zoom: size / 8. });
        assert_eq!(caption.text().as_str(), expected.as_ref());
        assert_eq!(find_named(w.toolbar.upcast_ref(), &name).unwrap(), button.clone().upcast::<gtk::Widget>());
    }
    w.window.destroy();
    pump(20);
}
