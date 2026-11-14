use super::*;

const CLASSES:[Class;5]=[Class::Required,Class::Validation,Class::Visible,Class::Anticipated,Class::Background];

#[test]
fn every_compile_class_keeps_completion_but_only_visible_demand_wakes() {
    let mut missing=Vec::new();
    for class in CLASSES {for failed in [false,true] {
        let mut queue=Queue::<()>::new(1);
        let owner=queue.owner();
        let called=Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count=called.clone();
        queue.owners.get_mut(&owner).unwrap().wake=Some(Arc::new(move||{count.fetch_add(1,std::sync::atomic::Ordering::Relaxed);}));
        queue.submit(Key::Job(1),Demand {owner,generation:None,class},false,||());
        let job=queue.take(Instant::now(),true).unwrap();
        let wakes=queue.finish(job.key,job.class,job.independent,if failed {Err("compile failure".into())}else {Ok(())});
        assert!(!queue.entries.contains_key(&Key::Job(1)));
        assert_eq!(queue.check(owner).is_err(),failed);
        for (_,wake) in wakes {wake();}
        let actual=called.load(std::sync::atomic::Ordering::Relaxed);
        let expected=usize::from(class<=Class::Visible);
        if actual!=expected {missing.push((class,failed,actual,expected));}
    }}
    assert!(missing.is_empty(),"compile completion wake classes differ: {missing:?}");
}

#[test]
fn stale_generations_and_retired_compile_owners_receive_no_completion() {
    for class in CLASSES {for retire in [false,true] {
        let mut queue=Queue::<()>::new(1);let owner=queue.owner();
        queue.owners.get_mut(&owner).unwrap().wake=Some(Arc::new(||panic!("a retired demand woke")));
        queue.submit(Key::Job(1),Demand {owner,generation:Some(0),class},false,||());
        let job=queue.take(Instant::now(),true).unwrap();
        if retire {queue.owners.remove(&owner);}else {queue.owners.get_mut(&owner).unwrap().generation+=1;}
        assert!(queue.finish(job.key,job.class,job.independent,Err("stale failure".into())).is_empty());
        assert!(queue.check(owner).is_ok());
        assert!(!queue.entries.contains_key(&Key::Job(1)));
    }}
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn compile_completion_before_wake_registration_remains_observable() {
    for failed in [false,true] {
        let scope=super::tests::scope(1);
        let (notify,completed)=std::sync::mpsc::channel();
        scope.set_wake(Arc::new(move||{let _=notify.send(());}));
        while completed.try_recv().is_ok() {}
        let id=scope.enqueue(Class::Required,move||if failed {Err("durable error".into())}else {Ok(())});
        completed.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(scope.job_ready(id));assert_eq!(scope.check().is_err(),failed);
        let (notify,late)=std::sync::mpsc::channel();
        scope.set_wake(Arc::new(move||{let _=notify.send(());}));
        late.recv_timeout(Duration::from_secs(2)).expect("late registration must observe durable completion");
        assert!(scope.job_ready(id));assert_eq!(scope.check().is_err(),failed);
    }
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn queued_recipe_completed_by_another_executor_wakes_its_owner() {
    let scope=super::tests::scope(1);
    let (notify,completed)=std::sync::mpsc::channel();
    scope.set_wake(Arc::new(move||{let _=notify.send(());}));
    while completed.try_recv().is_ok() {}
    let batch=scope.batch();
    let recipe=Deferred::new(||31u32);
    scope.warm(&recipe,Class::Visible,scope.generation());
    pollster::block_on(scope.prepare_all([&recipe])).unwrap();
    assert!(recipe.ready());
    drop(batch);
    completed.recv_timeout(Duration::from_secs(2)).expect("the completed queued recipe must notify its owner");
    assert_eq!(*recipe.compile(),31);
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn a_late_failed_recipe_request_wakes_with_its_durable_error() {
    let owner=super::tests::scope(1);
    let (notify,completed)=std::sync::mpsc::channel();
    owner.set_wake(Arc::new(move||{let _=notify.send(());}));
    while completed.try_recv().is_ok() {}
    let recipe=Deferred::<u32>::new(||panic!("durable recipe failure"));
    assert!(!owner.require([&recipe],Class::Required));
    completed.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(recipe.error().unwrap().contains("durable recipe failure"));
    let late=owner.0.coordinator.scope().unwrap();
    let (notify,observed)=std::sync::mpsc::channel();
    late.set_wake(Arc::new(move||{let _=notify.send(());}));
    while observed.try_recv().is_ok() {}
    assert!(!late.require([&recipe],Class::Required));
    observed.recv_timeout(Duration::from_secs(2)).expect("requesting a completed failure must wake the late owner");
    assert!(late.check().unwrap_err().to_string().contains("durable recipe failure"));
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn stale_compile_generations_ignore_already_failed_recipes() {
    let owner=super::tests::scope(1);
    let (notify,completed)=std::sync::mpsc::channel();
    owner.set_wake(Arc::new(move||{let _=notify.send(());}));
    while completed.try_recv().is_ok() {}
    let recipe=Deferred::<u32>::new(||panic!("obsolete compile failure"));
    owner.require([&recipe],Class::Required);
    completed.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(recipe.error().is_some());
    let late=owner.0.coordinator.scope().unwrap();
    let (notify,observed)=std::sync::mpsc::channel();
    late.set_wake(Arc::new(move||{let _=notify.send(());}));
    while observed.try_recv().is_ok() {}
    let obsolete=late.generation();late.advance();
    late.warm(&recipe,Class::Background,obsolete);
    assert!(late.check().is_ok(),"a stale compile demand must not fail the current owner");
    assert!(observed.try_recv().is_err(),"a stale compile demand must not wake the current owner");
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn a_queued_recipe_keeps_its_external_compile_failure() {
    let scope=super::tests::scope(1);
    let (notify,completed)=std::sync::mpsc::channel();
    scope.set_wake(Arc::new(move||{let _=notify.send(());}));
    while completed.try_recv().is_ok() {}
    let batch=scope.batch();
    let recipe=Deferred::<u32>::new(||panic!("original external compile failure"));
    scope.warm(&recipe,Class::Visible,scope.generation());
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||scope.blocking(||recipe.compile()))).is_err());
    assert!(recipe.error().unwrap().contains("original external compile failure"));
    drop(batch);
    completed.recv_timeout(Duration::from_secs(2)).expect("a failed queued recipe must notify its owner");
    let error=scope.check().unwrap_err().to_string();
    assert!(error.contains("original external compile failure"),"the owner lost the durable recipe failure: {error}");
}

#[cfg(not(target_arch="wasm32"))]
#[test]
fn optional_known_compile_errors_remain_durable_without_waking_the_host() {
    let owner=super::tests::scope(1);
    let (notify,completed)=std::sync::mpsc::channel();
    owner.set_wake(Arc::new(move||{let _=notify.send(());}));
    while completed.try_recv().is_ok() {}
    let recipe=Deferred::<u32>::new(||panic!("optional durable failure"));
    owner.require([&recipe],Class::Required);
    completed.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(recipe.error().unwrap().contains("optional durable failure"));
    for class in [Class::Anticipated,Class::Background] {
        let optional=owner.0.coordinator.scope().unwrap();
        let (notify,observed)=std::sync::mpsc::channel();
        optional.set_wake(Arc::new(move||{let _=notify.send(());}));
        while observed.try_recv().is_ok() {}
        optional.warm(&recipe,class,optional.generation());
        assert!(optional.check().unwrap_err().to_string().contains("optional durable failure"));
        assert!(observed.try_recv().is_err(),"{class:?} warming dirtied a host without required work");
    }
}
