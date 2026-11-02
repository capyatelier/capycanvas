fn main() -> gtk::glib::ExitCode {
    if let Some(result) = layer_linux::check_saved_data_cli() { return result; }
    unsafe { layer_linux::enable_gtk_color_management() };
    layer_linux::run()
}
