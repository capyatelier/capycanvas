#!/usr/bin/env python3
"""Bake the shared SVG tool bank into signed distances for GPU cursors."""
import argparse
import hashlib
import math
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
ICONS = (
    "pen", "marker", "pencil", "pastel", "paint", "watercolor", "oil-paint",
    "eraser", "airbrush", "spray", "decoration", "blend", "liquify", "clone",
    "heal", "spot-heal", "move", "transform", "crop", "lasso", "rectangle-select",
    "ellipse-select", "polygon-select", "auto-select", "color-select",
    "selection-brush", "tonal-select", "lasso-fill", "hand", "eyedropper",
    "fill", "gradient", "gradient-radial", "gradient-reflected", "line",
    "rectangle", "ellipse", "ruler", "ruler-parallel", "ruler-radial",
)
SIZE = 64
EXTENT = 20
HOTSPOTS = {
    "pen": (1.15, 14.85), "marker": (1.5, 14.5), "pencil": (2, 14),
    "pastel": (4.15, 12.6), "paint": (11.28, 3.87), "watercolor": (1, 14.2),
    "oil-paint": (5.53, 11.69), "eraser": (5, 14), "airbrush": (10, 8),
    "spray": (8, 8), "blend": (2.6, 11.7), "clone": (8, 14.25),
    "heal": (2.34, 13.66), "spot-heal": (4, 12), "eyedropper": (2, 14),
    "selection-brush": (5.3, 13.7), "auto-select": (2, 14), "color-select": (2, 13),
    "lasso": (3, 13), "lasso-fill": (3, 13), "line": (2, 14), "ruler": (2, 14),
}


def distances(mask, value):
    grid = [[0 if mask[y * SIZE + x] == value else 1e6 for x in range(SIZE)] for y in range(SIZE)]
    def transform(row):
        sites = [0]
        edges = [-math.inf, math.inf]
        for q in range(1, SIZE):
            while True:
                p = sites[-1]
                edge = (row[q] + q * q - row[p] - p * p) / (2 * (q - p))
                if edge > edges[-2]:
                    break
                sites.pop()
                edges.pop(-2)
            sites.append(q)
            edges.insert(-1, edge)
        k = 0
        result = []
        for q in range(SIZE):
            while edges[k + 1] < q:
                k += 1
            result.append((q - sites[k]) ** 2 + row[sites[k]])
        return result
    grid = [transform(row) for row in grid]
    return list(zip(*(transform(row) for row in zip(*grid))))


def bake(source):
    import cairo
    import gi
    gi.require_version("Rsvg", "2.0")
    from gi.repository import Rsvg
    surface = cairo.ImageSurface(cairo.FORMAT_A8, SIZE, SIZE)
    context = cairo.Context(surface)
    context.translate(SIZE / EXTENT * 2, SIZE / EXTENT * 2)
    handle = Rsvg.Handle.new_from_data(source)
    viewport = Rsvg.Rectangle()
    viewport.width = viewport.height = SIZE / EXTENT * 16
    handle.render_document(context, viewport)
    surface.flush()
    alpha = bytes(surface.get_data())
    mask = [a >= 128 for a in alpha]
    inside, outside = distances(mask, True), distances(mask, False)
    result = bytearray()
    for y in range(SIZE):
        for x in range(SIZE):
            a = alpha[y * SIZE + x]
            d = (0.5 - a / 255) if 0 < a < 255 else (
                (math.sqrt(outside[y][x]) - 0.5) * -1 if mask[y * SIZE + x]
                else math.sqrt(inside[y][x]) - 0.5)
            result.append(max(0, min(255, round(128 + d * EXTENT / SIZE * 16))))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    sources = [(name, (ROOT / f"apps/layer-web/icons/layer-{name}-symbolic.svg").read_bytes()) for name in ICONS]
    rust = "pub const TOOL_CURSOR_ICONS: &[(&str, &str)] = &[\n" + "".join(
        f'    ("{name}", "{hashlib.sha256(source).hexdigest()}"),\n' for name, source in sources) + "];\n"
    rust += "pub fn tool_cursor_marker(icon: &str) -> Option<f32> {\n    TOOL_CURSOR_ICONS.iter().position(|(name, _)| *name == icon).map(|i| 7. + i as f32)\n}\n"
    rust += "pub const TOOL_CURSOR_HOTSPOTS: &[[f32; 2]] = &[\n" + "".join(
        f"    [{float(x)}, {float(y)}],\n" for x, y in (HOTSPOTS.get(name, (8, 8)) for name in ICONS)) + "];\n"
    path = ROOT / "crates/layer-render/src/cursor_icons.rs"
    atlas = ROOT / "crates/layer-render-wgpu/src/tool-cursors.bin"
    data = b"".join(bake(source) for _, source in sources)
    if args.check:
        if path.read_text() != rust or atlas.read_bytes() != data:
            sys.exit("Tool cursor assets are stale; run /usr/bin/python3 tools/build/cursor-icons.py")
    else:
        atlas.write_bytes(data)
        path.write_text(rust)


if __name__ == "__main__":
    main()
