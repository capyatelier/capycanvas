#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub percent: u32,
    pub charging: bool,
    pub low: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryCharge {
    Unknown,
    Charging,
    Discharging,
    Idle,
    Full,
}

#[derive(Clone, Copy, Debug)]
pub struct BatteryReading {
    pub fraction: f64,
    pub full_wh: Option<f64>,
    pub charge: BatteryCharge,
    pub low: bool,
}

impl Battery {
    pub fn from_readings(readings: &[BatteryReading]) -> Option<Self> {
        if readings.is_empty() || readings.iter().any(|r| {
            !r.fraction.is_finite() || !(0. ..=1.).contains(&r.fraction)
                || r.full_wh.is_some_and(|v| !v.is_finite() || v <= 0.)
        }) {
            return None;
        }
        let fraction = if readings.len() == 1 {
            readings[0].fraction
        } else {
            let capacities: Vec<_> = readings.iter().map(|r| r.full_wh).collect::<Option<_>>()?;
            let largest = capacities.iter().copied().fold(0., f64::max);
            let total: f64 = capacities.iter().map(|v| v / largest).sum();
            readings.iter().zip(capacities).map(|(r, v)| r.fraction * (v / largest) / total).sum()
        };
        let percent = (fraction * 100.).round() as u32;
        Some(Self {
            percent,
            charging: readings.iter().any(|r| r.charge == BatteryCharge::Charging)
                || readings.iter().all(|r| r.charge == BatteryCharge::Full),
            low: percent <= 15 || readings.iter().all(|r| r.low),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(fraction: f64, full_wh: Option<f64>, charge: BatteryCharge) -> BatteryReading {
        BatteryReading { fraction, full_wh, charge, low: false }
    }

    #[test]
    fn percentage_warning_and_full_charge() {
        for (fraction, percent, low) in [(0., 0, true), (0.15, 15, true), (0.16, 16, false), (0.734, 73, false), (1., 100, false)] {
            assert_eq!(Battery::from_readings(&[reading(fraction, None, BatteryCharge::Discharging)]),
                Some(Battery { percent, charging: false, low }));
        }
        let mut native = reading(0.4, None, BatteryCharge::Full);
        native.low = true;
        assert_eq!(Battery::from_readings(&[native]), Some(Battery { percent: 40, charging: true, low: true }));
    }

    #[test]
    fn multiple_batteries_use_energy_capacity() {
        let mut readings = [reading(0.2, Some(60.), BatteryCharge::Discharging), reading(1., Some(20.), BatteryCharge::Full)];
        assert_eq!(Battery::from_readings(&readings), Some(Battery { percent: 40, charging: false, low: false }));
        readings[0].charge = BatteryCharge::Charging;
        assert!(Battery::from_readings(&readings).unwrap().charging);
        readings[0].low = true;
        assert!(!Battery::from_readings(&readings).unwrap().low);
        readings[0].full_wh = None;
        assert_eq!(Battery::from_readings(&readings), None);
    }

    #[test]
    fn unknown_or_invalid_readings_cannot_be_shown() {
        assert_eq!(Battery::from_readings(&[]), None);
        for fraction in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            assert_eq!(Battery::from_readings(&[reading(fraction, None, BatteryCharge::Unknown)]), None);
        }
        for full in [f64::NAN, f64::INFINITY, 0., -1.] {
            assert_eq!(Battery::from_readings(&[reading(0.5, Some(full), BatteryCharge::Unknown)]), None);
        }
        let large = reading(0.5, Some(f64::MAX), BatteryCharge::Idle);
        assert_eq!(Battery::from_readings(&[large, large]).unwrap().percent, 50);
    }
}
