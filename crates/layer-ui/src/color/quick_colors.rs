//! Neutral shortcuts preserve the remembered pair when no paint slot is selected.
use super::*;

pub(super) fn black() -> RgbColor {
    RgbColor::BLACK
}

#[derive(Clone, Debug, Serialize)]
pub struct QuickColorView {
    pub white: bool,
    pub label: &'static str,
    pub rgba: [f32; 4],
    pub selected: bool,
}
impl ColorState {
    pub fn quick_colors(&self) -> [QuickColorView; 2] {
        [false, true].map(|white| {
            let color = if white {
                RgbColor::WHITE
            } else {
                RgbColor::BLACK
            };
            QuickColorView {
                white,
                label: if white {
                    "Paint with white"
                } else {
                    "Paint with black"
                },
                rgba: color.rgba,
                selected: self.slot == ColorSlot::Temporary && self.temporary == color,
            }
        })
    }
}

// Old workspaces contain only the foreground/background picker coordinates.
// Append an independent black paint without altering either remembered slot.
fn widen<T: Default, E: serde::de::Error>(mut slots: Vec<T>) -> Result<[T; 3], E> {
    if slots.len() == 2 {
        slots.push(T::default());
    }
    slots
        .try_into()
        .map_err(|_| E::custom("Expected two or three paint slots"))
}
pub(super) fn deserialize_slots<'de, D, T>(reader: D) -> Result<[T; 3], D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    widen(Vec::<T>::deserialize(reader)?)
}
pub(super) fn deserialize_hdr_slots<'de, D>(reader: D) -> Result<Option<[HdrPaint; 3]>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<Vec<HdrPaint>>::deserialize(reader)?
        .map(widen)
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcuts_replace_selected_slot_or_use_independent_paint() {
        for hdr in [false, true] {
            let mut s = ColorState::default();
            s.set_hdr_enabled(hdr).unwrap();
            for slot in [ColorSlot::Foreground, ColorSlot::Background] {
                s.apply(ColorAction::Select { slot }).unwrap();
                for white in [false, true] {
                    let other = if slot == ColorSlot::Foreground {
                        s.background
                    } else {
                        s.foreground
                    };
                    s.apply(ColorAction::QuickColor { white }).unwrap();
                    assert_eq!(
                        s.definition(),
                        if white {
                            RgbColor::WHITE
                        } else {
                            RgbColor::BLACK
                        }
                    );
                    assert_eq!(s.slot, slot);
                    assert_eq!(
                        other,
                        if slot == ColorSlot::Foreground {
                            s.background
                        } else {
                            s.foreground
                        }
                    );
                }
            }
            let pair = [s.foreground, s.background];
            s.apply(ColorAction::Select {
                slot: ColorSlot::Transparent,
            })
            .unwrap();
            for white in [false, true, false] {
                s.apply(ColorAction::QuickColor { white }).unwrap();
                assert!(!s.transparent());
                assert_eq!(s.slot, ColorSlot::Temporary);
                assert_eq!([s.foreground, s.background], pair);
                assert!(s.quick_colors()[usize::from(white)].selected);
            }
            s.apply(ColorAction::PickWheel {
                part: ColorWheelPart::Field,
                point: [60., 75.],
                size: 128.,
            })
            .unwrap();
            assert_eq!(
                [s.foreground, s.background],
                pair,
                "wheel edits the independent paint too"
            );
            let saved = serde_json::to_string(&s).unwrap();
            let restored: ColorState = serde_json::from_str(&saved).unwrap();
            restored.validate().unwrap();
            assert_eq!(s, restored);
        }
    }
    #[test]
    fn old_two_slot_workspaces_keep_colors_and_hdr_coordinates() {
        for hdr in [false, true] {
            let mut s = ColorState::default();
            s.set_hdr_enabled(hdr).unwrap();
            let mut value = serde_json::to_value(&s).unwrap();
            value.as_object_mut().unwrap().remove("temporary");
            for key in ["hues", "coordinates", "hdr_picker"] {
                if let Some(slots) = value.get_mut(key).and_then(|v| v.as_array_mut()) {
                    slots.truncate(2);
                }
            }
            let restored: ColorState = serde_json::from_value(value).unwrap();
            restored.validate().unwrap();
            assert_eq!(restored, s);
        }
    }
}
