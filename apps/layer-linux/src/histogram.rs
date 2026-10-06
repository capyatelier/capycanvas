use crate::workspace::Workspace;
use gtk::prelude::*;
use gtk::glib;
use layer_ui::{HistogramAction, HistogramView, UiAction, UiState};
use std::{cell::{Cell, RefCell}, rc::{Rc, Weak}};

pub(crate) struct Footer {
    pub root: gtk::Box,
    status: gtk::Label,
    clipping: [gtk::ToggleButton; 2],
    updating: Rc<Cell<bool>>,
    workspace: Rc<RefCell<Weak<Workspace>>>,
}
impl Footer {
    pub fn new(prefix: &str) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let status = gtk::Label::builder().xalign(0.).hexpand(true).width_chars(1)
            .ellipsize(gtk::pango::EllipsizeMode::End).build();
        status.add_css_class("dim-label");status.set_widget_name(&format!("{prefix}-status"));root.append(&status);
        let updating = Rc::new(Cell::new(false));
        let workspace = Rc::new(RefCell::new(Weak::<Workspace>::new()));
        let buttons=crate::panel_controls::action_row();buttons.set_spacing(0);buttons.add_css_class("linked");root.append(&buttons);
        let clipping = std::array::from_fn(|index| {
            let button = gtk::ToggleButton::new();button.add_css_class("flat");
            let name = if index == 0 {"shadows"} else {"highlights"};
            button.set_widget_name(&format!("{prefix}-{name}"));
            crate::icons::set_button(&button,&format!("layer-tonal-{name}-symbolic"));
            button.connect_toggled(glib::clone!(#[strong] updating, #[strong] workspace, move |button| {
                let owner=workspace.borrow().upgrade();
                if !updating.get() && let Some(w) = owner {
                    w.dispatch(UiAction::Histogram {action: if index == 0 {HistogramAction::Shadows {enabled:button.is_active()}}
                        else {HistogramAction::Highlights {enabled:button.is_active()}}});
                }
            }));buttons.append(&button);button
        });
        Self {root,status,clipping,updating,workspace}
    }
    pub fn refresh(&self,w:&Rc<Workspace>,state:&UiState,status:&str) {
        *self.workspace.borrow_mut() = Rc::downgrade(w);self.updating.set(true);
        self.status.set_label(status);self.status.set_tooltip_text(Some(status));
        for (index,button) in self.clipping.iter().enumerate() {
            if let Some(label) = state.histogram.labels.get(index+1) {
                button.set_tooltip_text(Some(label));button.update_property(&[gtk::accessible::Property::Label(label)]);
            }
            button.set_active(if index == 0 {state.histogram.shadows} else {state.histogram.highlights});
        }
        self.updating.set(false);
    }
}

pub(crate) struct Inspector {
    pub root: gtk::Box,
    source: gtk::DropDown,
    channel: gtk::DropDown,
    chart: gtk::DrawingArea,
    toolbar: gtk::Box,
    footer: Footer,
    axis: [gtk::Label; 2],
    logarithmic: gtk::CheckButton,
    view: RefCell<HistogramView>,
    colors: Cell<[[u8; 3]; 4]>,
    updating: Cell<bool>,
    workspace: RefCell<Weak<Workspace>>,
    waveform: bool,
    plot: RefCell<Option<gtk::cairo::ImageSurface>>,
}
impl Inspector {
    pub fn new() -> Rc<Self> {Self::create(false)}
    pub fn waveform() -> Rc<Self> {Self::create(true)}
    fn create(waveform:bool) -> Rc<Self> {
        let prefix=if waveform {"waveform"} else {"histogram"};
        let root = crate::panel_controls::column();root.set_widget_name(&format!("{prefix}-panel"));
        root.set_margin_top(6);root.set_margin_bottom(6);root.set_margin_start(6);root.set_margin_end(6);
        let source = crate::panel_controls::dropdown(&[]);source.set_widget_name(&format!("{prefix}-source"));source.set_hexpand(true);
        let channel = crate::panel_controls::dropdown(&[]);channel.set_widget_name(&format!("{prefix}-channel"));channel.set_hexpand(true);
        let toolbar = crate::panel_controls::action_row();toolbar.set_homogeneous(true);toolbar.append(&source);toolbar.append(&channel);root.append(&toolbar);
        let chart = gtk::DrawingArea::builder().content_height(160).hexpand(true).build();chart.set_widget_name(&format!("{prefix}-chart"));
        let axis = [gtk::Label::builder().xalign(0.).hexpand(true).build(),gtk::Label::builder().xalign(1.).build()];
        if waveform {
            let graph=gtk::Overlay::new();graph.set_child(Some(&chart));
            for (index,label) in axis.iter().enumerate() {
                label.set_hexpand(false);label.set_halign(gtk::Align::Start);
                label.set_valign(if index==0 {gtk::Align::End} else {gtk::Align::Start});
                label.set_margin_start(3);label.set_can_target(false);graph.add_overlay(label);
            }
            root.append(&graph);
        } else {
            let axis_row = gtk::Box::new(gtk::Orientation::Horizontal,0);axis_row.add_css_class("dim-label");for label in &axis {axis_row.append(label);}
            root.append(&chart);root.append(&axis_row);
        }
        let logarithmic = crate::panel_controls::check("");logarithmic.set_widget_name(&format!("{prefix}-log"));
        root.append(&logarithmic);
        let footer = Footer::new(prefix);root.append(&footer.root);
        let panel = Rc::new(Self {root,source,channel,chart,toolbar,footer,axis,logarithmic,
            view:RefCell::default(),colors:Cell::new([[0;3];4]),updating:Cell::new(false),workspace:RefCell::default(),waveform,plot:RefCell::default()});
        for (index,dropdown) in [&panel.source,&panel.channel].into_iter().enumerate() {
            let weak = Rc::downgrade(&panel);
            dropdown.connect_selected_notify(move |dropdown| {if let Some(panel) = weak.upgrade() {
                panel.dispatch(if index == 0 {HistogramAction::Source {index:dropdown.selected() as u8}}
                    else if panel.waveform {HistogramAction::WaveformChannel {index:dropdown.selected() as u8}}
                    else {HistogramAction::Channel {index:dropdown.selected() as u8}});
            }});
        }
        let weak = Rc::downgrade(&panel);
        panel.logarithmic.connect_toggled(move |button| {if let Some(panel) = weak.upgrade() {
            panel.dispatch(if panel.waveform {HistogramAction::WaveformLogarithmic {enabled:button.is_active()}}
                else {HistogramAction::Logarithmic {enabled:button.is_active()}});
        }});
        let weak = Rc::downgrade(&panel);
        panel.chart.set_draw_func(move |_,cr,width,height| {if let Some(panel) = weak.upgrade() {
            if panel.waveform {
                if let Some(plot)=panel.plot.borrow().as_ref() {
                    let _=cr.save();cr.scale(f64::from(width)/f64::from(plot.width()),f64::from(height)/f64::from(plot.height()));
                    let _=cr.set_source_surface(plot,0.,0.);cr.source().set_filter(gtk::cairo::Filter::Nearest);let _=cr.paint();let _=cr.restore();
                }
            } else {draw(cr,&panel.view.borrow(),panel.colors.get(),f64::from(width),f64::from(height));}
        }});
        panel
    }
    fn dispatch(&self,action:HistogramAction) {
        let owner=self.workspace.borrow().upgrade();
        if !self.updating.get() && let Some(w) = owner {w.dispatch(UiAction::Histogram {action});}
    }
    pub fn duplicate(&self,w:&Rc<Workspace>) -> Rc<Self> {let panel=Self::create(self.waveform);*panel.workspace.borrow_mut()=Rc::downgrade(w);panel}
    pub fn refresh(&self,w:&Rc<Workspace>,state:&UiState) {self.refresh_view(w,state,if self.waveform {&state.waveform} else {&state.histogram});}
    pub fn refresh_tonal(&self,w:&Rc<Workspace>,state:&UiState) {
        self.refresh_view(w,state,&state.tonal_histogram);self.toolbar.set_visible(false);self.logarithmic.set_visible(false);self.chart.set_content_height(120);
        self.root.set_margin_top(0);self.root.set_margin_bottom(0);self.root.set_margin_start(0);self.root.set_margin_end(0);
    }
    fn refresh_view(&self,w:&Rc<Workspace>,state:&UiState,view:&HistogramView) {
        *self.workspace.borrow_mut()=Rc::downgrade(w);self.updating.set(true);
        let previous=self.view.borrow();
        let colors=state.palette.histogram_colors().map(|color|color.0);
        if self.waveform && (previous.channel!=view.channel || previous.logarithmic!=view.logarithmic || self.colors.get()!=colors
            || previous.data.as_ref().map(std::sync::Arc::as_ptr)!=view.data.as_ref().map(std::sync::Arc::as_ptr)) {
            *self.plot.borrow_mut()=view.waveform_premultiplied_rgba(colors).and_then(|([width,height],mut rgba)| {
                for pixel in rgba.chunks_exact_mut(4) {
                    let packed=u32::from_be_bytes([pixel[3],pixel[0],pixel[1],pixel[2]]);pixel.copy_from_slice(&packed.to_ne_bytes());
                }
                gtk::cairo::ImageSurface::create_for_data(rgba,gtk::cairo::Format::ARgb32,width as i32,height as i32,width as i32*4).ok()
            });
        }
        for (dropdown,before,after) in [(&self.source,&previous.sources,&view.sources),(&self.channel,&previous.channels,&view.channels)] {
            if before!=after {
                let model=dropdown.model().unwrap().downcast::<gtk::StringList>().unwrap();
                model.splice(0,model.n_items(),&after.iter().map(|s|s.as_ref()).collect::<Vec<_>>());
            }
        }
        drop(previous);
        self.source.set_selected(u32::from(view.source));self.channel.set_selected(u32::from(view.channel));
        for (control,id) in [(&self.source,layer_ui::MessageId::TOOLBAR_SOURCE),(&self.channel,layer_ui::MessageId::NATIVE_COLOR_CHANNEL)] {
            let label=w.localization().text(id);control.set_tooltip_text(Some(&label));control.update_property(&[gtk::accessible::Property::Label(&label)]);
        }
        let details=format!("{}\n{}",view.description,view.range);self.chart.set_tooltip_text(Some(&details));
        for (label,text) in self.axis.iter().zip(&view.axis) {label.set_label(text);}
        self.logarithmic.set_label(view.labels.first().map(|s|s.as_ref()));self.logarithmic.set_active(view.logarithmic);
        self.footer.refresh(w,state,&view.status);self.colors.set(colors);
        *self.view.borrow_mut()=view.clone();self.chart.queue_draw();self.updating.set(false);
    }
}

pub(crate) fn draw(cr:&gtk::cairo::Context,view:&HistogramView,colors:[[u8;3];4],width:f64,height:f64) {
    for (channel,bins) in view.histogram_plot() {
        let [r,g,b]=colors[channel].map(|v|f64::from(v)/255.);cr.set_source_rgba(r,g,b,0.55);
        for (x,&value) in bins.iter().enumerate() {
            let h=height*f64::from(value);cr.rectangle(x as f64*width/bins.len() as f64,height-h,width/bins.len() as f64+0.1,h);
        }
        let _=cr.fill();
    }
}
