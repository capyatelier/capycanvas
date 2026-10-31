use crate::*;
use crate::panel_measurements::PanelMeasurementCache;

fn context()->PanelMetricContext {
    PanelMetricContext {layout_grid:0.,generation:1,scale:2.,text_scale:1.,style:"native-tab-fixture".into()}
}

fn fixture(panels:&[Panel])->DockLayout {
    let mut layout=DockLayout::editor_default();
    let tabs=|id,panels:Vec<Panel>|DockNode::Tabs {id,active:panels[0],panels,tab_style:TabStyle::Automatic};
    layout.bands=vec![DockBand {id:90,edge:Edge::Left,extent:200.,alignment:None,root:DockNode::Split {
        id:91,axis:Axis::Vertical,fraction:0.5,first:Box::new(tabs(100,panels.to_vec())),
        second:Box::new(tabs(101,vec![Panel::Navigator])),
    }}];
    layout.floating.clear();layout.collapsed.clear();layout.column_stacks.clear();
    layout.fit_tab_groups=vec![100];layout.fit_height_groups=vec![100];
    let mut value=serde_json::to_value(layout).unwrap();value["next_id"]=200.into();
    serde_json::from_value(value).unwrap()
}

fn metrics(panel:Panel,russian:bool)->[f32;2] {
    let index=[Panel::Brushes,Panel::Stats,Panel::Sizes,Panel::Color,Panel::Palettes,Panel::ToolSettings]
        .iter().position(|p|*p==panel).unwrap_or(0);
    [if russian {[198.,140.,146.,92.,120.,160.][index]}else{[92.,115.,108.,78.,98.,70.][index]},36.]
}

fn cache(layout:&DockLayout)->PanelMeasurementCache {
    let mut cache=PanelMeasurementCache::default();
    cache.identities=layout.panels.iter().map(|p|PanelMeasurementIdentity {
        panel:p.id,tab:format!("{:?}",p.id),body:format!("body-{:?}",p.id),cacheable:true,
    }).collect();cache
}

fn render_report(layout:&DockLayout,cache:&PanelMeasurementCache,viewport:[f32;2],russian:bool)->PanelMeasurementBatch {
    let resolved=LayoutResolveInput::new(layout,None).with_cache(cache.clone()).resolve(viewport);
    let context=cache.context.clone().unwrap_or_else(context);
    let reports=resolved.measurement_requests.iter().map(|request| {
        let widths=metrics(request.identity.panel,russian);
        PanelMeasurementReport {request:request.clone(),context:context.clone(),tab_widths:Some(widths),
            body:Some(PanelBodyMeasurement {actual_width:request.width,
                content_height:36.*(560./request.width.max(1.)).ceil()+24.,
                scroll:Some(PanelScrollMeasurement {fixed_height:24.,unit_height:36.})})}
    }).collect();
    PanelMeasurementBatch {context,reports}
}

#[test]
fn native_tab_reports_reach_a_fixed_point_after_one_round() {
    let panels=[Panel::Brushes,Panel::Stats,Panel::Sizes,Panel::Color,Panel::Palettes,Panel::ToolSettings];
    for russian in [false,true] {for count in [3,5,6] {for shift in 0..count {for width in [640.,960.,1200.] {
        let mut order=panels[..count].to_vec();order.rotate_left(shift);
        let mut layout=fixture(&order);let viewport=[width,720.];let mut cache=cache(&layout);
        cache=cache.accept(render_report(&layout,&cache,viewport,russian)).unwrap();
        let requests=LayoutResolveInput::new(&layout,None).with_cache(cache.clone()).resolve(viewport).measurement_requests;
        cache=cache.accept(PanelMeasurementBatch {context:context(),reports:requests.into_iter().map(|request| {
            PanelMeasurementReport {tab_widths:Some([metrics(request.identity.panel,russian)[0],36.]),request,context:context(),body:None}
        }).collect()}).unwrap();
        layout.measurements=cache.project(&LayoutResolveInput::new(&layout,None).with_cache(cache.clone()).resolve(viewport).measurement_requests);
        layout.resize(90,[260.,300.],viewport).unwrap();
        assert!(!layout.fit_tab_groups.contains(&100));
        layout.move_panel(viewport,order[0],DockTarget::Tab {group:100,index:Some(count-1)}).unwrap();
        assert!(layout.fit_tab_groups.contains(&100));
        let report=render_report(&layout,&cache,viewport,russian);
        cache=cache.accept(report.clone()).unwrap();
        let first=LayoutResolveInput::new(&layout,None).with_cache(cache.clone()).resolve(viewport);
        let repeated=cache.accept(report).unwrap();
        assert_eq!(serde_json::to_vec(&first).unwrap(),serde_json::to_vec(&LayoutResolveInput::new(&layout,None).with_cache(repeated).resolve(viewport)).unwrap());
        cache=cache.accept(render_report(&layout,&cache,viewport,russian)).unwrap();
        let second=LayoutResolveInput::new(&layout,None).with_cache(cache).resolve(viewport);
        assert_eq!(serde_json::to_vec(&first).unwrap(),serde_json::to_vec(&second).unwrap(),
            "russian={russian}, tabs={order:?}, viewport={viewport:?}");
    }}}}
}

fn widths(layout:&ResolvedLayout)->Vec<u8> {
    serde_json::to_vec(&(
        (layout.work_area.x,layout.work_area.width),
        layout.groups.iter().map(|g|(g.id,g.bounds.x,g.bounds.width,&g.tab_presentations)).collect::<Vec<_>>(),
        layout.dividers.iter().filter(|d|d.axis==Axis::Horizontal).map(|d|(d.id,d.bounds.x,d.bounds.width)).collect::<Vec<_>>(),
        layout.collapsed.iter().map(|c|(c.id,c.bounds.x,c.bounds.width)).collect::<Vec<_>>(),
        layout.measurement_requests.iter().map(|request|(request.identity.panel,request.width)).collect::<Vec<_>>(),
    )).unwrap()
}

#[test]
fn body_heights_never_change_widths_in_nested_toolbars_or_presets() {
    let mut layouts=WorkspacePreset::ALL.into_iter().flat_map(|preset|Platform::ALL.into_iter().map(move|platform| {
        let mut layout=preset.layout(platform);layout.open_default_columns(platform);layout
    })).collect::<Vec<_>>();
    for fraction in [0.2,0.5,0.8] {
        let mut layout=fixture(&[Panel::Brushes,Panel::Stats,Panel::Sizes]);
        if let DockNode::Split {first,fraction:f,..}=&mut layout.bands[0].root {
            *f=fraction;
            *first=Box::new(DockNode::Split {id:102,axis:Axis::Horizontal,fraction:0.5,
                first:Box::new(DockNode::Tabs {id:103,panels:vec![Panel::Toolbar],active:Panel::Toolbar,tab_style:TabStyle::Automatic}),
                second:first.clone()});
        }
        layouts.push(layout);
    }
    for layout in layouts {for viewport in [[640.,480.],[960.,720.],[1600.,1000.]] {for russian in [false,true] {for grid in [0.,1.] {
        let mut cache=cache(&layout).with_context(PanelMetricContext {layout_grid:grid,..context()}).unwrap();
        cache=cache.accept(render_report(&layout,&cache,viewport,russian)).unwrap();
        let first=LayoutResolveInput::new(&layout,None).with_cache(cache.clone()).resolve(viewport);
        for height in [64.,300.,4000.] {
            let mut report=render_report(&layout,&cache,viewport,russian);
            for entry in &mut report.reports {entry.body.as_mut().unwrap().content_height=height;}
            cache=cache.accept(report).unwrap();
            let result=LayoutResolveInput::new(&layout,None).with_cache(cache.clone()).resolve(viewport);
            assert_eq!(widths(&first),widths(&result),"height={height}, viewport={viewport:?}, russian={russian}, bands={:?}",layout.bands);
            cache=cache.accept(render_report(&layout,&cache,viewport,russian)).unwrap();
            let first=LayoutResolveInput::new(&layout,None).with_cache(cache.clone()).resolve(viewport);
            cache=cache.accept(render_report(&layout,&cache,viewport,russian)).unwrap();
            assert_eq!(serde_json::to_vec(&first).unwrap(),serde_json::to_vec(&LayoutResolveInput::new(&layout,None).with_cache(cache.clone()).resolve(viewport)).unwrap());
        }
    }}}}
}
