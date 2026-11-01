use gtk::{gdk,glib,graphene,prelude::*,subclass::prelude::*};
use std::cell::RefCell;

#[derive(Default)]
pub struct View {state:RefCell<Option<(layer_ui::FillGestureView,layer_ui::ThemePalette)>>}
#[glib::object_subclass]
impl ObjectSubclass for View {const NAME:&'static str="CapyFillGesture";type Type=Overlay;type ParentType=gtk::Widget;}
impl ObjectImpl for View {}
impl WidgetImpl for View {
    fn snapshot(&self,snapshot:&gtk::Snapshot) {
        let state=self.state.borrow();let Some((view,palette))=state.as_ref() else{return;};
        let color=|layer_ui::HexColor([r,g,b])|gdk::RGBA::new(r as f32/255.,g as f32/255.,b as f32/255.,1.);
        let accent=color(palette.accent);let ink=color(palette.text);
        let path=gtk::gsk::PathBuilder::new();let stroke=gtk::gsk::Stroke::new(1.5);stroke.set_dash(&[4.,3.]);
        for [start,end] in view.seals.iter() {path.move_to(start[0],start[1]);path.line_to(end[0],end[1]);}
        snapshot.append_stroke(&path.to_path(),&stroke,&accent);
        if let Some(progress)=view.ring {
            let path=gtk::gsk::PathBuilder::new();path.add_circle(&graphene::Point::new(0.,0.),12.);
            let stroke=gtk::gsk::Stroke::new(2.);let length=24.*std::f32::consts::PI;stroke.set_dash(&[length*progress,length*(1.-progress)]);
            snapshot.save();snapshot.translate(&graphene::Point::new(view.tip[0],view.tip[1]));snapshot.rotate(-90.);snapshot.append_stroke(&path.to_path(),&stroke,&accent);snapshot.restore();
        }
        if let Some(pill)=&view.pill {
            let b=pill.bounds;let rect=graphene::Rect::new(b.x,b.y,b.width,b.height);
            let face=gtk::Snapshot::new();let [r,g,blue,a]=palette.glass.panel.0;
            face.push_rounded_clip(&gtk::gsk::RoundedRect::from_rect(rect,8.));face.append_color(&gdk::RGBA::new(r,g,blue,a),&rect);face.pop();
            if let Some(node)=face.to_node(){snapshot.append_node(&crate::glass::surface(node));}
            let y=b.y+b.height-7.;
            let gray=layer_ui::HexColor(std::array::from_fn(|i|((u16::from(palette.panel.0[i])+u16::from(palette.text.0[i]))/2) as u8));
            snapshot.append_color(&color(palette.input),&graphene::Rect::new(pill.track[0],y,pill.track[1]-pill.track[0],4.));
            snapshot.append_color(&color(gray),&graphene::Rect::new(pill.track[0],y,pill.fill_end-pill.track[0],4.));
            let widget=self.obj();let label=crate::color_readout::layout(widget.upcast_ref(),12.,false,&pill.label);
            let value=crate::color_readout::layout(widget.upcast_ref(),12.,false,&pill.value);
            crate::color_readout::append(snapshot,&label,[f64::from(b.x+10.),f64::from(b.y+20.)],0.,false,ink);
            crate::color_readout::append(snapshot,&value,[f64::from(b.x+b.width-10.)-crate::color_readout::advance(&value),f64::from(b.y+20.)],0.,false,ink);
        }
    }
}
glib::wrapper! {pub struct Overlay(ObjectSubclass<View>) @extends gtk::Widget,@implements gtk::Accessible,gtk::Buildable,gtk::ConstraintTarget;}
impl Overlay {
    pub fn new()->Self {let widget:Self=glib::Object::new();widget.set_can_target(false);widget.set_widget_name("fill-gesture");widget}
    pub fn resize_cursor(&self)->bool {self.imp().state.borrow().as_ref().is_some_and(|(view,_)|view.resize_cursor)}
    pub fn update(&self,view:Option<&layer_ui::FillGestureView>,palette:Option<layer_ui::ThemePalette>)->bool {
        let next=view.zip(palette).map(|(view,palette)|(view.clone(),palette));
        let geometry=|value:&Option<(layer_ui::FillGestureView,layer_ui::ThemePalette)>|value.as_ref().and_then(|(view,_)|view.pill.as_ref()).map(|pill|pill.bounds);
        let changed=geometry(&self.imp().state.borrow())!=geometry(&next);
        if *self.imp().state.borrow()!=next {*self.imp().state.borrow_mut()=next;self.queue_draw();}
        changed
    }
}
