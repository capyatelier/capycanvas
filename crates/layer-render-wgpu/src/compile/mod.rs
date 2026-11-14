use crate::{Deferred, GpuRasterError};
use std::{cell::Cell, collections::HashMap, sync::Arc, time::Duration};
use web_time::Instant;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(not(target_arch = "wasm32"))]
use native as platform;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::Coordinator;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::{retire, retire_device_services};
#[cfg(not(target_arch = "wasm32"))]
pub use native::{Activity, finish_shader_compiler_shutdown, stop_shader_compilers};
#[cfg(target_arch = "wasm32")]
use web as platform;
#[cfg(target_arch = "wasm32")]
pub(crate) use web::Coordinator;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Class {
    Required,
    Validation,
    Visible,
    Anticipated,
    Background,
}
impl Class {
    fn required(self) -> bool {
        self < Self::Visible
    }
}

thread_local! { static EXECUTING: Cell<bool> = const { Cell::new(false) }; }
struct Permit(bool);
impl Drop for Permit {
    fn drop(&mut self) {
        EXECUTING.set(self.0);
    }
}
pub(crate) fn execute<T>(work: impl FnOnce() -> T) -> T {
    let _permit = Permit(EXECUTING.replace(true));
    work()
}
pub(crate) fn assert_execution() {
    debug_assert!(
        EXECUTING.get(),
        "Cold recipe compiled outside the compilation coordinator"
    );
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait Resource: Send + Sync + 'static {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync + 'static> Resource for T {}
#[cfg(target_arch = "wasm32")]
pub(crate) trait Resource: 'static {}
#[cfg(target_arch = "wasm32")]
impl<T: 'static> Resource for T {}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait Task: FnOnce() -> Result<(), String> + Send + 'static {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: FnOnce() -> Result<(), String> + Send + 'static> Task for T {}
#[cfg(target_arch = "wasm32")]
pub(crate) trait Task: FnOnce() -> Result<(), String> + 'static {}
#[cfg(target_arch = "wasm32")]
impl<T: FnOnce() -> Result<(), String> + 'static> Task for T {}

struct Lease {
    coordinator: Arc<Coordinator>,
    id: u64,
}
#[derive(Clone)]
pub(crate) struct Scope(Arc<Lease>);
impl Coordinator {
    pub fn scope(self: &Arc<Self>) -> Option<Scope> {
        self.with_queue(|queue| {
            if queue.stopped {
                return None;
            }
            Some(Scope(Arc::new(Lease {
                coordinator: self.clone(),
                id: queue.owner(),
            })))
        })
    }
}

impl Scope {
    fn with_queue<T>(&self, work: impl FnOnce(&mut Queue<platform::Work>) -> T) -> T {
        self.0.coordinator.with_queue(work)
    }
    pub fn input(&self) {
        self.with_queue(Queue::input);
    }
    pub fn idle(&self, idle: bool, speculative_idle: bool) {
        let changed = self.with_queue(|q| {
            let o = q.owners.get_mut(&self.0.id).unwrap();
            if (o.idle, o.speculative_idle) == (idle, speculative_idle) {
                return false;
            }
            o.idle = idle;
            o.speculative_idle = speculative_idle;
            true
        });
        if changed {
            self.0.coordinator.notify();
        }
    }
    pub fn is_idle(&self) -> bool {
        self.with_queue(|q| q.idle())
    }
    pub fn delay(&self) -> Duration {
        self.with_queue(|q| q.delay(Instant::now()))
    }
    pub fn generation(&self) -> u64 {
        self.with_queue(|q| q.owners[&self.0.id].generation)
    }
    pub fn advance(&self) -> u64 {
        let generation = self.with_queue(|q| {
            let o = q.owners.get_mut(&self.0.id).unwrap();
            o.generation += 1;
            let g = o.generation;
            q.refresh();
            g
        });
        self.0.coordinator.drain();
        generation
    }
    pub fn require<'a, T: Resource>(
        &self,
        recipes: impl IntoIterator<Item = &'a Deferred<T>>,
        class: Class,
    ) -> bool {
        recipes.into_iter().fold(true, |ready, recipe| {
            if recipe.ready() {
                return ready;
            }
            self.request(recipe, class, None);
            false
        })
    }
    pub fn warm<T: Resource>(&self, recipe: &Deferred<T>, class: Class, generation: u64) {
        if !recipe.ready() {
            self.request(recipe, class, Some(generation));
        }
    }
    fn request<T: Resource>(&self, recipe: &Deferred<T>, class: Class, generation: Option<u64>) {
        let (changed,wake)=self.with_queue(|q| {
            let demand=Demand {owner:self.0.id,generation,class};
            if !q.live(&demand) {return (false,None);}
            if let Some(error)=recipe.error() {
                return (false,(q.fail(self.0.id,error.into()) && class<=Class::Visible).then(||q.owners[&self.0.id].wake.clone()).flatten());
            }
            (q.submit(Key::Recipe(recipe.identity()),demand,recipe.async_pipeline(),||platform::pipeline(recipe.clone())),None)
        });
        wake_owners(wake.map(|wake|(self.0.id,wake)),|owner,error|self.with_queue(|q|{q.fail(owner,error);}));
        if changed {self.0.coordinator.notify_one();}
    }
    pub fn enqueue(&self, class: Class, work: impl Task) -> u64 {
        self.submit(class, platform::task(work))
    }
    #[cfg(target_arch = "wasm32")]
    pub fn enqueue_async(
        &self,
        class: Class,
        work: impl FnOnce() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>
        + 'static,
    ) -> u64 {
        self.submit(class, platform::asynchronous(work))
    }
    fn submit(&self, class: Class, work: platform::Work) -> u64 {
        let id = self.with_queue(|q| {
            let id = q.next();
            q.submit(
                Key::Job(id),
                Demand {
                    owner: self.0.id,
                    generation: None,
                    class,
                },
                false,
                || work,
            );
            id
        });
        self.0.coordinator.notify_one();
        id
    }
    pub fn batch(&self) -> Batch {
        self.with_queue(|q| q.owners.get_mut(&self.0.id).unwrap().paused += 1);
        Batch(Arc::downgrade(&self.0))
    }
    pub fn pending(&self) -> usize {
        self.with_queue(|q| q.pending(self.0.id))
    }
    pub fn job_ready(&self, id: u64) -> bool {
        self.with_queue(|q| !q.entries.contains_key(&Key::Job(id)))
    }
    pub fn check(&self) -> Result<(), GpuRasterError> {
        self.with_queue(|q| q.check(self.0.id))
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn activity(&self) -> Activity {
        Activity(Arc::downgrade(&self.0))
    }
    pub fn set_wake(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        self.with_queue(|q| q.owners.get_mut(&self.0.id).unwrap().wake = Some(wake.clone()));
        wake_owners([(self.0.id,wake)],|owner,error|self.with_queue(|q|{q.fail(owner,error);}));
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn blocking<T>(&self, work: impl FnOnce() -> T) -> T {
        self.0.coordinator.blocking(work)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save(&self) {
        self.0.coordinator.save();
    }
    #[cfg(target_arch = "wasm32")]
    pub fn has_work(&self, allow_optional: bool) -> bool {
        self.with_queue(|q| {
            q.selected(Instant::now(), allow_optional).is_some()
                || allow_optional && q.optional != 0
        })
    }
    #[cfg(target_arch = "wasm32")]
    pub fn step(
        &self,
        allow_optional: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), GpuRasterError>>>> {
        self.0.coordinator.step(allow_optional)
    }
    pub async fn prepare_all<'a, T: Resource>(
        &self,
        recipes: impl IntoIterator<Item = &'a Deferred<T>>,
    ) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.check().map_err(|error|error.to_string())?;
            self.blocking(|| {
                for p in recipes {
                    self.check().map_err(|error|error.to_string())?;
                    if let Some(error) = p.error() {
                        return Err(error.into());
                    }
                    p.compile();
                }
                Ok(())
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            let recipes: Vec<_> = recipes.into_iter().cloned().collect();
            self.require(&recipes, Class::Required);
            while !recipes.iter().all(Deferred::ready) {
                self.step(false).await.map_err(|e| e.to_string())?;
                self.check().map_err(|e| e.to_string())?;
            }
            Ok(())
        }
    }
}
pub(crate) struct Batch(std::sync::Weak<Lease>);
impl Drop for Batch {
    fn drop(&mut self) {
        if let Some(lease) = self.0.upgrade() {
            lease
                .coordinator
                .with_queue(|q| q.owners.get_mut(&lease.id).unwrap().paused -= 1);
            lease.coordinator.notify();
        }
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.coordinator.with_queue(|q| {
            q.owners.remove(&self.id);
            q.refresh();
        });
        self.coordinator.drain();
    }
}

#[derive(Default)]
pub(crate) struct Needs {
    pub render: Vec<Deferred<wgpu::RenderPipeline>>,
    pub compute: Vec<Deferred<wgpu::ComputePipeline>>,
    pub styles: Vec<layer_render::DabStyle>,
}
impl Needs {
    pub fn ready(&self) -> bool {
        self.render.iter().all(Deferred::ready) && self.compute.iter().all(Deferred::ready)
    }
    pub fn covers(&self, style: &layer_render::DabStyle) -> bool {
        self.styles.contains(style) && self.ready()
    }
    pub fn require(&self, scope: &Scope, class: Class) -> bool {
        scope.require(&self.render, class) & scope.require(&self.compute, class)
    }
    pub fn warm(&self, scope: &Scope, class: Class, generation: u64) {
        for recipe in &self.render { scope.warm(recipe, class, generation); }
        for recipe in &self.compute { scope.warm(recipe, class, generation); }
    }
    pub async fn prepare(&self, scope: &Scope) -> Result<(), String> {
        scope.prepare_all(&self.render).await?;
        scope.prepare_all(&self.compute).await
    }
}

fn wake_owners(wakes:impl IntoIterator<Item=(u64,Arc<dyn Fn()+Send+Sync>)>,mut fail:impl FnMut(u64,String)) {
    for (owner,wake) in wakes {
        if let Err(payload)=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||wake())) {
            fail(owner,crate::panic_message(&*payload));
        }
    }
}

pub(super) const QUIET: Duration = Duration::from_millis(200);
pub(super) const BACKGROUND_QUIET: Duration = Duration::from_secs(1);
struct Owner {
    generation: u64,
    idle: bool,
    speculative_idle: bool,
    paused: usize,
    error: Option<String>,
    wake: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}
impl Default for Owner {
    fn default() -> Self {
        Self {
            generation: 0,
            idle: true,
            speculative_idle: true,
            paused: 0,
            error: None,
            wake: None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Key {
    Recipe(usize),
    Job(u64),
    #[cfg(not(target_arch = "wasm32"))]
    Save,
}
#[derive(Clone, Copy)]
struct Demand {
    owner: u64,
    generation: Option<u64>,
    class: Class,
}
struct Entry<W> {
    work: Option<W>,
    demands: Vec<Demand>,
    class: Class,
    sequence: u64,
    independent: bool,
    running: bool,
}
struct Running<W> {
    key: Key,
    class: Class,
    independent: bool,
    work: W,
}
struct Queue<W> {
    entries: HashMap<Key, Entry<W>>,
    owners: HashMap<u64, Owner>,
    sequence: u64,
    last_activity: Option<Instant>,
    required: usize,
    ordered: usize,
    optional: usize,
    workers: usize,
    stopped: bool,
    blocking: usize,
}
impl<W> Queue<W> {
    fn new(workers: usize) -> Self {
        Self {
            entries: HashMap::new(),
            owners: HashMap::new(),
            sequence: 0,
            last_activity: None,
            required: 0,
            ordered: 0,
            optional: 0,
            workers,
            stopped: false,
            blocking: 0,
        }
    }
    fn next(&mut self) -> u64 {
        self.sequence += 1;
        self.sequence
    }
    fn owner(&mut self) -> u64 {
        let id = self.next();
        self.owners.insert(id, Owner::default());
        id
    }
    fn input(&mut self) {
        self.last_activity = Some(Instant::now());
    }
    fn remaining(&self, class: Class, now: Instant) -> Duration {
        let quiet = if class == Class::Visible {
            QUIET
        } else {
            BACKGROUND_QUIET
        };
        self.last_activity.map_or(Duration::ZERO, |activity| {
            (activity + quiet).saturating_duration_since(now)
        })
    }
    fn idle(&self) -> bool {
        self.owners.values().all(|owner| owner.idle)
    }
    fn allows(&self, class: Class, now: Instant) -> bool {
        class.required()
            || self.idle()
                && (class == Class::Visible
                    || self.owners.values().all(|owner| owner.speculative_idle))
                && self.remaining(class, now).is_zero()
    }
    fn live(&self, demand: &Demand) -> bool {
        demand.owner == 0
            || self
                .owners
                .get(&demand.owner)
                .is_some_and(|owner| demand.generation.is_none_or(|g| g == owner.generation))
    }
    fn submit(
        &mut self,
        key: Key,
        demand: Demand,
        independent: bool,
        work: impl FnOnce() -> W,
    ) -> bool {
        if self.stopped {return false;}
        let sequence = self.next();
        let mut changed = false;
        let entry = self.entries.entry(key).or_insert_with(|| {
            changed = true;
            Entry {
                work: Some(work()),
                demands: Vec::new(),
                class: demand.class,
                sequence,
                independent,
                running: false,
            }
        });
        if !entry.demands.iter().any(|d| {
            d.owner == demand.owner && d.generation == demand.generation && d.class == demand.class
        }) {
            entry.demands.push(demand);
            changed = true;
        }
        if demand.class < entry.class {
            entry.class = demand.class;
            entry.sequence = sequence;
            changed = true;
        }
        changed
    }

    fn refresh(&mut self) {
        let owners = &self.owners;
        self.entries.retain(|_, entry| {
            entry.demands.retain(|d| {
                d.owner == 0
                    || owners
                        .get(&d.owner)
                        .is_some_and(|o| d.generation.is_none_or(|g| g == o.generation))
            });
            if let Some(class) = entry.demands.iter().map(|d| d.class).min()
                && class != entry.class
            {
                self.sequence += 1;
                entry.class = class;
                entry.sequence = self.sequence;
            }
            entry.running || !entry.demands.is_empty()
        });
    }
    fn selected(&self, now: Instant, allow_optional: bool) -> Option<Key> {
        if self.stopped {
            return None;
        }
        self.entries
            .iter()
            .filter(|(_, e)| {
                e.work.is_some()
                    && !e.running
                    && e.demands.iter().any(|d| {
                        self.live(d)
                            && d.class == e.class
                            && (d.owner == 0 || self.owners[&d.owner].paused == 0)
                    })
                    && (allow_optional || e.class.required())
                    && self.allows(e.class, now)
            })
            .min_by_key(|(_, e)| (e.class, e.sequence))
            .map(|(key, _)| *key)
    }
    fn take(&mut self, now: Instant, allow_optional: bool) -> Option<Running<W>> {
        self.refresh();
        let key = self.selected(now, allow_optional)?;
        let entry = &self.entries[&key];
        if if entry.class.required() {
            self.ordered != 0
                || self.required >= self.workers
                || !entry.independent && self.required != 0
        } else {
            self.required != 0
                || self.optional != 0
                || self.blocking != 0
                || self.entries.values().any(|e| {
                    e.class.required() && e.work.is_some() && e.demands.iter().any(|d| self.live(d))
                })
        } {
            return None;
        }
        let entry = self.entries.get_mut(&key).unwrap();
        entry.running = true;
        if entry.class.required() {
            self.required += 1;
            self.ordered += usize::from(!entry.independent);
        } else {
            self.optional += 1;
        }
        Some(Running {
            key,
            class: entry.class,
            independent: entry.independent,
            work: entry.work.take().unwrap(),
        })
    }
    fn finish(
        &mut self,
        key: Key,
        class: Class,
        independent: bool,
        result: Result<(), String>,
    ) -> Vec<(u64, std::sync::Arc<dyn Fn() + Send + Sync>)> {
        if class.required() {
            self.required -= 1;
            self.ordered -= usize::from(!independent);
            self.input();
        } else {
            self.optional -= 1;
        }
        if self
            .entries
            .get(&key)
            .is_some_and(|entry| entry.class.required())
        {
            self.input();
        }
        let Some(entry) = self.entries.get_mut(&key) else {
            return Vec::new();
        };
        entry.running = false;
        let error = result.err();
        let mut wakes = Vec::new();
        for demand in &entry.demands {
            if let Some(owner) = self.owners.get_mut(&demand.owner)
                && demand.generation.is_none_or(|g| g == owner.generation)
            {
                if let Some(error) = &error {
                    owner.error.get_or_insert(error.clone());
                }
                if demand.generation.is_none()
                    && let Some(wake) = &owner.wake
                {
                    wakes.push((demand.owner, wake.clone()));
                }
            }
        }
        self.entries.remove(&key);
        wakes
    }
    fn pending(&self, owner: u64) -> usize {
        self.entries
            .values()
            .filter(|e| {
                (e.work.is_some() || e.running)
                    && e.demands
                        .iter()
                        .any(|d| {
                            d.owner == owner
                                && d.generation.is_none()
                                && d.class.required()
                                && self.live(d)
                        })
            })
            .count()
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn drained(&self) -> bool {
        self.required == 0
            && self.optional == 0
            && self.blocking == 0
            && !self
                .entries
                .values()
                .any(|e| e.work.is_some() && e.demands.iter().any(|d| self.live(d)))
    }
    fn delay(&self, now: Instant) -> Duration {
        self.entries
            .values()
            .filter(|e| {
                e.work.is_some() && !e.class.required() && e.demands.iter().any(|d| self.live(d))
            })
            .map(|e| self.remaining(e.class, now))
            .min()
            .unwrap_or_else(|| self.remaining(Class::Background, now))
    }
    fn fail(&mut self,owner:u64,error:String)->bool {
        let Some(owner)=self.owners.get_mut(&owner) else {return false;};
        if owner.error.is_some() {return false;}
        owner.error=Some(error);true
    }
    fn check(&self, owner: u64) -> Result<(), GpuRasterError> {
        self.owners
            .get(&owner)
            .and_then(|o| o.error.as_ref())
            .map_or(Ok(()), |e| Err(GpuRasterError::Effect(e.clone())))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "native_tests.rs"]
mod native_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod tests;

#[cfg(test)]
mod admission_tests;

#[cfg(test)]
mod completion_tests;
