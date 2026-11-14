use std::sync::{Arc,Mutex,OnceLock,Weak,atomic::{AtomicBool,Ordering::SeqCst},mpsc};

type Action=Box<dyn FnOnce()+Send>;
type Notify=Arc<dyn Fn()+Send+Sync>;
trait Conditional:Send {fn ready(&self)->bool;}
struct Waiting<T,R> {value:T,ready:R}
impl<T:Send,R:Fn(&T)->bool+Send> Conditional for Waiting<T,R> {fn ready(&self)->bool {(self.ready)(&self.value)}}
trait Abandon:Send+Sync {fn abandon(&self);}
struct Ticket<F> {value:Mutex<Option<F>>}
impl<F:Send> Abandon for Ticket<F> {fn abandon(&self) {let value=self.value.lock().unwrap().take();drop(value);}}
enum Message {Dispose(Action),AfterGpu(Action),Flush(mpsc::Sender<()>)}
enum Sink {Open(mpsc::Sender<Message>),Closing {sender:mpsc::Sender<Message>,after_gpu:Vec<Action>},Closed}
struct Mailbox {
    sink:Mutex<Sink>,
    retired:Mutex<Vec<Box<dyn Conditional>>>,
    tickets:Mutex<Option<Vec<Weak<dyn Abandon>>>>,
    waiters:Mutex<Vec<(crate::residency::Blocked,Notify)>>,
    armed:AtomicBool,
    released:AtomicBool,
    terminal:Mutex<Vec<Notify>>,
    completion:Mutex<Vec<Notify>>,
    completions:std::sync::atomic::AtomicU64,
    thread:OnceLock<std::thread::Thread>,
}
#[derive(Clone)]
pub(crate) struct RetirementHandle(Arc<Mailbox>);
impl RetirementHandle {
    fn channel()->(Self,mpsc::Receiver<Message>) {
        let (sender,receiver)=mpsc::channel();
        (Self(Arc::new(Mailbox {sink:Mutex::new(Sink::Open(sender)),retired:Default::default(),tickets:Mutex::new(Some(Vec::new())),waiters:Default::default(),armed:AtomicBool::new(false),released:AtomicBool::new(false),terminal:Default::default(),completion:Default::default(),completions:Default::default(),thread:OnceLock::new()})),receiver)
    }
    fn wake(&self) {if let Some(thread)=self.0.thread.get() {thread.unpark();}}
    fn send(&self,message:Message)->Result<(),Message> {
        let result={let sink=self.0.sink.lock().unwrap();match &*sink {
            Sink::Open(sender)|Sink::Closing {sender,..}=>sender.send(message).map_err(|error|error.0),
            Sink::Closed=>Err(message),
        }};
        self.wake();result
    }
    fn close(&self) {
        let mut sink=self.0.sink.lock().unwrap();
        if let Sink::Open(sender)=&*sink {*sink=Sink::Closing {sender:sender.clone(),after_gpu:Vec::new()};}
        drop(sink);
        let terminal=std::mem::take(&mut *self.0.terminal.lock().unwrap());for wake in terminal {wake();}
        self.wake();
    }
    fn closed(&self)->bool {!matches!(*self.0.sink.lock().unwrap(),Sink::Open(_))}
    pub fn run(&self,action:impl FnOnce()+Send+'static) {
        if let Err(Message::Dispose(action))=self.send(Message::Dispose(Box::new(action))) {action();}
    }
    fn after_gpu(&self,action:Action) {
        let action={let mut sink=self.0.sink.lock().unwrap();match &mut *sink {
            Sink::Open(sender)=>match sender.send(Message::AfterGpu(action)) {Ok(())=>None,Err(error)=>{let Message::AfterGpu(action)=error.0 else {unreachable!()};Some(action)}},
            Sink::Closing {after_gpu,..}=>{after_gpu.push(action);None},
            Sink::Closed=>Some(action),
        }};
        self.wake();if let Some(action)=action {action();}
    }
    pub fn retirement<T:Send+'static>(&self,value:T)->Retirement<T> {Retirement {value:Some(value),handle:self.clone()}}
    pub fn dispose_when<T:Send+'static>(&self,value:T,ready:impl Fn(&T)->bool+Send+'static) {
        self.0.retired.lock().unwrap().push(Box::new(Waiting {value,ready}));self.pulse();
    }
    fn reap(&self) {
        let ready={let mut retired=self.0.retired.lock().unwrap();let mut ready=Vec::new();let mut index=0;
            while index<retired.len() {if retired[index].ready() {ready.push(retired.swap_remove(index));}else {index+=1;}}ready};
        drop(ready);
    }
    fn release(&self) {
        let closed=self.closed();
        let woken={let mut waiters=self.0.waiters.lock().unwrap();
            let woken=waiters.iter().filter(|(blocked,_)|closed || blocked.fits()).map(|(_,wake)|wake.clone()).collect::<Vec<_>>();
            waiters.retain(|(_,wake)|!woken.iter().any(|woken|Arc::ptr_eq(woken,wake)));
            self.0.armed.store(!waiters.is_empty(),SeqCst);woken};
        for (index,wake) in woken.iter().enumerate() {if !woken[..index].iter().any(|earlier|Arc::ptr_eq(earlier,wake)) {wake();}}
    }
    pub fn signal_release(&self) {
        if self.0.armed.load(SeqCst) {self.0.released.store(true,SeqCst);self.wake();}
    }
    fn pulse(&self) {
        let handle=self.clone();self.run(move||handle.reap());
    }
    pub fn on_next_completion(&self,wake:&Notify)->u64 {
        let mut completion=self.0.completion.lock().unwrap();
        if !completion.iter().any(|old|Arc::ptr_eq(old,wake)) {completion.push(wake.clone());}
        drop(completion);self.0.completions.load(SeqCst)
    }
    fn completed(&self) {
        self.0.completions.fetch_add(1,SeqCst);
        let wakes=std::mem::take(&mut *self.0.completion.lock().unwrap());for wake in wakes {wake();}
    }
    pub fn release_wake(&self)->Notify {let handle=self.clone();Arc::new(move||handle.pulse())}
    pub(crate) fn same(&self,other:&Self)->bool {Arc::ptr_eq(&self.0,&other.0)}
    pub(crate) fn wait_for_release(&self,blocked:&crate::residency::Blocked,wake:Notify) {
        let edge=blocked.bind(self);
        debug_assert!(edge.same(self),"a tier signals only the poll owner it first waited on");
        let notify={let mut waiters=edge.0.waiters.lock().unwrap();edge.0.armed.store(true,SeqCst);
            let fits=blocked.fits() || edge.closed();
            if !fits {match waiters.iter_mut().find(|(waiting,old)|Arc::ptr_eq(old,&wake) && waiting.same_wait(blocked)) {
                Some((waiting,_))=>if blocked.need()<waiting.need() {*waiting=blocked.clone();},
                None=>waiters.push((blocked.clone(),wake.clone())),
            }}
            fits};
        if notify {wake();}
    }
    fn callback<A:Send+'static,F:FnOnce(A)+Send+'static>(&self,callback:Option<F>)->impl FnOnce(A)+Send+'static+use<A,F> {
        let ticket=Arc::new(Ticket {value:Mutex::new(callback)});
        if ticket.value.lock().unwrap().is_some() {
            let erased:Arc<dyn Abandon>=ticket.clone();
            let mut tickets=self.0.tickets.lock().unwrap();
            if let Some(tickets)=tickets.as_mut() {
                tickets.retain(|ticket|ticket.strong_count()!=0);tickets.push(Arc::downgrade(&erased));
            }else {drop(tickets);ticket.abandon();}
        }
        let handle=self.clone();move|argument| {
            let callback=ticket.value.lock().unwrap().take();
            if let Some(callback)=callback {callback(argument);handle.pulse();handle.completed();}
        }
    }
    pub fn on_retired(&self,wake:Notify) {
        let notify={let mut terminal=self.0.terminal.lock().unwrap();if self.closed() {true}else {
            if !terminal.iter().any(|old|Arc::ptr_eq(old,&wake)) {terminal.push(wake.clone());}false
        }};
        if notify {wake();}
    }
    fn abandon(&self) {
        let tickets=self.0.tickets.lock().unwrap().take().unwrap_or_default();
        for ticket in tickets {if let Some(ticket)=ticket.upgrade() {ticket.abandon();}}
        self.reap();self.release();self.completed();
    }
    pub fn flush(&self)->Result<(),String> {
        if self.0.thread.get().is_some_and(|thread|thread.id()==std::thread::current().id()) {return Err("Device poll callback cannot wait on itself".into());}
        let (send,ready)=mpsc::channel();
        if self.send(Message::Flush(send)).is_err() {self.reap();return Ok(());}
        ready.recv_timeout(crate::READBACK_TIMEOUT).map_err(|error|error.to_string())
    }
}
pub(crate) struct Retirement<T:Send+'static> {value:Option<T>,handle:RetirementHandle}
impl<T:Send+'static> Retirement<T> {
    pub fn complete(mut self) {if let Some(value)=self.value.take() {self.handle.run(move||drop(value));}}
}
impl<T:Send+'static> Drop for Retirement<T> {fn drop(&mut self) {if let Some(value)=self.value.take() {self.handle.after_gpu(Box::new(move||drop(value)));}}}
struct Owner {handle:RetirementHandle,worker:Option<std::thread::JoinHandle<Action>>}
impl Owner {
    fn start(mut poll:impl FnMut()->Result<bool,String>+Send+'static,handle:RetirementHandle,receiver:mpsc::Receiver<Message>,queue:Arc<OnceLock<wgpu::Queue>>)->std::io::Result<Self> {
        let worker_handle=handle.clone();
        let worker=std::thread::Builder::new().name("capy-gpu-poll".into()).spawn(move|| {
            let mut failure=None;
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(&mut poll,&worker_handle,&receiver,&queue,&mut failure);
                worker_handle.close();finish(&worker_handle,&receiver);
            }));
            Box::new(move|| {
                drop(poll);drop(queue);drop(receiver);
                if let Some(panic)=failure.or_else(||result.err()) {std::panic::resume_unwind(panic);}
            }) as Action
        })?;
        handle.0.thread.set(worker.thread().clone()).unwrap();worker.thread().unpark();
        Ok(Self {handle,worker:Some(worker)})
    }
    fn shutdown(&mut self) {
        self.handle.close();
        if let Some(worker)=self.worker.take() {
            assert_ne!(worker.thread().id(),std::thread::current().id(),"device root cannot be owned by its poll callback");
            worker.join().expect("device poll owner panicked")();
        }
    }
}
impl Drop for Owner {fn drop(&mut self) {self.shutdown();}}
enum State {New {handle:RetirementHandle,receiver:mpsc::Receiver<Message>},Running(Owner),Retired(RetirementHandle)}
pub(crate) struct Root {state:Mutex<State>,queue:Arc<OnceLock<wgpu::Queue>>}
impl Drop for Root {
    fn drop(&mut self) {
        let state=self.state.get_mut().unwrap();
        match state {
            State::Running(owner)=>owner.shutdown(),
            State::New {handle,receiver}=>{handle.close();drain_idle(handle,receiver);},
            State::Retired(_)=>{},
        }
    }
}
#[derive(Clone)]
pub(crate) enum Binding {Owned(Arc<Root>),Recipe(Weak<Root>)}
impl Default for Binding {
    fn default()->Self {let (handle,receiver)=RetirementHandle::channel();Self::Owned(Arc::new(Root {state:Mutex::new(State::New {handle,receiver}),queue:Default::default()}))}
}
impl Binding {
    fn root(&self)->Option<Arc<Root>> {match self {Self::Owned(root)=>Some(root.clone()),Self::Recipe(root)=>root.upgrade()}}
    pub fn owned(&self)->Self {self.root().map(Self::Owned).unwrap_or_else(||self.clone())}
    pub fn recipe(&self)->Self {Self::Recipe(self.root().as_ref().map_or_else(Weak::new,Arc::downgrade))}
    pub fn same(&self,other:&Self)->bool {match (self.root(),other.root()) {(Some(a),Some(b))=>Arc::ptr_eq(&a,&b),_=>false}}
    pub fn bind_queue(&self,queue:&wgpu::Queue) {if let Some(root)=self.root() {root.queue.get_or_init(||queue.clone());}}
    fn live(state:&State)->bool {match state {State::New {handle,..}=>!handle.closed(),State::Running(owner)=>!owner.handle.closed(),State::Retired(_)=>false}}
    fn state_handle(state:&State)->RetirementHandle {match state {State::New {handle,..}|State::Retired(handle)=>handle.clone(),State::Running(owner)=>owner.handle.clone()}}
    #[cfg(test)]
    pub fn is_retired(&self)->bool {self.root().is_none_or(|root|!Self::live(&root.state.lock().unwrap()))}
    pub fn admit<T>(&self,work:impl FnOnce()->Result<T,crate::GpuRasterError>)->Result<T,crate::GpuRasterError> {
        let root=self.root().ok_or_else(||crate::GpuRasterError::WaitFailed("GPU device service retired".into()))?;
        let state=root.state.lock().unwrap();
        if !Self::live(&state) {return Err(crate::GpuRasterError::WaitFailed("GPU device service retired".into()));}work()
    }
    pub fn handle(&self,device:&wgpu::Device)->RetirementHandle {
        let Some(root)=self.root() else {let (handle,_)=RetirementHandle::channel();handle.close();return handle;};
        let mut state=root.state.lock().unwrap();let handle=Self::state_handle(&state);
        if matches!(*state,State::New {..}) && !handle.closed() {
            let State::New {receiver,..}=std::mem::replace(&mut *state,State::Retired(handle.clone())) else {unreachable!()};
            let device=device.clone();
            match Owner::start(move||device.poll(wgpu::PollType::Poll).map(|status|!status.is_queue_empty()).map_err(|error|error.to_string()),handle.clone(),receiver,root.queue.clone()) {
                Ok(owner)=>*state=State::Running(owner),
                Err(error)=>{handle.close();panic!("Device poll owner failed: {error}");},
            }
        }
        handle
    }
    pub fn callback<A:Send+'static,F:FnOnce(A)+Send+'static>(&self,device:&wgpu::Device,callback:F)->impl FnOnce(A)+Send+'static+use<A,F> {
        let handle=self.handle(device);
        let root=self.root();
        let state=root.as_ref().map(|root|root.state.lock().unwrap());
        if state.as_ref().is_some_and(|state|Self::live(state)) {drop(state);handle.callback(Some(callback))}
        else {drop(state);drop(callback);handle.callback::<A,F>(None)}
    }
    pub fn request(&self,device:&wgpu::Device) {let handle=self.handle(device);if !handle.closed() {handle.wake();}}
    pub fn retire_lost(&self,before_close:impl FnOnce()) {
        let Some(root)=self.root() else {return;};
        let previous={let mut state=root.state.lock().unwrap();let handle=Self::state_handle(&state);std::mem::replace(&mut *state,State::Retired(handle))};
        Self::state_handle(&previous).close();before_close();
        match previous {
            State::Running(mut owner)=>owner.shutdown(),
            State::New {handle,receiver}=>{drain_idle(&handle,&receiver);},
            State::Retired(_)=>{},
        }
    }
    pub fn submit<T>(&self,work:impl FnOnce()->T)->Option<T> {
        let Some(root)=self.root() else {drop(work);return None;};let state=root.state.lock().unwrap();
        if !Self::live(&state) {drop(state);drop(work);None}else {Some(work())}
    }
}
fn receive(handle:&RetirementHandle,receiver:&mpsc::Receiver<Message>,queue:Option<&wgpu::Queue>)->Vec<mpsc::Sender<()>> {
    let mut waiters=Vec::new();
    while let Ok(message)=receiver.try_recv() {
        let action=match message {
            Message::Dispose(action)=>{action();continue;},
            Message::AfterGpu(action)=>action,
            Message::Flush(waiter)=>{
                if queue.is_none() {waiters.push(waiter);continue;}
                Box::new(move||{let _=waiter.send(());}) as Action
            },
        };
        if let Some(queue)=queue {
            let destination=handle.clone();
            let ticket=handle.callback(Some(move|()|destination.run(action)));
            queue.on_submitted_work_done(move||ticket(()));
        }else {handle.run(action);}
    }
    handle.reap();if handle.0.released.swap(false,SeqCst) {handle.release();}waiters
}

fn finish(handle:&RetirementHandle,receiver:&mpsc::Receiver<Message>) {
    let old=std::mem::replace(&mut *handle.0.sink.lock().unwrap(),Sink::Closed);
    let waiters=receive(handle,receiver,None);
    if let Sink::Closing {after_gpu,..}=old {for action in after_gpu {action();}}
    handle.abandon();
    for waiter in waiters {let _=waiter.send(());}
}
fn drain_idle(handle:&RetirementHandle,receiver:&mpsc::Receiver<Message>) {finish(handle,receiver);}
fn run(poll:&mut impl FnMut()->Result<bool,String>,handle:&RetirementHandle,receiver:&mpsc::Receiver<Message>,queue:&OnceLock<wgpu::Queue>,failure:&mut Option<Box<dyn std::any::Any+Send>>) {
    let mut closing=None;
    loop {
        let step=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||->Result<(bool,bool),String> {
            if handle.closed() && closing.is_none() {
                let (send,ready)=mpsc::channel();closing=Some(ready);
                if let Some(queue)=queue.get() {
                    let ticket=handle.callback(Some(move|()|{let _=send.send(());}));
                    queue.on_submitted_work_done(move||ticket(()));
                }else {let _=send.send(());}
            }
            let waiters=receive(handle,receiver,queue.get());
            let active=poll()?;
            for waiter in waiters {let _=waiter.send(());}
            let drained=closing.as_ref().is_some_and(|ready|!matches!(ready.try_recv(),Err(mpsc::TryRecvError::Empty)));
            Ok((active,drained))
        }));
        let active=match step {
            Ok(Ok((_,true)))|Ok(Err(_))=>break,
            Ok(Ok((active,false)))=>active,
            Err(panic)=>{failure.get_or_insert(panic);handle.close();continue;},
        };
        if active || handle.closed() {std::thread::park_timeout(std::time::Duration::from_millis(1));}else {std::thread::park();}
    }
}

#[cfg(test)]
pub(crate) fn windowless()->(RetirementHandle,impl Drop) {let owner=tests::owner(||Ok(false));(owner.handle.clone(),owner)}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    pub(super) fn owner(poll:impl FnMut()->Result<bool,String>+Send+'static)->Owner {
        let (handle,receiver)=RetirementHandle::channel();Owner::start(poll,handle,receiver,Default::default()).unwrap()
    }
    #[test]
    fn retirement_releases_its_charge_and_waiter_while_later_gpu_submissions_continue() {
        use std::sync::atomic::{AtomicBool,AtomicUsize,Ordering};
        use std::time::Instant;
        use crate::residency::{Class,Tier};
        struct Charge {buffer:wgpu::Buffer,_charge:crate::residency::Charge,disposed:mpsc::Sender<Instant>}
        impl Drop for Charge {fn drop(&mut self) {self.buffer.destroy();let _=self.disposed.send(Instant::now());}}
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        assert_ne!(r.adapter.get_info().device_type,wgpu::DeviceType::Cpu);
        let (unused,_duration,raw)=crate::test_support::bounded_outstanding_gpu_work(&r);
        drop(unused);
        let handle=r.device.retirement_handle();handle.flush().unwrap();
        let tier=Arc::new(Tier::default());tier.set_limit(4,0);
        let buffer=r.device.create_buffer(&wgpu::BufferDescriptor {label:None,size:4,
            usage:wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
        let mut first=r.device.create_command_encoder(&Default::default());first.clear_buffer(&buffer,0,None);
        r.device.submit(&r.queue,[first.finish()]).unwrap();
        let (disposed,disposal)=mpsc::channel();
        let mut held=Some(handle.retirement(Charge {buffer,_charge:tier.charge(4),disposed}));
        let outstanding=Arc::new(AtomicUsize::new(0));
        let sustaining=Arc::new(AtomicBool::new(false));
        let idle=Arc::new(AtomicBool::new(false));
        let (completed,completion)=mpsc::channel();
        let submit=|| {
            outstanding.fetch_add(1,Ordering::AcqRel);
            r.device.submit(&r.queue,[raw(5)]).unwrap();
            let (outstanding,sustaining,idle,completed)=(outstanding.clone(),sustaining.clone(),idle.clone(),completed.clone());
            r.queue.on_submitted_work_done(move|| {
                if outstanding.fetch_sub(1,Ordering::AcqRel)==1 && sustaining.load(Ordering::Acquire) {idle.store(true,Ordering::Release);}
                let _=completed.send(());
            });
            r.device.request_poll();
        };
        for _ in 0..3 {submit();}
        sustaining.store(true,Ordering::Release);
        let start=Instant::now();let end=start+Duration::from_secs(2);
        let (woken,wake)=mpsc::channel();
        let mut dropped=None;let mut sent=3;let mut producer_error=None;
        while Instant::now()<end && sent<512 {
            if dropped.is_none() && start.elapsed()>=Duration::from_millis(100) {
                let check=tier.clone();let woken=woken.clone();
                tier.admit(Class::Visible,1).err().unwrap().wake_on_release(&handle,Arc::new(move|| {
                    if check.used()==0 {let _=woken.send(Instant::now());}
                }));
                dropped=Some(Instant::now());drop(held.take());
            }
            if let Err(error)=completion.recv_timeout(Duration::from_secs(1)) {producer_error=Some(error);break;}
            submit();sent+=1;
        }
        let loop_end=Instant::now();sustaining.store(false,Ordering::Release);
        drop(held);
        r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        handle.flush().unwrap();
        let released=disposal.recv_timeout(Duration::from_secs(1));
        let notified=wake.try_recv();
        let live_queue=!idle.load(Ordering::Acquire);
        assert!(producer_error.is_none() && sent<512 && loop_end>=end && live_queue,"sustained nonempty GPU queue prerequisite failed");
        let dropped=dropped.expect("retirement must drop during the sustained workload");
        let released=released.unwrap();
        assert!(released<loop_end && released.duration_since(dropped)<=Duration::from_millis(50),"retirement waited for whole-queue idle: {:?}",released.duration_since(dropped));
        let notified=notified.expect("retirement release must wake its capacity waiter");
        assert!(notified<loop_end && notified.duration_since(dropped)<=Duration::from_millis(50),"capacity waiter did not wake within50ms");
        assert_eq!(tier.used(),0);
    }
    #[test]
    fn close_joins_its_inflight_fence_while_a_raw_straggler_keeps_submitting() {
        use std::sync::atomic::{AtomicBool,AtomicUsize,Ordering};
        use std::time::Instant;
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        assert_ne!(r.adapter.get_info().device_type,wgpu::DeviceType::Cpu);
        let (unused,_duration,raw)=crate::test_support::bounded_outstanding_gpu_work(&r);drop(unused);
        let handle=r.device.retirement_handle();handle.flush().unwrap();
        let outstanding=Arc::new(AtomicUsize::new(0));let sustaining=Arc::new(AtomicBool::new(false));
        let idle=Arc::new(AtomicBool::new(false));let (completed,completion)=mpsc::channel();
        let submit=|| {
            outstanding.fetch_add(1,Ordering::AcqRel);r.queue.submit([raw(12)]);
            let (outstanding,sustaining,idle,completed)=(outstanding.clone(),sustaining.clone(),idle.clone(),completed.clone());
            r.queue.on_submitted_work_done(move|| {
                if outstanding.fetch_sub(1,Ordering::AcqRel)==1 && sustaining.load(Ordering::Acquire) {idle.store(true,Ordering::Release);}
                let _=completed.send(());
            });r.device.request_poll();
        };
        for _ in 0..3 {submit();}sustaining.store(true,Ordering::Release);
        let start=Instant::now();let end=start+Duration::from_secs(2);
        let (reference,reference_done)=mpsc::channel();let (joined,join_done)=mpsc::channel();
        let mut closing=None;let mut joined_at=None;let mut sent=3;let mut producer_error=None;
        while Instant::now()<end && sent<1024 {
            if closing.is_none() && start.elapsed()>=Duration::from_millis(100) {
                let binding=r.device.poll_binding();let queue=r.queue.clone();let reference=reference.clone();let joined=joined.clone();
                closing=Some(std::thread::spawn(move|| {
                    queue.on_submitted_work_done(move|| {let _=reference.send(Instant::now());});
                    binding.retire_lost(||{});let _=joined.send(Instant::now());
                }));
            }
            if joined_at.is_none() {joined_at=join_done.try_recv().ok();}
            if joined_at.is_some() {r.device.poll(wgpu::PollType::Poll).unwrap();}
            match completion.recv_timeout(Duration::from_millis(1)) {
                Ok(())=>{submit();sent+=1;},
                Err(mpsc::RecvTimeoutError::Timeout)=>{},
                Err(error)=>{producer_error=Some(error);break;},
            }
        }
        let loop_end=Instant::now();sustaining.store(false,Ordering::Release);
        r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();handle.pulse();
        if let Some(closing)=closing {closing.join().unwrap();}
        if joined_at.is_none() {joined_at=join_done.try_recv().ok();}
        let reference=reference_done.recv_timeout(Duration::from_secs(1));
        let live_queue=!idle.load(Ordering::Acquire);
        assert!(producer_error.is_none() && sent<1024 && loop_end>=end && live_queue,"continuous raw straggler prerequisite failed");
        let joined=joined_at.expect("close must join after cleanup");let reference=reference.unwrap();
        assert!(joined<loop_end && joined<=reference+Duration::from_millis(50),"close waited for the raw straggler to stop: {:?} after inflight fence",joined.saturating_duration_since(reference));
    }
    #[test]
    fn close_does_not_join_before_a_previously_submitted_fifty_ms_job_completes() {
        use std::time::Instant;
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        let (work,duration,_raw)=crate::test_support::bounded_outstanding_gpu_work(&r);
        work.submit(&r.queue);
        let (send,completed)=mpsc::channel();r.queue.on_submitted_work_done(move|| {let _=send.send(Instant::now());});
        let handle=r.device.retirement_handle();let binding=r.device.poll_binding();
        let (send,receive)=mpsc::channel();
        let closing=std::thread::spawn(move|| {binding.retire_lost(||{});let _=send.send(Instant::now());});
        let joined=receive.recv_timeout(Duration::from_secs(2));
        let completion_before_join=completed.try_recv();
        r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();handle.pulse();closing.join().unwrap();let actual=duration();
        assert!((40. ..=75.).contains(&actual));
        assert!(completion_before_join.is_ok(),"close joined before the real prior submission completed");
        assert!(joined.unwrap()<=completion_before_join.unwrap()+Duration::from_millis(50));
    }
    #[test]
    fn closing_an_empty_bound_gpu_queue_joins_within_fifty_ms() {
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        r.device.submit(&r.queue,[]).unwrap();r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let handle=r.device.retirement_handle();handle.flush().unwrap();let binding=r.device.poll_binding();
        let (send,receive)=mpsc::channel();let start=std::time::Instant::now();
        let closing=std::thread::spawn(move|| {binding.retire_lost(||{});let _=send.send(std::time::Instant::now());});
        let completed=receive.recv_timeout(Duration::from_millis(50));
        r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();handle.pulse();closing.join().unwrap();
        let elapsed=completed.as_ref().ok().map(|t|t.duration_since(start));
        assert!(completed.is_ok(),"empty close parked without driving its fence");
        assert!(elapsed.unwrap()<=Duration::from_millis(50));
    }
    #[test]
    fn an_idle_unsubmitted_retirement_releases_without_another_wake() {
        struct Probe(mpsc::Sender<std::time::Instant>);
        impl Drop for Probe {fn drop(&mut self) {let _=self.0.send(std::time::Instant::now());}}
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        r.device.submit(&r.queue,[]).unwrap();r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let handle=r.device.retirement_handle();handle.flush().unwrap();
        let (send,receive)=mpsc::channel();let held=handle.retirement(Probe(send));
        let start=std::time::Instant::now();drop(held);let completed=receive.recv_timeout(Duration::from_millis(50));
        let elapsed=completed.as_ref().ok().map(|t|t.duration_since(start));
        r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();handle.flush().unwrap();
        assert!(completed.is_ok(),"idle after-GPU registration needs a subsequent maintain, not another external wake");
        assert!(elapsed.unwrap()<=Duration::from_millis(50));
    }
    fn assert_cleanup_panic_preserves_the_joining_driver(terminal_panic:bool) {
        struct Driver(mpsc::Sender<std::thread::ThreadId>);
        impl Drop for Driver {fn drop(&mut self) {let _=self.0.send(std::thread::current().id());}}
        struct CallbackDrop {panics:bool,marker:mpsc::Sender<()>}
        impl Drop for CallbackDrop {fn drop(&mut self) {
            if self.panics {let _=self.marker.send(());panic!("controlled abandoned callback Drop panic");}
        }}
        let (driver,driver_drop)=mpsc::channel();let driver=Driver(driver);
        let (entered,entry)=mpsc::channel();let (release,held)=mpsc::channel();let mut first=Some((entered,held));
        let mut owner=owner(move|| {
            let _=&driver;
            if let Some((entered,held))=first.take() {entered.send(()).unwrap();held.recv_timeout(Duration::from_secs(10)).unwrap();}
            Err("controlled cleanup exit".into())
        });
        entry.recv_timeout(Duration::from_secs(10)).unwrap();let handle=owner.handle.clone();
        let (marker,panicking)=mpsc::channel();let terminal_marker=marker.clone();
        handle.on_retired(Arc::new(move|| {
            if terminal_panic {let _=terminal_marker.send(());panic!("controlled terminal notification panic");}
        }));
        let probe=CallbackDrop {panics:!terminal_panic,marker};
        let callback=handle.callback(Some(move|():()|drop(probe)));
        release.send(()).unwrap();
        let panic_entered=panicking.recv_timeout(Duration::from_secs(1));
        let joined=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||owner.shutdown()));
        let driver_thread=driver_drop.recv_timeout(Duration::from_secs(1));
        let closed=handle.closed();
        let (late,late_waiter)=mpsc::channel();
        std::sync::Arc::new(crate::residency::Tier::default()).admit(crate::residency::Class::Visible,1).err().unwrap().wake_on_release(&handle,Arc::new(move|| {let _=late.send(());}));
        let late_waiter=late_waiter.recv_timeout(Duration::from_secs(1));
        let payload=joined.as_ref().err().and_then(|panic|panic.downcast_ref::<&str>().copied().or_else(||panic.downcast_ref::<String>().map(String::as_str)));
        let expected=if terminal_panic {"controlled terminal notification panic"}else {"controlled abandoned callback Drop panic"};
        drop(callback);drop(owner);
        assert!(panic_entered.is_ok(),"cleanup panic path must actually execute");
        assert!(joined.is_err(),"the callback panic must propagate on the joining owner");
        assert!(closed,"cleanup panic left the handle Open");
        assert!(late_waiter.is_ok(),"a terminal handle must wake a newly refused waiter immediately");
        assert_eq!((driver_thread.unwrap(),payload),(std::thread::current().id(),Some(expected)),"cleanup must return the driver guard and resume the first panic on its joining owner");
    }
    #[test]
    fn terminal_notification_panic_keeps_the_driver_on_the_joining_owner() {
        assert_cleanup_panic_preserves_the_joining_driver(true);
    }
    #[test]
    fn abandoned_callback_drop_panic_keeps_the_driver_on_the_joining_owner() {
        assert_cleanup_panic_preserves_the_joining_driver(false);
    }
    #[test]
    fn undrained_receiver_payload_panic_keeps_the_driver_on_the_joining_owner() {
        use std::sync::atomic::{AtomicBool,Ordering};
        struct Driver(mpsc::Sender<std::thread::ThreadId>);
        impl Drop for Driver {fn drop(&mut self) {let _=self.0.send(std::thread::current().id());}}
        struct Primary {seen:Arc<AtomicBool>,entered:mpsc::Sender<()>}
        impl Drop for Primary {fn drop(&mut self) {
            self.seen.store(true,Ordering::Release);let _=self.entered.send(());panic!("controlled primary finish panic");
        }}
        struct Undrained(Arc<AtomicBool>);
        impl Drop for Undrained {fn drop(&mut self) {self.0.store(true,Ordering::Release);panic!("controlled undrained receiver payload panic");}}
        let (driver,driver_drop)=mpsc::channel();let driver=Driver(driver);
        let (entered,entry)=mpsc::channel();let (release,held)=mpsc::channel();let mut first=Some((entered,held));
        let mut owner=owner(move|| {
            let _=&driver;
            if let Some((entered,held))=first.take() {entered.send(()).unwrap();held.recv_timeout(Duration::from_secs(10)).unwrap();}
            Err("controlled poll exit after receive".into())
        });
        entry.recv_timeout(Duration::from_secs(10)).unwrap();
        let primary_seen=Arc::new(AtomicBool::new(false));let (primary,primary_entered)=mpsc::channel();
        let primary=Primary {seen:primary_seen.clone(),entered:primary};
        owner.handle.run(move||drop(primary));
        let secondary_dropped=Arc::new(AtomicBool::new(false));let secondary_ran=Arc::new(AtomicBool::new(false));
        let undrained=Undrained(secondary_dropped.clone());let ran=secondary_ran.clone();
        owner.handle.run(move|| {ran.store(true,Ordering::Release);drop(undrained);});
        release.send(()).unwrap();
        let primary_entered=primary_entered.recv_timeout(Duration::from_secs(1));
        let joined=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||owner.shutdown()));
        let driver_thread=driver_drop.recv_timeout(Duration::from_secs(1));
        assert!(primary_entered.is_ok() && primary_seen.load(Ordering::Acquire),"the primary finish panic must execute");
        assert!(secondary_dropped.load(Ordering::Acquire),"the residual receiver must own its panicking payload");
        assert!(!secondary_ran.load(Ordering::Acquire),"cleanup must not continue draining after the primary panic");
        assert!(joined.is_err(),"joining must propagate a panic");
        assert_eq!(driver_thread.unwrap(),std::thread::current().id(),"the residual receiver dropped the returned driver guard on its worker");
    }
    #[test]
    fn completion_barrier_disposes_prior_retirements_under_seeded_busy_submissions() {
        use std::sync::atomic::{AtomicBool,AtomicU64,Ordering};
        struct Charge {buffer:wgpu::Buffer,bytes:Arc<AtomicU64>}
        impl Drop for Charge {fn drop(&mut self) {self.buffer.destroy();self.bytes.fetch_sub(4,Ordering::AcqRel);}}
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        assert_ne!(r.adapter.get_info().device_type,wgpu::DeviceType::Cpu);
        let (unused,_duration,raw)=crate::test_support::bounded_outstanding_gpu_work(&r);drop(unused);
        let handle=r.device.retirement_handle();crate::test_support::complete(&r);
        let bytes=Arc::new(AtomicU64::new(0));let mut seed=0x139a84e7d6e135eeu64;
        for iteration in 0..1000 {
            seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;
            let count=1+(seed%16) as usize;
            let values=(0..count).map(|_| {
                let buffer=r.device.create_buffer(&wgpu::BufferDescriptor {label:None,size:4,
                    usage:wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
                bytes.fetch_add(4,Ordering::AcqRel);handle.retirement(Charge {buffer,bytes:bytes.clone()})
            }).collect::<Vec<_>>();
            r.device.submit(&r.queue,[raw(20)]).unwrap();
            let finished=Arc::new(AtomicBool::new(false));let fence=finished.clone();
            r.queue.on_submitted_work_done(move||fence.store(true,Ordering::Release));
            for _ in 0..((seed>>8)&7) {std::thread::yield_now();}
            drop(values);
            for _ in 0..((seed>>16)&3) {std::thread::yield_now();}
            let outstanding=!finished.load(Ordering::Acquire);
            crate::test_support::complete(&r);
            let observed=bytes.load(Ordering::Acquire);
            if observed!=0 {
                r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();handle.flush().unwrap();
            }
            assert_eq!(observed,0,"completion returned before prior retirements were disposed: iteration={iteration} seed={seed:#x} count={count} outstanding_before_complete={outstanding}");
        }
    }
    #[test]
    fn flush_waits_for_prior_retirements_and_the_inflight_gpu_prefix() {
        use std::sync::atomic::{AtomicBool,AtomicU64,Ordering};
        struct Charge {buffer:wgpu::Buffer,bytes:Arc<AtomicU64>}
        impl Drop for Charge {fn drop(&mut self) {self.buffer.destroy();self.bytes.fetch_sub(4,Ordering::AcqRel);}}
        let r=crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        assert_ne!(r.adapter.get_info().device_type,wgpu::DeviceType::Cpu);
        let (mut busy,duration,_raw)=crate::test_support::bounded_outstanding_gpu_work(&r);
        let handle=r.device.retirement_handle();
        let bytes=Arc::new(AtomicU64::new(0));let mut values=Vec::new();
        for _ in 0..8 {
            let buffer=r.device.create_buffer(&wgpu::BufferDescriptor {label:None,size:4,
                usage:wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
            busy.clear_buffer(&buffer,0,None);bytes.fetch_add(4,Ordering::AcqRel);
            values.push(handle.retirement(Charge {buffer,bytes:bytes.clone()}));
        }
        let fence=Arc::new(AtomicBool::new(false));let completed=fence.clone();
        busy.submit(&r.queue);r.queue.on_submitted_work_done(move||completed.store(true,Ordering::Release));
        drop(values);
        let flush=handle.flush();
        let observed=bytes.load(Ordering::Acquire);let completed=fence.load(Ordering::Acquire);
        let actual=duration();r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();handle.flush().unwrap();
        let cleaned=bytes.load(Ordering::Acquire);
        flush.unwrap();
        assert!((40. ..=75.).contains(&actual),"bounded GPU prerequisite: {actual} ms");
        assert_eq!(cleaned,0,"actual completion must release the buffer charges");
        assert_eq!((observed,completed),(0,true),"Flush must include every prior retirement and its in-flight GPU prefix");
    }
    #[test]
    fn shutdown_returns_with_live_resource_consumers_and_disposes_stragglers_inline() {
        struct Probe(mpsc::Sender<std::thread::ThreadId>);
        impl Drop for Probe {fn drop(&mut self) {self.0.send(std::thread::current().id()).unwrap();}}
        let mut owner=owner(||Ok(false));let handle=owner.handle.clone();
        let (send,receive)=mpsc::channel();let held=handle.retirement(Probe(send));
        let (done,finished)=mpsc::channel();let worker=std::thread::spawn(move|| {owner.shutdown();done.send(()).unwrap();});
        let completed=finished.recv_timeout(Duration::from_secs(1));
        drop(held);worker.join().unwrap();
        assert!(completed.is_ok(),"shutdown waited for consumers: {completed:?}");
        assert_eq!(receive.recv_timeout(Duration::from_secs(10)).unwrap(),std::thread::current().id());
    }
    #[test]
    fn callbacks_abandon_before_their_external_producer_drops() {
        let mut owner=owner(||Ok(false));let (send,receive)=mpsc::channel::<u32>();
        let callback=owner.handle.callback(Some(move|value|send.send(value).unwrap()));
        owner.shutdown();
        assert!(matches!(receive.try_recv(),Err(mpsc::TryRecvError::Disconnected)));
        callback(9);
    }
    #[test]
    fn callback_results_are_recorded_before_the_waiter_arrives() {
        let mut owner=owner(||Ok(false));let (send,receive)=mpsc::channel();
        owner.handle.callback(Some(move|value|send.send(value).unwrap()))(7);
        owner.handle.flush().unwrap();assert_eq!(receive.recv_timeout(Duration::from_secs(10)).unwrap(),7);
        owner.shutdown();
    }
    #[test]
    fn close_drains_every_accepted_disposal() {
        for _ in 0..64 {
            let (entered,ready)=mpsc::channel();let (release,held)=mpsc::channel();let mut first=Some((entered,held));
            let mut owner=owner(move|| {if let Some((entered,held))=first.take() {entered.send(()).unwrap();held.recv_timeout(Duration::from_secs(10)).unwrap();}Ok(false)});
            ready.recv_timeout(Duration::from_secs(10)).unwrap();let handle=owner.handle.clone();
            let (send,receive)=mpsc::channel();handle.send(Message::Dispose(Box::new(move||send.send(3).unwrap()))).unwrap_or_else(|_|panic!("open mailbox"));
            handle.close();release.send(()).unwrap();owner.shutdown();
            assert_eq!(receive.recv_timeout(Duration::from_secs(10)).unwrap(),3);
        }
    }
    #[test]
    fn polling_errors_close_once_and_release_the_driver_on_the_joining_thread() {
        struct Driver(mpsc::Sender<std::thread::ThreadId>);
        impl Drop for Driver {fn drop(&mut self) {self.0.send(std::thread::current().id()).unwrap();}}
        let (send,receive)=mpsc::channel();let driver=Driver(send);
        let mut owner=owner(move|| {let _=&driver;Err("lost".into())});
        let (send,woken)=mpsc::channel();owner.handle.on_retired(Arc::new(move||send.send(()).unwrap()));
        woken.recv_timeout(Duration::from_secs(10)).unwrap();owner.shutdown();
        assert_eq!(receive.recv_timeout(Duration::from_secs(10)).unwrap(),std::thread::current().id());
        assert!(woken.try_recv().is_err());
    }

    fn assert_poll_exit_abandons_callbacks(mut terminal:impl FnMut()->Result<bool,String>+Send+'static)->bool {
        let (entered,ready)=mpsc::channel();let (release,held)=mpsc::channel();let mut first=true;
        let mut owner=owner(move|| {
            if first {first=false;entered.send(()).unwrap();held.recv_timeout(Duration::from_secs(10)).unwrap();}
            terminal()
        });
        ready.recv_timeout(Duration::from_secs(10)).unwrap();
        let (closed,woken)=mpsc::channel();owner.handle.on_retired(Arc::new(move||{let _=closed.send(());}));
        let (send,receive)=mpsc::channel();
        let callback=owner.handle.callback(Some(move|value:u32|{let _=send.send(value);}));
        release.send(()).unwrap();
        let terminal=woken.recv_timeout(Duration::from_secs(1));
        let abandoned=receive.recv_timeout(Duration::from_secs(1));
        let joined=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||owner.shutdown()));
        drop(callback);
        assert!(terminal.is_ok(),"poll exit did not publish terminal state before join: {terminal:?}");
        assert!(matches!(abandoned,Err(mpsc::RecvTimeoutError::Disconnected)),
            "poll exit retained a callback's sender until join: {abandoned:?}");
        joined.is_err()
    }
    #[test]
    fn polling_errors_abandon_external_callbacks_before_join() {
        assert!(!assert_poll_exit_abandons_callbacks(||Err("controlled poll error".into())));
    }
    #[derive(Default)]
    struct CallbackCounts {
        runs:std::sync::atomic::AtomicUsize,
        unrun:std::sync::atomic::AtomicUsize,
    }
    struct UnrunCallback(Option<Arc<CallbackCounts>>);
    impl Drop for UnrunCallback {
        fn drop(&mut self) {if let Some(counts)=self.0.take() {counts.unrun.fetch_add(1,std::sync::atomic::Ordering::SeqCst);}}
    }
    fn counted_callback(counts:Arc<CallbackCounts>)->impl FnOnce(())+Send+'static {
        let mut value=UnrunCallback(Some(counts));
        move|()| {value.0.take().unwrap().runs.fetch_add(1,std::sync::atomic::Ordering::SeqCst);}
    }
    #[test]
    fn callback_registration_and_abandonment_have_terminal_linearizations() {
        use std::sync::atomic::Ordering;
        let mut outcomes=Vec::new();
        for order in 0..3 {
            let mut owner=owner(||Ok(false));let handle=owner.handle.clone();
            let counts=Arc::new(CallbackCounts::default());
            if order==0 {owner.shutdown();}
            let callback=handle.callback(Some(counted_callback(counts.clone())));
            let before_return=counts.unrun.load(Ordering::SeqCst);
            if order==2 {callback(());owner.shutdown();}else {owner.shutdown();callback(());}
            outcomes.push((order,before_return,counts.runs.load(Ordering::SeqCst),counts.unrun.load(Ordering::SeqCst)));
        }
        assert_eq!(outcomes,[(0,1,0,1),(1,0,0,1),(2,0,1,0)]);
    }
    #[test]
    fn abandoned_callbacks_can_register_nested_retirements_without_locking_the_registry() {
        struct Nested {handle:RetirementHandle,depth:usize,done:mpsc::Sender<()>}
        impl Drop for Nested {
            fn drop(&mut self) {
                if self.depth==0 {let _=self.done.send(());return;}
                let nested=Nested {handle:self.handle.clone(),depth:self.depth-1,done:self.done.clone()};
                let callback=self.handle.callback(Some(move|()|drop(nested)));
                drop(callback);
            }
        }
        let (done,finished)=mpsc::channel();
        let worker=std::thread::spawn(move|| {
            let mut owner=owner(||Ok(false));let handle=owner.handle.clone();
            let retired=handle.retirement(Nested {handle:handle.clone(),depth:2,done});
            let callback=handle.callback(Some(move|()|drop(retired)));
            owner.shutdown();drop(callback);
        });
        let completed=finished.recv_timeout(Duration::from_secs(1));
        if completed.is_ok() {worker.join().unwrap();}
        assert!(completed.is_ok(),"nested terminal registration deadlocked: {completed:?}");
    }
    #[test]
    fn seeded_registration_abandonment_races_complete_each_callback_once() {
        use std::sync::{Barrier,atomic::Ordering};
        let mut seed=0xd34db33f5eed1234u64;
        let mut held=Vec::<Box<dyn FnOnce(())+Send>>::new();let mut handles=Vec::new();let mut outcomes=Vec::new();
        for index in 0..1000 {
            seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let (handle,_receiver)=RetirementHandle::channel();let producer=handle.clone();let closer=handle.clone();
            let counts=Arc::new(CallbackCounts::default());let observed=counts.clone();
            let barrier=Arc::new(Barrier::new(2));let start=barrier.clone();
            let begin=std::time::Instant::now();
            let register=std::thread::spawn(move|| {
                start.wait();for _ in 0..(seed&7) {std::thread::yield_now();}
                let callback=producer.callback(Some(counted_callback(counts)));
                if seed&8!=0 {callback(());None}else {Some(Box::new(callback) as Box<dyn FnOnce(())+Send>)}
            });
            let abandon=std::thread::spawn(move|| {
                barrier.wait();for _ in 0..(seed.rotate_left(13)&7) {std::thread::yield_now();}
                closer.close();closer.abandon();
            });
            if let Some(callback)=register.join().unwrap() {held.push(callback);}
            abandon.join().unwrap();
            outcomes.push((index,observed.runs.load(Ordering::SeqCst),observed.unrun.load(Ordering::SeqCst),begin.elapsed()));
            handles.push(handle);
        }
        let invalid=outcomes.iter().filter(|(_,runs,unrun,elapsed)|runs+unrun!=1 || *elapsed>Duration::from_secs(1)).collect::<Vec<_>>();
        let missing=invalid.iter().map(|row|row.0).collect::<Vec<_>>();
        drop(held);for handle in handles {handle.abandon();}
        assert!(invalid.is_empty(),"registration/abandon race left callbacks unresolved or late: count={}, indices={missing:?}",invalid.len());
    }
    #[test]
    fn a_disposal_that_drops_no_charge_does_not_wake_a_capacity_waiter() {
        use crate::residency::Tier;
        let mut owner=owner(||Ok(false));let handle=owner.handle.clone();
        let tier=Arc::new(Tier::default());let (notify,woken)=mpsc::channel();
        tier.admit(crate::residency::Class::Visible,1).err().unwrap().wake_on_release(&handle,Arc::new(move||{let _=notify.send(());}));
        handle.run(||{});drop(handle.retirement(()));handle.dispose_when((),|_|true);handle.flush().unwrap();
        let spurious=woken.try_recv();
        tier.set_limit(1,0);let raised=woken.recv_timeout(Duration::from_secs(1));owner.shutdown();
        assert!(spurious.is_err(),"a disposal that frees no capacity woke a capacity waiter");
        assert!(raised.is_ok(),"a capacity increase must wake the waiter");
    }
    #[test]
    fn a_retirement_drop_wakes_its_capacity_waiter_without_a_bound_queue() {
        use std::sync::atomic::{AtomicBool,Ordering};
        use crate::residency::{Class,Tier};
        let active=Arc::new(AtomicBool::new(true));let polling=active.clone();
        let mut owner=owner(move||Ok(polling.load(Ordering::Acquire)));let handle=owner.handle.clone();
        let tier=Arc::new(Tier::default());tier.set_limit(1,0);let held=tier.charge(1);let (notify,woken)=mpsc::channel();
        tier.admit(Class::Visible,1).err().unwrap().wake_on_release(&handle,Arc::new(move||{let _=notify.send(());}));
        drop(handle.retirement(held));
        let released=woken.recv_timeout(Duration::from_secs(1));let freed=tier.used()==0;
        active.store(false,Ordering::Release);handle.wake();owner.shutdown();
        assert!(released.is_ok(),"Retirement freed capacity without waking its waiter: freed_before_cleanup={freed}, wake={released:?}");
        assert!(freed);
    }
    #[test]
    fn an_inline_callback_panic_closes_and_abandons_before_join_but_retains_the_driver() {
        struct Driver(mpsc::Sender<std::thread::ThreadId>);
        impl Drop for Driver {fn drop(&mut self) {let _=self.0.send(std::thread::current().id());}}
        let (handle,receiver)=RetirementHandle::channel();
        let (driver_send,driver_drop)=mpsc::channel();let driver=Driver(driver_send);
        let (closed,woken)=mpsc::channel();handle.on_retired(Arc::new(move||{let _=closed.send(());}));
        let (sender,pending)=mpsc::channel();let held=handle.callback(Some(move|value:u32|{let _=sender.send(value);}));
        let (fired,entered)=mpsc::channel();let mut callback=Some(handle.callback(Some(move|()|{let _=fired.send(());panic!("controlled inline callback panic");})));
        let mut owner=Owner::start(move|| {
            let _=&driver;if let Some(callback)=callback.take() {callback(());}Ok(false)
        },handle.clone(),receiver,Default::default()).unwrap();
        let entered=entered.recv_timeout(Duration::from_secs(1));
        let terminal=woken.recv_timeout(Duration::from_secs(1));
        let abandoned=pending.recv_timeout(Duration::from_secs(1));
        let closed=handle.closed();let driver_retained=matches!(driver_drop.try_recv(),Err(mpsc::TryRecvError::Empty));
        let _=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||owner.shutdown()));drop(held);
        let dropped=driver_drop.recv_timeout(Duration::from_secs(1));
        assert!(entered.is_ok());assert!(driver_retained,"raw poll driver dropped before its joining owner");
        assert_eq!(dropped.unwrap(),std::thread::current().id());
        assert!(closed && terminal.is_ok(),"inline callback panic left the handle live before join: closed={closed}, terminal={terminal:?}");
        assert!(matches!(abandoned,Err(mpsc::RecvTimeoutError::Disconnected)),"inline panic retained an unfired callback until join: {abandoned:?}");
    }
}
