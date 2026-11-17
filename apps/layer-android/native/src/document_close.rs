use layer_host::{NativeHost, Renderer, window::{Activation, DocumentWindow, PreparedClose}};
use layer_ui::UiSession;

pub(crate) struct CloseJob(pub Option<PreparedClose>);
impl CloseJob {
    pub fn commit(&mut self, window: &mut DocumentWindow<Box<UiSession<Renderer>>>, host: &mut NativeHost) -> Result<Activation, String> {
        window.validate_close(host, self.0.as_ref().ok_or("Drawing close task was already consumed")?)?;
        window.commit_close(host, self.0.take().unwrap(), |_| {}).map(|(activation, _)| activation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use layer_ui::{CommandId, Platform, UiAction};

    fn prepared() -> (NativeHost, DocumentWindow<Box<UiSession<Renderer>>>, Box<CloseJob>) {
        let mut host = NativeHost::new(Platform::Android).unwrap();
        host.session.suspend_renderer().unwrap();
        let mut window = DocumentWindow::default();
        host.dispatch(UiAction::Invoke { command: CommandId::CloseDocument }).unwrap();
        let job = Box::new(CloseJob(Some(window.prepare_close(&mut host, Default::default()).unwrap())));
        (host, window, job)
    }

    #[test]
    fn tab_controller_switch_requests_name_their_drawing_and_end_the_switch() {
        let mut host = NativeHost::new(Platform::Android).unwrap();
        host.session.suspend_renderer().unwrap();
        let mut window = DocumentWindow::<Box<UiSession<Renderer>>>::default();
        let mut request = |text: &str| window.request(&mut host, serde_json::from_str::<layer_host::window::TabRequest>(text).unwrap()).unwrap();
        assert_eq!(request(r#"{"op":"prepare","id":1}"#), serde_json::json!({"current": true}));
        assert_eq!(request(r#"{"op":"end_switch"}"#), serde_json::Value::Null);
    }

    #[test]
    fn rejected_commit_preserves_the_job_for_cancellation() {
        let (mut host, mut window, mut job) = prepared();
        host.session.replace_renderer(Renderer::default()).unwrap();
        assert!(job.commit(&mut window, &mut host).is_err());
        window.cancel_close(&mut host, job.0.take().unwrap()).unwrap();
        assert!(!host.document_close_prepared());
    }

    #[test]
    fn committed_payload_leaves_its_allocation_owned_until_release() {
        let (mut host, mut window, mut job) = prepared();
        let address = &*job as *const CloseJob;
        let activation = job.commit(&mut window, &mut host).unwrap();
        assert!(job.0.is_none());
        assert!(job.commit(&mut window,&mut host).err().unwrap().contains("already consumed"));
        assert_eq!(&*job as *const CloseJob, address);
        drop(job);
        drop(activation);
    }
}
