//! Shared search/list presentation for toolbar and window-bar tool pickers.
use super::*;

pub(super) struct ToolCatalog {
    pub root: gtk::Box,
    pub search: gtk::SearchEntry,
    pub choices: gtk::ListBox,
    key: RefCell<String>,
}
impl ToolCatalog {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.set_vexpand(true);
        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Search tools"));
        root.append(&search);
        let choices = gtk::ListBox::new();
        choices.set_selection_mode(gtk::SelectionMode::None);
        choices.add_css_class("boxed-list");
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&choices)
            .build();
        root.append(&scroll);
        Self {
            root,
            search,
            choices,
            key: RefCell::new(String::new()),
        }
    }
    pub fn clear(&self) {
        self.key.borrow_mut().clear();
    }
    pub fn update(&self, key: String, build: impl FnOnce(&Self)) {
        if *self.key.borrow() == key {
            return;
        }
        *self.key.borrow_mut() = key;
        while let Some(child) = self.choices.first_child() {
            self.choices.remove(&child);
        }
        build(self);
    }
    pub fn row(&self, label: &str, description: &str, icon: &str) -> adw::ActionRow {
        let row = adw::ActionRow::new();
        row.set_use_markup(false);
        row.set_title(label);
        row.set_subtitle(description);
        row.set_title_lines(1);
        row.set_subtitle_lines(1);
        row.add_prefix(&gtk::Image::from_icon_name(&format!(
            "layer-{icon}-symbolic"
        )));
        self.choices.append(&row);
        row
    }
}
