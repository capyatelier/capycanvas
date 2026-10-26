use crate::{Bounds, Edge, Panel, CollapsedColumnPlacement, ResolvedLayout, WORKSPACE_SPACING};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanelVectorSurface {
    pub group: u32,
    pub column: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<[i32; 7]>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triangles: Vec<[i32; 7]>,
}

#[derive(Default)]
pub struct PanelVectorCache {
    entries: Vec<PanelVectorEntry>,
    prepared: std::collections::VecDeque<PanelVectorEntry>,
    generation: u64,
}

#[derive(PartialEq)]
struct PanelVectorKey {
    contour: layer_render::panel_geometry::PanelContour,
    right: bool,
    source: [f32;4],
    source_radii: [f32;4],
}

struct PanelVectorEntry {
    group: u32,
    generation: u64,
    key: PanelVectorKey,
    commands: Vec<[i32;7]>,
    triangles: Vec<[i32;7]>,
}

impl PanelVectorCache {
    pub fn packed(&self) -> Vec<i32> {
        let mut result=Vec::new();
        for entry in &self.entries {
            result.extend([entry.group as i32,entry.generation as i32,(entry.generation>>32) as i32,
                entry.commands.len() as i32,entry.triangles.len() as i32]);
            result.extend(entry.commands.iter().chain(&entry.triangles).flatten());
        }
        result
    }

    pub fn resolve(&mut self,layout: &mut ResolvedLayout,scale: f32) {
        if scale>0. {
            let snap=|b: &mut Bounds| {
                let x=(b.x*scale).round()/scale;let y=(b.y*scale).round()/scale;
                b.width=((b.x+b.width)*scale).round()/scale-x;
                b.height=((b.y+b.height)*scale).round()/scale-y;b.x=x;b.y=y;
            };
            for group in &mut layout.groups {snap(&mut group.bounds);}
            for column in &mut layout.collapsed {
                snap(&mut column.bounds);snap(&mut column.content);
                if let Some(open)=&mut column.open {snap(&mut open.bounds);}
                for icon in column.groups.iter_mut().flat_map(|g|&mut g.icons) {snap(&mut icon.bounds);}
            }
        }
        let bridges=layout.panel_bridges();
        let mut live=Vec::new();
        layout.vector_surfaces.clear();
        for column in &layout.collapsed {
            let Some(open)=&column.open else {continue;};
            let Some(plane)=bridge_plane(open.direction,open.bounds) else {continue;};
            let targets=layout.panel_bridge_receivers(column,plane,open.direction);
            let (Some(top),Some(bottom))=(targets.first(),targets.last()) else {continue;};
            let direction=if open.direction==Edge::Right {1.} else {-1.};
            let width=if direction>0. {plane-column.bounds.x-column.bounds.width} else {column.bounds.x-plane};
            let facing=if direction>0. {[0,3]} else {[1,2]};
            for (index,receiver) in targets.iter().enumerate() {
                let b=receiver.bounds;
                let anchors=bridges.iter().filter(|(id,..)|*id==column.id).map(|(_,_,bridge)|
                    [bridge.anchor.y-b.y,bridge.anchor.y+bridge.anchor.height-b.y])
                    .filter(|a|a[0]<b.height+WORKSPACE_SPACING && a[1]> -WORKSPACE_SPACING).collect::<Vec<_>>();
                let mut gaps=targets.windows(2).map(|w|[w[0].bounds.y+w[0].bounds.height-b.y,w[1].bounds.y-b.y])
                    .filter(|g|g[1]>g[0]).collect::<Vec<_>>();
                if anchors.iter().any(|a|a[0]<top.bounds.y-b.y) {gaps.push([top.bounds.y-b.y-WORKSPACE_SPACING,top.bounds.y-b.y]);}
                if anchors.iter().any(|a|a[1]>bottom.bounds.y+bottom.bounds.height-b.y) {
                    gaps.push([bottom.bounds.y+bottom.bounds.height-b.y,bottom.bounds.y+bottom.bounds.height-b.y+WORKSPACE_SPACING]);
                }
                let mut radii=receiver.radii;
                if direction<0. {radii=[radii[1],radii[0],radii[3],radii[2]];}
                let spec=layer_render::panel_geometry::PanelContour {bounds:[0.,0.,b.width,b.height],radii,anchors,
                    neighbors:targets.iter().filter(|r|r.group!=receiver.group).map(|r| {
                        let radii=if direction>0. {r.radii} else {[r.radii[1],r.radii[0],r.radii[3],r.radii[2]]};
                        ([0.,r.bounds.y-b.y,r.bounds.width,r.bounds.height],radii)
                    }).collect(),
                    width:width.max(0.),gaps,envelope:[top.bounds.y-b.y,bottom.bounds.y+bottom.bounds.height-b.y,
                        layer_render::panel_geometry::panel_radii([0.,0.,top.bounds.width,top.bounds.height],top.radii)[facing[0]],
                        layer_render::panel_geometry::panel_radii([0.,0.,bottom.bounds.width,bottom.bounds.height],bottom.radii)[facing[1]]],
                    clip:[if index==0 {-1e6} else {(targets[index-1].bounds.y+targets[index-1].bounds.height+b.y)/2.-b.y},
                        if index+1==targets.len() {1e6} else {(b.y+b.height+targets[index+1].bounds.y)/2.-b.y}],
                };
                let source=[if direction>0. {column.bounds.x-plane} else {plane-column.bounds.x-column.bounds.width},
                    column.bounds.y-b.y,column.bounds.width,column.bounds.height];
                let source_radii=if direction>0. {column.corner_radii} else {
                    let r=column.corner_radii;[r[1],r[0],r[3],r[2]]
                };
                let key=PanelVectorKey {contour:spec,right:direction>0.,source,source_radii};
                let (generation,commands,triangles)=if let Some(entry)=self.entries.iter().chain(self.prepared.iter()).find(|entry|entry.group==receiver.group && entry.key==key) {
                    (entry.generation,entry.commands.clone(),entry.triangles.clone())
                } else {
                    let commands=key.contour.commands(0.025).into_iter().map(|mut c| {
                        let points=match c[0] as u32 {0|1=>1,2=>3,_=>0};
                        for i in (1..=points*2).step_by(2) {c[i]=if direction>0. {c[i]} else {b.width-c[i]};}
                        std::array::from_fn(|i|if i==0 {c[i] as i32} else {(c[i]*1024.).round() as i32})
                    }).collect::<Vec<_>>();
                    let mut global=key.contour.clone();global.clip=[-1e6,1e6];
                    let field=global.prepare();
                    let triangles=layer_render::panel_mesh_with_coverage(&commands,|p| {
                        let p=[if direction>0. {p[0]} else {b.width-p[0]},p[1]];
                        field.field(p).min(layer_render::panel_geometry::panel_rect(p,source,source_radii))<=0.
                    });
                    self.generation+=1;
                    (self.generation,commands,triangles)
                };
                live.push(PanelVectorEntry {group:receiver.group,generation,key,commands:commands.clone(),triangles:triangles.clone()});
                layout.vector_surfaces.push(PanelVectorSurface {group:receiver.group,column:column.id,commands,triangles});
            }
        }
        for entry in std::mem::replace(&mut self.entries,live) {
            if self.entries.iter().any(|current|current.group==entry.group && current.key==entry.key) {continue;}
            self.prepared.retain(|cached|cached.group!=entry.group || cached.key!=entry.key);
            self.prepared.push_front(entry);
        }
        let bytes=|entry: &PanelVectorEntry|std::mem::size_of::<PanelVectorEntry>()
            +(entry.commands.len()+entry.triangles.len())*std::mem::size_of::<[i32;7]>()
            +(entry.key.contour.anchors.len()+entry.key.contour.gaps.len())*std::mem::size_of::<[f32;2]>()
            +entry.key.contour.neighbors.len()*std::mem::size_of::<([f32;4],[f32;4])>();
        let mut retained=self.prepared.iter().map(bytes).sum::<usize>();
        while self.prepared.len()>512 || retained>4*1024*1024 {
            retained-=bytes(&self.prepared.pop_back().unwrap());
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanelBridgeReceiver {
    pub group: u32,
    pub bounds: Bounds,
    pub radii: [f32; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanelBridge {
    pub anchor: Bounds,
    pub plane: f32,
    pub direction: Edge,
    pub receivers: Vec<PanelBridgeReceiver>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelColumnMeasure {
    pub column: u32,
    pub bounds: Bounds,
    pub content: Bounds,
    pub icons: Vec<(Panel, Bounds)>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelBridgeMeasure {
    pub groups: Vec<(u32, Bounds)>,
    pub columns: Vec<PanelColumnMeasure>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PanelColumnGeometry {
    pub column: u32,
    pub bounds: Bounds,
    pub radii: [f32; 4],
    pub icons: Vec<(Panel, [f32; 4])>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PanelBridgeFrame {
    pub groups: Vec<PanelBridgeReceiver>,
    pub columns: Vec<PanelColumnGeometry>,
    pub bridges: Vec<(u32, Panel, PanelBridge)>,
}

impl PanelBridge {
    pub fn width(&self) -> f32 {
        if self.direction == Edge::Right { self.plane - self.anchor.x - self.anchor.width }
        else { self.anchor.x - self.plane }.max(0.)
    }
}

fn bridge_plane(direction: Edge,bounds: Bounds) -> Option<f32> {
    match direction {Edge::Right=>Some(bounds.x),Edge::Left=>Some(bounds.x+bounds.width),_=>None}
}

impl ResolvedLayout {
    pub fn measured_panel_bridges(mut self, measure: PanelBridgeMeasure) -> Result<PanelBridgeFrame, String> {
        let valid = |b: Bounds| [b.x,b.y,b.width,b.height].into_iter().all(f32::is_finite) && b.width>=0. && b.height>=0.;
        let mut groups = std::collections::HashSet::new();
        let mut columns = std::collections::HashSet::new();
        if measure.groups.iter().any(|(id,b)| !groups.insert(*id) || !valid(*b))
            || measure.columns.iter().any(|c| !columns.insert(c.column) || !valid(c.bounds) || !valid(c.content)
                || {let mut icons=std::collections::HashSet::new();c.icons.iter().any(|(panel,b)| !icons.insert(*panel) || !valid(*b))}) {
            return Err("Invalid panel measurements".into());
        }
        self.groups.retain_mut(|group| {
            let Some((_,bounds))=measure.groups.iter().find(|(id,_)|*id==group.id) else {return false;};
            group.bounds = *bounds;bounds.width>0. && bounds.height>0.
        });
        self.collapsed.retain_mut(|column| {
            let Some(allocated)=measure.columns.iter().find(|c|c.column==column.id) else {return false;};
            column.bounds=allocated.bounds;column.content=allocated.content;
            for icon in column.groups.iter_mut().flat_map(|group|&mut group.icons) {
                icon.bounds=allocated.icons.iter().find(|(panel,_)|*panel==icon.panel).map_or(Bounds::default(),|(_,bounds)|*bounds);
            }
            if let Some(open)=&mut column.open {
                let receivers=self.groups.iter().filter(|g|column.groups.iter().any(|c|c.group==g.id)).map(|g|g.bounds);
                let Some(bounds)=receivers.reduce(|a,b| {
                    let x=a.x.min(b.x);let y=a.y.min(b.y);
                    Bounds{x,y,width:(a.x+a.width).max(b.x+b.width)-x,height:(a.y+a.height).max(b.y+b.height)-y}
                }) else {column.open=None;return true;};
                open.bounds=bounds;
            }
            true
        });
        let layout=self.with_panel_bridges();
        Ok(PanelBridgeFrame {
            bridges:layout.panel_bridges(),
            groups:layout.groups.iter().map(|g|PanelBridgeReceiver{group:g.id,bounds:g.bounds,radii:g.corner_radii}).collect(),
            columns:layout.collapsed.iter().map(|c|PanelColumnGeometry{column:c.id,bounds:c.bounds,radii:c.corner_radii,
                icons:c.groups.iter().flat_map(|g|&g.icons).map(|i|(i.panel,i.corner_radii)).collect()}).collect(),
        })
    }

    fn panel_bridge_receivers(&self,column: &CollapsedColumnPlacement,plane: f32,direction: Edge) -> Vec<PanelBridgeReceiver> {
        let mut receivers=self.groups.iter().filter(|target| {
            let x=if direction==Edge::Right {target.bounds.x} else {target.bounds.x+target.bounds.width};
            target.bounds.width>0. && target.bounds.height>0. && (x-plane).abs()<0.5
                && column.groups.iter().any(|group|group.group==target.id)
        }).map(|group|PanelBridgeReceiver {group:group.id,bounds:group.bounds,radii:group.corner_radii}).collect::<Vec<_>>();
        receivers.sort_by(|a,b|a.bounds.y.total_cmp(&b.bounds.y));
        receivers
    }

    pub fn panel_bridges(&self) -> Vec<(u32, Panel, PanelBridge)> {
        let mut bridges = Vec::new();
        for column in &self.collapsed {
            let Some(open) = &column.open else { continue; };
            let Some(plane)=bridge_plane(open.direction,open.bounds) else {continue;};
            let receivers=self.panel_bridge_receivers(column,plane,open.direction);
            for group in &column.groups {
                let Some(anchor) = group.icons.iter().find(|icon| icon.panel == group.active)
                    .and_then(|icon| icon.bounds.intersection(column.content)) else { continue; };
                if !receivers.iter().any(|r| anchor.y < r.bounds.y + r.bounds.height + WORKSPACE_SPACING && anchor.y + anchor.height > r.bounds.y - WORKSPACE_SPACING) { continue; }
                let bridge=PanelBridge { anchor, plane, direction: open.direction, receivers: receivers.clone() };
                if bridge.width()>0. {bridges.push((column.id, group.active, bridge));}
            }
        }
        bridges
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_bridge_width_uses_the_allocated_facing_edges_in_both_directions() {
        let mut bridge = PanelBridge { anchor: Bounds { x: -42., y: 0., width: 36., height: 36. },
            plane: 0., direction: Edge::Right, receivers: Vec::new() };
        assert_eq!(bridge.width(), 6.);
        bridge.direction = Edge::Left;
        bridge.anchor.x = 6.;
        assert_eq!(bridge.width(), 6.);
        bridge.plane = 6.5;
        assert_eq!(bridge.width(), 0.);
    }
}
