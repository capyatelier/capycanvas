use super::{connect_contact, random_variant, tip_diameter, DabGenerator, DynamicPoint, Evaluation};
use layer_core::{
    BrushAccumulation, BrushBlendMode, BrushCombine, BrushExecution, BrushMapping, BrushSensor, BrushSnapshot,
    BrushTarget, TaperMode, MAX_BRUSH_MAPPINGS,
};
use layer_render::Dab;
use std::collections::VecDeque;

const PROPORTIONAL_LIMIT_WIDTHS: f32 = 100.;
const MAX_HELD_CONTACTS: usize = 4096;
const MAX_HELD_BYTES: usize = 1 << 20;

#[derive(Clone, Copy, Debug)]
pub(crate) struct TaperPlan {
    proportional: bool,
    lengths: [f32; 2],
    exponent: f32,
    mapping: usize,
    pub(super) full_width: f32,
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
    level: f32,
    pub(super) pressure: f32,
}

impl Transfer {
    pub(super) fn apply(self, value: f32, reference: f32) -> f32 {
        value * self.level + reference * self.level * (1. - self.level)
    }
}

pub(super) fn mapping_output(mapping: &BrushMapping, input: f32, transfer: Option<Transfer>) -> f32 {
    let width = mapping.input_max - mapping.input_min;
    let normalize = |input: f32| if width.abs() > f32::EPSILON { (input - mapping.input_min) / width } else { 0.0 };
    let output = mapping.curve.sample(normalize(input));
    transfer
        .map_or(output, |transfer| transfer.apply(output, mapping.curve.sample(normalize(transfer.pressure))))
        .mul_add(mapping.output_scale, mapping.output_bias)
}

pub(super) fn combine(target: f32, contribution: f32, combine: BrushCombine) -> f32 {
    match combine {
        BrushCombine::Replace => contribution,
        BrushCombine::Add => target + contribution,
        BrushCombine::Multiply => target * contribution,
    }
}

fn mapped_width(brush: &BrushSnapshot, mapping: &BrushMapping, pressure: f32) -> f32 {
    tip_diameter(brush, combine(brush.diameter, mapping_output(mapping, pressure, None), mapping.combine))
}

fn full_width(brush: &BrushSnapshot, mapping: &BrushMapping) -> f32 {
    let finite = [0., 1.].into_iter().all(|pressure| {
        let output = mapping_output(mapping, pressure, None);
        output.is_finite() && combine(brush.diameter, output, mapping.combine).is_finite()
    });
    if finite { mapped_width(brush, mapping, 1.) } else { tip_diameter(brush, brush.bounds.maximum_size) }
}

pub(crate) fn tapers(brush: &BrushSnapshot) -> bool {
    TaperPlan::new(brush).is_some()
}

impl TaperPlan {
    pub(crate) fn new(brush: &BrushSnapshot) -> Option<Self> {
        let lengths = brush.taper.lengths()?;
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
        Some(Self {
            proportional: brush.taper.mode == TaperMode::Proportional,
            lengths,
            exponent: 0.25 + 0.5 * brush.taper.pointiness,
            mapping: index,
            full_width: full_width(brush, size),
        })
    }

    fn zone(&self, end: usize, length: f32, brush: &BrushSnapshot, pressure: f32) -> f32 {
        let zone = if self.proportional {
            (self.lengths[end] * length).min(PROPORTIONAL_LIMIT_WIDTHS * self.full_width)
        } else {
            self.lengths[end] * mapped_width(brush, &brush.mappings[self.mapping], pressure)
        };
        zone.min(length / 3.)
    }

    fn start_limit(&self) -> f32 {
        if self.proportional { PROPORTIONAL_LIMIT_WIDTHS * self.full_width } else { self.lengths[0] * self.full_width }
    }

    fn settled(&self, records: &[ContactRecord], length: f32) -> bool {
        let (Some(first), Some(last)) = (records.first(), records.last()) else {
            return true;
        };
        let start = self.lengths[0] <= 0.
            || first.pose.stroke_distance >= self.start_limit()
            || if self.proportional {
                self.lengths[0].min(1. / 3.) * length >= self.start_limit()
            } else {
                length >= 3. * self.start_limit()
            };
        let end = self.lengths[1] <= 0. || {
            let zone = if self.proportional {
                (self.lengths[1] * length).min(PROPORTIONAL_LIMIT_WIDTHS * self.full_width).min(length / 3.)
            } else {
                self.lengths[1] * self.full_width
            };
            last.pose.stroke_distance <= length - zone
        };
        start && end
    }

    fn reach_after(&self, brush: &BrushSnapshot, records: &[ContactRecord], reach: f32, length: f32) -> f32 {
        if self.proportional {
            return reach;
        }
        records.iter().fold(reach, |reach, record| {
            let zone = self.zone(0, length, brush, record.peak);
            if zone > 0. { reach.max(record.pose.stroke_distance / zone) } else { reach }
        })
    }

    fn transfer(&self, reach: f32, pressure: f32) -> Transfer {
        let smooth = reach * reach * (3. - 2. * reach);
        Transfer { mapping: self.mapping, level: smooth.powf(self.exponent), pressure }
    }

    pub(super) fn transfers(
        &self,
        brush: &BrushSnapshot,
        records: &[ContactRecord],
        reach: f32,
        length: f32,
        terminal: f32,
    ) -> Vec<Option<Transfer>> {
        let mut transfers = vec![None; records.len()];
        let mut reach = reach;
        for (record, transfer) in records.iter().zip(&mut transfers) {
            if reach >= 1. {
                break;
            }
            let zone = self.zone(0, length, brush, record.peak);
            if !(zone > 0.) {
                break;
            }
            let progress = record.pose.stroke_distance / zone;
            reach = if self.proportional { progress } else { reach.max(progress) };
            if reach >= 1. {
                break;
            }
            *transfer = Some(self.transfer(reach, record.peak));
        }
        let mut reach = 0f32;
        let mut after = terminal;
        for (record, transfer) in records.iter().zip(&mut transfers).rev() {
            let peak = after.max(record.pose.point.pressure);
            after = after.max(record.interval);
            let zone = self.zone(1, length, brush, peak);
            if !(zone > 0.) {
                break;
            }
            let progress = (length - record.pose.stroke_distance) / zone;
            reach = if self.proportional { progress } else { reach.max(progress) };
            if reach >= 1. {
                break;
            }
            let end = self.transfer(reach, peak);
            if transfer.is_none_or(|start| end.level < start.level) {
                *transfer = Some(end);
            }
        }
        transfers
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
pub(crate) struct HeldStroke {
    cuts: Vec<u32>,
    sealed: usize,
    sealed_end: u32,
    released: usize,
    slices: VecDeque<HeldSlice>,
    records: VecDeque<ContactRecord>,
    dabs: VecDeque<Dab>,
    open: HeldSlice,
    reach: f32,
    previous: Option<Dab>,
    forced: bool,
}

impl HeldStroke {
    pub(crate) fn with_cuts(cuts: &[u32]) -> Self {
        Self { cuts: cuts.to_vec(), ..Self::default() }
    }

    pub(crate) fn cuts(&self) -> &[u32] {
        &self.cuts[..self.sealed]
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
        self.open.records += generator.records.len();
        self.records.extend(generator.records.drain(..));
        let before = self.dabs.len();
        self.dabs.extend(dabs);
        self.open.dabs += self.dabs.len() - before;
        self.open.end = self.open.end.max(end);
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
        match self.cuts.get(self.sealed) {
            Some(&cut) => debug_assert_eq!(cut, end, "a regenerated slice ends at its recorded cut"),
            None => self.cuts.push(end),
        }
        self.sealed += 1;
        self.sealed_end = end;
        self.slices.push_back(HeldSlice { end, ..self.open });
        self.open = HeldSlice::default();
    }

    fn over_ceiling(&self) -> bool {
        self.records.len() > MAX_HELD_CONTACTS
            || self.records.len() * std::mem::size_of::<ContactRecord>() + self.dabs.len() * std::mem::size_of::<Dab>()
                > MAX_HELD_BYTES
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
        let held = self.slices.iter().map(|slice| slice.records).sum::<usize>();
        let records = &self.records.make_contiguous()[..held];
        let transfers = plan.transfers(brush, records, self.reach, length, terminal);
        let (mut records_done, mut dabs_done, mut count) = (0, 0, 0);
        let mut shaped = Vec::new();
        for slice in &self.slices {
            let owned = &records[records_done..records_done + slice.records];
            let settled = match release {
                Release::All => true,
                Release::First(target) => count < target,
                Release::Settled => plan.settled(owned, length),
            };
            let over = release == Release::Settled && {
                let remaining = records.len() - records_done;
                remaining > MAX_HELD_CONTACTS
                    || remaining * std::mem::size_of::<ContactRecord>()
                        + (self.dabs.len() - dabs_done) * std::mem::size_of::<Dab>()
                        > MAX_HELD_BYTES
            };
            if !settled && !over {
                break;
            }
            self.forced |= !settled;
            shaped.clear();
            shaped.extend(self.dabs.range(dabs_done..dabs_done + slice.dabs).copied());
            generator.shape(brush, owned, &transfers[records_done..records_done + slice.records], self.previous, &mut shaped);
            self.previous = shaped.last().copied().or(self.previous);
            emit(&shaped);
            self.reach = plan.reach_after(brush, owned, self.reach, length);
            records_done += slice.records;
            dabs_done += slice.dabs;
            count += 1;
        }
        self.slices.drain(..count);
        self.records.drain(..records_done);
        self.dabs.drain(..dabs_done);
        self.released += count;
        debug_assert!(release != Release::Settled || !self.over_ceiling());
    }
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

    pub(crate) fn shape_final(&mut self, brush: &BrushSnapshot, dabs: &mut [Dab]) -> bool {
        let Some(plan) = self.taper else {
            return false;
        };
        let records = std::mem::take(&mut self.records);
        let transfers = plan.transfers(brush, &records, 0., self.modeled_length(), self.terminal_peak());
        self.shape(brush, &records, &transfers, None, dabs)
    }

    fn shape(
        &self,
        brush: &BrushSnapshot,
        records: &[ContactRecord],
        transfers: &[Option<Transfer>],
        mut previous: Option<Dab>,
        dabs: &mut [Dab],
    ) -> bool {
        if records.iter().map(|record| usize::from(record.copies)).sum::<usize>() != dabs.len() {
            debug_assert!(false, "every tapered dab belongs to one record");
            return false;
        }
        let mut dabs = dabs.iter_mut();
        for (record, transfer) in records.iter().zip(transfers) {
            let evaluated = transfer.map(|transfer| {
                self.evaluate_variant(&mut Evaluation::default(), record.pose, brush, record.variant, record.index, Some(transfer))
            });
            let mut seed = record.copy_seed;
            for dab in dabs.by_ref().take(record.copies.into()) {
                seed = random_variant(seed);
                if let Some(evaluated) = &evaluated {
                    *dab = self.copy_contact(evaluated, brush, seed);
                }
                connect_contact(previous, dab);
                previous = Some(*dab);
            }
        }
        true
    }
}
