# Liquify scope boundary

[M5–M6 specification](photo-editing-m5-m6.md)

Status: **scope decided; live Liquify deferred**.

Capy Canvas will support both baked and live Liquify long term. The existing
Liquify brush already changes pixels and remains supported with its current
presets, tools and stroke history.

Live Liquify will be a future **effect layer/filter** over editable underlying
content. It is outside this M5–M6 delivery. Do not implement it as attached
layer-placement geometry, and do not add speculative storage or renderer hooks.

The deferred work includes the XF-5 remainder: retained deformation,
Reconstruct/Reconstruct All, saved displacement data and the associated
performance prototype. These are not prerequisites or completion criteria for
this effort. The existing baked tool is not replaced or removed.

M5 retained placement contains an outer homography and an optional Warp mesh.
Its common pixel-write rule applies to the baked Liquify brush: an existing
affine-editable target follows the current path; projective or Warp content
requires explicit **Apply Transform to Pixels** before any destructive brush
write. Regression-test that integration without redesigning Liquify.

No displacement raster plane, field grid/input basis, field-specific history,
format change, resource accounting or future filter placeholder belongs in the
current implementation. Future live Liquify gets its own design and performance
qualification when that work begins.
