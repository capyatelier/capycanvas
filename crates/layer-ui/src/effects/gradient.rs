use super::*;
use layer_core::{ColorMixSpace,GradientDefinition,GradientStop};

#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum GradientDestination {
    Tool {epoch:u64},
    Effect {layer:u64,key:String,epoch:u64},
}
#[derive(Clone,Debug,PartialEq,Serialize,Deserialize)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum GradientEdit {
    Stop {index:Option<usize>,position:f32,color:Option<layer_core::color::RgbColor>,remove:bool},
    Position {index:usize,operation:NumericOperation},
    Interpolation {value:ColorMixSpace},
    Reverse,
    UseCurrentColor {index:usize},
    Reset,
}
#[derive(Clone,Debug,PartialEq,Serialize)]
pub struct GradientControls {
    pub destination:GradientDestination,
    pub can_add:bool,
    pub interpolation_label:String,
    pub interpolations:Vec<(ColorMixSpace,String)>,
    pub reverse_label:String,
}
impl GradientControls {
    pub(super) fn new(destination:GradientDestination,gradient:&GradientDefinition,l:&Localizer)->Self {
        Self {destination,can_add:gradient.stops.len()<32,interpolation_label:l.text(MessageId::RESOURCES_GRADIENT_INTERPOLATION).to_string(),
            interpolations:[(ColorMixSpace::Oklab,MessageId::RESOURCES_GRADIENT_MIX_OKLAB),(ColorMixSpace::LinearRgb,MessageId::RESOURCES_GRADIENT_MIX_LINEAR),(ColorMixSpace::Classic,MessageId::RESOURCES_GRADIENT_MIX_CLASSIC)].map(|(value,id)|(value,l.text(id).to_string())).into(),
            reverse_label:l.text(MessageId::RESOURCES_PARAMETER_GRADIENT_MAP_REVERSE).to_string()}
    }
}
impl<R:CanvasRenderer> UiSession<R> {
    pub(in crate::session) fn tool_gradient(&self)->GradientDefinition {
        self.layer_interaction.gradient.definition.clone().unwrap_or_else(|| {
            let colors=if self.selection_masks.target().is_some() {&self.selection_masks.colors} else {&self.state.colors};
            GradientDefinition::new(vec![GradientStop {position:0.,color:colors.foreground},GradientStop {position:1.,color:colors.background}])
        })
    }
    pub(in crate::session) fn gradient_tool_options(&self)->Vec<ToolOption> {
        if !matches!(self.layer_interaction.tool,LayerCanvasTool::Gradient {..}) {return Vec::new();}
        let definition=self.tool_gradient();
        let mut control=PropertyControl::new("gradient",&self.localization().text(MessageId::COMMAND_GRADIENT),PropertyKind::Gradient,EffectValue::Gradient(definition.clone()),EffectValue::Gradient(GradientDefinition::default()));
        control.gradient=Some(GradientControls::new(GradientDestination::Tool {epoch:self.state.document_file.epoch},&definition,self.localization()));
        vec![ToolOption::Choice {id:"gradient-shape",label:self.localization().text(MessageId::RESOURCES_PARAMETER_GRADIENT_FILL_STYLE),
            segmented:true,columns:None,beside:None,items:self.group_choices(ToolControlGroup::Slot(ToolSlotId::Gradient),None).into_iter().map(|(_,item)|item).collect()},ToolOption::Gradient(Box::new(control))]
    }
    pub(super) fn gradient_action(&mut self,target:GradientDestination,edit:GradientEdit)->Result<(),String> {
        let mut gradient=match &target {
            GradientDestination::Tool {epoch}=> {
                if *epoch!=self.state.document_file.epoch || !matches!(self.layer_interaction.tool,LayerCanvasTool::Gradient {..}) {return Ok(());}
                if edit == GradientEdit::Reset {self.layer_interaction.gradient.definition=None;return Ok(());}
                self.tool_gradient()
            },
            GradientDestination::Effect {layer,key,epoch}=> {
                if !self.property_editor.accepts(*layer,*epoch) {return Ok(());}
                if matches!(edit,GradientEdit::Reset) {return self.effect_action(EffectAction::Reset {layer:*layer,key:key.clone()});}
                let EffectValue::Gradient(gradient)=self.effect_parameter(*layer,key)? else {return Err(self.localization().text(MessageId::RESOURCES_ERROR_GRADIENT_REQUIRED).to_string());};gradient
            },
        };
        match edit {
            GradientEdit::Reverse=>gradient.reverse(),
            GradientEdit::UseCurrentColor {index}=> {
                let colors=if matches!(target,GradientDestination::Tool {..}) && self.selection_masks.target().is_some() {&self.selection_masks.colors} else {&self.state.colors};
                let stop=gradient.stops.get_mut(index).ok_or_else(||self.localization().text(MessageId::RESOURCES_ERROR_UNKNOWN_GRADIENT_STOP).to_string())?;
                stop.color=colors.definition();
            },
            GradientEdit::Interpolation {value}=>gradient.interpolation=value,
            GradientEdit::Position {index,operation}=> {
                let stop=gradient.stops.get(index).ok_or_else(||self.localization().text(MessageId::RESOURCES_ERROR_UNKNOWN_GRADIENT_STOP).to_string())?;
                if matches!(operation,NumericOperation::Step {steps} if steps==0.) {return Ok(());}
                let position=NumericControl::percent().resolve(f64::from(stop.position),operation).map_err(|reason|reason.message(self.localization()))?.value as f32;
                move_stop(&mut gradient,index,position);
            },
            GradientEdit::Stop {index,position,color,remove}=> {
                if !position.is_finite() {return Err(self.localization().text(MessageId::RESOURCES_ERROR_INVALID_GRADIENT_POSITION).to_string());}
                if let Some(index)=index {
                    if index>=gradient.stops.len() {return Err(self.localization().text(MessageId::RESOURCES_ERROR_UNKNOWN_GRADIENT_STOP).to_string());}
                    if remove {
                        if index>0 && index+1<gradient.stops.len() {gradient.stops.remove(index);}
                    } else {
                        move_stop(&mut gradient,index,position);
                        if let Some(color)=color {gradient.stops[index].color=color;}
                    }
                } else if !remove && gradient.stops.len()<32 {
                    let position=position.clamp(0.,1.);
                    if !gradient.stops.iter().any(|stop|stop.position==position) {
                        let color=match color {Some(color)=>color,None=>gradient.sample(position,self.engine.document().composition().color.space)?};
                        let index=gradient.stops.partition_point(|stop|stop.position<position);
                        gradient.stops.insert(index,GradientStop {position,color});
                    }
                }
            },
            _=>return Ok(()),
        }
        gradient.validate()?;
        match target {
            GradientDestination::Tool {..}=>self.layer_interaction.gradient.definition=Some(gradient),
            GradientDestination::Effect {layer,key,..}=>self.effect_action(EffectAction::Set {layer,key,value:EffectValue::Gradient(gradient)})?,
        }
        Ok(())
    }
}
fn move_stop(gradient:&mut GradientDefinition,index:usize,position:f32) {
    if index>0 && index+1<gradient.stops.len() {
        let lower=gradient.stops[index-1].position.next_up();let upper=gradient.stops[index+1].position.next_down();
        if lower<=upper {gradient.stops[index].position=position.clamp(lower,upper);}
    }
}
