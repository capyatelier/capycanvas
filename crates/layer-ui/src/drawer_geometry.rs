use crate::*;
use serde::Deserialize;
use serde_json::{Value,json};

#[derive(Deserialize)]
struct Geometry {
    anchor:DrawerAnchor,
    columns:Vec<Vec<Panel>>,
    tabs:Option<DrawerTabs>,
    compact:bool,
    layer_properties:Option<bool>,
}
impl Geometry {
    fn drawer(self)->ContentDrawer {
        ContentDrawer {anchor:self.anchor,columns:self.columns,tabs:self.tabs,compact:self.compact,
            layer_properties:self.layer_properties,tool_set:None,dismissal:DrawerDismissal::OutsideContact}
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Layer {
    measurement:PanelScrollMeasurement,
    rows:usize,
    properties_height:f32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    model:Geometry,
    column_drawers:Vec<Geometry>,
    tiles:Vec<DrawerTileMeasurement>,
    heights:Vec<f32>,
    toolbars:bool,
    layer:Option<Layer>,
}

impl DockLayout {
    pub fn drawer_toolbar(&self,panel:Panel,width:f32)->Result<Value,String> {
        if !width.is_finite()||width<=0. {return Err("Invalid drawer toolbar size".into());}
        let config=self.panel(panel)?;
        let geometry=toolbar_tile_layout(width,toolbar_content_height(width,config.tiles(),config.tile_style),
            Axis::Vertical,config.tiles(),false,config.tile_style);
        let content_height=geometry.tiles.iter().map(|bounds|bounds.y+bounds.height+4.).fold(0.,f32::max);
        let mut value=json!(geometry);value["content_height"]=json!(content_height);Ok(value)
    }
}

impl LayoutResolveInput<'_> {
    pub fn drawer_geometry(&self,viewport:[f32;2],source:&str)->Result<Value,String> {
        let request:Request=serde_json::from_str(source).map_err(|error|error.to_string())?;
        if !viewport.into_iter().all(|value|value.is_finite()&&value>0.)
            ||request.model.columns.is_empty()||request.model.columns.iter().any(Vec::is_empty)
            ||request.heights.len()!=request.model.columns.len()
            ||request.heights.iter().any(|height|!height.is_finite()||*height<0.) {
            return Err("Invalid drawer measurements".into());
        }
        let drawer=request.model.drawer();
        let layout=self.measured_layout(viewport);
        let mut customization=CustomizationState {column_drawers:request.column_drawers.into_iter().map(Geometry::drawer).collect(),..Default::default()};
        customization.measure_drawer_tiles(&layout,request.tiles)?;
        let placement=customization.drawer_placement(&drawer,&layout,viewport,&request.heights);
        let mut toolbars=Vec::new();
        if request.toolbars&&let Some(placement)=&placement {
            for (panels,column) in drawer.columns.iter().zip(&placement.columns) {
                for &panel in panels {
                    if !layout.panel(panel)?.tiles().is_empty() {
                        toolbars.push(json!({"panel":panel,"width":column.width,"layout":layout.drawer_toolbar(panel,column.width)?}));
                    }
                }
            }
        }
        let sizes=if let Some(layer)=request.layer {
            if ![layer.measurement.fixed_height,layer.measurement.unit_height,layer.properties_height].into_iter()
                .all(|value|value.is_finite()&&value>=0.)||drawer.columns.len()!=1||drawer.layer_properties.is_none() {
                return Err("Invalid layer drawer measurements".into());
            }
            let heights=LayerDrawerSizing::pages(layer.measurement,layer.rows,viewport[1],layer.properties_height);
            let placements=heights.map(|height|customization.drawer_placement(&drawer,&layout,viewport,&[height]));
            json!({"heights":heights,"placements":placements,"footer_height":LayerDrawerSizing::FOOTER_HEIGHT})
        }else{Value::Null};
        Ok(json!({"placement":placement,"toolbars":toolbars,"sizes":sizes}))
    }
}

#[cfg(test)]
mod tests {
use super::*;
const VIEWPORT:[f32;2]=[1200.,900.];

fn pure_drawer(layout:&DockLayout,model:&ContentDrawer,column_drawers:&[ContentDrawer],tiles:&[DrawerTileMeasurement],
    heights:&[f32],toolbars:bool,layer:Option<serde_json::Value>)->serde_json::Value {
    let source=serde_json::json!({"model":model,"column_drawers":column_drawers,"tiles":tiles,
        "heights":heights,"toolbars":toolbars,"layer":layer}).to_string();
    LayoutResolveInput::new(layout,None).drawer_geometry(VIEWPORT,&source).unwrap()
}

#[test]
fn supplied_header_and_dock_tile_drawers_match_shared_live_placement() {
    let control=ToolbarControl::Panel {panel:Panel::Brushes};
    let mut layout=DockLayout::default();
    layout.header.add(HeaderZone::Left,None,&[HeaderItem::Tool {control}]).unwrap();
    let id=layout.header.entries().find(|entry|entry.item==HeaderItem::Tool {control}).unwrap().id;
    layout.header_presentation=HeaderPresentation {height:88.,items:vec![HeaderItemBounds {
        id,bounds:Bounds {x:58.,y:10.,width:40.,height:46.}}]};
    let header=ContentDrawer::for_header(&layout,id).unwrap();
    let heights=[195.];
    let live=CustomizationState::default().drawer_placement(&header,&layout,VIEWPORT,&heights);
    let reply=pure_drawer(&layout,&header,&[],&[],&heights,false,None);
    assert_eq!(reply["placement"],serde_json::to_value(live).unwrap());
    assert_eq!(reply["placement"]["bounds"]["y"],serde_json::json!(88.));

    for edge in [Edge::Top,Edge::Bottom,Edge::Left,Edge::Right] {
        let mut layout=DockLayout::default();
        layout.move_panel(VIEWPORT,Panel::Toolbar,DockTarget::Edge {edge,outer:false}).unwrap();
        let tile=layout.panel(Panel::Toolbar).unwrap().tiles()[0].id;
        let model=ContentDrawer::for_tile(&layout,TileAnchor {panel:Panel::Toolbar,tile}).unwrap();
        let heights=vec![320.;model.columns.len()];
        let live=CustomizationState::default().drawer_placement(&model,&layout,VIEWPORT,&heights);
        let reply=pure_drawer(&layout,&model,&[],&[],&heights,false,None);
        assert_eq!(reply["placement"],serde_json::to_value(live).unwrap(),"{edge:?}");
    }
}

#[test]
fn collapsed_group_and_nested_live_tile_use_current_owner_and_measured_origin() {
    let mut layout=DockLayout::default();
    let group=layout.panel_group(Panel::Brushes).unwrap();
    layout.move_panel(VIEWPORT,Panel::Toolbar,DockTarget::Tab {group,index:None}).unwrap();
    layout.set_column_collapsed(group,true,VIEWPORT).unwrap();
    let parent=ContentDrawer::for_column(&layout,group,Panel::Toolbar).unwrap();
    let height=[450.];
    let parent_live=CustomizationState::default().drawer_placement(&parent,&layout,VIEWPORT,&height);
    assert_eq!(pure_drawer(&layout,&parent,&[],&[],&height,false,None)["placement"],
        serde_json::to_value(parent_live).unwrap());

    let DrawerAnchor::Column {column,..}=parent.anchor else {panic!("expected a collapsed column")};
    let tile=layout.panel(Panel::Toolbar).unwrap().tiles()[0].id;
    let anchor=TileAnchor {panel:Panel::Toolbar,tile};
    let child=ContentDrawer::for_tile(&layout,anchor).unwrap();
    let heights=vec![400.;child.columns.len()];
    let mut live=CustomizationState::default();
    live.column_drawers=vec![parent.clone()];
    let first=DrawerTileMeasurement {column,anchor,bounds:Bounds {x:80.,y:200.,width:36.,height:36.}};
    live.measure_drawer_tiles(&layout,vec![first]).unwrap();
    let expected=live.drawer_placement(&child,&layout,VIEWPORT,&heights);
    let reply=pure_drawer(&layout,&child,&live.column_drawers,&[first],&heights,false,None);
    assert_eq!(reply["placement"],serde_json::to_value(expected).unwrap());
    assert_eq!(reply["placement"]["anchor"]["y"],serde_json::json!(200.));

    let moved=DrawerTileMeasurement {bounds:Bounds {y:160.,height:20.,..first.bounds},..first};
    live.measure_drawer_tiles(&layout,vec![moved]).unwrap();
    let reply=pure_drawer(&layout,&child,&live.column_drawers,&[moved],&heights,false,None);
    assert_eq!(reply["placement"],serde_json::to_value(live.drawer_placement(&child,&layout,VIEWPORT,&heights)).unwrap());
    assert_eq!(reply["placement"]["anchor"]["y"],serde_json::json!(160.));

    let before=serde_json::to_value(&layout).unwrap();
    assert!(pure_drawer(&layout,&child,&[],&[moved],&heights,false,None)["placement"].is_null());
    assert!(pure_drawer(&layout,&child,&[parent],&[],&heights,false,None)["placement"].is_null());
    let foreign=DrawerTileMeasurement {column:column+100,..moved};
    assert!(pure_drawer(&layout,&child,&live.column_drawers,&[foreign],&heights,false,None)["placement"].is_null());
    let retired=DrawerTileMeasurement {anchor:TileAnchor {panel:Panel::Toolbar,tile:u32::MAX},..moved};
    assert!(pure_drawer(&layout,&child,&live.column_drawers,&[retired],&heights,false,None)["placement"].is_null());
    assert_eq!(serde_json::to_value(&layout).unwrap(),before);
}

#[test]
fn supplied_collapsed_toolbar_batch_uses_resolved_column_width() {
    let mut layout=DockLayout::default();
    let group=layout.panel_group(Panel::Brushes).unwrap();
    layout.move_panel(VIEWPORT,Panel::Toolbar,DockTarget::Tab {group,index:None}).unwrap();
    layout.select_tab(group,Panel::Toolbar).unwrap();
    layout.set_column_collapsed(group,true,VIEWPORT).unwrap();
    let model=ContentDrawer::for_column(&layout,group,Panel::Toolbar).unwrap();
    let reply=pure_drawer(&layout,&model,&[],&[],&[300.],true,None);
    assert!(reply["placement"].is_object());
    let width=reply["placement"]["columns"][0]["width"].as_f64().unwrap() as f32;
    assert_eq!(reply["toolbars"],serde_json::json!([{"panel":Panel::Toolbar,"width":width,
        "layout":layout.drawer_toolbar(Panel::Toolbar,width).unwrap()}]));
    assert!(reply["sizes"].is_null());
}

#[test]
fn supplied_layer_inspector_pages_match_shared_sizing() {
    let mut layout=DockLayout::default();
    let toolbar=layout.add_toolbar(None,"Layers",&[ToolbarControl::Panel {panel:Panel::Layers}]).unwrap();
    layout.move_panel(VIEWPORT,toolbar,DockTarget::Edge {edge:Edge::Left,outer:false}).unwrap();
    let tile=layout.panel(toolbar).unwrap().tiles()[0].id;
    let model=ContentDrawer::for_tile(&layout,TileAnchor {panel:toolbar,tile}).unwrap();
    assert_eq!(model.layer_properties,Some(false));
    let measurement=PanelScrollMeasurement {fixed_height:80.,unit_height:32.};
    let rows=21usize;
    let properties_height=370.;
    let layer=serde_json::json!({"measurement":measurement,"rows":rows,"properties_height":properties_height});
    let reply=pure_drawer(&layout,&model,&[],&[],&[300.],true,Some(layer));
    let heights=LayerDrawerSizing::pages(measurement,rows,VIEWPORT[1],properties_height);
    let placements=heights.map(|height|CustomizationState::default().drawer_placement(&model,&layout,VIEWPORT,&[height]));
    assert!(placements.iter().all(Option::is_some));
    assert_eq!(reply["sizes"]["heights"],serde_json::to_value(heights).unwrap());
    assert_eq!(reply["sizes"]["placements"],serde_json::to_value(placements).unwrap());
    assert_eq!(reply["sizes"]["footer_height"],serde_json::json!(LayerDrawerSizing::FOOTER_HEIGHT));

    assert_eq!(reply["toolbars"],serde_json::json!([]));
}

#[test]
fn supplied_drawer_geometry_rejects_invalid_measurements() {
    let layout=DockLayout::default();
    let tile=layout.panel(Panel::Toolbar).unwrap().tiles()[0].id;
    let model=ContentDrawer::for_tile(&layout,TileAnchor {panel:Panel::Toolbar,tile}).unwrap();
    let source=serde_json::json!({"model":model,"column_drawers":[],"tiles":[],
        "heights":vec![320.;model.columns.len()],"toolbars":false});
    let input=LayoutResolveInput::new(&layout,None);
    for viewport in [[0.,900.],[-1.,900.],[f32::NAN,900.]] {
        assert!(input.drawer_geometry(viewport,&source.to_string()).is_err());
    }
    for (key,value) in [("heights",serde_json::json!([])),("heights",serde_json::json!([-1.])),
        ("model",serde_json::json!({"anchor":model.anchor,"columns":[[]],"compact":false}))] {
        let mut invalid=source.clone();invalid[key]=value;
        assert!(input.drawer_geometry(VIEWPORT,&invalid.to_string()).is_err());
    }
}

#[test]
fn ordinary_drawer_contents_keep_endpoint_geometry_during_frame_motion() {
    let from=DrawerPlacement {bounds:Bounds {x:40.,y:60.,width:320.,height:480.},
        anchor:Bounds {x:0.,y:60.,width:36.,height:36.},direction:Edge::Right,
        columns:vec![Bounds {x:0.,y:0.,width:320.,height:480.}]};
    let to=DrawerPlacement {bounds:Bounds {width:240.,height:260.,..from.bounds},
        columns:vec![Bounds {width:240.,height:260.,..from.columns[0]}],..from.clone()};
    let mut request=serde_json::json!({"from":from,"to":to,"progress":0.5});
    let resized=drawer_frame(&request.to_string()).unwrap();
    request["clip_contents"]=serde_json::json!(true);
    let clipped=drawer_frame(&request.to_string()).unwrap();
    assert_eq!(clipped["placement"]["bounds"],resized["placement"]["bounds"]);
    assert_eq!(clipped["placement"]["columns"],serde_json::to_value(&to.columns).unwrap());
    assert_ne!(resized["placement"]["columns"],clipped["placement"]["columns"]);
}

}
