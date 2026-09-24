//! Small GPU probe summaries; source pixels are never read back for classification.
use layer_render::{TonalProbe, TonalSample};
pub(super) const STAT_WORDS: usize = 4498;
pub(super) fn sample(bytes: &[u8], probe: TonalProbe) -> Option<TonalSample> {
    if bytes.len() < STAT_WORDS * 4 {
        return None;
    }
    let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
    let count = (0..4434).map(word).sum::<u32>();
    if count == 0 {
        return None;
    }
    let stops = if probe.point {
        let (mut value, mut weight) = (0f64, 0f64);
        for i in 0..25 {
            let a = f64::from(f32::from_bits(word(4449 + i * 2)));
            value += f64::from(f32::from_bits(word(4448 + i * 2))) * a;
            weight += a;
        }
        let y = value / weight;
        let stop = if y > 0. {
            y.log2() as f32
        } else {
            layer_core::tonal::MIN_STOP
        };
        [stop; 2]
    } else {
        let quantile = |fraction: f64| {
            let target = (f64::from(count - 1) * fraction).floor() as u32;
            let mut sum = 0;
            for i in 0..4434 {
                sum += word(i);
                if sum > target {
                    return if i == 0 {
                        -149.
                    } else {
                        -149. + (i as f32 - 0.5) / 16.
                    };
                }
            }
            128.
        };
        [quantile(0.05), quantile(0.95)]
    };
    Some(TonalSample { stops, count })
}
