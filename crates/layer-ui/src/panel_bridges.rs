use crate::{Bounds, Edge, Panel, ResolvedLayout, WORKSPACE_SPACING};

#[derive(Clone, Debug, PartialEq)]
pub struct PanelBridgeReceiver {
    pub group: u32,
    pub bounds: Bounds,
    pub radii: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct PanelBridge {
    pub anchor: Bounds,
    pub plane: f32,
    pub direction: Edge,
    pub receivers: Vec<PanelBridgeReceiver>,
}

impl PanelBridge {
    pub fn width(&self) -> f32 {
        if self.direction == Edge::Right { self.plane - self.anchor.x - self.anchor.width }
        else { self.anchor.x - self.plane }.max(0.)
    }
}

impl ResolvedLayout {
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
