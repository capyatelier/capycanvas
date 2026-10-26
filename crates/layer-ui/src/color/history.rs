use super::*;

const STEPS: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaintMemory {
    color: RgbColor,
    space: RgbSpace,
    hue: f32,
    coordinates: Option<ColorCoordinates>,
    hdr: Option<HdrPaint>,
}
pub(crate) type PaintPair = [PaintMemory; 2];

impl ColorState {
    fn remembered(&self, slot: usize) -> PaintMemory {
        PaintMemory {
            color: if slot == 0 { self.foreground } else { self.background },
            space: self.rgb_space,
            hue: self.hues[slot],
            coordinates: self.coordinates[slot],
            hdr: self.hdr_picker.map(|paints| paints[slot]),
        }
    }
    pub(crate) fn remembered_paints(&self) -> PaintPair {
        [self.remembered(0), self.remembered(1)]
    }
    fn history_slot(&self) -> usize {
        usize::from(self.front_swatch() == ColorSlot::Background)
    }
    fn restore(&mut self, slot: usize, memory: PaintMemory) -> Result<(), String> {
        if memory.space != self.rgb_space || memory.hdr.is_some() != self.hdr_picker.is_some() {
            let selection = (self.slot, self.paint_slot);
            self.paint_slot = if slot == 0 { ColorSlot::Foreground } else { ColorSlot::Background };
            let restored = match memory.hdr.filter(|_| self.hdr_picker.is_some()) {
                Some(paint) => self.set_color_with_picker(memory.color, Some(paint)),
                None => self.set_color(memory.color),
            };
            (self.slot, self.paint_slot) = selection;
            return restored;
        }
        Self::validate_definition(memory.color)?;
        if self.hdr_picker.is_some() {
            layer_core::color::hdr::validate_pixel(self.hdr_depth, memory.color.linear_in(self.rgb_space)?).map_err(str::to_string)?;
        }
        if slot == 0 { self.foreground = memory.color } else { self.background = memory.color }
        self.hues[slot] = memory.hue;
        self.coordinates[slot] = memory.coordinates;
        if let (Some(paints), Some(paint)) = (&mut self.hdr_picker, memory.hdr) {
            paints[slot] = paint;
        }
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct ColorHistory {
    undo: [Vec<PaintMemory>; 2],
    redo: [Vec<PaintMemory>; 2],
}
impl ColorHistory {
    pub fn record(&mut self, before: PaintPair, colors: &ColorState) {
        for (slot, before) in before.into_iter().enumerate() {
            let after = colors.remembered(slot);
            if (before.color, before.hdr) == (after.color, after.hdr) {
                continue;
            }
            let undo = &mut self.undo[slot];
            if undo.len() == STEPS {
                undo.remove(0);
            }
            undo.push(before);
            self.redo[slot].clear();
        }
    }
    pub fn swap(&mut self) {
        self.undo.swap(0, 1);
        self.redo.swap(0, 1);
    }
    pub fn can_step(&self, colors: &ColorState, redo: bool) -> bool {
        let steps = if redo { &self.redo } else { &self.undo };
        !steps[colors.history_slot()].is_empty()
    }
    pub fn step(&mut self, colors: &mut ColorState, redo: bool) -> Result<bool, String> {
        let slot = colors.history_slot();
        let current = colors.remembered(slot);
        let (from, to) = if redo { (&mut self.redo, &mut self.undo) } else { (&mut self.undo, &mut self.redo) };
        let Some(&memory) = from[slot].last() else { return Ok(false) };
        colors.restore(slot, memory)?;
        from[slot].pop();
        to[slot].push(current);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pick(colors: &mut ColorState, history: &mut ColorHistory, point: [f32; 2]) {
        let before = colors.remembered_paints();
        colors.apply(ColorAction::PickWheel { part: ColorWheelPart::Field, point, size: 200. }).unwrap();
        history.record(before, colors);
    }

    #[test]
    fn each_paint_undoes_and_redoes_its_own_changes_with_exact_coordinates() {
        let mut colors = ColorState::default();
        let mut history = ColorHistory::default();
        colors.apply(ColorAction::PickWheel { part: ColorWheelPart::Hue, point: [100., 4.], size: 200. }).unwrap();
        colors.apply(ColorAction::PickWheel { part: ColorWheelPart::Field, point: [100., 100.], size: 200. }).unwrap();
        let gray = colors.clone();
        pick(&mut colors, &mut history, [140., 70.]);
        let first = colors.clone();
        pick(&mut colors, &mut history, [60., 120.]);
        colors.apply(ColorAction::Select { slot: ColorSlot::Background }).unwrap();
        pick(&mut colors, &mut history, [80., 80.]);
        let background = colors.background;
        colors.apply(ColorAction::Select { slot: ColorSlot::Foreground }).unwrap();

        assert!(history.step(&mut colors, false).unwrap());
        assert_eq!(colors.remembered_paints()[0], first.remembered_paints()[0]);
        assert!(history.step(&mut colors, false).unwrap());
        assert_eq!(colors.remembered_paints()[0], gray.remembered_paints()[0], "gray keeps its hue");
        assert!(!history.step(&mut colors, false).unwrap());
        assert_eq!(colors.background, background);
        assert_eq!(colors.slot, ColorSlot::Foreground);

        assert!(history.step(&mut colors, true).unwrap());
        assert_eq!(colors.remembered_paints()[0], first.remembered_paints()[0]);
        pick(&mut colors, &mut history, [150., 150.]);
        assert!(!history.can_step(&colors, true), "a new change clears redo");
    }

    #[test]
    fn swap_carries_each_history_with_its_color_and_transparency_targets_the_front_paint() {
        let mut colors = ColorState::default();
        let mut history = ColorHistory::default();
        let original = colors.foreground;
        pick(&mut colors, &mut history, [140., 70.]);
        let picked = colors.foreground;
        colors.apply(ColorAction::Swap).unwrap();
        history.swap();
        assert!(!history.can_step(&colors, false));
        colors.apply(ColorAction::Select { slot: ColorSlot::Background }).unwrap();
        colors.apply(ColorAction::ToggleTransparent).unwrap();
        assert!(history.step(&mut colors, false).unwrap());
        assert_eq!((colors.background, colors.foreground), (original, RgbColor::WHITE));
        assert!(colors.transparent());
        assert_ne!(picked, original);
    }

    #[test]
    fn unchanged_paint_and_picker_only_moves_add_no_step_and_history_is_bounded() {
        let mut colors = ColorState::default();
        let mut history = ColorHistory::default();
        let before = colors.remembered_paints();
        colors.apply(ColorAction::Shape { shape: ColorShape::Square }).unwrap();
        history.record(before, &colors);
        assert!(!history.can_step(&colors, false));
        for i in 0..=STEPS {
            pick(&mut colors, &mut history, if i % 2 == 0 { [140., 70.] } else { [60., 120.] });
        }
        let mut steps = 0;
        while history.step(&mut colors, false).unwrap() { steps += 1; }
        assert_eq!(steps, STEPS);
    }

    #[test]
    fn hdr_paint_restores_intensity_and_survives_a_document_space_change() {
        let mut colors = ColorState::default();
        colors.set_document_depth(layer_core::color::SampleDepth::F16).unwrap();
        let mut history = ColorHistory::default();
        let before = colors.remembered_paints();
        colors.apply(ColorAction::HdrIntensity { stops: 2. }).unwrap();
        history.record(before, &colors);
        let bright = colors.clone();
        pick(&mut colors, &mut history, [140., 70.]);
        colors.set_rgb_space(RgbSpace::DisplayP3).unwrap();
        assert!(history.step(&mut colors, false).unwrap());
        assert_eq!(colors.foreground, bright.foreground);
        assert_eq!(colors.hdr_intensity(), 2.);
        colors.validate().unwrap();
        assert!(history.step(&mut colors, false).unwrap());
        assert_eq!(colors.hdr_intensity(), 0.);
    }
}
