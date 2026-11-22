use gtk::glib;
use std::{cell::{Cell, RefCell}, rc::{Rc, Weak}, time::{Duration, Instant}};

const ARENAS: libc::c_int = 8;
const QUIET: Duration = Duration::from_secs(2);

type Workspaces = RefCell<Vec<Rc<crate::workspace::Workspace>>>;

thread_local! {
    static LAST_ACTIVITY: Cell<Option<Instant>> = const { Cell::new(None) };
    static RELEASE_ARMED: Cell<bool> = const { Cell::new(false) };
    static WORKSPACES: RefCell<Weak<Workspaces>> = const { RefCell::new(Weak::new()) };
}

pub(crate) fn cap_arenas() {
    unsafe { libc::mallopt(libc::M_ARENA_MAX, ARENAS) };
}

pub(crate) fn watch_contacts(workspaces: &Rc<Workspaces>) {
    WORKSPACES.set(Rc::downgrade(workspaces));
}

pub(crate) fn active() {
    LAST_ACTIVITY.set(Some(Instant::now()));
}

pub(crate) fn schedule_release() {
    if !RELEASE_ARMED.replace(true) { arm(QUIET); }
}

fn arm(delay: Duration) {
    glib::timeout_add_local_once(Duration::from_millis(delay.as_micros().div_ceil(1000) as u64), || {
        let contact = WORKSPACES.with_borrow(|workspaces| workspaces.upgrade()
            .is_some_and(|workspaces| workspaces.borrow().iter().any(|workspace| workspace.input.contact())));
        match wait_before_release(Instant::now(), LAST_ACTIVITY.get(), contact) {
            Some(delay) => arm(delay),
            None => {
                RELEASE_ARMED.set(false);
                let _ = release_freed();
            }
        }
    });
}

fn wait_before_release(now: Instant, last_activity: Option<Instant>, contact: bool) -> Option<Duration> {
    let quiet = last_activity.map_or(QUIET, |last| now.saturating_duration_since(last));
    if contact { Some(QUIET) } else { (quiet < QUIET).then(|| QUIET - quiet) }
}

fn release_freed() -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name("capy-heap-release".into()).spawn(|| { unsafe { libc::malloc_trim(0) }; })
}

#[cfg(test)]
mod tests {
    const THREADS: usize = 8;
    const BYTES_PER_THREAD: usize = 16 << 20;
    const SMALLEST: usize = 2 << 10;
    const LARGEST: usize = 96 << 10;

    fn resident_pages(ranges: &[(usize, usize)]) -> (usize, usize) {
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        let (mut resident, mut total) = (0, 0);
        for &(start, len) in ranges {
            let mut address = (start + 64).next_multiple_of(page);
            while address + page <= start + len {
                let mut state = 0u8;
                assert_eq!(unsafe { libc::mincore(address as *mut libc::c_void, page, &mut state) }, 0);
                resident += usize::from(state & 1 == 1);
                total += 1;
                address += page;
            }
        }
        (resident, total)
    }

    #[test]
    fn release_waits_for_quiet_input_and_lifted_contacts() {
        use super::{QUIET, wait_before_release};
        use std::time::Duration;
        let start = std::time::Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        assert_eq!(wait_before_release(at(2_000), Some(start), false), None);
        assert_eq!(wait_before_release(at(2_000), Some(at(500)), false), Some(Duration::from_millis(500)), "a frame or pen event in any window postpones the release");
        assert_eq!(wait_before_release(at(9_000), Some(start), true), Some(QUIET), "a resting pen or touch holds the release");
        assert_eq!(wait_before_release(at(9_000), Some(at(7_000)), false), None);
        assert_eq!(wait_before_release(at(0), None, false), None);
    }

    #[test]
    fn idle_release_returns_pages_freed_across_thread_arenas() {
        let freed = std::sync::Mutex::new(Vec::with_capacity(THREADS * BYTES_PER_THREAD / SMALLEST));
        let allocated = std::sync::Barrier::new(THREADS + 1);
        let measured = std::sync::Barrier::new(THREADS + 1);
        std::thread::scope(|scope| {
            let workers = (0..THREADS).map(|thread| {
                let (freed, allocated, measured) = (&freed, &allocated, &measured);
                scope.spawn(move || {
                    let fill = thread as u8 + 1;
                    let mut seed = 0x9e37_79b9_7f4a_7c15u64 ^ fill as u64;
                    let mut blocks = Vec::with_capacity(BYTES_PER_THREAD / SMALLEST);
                    let mut total = 0;
                    while total < BYTES_PER_THREAD {
                        seed ^= seed << 13;
                        seed ^= seed >> 7;
                        seed ^= seed << 17;
                        let len = SMALLEST + (seed % (LARGEST - SMALLEST) as u64) as usize;
                        blocks.push(vec![fill; len]);
                        total += len;
                    }
                    let last = blocks.len() - 1;
                    let mut kept = Vec::with_capacity(blocks.len() / 4 + 1);
                    let mut released = freed.lock().unwrap();
                    for (index, block) in blocks.into_iter().enumerate() {
                        if index % 4 == 3 || index == last { kept.push(block); }
                        else { released.push((block.as_ptr() as usize, block.len())); }
                    }
                    drop(released);
                    allocated.wait();
                    measured.wait();
                    (fill, kept)
                })
            }).collect::<Vec<_>>();
            allocated.wait();
            let freed = freed.lock().unwrap();
            let (before, total) = resident_pages(&freed);
            super::release_freed().unwrap().join().unwrap();
            let (after, _) = resident_pages(&freed);
            drop(freed);
            measured.wait();
            for worker in workers {
                let (fill, kept) = worker.join().unwrap();
                assert!(kept.iter().all(|block| block.iter().all(|&byte| byte == fill)));
            }
            assert!(total > 0 && before * 10 >= total * 9, "{before} of {total} freed pages were resident before release");
            assert!(after * 10 <= total, "{after} of {total} freed pages stayed resident after release");
        });
    }
}
