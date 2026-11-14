use crate::{FluentArgs, Localizer, MessageId};
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Chip {
    pub label: Arc<str>,
    pub warning: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Tip {
    pub text: String,
    pub lines: Vec<TipLine>,
    pub figures: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TipLine {
    pub label: String,
    pub value: String,
}

impl Tip {
    pub fn text(text: impl Into<String>) -> Self { Self { text: text.into(), ..Default::default() } }
    pub fn figures(text: impl Into<String>) -> Self { Self { text: text.into(), figures: true, ..Default::default() } }
    pub fn line(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.lines.push(TipLine { label: label.into(), value: value.into() });
        self
    }
    pub fn is_empty(&self) -> bool { self.text.is_empty() && self.lines.is_empty() }
    pub fn plain(&self) -> String {
        std::iter::once(self.text.clone()).filter(|text| !text.is_empty())
            .chain(self.lines.iter().map(|line| format!("{}: {}", line.label, line.value)))
            .collect::<Vec<_>>().join("\n")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MeterRow {
    pub label: String,
    pub chip: Option<Chip>,
    pub value: String,
    pub warning: bool,
    pub meter: Option<Meter>,
    pub tip: Tip,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MeterSection {
    pub title: String,
    pub note: String,
    pub chip: Option<Chip>,
    pub tip: Tip,
    pub rows: Vec<MeterRow>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeterTone { Fill, Soft, Rest, Over }

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct MeterSegment {
    pub from: f32,
    pub to: f32,
    pub tone: MeterTone,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Meter {
    pub segments: Vec<MeterSegment>,
    pub limit: f32,
}

impl Meter {
    pub fn usage(capacity: u64, allowance: Option<u64>, held: u64, tracked: u64, whole: u64) -> Self {
        let scale = [capacity, allowance.unwrap_or(0), held, tracked, whole].into_iter().fold(1, u64::max);
        let allowance = allowance.unwrap_or(scale);
        let at = |value: u64| (value as f64 / scale as f64) as f32;
        let segments = [
            MeterSegment { from: 0., to: at(held.min(allowance)), tone: MeterTone::Fill },
            MeterSegment { from: at(allowance), to: at(held), tone: MeterTone::Over },
            MeterSegment { from: at(held), to: at(tracked), tone: MeterTone::Soft },
            MeterSegment { from: at(held.max(tracked)), to: at(whole), tone: MeterTone::Rest },
        ];
        Self { segments: segments.into_iter().filter(|segment| segment.to > segment.from).collect(), limit: at(allowance) }
    }
}

const MIB: f64 = 1_048_576.;

fn compact(value: f64) -> String {
    if value >= 10. { return crate::group(value.round() as u64); }
    let tenths = (value * 10.).round() / 10.;
    if tenths.fract() == 0. { format!("{tenths:.0}") } else { format!("{tenths:.1}") }
}

fn scaled(bytes: u64) -> (f64, bool) {
    let megabytes = bytes as f64 / MIB;
    if megabytes >= 1000. { (megabytes / 1024., true) } else { (megabytes, false) }
}

fn in_unit(value: String, gigabytes: bool, l: &Localizer) -> String {
    let mut args = FluentArgs::new();
    args.set("value", value);
    l.format(if gigabytes { MessageId::DOCUMENTS_GIGABYTES } else { MessageId::DOCUMENTS_MEGABYTES }, &args)
}

pub fn byte_size(bytes: u64, l: &Localizer) -> String {
    let (value, gigabytes) = scaled(bytes);
    in_unit(compact(value), gigabytes, l)
}

pub fn byte_share(part: u64, whole: u64, l: &Localizer) -> String {
    let ((part_value, part_gigabytes), (whole_value, whole_gigabytes)) = (scaled(part), scaled(whole));
    if part_gigabytes == whole_gigabytes {
        in_unit(format!("{} / {}", compact(part_value), compact(whole_value)), whole_gigabytes, l)
    } else {
        format!("{} / {}", byte_size(part, l), byte_size(whole, l))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1 << 20;

    #[test]
    fn byte_text_keeps_three_digits_and_names_each_unit_once() {
        let l = Localizer::shared(crate::UiLanguage::English);
        assert_eq!(byte_size(6144 * MB, &l), "6 GB");
        assert_eq!(byte_size(1430 * MB, &l), "1.4 GB");
        assert_eq!(byte_size(12_600 * MB, &l), "12 GB");
        assert_eq!(byte_size(1000 * MB, &l), "1 GB");
        assert_eq!(byte_size(999 * MB, &l), "999 MB");
        assert_eq!(byte_size(36 * MB, &l), "36 MB");
        assert_eq!(byte_size(MB / 4, &l), "0.3 MB");
        assert_eq!(byte_share(1430 * MB, 6144 * MB, &l), "1.4 / 6 GB");
        assert_eq!(byte_share(36 * MB, 200_482 * MB, &l), "36 MB / 196 GB");
        assert_eq!(byte_share(0, 6144 * MB, &l), "0 MB / 6 GB");
        assert_eq!(byte_share(350 * MB, 900 * MB, &l), "350 / 900 MB");
        assert_eq!(byte_share(1650 * MB, 1536 * MB, &l), "1.6 / 1.5 GB");
    }

    #[test]
    fn usage_meter_stacks_use_cache_and_the_rest_on_the_capacity() {
        let fits = Meter::usage(8000, Some(6000), 800, 1200, 2000);
        assert_eq!(fits.limit, 0.75);
        assert_eq!(fits.segments, [
            MeterSegment { from: 0., to: 0.1, tone: MeterTone::Fill },
            MeterSegment { from: 0.1, to: 0.15, tone: MeterTone::Soft },
            MeterSegment { from: 0.15, to: 0.25, tone: MeterTone::Rest },
        ]);
        let over = Meter::usage(2000, Some(1500), 1650, 1800, 1700);
        assert_eq!(over.segments.iter().map(|segment| (segment.tone, segment.to)).collect::<Vec<_>>(),
            [(MeterTone::Fill, 0.75), (MeterTone::Over, 0.825), (MeterTone::Soft, 0.9)]);
        let fallback = Meter::usage(1500, Some(1500), 600, 900, 0);
        assert_eq!((fallback.limit, fallback.segments.len()), (1., 2));
        let unbounded = Meter::usage(8000, None, 800, 1200, 2000);
        assert_eq!((unbounded.limit, unbounded.segments.iter().map(|segment| segment.tone).collect::<Vec<_>>()), (1., vec![MeterTone::Fill, MeterTone::Soft, MeterTone::Rest]));
        assert_eq!(Meter::usage(0, None, 300, 900, 0).segments.iter().map(|segment| (segment.tone, segment.to)).collect::<Vec<_>>(),
            [(MeterTone::Fill, 1. / 3.), (MeterTone::Soft, 1.)]);
        assert!(Meter::usage(0, None, 0, 0, 0).segments.is_empty());
    }

    #[test]
    fn plain_tip_reads_text_then_lines() {
        let tip = Tip::figures("Apple M2 Pro · Metal").line("In use", "700 MB");
        assert_eq!(tip.plain(), "Apple M2 Pro · Metal\nIn use: 700 MB");
        assert!(Tip::default().is_empty());
    }
}
