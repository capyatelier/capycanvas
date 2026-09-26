//! Neutral shortcuts preserve the remembered pair when no paint slot is selected.
use super::*;

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
}
