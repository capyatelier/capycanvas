/// Android window state, owned exclusively by its render Looper.
pub(crate) struct App {
    pub window: crate::document_tabs::Window,
    pub tone: layer_host::tone::ToneService,
    pub scopes: crate::scopes::Cache,
    pub host: layer_host::NativeHost,
    pub language: layer_ui::LanguageTransition,
    pub published_bootstrap: crate::launch::PublishedBootstrap,
    pub language_preference: layer_ui::LanguagePreference,
    pub workspaces: Option<layer_workspace::WorkspaceController<layer_workspace::StoreWorker>>,
    pub workspace_wake: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    pub blank_presented: bool,
    pub profiling: bool,
    pub frame_cost: [i64; 11],
    pub pointer_records: Vec<f64>,
    pub cursor: layer_ui::CanvasCursor,
    pub surface: Option<crate::android::Surface>,
    pub display_hdr_available: bool,
    pub display_wide: bool,
    pub navigators: layer_host::scene::Navigators,
    pub glass: Vec<layer_render_wgpu::BackdropRegion>,
    pub instance: Option<wgpu::Instance>,
    pub gpu_generation: u64,
    pub gpu_watch: layer_host::DeviceWatch,
    pub screen_presented: std::time::Instant,
    pub frame_deadline: Option<std::time::Instant>,
}
impl App {
    pub fn new(saved: &str, preferred_tags: &[&str], screen: Option<layer_ui::ScreenInfo>) -> Result<Self, String> {
        let mut host = layer_host::NativeHost::launch_on_screen(layer_ui::Platform::Android, saved, crate::launch::localization(saved, preferred_tags), screen)?;
        if let Some(language) = crate::launch::current_preference() {
            let mut settings = host.session.state().settings.clone();
            settings.language = language;
            host.dispatch(layer_ui::UiAction::RestoreSettings { settings: Box::new(settings) })?;
        }
        host.startup = Default::default();
        host.session.set_document_replacement(false);
        Ok(Self {
            window: crate::document_tabs::Window::localized(host.session.localization()),
            tone: Default::default(),
            scopes: Default::default(),
            language: layer_ui::LanguageTransition::new(host.session.localization().clone()),
            published_bootstrap: Default::default(),
            language_preference: host.session.state().settings.language,
            host,
            workspaces: None,
            workspace_wake: None,
            blank_presented: false,
            profiling: false,
            frame_cost: [0; 11],
            pointer_records: Vec::new(),
            cursor: layer_ui::CanvasCursor::default(),
            surface: None,
            display_hdr_available: false,
            display_wide: false,
            instance: None,
            gpu_generation: 0,
            gpu_watch: Default::default(),
            navigators: layer_host::scene::Navigators::new(layer_host::scene::SlotUnits::Physical),
            glass: Vec::new(),
            screen_presented: std::time::Instant::now(),
            frame_deadline: None,
        })
    }
}

impl AsMut<layer_host::NativeHost> for App {
    fn as_mut(&mut self)->&mut layer_host::NativeHost {&mut self.host}
}
