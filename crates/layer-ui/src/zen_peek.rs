use super::*;

const EDGE_REACH: f32 = 24.;
const EDGE_SWIPE: f32 = 40.;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ZenPeek {
    edge: Option<Edge>,
    swipe: Option<(u64, Edge, [f32; 2])>,
}

impl<R: CanvasRenderer> UiSession<R> {
    pub fn layout_resolve_input(&self)->crate::LayoutResolveInput<'_> {
        crate::LayoutResolveInput::new(&self.state.workspace.layout,self.zen_peek.edge)
            .with_cache(self.panel_measurement_cache().for_publication())
    }
    pub fn zen_keeps(&self, group: u32) -> bool {
        self.layout_resolve_input().keeps(group)
    }

    fn edge_groups(&self, edge: Edge) -> impl Iterator<Item = GroupPlacement> + '_ {
        let viewport = self.logical_viewport.unwrap_or_default();
        self.layout(viewport)
            .groups
            .into_iter()
            .filter(move |g| !g.floating && self.state.workspace.layout.group_edge(g.id) == Some(edge))
    }

    pub(crate) fn edge_swipe(&mut self, id: u64, phase: ContactPhase, position: [f32; 2]) -> bool {
        let Some(viewport) = self.logical_viewport.filter(|_| self.state.workspace.zen_mode && self.interaction.hidden) else {
            self.zen_peek.swipe = None;
            return false;
        };
        let scale = (self.state.camera.viewport[0] as f32 / viewport[0]).max(f32::EPSILON);
        let position = position.map(|v| v / scale);
        match phase {
            ContactPhase::Down => {
                let edge = [
                    (Edge::Left, position[0]),
                    (Edge::Right, viewport[0] - position[0]),
                    (Edge::Top, position[1]),
                    (Edge::Bottom, viewport[1] - position[1]),
                ]
                .into_iter()
                .filter(|(_, distance)| *distance <= EDGE_REACH)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(edge, _)| edge);
                self.zen_peek.swipe = edge
                    .filter(|edge| self.edge_groups(*edge).next().is_some())
                    .map(|edge| (id, edge, position));
                false
            }
            ContactPhase::Move => {
                let Some((_, edge, start)) = self.zen_peek.swipe.filter(|s| s.0 == id) else { return false };
                let inward = match edge {
                    Edge::Left => position[0] - start[0],
                    Edge::Right => start[0] - position[0],
                    Edge::Top => position[1] - start[1],
                    Edge::Bottom => start[1] - position[1],
                };
                if inward < EDGE_SWIPE {
                    return false;
                }
                self.zen_peek = ZenPeek { edge: Some(edge), swipe: None };
                self.touch.clear();
                true
            }
            ContactPhase::Up | ContactPhase::Cancel => {
                if self.zen_peek.swipe.is_some_and(|s| s.0 == id) {
                    self.zen_peek.swipe = None;
                }
                false
            }
        }
    }

    pub(crate) fn end_zen_peek(&mut self, position: [f32; 2]) -> bool {
        let Some(edge) = self.zen_peek.edge else { return false };
        let inside = self.edge_groups(edge).any(|g| g.bounds.contains(position[0], position[1]));
        if !inside || !self.state.workspace.zen_mode {
            self.zen_peek = ZenPeek::default();
        }
        !inside
    }
}
