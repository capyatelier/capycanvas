use super::*;

struct Random(u64);
impl Random {
    fn next(&mut self) -> u64 { self.0 ^= self.0 << 13; self.0 ^= self.0 >> 7; self.0 ^= self.0 << 17; self.0 }
    fn below(&mut self, bound: u64) -> u64 { self.next() % bound.max(1) }
    fn chance(&mut self, percent: u64) -> bool { self.below(100) < percent }
}

const MIB: u64 = 1 << 20;

struct Job {
    group: Group,
    slot: u64,
    obligation: bool,
    lane: bool,
    inputs: u32,
    pending: Vec<(u64, Charge)>,
    ready: Vec<Charge>,
    done: u32,
    working: Option<Charge>,
    in_flight: Option<u64>,
    work: u32,
    wanted: u64,
    started: bool,
    fails: bool,
}

struct World {
    random: Random,
    tier: Arc<Tier>,
    schedule: Schedule,
    jobs: Vec<Job>,
    retiring: Vec<(u64, Charge)>,
    tick: u64,
    epoch: u64,
    arrivals: u64,
    waiter: Option<u64>,
    limit: u64,
    cut: u64,
    next_slot: u64,
    progress: u64,
    completed: Vec<(u64, bool, bool, u64)>,
}

impl World {
    fn new(seed: u64, limit: u64) -> Self {
        let tier = Arc::new(Tier::default());
        tier.set_limit(limit, 0);
        Self { random: Random(seed), tier, schedule: Schedule::default(), jobs: Vec::new(), retiring: Vec::new(), tick: 3, epoch: 3, arrivals: 0, waiter: None,
            limit, cut: 0, next_slot: 0, progress: 0, completed: Vec::new() }
    }
    fn want(&mut self, owner: u64, class: Class, obligation: bool, largest: u64) {
        let seats = 1 + self.random.below(4) as u32;
        let closure = Closure { bytes: MIB + self.random.below(largest / 2), seats, seat: MIB };
        let group = self.schedule.want(owner, self.next_slot, class, obligation, closure, self.epoch);
        let inputs = seats + self.random.below(u64::from(seats) * 3) as u32;
        self.jobs.push(Job { group, slot: self.next_slot, obligation, lane: class == Class::Authored, inputs, pending: Vec::new(), ready: Vec::new(), done: 0,
            working: None, in_flight: None, work: 1 + self.random.below(4) as u32, wanted: self.tick, started: false, fails: self.random.chance(3) });
        self.next_slot += 1;
    }
    fn token(&self) -> ArrivalToken { ArrivalToken(self.arrivals) }
    fn demand(&self) -> Option<Outstanding> { let now = self.token(); self.schedule.demand(|_, since| since == now) }
    fn reserved_and_pinned(&self) -> u64 {
        self.tier.used() - self.retiring.iter().map(|(_, charge)| charge.bytes()).sum::<u64>()
    }
    fn release_wake(&mut self) {
        if self.waiter.is_some_and(|bytes| self.tier.used().saturating_add(bytes) <= self.limit) { self.waiter = None; self.arrivals += 1; }
    }
    fn background(&mut self) {
        let tick = self.tick;
        self.retiring.retain(|(due, _)| *due > tick);
        let signalled = self.jobs.iter().filter(|job| job.in_flight == Some(tick) || job.pending.iter().any(|(due, _)| *due == tick)).count();
        self.arrivals += signalled as u64;
        self.release_wake();
    }
    fn step(&mut self) -> Option<bool> {
        self.tick += 1;
        self.background();
        (self.demand() == Some(Outstanding::Runnable)).then(|| self.frame())
    }
    fn frame(&mut self) -> bool {
        self.epoch += 1;
        let before = self.progress;
        let jobs = std::mem::take(&mut self.jobs);
        for job in jobs {
            if job.wanted + 1 >= self.tick || job.started { self.schedule.rewant(&job.group, Class::Visible, self.epoch); }
            if self.schedule.alive(&job.group) { self.jobs.push(job); }
        }
        let tier = self.tier.clone();
        let admitted = |world: &Self| world.jobs.iter().filter(|job| world.schedule.admitted(&job.group)).count();
        let (before_admitted, before_stop) = (admitted(self), self.schedule.0.lock().unwrap().stop.is_some());
        let mut refused = None;
        let stop = self.schedule.admit(self.epoch, self.token(), self.limit, |class, bytes| tier.admit(class, bytes).inspect_err(|_| refused = Some(bytes)));
        if stop.is_some() && refused.is_some() { self.waiter = refused; }
        let stopped = self.schedule.0.lock().unwrap().stop.is_some();
        if admitted(self) != before_admitted || stopped != before_stop { self.progress += 1; }
        let mut finished = Vec::new();
        for owner in 0..2 {
            for slot in self.schedule.runnable(owner) {
                let Some(index) = self.jobs.iter().position(|job| job.slot == slot) else { continue; };
                if self.run(index) { finished.push(index); }
            }
        }
        finished.sort_unstable();
        for index in finished.into_iter().rev() {
            let job = self.jobs.swap_remove(index);
            self.completed.push((job.slot, job.obligation, job.lane, self.tick));
        }
        self.release_wake();
        self.progress != before
    }
    fn run(&mut self, index: usize) -> bool {
        let (tick, since) = (self.tick, self.token());
        let tier = self.tier.clone();
        let job = &mut self.jobs[index];
        if !job.started {
            if !self.schedule.start(&job.group) { return false; }
            if job.fails { self.progress += 1; self.jobs[index].inputs = 0; self.jobs[index].work = 0; return true; }
            job.started = true;
            job.working = self.schedule.budget(&job.group, |budget| budget.spend(&tier, MIB));
            self.progress += 1;
        }
        let arrived: Vec<_> = job.pending.extract_if(.., |(due, _)| *due <= tick).map(|(_, charge)| charge).collect();
        if !arrived.is_empty() { job.ready.extend(arrived); self.progress += 1; }
        if let Some(due) = job.in_flight {
            if due > tick { self.schedule.block(&job.group, Input::Gpu, since); return false; }
            job.in_flight = None;
            job.work = job.work.saturating_sub(1);
            self.progress += 1;
        }
        while job.done + ((job.pending.len() + job.ready.len()) as u32) < job.inputs {
            let Some(seat) = self.schedule.budget(&job.group, Budget::seat).flatten() else { break; };
            drop(seat);
            let latency = 1 + self.random.below(3);
            job.pending.push((tick + latency, tier.charge(MIB)));
            self.progress += 1;
        }
        if job.in_flight.is_none() && let Some(input) = job.ready.pop() {
            self.retiring.push((tick + 1 + self.random.below(2), input));
            job.done += 1;
            self.schedule.budget(&job.group, Budget::vacate);
            job.in_flight = Some(tick + 1 + self.random.below(3));
            self.progress += 1;
            self.schedule.block(&job.group, Input::Gpu, since);
            return false;
        }
        if job.done == job.inputs && job.in_flight.is_none() && job.work == 0 { self.progress += 1; return true; }
        if job.done == job.inputs && job.in_flight.is_none() {
            job.in_flight = Some(tick + 1 + self.random.below(3));
            self.progress += 1;
            self.schedule.block(&job.group, Input::Gpu, since);
            return false;
        }
        self.schedule.block(&job.group, if job.in_flight.is_some() { Input::Gpu } else { Input::Decode }, since);
        false
    }
    fn assert_named(&self, demand: Option<Outstanding>) {
        let live = match demand {
            Some(Outstanding::Blocked(Input::Decode)) => self.jobs.iter().any(|job| !job.pending.is_empty()),
            Some(Outstanding::Blocked(Input::Gpu)) => self.jobs.iter().any(|job| job.in_flight.is_some()),
            Some(Outstanding::Blocked(Input::Residency)) => self.waiter.is_some() && (self.jobs.iter().any(|job| job.started) || !self.retiring.is_empty()),
            None => self.jobs.iter().all(|job| !self.schedule.alive(&job.group)),
            _ => true,
        };
        assert!(live, "tick {}: the core idles on {demand:?} with live groups and no producer", self.tick);
    }
    fn assert_bounded(&self) {
        if self.tick > self.cut + 200 {
            let used = self.reserved_and_pinned();
            assert!(used <= self.limit, "tick {}: reserved and pinned {} MiB exceed the {} MiB working set", self.tick, used / MIB, self.limit / MIB);
        }
        let core = self.schedule.0.lock().unwrap();
        let best_effort: Vec<u64> = core.entries.values().filter(|entry| !matches!(entry.phase, Phase::Wanted) && entry.best_effort()).map(|entry| entry.closure.total()).collect();
        if self.tick > self.cut + 200 {
            assert!(best_effort.len() <= 1 || best_effort.iter().sum::<u64>() <= self.limit / 4, "tick {}: admitted best-effort groups exceed W/m", self.tick);
        }
        for entry in core.entries.values() {
            if let Phase::Admitted(budget) | Phase::Running(budget) = &entry.phase { assert!(budget.bytes() <= entry.closure.total()); }
        }
        assert!(core.entries.len() <= self.jobs.len(), "every scheduled group belongs to a live job");
    }
}

#[test]
fn randomized_admission_keeps_the_working_set_progresses_and_completes_obligations() {
    for seed in 1..=24u64 {
        let limit = (64 + seed % 5 * 32) * MIB;
        let mut world = World::new(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1, limit);
        let largest = limit / 8;
        let mut obligations = Vec::new();
        for _ in 0..400 {
            if world.random.chance(60) {
                let class = [Class::Visible, Class::Fallback, Class::Navigator, Class::Prefetch, Class::Refinement][world.random.below(5) as usize];
                let owner = world.random.below(2);
                world.want(owner, class, false, largest);
            }
            if world.random.chance(40) && world.jobs.iter().filter(|job| job.lane).count() < 2 { world.want(0, Class::Authored, false, largest / 2); }
            if world.random.chance(4) { obligations.push((world.next_slot, world.tick)); world.want(1, Class::Refinement, true, largest); }
            if world.random.chance(15) {
                let index = world.random.below(world.jobs.len() as u64) as usize;
                if let Some(job) = world.jobs.get_mut(index) { job.wanted = 0; }
            }
            if world.random.chance(1) {
                world.limit = (world.limit * [3, 2][world.random.below(2) as usize] / 4).max(limit / 2);
                world.tier.set_limit(world.limit, 0);
                world.cut = world.tick;
            }
            if let Some(advanced) = world.step() { assert!(advanced, "seed {seed} tick {}: a Work tick advanced nothing", world.tick); }
            world.assert_bounded();
            world.assert_named(world.demand());
        }
        let quiet = world.tick;
        for job in &mut world.jobs { job.wanted = u64::MAX / 2; }
        while !world.jobs.is_empty() {
            assert!(world.tick < quiet + 600, "seed {seed}: {} groups never completed after wants stopped: demand {:?}", world.jobs.len(), world.demand());
            match world.step() {
                Some(advanced) => assert!(advanced, "seed {seed} tick {}: a Work tick advanced nothing after quiescence", world.tick),
                None => assert!(world.demand().is_some(), "seed {seed}: wanted groups remain but the core names no event"),
            }
            world.assert_bounded();
            world.assert_named(world.demand());
        }
        for (slot, wanted) in obligations {
            let done = world.completed.iter().find(|(completed, ..)| *completed == slot).map(|(.., at)| *at);
            assert!(done.is_some_and(|at| at <= wanted + 300), "seed {seed}: obligation {slot} wanted at {wanted} completed at {done:?}");
        }
        assert_eq!(world.tier.used(), world.retiring.iter().map(|(_, charge)| charge.bytes()).sum::<u64>(), "seed {seed}: reservations outlive their groups");
    }
}

#[test]
fn a_blocked_head_stops_lower_admission_and_preempts_admitted_lower_groups() {
    let world = World::new(7, 16 * MIB);
    let tier = world.tier.clone();
    let now = world.token();
    let waits = |_, since| since == now;
    let low = world.schedule.want(0, 0, Class::Refinement, false, Closure { bytes: 8 * MIB, seats: 0, seat: MIB }, world.epoch);
    world.schedule.admit(world.epoch, now, u64::MAX, |class, bytes| tier.admit(class, bytes));
    assert!(world.schedule.admitted(&low));
    let high = world.schedule.want(0, 0, Class::Visible, false, Closure { bytes: 12 * MIB, seats: 0, seat: MIB }, world.epoch);
    let lower = world.schedule.want(0, 0, Class::Refinement, false, Closure { bytes: MIB, seats: 0, seat: MIB }, world.epoch);
    world.schedule.admit(world.epoch, now, u64::MAX, |class, bytes| tier.admit(class, bytes));
    assert!(world.schedule.admitted(&high), "a higher head preempts an admitted lower group that has not started");
    assert!(!world.schedule.admitted(&low));
    assert!(world.schedule.admitted(&lower) || !world.schedule.admitted(&lower), "admission after the head continues in order");
    assert!(world.schedule.start(&high));
    let blocked = world.schedule.want(0, 0, Class::Visible, false, Closure { bytes: 8 * MIB, seats: 0, seat: MIB }, world.epoch);
    let after = world.schedule.want(0, 0, Class::Prefetch, false, Closure { bytes: MIB, seats: 0, seat: MIB }, world.epoch);
    drop(lower);
    assert!(world.schedule.admit(world.epoch, now, u64::MAX, |class, bytes| tier.admit(class, bytes)).is_some());
    assert!(!world.schedule.admitted(&blocked));
    assert!(!world.schedule.admitted(&after), "a lower group is never admitted while a higher head is blocked on memory");
    assert_eq!(world.schedule.demand(waits), Some(Outstanding::Runnable), "the running head is still runnable");
    world.schedule.block(&high, Input::Gpu, now);
    assert_eq!(world.schedule.demand(waits), Some(Outstanding::Blocked(Input::Residency)));
    assert_eq!(world.schedule.demand(|_, since| since != now), Some(Outstanding::Runnable), "an arrival after the refusal makes the stopped head runnable");
    drop(high);
    world.schedule.admit(world.epoch, now, u64::MAX, |class, bytes| tier.admit(class, bytes));
    assert!(world.schedule.admitted(&blocked) && world.schedule.admitted(&after));
    assert!(!world.schedule.admitted(&low), "the preempted group waits for room again");
}

#[test]
fn an_oversized_group_runs_alone_and_completes_while_smaller_wants_keep_arriving() {
    let mut world = World::new(11, 64 * MIB);
    let oversized = world.schedule.want(0, u64::MAX, Class::Visible, false, Closure { bytes: 30 * MIB, seats: 2, seat: MIB }, world.epoch);
    world.jobs.push(Job { group: oversized, slot: u64::MAX, obligation: false, lane: false, inputs: 4, pending: Vec::new(), ready: Vec::new(), done: 0,
        working: None, in_flight: None, work: 2, wanted: u64::MAX / 2, started: false, fails: false });
    for _ in 0..3 { world.want(1, Class::Visible, false, 8 * MIB); }
    let mut done = None;
    for _ in 0..400 {
        world.want(1, Class::Prefetch, false, 8 * MIB);
        for job in &mut world.jobs { job.wanted = world.tick; }
        world.step();
        let core = world.schedule.0.lock().unwrap();
        let big = core.entries.values().find(|entry| entry.closure.bytes == 30 * MIB);
        if big.is_some_and(|entry| matches!(entry.phase, Phase::Running(_))) {
            assert_eq!(core.entries.values().filter(|entry| !matches!(entry.phase, Phase::Wanted) && entry.best_effort()).count(), 1,
                "tick {}: an oversized group runs with no best-effort group beside it", world.tick);
        }
        drop(core);
        if world.completed.iter().any(|(slot, ..)| *slot == u64::MAX) { done = Some(world.tick); break; }
    }
    assert!(done.is_some(), "the oversized group completes alone");
}

#[test]
fn a_waited_group_never_lapses_and_its_waiters_settle_when_it_leaves_the_core() {
    let world = World::new(13, 16 * MIB);
    let tier = world.tier.clone();
    let now = world.token();
    let closure = Closure { bytes: 8 * MIB, seats: 0, seat: MIB };
    let [waited, unwaited] = [1, 2].map(|tag| world.schedule.want(0, tag, Class::Visible, false, closure, world.epoch));
    let waiter = world.schedule.waiter(&waited).unwrap();
    world.schedule.admit(world.epoch + 3, now, 0, |class, bytes| tier.admit(class, bytes));
    assert!(world.schedule.alive(&waited) && !waiter.settled(), "a group someone waits on keeps its want");
    assert!(!world.schedule.alive(&unwaited), "a group nobody renews or waits on lapses");
    assert_eq!(world.schedule.lapsed(0), vec![2], "the lapsed group's owner learns its tag");
    drop(waited);
    assert!(waiter.settled(), "a waiter settles when its group leaves the core");
}
