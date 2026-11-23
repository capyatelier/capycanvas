use super::{connect_contact, random_variant, size_jitter, tip_diameter, DabGenerator, DynamicPoint, Evaluation};
use layer_core::{
    BrushAccumulation, BrushBlendMode, BrushCombine, BrushExecution, BrushMapping, BrushSensor, BrushSnapshot,
    BrushTarget, Point, StrokePoint, TaperMode, MAX_BRUSH_MAPPINGS,
};
use layer_render::Dab;
use std::collections::{HashMap, VecDeque};

const PROPORTIONAL_LIMIT_WIDTHS: f32 = 50.;
const MAX_HELD_CONTACTS: usize = 16384;
const MAX_HELD_BYTES: usize = 4 << 20;
const MIN_HELD_PAGES: usize = 4;
const MAX_HELD_PAGES: usize = 64;
const PREVIEW_RUN: usize = 32;
const MAX_INSERTIONS: f32 = 64.;
const SAVED: Resolution = Resolution { share: 0.02, minimum: 0.05, preview: None };
const PATH_SHARE: f32 = 0.02;
const THIN_SHARE: f32 = 0.02;
const FIRST_SAMPLE: f32 = 1e-4;
const SAMPLE_RATIO: f32 = 1.5;

#[derive(Clone, Copy, Debug)]
struct Resolution {
    share: f32,
    minimum: f32,
    preview: Option<f32>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TaperPlan {
    proportional: bool,
    lengths: [f32; 2],
    exponent: f32,
    mapping: usize,
    pub(super) full_width: f32,
    pages: usize,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Profile {
    plan: TaperPlan,
    zones: [f32; 2],
    reference: f32,
    floor: f32,
    length: f32,
    scale: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ContactRecord {
    pub(super) pose: DynamicPoint,
    pub(super) variant: u32,
    pub(super) index: u32,
    pub(super) copy_seed: u32,
    pub(super) copies: u8,
    pub(super) interval: f32,
    pub(super) peak: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Transfer {
    pub(super) mapping: usize,
    progress: f32,
    exponent: f32,
    pub(super) pressure: f32,
    floor: f32,
}

impl Transfer {
    pub(super) fn apply(self, value: f32, scale: impl Fn(f32) -> f32) -> f32 {
        shaped(value, scale(self.pressure), scale(self.floor), self.progress, self.exponent)
    }
}

pub(super) fn outline(progress: f32, exponent: f32) -> f32 {
    let inside = 1. - progress;
    (1. - inside * inside).powf(exponent)
}

pub(super) fn shaped(value: f32, reference: f32, floor: f32, progress: f32, exponent: f32) -> f32 {
    let profile = reference * outline(progress, exponent);
    let entry = (2. * progress - 1.).clamp(0., 1.);
    profile + (value.max(floor) - profile) * entry * entry * (3. - 2. * entry)
}

pub(super) fn mapping_output(mapping: &BrushMapping, input: f32, transfer: Option<Transfer>) -> f32 {
    let width = mapping.input_max - mapping.input_min;
    let normalize = |input: f32| if width.abs() > f32::EPSILON { (input - mapping.input_min) / width } else { 0.0 };
    let output = mapping.curve.sample(normalize(input));
    transfer
        .map_or(output, |transfer| transfer.apply(output, |pressure| mapping.curve.sample(normalize(pressure))))
        .mul_add(mapping.output_scale, mapping.output_bias)
}

pub(super) fn combine(target: f32, contribution: f32, combine: BrushCombine) -> f32 {
    match combine {
        BrushCombine::Replace => contribution,
        BrushCombine::Add => target + contribution,
        BrushCombine::Multiply => target * contribution,
    }
}

fn mapped_width(brush: &BrushSnapshot, mapping: &BrushMapping, pressure: f32, transfer: Option<Transfer>) -> f32 {
    tip_diameter(brush, combine(brush.diameter, mapping_output(mapping, pressure, transfer), mapping.combine))
}

fn full_width(brush: &BrushSnapshot, mapping: &BrushMapping) -> f32 {
    let finite = [0., 1.].into_iter().all(|pressure| {
        let output = mapping_output(mapping, pressure, None);
        output.is_finite() && combine(brush.diameter, output, mapping.combine).is_finite()
    });
    if finite { mapped_width(brush, mapping, 1., None) } else { tip_diameter(brush, brush.bounds.maximum_size) }
}

pub(crate) fn tapers(brush: &BrushSnapshot) -> bool {
    TaperPlan::new(brush).is_some()
}

pub fn can_taper(brush: &BrushSnapshot) -> bool {
    size_mapping(brush).is_some()
}

fn size_mapping(brush: &BrushSnapshot) -> Option<(usize, &BrushMapping)> {
    let contact = brush.contact?;
    let rendering = brush.rendering;
    if brush.execution != BrushExecution::Dry
        || contact.dry.is_some()
        || contact.bristles.is_some()
        || rendering.accumulation != BrushAccumulation::Uniform
        || rendering.blend_mode != BrushBlendMode::Normal
        || rendering.edge_width != 0.
        || rendering.wet_edge != 0.
        || rendering.burnt_edge != 0.
        || brush.transport.is_some()
        || !(brush.diameter > 0.)
    {
        return None;
    }
    let mut sizes = brush.mappings.iter().take(MAX_BRUSH_MAPPINGS).enumerate()
        .filter(|(_, mapping)| mapping.target == BrushTarget::Diameter);
    let (index, size) = sizes.next()?;
    if sizes.next().is_some()
        || size.sensor != BrushSensor::Pressure
        || !(size.input_max > size.input_min)
        || !(size.output_scale > 0.)
        || size.curve.samples.windows(2).any(|pair| !(pair[1] >= pair[0]))
    {
        return None;
    }
    Some((index, size))
}

impl TaperPlan {
    pub(crate) fn new(brush: &BrushSnapshot) -> Option<Self> {
        let lengths = brush.taper.lengths()?;
        let (index, size) = size_mapping(brush)?;
        let plan = Self {
            proportional: brush.taper.mode == TaperMode::Proportional,
            lengths,
            exponent: 0.5 + 0.5 * brush.taper.pointiness,
            mapping: index,
            full_width: full_width(brush, size),
            pages: 0,
        };
        Some(Self { pages: plan.page_ceiling(), ..plan })
    }

    fn page_ceiling(&self) -> usize {
        let tile = layer_core::raster::TILE_SIZE as f32;
        let radius = 0.5 * self.full_width;
        let swept = Dab {
            center: Point { x: 0., y: 0. },
            radii: [radius; 2],
            rotation: [1., 0.],
            motion: [0.; 2],
            color_rgba_linear: [0.; 4],
            flow: 0.,
            hardness: 0.,
            texture_sign: [1.; 2],
            material: [0.; 4],
            previous: [radius, radius, 1., 0.],
            contact: [0.; 4],
            previous_contact: [0.; 4],
        }
        .bounds();
        let footprint = swept.max.x - swept.min.x;
        let span = (footprint / tile).ceil() as usize + 1;
        let crossings = (std::f32::consts::SQRT_2 * (self.held_path() + footprint) / tile).ceil() as usize;
        span.saturating_mul(span.saturating_add(crossings)).min(MAX_HELD_PAGES).max(MIN_HELD_PAGES.max(span * span))
    }

    fn extent(&self, end: usize, length: f32) -> f32 {
        if self.proportional {
            (self.lengths[end] * length).min(PROPORTIONAL_LIMIT_WIDTHS * self.full_width)
        } else {
            self.lengths[end] * self.full_width
        }
    }

    fn edge(&self, length: f32) -> f32 {
        length - self.extent(1, length)
    }

    fn profile_from(&self, length: f32, reference: f32, entry: f32) -> Profile {
        let zones = [self.extent(0, length), self.extent(1, length)];
        let zone = if zones[1] > 0. { zones[1] } else { zones[0] };
        let scale = if self.proportional { 1. } else { (length / zone).min(1.) };
        Profile { plan: *self, zones, reference, floor: reference.min(entry), length, scale }
    }

    fn held_path(&self) -> f32 {
        let [start, end] = self.lengths.map(|length| length.max(0.));
        if self.proportional {
            let limit = PROPORTIONAL_LIMIT_WIDTHS * self.full_width;
            let hold = if start > 0. { limit / start } else { 0. };
            hold.max(if end > 0. { limit } else { 0. })
        } else {
            self.full_width * if end > 0. { end } else { start }
        }
    }

    fn settled(&self, last: Option<&ContactRecord>, length: f32) -> bool {
        let Some(last) = last else {
            return true;
        };
        let start = if self.proportional {
            self.lengths[0] <= 0. || self.lengths[0] * length >= PROPORTIONAL_LIMIT_WIDTHS * self.full_width
        } else {
            self.lengths[1] > 0. || length >= self.extent(0, length)
        };
        let end = self.lengths[1] <= 0. || last.pose.stroke_distance <= length - self.extent(1, length);
        start && end
    }

    fn progress_at(&self, outline: f32) -> f32 {
        1. - (1. - outline.max(0.).powf(1. / self.exponent)).max(0.).sqrt()
    }

    fn transfer(&self, progress: f32, pressure: f32, floor: f32) -> Option<Transfer> {
        (progress < 1.).then(|| Transfer { mapping: self.mapping, progress: progress.max(0.), exponent: self.exponent, pressure, floor })
    }
}

impl Profile {
    fn distance(&self, zone: usize, progress: f32) -> f32 {
        let along = 1. - (1. - progress) / self.scale;
        if zone == 0 { along * self.zones[0] } else { self.length - along * self.zones[1] }
    }

    fn progress(&self, zone: usize, distance: f32) -> f32 {
        let along = if zone == 0 { distance / self.zones[0] } else { (self.length - distance) / self.zones[1] };
        1. - self.scale * (1. - along)
    }

    pub(super) fn transfer(&self, distance: f32, peak: f32) -> Option<Transfer> {
        let starting = self.plan.transfer(self.progress(0, distance), peak, 0.);
        let ending = self.plan.transfer(self.progress(1, distance), self.reference, self.floor);
        match (starting, ending) {
            (Some(starting), Some(ending)) if ending.progress < starting.progress => Some(ending),
            (starting, ending) => starting.or(ending),
        }
    }

    fn insertions(&self, from: f32, to: f32, widths: [f32; 2], tolerance: f32, width_at: impl Fn(f32) -> f32) -> Vec<f32> {
        let mut inserted = Vec::new();
        if !(to > from) {
            return inserted;
        }
        let parameter = |distance: f32| ((distance - from) / (to - from)).clamp(0., 1.);
        let width = |distance: f32| match distance {
            _ if distance == from => widths[0],
            _ if distance == to => widths[1],
            _ => width_at(parameter(distance)),
        };
        let meeting = self.length * self.zones[0] / (self.zones[0] + self.zones[1]);
        let parts = [(0, from, to.min(self.zones[0]).min(meeting)), (1, from.max(self.length - self.zones[1]).max(meeting), to)];
        for (zone, low, high) in parts.into_iter().filter(|(_, low, high)| high > low) {
            let steps = ((width(high) - width(low)).abs() / tolerance).ceil().min(MAX_INSERTIONS);
            let shape = |distance: f32| outline(self.progress(zone, distance).clamp(0., 1.), self.plan.exponent);
            let (start, end) = (shape(low), shape(high));
            if !(steps > 1.) || start == end {
                continue;
            }
            inserted.extend((1..steps as usize).map(|step| {
                let along = 1. - (1. - self.plan.progress_at(start + (end - start) * step as f32 / steps)) / self.scale;
                parameter(if zone == 0 { along * self.zones[0] } else { self.length - along * self.zones[1] })
            }));
        }
        inserted
    }
}

fn between(from: DynamicPoint, to: DynamicPoint, t: f32) -> DynamicPoint {
    let mix = |a: f32, b: f32| a + (b - a) * t;
    let turn = |a: f32, b: f32, period: f32| a + ((b - a + 0.5 * period).rem_euclid(period) - 0.5 * period) * t;
    let (a, b) = (from.point, to.point);
    DynamicPoint {
        point: StrokePoint {
            position: Point { x: mix(a.position.x, b.position.x), y: mix(a.position.y, b.position.y) },
            pressure: mix(a.pressure, b.pressure),
            tilt: [mix(a.tilt[0], b.tilt[0]), mix(a.tilt[1], b.tilt[1])],
            twist: turn(a.twist, b.twist, std::f32::consts::TAU),
            elapsed_micros: (f64::from(a.elapsed_micros)
                + (f64::from(b.elapsed_micros) - f64::from(a.elapsed_micros)) * f64::from(t)) as u32,
        },
        speed: mix(from.speed, to.speed),
        direction_turns: turn(from.direction_turns, to.direction_turns, 1.),
        stroke_distance: mix(from.stroke_distance, to.stroke_distance),
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct HeldSlice {
    end: u32,
    records: usize,
    dabs: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Release {
    Settled,
    All,
    First(usize),
}

#[derive(Clone, Debug, Default)]
struct Outline {
    staged: u64,
    kept: VecDeque<u64>,
    anchor: Option<Dab>,
    run: Vec<Dab>,
    multiple: usize,
    peaks: VecDeque<(f32, f32)>,
    last: Option<f32>,
}

impl Outline {
    fn push(&mut self, record: &ContactRecord, dab: Dab) {
        let sequence = self.staged;
        self.staged += 1;
        self.multiple += usize::from(record.copies != 1);
        let Some(anchor) = self.anchor else {
            self.kept.push_back(sequence);
            self.anchor = Some(dab);
            return;
        };
        if let Some(&latest) = self.run.last() {
            let before = self.run.len().checked_sub(2).map_or(anchor, |index| self.run[index]);
            let fits = self.run.len() < PREVIEW_RUN
                && latest.contact[2].floor() == before.contact[2].floor()
                && self.run.iter().all(|inner| redundant(&anchor, &dab, inner, f32::INFINITY, PATH_SHARE));
            if !fits {
                self.kept.push_back(sequence - 1);
                self.anchor = Some(latest);
                self.run.clear();
            }
        }
        self.run.push(dab);
    }

    fn seal(&mut self, record: &ContactRecord) {
        if let Some(previous) = self.last {
            self.peak(previous, record.interval);
        }
        self.peak(record.pose.stroke_distance, record.pose.point.pressure);
        self.last = Some(record.pose.stroke_distance);
    }

    fn peak(&mut self, key: f32, value: f32) {
        while self.peaks.back().is_some_and(|(_, peak)| *peak <= value) {
            self.peaks.pop_back();
        }
        self.peaks.push_back((key, value));
    }

    fn drain(&mut self, drained: impl Iterator<Item = ContactRecord>, remaining: usize) {
        let mut last = None;
        for record in drained {
            self.multiple -= usize::from(record.copies != 1);
            last = Some(record.pose.stroke_distance);
        }
        let base = self.staged - remaining as u64;
        while self.kept.front().is_some_and(|sequence| *sequence < base) {
            self.kept.pop_front();
        }
        if let Some(last) = last {
            while self.peaks.front().is_some_and(|(key, _)| *key <= last) {
                self.peaks.pop_front();
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Pages(HashMap<[i32; 2], u32>);

impl Pages {
    fn keys(dab: &Dab) -> impl Iterator<Item = [i32; 2]> {
        let tile = layer_core::raster::TILE_SIZE as f32;
        let bounds = dab.bounds();
        let span = |min: f32, max: f32| if bounds.is_empty() { 1..=0 } else { (min / tile).floor() as i32..=(max / tile).floor() as i32 };
        let columns = span(bounds.min.x, bounds.max.x);
        span(bounds.min.y, bounds.max.y).flat_map(move |y| columns.clone().map(move |x| [x, y]))
    }

    fn add(&mut self, dab: &Dab) {
        for key in Self::keys(dab) {
            *self.0.entry(key).or_insert(0) += 1;
        }
    }

    fn remove(&mut self, dab: &Dab) {
        for key in Self::keys(dab) {
            if let Some(count) = self.0.get_mut(&key) {
                *count -= 1;
                if *count == 0 {
                    self.0.remove(&key);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Chain<'a> {
    held: &'a VecDeque<ContactRecord>,
    sealed: usize,
    tail: &'a [ContactRecord],
    previous: Option<ContactRecord>,
}

impl Chain<'_> {
    fn len(&self) -> usize {
        self.sealed + self.tail.len()
    }

    fn get(&self, index: usize) -> ContactRecord {
        if index < self.sealed { self.held[index] } else { self.tail[index - self.sealed] }
    }

    fn before(&self, index: usize) -> Option<ContactRecord> {
        index.checked_sub(1).map(|index| self.get(index)).or(self.previous)
    }

    fn distance(&self, index: usize) -> f32 {
        self.get(index).pose.stroke_distance
    }

    fn after(&self, distance: f32) -> usize {
        let held = self.held.partition_point(|record| record.pose.stroke_distance < distance).min(self.sealed);
        if held < self.sealed { held } else { self.sealed + self.tail.partition_point(|record| record.pose.stroke_distance < distance) }
    }

    fn nearest(&self, distance: f32) -> usize {
        match self.after(distance) {
            0 => 0,
            after if after >= self.len() => self.len() - 1,
            after if distance - self.distance(after - 1) <= self.distance(after) - distance => after - 1,
            after => after,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Scratch {
    records: Vec<ContactRecord>,
    dabs: Vec<Dab>,
    shaped: Vec<Dab>,
    group: Vec<Dab>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct HeldStroke {
    cuts: Vec<u32>,
    sealed: usize,
    sealed_end: u32,
    released: usize,
    slices: VecDeque<HeldSlice>,
    records: VecDeque<ContactRecord>,
    dabs: VecDeque<Dab>,
    open: HeldSlice,
    previous: Option<(ContactRecord, Dab)>,
    forced: bool,
    outline: Outline,
    pages: Pages,
    scratch: Scratch,
}

impl HeldStroke {
    pub(crate) fn with_cuts(cuts: &[u32]) -> Self {
        Self { cuts: cuts.to_vec(), ..Self::default() }
    }

    pub(crate) fn cuts(&self) -> &[u32] {
        &self.cuts
    }

    pub(crate) fn fed(&self) -> u32 {
        self.sealed_end.max(self.open.end)
    }

    pub(crate) fn released(&self) -> usize {
        self.released
    }

    pub(crate) fn forced(&self) -> bool {
        self.forced
    }

    pub(crate) fn restart(&mut self) {
        *self = Self::with_cuts(&self.cuts);
    }

    pub(crate) fn stage(&mut self, generator: &mut DabGenerator, dabs: impl IntoIterator<Item = Dab>, end: u32) {
        let first = self.records.len();
        self.open.records += generator.records.len();
        self.records.extend(generator.records.drain(..));
        let before = self.dabs.len();
        self.dabs.extend(dabs);
        self.open.dabs += self.dabs.len() - before;
        self.open.end = self.open.end.max(end);
        let mut dab = before;
        for record in self.records.range(first..) {
            if let Some(&first_copy) = self.dabs.get(dab) {
                self.outline.push(record, first_copy);
            }
            dab += usize::from(record.copies);
        }
        for dab in self.dabs.range(before..) {
            self.pages.add(dab);
        }
    }

    #[cfg(test)]
    pub(crate) fn pages(&self) -> usize {
        self.pages.0.len()
    }

    #[cfg(test)]
    pub(crate) fn held_dabs(&self) -> impl Iterator<Item = &Dab> {
        self.dabs.iter()
    }

    pub(crate) fn at_cut(&self, end: u32) -> bool {
        self.cuts.get(self.sealed) == Some(&end)
    }

    pub(crate) fn finishes_alone(&self, end: u32) -> bool {
        self.at_cut(end) && self.cuts.get(self.sealed + 1) == Some(&end)
    }

    pub(crate) fn seal(&mut self) {
        let end = self.open.end.max(self.sealed_end);
        if end == self.sealed_end && self.open.records == 0 && self.open.dabs == 0 {
            return;
        }
        if let Some(&cut) = self.cuts.get(self.sealed) {
            debug_assert_eq!(cut, end, "a regenerated group ends at its recorded cut");
        }
        let first = self.records.len() - self.open.records;
        for record in self.records.range(first..) {
            self.outline.seal(record);
        }
        self.sealed += 1;
        self.sealed_end = end;
        self.slices.push_back(HeldSlice { end, ..self.open });
        self.open = HeldSlice::default();
    }

    fn chain<'a>(&'a self, tail: &'a [ContactRecord]) -> Chain<'a> {
        Chain { held: &self.records, sealed: self.records.len() - self.open.records, tail, previous: self.previous.map(|(record, _)| record) }
    }

    pub(super) fn profile(&self, plan: &TaperPlan, tail: &[ContactRecord], length: f32, terminal: f32) -> Profile {
        let chain = self.chain(tail);
        let edge = plan.edge(length);
        let mut reference = terminal;
        if let Some((_, peak)) = self.outline.peaks.get(self.outline.peaks.partition_point(|(key, _)| *key < edge)) {
            reference = reference.max(*peak);
        }
        for index in chain.sealed..chain.len() {
            let current = chain.get(index);
            if current.pose.stroke_distance >= edge {
                reference = reference.max(current.pose.point.pressure);
            }
            if chain.before(index).is_some_and(|before| before.pose.stroke_distance >= edge) {
                reference = reference.max(current.interval);
            }
        }
        let inner = chain.after(edge);
        let mut entry = f32::INFINITY;
        if let Some(from) = chain.before(inner).filter(|_| inner < chain.len()) {
            let (from, to) = (from.pose, chain.get(inner).pose);
            let t = (edge - from.stroke_distance) / (to.stroke_distance - from.stroke_distance);
            entry = from.point.pressure + (to.point.pressure - from.point.pressure) * t;
            reference = reference.max(entry);
        }
        plan.profile_from(length, reference, entry)
    }

    pub(crate) fn release(
        &mut self,
        generator: &DabGenerator,
        brush: &BrushSnapshot,
        release: Release,
        mut emit: impl FnMut(&[Dab]),
    ) {
        let Some(plan) = generator.taper else {
            return;
        };
        let (length, terminal) = (generator.modeled_length(), generator.terminal_peak());
        let held = self.records.len() - self.open.records;
        let profile = self.profile(&plan, &[], length, terminal);
        let (mut records_done, mut dabs_done, mut count, mut group_end, mut recorded) = (0, 0, 0, None, 0);
        let Scratch { records: owned, dabs: input, shaped, group } = &mut self.scratch;
        group.clear();
        for slice in &self.slices {
            let last = slice.records.checked_sub(1).map(|offset| &self.records[records_done + offset]);
            let settled = match release {
                Release::All => true,
                Release::First(target) => count < target,
                Release::Settled => plan.settled(last, length),
            };
            let over = !settled && release == Release::Settled && {
                let remaining = held - records_done;
                remaining > MAX_HELD_CONTACTS
                    || remaining * std::mem::size_of::<ContactRecord>()
                        + (self.dabs.len() - dabs_done) * std::mem::size_of::<Dab>()
                        > MAX_HELD_BYTES
                    || self.pages.0.len() > plan.pages
            };
            if !settled && !over {
                break;
            }
            self.forced |= !settled;
            owned.clear();
            owned.extend(self.records.range(records_done..records_done + slice.records));
            input.clear();
            input.extend(self.dabs.range(dabs_done..dabs_done + slice.dabs));
            for dab in input.iter() {
                self.pages.remove(dab);
            }
            shaped.clear();
            generator.shape(brush, &profile, owned, self.previous, input, false, SAVED, shaped);
            self.previous = owned.last().copied().zip(shaped.last().copied()).or(self.previous);
            if self.released + recorded < self.cuts.len() {
                debug_assert_eq!(self.cuts[self.released + recorded], slice.end, "a recorded group is released whole");
                emit(shaped);
                recorded += 1;
            } else {
                group.extend_from_slice(shaped);
                group_end = Some(slice.end);
            }
            records_done += slice.records;
            dabs_done += slice.dabs;
            count += 1;
        }
        if let Some(end) = group_end {
            emit(group);
            self.cuts.push(end);
            recorded += 1;
        }
        self.slices.drain(..count);
        let remaining = self.records.len() - records_done;
        self.outline.drain(self.records.drain(..records_done), remaining);
        self.dabs.drain(..dabs_done);
        self.released += recorded;
    }

    pub(crate) fn present(&self, tip: &mut DabGenerator, brush: &BrushSnapshot, scale: f32, start: usize, output: &mut Vec<Dab>) {
        let (Some(plan), Some((_, mapping))) = (tip.taper, size_mapping(brush)) else {
            return;
        };
        let tail = std::mem::take(&mut tip.records);
        let chain = self.chain(&tail);
        if chain.len() == 0 {
            return Self::finish_present(output, start, 0, Vec::new(), self.previous, Vec::new());
        }
        let (held, sealed_dabs) = (chain.sealed, self.dabs.len() - self.open.dabs);
        let recorded = tail.iter().map(|record| usize::from(record.copies)).sum::<usize>();
        let firsts: Vec<usize> = if self.outline.multiple == 0 && recorded == tail.len() {
            Vec::new()
        } else {
            (0..chain.len())
                .scan(0, |dabs, index| {
                    let first = *dabs;
                    *dabs += usize::from(chain.get(index).copies);
                    Some(first)
                })
                .collect()
        };
        let first = |index: usize| firsts.get(index).copied().unwrap_or(index);
        let dab = |at: usize| if at < sealed_dabs { self.dabs[at] } else { output[start + at - sealed_dabs] };
        let profile = self.profile(&plan, &tail, tip.modeled_length(), tip.terminal_peak());
        let base = self.outline.staged - self.records.len() as u64;
        let mut kept: Vec<usize> = self.outline.kept.iter().map(|sequence| (sequence - base) as usize).filter(|index| *index < held).collect();
        kept.extend([0, held.saturating_sub(1)].into_iter().filter(|_| held > 0));
        kept.extend(held..chain.len());
        for zone in [0, 1].into_iter().filter(|zone| profile.zones[*zone] > 0.) {
            kept.extend(samples().map(|progress| profile.distance(zone, progress)).filter(|target| target.is_finite()).map(|target| chain.nearest(target)));
        }
        let varies = varies(brush);
        if varies {
            kept.extend(0..chain.after(profile.distance(0, 1.)));
            kept.extend(chain.after(profile.distance(1, 1.))..chain.len());
        }
        kept.sort_unstable();
        kept.dedup();
        if !varies {
            let estimates: Vec<_> = kept
                .iter()
                .map(|&index| {
                    let (current, original) = (chain.get(index), dab(first(index)));
                    let jitter = size_jitter(brush, random_variant(current.copy_seed));
                    let transfer = profile.transfer(current.pose.stroke_distance, current.peak);
                    transfer.map_or(original, |transfer| estimated(brush, mapping, &original, jitter, current.pose, Some(transfer)))
                })
                .collect();
            kept = selection(&estimates, scale, |_| THIN_SHARE).into_iter().map(|position| kept[position]).collect();
        }
        let resolution = Resolution { share: 0.04, minimum: 0.5 / scale.max(f32::EPSILON), preview: Some(scale.max(f32::EPSILON)) };
        let contacts = |indices: &[usize]| -> (Vec<_>, Vec<_>) {
            let dabs = indices.iter().flat_map(|&index| (first(index)..first(index) + usize::from(chain.get(index).copies)).map(dab));
            (indices.iter().map(|&index| chain.get(index)).collect(), dabs.collect())
        };
        let split = kept.partition_point(|index| *index < held);
        let ((held_records, held_dabs), (tail_records, tail_dabs)) = (contacts(&kept[..split]), contacts(&kept[split..]));
        let mut shaped = Vec::new();
        tip.shape(brush, &profile, &held_records, self.previous, &held_dabs, false, resolution, &mut shaped);
        let joint = held_records.last().copied().zip(shaped.last().copied()).or(self.previous);
        let mut body = Vec::new();
        tip.shape(brush, &profile, &tail_records, joint, &tail_dabs, true, resolution, &mut body);
        Self::finish_present(output, start, recorded, body, joint, shaped);
    }

    fn finish_present(output: &mut Vec<Dab>, start: usize, recorded: usize, body: Vec<Dab>, joint: Option<(ContactRecord, Dab)>, held: Vec<Dab>) {
        let shaped = body.len();
        output.splice(start..start + recorded, body);
        if let Some(last) = output[start..start + shaped].last().copied().or(joint.map(|(_, dab)| dab)) {
            for dab in &mut output[start + shaped..] {
                *dab = Dab {
                    center: dab.center,
                    motion: dab.motion,
                    previous: [last.radii[0], last.radii[1], last.rotation[0], last.rotation[1]],
                    previous_contact: last.contact,
                    ..last
                };
            }
        }
        output.splice(start..start, held);
    }
}

fn samples() -> impl Iterator<Item = f32> {
    let geometric = std::iter::successors(Some(FIRST_SAMPLE), |progress| Some(progress * SAMPLE_RATIO)).take_while(|progress| *progress < 1.);
    [0., 0.5, 1.].into_iter().chain(geometric)
}

fn varies(brush: &BrushSnapshot) -> bool {
    brush.shape.size_jitter != 0.
        || brush.path.jitter_along != 0.
        || brush.path.jitter_across != 0.
        || brush.mappings.iter().any(|mapping| matches!(mapping.target, BrushTarget::ScatterAlong | BrushTarget::ScatterAcross))
}

fn redundant(from: &Dab, to: &Dab, dab: &Dab, scale: f32, share: f32) -> bool {
    let along = [to.center.x - from.center.x, to.center.y - from.center.y];
    let length = along[0] * along[0] + along[1] * along[1];
    let t = if length > 0. {
        (((dab.center.x - from.center.x) * along[0] + (dab.center.y - from.center.y) * along[1]) / length).clamp(0., 1.)
    } else {
        0.
    };
    let mix = |a: f32, b: f32| a + (b - a) * t;
    let tolerance = (0.5 / scale.max(f32::EPSILON)).max(share * dab.radii[0].min(dab.radii[1]));
    let offset = (dab.center.x - mix(from.center.x, to.center.x)).hypot(dab.center.y - mix(from.center.y, to.center.y));
    offset + (dab.radii[0] - mix(from.radii[0], to.radii[0])).abs().max((dab.radii[1] - mix(from.radii[1], to.radii[1])).abs())
        <= tolerance
        && (dab.contact[0] - mix(from.contact[0], to.contact[0])).abs() <= 0.02
        && (dab.rotation[0] - mix(from.rotation[0], to.rotation[0])).abs().max((dab.rotation[1] - mix(from.rotation[1], to.rotation[1])).abs()) <= 0.02
}

pub(super) fn selection(dabs: &[Dab], scale: f32, share: impl Fn(&Dab) -> f32) -> Vec<usize> {
    let anchored = |index: usize| index > 0 && dabs[index].contact[2].floor() != dabs[index - 1].contact[2].floor();
    let mut kept = Vec::with_capacity(dabs.len().min(64));
    let mut anchor = 0;
    while anchor < dabs.len() {
        kept.push(anchor);
        let mut next = anchor + 1;
        while next + 1 < dabs.len()
            && next + 1 - anchor <= PREVIEW_RUN
            && !anchored(next)
            && (anchor + 1..=next).all(|index| redundant(&dabs[anchor], &dabs[next + 1], &dabs[index], scale, share(&dabs[index])))
        {
            next += 1;
        }
        anchor = next;
    }
    kept
}

fn estimated(brush: &BrushSnapshot, mapping: &BrushMapping, template: &Dab, jitter: f32, pose: DynamicPoint, transfer: Option<Transfer>) -> Dab {
    let pressure = pose.point.pressure;
    let ratio = mapped_width(brush, mapping, pressure, transfer) * jitter / (2. * template.radii[0]).max(f32::EPSILON);
    let mut contact = template.contact;
    contact[0] = transfer.map_or(pressure, |transfer| transfer.apply(pressure, |pressure| pressure));
    contact[2] = pose.stroke_distance / brush.diameter.max(0.01);
    Dab { radii: template.radii.map(|radius| radius * ratio), contact, ..*template }
}

impl DabGenerator {
    pub(crate) fn begin_taper(&mut self, brush: &BrushSnapshot) -> bool {
        self.taper = TaperPlan::new(brush);
        self.taper.is_some()
    }

    fn modeled_length(&self) -> f32 {
        self.last.map_or(0., |last| last.stroke_distance)
    }

    fn terminal_peak(&self) -> f32 {
        self.last.map_or(self.interval_peak, |last| self.interval_peak.max(last.point.pressure))
    }

    pub(crate) fn shape_final(&mut self, brush: &BrushSnapshot, dabs: &mut Vec<Dab>, start: usize) -> bool {
        let Some(plan) = self.taper else {
            return false;
        };
        let records = std::mem::take(&mut self.records);
        let length = self.modeled_length();
        let profile = HeldStroke::default().profile(&plan, &records, length, self.terminal_peak());
        let mut shaped = Vec::new();
        if !records.iter().any(|record| profile.transfer(record.pose.stroke_distance, record.peak).is_some())
            || !self.shape(brush, &profile, &records, None, &dabs[start..], false, SAVED, &mut shaped)
        {
            return false;
        }
        dabs.splice(start.., shaped);
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn shape(
        &self,
        brush: &BrushSnapshot,
        profile: &Profile,
        records: &[ContactRecord],
        mut previous: Option<(ContactRecord, Dab)>,
        input: &[Dab],
        anchored: bool,
        resolution: Resolution,
        output: &mut Vec<Dab>,
    ) -> bool {
        if records.iter().map(|record| usize::from(record.copies)).sum::<usize>() != input.len() {
            debug_assert!(false, "every tapered dab belongs to one record");
            return false;
        }
        let thinning = resolution.preview.zip(size_mapping(brush).map(|(_, mapping)| mapping));
        let mut linked = previous.map(|(_, dab)| dab);
        let mut push = |mut dab: Dab, output: &mut Vec<Dab>| {
            connect_contact(linked, &mut dab);
            linked = Some(dab);
            output.push(dab);
            dab
        };
        let mut input = input.iter();
        for record in records {
            let evaluated = profile.transfer(record.pose.stroke_distance, record.peak).map(|transfer| {
                self.evaluate_variant(&mut Evaluation::default(), record.pose, brush, record.variant, record.index, Some(transfer))
            });
            let mut seed = record.copy_seed;
            for (copy, original) in input.by_ref().take(record.copies.into()).enumerate() {
                seed = random_variant(seed);
                let mut dab = evaluated.as_ref().map_or(*original, |evaluated| self.copy_contact(evaluated, brush, seed));
                if anchored {
                    dab.center = original.center;
                }
                if copy == 0 && let Some((before, last)) = previous {
                    let (from, to) = (before.pose.stroke_distance, record.pose.stroke_distance);
                    let width_at = |t: f32| {
                        let pose = between(before.pose, record.pose, t);
                        let transfer = profile.transfer(pose.stroke_distance, before.peak.max(pose.point.pressure));
                        self.evaluate_variant(&mut Evaluation::default(), pose, brush, record.variant, record.index, transfer).dab.radii[0] * 2.
                    };
                    let widths = [last.radii[0] * 2., dab.radii[0] * 2.];
                    let tolerance = (resolution.share * widths[0].max(widths[1])).max(resolution.minimum);
                    let inserted = profile.insertions(from, to, widths, tolerance, width_at);
                    let inserted = match thinning {
                        Some((scale, mapping)) => {
                            let jitter = size_jitter(brush, random_variant(record.copy_seed));
                            let candidate = |t: f32| {
                                let pose = between(before.pose, record.pose, t);
                                let transfer = profile.transfer(pose.stroke_distance, before.peak.max(pose.point.pressure));
                                let centre = Point { x: last.center.x + (dab.center.x - last.center.x) * t, y: last.center.y + (dab.center.y - last.center.y) * t };
                                Dab { center: centre, ..estimated(brush, mapping, &dab, jitter, pose, transfer) }
                            };
                            let candidates: Vec<_> =
                                std::iter::once(last).chain(inserted.iter().map(|&t| candidate(t))).chain(std::iter::once(dab)).collect();
                            selection(&candidates, scale, |_| 0.04).into_iter().filter(|&index| index > 0 && index <= inserted.len()).map(|index| inserted[index - 1]).collect()
                        }
                        None => inserted,
                    };
                    for t in inserted {
                        let pose = between(before.pose, record.pose, t);
                        let transfer = profile.transfer(pose.stroke_distance, before.peak.max(pose.point.pressure));
                        let evaluated = self.evaluate_variant(&mut Evaluation::default(), pose, brush, record.variant, record.index, transfer);
                        let mut inserted = self.copy_contact(&evaluated, brush, random_variant(record.copy_seed));
                        if anchored {
                            inserted.center = Point {
                                x: last.center.x + (dab.center.x - last.center.x) * t,
                                y: last.center.y + (dab.center.y - last.center.y) * t,
                            };
                        }
                        push(inserted, output);
                    }
                }
                previous = Some((*record, push(dab, output)));
            }
        }
        true
    }
}
