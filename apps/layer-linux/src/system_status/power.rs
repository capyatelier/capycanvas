use layer_ui::{Battery, BatteryCharge, BatteryReading};
use std::{fs, path::Path};

pub(super) fn read(root: &Path) -> Option<Battery> {
    let mut batteries = Vec::new();
    for entry in fs::read_dir(root).ok()? {
        let path = entry.ok()?.path().join("uevent");
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        if field(&text, "TYPE") != Some("Battery") || field(&text, "SCOPE") == Some("Device")
            || field(&text, "PRESENT") == Some("0") {
            continue;
        }
        if !matches!(field(&text, "PRESENT"), None | Some("1"))
            || !matches!(field(&text, "SCOPE"), None | Some("System" | "Unknown")) {
            return None;
        }
        batteries.push(reading(&text)?);
    }
    Battery::from_readings(&batteries)
}

fn field<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines().filter_map(|line| line.strip_prefix("POWER_SUPPLY_")?.split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value.trim()))
}

fn number(text: &str, name: &str) -> Option<f64> {
    field(text, name)?.parse::<u64>().ok().map(|v| v as f64)
}

fn positive(text: &str, name: &str) -> Option<f64> {
    number(text, name).filter(|v| *v > 0.)
}

fn capacity(text: &str, unit: &str) -> Option<(f64, Option<f64>)> {
    let empty = field(text, &format!("{unit}_EMPTY")).map_or(Some(0.), |v| v.parse::<u64>().ok().map(|v| v as f64))?;
    let usable = positive(text, &format!("{unit}_FULL"))? - empty;
    if usable <= 0. { return None; }
    let fraction = number(text, &format!("{unit}_NOW")).map(|now| ((now - empty) / usable).clamp(0., 1.));
    Some((usable, fraction))
}

fn reading(text: &str) -> Option<BatteryReading> {
    let energy = capacity(text, "ENERGY");
    let charge = capacity(text, "CHARGE");
    let fraction = number(text, "CAPACITY").filter(|v| *v <= 100.).map(|v| v / 100.)
        .or_else(|| energy?.1).or_else(|| charge?.1)?;
    let full_wh = energy.map(|v| v.0 / 1_000_000.).or_else(|| {
        let voltage = positive(text, "VOLTAGE_MIN_DESIGN").or_else(|| positive(text, "VOLTAGE_NOW"))?;
        Some(charge?.0 * voltage / 1_000_000_000_000.)
    });
    Some(BatteryReading {
        fraction,
        full_wh,
        charge: match field(text, "STATUS") {
            Some("Charging") => BatteryCharge::Charging,
            Some("Discharging") => BatteryCharge::Discharging,
            Some("Not charging") => BatteryCharge::Idle,
            Some("Full") => BatteryCharge::Full,
            _ => BatteryCharge::Unknown,
        },
        low: matches!(field(text, "CAPACITY_LEVEL"), Some("Low" | "Critical")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_energy_and_charge_units_are_converted_to_watt_hours() {
        let energy = reading("POWER_SUPPLY_ENERGY_NOW=12000000\nPOWER_SUPPLY_ENERGY_FULL=60000000\nPOWER_SUPPLY_STATUS=Discharging\n").unwrap();
        let charge = reading("POWER_SUPPLY_CHARGE_NOW=2000000\nPOWER_SUPPLY_CHARGE_FULL=2000000\nPOWER_SUPPLY_VOLTAGE_MIN_DESIGN=10000000\nPOWER_SUPPLY_STATUS=Full\n").unwrap();
        assert_eq!((energy.fraction, energy.full_wh), (0.2, Some(60.)));
        assert_eq!((charge.fraction, charge.full_wh), (1., Some(20.)));
        assert_eq!(Battery::from_readings(&[energy, charge]), Some(Battery { percent: 40, charging: false, low: false }));
        assert!(reading("POWER_SUPPLY_CAPACITY=101\n").is_none());
        assert!(reading("POWER_SUPPLY_CAPACITY=-1\n").is_none());
        assert!(reading("POWER_SUPPLY_ENERGY_NOW=20\nPOWER_SUPPLY_ENERGY_FULL=0\n").is_none());
    }

    #[test]
    fn kernel_capacity_and_low_warning_are_preserved() {
        let value = reading("POWER_SUPPLY_CAPACITY=40\nPOWER_SUPPLY_STATUS=Charging\nPOWER_SUPPLY_CAPACITY_LEVEL=Critical\n").unwrap();
        assert_eq!(Battery::from_readings(&[value]), Some(Battery { percent: 40, charging: true, low: true }));
        let text = "POWER_SUPPLY_CAPACITY=0\nPOWER_SUPPLY_STATUS=Not charging\n";
        assert_eq!(Battery::from_readings(&[reading(text).unwrap()]), Some(Battery { percent: 0, charging: false, low: true }));
    }

    #[test]
    fn nonzero_empty_thresholds_define_usable_capacity() {
        for unit in ["ENERGY", "CHARGE"] {
            for (now, expected) in [(6_000_000, 0), (12_000_000, 0), (36_000_000, 50), (60_000_000, 100), (66_000_000, 100)] {
                let text = format!("POWER_SUPPLY_{unit}_NOW={now}\nPOWER_SUPPLY_{unit}_EMPTY=12000000\nPOWER_SUPPLY_{unit}_FULL=60000000\nPOWER_SUPPLY_VOLTAGE_MIN_DESIGN=1000000\n");
                let value = reading(&text).unwrap();
                assert_eq!(value.full_wh, Some(48.));
                assert_eq!(Battery::from_readings(&[value]).unwrap().percent, expected);
            }
            for empty in ["60000000", "66000000", "bad"] {
                let text = format!("POWER_SUPPLY_{unit}_NOW=12000000\nPOWER_SUPPLY_{unit}_EMPTY={empty}\nPOWER_SUPPLY_{unit}_FULL=60000000\n");
                assert!(reading(&text).is_none());
            }
        }
    }

    #[test]
    fn live_sysfs_tree_tracks_removal_and_excludes_peripherals() {
        let root = std::env::temp_dir().join(format!("capy-power-{}", layer_core::PortableId::random()));
        let device = root.join("devices/battery");
        let class = root.join("class");
        fs::create_dir_all(&device).unwrap();
        fs::create_dir_all(&class).unwrap();
        fs::create_dir_all(class.join("mouse")).unwrap();
        fs::write(class.join("mouse/uevent"), "POWER_SUPPLY_TYPE=Battery\nPOWER_SUPPLY_SCOPE=Device\nPOWER_SUPPLY_CAPACITY=92\n").unwrap();
        fs::create_dir_all(class.join("AC")).unwrap();
        fs::write(class.join("AC/uevent"), "POWER_SUPPLY_TYPE=Mains\nPOWER_SUPPLY_CAPACITY=100\n").unwrap();
        std::os::unix::fs::symlink(&device, class.join("BAT0")).unwrap();
        let update = |text: &str| fs::write(device.join("uevent"), text).unwrap();
        update("POWER_SUPPLY_TYPE=Battery\nPOWER_SUPPLY_CAPACITY=73\nPOWER_SUPPLY_STATUS=Discharging\n");
        assert_eq!(read(&class), Some(Battery { percent: 73, charging: false, low: false }));
        update("POWER_SUPPLY_TYPE=Battery\nPOWER_SUPPLY_SCOPE=System\nPOWER_SUPPLY_PRESENT=1\nPOWER_SUPPLY_CAPACITY=84\nPOWER_SUPPLY_STATUS=Charging\n");
        assert_eq!(read(&class), Some(Battery { percent: 84, charging: true, low: false }));
        for suffix in ["POWER_SUPPLY_PRESENT=0", "POWER_SUPPLY_SCOPE=Device", "POWER_SUPPLY_PRESENT=bad"] {
            update(&format!("POWER_SUPPLY_TYPE=Battery\nPOWER_SUPPLY_CAPACITY=73\n{suffix}\n"));
            assert_eq!(read(&class), None);
        }
        update("POWER_SUPPLY_TYPE=Battery\nPOWER_SUPPLY_CAPACITY=bad\n");
        assert_eq!(read(&class), None);
        update("POWER_SUPPLY_TYPE=Mains\nPOWER_SUPPLY_ONLINE=1\n");
        assert_eq!(read(&class), None);
        fs::remove_dir_all(&device).unwrap();
        assert_eq!(read(&class), None);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(read(&class), None);
    }
}
