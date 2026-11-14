use super::*;
use layer_core::{Document, authored::{Artwork, PaintBase, SourceTarget}};
use crate::test_support::{GatedStore,ReadGate};
use std::sync::Mutex;
use std::time::{Duration,Instant};

const DEADLINE:Duration=Duration::from_secs(10);

fn parked(fail:bool)->(Arc<SourceImage>,Arc<ReadGate>,GatedStore) {
    let source=layer_core::color::source::rgba8_source([PAGE_SIZE;2],|_,_|[255,128,0,255]);
    let mut artwork=Artwork::new([PAGE_SIZE;2]).unwrap();
    let (_,SourceTarget::Paint(target))=crate::test_support::add_paint(&mut artwork,"Gated source",[PAGE_SIZE;2]) else {unreachable!()};
    artwork.paint.get_mut(target).unwrap().base=Some(PaintBase::new(source.clone().into()));
    let retained=layer_core::Editor::new(Document::from_artwork(artwork).unwrap()).retained_tiles();
    let store=GatedStore::new(&retained,fail);let gate=store.gate.clone();
    (source,gate,store)
}
fn queue(pool:&Arc<DecodePool>,destination:RgbSpace)->MipDecodeQueue {
    let mut queue=MipDecodeQueue::native(Arc::new(layer_core::raster::DecodedTileCache::new(0)),destination).unwrap();
    queue.pool=pool.clone();queue
}
fn result(queue:&mut MipDecodeQueue)->PreparedImageWork {
    let deadline=Instant::now()+DEADLINE;
    loop {
        if let Some(result)=queue.poll() {return result;}
        assert!(Instant::now()<deadline,"accepted decode did not complete");std::thread::sleep(Duration::from_millis(1));
    }
}
fn image_pixels(result:PreparedImageWork)->(PortableId,Arc<SourceImage>,crate::scene::sources::PreparedSourcePixels) {
    let PreparedImageWork::Image(tile)=result else {panic!("image request returned a different result kind")};
    (tile.id,tile.source,tile.pixels.unwrap())
}
fn samples(pixels:&crate::scene::sources::PreparedSourcePixels)->&[u8] {
    match pixels {crate::scene::sources::PreparedSourcePixels::NativeSamples(bytes)
        |crate::scene::sources::PreparedSourcePixels::PremultipliedWorkingPixels(bytes)=>bytes.as_slice()}
}

#[test]
fn queue_churn_keeps_one_shared_pool_and_does_not_retain_idle_contexts() {
    let cache=Arc::new(layer_core::raster::DecodedTileCache::new(0));
    let first=MipDecodeQueue::native(cache.clone(),RgbSpace::Srgb).unwrap();let pool=first.pool.clone();
    assert!((1..=3).contains(&(pool.capacity/6)));
    let epoch=pool.epoch.load(Ordering::Acquire);
    for index in 0..128 {
        let queue=MipDecodeQueue::native(cache.clone(),if index%2==0 {RgbSpace::Srgb}else {RgbSpace::DisplayP3}).unwrap();
        assert!(Arc::ptr_eq(&queue.pool,&pool));assert_eq!(queue.capacity,pool.capacity);
        assert!(queue.capacity<=18);assert!(!queue.pending());assert!(!queue.ready());
        let context=Arc::downgrade(&queue.context);drop(queue);assert!(context.upgrade().is_none());
    }
    assert_eq!(pool.epoch.load(Ordering::Acquire),epoch);assert_eq!(Arc::strong_count(&cache),2);
}

#[test]
fn full_pool_retries_on_its_capacity_epoch_and_drops_cancelled_queued_and_active_work() {
    let pool=DecodePool::start(1).unwrap();
    let (a,ga,_release_a)=parked(true);let (b,gb,_release_b)=parked(true);let (c,gc,_release_c)=parked(false);
    let d=layer_core::color::source::rgba8_source([1;2],|_,_|[9,8,7,255]);
    let mut active=queue(&pool,RgbSpace::Srgb);let mut cancelled=queue(&pool,RgbSpace::Srgb);let mut waiting=queue(&pool,RgbSpace::Srgb);
    let cancelled_context=Arc::downgrade(&cancelled.context);let cancelled_cache=Arc::downgrade(&cancelled.context.cache);let cancelled_ready=cancelled.ready.clone();
    let (entered,seen)=std::sync::mpsc::channel();let (release,held)=std::sync::mpsc::channel();let held=Mutex::new(held);
    active.set_wake(Arc::new(move || {let _=entered.send(());let _=held.lock().unwrap().recv();}));
    assert!(active.request(PortableId::random(),a.clone(),[0;2]));ga.entered();ga.release();
    seen.recv_timeout(DEADLINE).expect("the completed decode holds the worker at its notification");
    assert!(cancelled.request(PortableId::random(),b.clone(),[0;2]));gb.entered();
    let dead_source=Arc::downgrade(&c);assert!(cancelled.request(PortableId::random(),c,[0;2]));gc.entered();
    let coordinates=Arc::new(Mutex::new(crate::object_image_mips::CoordinateSlot::new(&Default::default())));
    for first in 0..pool.capacity-2 {assert!(cancelled.request_coordinates([1.,0.,0.,1.,0.,0.],[1;2],first as u32,1,Arc::downgrade(&coordinates)));}
    let epoch=pool.epoch.load(Ordering::Acquire);let id=PortableId::random();
    for _ in 0..32 {
        assert!(!waiting.request(id,d.clone(),[0;2]));assert!(waiting.pending());assert!(!waiting.ready());assert!(waiting.outstanding.is_empty());
        assert_eq!(waiting.waiting,Some(epoch));assert_eq!(pool.epoch.load(Ordering::Acquire),epoch);
    }
    let (notified,woken)=std::sync::mpsc::channel();waiting.set_wake(Arc::new(move || {let _=notified.send(());}));
    drop(cancelled);assert!(cancelled_context.upgrade().is_none());release.send(()).unwrap();
    woken.recv_timeout(DEADLINE).expect("free request capacity wakes a queue without its own result");
    assert_ne!(pool.epoch.load(Ordering::Acquire),epoch);assert!(!waiting.pending());
    assert!(waiting.request(id,d.clone(),[0;2]));assert!(waiting.request(id,d.clone(),[0;2]));assert_eq!(waiting.outstanding.len(),1);
    gb.release();gc.release();
    let (actual_id,actual_source,pixels)=image_pixels(result(&mut waiting));
    assert_eq!(actual_id,id);assert!(Arc::ptr_eq(&actual_source,&d));assert_eq!(&samples(&pixels)[..4],&[9,8,7,255]);
    assert!(!waiting.pending());assert!(!waiting.ready());assert_eq!(cancelled_ready.load(Ordering::Acquire),0);
    assert!(cancelled_cache.upgrade().is_none());assert!(dead_source.upgrade().is_none());assert_eq!(gc.reads.load(Ordering::Acquire),1);
    assert_eq!(ga.reads.load(Ordering::Acquire),1);assert_eq!(gb.reads.load(Ordering::Acquire),1);
    let PreparedImageWork::Image(failed)=result(&mut active) else {panic!("gated image result")};
    assert!(failed.pixels.is_err());assert!(!active.pending());
}

#[test]
fn unpolled_results_stay_queue_local_and_do_not_block_another_queue() {
    let pool=DecodePool::start(1).unwrap();let mut full=queue(&pool,RgbSpace::Srgb);let mut other=queue(&pool,RgbSpace::DisplayP3);
    let inputs:Vec<_>=(0..full.capacity).map(|i|(PortableId::random(),layer_core::color::source::rgba8_source([1;2],|_,_|[i as u8,31,19,255]))).collect();
    let deadline=Instant::now()+DEADLINE;
    for (id,source) in &inputs {
        while !full.request(*id,source.clone(),[0;2]) {assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(1));}
        assert!(full.request(*id,source.clone(),[0;2]));
    }
    while full.ready.load(Ordering::Acquire)!=full.capacity {assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(1));}
    assert!(!full.accepts_held(0,false));assert!(full.accepts_held(0,true));
    let source=layer_core::color::source::rgba8_source([1;2],|_,_|[91,82,73,255]);let id=PortableId::random();
    assert!(other.request(id,source.clone(),[0;2]));let (actual_id,actual_source,pixels)=image_pixels(result(&mut other));
    assert_eq!(actual_id,id);assert!(Arc::ptr_eq(&actual_source,&source));assert_eq!(&samples(&pixels)[..4],&[91,82,73,255]);
    assert_eq!(full.ready.load(Ordering::Acquire),full.capacity);assert!(!other.pending());assert!(!other.ready());
    let mut seen=std::collections::BTreeSet::new();
    for remaining in (0..full.capacity).rev() {
        let (id,source,pixels)=image_pixels(full.poll().unwrap());let index=inputs.iter().position(|(expected,_)|*expected==id).unwrap();
        assert!(seen.insert(id));assert!(Arc::ptr_eq(&source,&inputs[index].1));assert_eq!(&samples(&pixels)[..4],&[index as u8,31,19,255]);
        assert_eq!(full.ready.load(Ordering::Acquire),remaining);
    }
    assert_eq!(seen.len(),inputs.len());assert!(!full.pending());assert!(!full.ready());assert!(full.poll().is_none());
}

#[test]
fn shared_workers_cache_embedded_profile_decoders_by_destination_space() {
    let pool=DecodePool::start(1).unwrap();let mut srgb=queue(&pool,RgbSpace::Srgb);let mut p3=queue(&pool,RgbSpace::DisplayP3);
    assert!(!Arc::ptr_eq(&srgb.context.cache,&p3.context.cache));
    let mut source=(*layer_core::color::source::rgba8_source([PAGE_SIZE;2],|x,y|[(x%256) as u8,(y%256) as u8,64,128])).clone();
    source.interpretation.profile=layer_core::color::ColorProfile::Icc(layer_color::profile_bytes(&layer_core::color::ColorProfile::Builtin(RgbSpace::Srgb)).unwrap().into());
    let source=Arc::new(source);
    let references=[RgbSpace::Srgb,RgbSpace::DisplayP3].map(|space| {
        let decoder=layer_color::WorkingDecoder::new(&source.interpretation,space,Default::default()).unwrap();
        prepare_image_tile(&source,[0;2],space,&layer_core::raster::DecodedTileCache::new(0),Some(&decoder)).unwrap()
    });
    assert_ne!(samples(&references[0]),samples(&references[1]));
    for index in [0,1,0,1,1,0] {
        let queue=if index==0 {&mut srgb}else {&mut p3};let id=PortableId::random();
        assert!(queue.request(id,source.clone(),[0;2]));let (actual_id,actual_source,pixels)=image_pixels(result(queue));
        assert_eq!(actual_id,id);assert!(Arc::ptr_eq(&actual_source,&source));assert_eq!(samples(&pixels),samples(&references[index]));
        assert!(!queue.pending());assert!(!queue.ready());
    }
}
