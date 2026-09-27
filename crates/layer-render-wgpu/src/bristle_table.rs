use super::{lattice_noise, periodic_value_noise};

pub(super) const HAIRS_ASSET: &str = "builtin:brush-grain/bristle-hairs-v1";
pub(super) const FIELD_ASSET: &str = "builtin:brush-grain/bristle-noise-v1";

const WIDTH: usize = 2048;
const LEVELS: usize = 10;
const GROUPS: usize = 40;
const STREAKS: usize = 128;
const HAIRS: usize = 512;
const GROOVES: usize = 20;
const SEED: u32 = 0x4841_4952;

pub(super) fn hairs() -> (u32, u32, Vec<u8>) {
    let unit = |index: usize, salt: u32| lattice_noise(index as u32, salt, SEED);
    let normal = |index: usize, salt: u32| {
        (unit(index, salt) + unit(index, salt + 1) + unit(index, salt + 2) - 1.5) * 2.
    };
    let smooth = |x: f32, cells: u32, salt: u32| {
        periodic_value_noise(x.rem_euclid(WIDTH as f32) / WIDTH as f32, 0.5, cells, 1, SEED ^ salt)
    };
    let mut load = vec![0f32; WIDTH];
    let mut tip = vec![0f32; WIDTH];
    let mut nearest = vec![f32::INFINITY; WIDTH];
    let mut deposit = |center: f32, sigma: f32, strength: f32, length: Option<f32>| {
        let reach = (sigma * 4.).ceil() as i32 + 1;
        for step in -reach..=reach {
            let x = (center.floor() as i32 + step).rem_euclid(WIDTH as i32) as usize;
            let distance = (center.floor() + step as f32 + 0.5 - center).abs();
            load[x] += strength * (-0.5 * (distance / sigma).powi(2)).exp();
            if let Some(length) = length && distance < nearest[x] {
                nearest[x] = distance;
                tip[x] = length;
            }
        }
    };
    let group = WIDTH as f32 / GROUPS as f32;
    for index in 0..GROUPS {
        let center = (index as f32 + 0.5 + (unit(index, 1) - 0.5) * 0.9) * group;
        let sigma = group * (0.2 + 0.25 * unit(index, 2));
        deposit(center, sigma, 0.5 * (0.4 * normal(index, 4)).exp(), Some(0.14 * (unit(index, 7) - 0.5)));
    }
    let streak = WIDTH as f32 / STREAKS as f32;
    for index in 0..STREAKS {
        let center = (index as f32 + 0.5 + (unit(index, 31) - 0.5) * 0.9) * streak;
        let sigma = 1.2 + 2.5 * unit(index, 32).powi(2);
        deposit(center, sigma, 0.55 * (0.45 * normal(index, 34)).exp(), None);
    }
    let hair = WIDTH as f32 / HAIRS as f32;
    for index in 0..HAIRS {
        let center = (index as f32 + 0.5 + (unit(index, 11) - 0.5) * 0.8) * hair;
        deposit(center, 0.5 + 0.7 * unit(index, 12), 0.2 * unit(index, 13), None);
    }
    for (x, value) in load.iter_mut().enumerate() {
        *value *= 0.7 + 0.6 * smooth(x as f32, 18, 3);
    }
    for index in 0..GROOVES {
        let center = unit(index, 21) * WIDTH as f32;
        let width = 1. + 2.5 * unit(index, 22);
        for (x, value) in load.iter_mut().enumerate() {
            let distance = ((x as f32 + 0.5 - center + WIDTH as f32 * 1.5).rem_euclid(WIDTH as f32)
                - WIDTH as f32 * 0.5)
                .abs();
            *value *= 1. - 0.7 * (-(distance / width).powi(2)).exp();
        }
    }
    let mean = load.iter().sum::<f32>() / WIDTH as f32;
    for (x, value) in load.iter_mut().enumerate() {
        *value *= 0.7 / mean.max(0.0001);
        tip[x] += 0.16 * (smooth(x as f32, 60, 8) - 0.5);
    }
    let mut pixels = vec![0u8; WIDTH * LEVELS * 2];
    let encode = |value: f32| (value.clamp(0., 1.) * 255.).round() as u8;
    for level in 0..LEVELS {
        let width = 1usize << level;
        let filtered_load = box_filter(&load, width);
        let filtered_tip = box_filter(&tip, width);
        for x in 0..WIDTH {
            pixels[level * WIDTH + x] = encode(filtered_load[x] / 2.);
            pixels[(LEVELS + level) * WIDTH + x] = encode(0.5 + filtered_tip[x] * 2.);
        }
    }
    (WIDTH as u32, (LEVELS * 2) as u32, pixels)
}

fn box_filter(values: &[f32], width: usize) -> Vec<f32> {
    if width <= 1 {
        return values.to_vec();
    }
    let count = values.len();
    let mut prefix = vec![0f64; count * 2 + 1];
    for index in 0..count * 2 {
        prefix[index + 1] = prefix[index] + f64::from(values[index % count]);
    }
    (0..count)
        .map(|x| {
            let start = (x + count - width / 2) % count;
            ((prefix[start + width] - prefix[start]) / width as f64) as f32
        })
        .collect()
}

pub(super) fn field() -> (u32, u32, Vec<u8>) {
    let mut pixels = Vec::with_capacity(512 * 512);
    for y in 0..512 {
        for x in 0..512 {
            let value = periodic_value_noise(
                (x as f32 + 0.5) / 512.,
                (y as f32 + 0.5) / 512.,
                64,
                64,
                0x4252_4953,
            );
            pixels.push((value * 255.).round() as u8);
        }
    }
    (512, 512, pixels)
}

#[cfg(test)]
mod tests {
    #[test]
    fn hair_table_levels_average_the_same_hairs() {
        let (width, height, pixels) = super::hairs();
        assert_eq!((width, height), (2048, 20));
        let mean = |row: usize| {
            pixels[row * 2048..(row + 1) * 2048].iter().map(|&v| f64::from(v)).sum::<f64>() / 2048.
        };
        for level in 1..10 {
            assert!((mean(level) - mean(0)).abs() < 2., "level {level} changed the mean load");
        }
        let fine = &pixels[..2048];
        let coarse = &pixels[9 * 2048..10 * 2048];
        let range = |row: &[u8]| row.iter().max().unwrap() - row.iter().min().unwrap();
        assert!(range(fine) > 3 * range(coarse), "wide levels must smooth individual hairs");
    }
}
