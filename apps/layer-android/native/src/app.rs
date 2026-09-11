/// Android window state, owned exclusively by its render Looper.
pub(crate) struct App {
    pub host: layer_host::NativeHost,
    pub blank_presented: bool,
    pub profiling: bool,
    pub frame_cost: [i64; 5],
    pub pointer_records: Vec<f64>,
    pub cursor: layer_ui::CanvasCursor,
    pub surface: Option<crate::android::Surface>,
    pub cache_directory: String,
    pub overviews: Vec<crate::android::OverviewSlot>,
    pub instance: Option<wgpu::Instance>,
}
impl App {
    pub fn new() -> Result<Self, String> {
        let mut host = layer_host::NativeHost::new(layer_ui::Platform::Android)?;
        host.startup = Default::default();
        host.session.set_document_replacement(true);
        host.dispatch(layer_ui::UiAction::RestoreWorkspace {
            workspace: layer_ui::WorkspaceState::for_platform(layer_ui::Platform::Android),
        })?;
        Ok(Self {
            host,
            blank_presented: false,
            profiling: false,
            frame_cost: [0; 5],
            pointer_records: Vec::new(),
            cursor: layer_ui::CanvasCursor::default(),
            surface: None,
            instance: None,
            cache_directory: String::new(),
            overviews: Vec::new(),
        })
    }
}
