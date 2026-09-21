//! Retained display damage shared by the canvas, cursor, and Navigator.
use crate::pixel_rect::PixelRect;
use layer_render::ViewState;

pub(super) fn damage(rect: PixelRect, view: ViewState, turns: u32) -> PixelRect {
    if rect.is_empty() {
        return rect;
    }
    let [a, b, c, d, tx, ty] = view.document_to_surface;
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    let [w, h] = [view.width_px as f32, view.height_px as f32];
    // Bilinear source support grows with zoom; expand in document space too.
    for x in [
        rect.min_x().saturating_sub(1),
        rect.max_x().saturating_add(1),
    ] {
        for y in [
            rect.min_y().saturating_sub(1),
            rect.max_y().saturating_add(1),
        ] {
            let (x, y) = (
                a * x as f32 + c * y as f32 + tx,
                b * x as f32 + d * y as f32 + ty,
            );
            let p = match turns {
                1 => [h - y, x],
                2 => [w - x, h - y],
                3 => [y, w - x],
                _ => [x, y],
            };
            for i in 0..2 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
    }
    let extent = if turns % 2 == 0 {
        [view.width_px, view.height_px]
    } else {
        [view.height_px, view.width_px]
    };
    // Cover filter footprints and rounding at damage edges.
    PixelRect::new(
        (min[0].floor() - 3.).max(0.) as u32,
        (min[1].floor() - 3.).max(0.) as u32,
        (max[0].ceil() + 3.).min(extent[0] as f32).max(0.) as u32,
        (max[1].ceil() + 3.).min(extent[1] as f32).max(0.) as u32,
    )
}

/// State for an explicitly retained destination (not an acquired FIFO image).
/// No extra full-screen texture is allocated for this mode.
pub(super) struct Retained {
    pub valid: bool,
    pub revision: u64,
    pub hdr: [f32; 8],
    pub proof: [u32; 4],
    pub cursor: PixelRect,
    pub overviews: Vec<[f32; 24]>,
}
impl Default for Retained {
    fn default() -> Self {
        Self {
            valid: false,
            revision: u64::MAX,
            hdr: [f32::NAN; 8],
            proof: [u32::MAX; 4],
            cursor: PixelRect::EMPTY,
            overviews: Vec::new(),
        }
    }
}

pub(super) fn surface_bounds(bounds: [f32; 4], view: ViewState, turns: u32) -> PixelRect {
    let [x, y, width, height] = bounds;
    let [w, h] = [view.width_px as f32, view.height_px as f32];
    let [left, top, right, bottom] = match turns {
        1 => [h - y - height, x, h - y, x + width],
        2 => [w - x - width, h - y - height, w - x, h - y],
        3 => [y, w - x - width, y + height, w - x],
        _ => [x, y, x + width, y + height],
    };
    let extent = if turns % 2 == 0 { [w, h] } else { [h, w] };
    PixelRect::new(
        left.floor().clamp(0., extent[0]) as u32,
        top.floor().clamp(0., extent[1]) as u32,
        right.ceil().clamp(0., extent[0]) as u32,
        bottom.ceil().clamp(0., extent[1]) as u32,
    )
}

pub(super) fn cursor_bounds(
    segments: &[layer_render::CursorSegment],
    view: ViewState,
    turns: u32,
) -> PixelRect {
    segments.iter().fold(PixelRect::EMPTY, |area, s| {
        let margin = 3.5 * s.scale.max(1.);
        let left = s.from[0].min(s.to[0]) - margin;
        let top = s.from[1].min(s.to[1]) - margin;
        let right = s.from[0].max(s.to[0]) + margin;
        let bottom = s.from[1].max(s.to[1]) + margin;
        area.union(surface_bounds(
            [left, top, right - left, bottom - top],
            view,
            turns,
        ))
    })
}

/// Merge intersecting regions, retaining distant canvas/overlay damage as
/// separate passes. Restart after a merge because its bounds may meet an
/// earlier region. The list is small (paint, cursor, visible Navigators).
pub(super) fn add_region(regions: &mut Vec<PixelRect>, mut area: PixelRect) {
    if area.is_empty() {
        return;
    }
    let mut i = 0;
    while i < regions.len() {
        if !regions[i].intersect(area).is_empty() {
            area = area.union(regions.swap_remove(i));
            i = 0;
        } else {
            i += 1;
        }
    }
    regions.push(area);
}
