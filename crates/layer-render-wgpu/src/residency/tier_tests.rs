use super::*;
use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}, mpsc};
use std::time::Duration;

fn tier(limit: u64, floor: u64) -> Arc<Tier> { Arc::new(Tier { limit: AtomicU64::new(limit), floor: AtomicU64::new(floor), ..Tier::default() }) }

fn next(seed: &mut u64) -> u64 { *seed ^= *seed << 13; *seed ^= *seed >> 7; *seed ^= *seed << 17; *seed }

#[test]
fn seeded_charges_frees_and_retirements_match_a_full_walk() {
    let (edge, owner) = crate::device_poll::windowless();
    let tier = tier(u64::MAX, 0);
    let mut live: Vec<Charge> = Vec::new();
    let mut retired: Vec<(u64, Arc<AtomicBool>)> = Vec::new();
    let mut seed = 0x5eed_b1b0_d6e1_35eeu64;
    for step in 0..20_000 {
        let roll = next(&mut seed);
        match roll % 5 {
            0 | 1 => live.push(tier.charge(1 + (roll >> 8) % (1 << 20))),
            2 if !live.is_empty() => drop(live.swap_remove((roll >> 8) as usize % live.len())),
            3 if !live.is_empty() => {
                let charge = live.swap_remove((roll >> 8) as usize % live.len());
                let fence = Arc::new(AtomicBool::new(false));
                retired.push((charge.bytes(), fence.clone()));
                edge.dispose_when(charge, move |_| fence.load(Ordering::Acquire));
            },
            4 if !retired.is_empty() => {
                let (_, fence) = retired.swap_remove((roll >> 8) as usize % retired.len());
                fence.store(true, Ordering::Release);
                edge.release_wake()();
            },
            _ => {},
        }
        if step % 97 == 0 {
            edge.flush().unwrap();
            let walk = live.iter().map(Charge::bytes).sum::<u64>() + retired.iter().map(|(bytes, _)| bytes).sum::<u64>();
            assert_eq!(tier.used(), walk, "step={step} seed={seed:#x}");
        }
    }
    for (_, fence) in &retired { fence.store(true, Ordering::Release); }
    edge.release_wake()();
    drop(live);
    edge.flush().unwrap();
    assert_eq!(tier.used(), 0);
    drop(owner);
}

#[test]
fn a_refusal_behind_this_frames_claims_waits_for_room_beyond_them() {
    let (edge, owner) = crate::device_poll::windowless();
    let tier = tier(8, 0);
    let resident = tier.charge(3);
    tier.begin_frame();
    let earlier = tier.charge(3);
    let blocked = tier.admit(Class::Prefetch, 3).err().expect("the frame's earlier claim leaves no room");
    let (send, woken) = mpsc::channel();
    blocked.clone().wake_on_release(&edge, Arc::new(move || { let _ = send.send(()); }));
    drop(earlier);
    tier.end_frame();
    assert!(!blocked.fits(), "the next frame claims the same room before this unit");
    assert!(woken.recv_timeout(Duration::from_millis(50)).is_err(), "releasing the frame's own claim does not wake the unit");
    drop(resident);
    assert!(blocked.fits(), "room beyond the frame's claims fits the unit");
    woken.recv_timeout(Duration::from_secs(1)).expect("room beyond the frame's claims wakes the unit");
    drop(owner);
}

#[test]
fn a_refusal_behind_this_frames_lasting_claims_fits_once_the_frame_ends() {
    let tier = tier(8, 0);
    tier.begin_frame();
    let lasting = tier.charge(5);
    let blocked = tier.admit(Class::Prefetch, 4).err().expect("the frame's lasting claim leaves no room");
    tier.end_frame();
    assert!(!blocked.fits(), "the lasting claim still fills the room");
    drop(lasting);
    assert!(blocked.fits(), "a lasting claim is not counted again once it is released");
}

#[test]
fn a_capacity_refusal_wakes_once_on_release_without_polling() {
    let (edge, owner) = crate::device_poll::windowless();
    let tier = tier(8, 0);
    let held = tier.charge(8);
    let blocked = tier.admit(Class::Visible, 4).err().expect("a full tier refuses");
    let (send, woken) = mpsc::channel();
    blocked.wake_on_release(&edge, Arc::new(move || { let _ = send.send(()); }));
    assert!(woken.recv_timeout(Duration::from_millis(20)).is_err(), "woken before any release");
    let release = std::thread::spawn(move || drop(held));
    woken.recv_timeout(Duration::from_secs(1)).expect("a Charge release must wake the refused unit");
    release.join().unwrap();
    let admitted = tier.admit(Class::Visible, 4).unwrap().charge(4);
    drop(tier.charge(1));
    assert!(woken.recv_timeout(Duration::from_millis(20)).is_err(), "a consumed wake must not repeat");
    let full = tier.admit(Class::Visible, 8).err().unwrap();
    drop(admitted);
    let (send, woken) = mpsc::channel();
    full.wake_on_release(&edge, Arc::new(move || { let _ = send.send(()); }));
    woken.recv_timeout(Duration::from_secs(1)).expect("a release before registration must be found by the recheck");
    drop(owner);
}

#[test]
fn a_prefetch_reservation_cannot_take_the_floor() {
    let tier = tier(16, 4);
    let prefetch = tier.admit(Class::Prefetch, 12).unwrap();
    assert!(tier.admit(Class::Prefetch, 1).is_err(), "class 4 took the floor");
    let visible = tier.admit(Class::Visible, 4).unwrap();
    assert_eq!(tier.used(), 16);
    drop(prefetch);
    assert!(tier.admit(Class::Prefetch, 9).is_err(), "class 4 took the floor held by visible work");
    let refill = tier.admit(Class::Prefetch, 8).unwrap();
    assert_eq!(tier.used(), 12);
    drop((visible, refill));
    assert_eq!(tier.used(), 0);
}

#[test]
fn two_background_admits_before_any_charge_cannot_share_the_same_free_space() {
    let tier = tier(16, 4);
    let first = tier.admit(Class::Prefetch, 7).unwrap();
    assert!(tier.admit(Class::Prefetch, 7).is_err(), "the second admit saw the first one's bytes as free");
    let charge = first.charge(5);
    assert_eq!(tier.used(), 5);
    let second = tier.admit(Class::Prefetch, 7).unwrap();
    drop((charge, second));
    assert_eq!(tier.used(), 0);
}

#[test]
fn a_reservation_within_a_renderer_tier_counts_in_the_renderer_tier_until_its_charge_drops() {
    let renderer = tier(16,0);
    let scene = Tier::within(&renderer);
    scene.set_limit(16, 0);
    let reservation = scene.admit(Class::Visible, 12).unwrap();
    assert_eq!(renderer.used(), 12, "the reservation is invisible to the renderer tier");
    let charge = reservation.charge(5);
    assert_eq!((scene.used(), renderer.used()), (5, 5));
    let grown = scene.admit(Class::Visible, 2).unwrap().charge(9);
    assert_eq!((scene.used(), renderer.used()), (14, 14));
    drop((charge, grown));
    assert_eq!((scene.used(), renderer.used()), (0, 0));
}

#[test]
fn child_admission_obeys_the_shared_parent_floor_and_rolls_back_refusals() {
    let parent=tier(16,4);
    let first=Tier::within(&parent);let second=Tier::within(&parent);
    let held=first.admit(Class::Prefetch,12).unwrap();
    assert!(second.admit(Class::Prefetch,1).is_err());
    assert_eq!((parent.used(),first.used(),second.used()),(12,12,0));
    let required=second.admit(Class::Visible,4).unwrap();
    drop((held,required));
    assert_eq!((parent.used(),first.used(),second.used()),(0,0,0));
}

#[test]
fn consuming_a_child_reservation_never_charges_its_parent_twice() {
    let parent=tier(16,0);let child=Tier::within(&parent);
    let mut reserved=child.admit(Class::Visible,16).unwrap();
    let mut allocation=child.charge(0);
    allocation.merge(reserved.split(6).unwrap().charge(6));
    assert_eq!((parent.used(),child.used()),(16,16));
    drop(reserved);
    assert_eq!((parent.used(),child.used()),(6,6));
    drop(allocation);assert_eq!(parent.used(),0);
}
