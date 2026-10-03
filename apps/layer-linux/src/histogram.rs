use crate::workspace::Workspace;
use gtk::prelude::*;
use layer_ui::{HistogramAction, HistogramView, UiAction, UiState};
use std::{cell::{Cell, RefCell}, rc::{Rc, Weak}};

pub(crate) struct Inspector {
    pub root: gtk::Box,
    source: gtk::DropDown,
    channel: gtk::DropDown,
    chart: gtk::DrawingArea,
    status: gtk::Label,
    description: gtk::Label,
    range: gtk::Label,
    axis: [gtk::Label; 2],
    options: [gtk::CheckButton; 3],
    view: RefCell<HistogramView>,
    colors: Cell<[[u8; 3]; 4]>,
    updating: Cell<bool>,
    workspace: RefCell<Weak<Workspace>>,
}
impl Inspector {
    pub fn new() -> Rc<Self> {
        let root = crate::panel_controls::column();
        root.set_widget_name("histogram-panel");
        root.set_margin_top(8);root.set_margin_bottom(8);root.set_margin_start(8);root.set_margin_end(8);
        let source = gtk::DropDown::from_strings(&[]);source.set_widget_name("histogram-source");
        let channel = gtk::DropDown::from_strings(&[]);channel.set_widget_name("histogram-channel");
        let chart = gtk::DrawingArea::builder().content_height(120).hexpand(true).build();chart.set_widget_name("histogram-chart");
        let status = gtk::Label::builder().xalign(0.).wrap(true).build();status.set_widget_name("histogram-status");
        let description = gtk::Label::builder().xalign(0.).wrap(true).build();description.set_widget_name("histogram-description");
        description.add_css_class("dim-label");
        let range = gtk::Label::builder().xalign(0.).wrap(true).build();range.set_widget_name("histogram-range");
        let axis = [gtk::Label::builder().xalign(0.).hexpand(true).build(),gtk::Label::builder().xalign(1.).build()];
        let axis_row = gtk::Box::new(gtk::Orientation::Horizontal,0);for label in &axis {axis_row.append(label);}
        let options = std::array::from_fn(|_| gtk::CheckButton::new());
        root.append(&source);root.append(&channel);root.append(&chart);root.append(&axis_row);root.append(&status);
        for (button, name) in options.iter().zip(["histogram-log", "histogram-shadows", "histogram-highlights"]) {button.set_widget_name(name);root.append(button);}
        let details = gtk::Box::new(gtk::Orientation::Vertical,8);details.append(&range);details.append(&description);
        let expander = gtk::Expander::builder().child(&details).build();expander.set_widget_name("histogram-details");root.append(&expander);
        let panel = Rc::new(Self { root, source, channel, chart, status, description, range, axis, options, view: RefCell::default(),
            colors: Cell::new([[0;3];4]), updating: Cell::new(false), workspace: RefCell::default() });
        for (index, dropdown) in [&panel.source, &panel.channel].into_iter().enumerate() {
            let weak = Rc::downgrade(&panel);
            dropdown.connect_selected_notify(move |dropdown| {if let Some(panel) = weak.upgrade() {
                panel.dispatch(if index == 0 { HistogramAction::Source { index: dropdown.selected() as u8 } }
                    else { HistogramAction::Channel { index: dropdown.selected() as u8 } });
            }});
        }
        for (index, button) in panel.options.iter().enumerate() {
            let weak = Rc::downgrade(&panel);
            button.connect_toggled(move |button| {if let Some(panel) = weak.upgrade() {
                let enabled = button.is_active();panel.dispatch(match index {0 => HistogramAction::Logarithmic { enabled },
                    1 => HistogramAction::Shadows { enabled }, _ => HistogramAction::Highlights { enabled }});
            }});
        }
        let weak = Rc::downgrade(&panel);
        panel.chart.set_draw_func(move |_, cr, width, height| {if let Some(panel) = weak.upgrade() {
            draw(cr, &panel.view.borrow(), panel.colors.get(), f64::from(width), f64::from(height));
        }});
        panel
    }
    fn dispatch(&self, action: HistogramAction) {
        if self.updating.get() {return;}
        let workspace = self.workspace.borrow().upgrade();
        if let Some(w) = workspace {w.dispatch(UiAction::Histogram { action });}
    }
    pub fn duplicate(&self, w: &Rc<Workspace>) -> Rc<Self> {
        let panel = Self::new();*panel.workspace.borrow_mut() = Rc::downgrade(w);panel
    }
    pub fn refresh(&self, w: &Rc<Workspace>, state: &UiState) {
        *self.workspace.borrow_mut() = Rc::downgrade(w);
        self.refresh_view(w,state,&state.histogram);
    }
    pub fn refresh_tonal(&self,w:&Rc<Workspace>,state:&UiState) {
        *self.workspace.borrow_mut()=Rc::downgrade(w);
        let mut view=state.tonal_histogram.clone();
        view.labels=state.histogram.labels.clone();view.shadows=state.histogram.shadows;view.highlights=state.histogram.highlights;
        view.axis=["0".into(),"1".into()];
        self.refresh_view(w,state,&view);
        self.source.set_visible(false);self.channel.set_visible(false);self.options[0].set_visible(false);
        if let Some(details)=self.root.last_child() {details.set_visible(false);}
    }
    fn refresh_view(&self,w:&Rc<Workspace>,state:&UiState,view:&HistogramView) {
        self.updating.set(true);
        let source_changed = self.view.borrow().sources != view.sources;
        let channel_changed = self.view.borrow().channels != view.channels;
        for (dropdown, changed, after) in [(&self.source,source_changed,&view.sources),(&self.channel,channel_changed,&view.channels)] {
            if changed {
                let model = dropdown.model().unwrap().downcast::<gtk::StringList>().unwrap();
                model.splice(0, model.n_items(), &after.iter().map(|s| s.as_ref()).collect::<Vec<_>>());
            }
        }
        self.source.set_selected(u32::from(view.source));self.channel.set_selected(u32::from(view.channel));
        self.status.set_label(&view.status);self.description.set_label(&view.description);self.range.set_label(&view.range);
        for (label,text) in self.axis.iter().zip(&view.axis) {label.set_label(text);}
        for (index,button) in self.options.iter().enumerate() {
            button.set_label(view.labels.get(index).map(|s|s.as_ref()));
            button.set_active([view.logarithmic,view.shadows,view.highlights][index]);
        }
        if let Some(expander) = self.root.last_child().and_downcast::<gtk::Expander>() { expander.set_label(Some(&w.localization().text(layer_ui::MessageId::NATIVE_COLOR_DETAILS))); }
        self.colors.set(state.palette.histogram_colors().map(|color| color.0));
        *self.view.borrow_mut() = view.clone();self.chart.queue_draw();self.updating.set(false);
    }
}

pub(crate) fn draw(cr:&gtk::cairo::Context,view:&HistogramView,colors:[[u8;3];4],width:f64,height:f64) {
    let Some(data) = &view.data else {return;};
    let channels: &[usize] = match view.channel {1 => &[0],2 => &[1],3 => &[2],4 => &[3],_ => &[0,1,2]};
    let plot = data.plot_bins();let scale = |v:u64| if view.logarithmic {(v as f64).ln_1p()} else {v as f64};
    let maximum = channels.iter().flat_map(|i| &data.channels[*i].bins[plot.clone()]).copied().max().unwrap_or(1).max(1);
    for &channel in channels {
        let [r,g,b] = colors[channel].map(|v| f64::from(v)/255.);cr.set_source_rgba(r,g,b,0.55);
        for (x,&count) in data.channels[channel].bins[plot.clone()].iter().enumerate() {
            let h = height*scale(count)/scale(maximum);
            cr.rectangle(x as f64*width/plot.len() as f64, height-h, width/plot.len() as f64+0.1, h);
        }
        let _ = cr.fill();
    }
}
