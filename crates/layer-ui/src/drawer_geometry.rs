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
    heights:Option<Vec<f32>>,
    toolbars:bool,
    layer:Option<Layer>,
}

impl CustomizationState {
    pub fn layer_drawer_sizing(&self,drawer:Option<&ContentDrawer>,layout:&LayoutResolveInput<'_>,viewport:[f32;2],
        measurement:PanelScrollMeasurement,rows:usize,properties_height:f32)->Result<Value,String> {
        if drawer.is_some_and(|drawer|drawer.columns.len()!=1||drawer.layer_properties.is_none()) {
            return Err("Invalid layer drawer measurements".into());
        }
        let measure=drawer.and_then(|drawer|self.drawer_measure(drawer,layout,viewport));
        layer_sizes(measure.as_ref(),measurement,rows,viewport[1],properties_height)
    }
}

pub(crate) fn layer_sizes(measure:Option<&DrawerMeasure>,measurement:PanelScrollMeasurement,rows:usize,
    viewport_height:f32,properties_height:f32)->Result<Value,String> {
    if ![measurement.fixed_height,measurement.unit_height,viewport_height,properties_height].into_iter()
        .all(|value|value.is_finite()&&value>=0.) {
        return Err("Invalid layer drawer measurements".into());
    }
    let heights=LayerDrawerSizing::presented_pages(measurement,rows,viewport_height,properties_height,measure.and_then(DrawerMeasure::phone_layout));
    let placements=heights.map(|height|measure.and_then(|measure|measure.placement(&[height])));
    Ok(json!({"measurement":measurement,"properties_height":properties_height,"heights":heights,"placements":placements,"footer_height":LayerDrawerSizing::FOOTER_HEIGHT}))
}

impl LayoutResolveInput<'_> {
    pub fn drawer_geometry(&self,viewport:[f32;2],source:&str)->Result<Value,String> {
        let request:Request=serde_json::from_str(source).map_err(|error|error.to_string())?;
        if !viewport.into_iter().all(|value|value.is_finite()&&value>0.)
            ||request.model.columns.is_empty()||request.model.columns.iter().any(Vec::is_empty)
            ||request.heights.as_ref().is_some_and(|heights|heights.len()!=request.model.columns.len()
                ||heights.iter().any(|height|!height.is_finite()||*height<0.)) {
            return Err("Invalid drawer measurements".into());
        }
        let drawer=request.model.drawer();
        let layout=self.measured_layout(viewport);
        let mut customization=CustomizationState {column_drawers:request.column_drawers.into_iter().map(Geometry::drawer).collect(),..Default::default()};
        customization.measure_drawer_tiles(&layout,request.tiles)?;
        let input=LayoutResolveInput::new(&layout,None).with_phone(self.phone_state());
        let measure=customization.drawer_measure(&drawer,&input,viewport);
        let placement=measure.as_ref().and_then(|measure|request.heights.as_ref().and_then(|heights|measure.placement(heights)));
        let mut toolbars=Vec::new();
        if request.toolbars&&let Some(measure)=&measure {
            for (panels,column) in drawer.columns.iter().zip(&measure.placement.columns) {
                for &panel in panels {
                    if !layout.panel(panel)?.tiles().is_empty() {
                        toolbars.push(json!({"panel":panel,"width":column.width,"layout":layout.drawer_toolbar(panel,column.width)?}));
                    }
                }
            }
        }
        let sizes=if let Some(layer)=request.layer {
            customization.layer_drawer_sizing(Some(&drawer),&input,viewport,layer.measurement,layer.rows,layer.properties_height)?
        }else{Value::Null};
        Ok(json!({"measure":measure,"placement":placement,"toolbars":toolbars,"sizes":sizes}))
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
    let parent_live=CustomizationState::default().drawer_placement(&parent,&LayoutResolveInput::new(&layout,None),VIEWPORT,&height);
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
    let expected=live.drawer_placement(&child,&LayoutResolveInput::new(&layout,None),VIEWPORT,&heights);
    let reply=pure_drawer(&layout,&child,&live.column_drawers,&[first],&heights);
    assert_eq!(reply["placement"],serde_json::to_value(expected).unwrap());
    assert_eq!(reply["placement"]["anchor"]["y"],serde_json::json!(200.));

    let moved=DrawerTileMeasurement {bounds:Bounds {y:160.,height:20.,..first.bounds},..first};
    live.measure_drawer_tiles(&layout,vec![moved]).unwrap();
    let reply=pure_drawer(&layout,&child,&live.column_drawers,&[moved],&heights);
    assert_eq!(reply["placement"],serde_json::to_value(live.drawer_placement(&child,&LayoutResolveInput::new(&layout,None),VIEWPORT,&heights)).unwrap());
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

#[test]
fn snapped_drawer_bridges_meet_both_surfaces_on_every_edge_and_alignment() {
    for direction in [Edge::Top, Edge::Right, Edge::Bottom, Edge::Left] {
        for mirrored in [false, true] {
            let anchor=Bounds {x:100.2,y:100.3,width:36.4,height:36.5};
            let along=if mirrored {56.7} else {92.1};
            let bounds=match direction {
                Edge::Bottom=>Bounds {x:along,y:144.8,width:88.7,height:90.3},
                Edge::Top=>Bounds {x:along,y:2.1,width:88.7,height:90.2},
                Edge::Right=>Bounds {x:144.6,y:along,width:90.3,height:88.7},
                Edge::Left=>Bounds {x:2.1,y:along,width:90.1,height:88.7},
            };
            for scale in [1.,1.25,1.5,2.] {
                let placement=DrawerPlacement {bounds,anchor,direction,columns:Vec::new(),tab_presentations:Vec::new()}.snapped(scale);
                for b in [placement.anchor,placement.bounds] {
                    for edge in [b.x,b.y,b.x+b.width,b.y+b.height] {
                        assert!((edge*scale-(edge*scale).round()).abs()<0.0001);
                    }
                }
                let connection=placement.connection().unwrap();
                let [xx,yx,xy,yy,tx,ty]=connection.transform;
                let world=|v:f32|[connection.bounds.x+xx*connection.length*0.5+xy*v+tx,
                    connection.bounds.y+yx*connection.length*0.5+yy*v+ty];
                let (axis,near,far)=match direction {
                    Edge::Bottom=>(1,placement.anchor.y+placement.anchor.height,placement.bounds.y),
                    Edge::Top=>(1,placement.anchor.y,placement.bounds.y+placement.bounds.height),
                    Edge::Right=>(0,placement.anchor.x+placement.anchor.width,placement.bounds.x),
                    Edge::Left=>(0,placement.anchor.x,placement.bounds.x+placement.bounds.width),
                };
                assert!((world(0.)[axis]-near).abs()<0.0001,"{direction:?}/{mirrored}/{scale}");
                assert!((world(connection.depth)[axis]-far).abs()<0.0001,"{direction:?}/{mirrored}/{scale}");
            }
        }
    }
}

#[test]
fn portrait_color_drawer_uses_its_column_width_when_the_height_fits() {
    let layout=WorkspacePreset::Painter.layout(Platform::Gtk);
    let drawer=ContentDrawer::for_control(ToolbarControl::Color,DrawerAnchor::Phone {tile:None}).unwrap();
    let input=LayoutResolveInput::new(&layout,None).with_phone(PhoneState { platform: Some(Platform::Gtk), ..Default::default() });
    for viewport in [[412.,891.],[375.,812.],[360.,780.],[320.,780.],[280.,780.]] {
        let phone=PhoneState { platform: Some(Platform::Gtk), ..Default::default() }.resolve(&layout,viewport).unwrap();
        let width=phone.bars[0].width;
        let color=ColorPanelLayout::new(width-2.*PANEL_CONTENT_INSET).unwrap().height()+2.*PANEL_CONTENT_INSET;
        let placement=CustomizationState::default().drawer_placement(&drawer,&input,viewport,&[color,220.]).unwrap();
        assert_eq!(placement.columns[0].width,width);
        assert!((placement.columns[0].height-color).abs()<0.001,"{viewport:?}: {placement:?}");
    }
}

}
