//! Read persisted layout fields before migrating obsolete nested collapse state.
use super::*;

#[derive(Deserialize)]
struct SavedDockLayout {
    #[serde(default)]
    header: crate::HeaderLayout,
    #[serde(default)]
    canvas_info: crate::CanvasInfoLayout,
    bands: Vec<DockBand>,
    #[serde(
        default = "PanelConfig::defaults",
        deserialize_with = "read_panel_registry"
    )]
    panels: Vec<PanelConfig>,
    #[serde(default)]
    floating: Vec<FloatingGroup>,
    #[serde(default)]
    collapsed: Vec<CollapsedColumn>,
    #[serde(default, alias = "column_settings")]
    column_stacks: Vec<ColumnStack>,
    #[serde(default)]
    fit_tab_groups: Vec<u32>,
    #[serde(default = "initial_tile_id")]
    next_tile_id: u32,
    next_id: u32,
}

impl<'de> Deserialize<'de> for DockLayout {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let saved = SavedDockLayout::deserialize(d)?;
        let mut layout = Self {
            header: saved.header,
            canvas_info: saved.canvas_info,
            bands: saved.bands,
            panels: saved.panels,
            floating: saved.floating,
            collapsed: saved.collapsed,
            column_stacks: saved.column_stacks,
            fit_tab_groups: saved.fit_tab_groups,
            next_tile_id: saved.next_tile_id,
            next_id: saved.next_id,
            header_presentation: Default::default(),
            column_scroll: Vec::new(),
            measurements: Vec::new(),
            titlebar_insets: [0.; 3],
            bottom_inset: 0.,
        };
        layout.expand_saved_nested_columns();
        Ok(layout)
    }
}
