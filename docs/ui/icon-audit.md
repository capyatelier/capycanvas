# Android, web and GTK icon audit

Audited 2026-09-12. The shared bank now contains 157 SVGs: 99 original assets and 58 additions. The first pass covered asset files and commands but missed category identities and consumers that ignored model icons. This second pass starts from the visible control models and their Android/web renderers. Every asset was inspected at 16, 24 and 32 logical pixels in light and dark palettes. The drawings are original project vectors; references inform meanings, not copied artwork.

## Design decisions

Use solid silhouettes for painting tools and concrete objects. Keep clean contour geometry where it carries the meaning: selection boundaries, shapes, links, guides and cursor previews. Use consistent negative space and optical centering. Foreground paint follows the theme; explicit black/white swatches retain their colors, including under disabled opacity. These weight, fill and size checks follow the distinctions described in the [Material Symbols guide](https://developers.google.com/fonts/docs/material_symbols).

Review correction: the eyedropper bulb and inner opening now share the shaft's diagonal centerline. The corrected silhouette was inspected enlarged and at 16/24/32 pixels, including native tablet captures in both themes. Web's 282 production-control checks and 157-asset render matrix and both Android icon tests passed again after this correction; the rebuilt APK was installed with app data retained. Evidence is in `artifacts/icon-audit/eyedropper/`.

Clear content uses the requested empty-center burst, distinct from the local Eraser tool and Delete Layer. This follows the clear-content command convention in [Clip Studio Paint](https://help.clip-studio.com/en-us/manual_en/270_canvas/Deleting_content.htm); it is not claimed as a universal standard across all drawing apps.

The finger/smear choice expresses the paint-smudging behavior described by [Adobe](https://helpx.adobe.com/photoshop/using/smudge-image-areas.html). The spiral is our compact representation of the pixel-warp/twirl family documented by [Clip Studio Paint](https://help.clip-studio.com/en-us/manual_en/360_transform/Liquify_tool.htm). Transformation, canvas fitting, selection operations, local filling and selection filling each receive distinct symbols. Brush-preset toolbar items, the customization picker, and preset rows now use the preset’s specific medium. Stroke previews remain alongside their labels and icons.

## SVG inventory

The Reference column identifies the relevant functional/conventional family. The chosen drawing and retain/redraw decisions are this audit’s design judgments. Generic interface choices use familiar symbols; the Material reference guides weight/size treatment, rather than prescribing every individual metaphor.

| SVG key | Action or role | Chosen drawing | Decision | Reference |
| --- | --- | --- | --- | --- |
| `add-layer` | New paint layer | Document with a plus | Added | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `adjustments` | Filters / adjustments | Three solid slider controls | Redrawn | [adjustments](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/adjustment-layers-options.html) |
| `airbrush` | Airbrush | Solid spray gun with separated dots | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `alpha-lock` | Lock transparent pixels | Checkerboard and closed padlock | Retained | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `animation` | Animation resource | Film frame and play triangle | Redrawn | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `appearance` | Switch light/dark theme | Half-filled circle | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `auto-select` | Select connected color | Magic wand and spark | Retained | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `back` | Return to previous settings page | Left arrow (mirrored for forward navigation) | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `black_white` | Black and white adjustment | Split dark/light rectangle | Redrawn | [adjustments](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/adjustment-layers-options.html) |
| `blend` | Blend / smear existing paint | Solid fingertip dragging a paint smear | Redrawn | [smudge](https://helpx.adobe.com/photoshop/using/smudge-image-areas.html) |
| `brightness_contrast` | Brightness / contrast | Half-filled sun with a visible center | Redrawn | [adjustments](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/adjustment-layers-options.html) |
| `brush` | Paintbrush | Tapered handle, separated bristle tip | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `check` | Apply transform / confirm choice | Checkmark | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `chevron-double-left` | Expand right sidebar | Paired chevrons toward the canvas | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `chevron-double-right` | Expand left sidebar | Paired chevrons toward the canvas | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `chevron-down` | Open choices | Downward disclosure chevron | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `clear` | Clear editing-layer content | Eight rays around an empty center | Added | [clear](https://help.clip-studio.com/en-us/manual_en/270_canvas/Deleting_content.htm) |
| `clip` | Clip to layer below | Downward arrow between solid layer slabs | Redrawn | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `close-document` | Close current drawing | Document with a cross cutout | Added | [files](https://help.clip-studio.com/en-us/manual_en/690_interface/Command_Bar.htm) |
| `close` | Cancel transform / dismiss dialog | Centered geometric cross | Added | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `color` | Current paint color | Live filled circular swatch with foreground rim | Retained | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `color_balance` | Color balance adjustment | Balanced scales with solid pans | Redrawn | [adjustments](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/adjustment-layers-options.html) |
| `colors` | Foreground/background color pair; the Color tool | Larger foreground circle over a smaller background circle at its lower right; live paints on hosts | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `column-expand` | Legacy sidebar expand resource | Paired outward chevrons; foreground tint | Redrawn | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `cursor-brush-cross` | Brush outline and cross cursor preference | Broken ring plus crosshair | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `cursor-brush` | Brush outline cursor preference | Broken ring showing the cursor contour | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `cursor-cross` | Crosshair cursor preference | Centered crosshair | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `cursor-dot` | Dot cursor preference | Solid centered dot | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `cursor-none` | No cursor preference | Short neutral dash | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `curves` | Curves adjustment | Response curve, axes and a control point | Redrawn | [adjustments](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/adjustment-layers-options.html) |
| `decoration` | Decoration / stamp brush | Solid irregularly sized sparkles | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `delete` | Delete layer(s) / ruler | Trash can with clear interior slots | Redrawn | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `deselect` | Remove pixel selection | Slashed marching boundary | Added | [selection](https://docs.krita.org/en/reference_manual/main_menu/select_menu.html) |
| `down` | Lower layer / decrease ordering | Downward arrow | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `ellipse` | Draw ellipse | Geometric ellipse with consistent rim weight | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `eraser` | Erase locally with a brush | Solid tilted eraser with separated erase face | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `export-document` | Export PNG | Arrow leaving a document frame | Redrawn | [files](https://help.clip-studio.com/en-us/manual_en/690_interface/Command_Bar.htm) |
| `exposure` | Exposure adjustment | Diagonal light/dark field with plus/minus | Redrawn | [adjustments](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/adjustment-layers-options.html) |
| `eye-hidden` | Layer hidden / show layer | Slashed eye, recognizable silhouette | Redrawn | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `eye` | Layer visible / hide layer | Eye silhouette with pupil and clear iris | Redrawn | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `eyedropper` | Sample color | Solid bulb and pipette with a clear channel | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `figure` | Shape tool group | Overlapping square and circle | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `fill-selection` | Fill selected pixels | Filled region within marching boundary | Added | [selection](https://docs.krita.org/en/reference_manual/main_menu/select_menu.html) |
| `fill` | Fill connected area | Tilted paint bucket and falling drop | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `fit` | Fit canvas to viewport | Four inward framing corners | Retained | [view](https://docs.krita.org/en/reference_manual/main_menu/view_menu.html) |
| `flip-horizontal` | Mirror view horizontally | Left/right triangles around vertical axis | Retained | [view](https://docs.krita.org/en/reference_manual/main_menu/view_menu.html) |
| `flip-vertical` | Mirror view vertically | Up/down triangles around horizontal axis | Retained | [view](https://docs.krita.org/en/reference_manual/main_menu/view_menu.html) |
| `folder-open` | Expanded layer group | Open folder silhouette | Redrawn | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `folder` | Layer group / create group | Closed folder silhouette | Redrawn | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `fullscreen-enter` | Enter fullscreen | Two outward diagonal arrows | Retained | [view](https://docs.krita.org/en/reference_manual/main_menu/view_menu.html) |
| `fullscreen-exit` | Leave fullscreen | Two inward diagonal arrows | Retained | [view](https://docs.krita.org/en/reference_manual/main_menu/view_menu.html) |
| `gradient` | Gradient tool / linear gradient options | Tonal ramp with solid bands | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `gradient_map` | Gradient map adjustment | Tonal ramp plus mapping arrow | Redrawn | [adjustments](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/adjustment-layers-options.html) |
| `grip` | Grab handle | Six equally spaced dots | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `hand` | Pan canvas | Solid open hand; separate fingers | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `hue_saturation` | Hue / saturation adjustment | Three tonal swatches and adjustment slider | Redrawn | [adjustments](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/adjustment-layers-options.html) |
| `image` | Import image / image content | Picture frame, mountains and sun | Redrawn | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `info` | About / information | Lowercase information mark inside a circle | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `invert-selection` | Invert pixel selection | Complementary regions within marching boundary | Added | [selection](https://docs.krita.org/en/reference_manual/main_menu/select_menu.html) |
| `keyboard` | Keyboard shortcuts settings | Keyboard with distinct key rows | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `lasso-fill` | Fill a freehand region | Filled lasso loop and trailing cord | Added | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `lasso` | Freehand selection | Loop and trailing lasso cord | Retained | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `layers` | Layers panel | Three distinct stacked solid sheets | Redrawn | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `levels` | Levels adjustment | Solid histogram bars and baseline | Redrawn | [adjustments](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/adjustment-layers-options.html) |
| `line` | Draw straight line | Single diagonal solid strip | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `link` | Link layers / preserve transform aspect | Interlocking chain links | Retained | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `liquify` | Push / twist existing pixels | Centered tapered spiral silhouette | Redrawn | [liquify](https://help.clip-studio.com/en-us/manual_en/360_transform/Liquify_tool.htm) |
| `lock` | Lock layer editing | Solid closed padlock and keyhole | Redrawn | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `mask` | Add layer mask | Solid rectangular mask with circular cutout | Redrawn | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `menu` | Application menu / Commands panel | Three horizontal menu bars | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `minus` | Zoom out / decrement / divider choice | Centered minus | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `more` | More layer actions | Vertical ellipsis | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `move` | Move artwork | Four solid directional arrowheads; unclipped ends | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `navigator` | Navigator panel | Compass and navigation needle | Redrawn | [view](https://docs.krita.org/en/reference_manual/main_menu/view_menu.html) |
| `new-document` | Create drawing | Solid folded document with plus cutout | Redrawn | [files](https://help.clip-studio.com/en-us/manual_en/690_interface/Command_Bar.htm) |
| `new-toolbar` | Create toolbar | Toolbar frame, cells and plus | Added | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `new-window` | Open another drawing window | Window title strip and plus | Added | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `opacity` | Brush opacity | Half-filled circular opacity marker | Retained | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `open-document` | Open drawing | Open folder | Redrawn | [files](https://help.clip-studio.com/en-us/manual_en/690_interface/Command_Bar.htm) |
| `pen` | Ink pen | Filled nib, slit, breather hole and cap | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `pencil` | Sketch pencil | Solid hexagonal pencil and pointed lead | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `pin` | Pin drawer | Solid pushpin | Redrawn | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `plus` | Zoom in / increment / new workspace | Centered plus | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `posterize` | Posterize adjustment | Solid stepped tonal levels | Redrawn | [adjustments](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/adjustment-layers-options.html) |
| `properties` | Properties panel | Property cells and value rows | Redrawn | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `rectangle` | Draw rectangle | Regular rectangle with consistent rim weight | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `redo` | Redo document / workspace edit | Rightward returning arrow | Retained | [files](https://help.clip-studio.com/en-us/manual_en/690_interface/Command_Bar.htm) |
| `reference` | Use layers as references | Lighthouse with distinct light beams | Redrawn | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `reset-layout` | Restore docking layout | Panel layout with reset arrow | Added | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `rotate-left` | Rotate view left | Counterclockwise circular arrow | Retained | [view](https://docs.krita.org/en/reference_manual/main_menu/view_menu.html) |
| `rotate-right` | Rotate view right | Clockwise circular arrow | Retained | [view](https://docs.krita.org/en/reference_manual/main_menu/view_menu.html) |
| `ruler-parallel` | Parallel guide | Parallel line family | Retained | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `ruler-radial` | Radial guide | Lines radiating from a visible center | Retained | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `ruler-snap` | Snap brush strokes to guides | Horseshoe magnet | Retained | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `ruler` | Create/show drawing guides | Solid ruler with open edge ticks | Redrawn | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `save-as` | Save editable copy | Disk and edit pencil | Added | [files](https://help.clip-studio.com/en-us/manual_en/690_interface/Command_Bar.htm) |
| `save-document` | Save drawing | Solid disk with label and shutter cutouts | Redrawn | [files](https://help.clip-studio.com/en-us/manual_en/690_interface/Command_Bar.htm) |
| `search` | Search settings / tools | Magnifying glass | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `select-all` | Select entire canvas | Complete marching boundary | Added | [selection](https://docs.krita.org/en/reference_manual/main_menu/select_menu.html) |
| `selection-checked` | Layer selected | Checkbox with checkmark | Retained | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `selection-empty` | Layer not selected | Empty checkbox | Retained | [layers](https://help.clip-studio.com/en-us/manual_en/180_layers/Using_layers.htm) |
| `settings` | Preferences / Tool settings panel | Solid cogwheel | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `size` | Brush size | Two solid dots of different diameters | Retained | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `source-code` | Open source repository | Code brackets and slash | Added | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `stats` | Diagnostics panel | Line chart and axes | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `swap` | Exchange foreground/background colors | Opposing arrows | Retained | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `toolbar` | Tools/custom toolbar panel / manage toolbars | Toolbar cells and content strip | Added | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `transform` | Scale / rotate artwork | Bounding box with four solid corner handles | Added | [tools](https://docs.krita.org/en/reference_manual/tools.html) |
| `undo` | Undo document / workspace edit | Leftward returning arrow | Retained | [files](https://help.clip-studio.com/en-us/manual_en/690_interface/Command_Bar.htm) |
| `up` | Raise layer | Upward arrow | Retained | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `vibrance` | Vibrance adjustment | Split inverted triangle; no warning exclamation | Redrawn | [adjustments](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/adjustment-layers-options.html) |
| `website` | Open application website | Globe with meridians | Added | [interface](https://developers.google.com/fonts/docs/material_symbols) |
| `zen-bathing` | Zen mode / brand choice | Owner-supplied capybara bathing; retains brand geometry | Retained | [brand](../../BRANDING.md) |
| `zen-facing-forward` | Zen mode / brand choice | Owner-supplied capybara facing forward; retains brand geometry | Retained | [brand](../../BRANDING.md) |
| `zen-looking-up` | Zen mode / brand choice | Owner-supplied capybara looking up; retains brand geometry | Retained | [brand](../../BRANDING.md) |
| `zen-sleeping` | Zen mode / brand choice | Owner-supplied capybara sleeping; retains brand geometry | Retained | [brand](../../BRANDING.md) |

| `bloom` | Bloom | Bright star and diffuse halo | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `blur` | Gaussian Blur | Concentric fading disk | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `chromatic-aberration` | Chromatic Aberration | Offset overlapping channels | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `crosshatch` | Crosshatch | Evenly spaced crossing strips | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `crt` | CRT | Monitor and scan lines | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `denoise` | Denoise | Scattered samples beside a clean solid region | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `domain-warp` | Domain Warp / Distort category | Displaced grid bands | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `edge-detect` | Edge Detect | Isolated nested boundaries | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `ellipse-both` | Ellipse outline and fill | Rim and separated interior | Added in second pass | [function](https://docs.krita.org/en/reference_manual/tools.html) |
| `ellipse-fill` | Filled ellipse | Solid ellipse | Added in second pass | [function](https://docs.krita.org/en/reference_manual/tools.html) |
| `emboss` | Emboss | Offset raised edge and inset face | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `glass` | Glass | Pane with reflection strips | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `gradient-radial` | Radial color to color | Concentric tonal disks | Added in second pass | [function](https://docs.krita.org/en/reference_manual/tools/gradient_draw.html) |
| `gradient-radial-transparent` | Radial color to clear | Concentric tonal disks with transparency checks | Added in second pass | [function](https://docs.krita.org/en/reference_manual/tools/gradient_draw.html) |
| `gradient-transparent` | Linear color to clear | Tonal bands ending in transparency checks | Added in second pass | [function](https://docs.krita.org/en/reference_manual/tools/gradient_draw.html) |
| `grain` | Film Grain / Texture category | Irregular grains | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `halftone` | Halftone | Regular dots of graduated size | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `heat-haze` | Heat Haze | Parallel undulating heat bands | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `high-pass` | High Pass | Contrasting edge over a neutral field | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `iridescence` | Iridescence | Rainbow interference arcs | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `kaleidoscope` | Kaleidoscope | Six symmetric facets | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `marker` | Marker | Solid chisel marker | Added in second pass | [function](https://docs.krita.org/en/reference_manual/krita_4_preset_bundle.html) |
| `mosaic` | Pixel Mosaic | Four tonal pixel blocks | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `motion-blur` | Motion Blur | Directional streaks and fading dots | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `oil-paint` | Oil paint | Capped paint tube with label and crimp | Added in second pass | [function](https://docs.krita.org/en/reference_manual/krita_4_preset_bundle.html) |
| `paint` | Paint | Flat bristle brush and ferrule | Added in second pass | [function](https://docs.krita.org/en/reference_manual/krita_4_preset_bundle.html) |
| `pastel` | Pastel | Blunt wrapped pastel stick | Added in second pass | [function](https://docs.krita.org/en/reference_manual/krita_4_preset_bundle.html) |
| `rainy-glass` | Rainy Glass | Water drops and descending trails | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `rectangle-both` | Rectangle outline and fill | Rim and separated interior | Added in second pass | [function](https://docs.krita.org/en/reference_manual/tools.html) |
| `rectangle-fill` | Filled rectangle | Solid rectangle | Added in second pass | [function](https://docs.krita.org/en/reference_manual/tools.html) |
| `reset` | Reset gradient parameters | Circular reset arrow | Added in second pass | [function](https://docs.krita.org/en/reference_manual/tools.html) |
| `ripple` | Ripple | Concentric ripple rings | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `sharpen` | Unsharp Mask / Detail category | Sharp split triangular edge | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `soft-focus` | Soft Focus | Lens rim around a diffuse center | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `solarize` | Solarize | Reversed adjacent tonal regions | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `split-tone` | Split Tone | Separate light and dark circular fields | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `spray` | Spray | Upright aerosol can and spray dots | Added in second pass | [function](https://docs.krita.org/en/reference_manual/krita_4_preset_bundle.html) |
| `vhs` | VHS | Video cassette and reels | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `vignette` | Vignette | Dark corners around an oval center | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |
| `watercolor` | Watercolor | Round waterbrush and water drop | Added in second pass | [function](https://docs.krita.org/en/reference_manual/krita_4_preset_bundle.html) |
| `white-balance` | White Balance | Temperature thermometer | Added in second pass | [function](https://docs.krita.org/en/reference_manual/filters.html) |

## Control coverage added in the second pass

The authoritative inventory is `ToolGroup::ALL`, the 24 built-in brush presets, `CommandId::TOOLS` plus Lasso Fill, the tool-specific mode projections, and the 40 entries in `assets/filters/manifest.json`. Both production hosts are exercised from these models; an asset existing on disk is insufficient.

### Painting categories

The physical-medium metaphors below distinguish related tools without trying to encode each brush algorithm. [Krita’s brush-family reference](https://docs.krita.org/en/reference_manual/krita_4_preset_bundle.html) distinguishes ink, markers, dry media, wet painting and watercolors. [Clip Studio’s watercolor/thick-paint guide](https://tips.clip-studio.com/en-us/articles/1514) supplies the medium context for waterbrush versus thick paint; the specific glyphs are original design choices.

| Visible category | Icon | Metaphor |
| --- | --- | --- |
| Pen | `pen` | Filled nib |
| Marker | `marker` | Solid chisel marker |
| Pencil | `pencil` | Pointed pencil |
| Pastel | `pastel` | Blunt wrapped pastel stick |
| Paint | `paint` | Flat bristle brush and ferrule |
| Watercolor | `watercolor` | Round waterbrush and water drop |
| Oil paint | `oil-paint` | Capped paint tube with label and crimp |
| Eraser | `eraser` | Block eraser |
| Airbrush | `airbrush` | Spray gun |
| Spray | `spray` | Upright aerosol can and spray dots |
| Texture | `decoration` | Scattered stamp marks |
| Blend | `blend` | Smudging fingertip |
| Liquify | `liquify` | Solid pixel-warp spiral |

All 24 preset rows retain their GPU stroke preview and display their category icon. The same identity reaches toolbar tiles and the toolbar customization picker. Sharing an icon within one medium is intentional: the stroke preview and preset name distinguish its variants. No category inherits an unrelated parent-tool fallback.

### Other tool choices

| Visible controls | Icon decisions |
| --- | --- |
| Fill / Auto Select sources | Eye for visible artwork, layer stack for editing layer, lighthouse for reference layers, following [reference-layer semantics](https://help.clip-studio.com/en-us/manual_en/180_layers/Reference_layers.htm). |
| Eyedropper sources | Eye for visible color, layers for layer color. |
| Four gradient modes | Linear bands versus radial disks; checker cells distinguish transparency, consistent with [gradient geometry](https://docs.krita.org/en/reference_manual/tools/gradient_draw.html). |
| Line / Rectangle / Ellipse | Shape contours identify the family; solid interior and a separated outline/interior distinguish Fill and Outline + fill. |
| Move / Scale and rotate | Directional arrows versus transform handles. |
| Straight / Parallel / Radial rulers | Ruler, parallel strips, radial guides. |
| Lasso / Lasso Fill | Open lasso boundary versus filled region. |
| Tool settings actions | Command glyphs alongside labels; native checkboxes retain their check state. |

### Filters

Thirty bundled filters previously inherited the generic adjustment-sliders icon. All 40 now have a specific identity, visible beside their GPU preview labels and on filter-layer content. Seven category headers and the selected category also carry a meaningful icon.

[Adobe’s filter reference](https://helpx.adobe.com/photoshop/using/filter-effects-reference.html) and [Krita’s filter families](https://docs.krita.org/en/reference_manual/filters.html) establish the operations represented here. Many procedural effects have no standardized glyph; their result-based drawings are this project’s design choices.

| Filter | Icon |
| --- | --- |
| Curves | `curves` |
| Levels | `levels` |
| Brightness / Contrast | `brightness_contrast` |
| Hue / Saturation | `hue_saturation` |
| Color Balance | `color_balance` |
| Exposure | `exposure` |
| Vibrance | `vibrance` |
| Black & White | `black_white` |
| Gradient Map | `gradient_map` |
| Posterize | `posterize` |
| Gaussian Blur | `blur` |
| Unsharp Mask | `sharpen` |
| High Pass | `high-pass` |
| Motion Blur | `motion-blur` |
| Denoise | `denoise` |
| Edge Detect | `edge-detect` |
| White Balance | `white-balance` |
| Split Tone | `split-tone` |
| Vignette | `vignette` |
| Film Grain | `grain` |
| Bloom | `bloom` |
| Soft Focus | `soft-focus` |
| Halftone | `halftone` |
| Crosshatch | `crosshatch` |
| Emboss | `emboss` |
| Pixel Mosaic | `mosaic` |
| Chromatic Aberration | `chromatic-aberration` |
| Painterly | `paint` |
| Solarize | `solarize` |
| Pencil | `pencil` |
| Kaleidoscope | `kaleidoscope` |
| Swirl | `liquify` |
| Ripple | `ripple` |
| Glass | `glass` |
| Rainy Glass | `rainy-glass` |
| VHS | `vhs` |
| CRT | `crt` |
| Heat Haze | `heat-haze` |
| Iridescence | `iridescence` |
| Domain Warp | `domain-warp` |

Category mapping: Tone → Levels; Color → hue/saturation swatches; Detail → sharpened edge; Blur → diffuse disk; Artistic → flat brush; Distort → warped grid; Texture → grain. Native select-menu values, numeric fields, ordinary text menu entries, and noninteractive section headings remain text controls; they do not depend on a glyph to identify their action.

## Host integration

Rust owns command, preset and panel identities. Android and web both load the same SVG bank; the layer-footer New Layer button uses the same symbol as the command. Android draws the original vectors into the actual device viewport instead of scaling a fixed bitmap and tinting the whole image. Foreground alpha is preserved, so secondary category glyphs match muted text while fixed swatch paints stay opaque. The icon fixture verifies black/white fills survive both theme changes and disabled compositing.

Font-drawn close buttons in toolbar management and web preferences/shortcut dialogs now use the shared geometric cross. Android workspace management uses shared close/plus/more symbols, and preferences Clear Search uses the same close glyph. Both hosts now use shared double-chevron SVGs for collapsed-column expansion; the glyph points toward the canvas and retains the existing button target. Web workspace-management close/add controls already use centered CSS geometry and remain appropriate. The battery/charging indicator uses its existing solid shell, level and lightning bolt; document dimension multiplication signs remain text.

## Apple integration — 2026-09-13

Both Apple apps generate vector assets from all 157 canonical SVGs. Brush
presets retain their stroke preview and also show the model's medium icon;
filter rows, category headings and the category picker use the shared filter
icons. New Layer uses `add-layer`, and custom close/check/more/plus controls
use the shared bank. Native system menus retain their platform indicators.
Tool-setting actions also show the shared command glyph, including Link for
Keep Proportions and Close for Cancel Transform. Their leading icon, six-point
gap and wrapping follow the browser; all 96 AppKit button bounds match Chrome
across both themes, two panel widths and four enabled/selected combinations.
Asset lookup strips only the filename prefix and suffix, preserving internal
words in names such as `layer-add-layer-symbolic`.

Both signed Debug targets build. The six SVG conversion tests, 306 shared UI
tests and 41 Apple bridge tests pass. Actual compiled AppKit/SwiftUI vectors
were compared against isolated Chrome at 16/24/32 points, light/dark colors,
and normal/accent/disabled opacity: 18 complete grids and 2,826 glyphs per host.
The 216 flat paint samples all pass, with maximum channel error 1/255. Complete
unmasked comparisons retain 27,869,184 pixels per host; mean absolute channel
error is 0.145/255, maximum 107/255, and differing-pixel fractions range from
0.97% to 12.81%. Exact raster parity is not claimed. These are AppKit component
captures, not physical UIKit pixel evidence or whole-editor visual acceptance.

The same manifest also runs through UIKit in a disposable iPad simulator app,
using the real compiled vectors and production `SharedIcon` view. All 216 flat
paint samples match exactly. The full 18 comparisons retain 27,869,184 pixels:
mean channel error 0.133/255, maximum 107/255, and exact differing fractions
0.92–2.41%. Exact pixel parity remains open. This adds UIKit component evidence;
it does not establish full-editor geometry, physical-device pixels or input.

Use the [Apple icon capture commands](../../tools/visual/README.md#shared-icon-paints)
for regeneration and comparison. Local evidence remains in ignored
`artifacts/apple-icons-current-v1`, `artifacts/apple-icons-uikit-v2` and
`artifacts/apple-tool-actions-icons-v1`. The isolated physical iPad editor also has
the updated icon bank; its ongoing row-input check is separate from icon parity.

## Reproduction

Build and run as described in [Android development](../development/android.md) and [Web development](../development/web.md).

```sh
cargo test -p layer-ui --lib
python3 apps/layer-apple/tests/test_icon_assets.py
# Android: assembleDebug, assembleDebugAndroidTest, lintDebug; install with adb install -r.
# Instrument art.capycanvas.AndroidIconTest and art.capycanvas.AndroidIconEditorTest.
# Icon-only captures: files/validation/icons and files/validation/icon-editor.
node apps/layer-web/test.mjs --icons
node apps/layer-web/test.mjs --editor
node apps/layer-web/test.mjs --medium-tiles
```

`--icons` first traverses every category, preset, non-painting mode, and filter in the production DOM in both themes. It checks glyph identity, visibility, fit, selection, and retained previews, then checks all 62 command identities and mounted SVG geometry/accessibility and captures every asset in 18 combinations: 16/24/32, light/dark, normal/accent/disabled. Android uses an isolated Compose activity for the complete bank and an isolated workspace for actual category/preset/mode/filter traversal and toolbar bounds. It does not edit a user document. Transform glyphs are inspected in their toolbar and Move-tool context; transforms stay disabled on the deliberately blank fixture. Lasso Fill is reached through its layer-tool action. Retain full-image captures alongside flat-paint checks; edge rasterization differences are not proof of misalignment.

Local evidence and actual results are recorded in `artifacts/icon-audit/`. Tablet browser automation requires access to its Chrome debugging session; desktop browser results do not establish Android Chrome coverage.

## Validation results

Second-pass evidence lives in `artifacts/icon-audit/redo/`; first-pass evidence remains in the parent directory.

- Shared Rust UI: 304 tests passed, including medium identity through categories, subtools and customization, distinct mode icons, and bundled filter/catalog asset coverage.
- All 157 SVGs pass the shared native paint exporter; its six existing tests passed.
- Android arm64 app and test APK build and lint pass. The isolated 157-icon device fixture passes at 16/24/32 dp in light/dark, normal/accent/disabled states.
- Graphical Chrome with hardware WebGPU: 282 production-control checks in two themes passed; 62 commands, 152 mounted icons, and 157 SVGs in 18 render cases passed. The editor workflow suite passed, including tool controls, drawers, project save/open, export, cancellation, and persistence.
- Wacom MovinkPad 14: the full production control traversal passed in both themes, including all 13 painting categories and 24 presets, available non-painting modes, all 40 filter rows, and toolbar size/centering at 16/24/32 dp. The separate foreground-alpha/fixed-paint regression passed, as did first-tap toolbar/drawer interaction.
- Matched native/Chrome grids: 216 flat-paint samples passed (maximum channel error 1/255). The 18 full comparisons retain 21,337,344 pixels: 454,164 differ exactly, with mean absolute channel error 0.231/255. Exact raster parity is not claimed.
- Final medium, medium-labeled and large-labeled toolbar scenarios passed across dock edges, floating controls, drawers, Zen and reload persistence. The final APK was installed with `adb install -r`; app data was retained. APK/SVG hashes and result paths are in `artifacts/icon-audit/redo/delivery.json`.

The final native app is open on the tablet for review. The refreshed web preview is available in its Chrome browser through USB forwarding.

Web coverage here is desktop graphical Chrome. Android Chrome automation remains unavailable: automatic approval review rejected DevTools forwarding because it could expose other browser sessions. The tablet preview is served through USB reverse forwarding; native tests use isolated stores and UI-only captures.

## GTK migration

GTK consumes the same approved bank and Rust icon identities. Categories, preset
captions, tool action buttons, filter captions and category headings now display
their specific glyphs. Preset/filter GPU previews remain present. New Layer,
gradient reset, workspace actions and collapsed-column chevrons match their
Android/web counterparts. Native dialogs keep their toolkit controls.

All shared icons use cached `GtkSvg` paintables at the requested size. GTK's
traditional symbolic loader ignores explicit fill/stroke paints; using
[GtkSvg](https://docs.gtk.org/gtk4/class.Svg.html) with symbolic foreground paint
servers preserves the canonical transforms, group opacity, fixed swatches and
drawing order. The color toolbar explicitly binds its two swatches to the live
foreground/background palette. Unknown external icon names retain GTK's native
lookup fallback. This requires GTK 4.22, documented in the
[Linux guide](../development/linux.md).

The native audit uses a private Mutter compositor, hardware Vulkan and fresh
settings/workspace storage. `--icons` traverses all 13 painting categories,
24 presets, non-painting modes, 40 filters and 7 filter categories in both themes
(280 control checks). Every preset and filter is scrolled into view; individual
preset captures supplement the visible panel captures. Toolbar checks cover
16/24/32-pixel glyphs, centering and live foreground/background swatches. The
157-asset matrix adds 18 combinations of theme, size and normal/accent/disabled
state at each monitor scale.

```sh
bash tools/performance/workspace-motion.sh gtk --icons
LAYER_MOTION_SCALE=2 LAYER_MOTION_VIEWPORT=2400x2000 \
  LAYER_TEST_ARTIFACTS="$PWD/artifacts/icon-audit/gtk-2x" \
  bash tools/performance/workspace-motion.sh gtk --icons
bash tools/performance/workspace-motion.sh gtk --drawer-style
```

The runner configures the private monitor through Mutter and the fixture asserts
the actual GTK scale; `GDK_SCALE` alone did not establish a 2× Wayland monitor.
Evidence is in `artifacts/icon-audit/gtk/`, `gtk-2x/`, `gtk-drawers/` and the
`eyedropper/gtk-*.log` files. Full Chrome comparisons retain every pixel and show
rasterization differences at edges: mean absolute channel error is 0.390/255 at
1× and 0.258/255 at 2×. All 216 paint samples pass at 2×, with maximum channel
error 1/255. At 1× the generic sampling utility's narrow swatch-outline point
lands on an antialiased edge; its 10 failed samples are not flat interiors. The
native fixture's fixed-interior paint checks pass at both scales. Exact raster
parity is not claimed.

Focused GTK workflows cover tool/color controls, toolbar sizing and grip targets,
Zen icon selection, and applying/editing all 40 filters. The sizing test's old
one-lane assumption was reproduced against the pre-migration commit, then
replaced with native allocation checks against the shared wrap projection.
Filter tests activate tabs only when needed, wait for preview readiness, and
activate Diagnostics before asserting telemetry. Icon changes preserve the
original grip allocations and input handlers.

Final checks passed: the release build and GTK unit tests, native icon audits at
1× and 2×, tool/color controls, toolbar sizing, Zen selection, all filter editing
workflows, and the drawer/toolbar pointer suite in both themes.

## Toolbar numeric fields — 2026-09-23

Numeric fields use the shared `tool_setting_icon` mapping. The bank adds 22
16px vectors, using the existing foreground paint and 1.5px contour weight.
These distinguish parameter meanings from the tools that happen to expose them:

| Fields | Glyphs / meaning |
| --- | --- |
| Brush size, size variation | Diameter dimension / differently sized stamps |
| Flow, hardness, spacing | Paint moving / crisp versus soft edge / separated stamps |
| Angle, rotation variation | Measured angle / varying stamp rotations |
| Paint load, water load, dilution | Filled paint drop / water drop / mixing drops |
| Wet bleed, dry bleed | Liquid versus dry pigment moving outward |
| Edge strength, edge width | Nested edge / measured boundary thickness |
| Width, height, ratio width/height | Horizontal / vertical dimensions |
| X, Y position | Point relative to the respective coordinate axis |
| Feather radius, smoothing, gap closing, expansion | Feather / smoothed contour / bridged gap / outward boundary |
| Strength | Gauge |

Opacity, texture, color pickup, bleed distance and tolerance retain the existing
opacity, grain, eyedropper, ruler and color-selection glyphs. Core coverage checks
all published brush, region, selection and transform fields against the bank.
The GTK icon audit renders every packaged glyph at 16/24/32px in both themes.
