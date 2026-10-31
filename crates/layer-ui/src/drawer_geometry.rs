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

impl CustomizationState {
    pub fn layer_drawer_sizing(&self,drawer:Option<&ContentDrawer>,layout:&DockLayout,viewport:[f32;2],
        measurement:PanelScrollMeasurement,rows:usize,properties_height:f32)->Result<Value,String> {
        if ![measurement.fixed_height,measurement.unit_height,properties_height].into_iter()
            .all(|value|value.is_finite()&&value>=0.)
            ||drawer.is_some_and(|drawer|drawer.columns.len()!=1||drawer.layer_properties.is_none()) {
            return Err("Invalid layer drawer measurements".into());
        }
        let heights=LayerDrawerSizing::pages(measurement,rows,viewport[1],properties_height);
        let placements=heights.map(|height|drawer.and_then(|drawer|self.drawer_placement(drawer,layout,viewport,&[height])));
        Ok(json!({"heights":heights,"placements":placements,"footer_height":LayerDrawerSizing::FOOTER_HEIGHT}))
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
            customization.layer_drawer_sizing(Some(&drawer),&layout,viewport,layer.measurement,layer.rows,layer.properties_height)?
        }else{Value::Null};
        Ok(json!({"placement":placement,"toolbars":toolbars,"sizes":sizes}))
    }
}

#[cfg(test)]
mod tests {
use super::*;
const VIEWPORT:[f32;2]=[1200.,900.];

fn pure_drawer(layout:&DockLayout,model:&ContentDrawer,column_drawers:&[ContentDrawer],tiles:&[DrawerTileMeasurement],
    heights:&[f32])->serde_json::Value {
    let source=serde_json::json!({"model":model,"column_drawers":column_drawers,"tiles":tiles,
        "heights":heights,"toolbars":false}).to_string();
    LayoutResolveInput::new(layout,None).drawer_geometry(VIEWPORT,&source).unwrap()
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
    assert_eq!(pure_drawer(&layout,&parent,&[],&[],&height)["placement"],
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
    let reply=pure_drawer(&layout,&child,&live.column_drawers,&[first],&heights);
    assert_eq!(reply["placement"],serde_json::to_value(expected).unwrap());
    assert_eq!(reply["placement"]["anchor"]["y"],serde_json::json!(200.));

    let moved=DrawerTileMeasurement {bounds:Bounds {y:160.,height:20.,..first.bounds},..first};
    live.measure_drawer_tiles(&layout,vec![moved]).unwrap();
    let reply=pure_drawer(&layout,&child,&live.column_drawers,&[moved],&heights);
    assert_eq!(reply["placement"],serde_json::to_value(live.drawer_placement(&child,&layout,VIEWPORT,&heights)).unwrap());
    assert_eq!(reply["placement"]["anchor"]["y"],serde_json::json!(160.));

    let before=serde_json::to_value(&layout).unwrap();
    assert!(pure_drawer(&layout,&child,&[],&[moved],&heights)["placement"].is_null());
    assert!(pure_drawer(&layout,&child,&[parent],&[],&heights)["placement"].is_null());
    let foreign=DrawerTileMeasurement {column:column+100,..moved};
    assert!(pure_drawer(&layout,&child,&live.column_drawers,&[foreign],&heights)["placement"].is_null());
    let retired=DrawerTileMeasurement {anchor:TileAnchor {panel:Panel::Toolbar,tile:u32::MAX},..moved};
    assert!(pure_drawer(&layout,&child,&live.column_drawers,&[retired],&heights)["placement"].is_null());
    assert_eq!(serde_json::to_value(&layout).unwrap(),before);
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
fn layer_drawer_width_is_stable_across_scaled_viewports() {
    let drawer=ContentDrawer {anchor:DrawerAnchor::Header {id:1},columns:vec![vec![Panel::Layers]],
        tabs:None,compact:false,layer_properties:None,tool_set:None,dismissal:DrawerDismissal::OutsideContact};
    assert_eq!(drawer.column_widths(1920.),[280.]);
    for scale in [1.,1.25,1.5,2.] {assert_eq!(drawer.column_widths(1920./scale),[280.]);}
}

#[test]
fn native_drawer_frames_snap_columns_and_the_connection_to_the_same_edges() {
    let placement=DrawerPlacement {tab_presentations:Vec::new(),bounds:Bounds {x:100.3,y:76.2,width:240.4,height:360.3},
        anchor:Bounds {x:140.2,y:40.1,width:36.,height:36.1},direction:Edge::Bottom,
        columns:vec![Bounds {x:0.,y:0.,width:120.2,height:360.3},Bounds {x:120.2,y:0.,width:120.2,height:360.3}]};
    for scale in [1.,1.25,1.5,2.] {
        let source=json!({"from":placement,"to":placement,"progress":1.,"pixel_scale":scale}).to_string();
        let frame=drawer_frame(&source).unwrap();
        let snapped:DrawerPlacement=serde_json::from_value(frame["placement"].clone()).unwrap();
        let first=snapped.columns[0];let second=snapped.columns[1];
        assert!((first.x+first.width-second.x).abs()<0.0001);
        assert!((second.x+second.width-snapped.bounds.width).abs()<0.0001);
        for b in [snapped.bounds,snapped.anchor] {for v in [b.x,b.y,b.x+b.width,b.y+b.height] {assert!((v*scale-(v*scale).round()).abs()<0.0001);}}
        let connection:DrawerConnection=serde_json::from_value(frame["connection"].clone()).unwrap();
        assert!((connection.bounds.y+connection.bounds.height-snapped.bounds.y).abs()<0.0001);
    }
}

}
