use super::*;
use layer_core::GradientDefinition;
use layer_ui::{GradientControls,GradientEdit};

pub(crate) struct GradientEditor {
    pub root:gtk::Box,
    pub position:NumberControl,
    value:Rc<RefCell<GradientDefinition>>,
    controls:Rc<RefCell<GradientControls>>,
    sync:Rc<dyn Fn()>,
}
impl GradientEditor {
    pub fn new(w:&Rc<Workspace>,control:&layer_ui::PropertyControl)->Self {
        let root=gtk::Box::new(gtk::Orientation::Vertical,6);
        root.add_css_class("gradient-editor");
        let bar=gradient_preview::GradientPreview::new();bar.set_widget_name("effect-gradient");bar.set_focusable(true);
        let value=Rc::new(RefCell::new(GradientDefinition::default()));
        let controls=Rc::new(RefCell::new(control.gradient.clone().expect("shared gradient controls")));
        let selected=Rc::new(Cell::new(0usize));let updating=Rc::new(Cell::new(false));
        let color=crate::color_editor::ColorButton::new();color.bind_copy(w);color.widget.set_widget_name("effect-gradient-color");
        let position=NumberControl::value_only(layer_ui::NumericControl::percent(),"",w.localization());
        position.set_widget_name("effect-gradient-position");position.set_hexpand(true);position.set_valign(gtk::Align::Center);
        let remove=crate::icons::button("layer-minus-symbolic");remove.set_widget_name("effect-gradient-remove");
        let reset=crate::icons::button("layer-reset-symbolic");reset.set_widget_name("effect-gradient-reset");
        let interpolation=crate::panel_controls::dropdown(&[]);interpolation.set_widget_name("gradient-interpolation");interpolation.set_hexpand(true);
        let reverse=crate::icons::button("layer-flip-horizontal-symbolic");reverse.set_widget_name("gradient-reverse");
        let bucket=crate::icons::button("layer-fill-symbolic");bucket.set_widget_name("effect-gradient-use-color");
        let top=gtk::Box::new(gtk::Orientation::Horizontal,6);top.append(&interpolation);top.append(&reverse);top.append(&reset);
        let stop=gtk::Box::new(gtk::Orientation::Horizontal,6);stop.append(&position);stop.append(&remove);stop.append(&color.widget);stop.append(&bucket);
        root.append(&top);root.append(&bar);root.append(&stop);
        w.on_localization(glib::clone!(#[weak] bar,#[weak] position,#[weak] remove,#[weak] reset,#[weak] bucket,#[upgrade_or] false,move |l| {
            let copy=layer_ui::NativeCopy::new(l).color;
            bar.set_tooltip_text(Some(&copy.add_stop));bar.update_property(&[gtk::accessible::Property::Label(&copy.add_stop)]);
            position.set_caption(&copy.position,"",l.clone());
            for (button,label) in [(&remove,&copy.remove_stop),(&reset,&copy.reset_gradient),(&bucket,&copy.use_selected)] {button.set_tooltip_text(Some(label));button.update_property(&[gtk::accessible::Property::Label(label)]);}
            true
        }));
        let action=Rc::new(glib::clone!(#[strong] controls,move |edit| EffectAction::Gradient {target:controls.borrow().destination.clone(),edit}));
        let dispatch:Rc<dyn Fn(GradientEdit,Option<ContactPhase>)>=Rc::new(glib::clone!(#[weak] w,#[strong] action,move |edit,phase| {
            let action=action(edit);w.dispatch(UiAction::Effect {action:phase.map_or_else(||action.clone(),|phase|EffectAction::Gesture {phase,action:Box::new(action.clone())})});
        }));
        let sync:Rc<dyn Fn()>=Rc::new(glib::clone!(#[weak] w,#[strong] value,#[strong] controls,#[strong] selected,#[strong] updating,#[strong] color,
            #[weak] bar,#[weak] position,#[weak] remove,#[weak] interpolation,#[weak] reverse,move || {
            updating.set(true);
            let gradient=value.borrow();let c=controls.borrow();let index=selected.get().min(gradient.stops.len()-1);selected.set(index);
            let stop=&gradient.stops[index];color.set_color(stop.color,w.view_color());position.set_value(stop.position.into());
            let interior=index>0 && index+1<gradient.stops.len();position.set_sensitive(interior);remove.set_sensitive(interior);
            let labels=c.interpolations.iter().map(|(_,label)|label.as_str()).collect::<Vec<_>>();
            let model=interpolation.model().unwrap().downcast::<gtk::StringList>().unwrap();
            if labels.len()!=model.n_items() as usize || labels.iter().enumerate().any(|(i,label)|model.string(i as u32).as_deref()!=Some(*label)) {model.splice(0,model.n_items(),&labels);}
            interpolation.set_selected(c.interpolations.iter().position(|(mode,_)|*mode==gradient.interpolation).unwrap_or(0) as u32);
            interpolation.set_tooltip_text(Some(&c.interpolation_label));interpolation.update_property(&[gtk::accessible::Property::Label(&c.interpolation_label)]);
            reverse.set_tooltip_text(Some(&c.reverse_label));reverse.update_property(&[gtk::accessible::Property::Label(&c.reverse_label)]);
            if let Some(g)=w.gpu.borrow().as_ref() {bar.set_gradient(&gradient,index,g.session.engine().document().composition().color,w.view_color());}
            updating.set(false);
        }));
        interpolation.connect_selected_notify(glib::clone!(#[strong] updating,#[strong] controls,#[strong] dispatch,move |drop| {
            let choice=controls.borrow().interpolations.get(drop.selected() as usize).map(|(value,_)|*value);
            if !updating.get() && let Some(value)=choice {dispatch(GradientEdit::Interpolation {value},None);}
        }));
        reverse.connect_clicked(glib::clone!(#[strong] selected,#[strong] value,#[strong] dispatch,move |_| {
            selected.set(value.borrow().stops.len()-1-selected.get());dispatch(GradientEdit::Reverse,None);
        }));
        bucket.connect_clicked(glib::clone!(#[strong] selected,#[strong] dispatch,move |_|dispatch(GradientEdit::UseCurrentColor {index:selected.get()},None)));
        let capture=Rc::new(Cell::new(None::<(f32,f64,f32)>));
        let held=Rc::new(Cell::new(None::<gtk::gdk::Key>));
        let drag=gtk::GestureDrag::new();drag.set_button(1);
        drag.connect_drag_begin(glib::clone!(#[weak] bar,#[strong] value,#[strong] controls,#[strong] selected,#[strong] capture,#[strong] dispatch,#[strong] sync,move |gesture,x,_| {
            let Some((screen_x,_))=gesture.current_event().and_then(|event|event.position()) else {return;};
            let width=(bar.width() as f32-12.).max(1.);let position=((x as f32-6.)/width).clamp(0.,1.);
            let gradient=value.borrow();let existing=gradient.stops.iter().position(|stop|(stop.position-position).abs()*width<8.);
            if existing.is_none() && !controls.borrow().can_add {return;}
            let index=existing.unwrap_or_else(||gradient.stops.partition_point(|stop|stop.position<position));
            let position=existing.map_or(position,|i|gradient.stops[i].position);drop(gradient);
            selected.set(index);bar.grab_focus();capture.set(Some((position,screen_x,width)));gesture.set_state(gtk::EventSequenceState::Claimed);
            dispatch(GradientEdit::Stop {index:existing,position,color:None,remove:false},Some(ContactPhase::Down));sync();
        }));
        for (phase,ending) in [(ContactPhase::Move,false),(ContactPhase::Up,true)] {
            let callback=glib::clone!(#[strong] capture,#[strong] selected,#[strong] dispatch,move |gesture:&gtk::GestureDrag,_,_| {
                let Some((position,sx,width))=(if ending {capture.take()} else {capture.get()}) else {return;};
                let Some((px,_))=gesture.current_event().and_then(|event|event.position()) else {if ending {dispatch(GradientEdit::Reset,Some(ContactPhase::Cancel));}return;};
                dispatch(GradientEdit::Stop {index:Some(selected.get()),position:(position+(px-sx) as f32/width).clamp(0.,1.),color:None,remove:false},Some(phase));
            });
            if ending {drag.connect_drag_end(callback);} else {drag.connect_drag_update(callback);}
        }
        let cancel=Rc::new(glib::clone!(#[strong] capture,#[strong] held,#[strong] dispatch,move || if capture.take().is_some() | held.take().is_some() {dispatch(GradientEdit::Stop {index:None,position:0.,color:None,remove:false},Some(ContactPhase::Cancel));}));
        drag.connect_cancel(glib::clone!(#[strong] cancel,move |_,_|cancel()));bar.add_controller(drag);
        bar.connect_unmap(glib::clone!(#[strong] cancel,move |_|cancel()));
        let focus=gtk::EventControllerFocus::new();focus.connect_leave(glib::clone!(#[strong] cancel,move |_|cancel()));bar.add_controller(focus);
        let keys=gtk::EventControllerKey::new();keys.connect_key_pressed(glib::clone!(#[strong] selected,#[strong] held,#[strong] dispatch,#[strong] cancel,move |_,key,_,modifiers| {
            use gtk::gdk::Key;
            match key {
                Key::Escape=>cancel(),
                Key::Delete|Key::BackSpace=>{cancel();dispatch(GradientEdit::Stop {index:Some(selected.get()),position:0.,color:None,remove:true},None);},
                Key::Left|Key::Right=> {
                    let phase=if held.replace(Some(key)).is_some() {ContactPhase::Move} else {ContactPhase::Down};
                    dispatch(GradientEdit::Position {index:selected.get(),operation:layer_ui::NumericOperation::Step {steps:if key==Key::Left {-1.} else {1.}*if modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK) {10.} else {1.}}},Some(phase));
                },
                _=>return glib::Propagation::Proceed,
            };glib::Propagation::Stop
        }));
        keys.connect_key_released(glib::clone!(#[strong] held,#[strong] selected,#[strong] dispatch,move |_,key,_,_| {
            if held.get()==Some(key) {held.set(None);
                dispatch(GradientEdit::Position {index:selected.get(),operation:layer_ui::NumericOperation::Step {steps:0.}},Some(ContactPhase::Up));}
        }));bar.add_controller(keys);
        color.widget.connect_clicked(glib::clone!(#[weak] w,#[weak] color,#[strong] selected,#[strong] value,#[strong] controls,#[strong] dispatch,move |_| {
            let index=selected.get();let original=value.borrow().clone();let Some(stop)=original.stops.get(index) else {return;};
            let target=controls.borrow().destination.clone();let controls=controls.clone();
            let selected=selected.clone();let value=value.clone();let dispatch=dispatch.clone();let weak=Rc::downgrade(&color);
            crate::color_editor::choose(&w,stop.color,move |_,color| {if controls.borrow().destination==target && weak.upgrade().is_some_and(|b|b.widget.root().is_some()) && selected.get()==index && *value.borrow()==original {
                dispatch(GradientEdit::Stop {index:Some(index),position:original.stops[index].position,color:Some(color),remove:false},None);
            }});
        }));
        bind_number(&position,w,glib::clone!(#[strong] selected,#[strong] action,move |value|action(GradientEdit::Position {index:selected.get(),operation:layer_ui::NumericOperation::Value {value}})));
        remove.connect_clicked(glib::clone!(#[strong] selected,#[strong] dispatch,move |_| {let index=selected.get();selected.set(index.saturating_sub(1));dispatch(GradientEdit::Stop {index:Some(index),position:0.,color:None,remove:true},None);}));
        reset.connect_clicked(glib::clone!(#[strong] dispatch,move |_|dispatch(GradientEdit::Reset,None)));
        Self {root,position,value,controls,sync}
    }
    pub fn update_control(&self,control:&layer_ui::PropertyControl) {
        if let EffectValue::Gradient(value)=&control.value {self.update(value,control.gradient.as_ref().expect("shared gradient controls"));}
    }
    pub fn update(&self,value:&GradientDefinition,controls:&GradientControls) {
        *self.value.borrow_mut()=value.clone();*self.controls.borrow_mut()=controls.clone();(self.sync)();
    }
}

pub(crate) struct GradientButton {
    pub root:gtk::MenuButton,
    editor:GradientEditor,
    preview:gradient_preview::GradientPreview,
    workspace:std::rc::Weak<Workspace>,
}
impl GradientButton {
    pub fn new(w:&Rc<Workspace>,control:&layer_ui::PropertyControl)->Self {
        let editor=GradientEditor::new(w,control);
        editor.root.set_size_request(240,-1);
        let popover=gtk::Popover::new();popover.set_child(Some(&editor.root));
        w.watch_popover(&popover);
        let preview=gradient_preview::GradientPreview::new();preview.set_compact(true);
        let root=gtk::MenuButton::new();root.set_child(Some(&preview));root.set_popover(Some(&popover));
        root.set_widget_name("toolbar-gradient");root.set_size_request(120,-1);
        let field=Self {root,editor,preview,workspace:Rc::downgrade(w)};
        field.update(control);field
    }
    pub fn update(&self,control:&layer_ui::PropertyControl) {
        self.root.set_tooltip_text(Some(&control.label));self.root.update_property(&[gtk::accessible::Property::Label(&control.label)]);
        self.editor.update_control(control);
        if let EffectValue::Gradient(value)=&control.value && let Some(w)=self.workspace.upgrade() && let Some(g)=w.gpu.borrow().as_ref() {
            self.preview.set_gradient(value,0,g.session.engine().document().composition().color,w.view_color());
        }
    }
}
