//! Native tablet metadata. Never infer a screenless device from its pen axes:
//! display tablets and screenless tablets can report identical samples.
use adw::prelude::*;
use gtk::gdk;
use layer_engine::SampleFlags;
use std::{cell::RefCell, collections::HashMap, path::Path};

#[derive(Default)]
pub(super) struct TabletDevices(RefCell<HashMap<gdk::Device, SampleFlags>>);

impl TabletDevices {
    pub fn flags(&self, device: &gdk::Device, tool: Option<&gdk::DeviceTool>) -> SampleFlags {
        *self
            .0
            .borrow_mut()
            .entry(device.clone())
            .or_insert_with(|| {
                // Wayland events carry the logical pointer, which has no USB
                // IDs. The physical tablet owns the active tool and metadata.
                let physical = tool.and_then(|tool| {
                    device
                        .seat()
                        .devices(gdk::SeatCapabilities::TABLET_STYLUS)
                        .into_iter()
                        .find(|candidate| candidate.device_tool().as_ref() == Some(tool))
                });
                let device = physical.as_ref().unwrap_or(device);
                tablet_flags(
                    Path::new("/sys/class/input"),
                    device.vendor_id().as_deref(),
                    device.product_id().as_deref(),
                    device.name().as_str(),
                )
            })
    }
}

fn bit(bitmap: &str, index: usize) -> bool {
    let width = usize::BITS as usize;
    bitmap
        .split_whitespace()
        .rev()
        .nth(index / width)
        .and_then(|word| usize::from_str_radix(word, 16).ok())
        .is_some_and(|word| word & (1 << (index % width)) != 0)
}

fn tablet_flags(
    root: &Path,
    vendor: Option<&str>,
    product: Option<&str>,
    name: &str,
) -> SampleFlags {
    let hex = |value: &str| u16::from_str_radix(value.trim().trim_start_matches("0x"), 16).ok();
    let ids = vendor.and_then(hex).zip(product.and_then(hex));
    let mut matches = Vec::new();
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        if !entry.file_name().to_string_lossy().starts_with("input") {
            continue;
        }
        let path = entry.path();
        let read = |file| std::fs::read_to_string(path.join(file)).unwrap_or_default();
        let same_name = read("name").trim() == name;
        let same_ids = ids.is_some_and(|(vendor, product)| {
            hex(&read("id/vendor")) == Some(vendor) && hex(&read("id/product")) == Some(product)
        });
        if !(same_ids || (ids.is_none() && same_name)) {
            continue;
        }
        let keys = read("capabilities/key");
        // BTN_TOOL_PEN / BTN_TOOL_RUBBER exclude sibling touchpads and buttons.
        if !bit(&keys, 0x140) && !bit(&keys, 0x141) {
            continue;
        }
        let properties = read("properties");
        // INPUT_PROP_POINTER explicitly means an on-screen pointer is needed.
        // A missing property is unknown, not evidence of an indirect tablet.
        matches.push((same_name, bit(&properties, 0)));
    }
    let exact_name = matches.iter().any(|(same_name, _)| *same_name);
    let mut matches = matches
        .iter()
        .filter(|(same_name, _)| !exact_name || *same_name)
        .peekable();
    if matches.peek().is_some() && matches.all(|(_, indirect)| *indirect) {
        SampleFlags::INDIRECT_POINTER
    } else {
        SampleFlags::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_confirmed_indirect_pen_devices_get_the_pointer_flag() {
        let root =
            std::env::temp_dir().join(format!("capy-tablet-metadata-{}", std::process::id()));
        let path = root.join("input0");
        std::fs::create_dir_all(path.join("id")).unwrap();
        std::fs::create_dir_all(path.join("capabilities")).unwrap();
        for (file, value) in [
            ("name", "Test tablet Pen"),
            ("id/vendor", "056a"),
            ("id/product", "1234"),
        ] {
            std::fs::write(path.join(file), value).unwrap();
        }
        let keys = format!("1{}", " 0".repeat(0x140 / usize::BITS as usize));
        std::fs::write(path.join("capabilities/key"), keys).unwrap();
        for (properties, indirect) in [("1", true), ("2", false), ("0", false), ("", false)] {
            std::fs::write(path.join("properties"), properties).unwrap();
            assert_eq!(
                tablet_flags(&root, Some("056a"), Some("1234"), "Test tablet Pen")
                    .contains(SampleFlags::INDIRECT_POINTER),
                indirect
            );
        }
        std::fs::write(path.join("properties"), "1").unwrap();
        assert_eq!(
            tablet_flags(&root, Some("056a"), Some("abcd"), "Different tablet"),
            SampleFlags::NONE
        );
        std::fs::write(path.join("capabilities/key"), "1").unwrap();
        assert_eq!(
            tablet_flags(&root, Some("056a"), Some("1234"), "Test tablet Pen"),
            SampleFlags::NONE
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
