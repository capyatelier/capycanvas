# Magic Wand and Fill on gigapixel drawings

[Design history](README.md)

Written 2026-11-23, when the canvas side was raised to 131,072 px. **Decision: Magic
Wand and Fill keep refusing drawings above the region limit for now.** Option A is
the chosen path when this work is scheduled.

## The limit

Before every Magic Wand or Fill tap, `region_tools.rs` checks `region_fits` with
the whole drawing's size. The region pipeline (`region_requests.rs`, `flood.rs`,
`flood_lines.rs`) sizes its classified mask, lines, labels and coverage by the
document extent. To pass, the whole drawing must:
- fit one GPU texture side (16,384 px);
- keep its labels within the 128 MiB storage binding, which is about 107 MP;
- keep each side below 65,535 px for the line store.

A larger drawing gets "Too large for this device's available memory" and no
change, wherever the tap lands.

## Options

| | What | Exact | Size |
| --- | --- | --- | --- |
| A | Run the existing pipeline on a window around the tap | Yes, inside the window; a region that reaches the window edge is refused | ≈ 200–350 production lines, ≈ 150 test lines |
| B | Flood the whole drawing in bands and merge labels across them | Yes, at any size | ≈ 800–1,500 lines; gap closing spans the whole region, so band seams are hard |
| C | Flood a downscaled copy | No: line-art edges come out wrong | Small; rejected |

## Option A

1. **Window.** The largest rectangle that `flood::fits` accepts, centred on the tap
   and clamped to the drawing. A drawing that fits today uses the whole drawing,
   so its results don't change.
2. **Pipeline.** The region request path uses the window's extent in place of the
   document extent. Only tiles inside the window are classified, and the tap
   position becomes window-relative.
3. **Output.** Fill paints its coverage at the window's origin. Magic Wand returns
   `SelectionShape::Pixels` for the window with a translation affine, which
   `Selection { shape, affine }` already supports.
4. **Edge rule.** A region within the gap reach plus one pixel of a window side
   that isn't a drawing side is refused with the existing notice. The margin stops
   gap closing from sealing against lines the window cut.
5. **Not covered.** Non-contiguous Magic Wand and enclosure fills stay
   whole-drawing, so they're still refused above the limit. Pixel selections stay
   bounded by the selection memory check.

**Cost.** A tap's time follows the window, not the drawing. A 100 MP window takes
about 20 s on the low tier by the region cost model, where 12 MP takes 2–3 s; the
other tiers are several times faster. A first window the size of the view plus a
margin, grown only when the region reaches its edge, would make zoomed-in taps
fast. It's left for later.

**Tests.** All run without a window:
1. A closed shape at the far edge of a 131,072 × 8,192 drawing fills exactly.
2. An open region that reaches the window edge is refused with no change.
3. A gap within the reach near the window edge does not seal.
4. Drawings that fit today give byte-identical results.

The device check is the gigapixel chain's `pano.wand` and `pano.fill` steps.
