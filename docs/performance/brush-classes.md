# Brush classes

[Performance targets](../PERFORMANCE_TARGETS.md)

The class decides a brush's guaranteed size.

- **Measurements decide the class** where they exist. The
  [1000 px Apple and Windows matrices](../development/apple-port-1000px-20260922.md)
  split the dry presets cleanly into a fast group and a slow group.
- **Otherwise the execution path decides it** (see [Brushes](../internals/brushes.md)).

| Class | Guaranteed size | What puts a brush here | Brushes (preset id) |
| --- | --- | --- | --- |
| Simple | 2048 px (low tier: 1024 px, goal 2048 px) | Dry deposit from one tip, with at most paper grain, edge roughness or pooling. No canvas sampling. | G-Pen (1), Rough G-Pen (28), Calligraphy Pen (29), Antique Pen (30), Realistic Pen (31), Wet Ink (32), Pencil (2), Pointy Pencil (25), Shading Pencil (26), Charcoal (27), Chalk (6), Eraser (3), Airbrush (5); the Selection Brush and mask painting with a simple preset |
| Complex | 1024 px | Heavier per-contact work: several textures, bristle fibres with depletion, scatter envelopes, the ordered Multiply path, or measured cost in the slow group | Marker (7), Blotty Ink (33), Realistic Brushed Ink (34), Pastel Block (17), Paintbrush (4), Textured Flat (15), Dry Scumble (16), Transparent Glaze (18), Multiply Glaze (14), Dual Texture (9), Spray (8) |
| Very complex | 512 px | Samples the canvas or keeps paint state: wet reservoirs and mixing, watercolour transport, smudge backtrace, Liquify gathers. Cost grows with the sampled area, not just the footprint. | Opaque Gouache (19), Watercolor Wash (20), Wet Watercolor (21), Loaded Oil (22), Palette Knife (23), Wet Round (11), Natural Blender (24), Smudge (10), Liquify Push (12), Liquify Twirl Clockwise (36), Liquify Twirl Counterclockwise (13), Liquify Pinch (37), Liquify Expand (38), Liquify Crystals (39) |

**Notes on specific presets:**

- **Marker** is mechanically simple, but on every measured GPU it runs with the
  complex group.
- **Wet Ink** does not sample the canvas despite its name.
- **Default sizes above the class guarantee:**
  - Watercolor Wash (620 px), Liquify Twirl (620 px), Palette Knife (560 px),
    Wet Watercolor (540 px), and Liquify Pinch and Expand (520 px).
  - Their defaults must also meet the target, or be reduced to 512 px.
- **Eraser mode** keeps the class of the preset it erases with.
- **Mask painting** evaluates only a preset's footprint, so it is never very
  complex.
