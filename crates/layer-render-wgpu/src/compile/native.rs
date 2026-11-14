use super::*;
use std::sync::{Arc, Condvar, Mutex, Weak};

pub(super) type Work = Box<dyn FnOnce() -> Result<(), String> + Send>;
pub(super) struct Shared {
    pub(super) queue: Mutex<Queue<Work>>,
    wake: Condvar,
    saved: Option<Arc<crate::SavedPipelines>>,
}
pub(crate) struct Coordinator {
    pub(super) shared: Arc<Shared>,
    threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
    poll: crate::device_poll::Binding,
}
#[derive(Clone)]
pub struct Activity(pub(super) Weak<Lease>);
static RETIRED: Mutex<Vec<std::thread::JoinHandle<()>>> = Mutex::new(Vec::new());
static LIVE: Mutex<Vec<Weak<Coordinator>>> = Mutex::new(Vec::new());
pub(crate) fn retire(workers: impl IntoIterator<Item = std::thread::JoinHandle<()>>) {
    let mut retired = RETIRED.lock().unwrap();
    retired.retain(|worker| !worker.is_finished());
    retired.extend(workers);
}
fn live_coordinators() -> Vec<Arc<Coordinator>> {
    LIVE.lock().unwrap().iter().filter_map(Weak::upgrade).collect()
}
pub(crate) fn retire_device_services(poll: crate::device_poll::Binding, root: impl Send + 'static) -> std::io::Result<()> {
    let worker = std::thread::Builder::new().name("capy-retired-device".into()).spawn(move || {
        poll.retire_lost(|| {
            let live: Vec<_> = live_coordinators().into_iter()
                .filter(|coordinator| coordinator.poll.same(&poll)).collect();
            for coordinator in &live { coordinator.stop(true); }
            for coordinator in live { coordinator.join(); }
        });
        drop(root);
    })?;
    retire([worker]);
    Ok(())
}
pub fn finish_shader_compiler_shutdown() {
    static FINISHING: Mutex<()> = Mutex::new(());
    let _finishing = FINISHING.lock().unwrap();
    loop {
        let mut joined = false;
        for coordinator in live_coordinators() {
            if coordinator.stop(false) {
                joined |= coordinator.join();
            }
        }
        let threads = std::mem::take(&mut *RETIRED.lock().unwrap());
        if threads.is_empty() && !joined {
            break;
        }
        for thread in threads {
            let _ = thread.join();
        }
    }
}
pub fn stop_shader_compilers() {
    for coordinator in live_coordinators() {
        coordinator.stop(false);
    }
}
pub(super) fn workers(parallelism: usize) -> usize {
    (parallelism * 3).div_ceil(8).clamp(1, 6)
}
impl Coordinator {
    pub fn new(device: &crate::PipelineDevice) -> Result<Arc<Self>, GpuRasterError> {
        let poll = device.poll_binding();
        poll.admit(|| Self::build(
            std::thread::available_parallelism().map_or(1, |n| workers(n.get())),
            device.saved_pipelines().cloned(),
            poll.owned(),
        ))
    }
    #[cfg(test)]
    pub(super) fn with_workers(
        workers: usize,
        saved: Option<Arc<crate::SavedPipelines>>,
    ) -> Result<Arc<Self>, GpuRasterError> {
        Self::build(workers, saved, crate::device_poll::Binding::default())
    }
    #[cfg(test)]
    pub(super) fn with_binding(workers: usize, poll: &crate::device_poll::Binding) -> Result<Arc<Self>, GpuRasterError> {
        poll.admit(|| Self::build(workers, None, poll.owned()))
    }
    fn build(
        workers: usize,
        saved: Option<Arc<crate::SavedPipelines>>,
        poll: crate::device_poll::Binding,
    ) -> Result<Arc<Self>, GpuRasterError> {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::new(workers)),
            wake: Condvar::new(),
            saved,
        });
        let coordinator = Self {
            shared,
            threads: Mutex::new(Vec::with_capacity(workers + 1)),
            poll,
        };
        for index in 0..=workers {
            let shared = coordinator.shared.clone();
            coordinator.threads.lock().unwrap().push(
                std::thread::Builder::new()
                    .name(format!("capy-shaders-{index}"))
                    .spawn(move || compile(shared))
                    .map_err(|e| GpuRasterError::Effect(e.to_string()))?,
            );
        }
        let coordinator = Arc::new(coordinator);
        let mut live = LIVE.lock().unwrap();
        live.retain(|owner| owner.strong_count() != 0);
        live.push(Arc::downgrade(&coordinator));
        Ok(coordinator)
    }
    pub(super) fn stop(&self, force: bool) -> bool {
        let mut queue = self.shared.queue.lock().unwrap();
        if !force && (!queue.owners.is_empty() || queue.sequence == 0) {
            return false;
        }
        queue.stopped = true;
        queue.entries.clear();
        let wakes=if force {
            queue.owners.iter_mut().filter_map(|(&id,owner)| {
                owner.error.get_or_insert_with(||"GPU device service retired".into());
                owner.wake.clone().map(|wake|(id,wake))
            }).collect::<Vec<_>>()
        }else {Vec::new()};
        drop(queue);
        wake_owners(wakes,|owner,error|self.with_queue(|queue|{queue.fail(owner,error);}));
        self.notify();
        true
    }
    pub(super) fn join(&self) -> bool {
        let mut threads = self.threads.lock().unwrap();
        let joined = !threads.is_empty();
        for thread in threads.drain(..) {
            let _ = thread.join();
        }
        joined
    }
    pub(super) fn with_queue<T>(&self, work: impl FnOnce(&mut Queue<Work>) -> T) -> T {
        work(&mut self.shared.queue.lock().unwrap())
    }
    pub(super) fn notify(&self) {
        self.shared.wake.notify_all();
    }
    pub(super) fn notify_one(&self) {
        self.shared.wake.notify_one();
    }
    pub(super) fn drain(&self) {
        let mut queue = self.shared.queue.lock().unwrap();
        if queue.drained() {
            schedule_save(&self.shared, &mut queue);
        }
        drop(queue);
        self.shared.wake.notify_all();
    }
    pub(super) fn blocking<T>(&self, work: impl FnOnce() -> T) -> T {
        self.shared.queue.lock().unwrap().blocking += 1;
        let _blocking = Blocking(&self.shared);
        execute(work)
    }
    pub(super) fn save(&self) {
        let mut queue = self.shared.queue.lock().unwrap();
        schedule_save(&self.shared, &mut queue);
        drop(queue);
        self.notify_one();
    }
}
pub(super) fn pipeline<T: Resource>(recipe: Deferred<T>) -> Work {
    Box::new(move || {
        recipe.compile();
        Ok(())
    })
}
pub(super) fn task(work: impl Task) -> Work {
    Box::new(work)
}
struct Blocking<'a>(&'a Shared);
impl Drop for Blocking<'_> {
    fn drop(&mut self) {
        let mut queue = self.0.queue.lock().unwrap();
        queue.blocking -= 1;
        if queue.drained() {
            schedule_save(self.0, &mut queue);
        }
        drop(queue);
        self.0.wake.notify_all();
    }
}
impl Activity {
    pub fn input(&self) {
        if let Some(lease) = self.0.upgrade() {
            Scope(lease).input();
        }
    }
    pub fn idle(&self, idle: bool, speculative_idle: bool) {
        if let Some(lease) = self.0.upgrade() {
            Scope(lease).idle(idle, speculative_idle);
        }
    }
}
fn schedule_save(shared: &Shared, queue: &mut Queue<Work>) {
    if queue.stopped {
        return;
    }
    if let Some(store) = shared.saved.as_ref()
        && let Some(save) = store.claim_save()
    {
        queue.submit(
            Key::Save,
            Demand {
                owner: 0,
                generation: None,
                class: Class::Background,
            },
            false,
            || Box::new(move || {
                save.run();
                Ok(())
            }),
        );
    }
}
fn compile(shared: Arc<Shared>) {
    loop {
        let job = {
            let mut queue = shared.queue.lock().unwrap();
            loop {
                if queue.stopped {
                    return;
                }
                if let Some(job) = queue.take(Instant::now(), true) {
                    break job;
                }
                let delay = queue.delay(Instant::now());
                queue = if queue.idle() && !delay.is_zero() {
                    shared.wake.wait_timeout(queue, delay).unwrap().0
                } else {
                    shared.wake.wait(queue).unwrap()
                };
            }
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute(job.work)))
            .unwrap_or_else(|payload| Err(crate::panic_message(&*payload)));
        let wakes = {
            let mut queue = shared.queue.lock().unwrap();
            let wakes = queue.finish(job.key, job.class, job.independent, result);
            if queue.drained() {
                schedule_save(&shared, &mut queue);
            }
            wakes
        };
        shared.wake.notify_all();
        wake_owners(wakes,|owner,error|{shared.queue.lock().unwrap().fail(owner,error);});
    }
}
impl Drop for Coordinator {
    fn drop(&mut self) {
        self.stop(true);
        self.join();
    }
}
