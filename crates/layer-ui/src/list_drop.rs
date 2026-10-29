#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListDropEdge { BeforeFirst, Inside, AfterLast }

pub fn list_drop_edge(position: f32, first: f32, last: f32) -> Option<ListDropEdge> {
    if !position.is_finite() || !first.is_finite() || !last.is_finite() || first >= last { return None; }
    Some(if position < first { ListDropEdge::BeforeFirst }
        else if position >= last { ListDropEdge::AfterLast }
        else { ListDropEdge::Inside })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn panel_space_before_and_after_the_list_selects_its_edges() {
        assert_eq!(list_drop_edge(-30., 10., 90.), Some(ListDropEdge::BeforeFirst));
        assert_eq!(list_drop_edge(10., 10., 90.), Some(ListDropEdge::Inside));
        assert_eq!(list_drop_edge(89., 10., 90.), Some(ListDropEdge::Inside));
        assert_eq!(list_drop_edge(90., 10., 90.), Some(ListDropEdge::AfterLast));
        assert_eq!(list_drop_edge(150., 10., 90.), Some(ListDropEdge::AfterLast));
        for values in [[f32::NAN, 10., 90.], [0., 90., 10.], [0., 0., f32::INFINITY], [0., 10., 10.]] {
            assert_eq!(list_drop_edge(values[0], values[1], values[2]), None);
        }
    }
}
