use crate::*;
use serde::Serialize;
use serde_json::{Value,json};
use sha2::{Digest,Sha256};
use std::io::Write;

struct ShapeHash(Sha256);
impl Write for ShapeHash {
    fn write(&mut self,bytes:&[u8])->std::io::Result<usize> {self.0.update(bytes);Ok(bytes.len())}
    fn flush(&mut self)->std::io::Result<()> {Ok(())}
}
fn key(value:&impl Serialize)->String {
    let mut hash=ShapeHash(Sha256::new());serde_json::to_writer(&mut hash,value).unwrap();format!("{:x}",hash.0.finalize())
}
fn setting(value:&ToolSetting)->Value {json!([value.id,value.label,value.group_id,value.group,value.numeric,value.preview])}
fn tools(value:&ToolSetView)->Value {
    let items=|items:&[ToolSetItem]|items.iter().map(|item|json!([item.label,item.icon,item.preview.is_some()])).collect::<Vec<_>>();
    json!([items(&value.groups),items(&value.subtools)])
}
fn property(value:&PropertyControl)->Value {
    let curve=value.curve.as_ref().map(|curve|json!({"readouts":curve.coordinate_readouts,"polygon":curve.control_polygon,
        "numeric":curve.numeric,"domain":curve.domain,"help":curve.help,"reset":curve.reset_label,
        "axes":curve.axes.iter().map(|axis|(&axis.label,&axis.minimum,&axis.maximum)).collect::<Vec<_>>() }));
    let gradient=value.gradient.as_ref().map(|gradient|json!([gradient.interpolation_label,gradient.interpolations,gradient.reverse_label]));
    let paper=value.paper.as_ref().map(|paper|json!([paper.tiles.iter().map(|tile|(&tile.label,tile.actions.len())).collect::<Vec<_>>(),paper.import.is_some()]));
    json!({"key":value.key,"label":value.label,"page":value.page,"section":value.section,"section_id":value.section_id,"kind":value.kind,
        "color_action":value.color_action.is_some(),"curve":curve,"gradient":gradient,"paper":paper})
}
fn option(value:&ToolOption)->Value {
    match value {
        ToolOption::Numeric(value)=>setting(value),
        ToolOption::Range {id,label,bounds}=>json!([id,label,bounds.iter().map(setting).collect::<Vec<_>>()]),
        ToolOption::Gradient(value)=>property(value),
        ToolOption::Choice {id,label,labeled,segmented,columns,beside,items}=>json!([id,label,labeled,segmented,columns,beside,
            items.iter().map(|item|json!([item.label,item.icon,item.preview.is_some()])).collect::<Vec<_>>()]),
        ToolOption::Action {state,checkable}=>json!([state.id,state.label,state.icon,checkable]),
    }
}
fn scope(value:&HistogramView)->Value {json!([value.sources,value.channels,value.labels,value.axis])}
fn body(state:&UiState,panel:Panel)->Value {
    match panel {
        Panel::Brushes|Panel::BrushSets|Panel::SculptSets|Panel::Tools=>json!([tools(state.tool_panel(panel)),
            state.tool_extra.iter().any(|extra|matches!(extra,ToolOption::Choice {id:"tonal-tones",..}))]),
        Panel::ToolSettings=>json!({"picker":state.layer_tools.tool.picks_color(),
            "calibrating":state.color_picker.calibrating,"can_sample_layer":state.color_picker.can_sample_layer,"sample_sizes":state.color_picker.sample_sizes,
            "settings":state.tool_settings.iter().map(setting).collect::<Vec<_>>(),"actions":state.tool_actions,
            "extra":state.tool_extra.iter().map(option).collect::<Vec<_>>() }),
        Panel::Color=>json!(state.display_colors().has_hdr_picker()),
        Panel::Palettes=>json!([state.color_library.palettes.iter().find(|p|p.id==state.color_library.active).map(|p|p.swatches.len()),
            ColorLibrary::color_detail(state.display_colors())]),
        Panel::Layers=>json!([state.layers.iter().map(|layer|(&layer.label,&layer.description,layer.depth,layer.collapsed)).collect::<Vec<_>>(),
            state.layer_tools.add_paper.is_some(),state.layer_tools.footer.iter().map(|slot|(&slot.button,slot.min_width,&slot.item.label)).collect::<Vec<_>>()]),
        Panel::Adjustments=>json!([state.adjustments.iter().map(|choice|(&choice.label,&choice.category,&choice.category_label)).collect::<Vec<_>>(),
            state.filter_picker.search.is_some(),state.filter_picker.empty_label]),
        Panel::FilterTypes=>json!(state.filter_categories.iter().map(|category|&category.label).collect::<Vec<_>>()),
        Panel::Properties=>{
            let view=&state.layer_properties;
            json!({"title":view.title,"layer_type":view.layer_type,"filter":view.add_filter.as_ref().map(|menu|&menu.title),
                "histogram":view.histogram,"pages":view.pages,"page":view.page,"resource":view.resource_label,
                "actions":view.actions.iter().map(|action|(&action.label,&action.icon,&action.group)).collect::<Vec<_>>(),
                "controls":view.controls.iter().map(property).collect::<Vec<_>>()})
        },
        Panel::Navigator=>json!(state.tabs.first().map(|tab|(tab.width,tab.height))),
        Panel::Histogram=>scope(&state.histogram),Panel::Waveform=>scope(&state.waveform),
        Panel::Proof=>json!([state.soft_proof,state.preview_sdr,state.display_colors().has_hdr_picker()]),
        _=>Value::Null,
    }
}
pub(crate) fn identities(state:&UiState)->Vec<PanelMeasurementIdentity> {
    state.workspace.layout.panels.iter().map(|panel| {
        let tab=state.workspace.layout.tab_presentation(panel.id);
        let style=(state.theme,state.localization.language());
        let tab=key(&(panel.title_localized(&state.localization),panel.icon(),tab.show_icon,tab.show_name,style));
        let cacheable=!matches!(panel.id,Panel::Proof|Panel::Stats|Panel::Palettes);
        let body=key(&(&panel.content,panel.tile_style,style,body(state,panel.id)));
        PanelMeasurementIdentity {panel:panel.id,tab,body,cacheable}
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state()->UiState {crate::session::test_support::session(Platform::Windows).state().clone()}
    fn identity(state:&UiState,panel:Panel)->PanelMeasurementIdentity {identities(state).into_iter().find(|identity|identity.panel==panel).unwrap()}
    #[test]
    fn numeric_and_scope_publications_keep_intrinsic_keys_but_copy_and_rows_change_them() {
        let mut state=state();let initial=identities(&state);
        state.revision+=100;state.brush.diameter+=12.;state.brush.opacity=0.25;
        state.histogram.captured_time=Some(12.);state.histogram.status="Updating".into();
        state.histogram.description="new accessibility description".into();state.histogram.range="new range".into();
        assert_eq!(initial,identities(&state));
        let before=identity(&state,Panel::Layers);state.layers.push(state.layers[0].clone());
        assert_ne!(before.body,identity(&state,Panel::Layers).body);assert_eq!(before.tab,identity(&state,Panel::Layers).tab);
        let before=identity(&state,Panel::Histogram);state.histogram.labels.push("A wrapping checkbox label".into());
        assert_ne!(before.body,identity(&state,Panel::Histogram).body);
        let before=identity(&state,Panel::Color);state.theme=if state.theme==Theme::Dark {Theme::Light}else{Theme::Dark};
        assert_ne!(before,identity(&state,Panel::Color));
        let before=identity(&state,Panel::Color);
        state.localization=Localizer::shared(*UiLanguage::ALL.iter().find(|language|**language!=state.localization.language()).unwrap());
        assert_ne!(before,identity(&state,Panel::Color));
    }
    #[test]
    fn property_section_identity_changes_height_key_even_with_equal_heading_text() {
        use layer_core::{EffectValue,ResourceLabel};
        let mut state=state();
        let mut control=PropertyControl {curve:None,gradient:None,paper:None,page:None,plot:Vec::new(),key:"example".into(),label:"Visible".into(),section:None,section_id:None,kind:PropertyKind::Toggle,value:EffectValue::Toggle(false),default:EffectValue::Toggle(false),modified:false,color_action:None,disabled:false};
        control.section=Some("Same heading".into());control.section_id=Some(ResourceLabel::Literal("section-a".into()));
        state.layer_properties.controls=vec![control];let before=identity(&state,Panel::Properties);
        state.layer_properties.controls[0].section_id=Some(ResourceLabel::Literal("section-b".into()));
        assert_ne!(before.body,identity(&state,Panel::Properties).body);
        let before=identity(&state,Panel::Properties);state.layer_properties.controls[0].value=EffectValue::Toggle(true);
        assert_eq!(before.body,identity(&state,Panel::Properties).body);
    }
}
