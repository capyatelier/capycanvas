use crate::{Bounds, Edge, Panel, ResolvedLayout, WORKSPACE_SPACING};
use serde::{Deserialize, Serialize};

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

impl ResolvedLayout {
    pub fn measured_panel_bridges(mut self, measure: PanelBridgeMeasure) -> Result<PanelBridgeFrame, String> {
        let valid = |b: Bounds| [b.x,b.y,b.width,b.height].into_iter().all(f32::is_finite) && b.width>=0. && b.height>=0.;
        let mut groups = std::collections::HashSet::new();
        let mut columns = std::collections::HashSet::new();
        if measure.groups.iter().any(|(id,b)| !groups.insert(*id) || !valid(*b) || !self.groups.iter().any(|g|g.id==*id))
            || measure.columns.iter().any(|c| !columns.insert(c.column) || !valid(c.bounds) || !valid(c.content)
                || !self.collapsed.iter().any(|column|column.id==c.column)
                || {let mut icons=std::collections::HashSet::new();c.icons.iter().any(|(panel,b)| !icons.insert(*panel) || !valid(*b)
                    || !self.collapsed.iter().filter(|column|column.id==c.column).flat_map(|column|&column.groups)
                        .flat_map(|group|&group.icons).any(|icon|icon.panel==*panel))}) {
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

    pub fn panel_bridges(&self) -> Vec<(u32, Panel, PanelBridge)> {
        let mut bridges = Vec::new();
        for column in &self.collapsed {
            let Some(open) = &column.open else { continue; };
            let plane = match open.direction {Edge::Right=>open.bounds.x,Edge::Left=>open.bounds.x+open.bounds.width,_=>continue};
            let mut receivers: Vec<_> = self.groups.iter().filter(|target| {
                let x = if open.direction == Edge::Right { target.bounds.x } else { target.bounds.x + target.bounds.width };
                target.bounds.width>0. && target.bounds.height>0. && (x - plane).abs() < 0.5
                    && column.groups.iter().any(|group| group.group == target.id)
            }).map(|group| PanelBridgeReceiver {group:group.id,bounds:group.bounds,radii:group.corner_radii}).collect();
            receivers.sort_by(|a, b| a.bounds.y.total_cmp(&b.bounds.y));
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
