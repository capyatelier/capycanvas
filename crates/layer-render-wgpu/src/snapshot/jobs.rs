use super::*;

#[cfg(not(target_arch = "wasm32"))]
impl SnapshotGpu {
    async fn run_snapshot(&self, request: layer_render::SnapshotRequest, control: CaptureControl)
        -> Result<layer_render::SnapshotResult, String> {
        match request {
            layer_render::SnapshotRequest::LevelsStatistics(query)=>self.levels_statistics(query,control).await.map(layer_render::SnapshotResult::LevelsStatistics),
            layer_render::SnapshotRequest::ArtworkStatistics(request) => self.artwork_statistics(request, control).await.map(layer_render::SnapshotResult::ArtworkStatistics),
            layer_render::SnapshotRequest::ArtworkSample(request) => self.artwork_sample(request, control).await.map(layer_render::SnapshotResult::ArtworkSample),
            layer_render::SnapshotRequest::Bounds(request) => self.content_bounds(request, control).await.map(layer_render::SnapshotResult::Bounds),
            layer_render::SnapshotRequest::TransformPixels(plan) => self.transform_pixels(plan, control).await
                .map(layer_render::SnapshotResult::TransformPixels),
            layer_render::SnapshotRequest::Image(capture) => self.image_capture(capture, control).await
                .map(layer_render::SnapshotResult::Image),
            layer_render::SnapshotRequest::Remap(plan) => self.remap(plan, control).await
                .map(layer_render::SnapshotResult::Remap),
        }
    }
}

enum Outcome<T> {Pending,Ready(Result<T,String>),Taken}
struct Completion<T> {outcome:Outcome<T>,wake:Option<Arc<dyn Fn()+Send+Sync>>}
struct Publisher<T>(Option<Arc<std::sync::Mutex<Completion<T>>>>);
impl<T> Publisher<T> {
    fn publish(shared:&Arc<std::sync::Mutex<Completion<T>>>,result:Result<T,String>) {
        let wake={
            let mut completion=shared.lock().unwrap();
            if matches!(completion.outcome,Outcome::Pending) {
                completion.outcome=Outcome::Ready(result);completion.wake.clone()
            }else {None}
        };
        if let Some(wake)=wake {wake();}
    }
    fn send(mut self,result:Result<T,String>) {Self::publish(&self.0.take().unwrap(),result);}
}
impl<T> Drop for Publisher<T> {
    fn drop(&mut self) {if let Some(shared)=self.0.take() {Self::publish(&shared,Err("The operation stopped".into()));}}
}
pub struct Job<T> {
    control: CaptureControl,
    completion:Arc<std::sync::Mutex<Completion<T>>>,
    #[cfg(not(target_arch = "wasm32"))]
    worker: Option<std::thread::JoinHandle<()>>,
}
pub type SnapshotJob = Job<layer_render::SnapshotResult>;
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.control.cancel();
        #[cfg(not(target_arch = "wasm32"))]
        crate::compile::retire(self.worker.take());
    }
}
#[cfg(target_arch = "wasm32")]
pub type BrowserSnapshot = std::rc::Rc<dyn Fn(layer_render::SnapshotRequest, CaptureControl)
    -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<layer_render::SnapshotResult, String>>>>>;

impl<T> Job<T> {
    fn pending(control:CaptureControl)->(Self,Publisher<T>) {
        let completion=Arc::new(std::sync::Mutex::new(Completion {outcome:Outcome::Pending,wake:None}));
        (Self {control,completion:completion.clone(),#[cfg(not(target_arch="wasm32"))] worker:None},Publisher(Some(completion)))
    }
    pub fn set_wake(&self,wake:Arc<dyn Fn()+Send+Sync>) {
        let (previous,notify)={
            let mut completion=self.completion.lock().unwrap();
            if matches!(completion.outcome,Outcome::Taken) {return;}
            let previous=completion.wake.replace(wake.clone());
            (previous,matches!(completion.outcome,Outcome::Ready(_)))
        };
        drop(previous);
        if notify {wake();}
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn spawn(name: &str, work: impl FnOnce(CaptureControl) -> Result<T, String> + Send + 'static) -> Result<Self, String>
    where T: Send + 'static {
        let control = CaptureControl::default();
        let capture = control.clone();
        let (mut job,publisher)=Self::pending(control);
        let worker = std::thread::Builder::new().name(name.into()).stack_size(8 * 1024 * 1024).spawn(move || {
            publisher.send(work(capture));
        }).map_err(|e| e.to_string())?;
        job.worker=Some(worker);Ok(job)
    }
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn spawn_local<F: std::future::Future<Output = Result<T, String>> + 'static>(work: impl FnOnce(CaptureControl) -> F + 'static) -> Self
    where T: 'static {
        let control = CaptureControl::default();
        let capture = control.clone();
        let (job,publisher)=Self::pending(control);
        wasm_bindgen_futures::spawn_local(async move {
            publisher.send(work(capture).await);
        });
        job
    }
    pub fn take(&mut self) -> Option<Result<T, String>> {
        let (result,wake)={
            let mut completion=self.completion.lock().unwrap();
            match std::mem::replace(&mut completion.outcome,Outcome::Taken) {
                Outcome::Pending=> {completion.outcome=Outcome::Pending;(None,None)},
                Outcome::Ready(result)=>(Some(result),completion.wake.take()),
                Outcome::Taken=>(None,None),
            }
        };
        drop(wake);result
    }
}
impl SnapshotJob {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start(gpu: SnapshotGpu, request: layer_render::SnapshotRequest) -> Result<Self, String> {
        let wake=gpu.preparation_wake.clone();
        let job=Self::spawn("capy-snapshot", move |control| pollster::block_on(gpu.run_snapshot(request, control)))?;
        if let Some(wake)=wake {job.set_wake(wake);}
        Ok(job)
    }
    #[cfg(target_arch = "wasm32")]
    pub fn start(worker: BrowserSnapshot, request: layer_render::SnapshotRequest) -> Result<Self, String> {
        Ok(Self::spawn_local(move |control| worker(request, control)))
    }
}

#[cfg(test)]
mod completion_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize,Ordering};
    fn counter()->(Arc<AtomicUsize>,Arc<dyn Fn()+Send+Sync>) {
        let calls=Arc::new(AtomicUsize::new(0));let count=calls.clone();
        (calls,Arc::new(move || {count.fetch_add(1,Ordering::Relaxed);}))
    }
    #[test]
    fn completion_before_subscription_keeps_success_and_error() {
        for result in [Ok(7u32),Err("analysis failure".into())] {
            let (mut job,publisher)=Job::pending(CaptureControl::default());
            publisher.send(result.clone());
            let (calls,wake)=counter();job.set_wake(wake);
            assert_eq!(calls.load(Ordering::Relaxed),1);
            assert_eq!(job.take(),Some(result));assert_eq!(job.take(),None);
        }
    }
    #[test]
    fn completion_after_subscription_publishes_success_and_error_before_waking() {
        for result in [Ok(11u32),Err("analysis failure".into())] {
            let (mut job,publisher)=Job::pending(CaptureControl::default());
            let (calls,wake)=counter();job.set_wake(wake);assert_eq!(calls.load(Ordering::Relaxed),0);
            publisher.send(result.clone());
            assert_eq!(calls.load(Ordering::Relaxed),1);assert_eq!(job.take(),Some(result));
        }
    }
    #[test]
    fn an_abandoned_publisher_wakes_with_the_existing_operation_stopped_error() {
        let (mut job,publisher)=Job::<u32>::pending(CaptureControl::default());
        let (calls,wake)=counter();job.set_wake(wake);drop(publisher);
        assert_eq!(calls.load(Ordering::Relaxed),1);
        assert_eq!(job.take(),Some(Err("The operation stopped".into())));assert_eq!(job.take(),None);
    }
    #[cfg(not(target_arch="wasm32"))]
    #[test]
    fn a_panicked_worker_wakes_a_late_waiter_with_the_existing_stopped_error() {
        let mut job=Job::<u32>::spawn("capy-analysis-panic-test",|_|panic!("analysis worker failed")).unwrap();
        assert!(job.worker.take().unwrap().join().is_err());
        let (calls,wake)=counter();job.set_wake(wake);assert_eq!(calls.load(Ordering::Relaxed),1);
        assert_eq!(job.take(),Some(Err("The operation stopped".into())));
    }
    #[test]
    fn publication_and_late_subscription_notify_outside_the_completion_mutex() {
        for late in [false,true] {
            let (mut job,publisher)=Job::pending(CaptureControl::default());
            let state=Arc::downgrade(&job.completion);
            let wake:Arc<dyn Fn()+Send+Sync>=Arc::new(move || {
                let state=state.upgrade().unwrap();let completion=state.try_lock().expect("completion callback holds the result mutex");
                assert!(matches!(completion.outcome,Outcome::Ready(Ok(13))));
            });
            if late {publisher.send(Ok(13u32));job.set_wake(wake);}else {job.set_wake(wake);publisher.send(Ok(13u32));}
            assert_eq!(job.take(),Some(Ok(13)));
        }
    }
    #[test]
    fn taking_the_result_releases_the_subscription_and_does_not_rearm_it() {
        let (mut job,publisher)=Job::pending(CaptureControl::default());
        let (calls,wake)=counter();let subscription=Arc::downgrade(&wake);job.set_wake(wake);
        publisher.send(Ok(17u32));assert_eq!(calls.load(Ordering::Relaxed),1);
        assert_eq!(job.take(),Some(Ok(17)));assert!(subscription.upgrade().is_none());
        let (_,wake)=counter();job.set_wake(wake);assert_eq!(job.take(),None);
    }

}
