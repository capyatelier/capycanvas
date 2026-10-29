use super::*;

fn lower_contains(rect: &gtk::graphene::Rect, corners: [i8; 2], x: f64, y: f64) -> bool {
    let (left, right, top, bottom) = (rect.x() as f64, (rect.x() + rect.width()) as f64,
        rect.y() as f64, (rect.y() + rect.height()) as f64);
    if y < top || y >= bottom { return false; }
    let fourth = |v: f64| v * v * v * v;
    if x >= left && x < right {
        for (radius, distance) in [(corners[0], x - left), (corners[1], right - x)] {
            let r = f64::from(radius);
            if r > 0. && distance < r && bottom - y < r
                && fourth((r - distance) / r) + fourth((y - bottom + r) / r) > 1. {
                return false;
            }
        }
        return true;
    }
    for (radius, distance) in [(corners[0], left - x), (corners[1], x - right)] {
        let r = -f64::from(radius);
        if r > 0. && distance >= 0. && distance < r && bottom - y < r
            && fourth((r - distance) / r) + fourth((y - bottom + r) / r) >= 1. {
            return true;
        }
    }
    false
}

fn lower_coverage(rect: &gtk::graphene::Rect, corners: [i8; 2], x: usize, y: usize, scale: f64) -> f64 {
    let mut covered = 0;
    for sy in 0..4 { for sx in 0..4 {
        let px = (x as f64 + (sx as f64 + 0.5) / 4.) / scale;
        let py = (y as f64 + (sy as f64 + 0.5) / 4.) / scale;
        covered += usize::from(lower_contains(rect, corners, px, py));
    } }
    covered as f64 / 16.
}

fn lower_boundary(rect: &gtk::graphene::Rect, corners: [i8; 2], x: usize, y: usize, scale: f64) -> bool {
    let (x, y) = ((x as f64 + 0.5) / scale, (y as f64 + 0.5) / scale);
    let center = lower_contains(rect, corners, x, y);
    [-1., 0., 1.].into_iter().any(|dy| [-1., 0., 1.].into_iter()
        .any(|dx| lower_contains(rect, corners, x + dx / scale, (y + dy / scale).min((rect.y() + rect.height()) as f64 - 1e-6)) != center))
}

fn contour_pixels(texture: &gdk::Texture) -> Vec<u8> {
    let mut pixels = vec![0; texture.width() as usize * texture.height() as usize * 4];
    texture.download(&mut pixels, texture.width() as usize * 4);
    pixels
}


#[test]
#[ignore = "isolated Mutter native-input.js --native-test=native_panel_tab_contours_are_continuous"]
fn native_panel_tab_contours_are_continuous() {
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").expect("contour artifacts"));
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.PanelTabContinuity");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let fixture = layer_ui::WorkspaceState::default();
    let group = fixture.layout.panel_group(Panel::Layers).unwrap();
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let input = RemoteInput::new();
    input.ready();
    let mut gaps = Vec::new();let mut coverage_reports=Vec::new();
    for drawer in [false, true] {
        w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(fixture.clone()) });
        if drawer {
            w.dispatch(UiAction::DoubleClickPanelHandle { group, viewport });
            enable_individual_column_panels(&w, group);
            w.dispatch(UiAction::Customize { action: CustomizationAction::ToggleColumnDrawer { group, panel: Panel::Layers } });
        }
        for theme in [Theme::Dark, Theme::Light] {
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            for level in [layer_ui::Transparency::Off, layer_ui::Transparency::Low, layer_ui::Transparency::Medium, layer_ui::Transparency::High] {
                use_transparency(&w, level);
                for style in [TabStyle::Icon, TabStyle::IconName] {
                    w.dispatch(UiAction::Customize { action: CustomizationAction::SetTabStyle { group, style } });
                    for panel in [Panel::Layers, Panel::Adjustments, Panel::Properties] {
                        w.dispatch(UiAction::SelectPanelTab { group, panel });
                        pump(80);
                        let tab = if drawer { named::<gtk::Button>(w.window.upcast_ref(), &format!("column-drawer-tab-{panel:?}")) }
                            else { w.groups.borrow().iter().find(|g| g.id == group).unwrap().tabs.iter().find(|(p, _)| *p == panel).unwrap().1.clone() };
                        let strip = tab.parent().unwrap();
                        until(|| tab.is_mapped() && strip.width() > 0 && strip.height() > 0, "native tab strip allocation");
                        let bounds = tab.compute_bounds(&strip).unwrap();
                        assert_eq!(bounds.y(), 0.);
                        assert_eq!(bounds.height(), strip.height() as f32);
                        let floor = ["tab-strip-tint", "tab-foot-tint"].map(|class| find_css(&strip, class).unwrap().color().alpha())
                            .into_iter().fold(1f32, f32::min) * 255.;
                        let grid=crate::squircle::PixelGrid::for_widget(&strip);
                        assert!(grid.scale.into_iter().all(|scale| scale > 0.), "native tab pixel grid has a physical scale");
                        let scale=grid.scale[0].abs();let phase=grid.offset.map(|v|v.rem_euclid(1.));
                        let snapshot = gtk::Snapshot::new();
                        snapshot.translate(&gtk::graphene::Point::new(phase[0],phase[1]));
                        snapshot.scale(scale, scale);
                        gtk::WidgetPaintable::new(Some(&strip)).snapshot(&snapshot, strip.width() as f64, strip.height() as f64);
                        let node = snapshot.to_node().unwrap();
                        if level.enabled() {
                            let mut regions = Vec::new();
                            crate::glass::collect(&node, &w.palette.get().unwrap().glass.surfaces(), &mut regions);
                            let expected=[phase[0],phase[1],strip.width() as f32*scale,strip.height() as f32*scale];
                            assert!(regions.iter().any(|region|region.bounds.into_iter().zip(expected).all(|(a,b)|(a-b).abs()<0.01)), "tab strip retains its complete blurred backdrop");
                        }
                        let node = crate::squircle::converted(&node, 1.);
                        let texture = w.window.renderer().unwrap().render_texture(&node, Some(&gtk::graphene::Rect::new(0., 0., (strip.width() as f32 * scale+phase[0]).ceil(), (strip.height() as f32 * scale+phase[1]).ceil())));
                        let pixels = contour_pixels(&texture);
                        let interior=|i:usize|{let x=(i%texture.width() as usize)as f32;let y=(i/texture.width() as usize)as f32;x>=phase[0]&&x+1.<=phase[0]+strip.width() as f32*scale&&y>=phase[1]&&y+1.<=phase[1]+strip.height() as f32*scale};
                        let minimum=pixels.as_chunks::<4>().0.iter().enumerate().filter(|(i,_)|interior(*i)).map(|(_,p)|p[3]).min().unwrap_or(255);
                        let gap=pixels.as_chunks::<4>().0.iter().enumerate().find_map(|(i,p)|
                            (interior(i)&&f32::from(p[3])+2.<floor).then_some((i,p[3])));

                        let name = format!("{}-{theme:?}-{level:?}-{style:?}-{panel:?}", if drawer { "drawer" } else { "docked" }).to_lowercase();
                        if gap.is_some() || panel == Panel::Adjustments {
                            texture.save_to_png(output.join(format!("{name}.png"))).unwrap();
                        }
                        coverage_reports.push(serde_json::json!({"case":name,"floor":floor,"tolerance":2,"scale":scale,"phase":phase,"minimum_alpha":minimum,"first_gap":gap}));
                        if let Some((i,alpha))=gap{gaps.push(format!("{name}: alpha {alpha} below {floor:.1} at ({}, {})",i%texture.width() as usize,i/texture.width() as usize));}
                    }
                }
            }
        }
    }
    input.finish();
    std::fs::write(output.join("continuity-reference-edges.json"),serde_json::to_vec_pretty(&coverage_reports).unwrap()).unwrap();
    assert!(gaps.is_empty(), "tab contours expose the backdrop:\n{}", gaps.join("\n"));
}

#[test]
#[ignore = "isolated Mutter native-input.js --native-test=native_panel_tab_lower_contours"]
fn native_panel_tab_lower_contours() {
    let output = std::path::PathBuf::from(std::env::var("LAYER_TEST_ARTIFACTS").expect("contour artifacts"));
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.PanelTabContours");
    let w = fixture_workspace(&app);
    w.window.maximize();
    w.window.present();
    pump(1600);
    let viewport = [w.surface.width() as f32, w.surface.height() as f32];
    let panels = [Panel::Layers, Panel::Adjustments, Panel::Properties, Panel::Sizes];
    let mut fixture = layer_ui::WorkspaceState::default();
    let group = fixture.layout.panel_group(Panel::Layers).unwrap();
    fixture.layout.move_panel(viewport, Panel::Sizes, DockTarget::Tab { group, index: None }).unwrap();
    let mut input = RemoteInput::new().settle_ms(30).timeout_secs(10);
    input.ready();
    let mut checked = 0;
    let mut foot_pixels = 0;
    let mut transparent_body_delta = 0;
    for drawer in [false, true] {
        w.dispatch(UiAction::RestoreWorkspace { workspace: Box::new(fixture.clone()) });
        w.dispatch(UiAction::Customize { action: CustomizationAction::SetTabStyle { group, style: TabStyle::Icon } });
        if drawer {
            w.dispatch(UiAction::DoubleClickPanelHandle { group, viewport });
            enable_individual_column_panels(&w, group);
            w.dispatch(UiAction::Customize { action: CustomizationAction::ToggleColumnDrawer { group, panel: Panel::Layers } });
        }
        pump(400);
        let tabs: Vec<gtk::Button> = panels.iter().map(|panel| {
            if drawer { named::<gtk::Button>(w.window.upcast_ref(), &format!("column-drawer-tab-{panel:?}")) }
            else { w.groups.borrow().iter().find(|g| g.id == group).unwrap().tabs.iter()
                .find(|(p, _)| p == panel).unwrap().1.clone() }
        }).collect();
        let mut parent = tabs[0].parent().unwrap();
        let clip = loop {
            if let Ok(clip) = parent.clone().downcast::<gtk::ScrolledWindow>() { break clip; }
            parent = parent.parent().expect("tab scroll viewport");
        };
        let mut header = clip.clone().upcast::<gtk::Widget>();
        while !header.has_css_class("dock-tabs") { header = header.parent().expect("tab header"); }
        for theme in [Theme::Dark, Theme::Light] {
            w.dispatch(UiAction::SetTheme { theme: Some(theme) });
            for level in [layer_ui::Transparency::Off, layer_ui::Transparency::Low, layer_ui::Transparency::Medium, layer_ui::Transparency::High] {
                use_transparency(&w, level);
                for edge in ["wide", "left", "right"] {
                    clip.set_hexpand(edge == "wide");
                    clip.set_size_request(if edge == "wide" { -1 } else { 24 }, -1);
                    clip.set_halign(if edge == "wide" { gtk::Align::Fill } else { gtk::Align::Start });
                    pump(80);
                    for active in 0..tabs.len() {
                        w.dispatch(UiAction::SelectPanelTab { group, panel: panels[active] });
                        pump(40);
                        let body_widget = if drawer {
                            find_named(w.surface.upcast_ref(), &format!("column-drawer-{group}")).unwrap()
                        } else {
                            w.groups.borrow().iter().find(|g| g.id == group).unwrap().stack.clone().upcast::<gtk::Widget>()
                        };
                        let body_texture = icons::capture_widget(&w.window, &body_widget);
                        let body_pixels = contour_pixels(&body_texture);
                        let body_scale = body_widget.scale_factor() as usize;
                        let body_y = if drawer { TAB_BAR_HEIGHT as usize + 3 } else { 3 };
                        let body_bounds = body_widget.compute_bounds(&header).unwrap();
                        if active == 1 && edge == "wide" && matches!(level, layer_ui::Transparency::Off | layer_ui::Transparency::High) {
                            body_texture.save_to_png(output.join(format!("{}-{theme:?}-{level:?}-active-body.png", if drawer { "drawer" } else { "docked" }).to_lowercase())).unwrap();
                        }
                        for hovered in (0..tabs.len()).filter(|i| *i != active) {
                            input.perform(serde_json::json!([{"point": [viewport[0] * 0.5, viewport[1] * 0.7]}]));
                            let strip = tabs[hovered].parent().unwrap();
                            let target = tabs[hovered].compute_bounds(&strip).unwrap();
                            let adjustment = clip.hadjustment();
                            let scroll = match edge {
                                "left" => target.x() as f64 + 3.,
                                "right" => (target.x() + target.width()) as f64 - adjustment.page_size() + 3.,
                                _ => 0.,
                            };
                            adjustment.set_value(scroll.clamp(adjustment.lower(), (adjustment.upper() - adjustment.page_size()).max(adjustment.lower())));
                            pump(40);
                            let clip_bounds = clip.compute_bounds(&header).unwrap();
                            let tab = tabs[hovered].compute_bounds(&header).unwrap();
                            let active_tab = tabs[active].compute_bounds(&header).unwrap();
                            let visible = tab.intersection(&clip_bounds).expect("hover tab intersects viewport");
                            let point = screen_point(header.upcast_ref(), &w.window,
                                [(visible.x() + visible.width() * 0.5) / header.width() as f32,
                                 (tab.y() + tab.height() * 0.5) / header.height() as f32]);
                            let _ = icons::capture_widget(&w.window, &header);
                            until(|| w.chrome_released.get() == w.chrome.revision(), "contour baseline chrome published");
                            let baseline = icons::capture_widget(&w.window, &header);
                            input.perform(serde_json::json!([{"point": point}]));
                            until(|| tabs[hovered].state_flags().contains(gtk::StateFlags::PRELIGHT), "contour tab receives native hover");
                            let actual = icons::capture_widget(&w.window, &header);
                            assert_eq!((baseline.width(), baseline.height()), (actual.width(), actual.height()));
                            let name = format!("{}-{theme:?}-{level:?}-{edge}-a{active}-h{hovered}", if drawer { "drawer" } else { "docked" }).to_lowercase();
                            let (before, after) = (contour_pixels(&baseline), contour_pixels(&actual));
                            let scale = header.scale_factor() as f64;
                            let stride = actual.width() as usize * 4;
                            let sx = ((visible.x() + visible.width() * 0.5) as f64 * scale).floor() as usize;
                            let sy = ((tab.y() + tab.height() - 7.5) as f64 * scale).floor() as usize;
                            let sample = sy * stride + sx * 4;
                            let delta: [i16; 4] = std::array::from_fn(|c| after[sample + c] as i16 - before[sample + c] as i16);
                            if !delta[..3].iter().any(|v| v.abs() >= 4) {
                                baseline.save_to_png(output.join(format!("{name}-before.png"))).unwrap();
                                actual.save_to_png(output.join(format!("{name}-hover.png"))).unwrap();
                            }
                            assert!(delta[..3].iter().any(|v| v.abs() >= 4), "{name}: visible native hover fill: {delta:?}");
                            let corners = [if hovered == 0 { 0 } else if hovered - 1 == active { 6 } else { -6 },
                                if hovered + 1 == tabs.len() { 0 } else if hovered + 1 == active { 6 } else { -6 }];
                            let mut mismatches = Vec::new();
                            let y0 = ((tab.y() + tab.height() - 8.) as f64 * scale).floor().max(0.) as usize;
                            let y1 = ((tab.y() + tab.height()) as f64 * scale).ceil().min(actual.height() as f64) as usize;
                            for y in y0..y1 { for x in 0..actual.width() as usize {
                                let in_clip = clip_bounds.contains_point(&gtk::graphene::Point::new(((x as f64 + 0.5) / scale) as f32, ((y as f64 + 0.5) / scale) as f32));
                                let hover = if in_clip { lower_coverage(&tab, corners, x, y, scale) } else { 0. };
                                let foot = lower_coverage(&active_tab, [-6, -6], x, y, scale);
                                let coverage = hover * (1. - foot);
                                let hover_boundary = lower_boundary(&tab, corners, x, y, scale);
                                let foot_boundary = lower_boundary(&active_tab, [-6, -6], x, y, scale);
                                let i = y * stride + x * 4;
                                if in_clip && foot == 1. && ((x as f64 + 0.5) / scale < active_tab.x() as f64
                                    || (x as f64 + 0.5) / scale > (active_tab.x() + active_tab.width()) as f64) {
                                    foot_pixels += 1;
                                    if !foot_boundary {
                                        let face_x = (((x as f64 + 0.5) / scale).clamp(active_tab.x() as f64 + 2., (active_tab.x() + active_tab.width()) as f64 - 2.) * scale).floor() as usize;
                                        let face_offset = y * stride + face_x * 4;
                                        for c in 0..4 {
                                            if before[i + c].abs_diff(before[face_offset + c]) > 2 && mismatches.len() < 24 {
                                                mismatches.push(serde_json::json!({"kind":"active foot matches face","pixel":[x,y],"channel":c,"observed":before[i+c],"expected":before[face_offset+c]}));
                                            }
                                        }
                                        let body_x = (((x as f64 + 0.5) / scale - body_bounds.x() as f64)
                                            .clamp(18., (body_widget.width() - 18).max(18) as f64) * body_scale as f64).floor() as usize;
                                        let body_offset = body_y * body_scale * body_texture.width() as usize * 4 + body_x * 4;
                                        let straight = |pixels: &[u8], offset: usize, c: usize| (f64::from(pixels[offset + c]) * 255. / f64::from(pixels[offset + 3].max(1))).round() as u8;
                                        for c in 0..3 {
                                            let (foot_rgb, body_rgb) = (straight(&before, i, c), straight(&body_pixels, body_offset, c));
                                            if level != layer_ui::Transparency::Off { transparent_body_delta = transparent_body_delta.max(foot_rgb.abs_diff(body_rgb)); }
                                            if level == layer_ui::Transparency::Off && foot_rgb.abs_diff(body_rgb) > 2 && mismatches.len() < 24 {
                                                mismatches.push(serde_json::json!({"kind":"active foot matches body","pixel":[x,y],"channel":c,"observed":foot_rgb,"expected":body_rgb}));
                                            }
                                        }
                                    }
                                }
                                for c in 0..4 {
                                    let observed = after[i + c] as f64 - before[i + c] as f64;
                                    let expected = f64::from(delta[c]) * coverage;
                                    let tolerance = if foot == 1. && !foot_boundary { 1. }
                                        else if hover_boundary || foot_boundary { 5. } else { 2. };
                                    if (observed - expected).abs() > tolerance
                                        && mismatches.len() < 24 { mismatches.push(serde_json::json!({"pixel":[x,y],"channel":c,"observed":observed,"expected":expected,"hover":hover,"active":foot})); }
                                }
                            } }
                            if !mismatches.is_empty() || ((active == 1 && [0, 2, 3].contains(&hovered)) || (active == 0 && hovered == 2)) {
                                baseline.save_to_png(output.join(format!("{name}-before.png"))).unwrap();
                                actual.save_to_png(output.join(format!("{name}-hover.png"))).unwrap();
                                if !mismatches.is_empty() { body_texture.save_to_png(output.join(format!("{name}-body.png"))).unwrap(); }
                            }
                            assert!(mismatches.is_empty(), "{name}: full lower contour {corners:?} shared={:?} selected={:?} mismatches={mismatches:?}", w.surface.imp().layout.borrow().tab_presentation(panels[hovered]).hover_bottom, tabs.iter().map(|tab| tab.has_css_class("selected-tool")).collect::<Vec<_>>());
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(foot_pixels > 0, "active-foot body shade coverage");
    std::fs::write(output.join("coverage.json"), serde_json::json!({"cases":checked,"active_foot_pixels":foot_pixels,"transparent_body_rgb_delta_max_diagnostic":transparent_body_delta,"tabs":4,"scale":w.window.scale_factor(),"themes":2,"transparencies":4,"hosts":["docked","drawer"],"clips":["wide","left","right"]}).to_string()).unwrap();
    input.finish();
}
