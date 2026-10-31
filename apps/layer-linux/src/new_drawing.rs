//! The New drawing dialog presents the session's `NewDrawingDialogView`;
//! presets, paper and paper color slide up over the page.
use crate::color_editor::{ColorForm, SHEET_TILE, add_tile, swatch_tiles};
use crate::display_color::ColorPatch;
use crate::number_control::NumberControl;
use crate::paper_picker::{PaperThumbnail, texture_grid, texture_tile};
use crate::slide_sheet::SlideSheet;
use crate::workspace::Workspace;
use adw::prelude::*;
use gtk::{gdk, glib};
use layer_core::color::RgbColor;
use layer_ui::*;
use std::{cell::{Cell, RefCell}, rc::{Rc, Weak}, sync::Arc, task::{Poll, Waker}};

const PREVIEW: [i32; 2] = [232, 250];
const PREVIEW_FIT: [i32; 2] = [196, 214];
const NARROW_PREVIEW: [i32; 2] = [348, 140];
const NARROW_PREVIEW_FIT: [i32; 2] = [312, 104];
const PRESET_TILE: i32 = 128;
const PRESET_FRAME: [i32; 2] = [116, 86];
const PRESET_FIT: i32 = 74;
const SUMMARY_ART: i32 = 36;
const THUMBNAIL: i32 = 36;
const CLEAR: RgbColor = RgbColor { rgba: [1., 1., 1., 0.], ..RgbColor::WHITE };

fn grain(preview: &[u8]) -> Option<gtk::cairo::ImageSurface> {
    let size = layer_core::paper::PREVIEW_SIZE as i32;
    let mut surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, size, size).ok()?;
    let stride = surface.stride() as usize;
    {
        let mut data = surface.data().ok()?;
        for (index, value) in preview.iter().enumerate().take((size * size) as usize) {
            let shade = f32::from(*value) / 255. - 0.5;
            let alpha = if shade < 0. { -shade * 0.7 } else { shade * 0.25 };
            let light = if shade < 0. { 0. } else { alpha };
            let offset = index / size as usize * stride + index % size as usize * 4;
            data[offset..offset + 4].copy_from_slice(&[(light * 255.) as u8, (light * 255.) as u8, (light * 255.) as u8, (alpha * 255.) as u8]);
        }
    }
    Some(surface)
}

/// A sheet drawn at its proportions in its paper color, grain and transparency.
struct SheetArt {
    root: gtk::Box,
    sheet: gtk::Overlay,
    patch: ColorPatch,
    grain: gtk::DrawingArea,
    surface: Rc<RefCell<Option<gtk::cairo::ImageSurface>>>,
    fit: Cell<[i32; 2]>,
}
impl SheetArt {
    fn new(bounds: [i32; 2], fit: [i32; 2], mini: bool) -> Self {
        let patch = ColorPatch::new(false);
        let grain = gtk::DrawingArea::new();
        grain.set_can_target(false);
        let surface: Rc<RefCell<Option<gtk::cairo::ImageSurface>>> = Rc::default();
        let tile = if mini { 14. } else { 40. } / layer_core::paper::PREVIEW_SIZE as f64;
        grain.set_draw_func(glib::clone!(#[strong] surface, move |_, cr, _, _| {
            let Some(surface) = surface.borrow().clone() else { return };
            let pattern = gtk::cairo::SurfacePattern::create(&surface);
            pattern.set_extend(gtk::cairo::Extend::Repeat);
            cr.scale(tile, tile);
            if cr.set_source(&pattern).is_ok() { let _ = cr.paint(); }
        }));
        let sheet = gtk::Overlay::new();
        sheet.set_child(Some(&patch));
        sheet.add_overlay(&grain);
        sheet.add_css_class("new-drawing-sheet");
        if mini { sheet.add_css_class("mini"); }
        sheet.set_overflow(gtk::Overflow::Hidden);
        sheet.set_halign(gtk::Align::Center);
        sheet.set_valign(gtk::Align::Center);
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.set_size_request(bounds[0], bounds[1]);
        root.append(&sheet);
        sheet.set_vexpand(true);
        Self { root, sheet, patch, grain, surface, fit: Cell::new(fit) }
    }
    fn resize(&self, bounds: [i32; 2], fit: [i32; 2]) {
        self.root.set_size_request(bounds[0], bounds[1]);
        self.fit.set(fit);
    }
    fn show(&self, workspace: &Workspace, sheet: &SheetView) {
        let [width, height] = sheet.extent.map(f64::from);
        let fit = self.fit.get();
        let side = f64::from(fit[0].min(fit[1]));
        let scale = (side / width.max(height)).min(f64::from(fit[0]) / width).min(f64::from(fit[1]) / height);
        self.sheet.set_size_request((width * scale).round().max(4.) as i32, (height * scale).round().max(4.) as i32);
        self.patch.set_display_color(if sheet.shown { sheet.color } else { CLEAR }, workspace.paint_view_color(), 1.);
        let preview = sheet.texture.as_deref().and_then(|id| workspace.gpu.try_borrow().ok()?.as_ref()?.session.paper_preview(id));
        *self.surface.borrow_mut() = preview.as_deref().and_then(grain);
        self.grain.set_opacity(if sheet.shown { 1. } else { 0.35 });
        self.grain.queue_draw();
    }
}

fn section(heading: &gtk::Label, content: &impl IsA<gtk::Widget>) -> gtk::Box {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 6);
    section.append(heading);
    section.append(content);
    section
}

fn heading() -> gtk::Label {
    let label = gtk::Label::builder().xalign(0.).build();
    label.add_css_class("heading");
    label
}

fn dim(css: &str) -> gtk::Label {
    let label = gtk::Label::builder().xalign(0.).ellipsize(gtk::pango::EllipsizeMode::End).build();
    label.add_css_class("dim-label");
    if !css.is_empty() { label.add_css_class(css); }
    label
}

fn footer_button(name: &str, suggested: bool) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_widget_name(name);
    if suggested { button.add_css_class("suggested-action"); }
    button
}

fn footer() -> gtk::Box {
    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    footer.add_css_class("dialog-footer");
    footer
}

fn gap() -> gtk::Box {
    let gap = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    gap.set_hexpand(true);
    gap
}

fn drawer(name: &str, head: &impl IsA<gtk::Widget>, body: &impl IsA<gtk::Widget>, bottom: &gtk::Box) -> gtk::Box {
    let top = gtk::Box::new(gtk::Orientation::Vertical, 0);
    top.add_css_class("new-drawing-drawer-head");
    top.append(head);
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    page.add_css_class("new-drawing-drawer-body");
    page.append(body);
    let viewport = gtk::Viewport::builder().vscroll_policy(gtk::ScrollablePolicy::Natural).child(&crate::squircle::Squircles::new(&page)).build();
    let scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&viewport)
        .build());
    let sheet = gtk::Box::new(gtk::Orientation::Vertical, 0);
    sheet.add_css_class("dialog-content");
    sheet.set_widget_name(name);
    sheet.append(&crate::squircle::Squircles::new(&top));
    sheet.append(&scroll);
    sheet.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    sheet.append(&crate::squircle::Squircles::new(bottom));
    sheet
}

fn detach(widget: &gtk::Widget) {
    if let Some(grid) = widget.parent().and_downcast::<gtk::Grid>() { grid.remove(widget); }
    else if let Some(parent) = widget.parent().and_downcast::<gtk::Box>() { parent.remove(widget); }
}

fn title() -> gtk::Label {
    let title = gtk::Label::new(None);
    title.add_css_class("title-4");
    title.set_halign(gtk::Align::Center);
    title
}

struct Page {
    title: gtk::Label,
    body: gtk::Box,
    left: gtk::Box,
    art: SheetArt,
    preview: gtk::Button,
    preset: gtk::Label,
    capacity: gtk::Label,
    name_label: gtk::Label,
    name: gtk::Entry,
    size_heading: gtk::Label,
    width_prefix: gtk::Label,
    height_prefix: gtk::Label,
    width_slot: gtk::Box,
    height_slot: gtk::Box,
    numbers: RefCell<Option<(NumericControl, NumberControl, NumberControl)>>,
    link: gtk::ToggleButton,
    unit: gtk::DropDown,
    portrait: gtk::ToggleButton,
    landscape: gtk::ToggleButton,
    resolution_label: gtk::Label,
    resolution: NumberControl,
    resolution_unit: gtk::Label,
    resolutions: gtk::MenuButton,
    resolution_menu: gtk::Popover,
    resolution_choices: gtk::Box,
    size: gtk::Label,
    color_heading: gtk::Label,
    space_label: gtk::Label,
    depth_label: gtk::Label,
    blending_label: gtk::Label,
    space: gtk::DropDown,
    depth: gtk::DropDown,
    blending: gtk::DropDown,
    paper_heading: gtk::Label,
    texture: gtk::Button,
    texture_thumbnail: PaperThumbnail,
    texture_name: gtk::Label,
    texture_relief: gtk::Label,
    paper_color: gtk::Button,
    paper_patch: ColorPatch,
    paper_color_name: gtk::Label,
    presets: gtk::Button,
    presets_label: gtk::Label,
    cancel: gtk::Button,
    create: gtk::Button,
}

struct Browser {
    slide: Rc<SlideSheet>,
    search: gtk::SearchEntry,
    tabs: adw::WrapBox,
    grid: gtk::FlowBox,
    empty: gtk::Label,
    summary_art: SheetArt,
    summary_name: gtk::Label,
    summary_size: gtk::Label,
    summary_details: gtk::Label,
    actions: gtk::Stack,
    cancel: gtk::Button,
    save_as: gtk::Button,
    select: gtk::Button,
    save_name: gtk::Entry,
    save_group: gtk::DropDown,
    save_cancel: gtk::Button,
    save_confirm: gtk::Button,
    menu: gtk::Popover,
}

struct PaperDrawer {
    slide: Rc<SlideSheet>,
    title: gtk::Label,
    grid: gtk::FlowBox,
    import: gtk::FlowBoxChild,
    import_label: gtk::Label,
    relief_row: gtk::Box,
    relief_label: gtk::Label,
    relief: NumberControl,
    relief_unit: gtk::Label,
    cancel: gtk::Button,
    apply: gtk::Button,
}

struct ColorDrawer {
    slide: Rc<SlideSheet>,
    title: gtk::Label,
    content: gtk::Box,
    color_mode: gtk::ToggleButton,
    transparent_mode: gtk::ToggleButton,
    color_label: gtk::Label,
    transparent_label: gtk::Label,
    palette_count: gtk::Label,
    well: gtk::Box,
    palette: gtk::Box,
    palette_title: gtk::Label,
    add: RefCell<Option<gtk::Button>>,
    palette_colors: RefCell<Vec<(RgbColor, String)>>,
    menu: gtk::Popover,
    cancel: gtk::Button,
    apply: gtk::Button,
    form: RefCell<Option<Rc<ColorForm>>>,
}

pub(crate) struct NewDrawing {
    this: Weak<Self>,
    workspace: Weak<Workspace>,
    localization: RefCell<Arc<Localizer>>,
    updating: Cell<bool>,
    dialog: adw::Dialog,
    page: Page,
    browser: Browser,
    papers: PaperDrawer,
    colors: ColorDrawer,
    shown: RefCell<Option<NewDrawingDialogView>>,
    done: RefCell<(bool, Option<Waker>)>,
}

pub(crate) fn screen(surface: &gdk::Surface) -> Option<ScreenInfo> {
    let monitor = surface.display().monitor_at_surface(surface)?;
    let geometry = monitor.geometry();
    let scale = monitor.scale();
    let extent = [(f64::from(geometry.width()) * scale).round() as u32, (f64::from(geometry.height()) * scale).round() as u32];
    let millimetres = monitor.width_mm();
    let ppi = (millimetres > 0).then(|| (f64::from(extent[0]) / (f64::from(millimetres) / 25.4)).round() as u32);
    Some(ScreenInfo { extent, ppi })
}

pub(crate) async fn run(w: &Rc<Workspace>) -> Result<Option<NewDrawingOrder>, String> {
    let untitled = w.documents.model.borrow().upcoming_id();
    let opened = w.gpu.borrow_mut().as_mut().ok_or_else(|| NewDocumentError::CanvasUnavailable.message(&w.localization()))?
        .session.dispatch(UiAction::NewDrawing { action: NewDrawingAction::Open { screen: w.window.surface().as_ref().and_then(screen), untitled } });
    w.changed(opened.clone());
    opened?;
    let dialog = NewDrawing::build(w);
    *w.new_drawing.borrow_mut() = Rc::downgrade(&dialog);
    dialog.dialog.present(Some(&w.window));
    let waiting = dialog.clone();
    std::future::poll_fn(move |context| {
        let mut done = waiting.done.borrow_mut();
        if done.0 { return Poll::Ready(()); }
        done.1 = Some(context.waker().clone());
        Poll::Pending
    }).await;
    Ok(w.gpu.borrow().as_ref().and_then(|g| g.session.new_drawing_order().cloned()))
}

impl NewDrawing {
    fn build(w: &Rc<Workspace>) -> Rc<Self> {
        let localization = w.localization();
        let dialog = adw::Dialog::builder().content_width(720).content_height(500).width_request(320).height_request(300).build();
        dialog.set_widget_name("new-drawing-dialog");
        dialog.add_css_class("picker-dialog");
        dialog.add_css_class("new-drawing-dialog");

        let art = SheetArt::new(PREVIEW, PREVIEW_FIT, false);
        let preview = gtk::Button::builder().child(&art.root).build();
        preview.add_css_class("new-drawing-preview");
        preview.set_widget_name("new-drawing-preview");
        preview.set_overflow(gtk::Overflow::Hidden);
        let preset = gtk::Label::builder().justify(gtk::Justification::Center).ellipsize(gtk::pango::EllipsizeMode::End).build();
        preset.set_widget_name("new-drawing-caption");
        let capacity = gtk::Label::builder().ellipsize(gtk::pango::EllipsizeMode::End).build();
        capacity.add_css_class("new-drawing-small");
        capacity.set_widget_name("new-drawing-capacity");
        let caption = gtk::Box::new(gtk::Orientation::Vertical, 2);
        caption.append(&preset);
        caption.append(&capacity);
        let left = gtk::Box::new(gtk::Orientation::Vertical, 8);
        left.set_valign(gtk::Align::Start);
        left.set_size_request(PREVIEW[0], -1);
        left.append(&preview);
        left.append(&caption);

        let row_label = || gtk::Label::builder().xalign(0.).build();
        let name_label = row_label();
        let name = gtk::Entry::builder().hexpand(true).build();
        name.set_widget_name("new-drawing-name");
        let name_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        name_row.append(&name_label);
        name_row.append(&name);

        let size_heading = heading();
        let prefixes = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
        let slot = |name: &str| {
            let prefix = gtk::Label::builder().xalign(0.).build();
            prefix.add_css_class("new-drawing-affix");
            prefixes.add_widget(&prefix);
            let slot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            slot.add_css_class("new-drawing-field");
            slot.set_widget_name(name);
            slot.append(&prefix);
            (slot, prefix)
        };
        let (width_slot, width_prefix) = slot("new-drawing-width");
        let (height_slot, height_prefix) = slot("new-drawing-height");
        let link = gtk::ToggleButton::new();
        link.add_css_class("flat");
        link.add_css_class("new-drawing-link");
        link.set_widget_name("new-drawing-link");
        let unit = crate::panel_controls::dropdown(&[]);
        unit.set_widget_name("new-drawing-unit");
        let orientation = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        orientation.set_widget_name("new-drawing-orientation");
        orientation.add_css_class("choice-well");
        orientation.add_css_class("new-drawing-joined");
        orientation.add_css_class("new-drawing-icons");
        orientation.set_valign(gtk::Align::Center);
        let portrait = gtk::ToggleButton::builder().child(&crate::icons::image("layer-portrait-symbolic")).build();
        portrait.set_widget_name("new-drawing-portrait");
        let landscape = gtk::ToggleButton::builder().child(&crate::icons::image("layer-landscape-symbolic")).build();
        landscape.set_widget_name("new-drawing-landscape");
        orientation.append(&portrait);
        orientation.append(&landscape);
        let linked = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        linked.append(&width_slot);
        linked.append(&link);
        linked.append(&height_slot);
        let dimensions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        for widget in [linked.upcast_ref::<gtk::Widget>(), unit.upcast_ref(), gap().upcast_ref(), orientation.upcast_ref()] {
            dimensions.append(widget);
        }

        let resolution_label = row_label();
        resolution_label.set_margin_end(4);
        let resolution = NumberControl::field(resolution_control(), "", 4, localization.clone());
        resolution.set_widget_name("new-drawing-resolution");
        resolution.set_hexpand(false);
        let resolution_unit = gtk::Label::new(None);
        resolution_unit.add_css_class("new-drawing-affix");
        let resolution_choices = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let resolution_menu = gtk::Popover::builder().has_arrow(false).position(gtk::PositionType::Bottom).halign(gtk::Align::Start).child(&resolution_choices).build();
        resolution_menu.set_offset(0, 4);
        resolution_menu.set_widget_name("new-drawing-resolution-menu");
        let resolutions = gtk::MenuButton::builder().popover(&resolution_menu).child(&crate::icons::image("layer-chevron-down-symbolic")).build();
        resolutions.add_css_class("flat");
        resolutions.set_valign(gtk::Align::Center);
        resolutions.set_widget_name("new-drawing-resolution-choices");
        let resolution_field = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        resolution_field.add_css_class("new-drawing-field");
        resolution_field.add_css_class("new-drawing-combo");
        resolution_field.set_widget_name("new-drawing-resolution-field");
        resolution_field.append(&resolution);
        resolution_field.append(&resolution_unit);
        resolution_field.append(&resolutions);
        resolution_menu.connect_show(glib::clone!(#[weak] resolution_field, #[weak] resolutions, move |menu| {
            if let Some(field) = resolution_field.compute_bounds(&resolutions) {
                menu.set_pointing_to(Some(&gdk::Rectangle::new(field.x() as i32, field.y() as i32, field.width() as i32, field.height() as i32)));
            }
        }));
        let size = dim("new-drawing-small");
        size.set_hexpand(true);
        size.set_margin_start(10);
        size.set_widget_name("new-drawing-size");
        let resolution_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        resolution_row.append(&resolution_label);
        resolution_row.append(&resolution_field);
        resolution_row.append(&size);
        let size_rows = gtk::Box::new(gtk::Orientation::Vertical, 10);
        size_rows.append(&dimensions);
        size_rows.append(&resolution_row);
        let size_section = section(&size_heading, &size_rows);

        let color_heading = heading();
        let colors = gtk::Grid::builder().column_spacing(6).column_homogeneous(true).build();
        let captioned = |column: i32, span: i32, name: &str| {
            let label = dim("new-drawing-caption");
            let choice = crate::panel_controls::dropdown(&[]);
            choice.set_widget_name(name);
            let cell = gtk::Box::new(gtk::Orientation::Vertical, 2);
            cell.append(&label);
            cell.append(&choice);
            colors.attach(&cell, column, 0, span, 1);
            (label, choice)
        };
        let (space_label, space) = captioned(0, 4, "new-drawing-space");
        let (depth_label, depth) = captioned(4, 5, "new-drawing-depth");
        let (blending_label, blending) = captioned(9, 4, "new-drawing-blending");
        let color_section = section(&color_heading, &colors);

        let paper_heading = heading();
        let paper_tile = |name: &str, thumbnail: &gtk::Widget| {
            let label = gtk::Label::builder().xalign(0.).ellipsize(gtk::pango::EllipsizeMode::End).build();
            label.add_css_class("heading");
            let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
            text.set_hexpand(true);
            text.set_valign(gtk::Align::Center);
            text.append(&label);
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            content.append(thumbnail);
            content.append(&text);
            let button = gtk::Button::builder().child(&content).hexpand(true).build();
            button.add_css_class("new-drawing-paper-tile");
            button.set_widget_name(name);
            (button, label, text)
        };
        let texture_thumbnail = PaperThumbnail::new(THUMBNAIL);
        let (texture, texture_name, texture_text) = paper_tile("new-drawing-texture", texture_thumbnail.image.upcast_ref());
        let texture_relief = dim("new-drawing-caption");
        texture_relief.set_widget_name("new-drawing-texture-relief");
        texture_text.append(&texture_relief);
        let paper_patch = ColorPatch::new(false);
        paper_patch.set_size_request(THUMBNAIL, THUMBNAIL);
        paper_patch.set_overflow(gtk::Overflow::Hidden);
        paper_patch.add_css_class("paper-preview");
        let (paper_color, paper_color_name, _) = paper_tile("new-drawing-paper-color", paper_patch.upcast_ref());
        let paper_tiles = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        paper_tiles.set_homogeneous(true);
        paper_tiles.append(&texture);
        paper_tiles.append(&paper_color);
        let paper_section = section(&paper_heading, &paper_tiles);

        let right = gtk::Box::new(gtk::Orientation::Vertical, 14);
        right.set_hexpand(true);
        for section in [name_row.upcast_ref::<gtk::Widget>(), size_section.upcast_ref(), color_section.upcast_ref(), paper_section.upcast_ref()] {
            right.append(section);
        }
        let body = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        body.append(&left);
        body.append(&right);
        let page_title = title();
        let page_box = gtk::Box::new(gtk::Orientation::Vertical, 14);
        page_box.add_css_class("dialog-page");
        page_box.add_css_class("new-drawing-page");
        page_box.append(&page_title);
        page_box.append(&body);
        let viewport = gtk::Viewport::builder().vscroll_policy(gtk::ScrollablePolicy::Natural).child(&crate::squircle::Squircles::new(&page_box)).build();
        let page_scroll = crate::input::pen_scroller(gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&viewport)
            .build());

        let presets_label = gtk::Label::new(None);
        let presets_icon = crate::icons::image("layer-chevron-down-symbolic");
        presets_icon.add_css_class("new-drawing-up");
        let presets_content = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        presets_content.append(&presets_label);
        presets_content.append(&presets_icon);
        let presets = gtk::Button::builder().child(&presets_content).build();
        presets.set_widget_name("new-drawing-presets-button");
        let cancel = footer_button("new-drawing-cancel", false);
        let create = footer_button("new-drawing-create", true);
        let main_footer = footer();
        for widget in [presets.upcast_ref::<gtk::Widget>(), gap().upcast_ref(), cancel.upcast_ref(), create.upcast_ref()] { main_footer.append(widget); }
        let main = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main.append(&page_scroll);
        main.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        main.append(&crate::squircle::Squircles::new(&main_footer));

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&main));
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("dialog-content");
        content.append(&overlay);
        overlay.set_vexpand(true);
        dialog.set_child(Some(&content));
        dialog.set_default_widget(Some(&create));

        let browser = {
            let search = gtk::SearchEntry::builder().hexpand(true).build();
            search.set_widget_name("new-drawing-search");
            let tabs = adw::WrapBox::builder().child_spacing(4).line_spacing(4).build();
            tabs.set_widget_name("new-drawing-tabs");
            let grid = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::Single)
                .homogeneous(true)
                .min_children_per_line(2)
                .max_children_per_line(8)
                .row_spacing(12)
                .column_spacing(10)
                .valign(gtk::Align::Start)
                .build();
            grid.add_css_class("texture-grid");
            grid.set_widget_name("new-drawing-presets");
            let empty = dim("new-drawing-small");
            empty.set_wrap(true);
            empty.set_visible(false);
            let list = gtk::Box::new(gtk::Orientation::Vertical, 14);
            list.append(&tabs);
            list.append(&grid);
            list.append(&empty);
            let summary_art = SheetArt::new([SUMMARY_ART; 2], [SUMMARY_ART - 2; 2], true);
            let summary_name = gtk::Label::builder().xalign(0.).ellipsize(gtk::pango::EllipsizeMode::End).build();
            summary_name.add_css_class("new-drawing-summary-name");
            summary_name.set_widget_name("new-drawing-summary-name");
            let summary_size = dim("new-drawing-small");
            summary_size.set_widget_name("new-drawing-summary-size");
            let summary_details = dim("new-drawing-small");
            summary_details.set_widget_name("new-drawing-summary-details");
            let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            top.append(&summary_name);
            top.append(&summary_size);
            let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
            text.set_valign(gtk::Align::Center);
            text.append(&top);
            text.append(&summary_details);
            let summary = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            summary.set_hexpand(true);
            summary.append(&summary_art.root);
            summary.append(&text);
            let cancel = footer_button("new-drawing-browser-cancel", false);
            let save_as = footer_button("new-drawing-save-as", false);
            let select = footer_button("new-drawing-select", true);
            let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            buttons.add_css_class("new-drawing-actions");
            buttons.set_halign(gtk::Align::End);
            buttons.append(&cancel);
            buttons.append(&save_as);
            buttons.append(&select);
            let save_name = gtk::Entry::builder().width_chars(10).build();
            save_name.set_widget_name("new-drawing-save-name");
            let save_group = gtk::DropDown::from_strings(&[]);
            save_group.set_widget_name("new-drawing-save-group");
            let save_cancel = footer_button("new-drawing-save-cancel", false);
            let save_confirm = footer_button("new-drawing-save-confirm", true);
            let saving = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            saving.add_css_class("new-drawing-actions");
            saving.set_halign(gtk::Align::End);
            saving.append(&save_name);
            saving.append(&save_group);
            saving.append(&save_cancel);
            saving.append(&save_confirm);
            let actions = gtk::Stack::builder().hhomogeneous(false).interpolate_size(true).transition_type(gtk::StackTransitionType::Crossfade).build();
            actions.add_named(&buttons, Some("buttons"));
            actions.add_named(&saving, Some("save"));
            let bottom = footer();
            bottom.append(&summary);
            bottom.append(&actions);
            let sheet = drawer("new-drawing-browser", &search, &list, &bottom);
            let menu = gtk::Popover::builder().has_arrow(false).build();
            menu.set_widget_name("new-drawing-preset-menu");
            Browser {
                slide: SlideSheet::new(&overlay, &sheet), search, tabs, grid, empty, summary_art, summary_name, summary_size, summary_details, actions,
                cancel, save_as, select, save_name, save_group, save_cancel, save_confirm, menu,
            }
        };

        let papers = {
            let title = title();
            let grid = texture_grid();
            grid.set_widget_name("new-drawing-papers");
            let thumbnail = PaperThumbnail::new(96);
            let import = texture_tile(&thumbnail, w, None, "");
            thumbnail.show_icon("layer-plus-symbolic");
            import.set_widget_name("new-drawing-paper-import");
            let import_label = import.child().and_then(|content| content.last_child()).and_downcast::<gtk::Label>().expect("texture tile caption");
            let relief_label = gtk::Label::builder().xalign(0.).ellipsize(gtk::pango::EllipsizeMode::End).build();
            let relief = NumberControl::field(relief_control(), "", 4, localization.clone());
            relief.set_widget_name("new-drawing-paper-relief");
            relief.set_hexpand(false);
            let relief_unit = gtk::Label::new(None);
            relief_unit.add_css_class("new-drawing-affix");
            let relief_field = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            relief_field.add_css_class("new-drawing-field");
            relief_field.add_css_class("new-drawing-combo");
            relief_field.set_widget_name("new-drawing-paper-relief-field");
            relief_field.append(&relief);
            relief_field.append(&relief_unit);
            let relief_row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            relief_row.append(&relief_label);
            relief_row.append(&relief_field);
            let cancel = footer_button("new-drawing-paper-cancel", false);
            let apply = footer_button("new-drawing-paper-use", true);
            let bottom = footer();
            for widget in [relief_row.upcast_ref::<gtk::Widget>(), gap().upcast_ref(), cancel.upcast_ref(), apply.upcast_ref()] { bottom.append(widget); }
            let sheet = drawer("new-drawing-paper-drawer", &title, &grid, &bottom);
            PaperDrawer { slide: SlideSheet::new(&overlay, &sheet), title, grid, import, import_label, relief_row, relief_label, relief, relief_unit, cancel, apply }
        };

        let colors = {
            let title = title();
            let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let well = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            well.add_css_class("choice-well");
            well.add_css_class("new-drawing-joined");
            well.set_valign(gtk::Align::Center);
            well.set_hexpand(true);
            well.set_halign(gtk::Align::End);
            well.set_widget_name("new-drawing-color-mode");
            let mode = |icon: &str, name: &str| {
                let image = crate::icons::image(icon);
                image.set_pixel_size(14);
                let label = gtk::Label::new(None);
                let content = gtk::Box::new(gtk::Orientation::Horizontal, 5);
                content.append(&image);
                content.append(&label);
                let toggle = gtk::ToggleButton::builder().child(&content).build();
                toggle.set_widget_name(name);
                well.append(&toggle);
                (toggle, label)
            };
            let (color_mode, color_label) = mode("layer-solid-symbolic", "new-drawing-color-mode-color");
            let (transparent_mode, transparent_label) = mode("layer-transparent-symbolic", "new-drawing-color-mode-transparent");
            transparent_mode.set_group(Some(&color_mode));
            let palette_title = gtk::Label::builder().xalign(0.).build();
            let palette_count = dim("");
            let palette = gtk::Box::new(gtk::Orientation::Vertical, 6);
            palette.set_margin_top(8);
            palette.set_widget_name("new-drawing-paper-colors");
            let cancel = footer_button("new-drawing-color-cancel", false);
            let apply = footer_button("new-drawing-color-use", true);
            let bottom = footer();
            for widget in [gap().upcast_ref::<gtk::Widget>(), cancel.upcast_ref(), apply.upcast_ref()] { bottom.append(widget); }
            let sheet = drawer("new-drawing-color-drawer", &title, &content, &bottom);
            let menu = gtk::Popover::builder().has_arrow(false).build();
            ColorDrawer {
                slide: SlideSheet::new(&overlay, &sheet), title, content, color_mode, transparent_mode, color_label, transparent_label, palette_count, well, palette, palette_title, add: RefCell::default(), palette_colors: RefCell::default(), menu,
                cancel, apply, form: RefCell::default(),
            }
        };

        let page = Page {
            title: page_title, body, left, art, preview, preset, capacity, name_label, name, size_heading, width_prefix, height_prefix,
            width_slot, height_slot, numbers: RefCell::default(), link, unit, portrait, landscape, resolution_label, resolution, resolution_unit,
            resolutions, resolution_menu, resolution_choices, size, color_heading, space_label, depth_label, blending_label, space, depth, blending,
            paper_heading, texture, texture_thumbnail, texture_name, texture_relief, paper_color, paper_patch, paper_color_name, presets,
            presets_label, cancel, create,
        };
        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            workspace: Rc::downgrade(w),
            localization: RefCell::new(localization),
            updating: Cell::new(false),
            dialog,
            page,
            browser,
            papers,
            colors,
            shown: RefCell::default(),
            done: RefCell::default(),
        });
        this.bind(w);
        this.render();
        this
    }

    fn workspace(&self) -> Option<Rc<Workspace>> { self.workspace.upgrade() }

    fn view(&self) -> Option<NewDrawingDialogView> {
        self.workspace()?.gpu.borrow().as_ref()?.session.state().new_drawing.clone()
    }

    fn send(&self, action: NewDrawingAction) {
        if self.updating.get() { return; }
        let Some(w) = self.workspace() else { return };
        w.dispatch(UiAction::NewDrawing { action });
    }

    fn finish(&self) {
        let waker = {
            let mut done = self.done.borrow_mut();
            if done.0 { return; }
            done.0 = true;
            done.1.take()
        };
        self.close_colors();
        self.dialog.close();
        if let Some(waker) = waker { waker.wake(); }
    }

    pub(crate) fn render(&self) {
        let (Some(w), Some(view)) = (self.workspace(), self.view()) else { return self.finish() };
        let previous = self.shown.replace(Some(view.clone()));
        self.updating.set(true);
        if previous.as_ref().is_none_or(|previous| previous.copy != view.copy) { self.relabel(&view); }
        self.render_page(&w, &view);
        self.render_drawer(&w, &view, previous.as_ref().and_then(|previous| previous.drawer.as_ref()));
        self.updating.set(false);
    }

    fn relabel(&self, view: &NewDrawingDialogView) {
        let copy = &view.copy;
        let label = |widget: &gtk::Widget, text: &str| widget.update_property(&[gtk::accessible::Property::Label(text)]);
        let model = |dropdown: &gtk::DropDown, labels: Vec<&str>| dropdown.set_model(Some(&gtk::StringList::new(&labels)));
        let page = &self.page;
        self.dialog.set_title(&copy.title);
        page.title.set_text(&copy.title);
        page.name_label.set_text(&copy.name);
        label(page.name.upcast_ref(), &copy.name);
        page.size_heading.set_text(&copy.size);
        page.width_prefix.set_text(&copy.width);
        page.height_prefix.set_text(&copy.height);
        page.link.set_tooltip_text(Some(&copy.link));
        label(page.link.upcast_ref(), &copy.link);
        model(&page.unit, view.units.iter().map(|unit| unit.label.as_ref()).collect());
        label(page.unit.upcast_ref(), &copy.unit);
        for (toggle, text) in [(&page.portrait, &copy.portrait), (&page.landscape, &copy.landscape)] {
            toggle.set_tooltip_text(Some(text));
            label(toggle.upcast_ref(), text);
        }
        page.resolution_label.set_text(&copy.resolution);
        page.resolution.set_caption(&copy.resolution, "", self.localization.borrow().clone());
        if let Some((_, width, height)) = page.numbers.borrow().as_ref() {
            width.set_caption(&copy.width_name, "", self.localization.borrow().clone());
            height.set_caption(&copy.height_name, "", self.localization.borrow().clone());
        }
        page.resolution_unit.set_text(&copy.resolution_unit);
        page.resolutions.set_tooltip_text(Some(&copy.common_resolutions));
        label(page.resolutions.upcast_ref(), &copy.common_resolutions);
        page.color_heading.set_text(&copy.color);
        page.space_label.set_text(&copy.space);
        page.depth_label.set_text(&copy.depth);
        page.blending_label.set_text(&copy.blending);
        model(&page.space, view.spaces.iter().map(|space| space.label.as_ref()).collect());
        model(&page.depth, view.depths.iter().map(|depth| depth.label.as_ref()).collect());
        model(&page.blending, view.blendings.iter().map(|blending| blending.label.as_ref()).collect());
        for (dropdown, text) in [(&page.space, &copy.space), (&page.depth, &copy.depth), (&page.blending, &copy.blending)] { label(dropdown.upcast_ref(), text); }
        page.paper_heading.set_text(&copy.paper);
        page.texture.set_tooltip_text(Some(&copy.paper));
        page.paper_color.set_tooltip_text(Some(&copy.paper_color));
        page.presets_label.set_text(&copy.presets);
        page.preview.set_tooltip_text(Some(&copy.presets));
        label(page.preview.upcast_ref(), &copy.presets);
        page.cancel.set_label(&copy.cancel);
        page.create.set_label(&copy.create);
        let browser = &self.browser;
        browser.search.set_placeholder_text(Some(&copy.search));
        label(browser.search.upcast_ref(), &copy.search);
        browser.cancel.set_label(&copy.cancel);
        browser.select.set_label(&copy.select);
        browser.save_name.set_placeholder_text(Some(&copy.preset_name));
        label(browser.save_name.upcast_ref(), &copy.preset_name);
        browser.save_group.set_tooltip_text(Some(&copy.preset_category));
        label(browser.save_group.upcast_ref(), &copy.preset_category);
        browser.save_cancel.set_label(&copy.cancel);
        let papers = &self.papers;
        papers.title.set_text(&copy.paper);
        papers.import_label.set_text(&copy.import);
        papers.import.set_tooltip_text(Some(&copy.import));
        label(papers.import.upcast_ref(), &copy.import);
        papers.relief_label.set_text(&copy.relief);
        papers.relief_label.set_tooltip_text(Some(&copy.relief));
        papers.relief.set_caption(&copy.relief, "", self.localization.borrow().clone());
        papers.relief_unit.set_text(&copy.relief_unit);
        papers.cancel.set_label(&copy.cancel);
        papers.apply.set_label(&copy.use_texture);
        let colors = &self.colors;
        colors.title.set_text(&copy.paper_color);
        colors.color_label.set_text(&copy.color);
        colors.transparent_label.set_text(&copy.transparent);
        label(colors.well.upcast_ref(), &copy.sheet_mode);
        colors.palette_title.set_text(&copy.paper_colors);
        colors.cancel.set_label(&copy.cancel);
        colors.apply.set_label(&copy.use_color);
    }

    fn numbers(&self, view: &NewDrawingDialogView) -> (NumberControl, NumberControl) {
        if let Some((current, width, height)) = self.page.numbers.borrow().as_ref() && *current == view.numeric {
            return (width.clone(), height.clone());
        }
        let l = self.localization.borrow().clone();
        let control = |slot: &gtk::Box, title: &str, edit: fn(f64) -> NewDrawingAction| {
            while let Some(child) = slot.last_child().filter(|child| child.is::<NumberControl>()) { slot.remove(&child); }
            let number = NumberControl::field(view.numeric.clone(), title, 6, l.clone());
            number.set_hexpand(false);
            let weak = self.this.clone();
            number.connect_value_changed(move |number| if let Some(dialog) = weak.upgrade() { dialog.send(edit(number.value())); });
            slot.append(&number);
            number
        };
        let width = control(&self.page.width_slot, &view.copy.width_name, |value| NewDrawingAction::Width { value });
        let height = control(&self.page.height_slot, &view.copy.height_name, |value| NewDrawingAction::Height { value });
        *self.page.numbers.borrow_mut() = Some((view.numeric.clone(), width.clone(), height.clone()));
        (width, height)
    }

    fn render_page(&self, w: &Workspace, view: &NewDrawingDialogView) {
        let page = &self.page;
        let setup = &view.setup;
        page.name.set_placeholder_text(Some(&view.untitled));
        if page.name.text() != view.name { page.name.set_text(&view.name); }
        let (width, height) = self.numbers(view);
        for (number, value) in [(&width, view.values[0]), (&height, view.values[1])] {
            if !number.composing() { number.set_value(value); }
        }
        page.link.set_active(view.linked);
        crate::icons::set_button(&page.link, if view.linked { "layer-link-symbolic" } else { "layer-unlink-symbolic" });
        let position = |found: Option<usize>| found.unwrap_or(0) as u32;
        page.unit.set_selected(position(view.units.iter().position(|unit| unit.value == setup.unit)));
        page.portrait.set_active(view.page.orientation == Orientation::Portrait);
        page.landscape.set_active(view.page.orientation == Orientation::Landscape);
        page.resolution.set_value(f64::from(setup.resolution));
        page.size.set_text(&view.page.size);
        if view.page.problem { page.size.add_css_class("error"); page.size.remove_css_class("dim-label"); }
        else { page.size.remove_css_class("error"); page.size.add_css_class("dim-label"); }
        page.space.set_selected(position(view.spaces.iter().position(|space| space.value == setup.color.space)));
        page.depth.set_selected(position(view.depths.iter().position(|depth| depth.value == setup.color.depth)));
        page.blending.set_selected(position(view.blendings.iter().position(|blending| blending.value == setup.blend_space)));
        page.blending.set_sensitive(view.blending_note.is_none());
        page.blending.set_tooltip_text(view.blending_note.as_deref());
        match &view.sheet.texture {
            Some(id) => page.texture_thumbnail.show(w, Some(id)),
            None => page.texture_thumbnail.show_icon("layer-none-symbolic"),
        }
        page.texture_name.set_text(&view.page.texture);
        page.texture_relief.set_text(view.page.relief.as_deref().unwrap_or_default());
        page.texture_relief.set_visible(view.page.relief.is_some());
        page.paper_patch.set_display_color(if view.sheet.shown { view.sheet.color } else { CLEAR }, w.paint_view_color(), 1.);
        page.paper_color_name.set_text(&view.page.paper_color);
        page.paper_color.set_sensitive(view.paper_color_enabled);
        page.art.show(w, &view.sheet);
        page.preset.set_markup(&format!("<span weight=\"600\">{}</span> <span alpha=\"60%\">· {}</span>", glib::markup_escape_text(&view.page.preset), glib::markup_escape_text(&view.page.orientation_label)));
        page.capacity.set_text(&view.page.capacity);
        page.capacity.set_tooltip_text(Some(&view.page.memory));
        page.create.set_sensitive(view.can_create);
        self.fill_resolutions(view);
    }

    fn fill_resolutions(&self, view: &NewDrawingDialogView) {
        let choices = &self.page.resolution_choices;
        while let Some(child) = choices.first_child() { choices.remove(&child); }
        for (index, choice) in view.resolutions.iter().enumerate() {
            let value = gtk::Label::builder().label(&choice.label).xalign(0.).hexpand(true).build();
            value.add_css_class("numeric");
            let purpose = dim("");
            purpose.set_text(&choice.purpose);
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 16);
            content.append(&value);
            content.append(&purpose);
            let button = gtk::Button::builder().child(&content).sensitive(choice.enabled).build();
            button.add_css_class("flat");
            button.set_widget_name(&format!("new-drawing-resolution-{index}"));
            if choice.selected { button.add_css_class("selected"); }
            let (weak, resolution) = (self.this.clone(), f64::from(choice.value));
            button.connect_clicked(move |_| {
                let Some(dialog) = weak.upgrade() else { return };
                dialog.page.resolution_menu.popdown();
                dialog.send(NewDrawingAction::Resolution { value: resolution });
            });
            choices.append(&button);
        }
    }

    fn render_drawer(&self, w: &Rc<Workspace>, view: &NewDrawingDialogView, previous: Option<&NewDrawingDrawerView>) {
        let kind = |drawer: Option<&NewDrawingDrawerView>| drawer.map(std::mem::discriminant);
        let opening = kind(previous) != kind(view.drawer.as_ref());
        let slides = [&self.browser.slide, &self.papers.slide, &self.colors.slide];
        let open = match &view.drawer {
            Some(NewDrawingDrawerView::Presets(browser)) => {
                self.render_browser(w, browser, previous.and_then(|previous| match previous { NewDrawingDrawerView::Presets(browser) => Some(&**browser), _ => None }).filter(|_| !opening));
                Some(0)
            }
            Some(NewDrawingDrawerView::Paper { tiles, relief }) => {
                self.render_papers(w, tiles, relief, previous.and_then(|previous| match previous { NewDrawingDrawerView::Paper { tiles, .. } => Some(tiles), _ => None }).filter(|_| !opening));
                Some(1)
            }
            Some(NewDrawingDrawerView::PaperColor(colors)) => {
                if opening { self.open_colors(w, colors); }
                self.render_colors(w, colors);
                Some(2)
            }
            None => None,
        };
        if opening && open != Some(2) { self.close_colors(); }
        for (index, slide) in slides.into_iter().enumerate() { slide.show(open == Some(index)); }
        self.page.body.set_sensitive(open.is_none());
        if !opening { return; }
        match open {
            Some(0) => self.browser.search.grab_focus(),
            Some(1) => self.papers.grid.grab_focus(),
            Some(_) => false,
            None => match previous {
                Some(NewDrawingDrawerView::Presets(_)) => self.page.presets.grab_focus(),
                Some(NewDrawingDrawerView::Paper { .. }) => self.page.texture.grab_focus(),
                _ => self.page.paper_color.grab_focus(),
            },
        };
    }

    fn render_browser(&self, w: &Workspace, view: &PresetBrowserView, previous: Option<&PresetBrowserView>) {
        let browser = &self.browser;
        if previous.is_none_or(|previous| previous.query != view.query) && browser.search.text() != view.query {
            browser.search.set_text(&view.query);
        }
        if previous.is_none_or(|previous| previous.tabs != view.tabs) { self.fill_tabs(view); }
        browser.tabs.set_sensitive(view.tabs_enabled);
        let content = |tiles: &[PresetTileView]| tiles.iter().map(|tile| PresetTileView { selected: false, ..tile.clone() }).collect::<Vec<_>>();
        if previous.is_none_or(|previous| content(&previous.tiles) != content(&view.tiles)) { self.fill_presets(w, view); }
        match view.tiles.iter().position(|tile| tile.selected).and_then(|index| browser.grid.child_at_index(index as i32)) {
            Some(child) => browser.grid.select_child(&child),
            None => browser.grid.unselect_all(),
        }
        browser.empty.set_text(view.empty.as_deref().unwrap_or_default());
        browser.empty.set_visible(view.empty.is_some());
        browser.summary_art.show(w, &view.summary_sheet);
        browser.summary_name.set_text(&view.summary.name);
        browser.summary_size.set_text(&view.summary.size);
        browser.summary_details.set_text(&view.summary.details);
        browser.save_as.set_label(&view.save_label);
        browser.select.set_sensitive(view.can_select);
        let Some(save) = &view.save else {
            browser.actions.set_visible_child_name("buttons");
            return;
        };
        let previous_save = previous.and_then(|previous| previous.save.as_ref());
        let starting = previous_save.is_none();
        if previous_save.is_none_or(|previous| previous.groups != save.groups) {
            browser.save_group.set_model(Some(&gtk::StringList::new(&save.groups.iter().map(|group| group.label.as_ref()).collect::<Vec<_>>())));
        }
        if browser.save_name.text() != save.name { browser.save_name.set_text(&save.name); }
        browser.save_group.set_selected(save.groups.iter().position(|group| group.value == save.group).unwrap_or(0) as u32);
        browser.save_confirm.set_label(&save.confirm);
        browser.save_confirm.set_sensitive(save.can_save);
        if save.can_save { browser.save_name.remove_css_class("error"); } else { browser.save_name.add_css_class("error"); }
        browser.actions.set_visible_child_name("save");
        if starting {
            browser.save_name.grab_focus();
            browser.save_name.select_region(0, -1);
        }
    }

    fn fill_tabs(&self, view: &PresetBrowserView) {
        let well = &self.browser.tabs;
        while let Some(child) = well.first_child() { well.remove(&child); }
        let mut first: Option<gtk::ToggleButton> = None;
        for (index, tab) in view.tabs.iter().enumerate() {
            let toggle = gtk::ToggleButton::with_label(&tab.label);
            toggle.add_css_class("new-drawing-pill");
            toggle.set_widget_name(&format!("new-drawing-tab-{index}"));
            if let Some(first) = &first { toggle.set_group(Some(first)); } else { first = Some(toggle.clone()); }
            toggle.set_active(tab.selected);
            let weak = self.this.clone();
            toggle.connect_toggled(move |toggle| if toggle.is_active() && let Some(dialog) = weak.upgrade() { dialog.send(NewDrawingAction::Tab { index }); });
            well.append(&toggle);
        }
    }

    fn fill_presets(&self, w: &Workspace, view: &PresetBrowserView) {
        let browser = &self.browser;
        browser.menu.popdown();
        if browser.menu.parent().is_some() { browser.menu.unparent(); }
        browser.grid.remove_all();
        for (index, tile) in view.tiles.iter().enumerate() {
            let art = SheetArt::new(PRESET_FRAME, [PRESET_FIT; 2], true);
            art.show(w, &tile.sheet);
            let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
            frame.add_css_class("new-drawing-preset-frame");
            frame.set_halign(gtk::Align::Center);
            frame.append(&art.root);
            let caption = |text: &str, class: &str, dim: bool| {
                let label = gtk::Label::builder().label(text).wrap(true).lines(2).justify(gtk::Justification::Center)
                    .ellipsize(gtk::pango::EllipsizeMode::End).width_chars(1).max_width_chars(1).build();
                label.add_css_class(class);
                if dim { label.add_css_class("dim-label"); }
                label
            };
            let content = gtk::Box::new(gtk::Orientation::Vertical, 4);
            content.set_size_request(PRESET_TILE, -1);
            content.set_valign(gtk::Align::Start);
            content.append(&frame);
            let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
            text.append(&caption(&tile.name, "new-drawing-tile-name", false));
            text.append(&caption(&tile.dimensions, "new-drawing-caption", true));
            text.append(&caption(&tile.details, "new-drawing-caption", true));
            content.append(&text);
            if !tile.enabled { content.set_opacity(0.4); }
            let child = gtk::FlowBoxChild::builder().child(&content).tooltip_text(format!("{}\n{} · {}", tile.name, tile.dimensions, tile.details)).build();
            child.set_widget_name(&format!("new-drawing-preset-{index}"));
            child.update_property(&[gtk::accessible::Property::Label(&tile.name)]);
            if tile.own {
                let click = gtk::GestureClick::builder().button(3).build();
                let weak = self.this.clone();
                click.connect_pressed(move |_, _, _, _| if let Some(dialog) = weak.upgrade() { dialog.preset_menu(index); });
                child.add_controller(click);
                let hold = gtk::GestureLongPress::new();
                let weak = self.this.clone();
                hold.connect_pressed(move |_, _, _| if let Some(dialog) = weak.upgrade() { dialog.preset_menu(index); });
                child.add_controller(hold);
            }
            browser.grid.append(&child);
        }
    }

    fn preset_menu(&self, index: usize) {
        let Some(copy) = self.shown.borrow().as_ref().map(|view| view.copy.clone()) else { return };
        let browser = &self.browser;
        let Some(child) = browser.grid.child_at_index(index as i32) else { return };
        let items = gtk::Box::new(gtk::Orientation::Vertical, 0);
        for (label, delete) in [(copy.rename, false), (copy.delete, true)] {
            let button = gtk::Button::with_label(&label);
            button.add_css_class("flat");
            let weak = self.this.clone();
            button.connect_clicked(move |_| {
                let Some(dialog) = weak.upgrade() else { return };
                dialog.browser.menu.popdown();
                let weak = weak.clone();
                glib::idle_add_local_once(move || if let Some(dialog) = weak.upgrade() {
                    dialog.send(if delete { NewDrawingAction::DeletePreset { index } } else { NewDrawingAction::RenamePreset { index } });
                });
            });
            items.append(&button);
        }
        browser.menu.set_child(Some(&items));
        if browser.menu.parent().as_ref() != Some(child.upcast_ref()) {
            if browser.menu.parent().is_some() { browser.menu.unparent(); }
            browser.menu.set_parent(&child);
        }
        browser.menu.popup();
    }

    fn render_papers(&self, w: &Workspace, tiles: &[PaperChoice], relief: &PaperReliefView, previous: Option<&Vec<PaperChoice>>) {
        let papers = &self.papers;
        let grid = &papers.grid;
        let content = |tiles: &[PaperChoice]| tiles.iter().map(|tile| (tile.id.clone(), tile.label.clone())).collect::<Vec<_>>();
        if previous.is_none_or(|previous| content(previous) != content(tiles)) {
            grid.remove_all();
            for (index, tile) in tiles.iter().enumerate() {
                let thumbnail = PaperThumbnail::new(96);
                let child = texture_tile(&thumbnail, w, tile.id.as_ref(), &tile.label);
                if tile.id.is_none() { thumbnail.show_icon("layer-none-symbolic"); }
                child.set_widget_name(&format!("new-drawing-paper-tile-{index}"));
                grid.append(&child);
            }
            grid.append(&papers.import);
        }
        if let Some(child) = tiles.iter().position(|tile| tile.selected).and_then(|index| grid.child_at_index(index as i32)) { grid.select_child(&child); }
        if !papers.relief.composing() { papers.relief.set_value(relief.value); }
        papers.relief_row.set_sensitive(relief.enabled);
    }

    fn use_paper(&self) {
        if self.papers.relief.commit_text() { self.send(NewDrawingAction::UsePaper); }
    }

    fn picked_paper(&self) -> Option<Option<String>> {
        let index = self.papers.grid.selected_children().first().and_then(|child| usize::try_from(child.index()).ok())?;
        match self.shown.borrow().as_ref()?.drawer.as_ref()? {
            NewDrawingDrawerView::Paper { tiles, .. } => tiles.get(index).map(|tile| tile.id.as_deref().map(str::to_string)),
            _ => None,
        }
    }

    fn import_paper(&self) {
        let (Some(w), weak) = (self.workspace(), self.this.clone()) else { return };
        glib::MainContext::default().spawn_local(async move {
            let result = crate::files::paper::run(&w, layer_ui::paper_library::PaperRequest::Import { layer: None }).await;
            if let Some(dialog) = weak.upgrade() { dialog.render(); }
            if let Err(error) = result { w.changed(Err(error)); }
        });
    }

    fn open_colors(&self, w: &Rc<Workspace>, view: &PaperColorDrawerView) {
        let Some(editor) = w.gpu.borrow().as_ref().map(|g| ColorEditor::for_color(g.session.state().display_colors(), view.color, true)) else { return };
        let editor = match editor {
            Ok(editor) => editor,
            Err(error) => { w.changed(Err(error.message(&w.localization()))); return; }
        };
        let form = ColorForm::build(w, editor);
        form.show_single_color();
        form.hex_copy.set_visible(false);
        for row in &form.rows { row.copy.set_visible(false); }
        for widget in [self.colors.well.upcast_ref::<gtk::Widget>(), self.colors.palette.upcast_ref()] { detach(widget); }
        form.header.remove(&form.pick);
        if let Some(hex) = form.header.child_at(2, 0) {
            form.header.remove(&hex);
            hex.set_hexpand(false);
            hex.set_halign(gtk::Align::Start);
            form.header.attach(&hex, 1, 0, 1, 1);
        }
        form.header.attach(&self.colors.well, 2, 0, 1, 1);
        form.values.append(&self.colors.palette);
        let weak = self.this.clone();
        form.connect_viewed(move |viewed| if let Some(dialog) = weak.upgrade() { dialog.send(NewDrawingAction::PaperColorValue { color: viewed.value }); });
        let weak = self.this.clone();
        form.connect_blocked(move |blocked| if let Some(dialog) = weak.upgrade() { dialog.colors.apply.set_sensitive(!blocked); });
        let content = &self.colors.content;
        while let Some(child) = content.first_child() { content.remove(&child); }
        content.append(&form.body);
        self.colors.palette_colors.borrow_mut().clear();
        self.colors.form.replace(Some(form.clone()));
        form.bind(false);
        form.arrange(self.dialog.width() > 0 && self.dialog.width() < 700);
    }

    fn render_colors(&self, w: &Workspace, view: &PaperColorDrawerView) {
        let Some(form) = self.colors.form.borrow().clone() else { return };
        let colors = &self.colors;
        if view.transparent { colors.transparent_mode.set_active(true); } else { colors.color_mode.set_active(true); }
        let shown: Vec<_> = view.palette.iter().map(|choice| (choice.color, choice.name.clone())).collect();
        if *colors.palette_colors.borrow() != shown { self.fill_palette(&form, view); }
        colors.palette_count.set_text(&view.count);
        if let Some(add) = colors.add.borrow().as_ref() { add.set_sensitive(view.can_add); }
        if let Some(tiles) = colors.palette.last_child().and_downcast::<gtk::FlowBox>() {
            for (index, choice) in view.palette.iter().enumerate() {
                let Some(tile) = tiles.child_at_index(index as i32).and_then(|child| child.child()) else { continue };
                if choice.selected { tile.add_css_class("selected"); } else { tile.remove_css_class("selected"); }
            }
        }
        for widget in [form.left.upcast_ref::<gtk::Widget>(), form.values.upcast_ref()] { widget.set_opacity(if view.transparent { 0.4 } else { 1. }); }
        for class in ["dim-label", "new-drawing-transparent"] {
            if view.transparent { form.hex.label.add_css_class(class); } else { form.hex.label.remove_css_class(class); }
        }
        if view.transparent {
            form.new_patch.set_display_color(CLEAR, w.paint_view_color(), 1.);
            if let Some(copy) = self.shown.borrow().as_ref() { form.hex.label.set_text(&copy.copy.transparent); }
        }
    }

    fn fill_palette(&self, form: &ColorForm, view: &PaperColorDrawerView) {
        let colors = &self.colors;
        let palette = &colors.palette;
        while let Some(child) = palette.first_child() { palette.remove(&child); }
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        for label in [&colors.palette_title, &colors.palette_count] { detach(label.upcast_ref()); head.append(label); }
        palette.append(&head);
        let tiles = swatch_tiles();
        tiles.set_widget_name("new-drawing-paper-color-tiles");
        for (index, choice) in view.palette.iter().enumerate() {
            let tile = form.tile(choice.color, &choice.name, &format!("new-drawing-paper-color-{index}"), SHEET_TILE);
            let weak = self.this.clone();
            let click = gtk::GestureClick::builder().button(3).build();
            click.connect_pressed(move |gesture, _, _, _| if let Some(dialog) = weak.upgrade() && let Some(widget) = gesture.widget() { dialog.palette_menu(&widget, index); });
            tile.add_controller(click);
            let weak = self.this.clone();
            let hold = gtk::GestureLongPress::new();
            hold.connect_pressed(move |gesture, _, _| if let Some(dialog) = weak.upgrade() && let Some(widget) = gesture.widget() { dialog.palette_menu(&widget, index); });
            tile.add_controller(hold);
            tiles.append(&tile);
        }
        let label = self.shown.borrow().as_ref().map(|view| view.copy.add_paper_color.clone()).unwrap_or_default();
        let add = add_tile("new-drawing-add-paper-color", &label);
        let weak = self.this.clone();
        add.connect_clicked(move |_| if let Some(dialog) = weak.upgrade() { dialog.send(NewDrawingAction::AddPaperColor); });
        tiles.append(&add);
        colors.add.replace(Some(add));
        palette.append(&tiles);
        *colors.palette_colors.borrow_mut() = view.palette.iter().map(|choice| (choice.color, choice.name.clone())).collect();
    }

    fn palette_menu(&self, tile: &gtk::Widget, index: usize) {
        let Some(label) = self.shown.borrow().as_ref().map(|view| view.copy.delete.clone()) else { return };
        let menu = &self.colors.menu;
        let delete = gtk::Button::with_label(&label);
        delete.add_css_class("flat");
        let weak = self.this.clone();
        delete.connect_clicked(move |_| {
            let Some(dialog) = weak.upgrade() else { return };
            dialog.colors.menu.popdown();
            let weak = weak.clone();
            glib::idle_add_local_once(move || if let Some(dialog) = weak.upgrade() { dialog.send(NewDrawingAction::RemovePaperColor { index }); });
        });
        menu.set_child(Some(&delete));
        if menu.parent().is_some() { menu.unparent(); }
        menu.set_parent(tile);
        menu.popup();
    }

    fn close_colors(&self) {
        let menu = &self.colors.menu;
        menu.popdown();
        if menu.parent().is_some() { menu.unparent(); }
        self.colors.form.replace(None);
    }

    fn create(&self) {
        let Some(view) = self.shown.borrow().clone() else { return };
        let (width, height) = self.numbers(&view);
        if [&width, &height, &self.page.resolution].into_iter().all(NumberControl::commit_text) { self.send(NewDrawingAction::Create); }
    }

    fn bind(&self, w: &Rc<Workspace>) {
        let weak = self.this.clone();
        let sends = move |action: NewDrawingAction| { let weak = weak.clone(); move || if let Some(dialog) = weak.upgrade() { dialog.send(action.clone()); } };
        let page = &self.page;
        let weak = self.this.clone();
        page.name.connect_changed(move |name| if let Some(dialog) = weak.upgrade() { dialog.send(NewDrawingAction::Name { text: name.text().into() }); });
        let weak = self.this.clone();
        page.link.connect_toggled(move |link| if let Some(dialog) = weak.upgrade() { dialog.send(NewDrawingAction::Linked { linked: link.is_active() }); });
        for (toggle, orientation) in [(&page.portrait, Orientation::Portrait), (&page.landscape, Orientation::Landscape)] {
            let send = sends(NewDrawingAction::Orientation { orientation });
            toggle.connect_clicked(move |_| send());
        }
        let weak = self.this.clone();
        page.resolution.connect_value_changed(move |number| if let Some(dialog) = weak.upgrade() { dialog.send(NewDrawingAction::Resolution { value: number.value() }); });
        let choose = |dropdown: &gtk::DropDown, action: fn(&NewDrawingDialogView, usize) -> Option<NewDrawingAction>| {
            let weak = self.this.clone();
            dropdown.connect_selected_notify(move |dropdown| {
                let Some(dialog) = weak.upgrade() else { return };
                let chosen = dialog.shown.borrow().as_ref().and_then(|view| action(view, dropdown.selected() as usize));
                if let Some(chosen) = chosen { dialog.send(chosen); }
            });
        };
        choose(&page.unit, |view, index| view.units.get(index).map(|unit| NewDrawingAction::Unit { unit: unit.value }));
        choose(&page.space, |view, index| view.spaces.get(index).map(|space| NewDrawingAction::Space { space: space.value }));
        choose(&page.depth, |view, index| view.depths.get(index).map(|depth| NewDrawingAction::Depth { depth: depth.value }));
        choose(&page.blending, |view, index| view.blendings.get(index).map(|blending| NewDrawingAction::Blending { blending: blending.value }));
        for (button, action) in [(&page.texture, NewDrawingAction::Paper), (&page.paper_color, NewDrawingAction::PaperColor), (&page.presets, NewDrawingAction::Presets),
            (&page.preview, NewDrawingAction::Presets), (&page.cancel, NewDrawingAction::Cancel)] {
            let send = sends(action);
            button.connect_clicked(move |_| send());
        }
        let weak = self.this.clone();
        page.create.connect_clicked(move |_| if let Some(dialog) = weak.upgrade() { dialog.create(); });
        let weak = self.this.clone();
        page.name.connect_activate(move |_| if let Some(dialog) = weak.upgrade() { dialog.create(); });

        let browser = &self.browser;
        let weak = self.this.clone();
        browser.search.connect_search_changed(move |search| if let Some(dialog) = weak.upgrade() { dialog.send(NewDrawingAction::Search { query: search.text().into() }); });
        let weak = self.this.clone();
        crate::input::stop_search(&browser.search, move |_| {
            let Some(dialog) = weak.upgrade() else { return };
            dialog.send(NewDrawingAction::Back);
        });
        let weak = self.this.clone();
        browser.grid.connect_selected_children_changed(move |grid| {
            let Some(index) = grid.selected_children().first().and_then(|child| usize::try_from(child.index()).ok()) else { return };
            if let Some(dialog) = weak.upgrade() { dialog.send(NewDrawingAction::PickPreset { index }); }
        });
        browser.grid.set_activate_on_single_click(false);
        let weak = self.this.clone();
        browser.grid.connect_child_activated(move |_, child| if let Some(dialog) = weak.upgrade() && let Ok(index) = usize::try_from(child.index()) {
            dialog.send(NewDrawingAction::PickPreset { index });
            dialog.send(NewDrawingAction::UsePreset);
        });
        for (button, action) in [(&browser.cancel, NewDrawingAction::CloseDrawer), (&browser.save_as, NewDrawingAction::SaveAs), (&browser.select, NewDrawingAction::UsePreset),
            (&browser.save_cancel, NewDrawingAction::CancelSave), (&browser.save_confirm, NewDrawingAction::SavePreset)] {
            let send = sends(action);
            button.connect_clicked(move |_| send());
        }
        let weak = self.this.clone();
        browser.save_name.connect_changed(move |name| if let Some(dialog) = weak.upgrade() { dialog.send(NewDrawingAction::PresetName { text: name.text().into() }); });
        let send = sends(NewDrawingAction::SavePreset);
        browser.save_name.connect_activate(move |_| send());
        let weak = self.this.clone();
        browser.save_group.connect_selected_notify(move |dropdown| {
            let Some(dialog) = weak.upgrade() else { return };
            if let Some(group) = PresetGroup::ALL.get(dropdown.selected() as usize) { dialog.send(NewDrawingAction::PresetGroup { group: *group }); }
        });

        let papers = &self.papers;
        let weak = self.this.clone();
        papers.grid.connect_selected_children_changed(move |_| if let Some(dialog) = weak.upgrade() {
            match dialog.picked_paper() {
                Some(id) => dialog.send(NewDrawingAction::PickPaper { id }),
                None if !dialog.updating.get() => dialog.render(),
                None => {}
            }
        });
        papers.grid.set_activate_on_single_click(false);
        let weak = self.this.clone();
        papers.grid.connect_child_activated(move |_, child| if let Some(dialog) = weak.upgrade() {
            if *child == dialog.papers.import { dialog.import_paper(); } else { dialog.use_paper(); }
        });
        let click = gtk::GestureClick::new();
        let weak = self.this.clone();
        click.connect_pressed(|gesture, _, _, _| { gesture.set_state(gtk::EventSequenceState::Claimed); });
        click.connect_released(move |gesture, presses, x, y| if presses == 1 && gesture.widget().is_some_and(|tile| tile.contains(x, y)) && let Some(dialog) = weak.upgrade() {
            dialog.import_paper();
        });
        papers.import.add_controller(click);
        let weak = self.this.clone();
        papers.relief.connect_value_changed(move |number| if let Some(dialog) = weak.upgrade() { dialog.send(NewDrawingAction::PaperRelief { value: number.value() }); });
        let send = sends(NewDrawingAction::CloseDrawer);
        papers.cancel.connect_clicked(move |_| send());
        let weak = self.this.clone();
        papers.apply.connect_clicked(move |_| if let Some(dialog) = weak.upgrade() { dialog.use_paper(); });

        let colors = &self.colors;
        for (toggle, transparent) in [(&colors.color_mode, false), (&colors.transparent_mode, true)] {
            let weak = self.this.clone();
            toggle.connect_toggled(move |toggle| if toggle.is_active() && let Some(dialog) = weak.upgrade() { dialog.send(NewDrawingAction::Transparent { transparent }); });
        }
        for (button, action) in [(&colors.cancel, NewDrawingAction::CloseDrawer), (&colors.apply, NewDrawingAction::UsePaperColor)] {
            let send = sends(action);
            button.connect_clicked(move |_| send());
        }

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Bubble);
        let weak = self.this.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(dialog) = weak.upgrade().filter(|dialog| key == gdk::Key::Escape && dialog.shown.borrow().as_ref().is_some_and(|view| view.drawer.is_some())) else {
                return glib::Propagation::Proceed;
            };
            dialog.send(NewDrawingAction::Back);
            glib::Propagation::Stop
        });
        self.dialog.add_controller(keys);
        let narrow = adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 700sp").unwrap());
        let weak = self.this.clone();
        narrow.connect_apply(move |_| if let Some(dialog) = weak.upgrade() { dialog.arrange(true); });
        let weak = self.this.clone();
        narrow.connect_unapply(move |_| if let Some(dialog) = weak.upgrade() { dialog.arrange(false); });
        self.dialog.add_breakpoint(narrow);
        let weak = self.this.clone();
        self.dialog.connect_closed(move |_| {
            let Some(dialog) = weak.upgrade() else { return };
            if !dialog.done.borrow().0 { dialog.send(NewDrawingAction::Cancel); }
            dialog.finish();
        });
        let weak = self.this.clone();
        w.on_localization(move |localization| {
            let Some(dialog) = weak.upgrade().filter(|dialog| !dialog.done.borrow().0) else { return false };
            *dialog.localization.borrow_mut() = localization.clone();
            if let Some(form) = dialog.colors.form.borrow().clone() { form.set_localization(localization); form.relabel(); form.refresh(); }
            dialog.render();
            true
        });
    }

    fn arrange(&self, narrow: bool) {
        let page = &self.page;
        page.body.set_orientation(if narrow { gtk::Orientation::Vertical } else { gtk::Orientation::Horizontal });
        page.left.set_halign(if narrow { gtk::Align::Center } else { gtk::Align::Fill });
        let (bounds, fit) = if narrow { (NARROW_PREVIEW, NARROW_PREVIEW_FIT) } else { (PREVIEW, PREVIEW_FIT) };
        page.left.set_size_request(bounds[0], -1);
        page.art.resize(bounds, fit);
        if let (Some(w), Some(view)) = (self.workspace(), self.shown.borrow().as_ref()) { page.art.show(&w, &view.sheet); }
        if let Some(form) = self.colors.form.borrow().as_ref() { form.arrange(narrow); }
    }
}

pub(crate) async fn paper_texture(w: &Rc<Workspace>, material: &layer_ui::paper_library::PaperMaterial) -> Result<layer_core::TextureSource, String> {
    use layer_ui::paper_library::{PaperMaterial, PaperRequest};
    let PaperMaterial::Stored { id, fallback } = material else { return Ok(material.fallback()) };
    crate::files::paper::run(w, PaperRequest::Load { id: id.clone() }).await?;
    Ok(match w.gpu.borrow().as_ref().map(|g| g.session.paper_material(id)) {
        Some(PaperMaterial::Ready(texture)) => texture,
        _ => fallback.clone(),
    })
}
