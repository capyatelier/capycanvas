/// Android window state, owned exclusively by its render Looper.
pub(crate) struct App {
    pub tone: crate::hdr::ToneState,
    pub proof: layer_ui::proof_workflow::ProofView,
    pub host: layer_host::NativeHost,
    pub workspaces: Option<layer_workspace::WorkspaceController<layer_workspace::StoreWorker>>,
    pub blank_presented: bool,
    pub profiling: bool,
    pub frame_cost: [i64; 5],
    pub pointer_records: Vec<f64>,
    pub cursor: layer_ui::CanvasCursor,
    pub surface: Option<crate::android::Surface>,
    pub cache_directory: String,
    pub overviews: Vec<crate::android::OverviewSlot>,
    pub instance: Option<wgpu::Instance>,
    pub gpu_generation: u64,
    pub gpu_failure: std::sync::Arc<std::sync::OnceLock<String>>,
}
impl App {
    pub fn new() -> Result<Self, String> {
        let mut host = layer_host::NativeHost::new(layer_ui::Platform::Android)?;
        host.startup = Default::default();
        host.session.set_document_replacement(true);
        host.dispatch(layer_ui::UiAction::RestoreWorkspace {
            workspace: Box::new(layer_ui::WorkspaceState::for_platform(
                layer_ui::Platform::Android,
            )),
        })?;
        Ok(Self {
            proof: Default::default(),
            tone: Default::default(),
            host,
            workspaces: None,
            blank_presented: false,
            profiling: false,
            frame_cost: [0; 5],
            pointer_records: Vec::new(),
            cursor: layer_ui::CanvasCursor::default(),
            surface: None,
            instance: None,
            gpu_generation: 0,
            gpu_failure: Default::default(),
            cache_directory: String::new(),
            overviews: Vec::new(),
        })
    }
}
