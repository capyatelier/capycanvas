/// Android window state, owned exclusively by its render Looper.
pub(crate) struct App {
    pub host: layer_host::NativeHost,
    pub profiling: bool,
    pub frame_cost: [i64; 5],
    pub pointer_records: Vec<f64>,
    pub cursor: layer_ui::CanvasCursor,
    pub surface: Option<crate::android::Surface>,
    pub instance: Option<wgpu::Instance>,
}
impl App {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            host: layer_host::NativeHost::new(layer_ui::Platform::Android)?,
            profiling: false,
            frame_cost: [0; 5],
            pointer_records: Vec::new(),
            cursor: layer_ui::CanvasCursor::default(),
            surface: None,
            instance: None,
        })
    }
}
