# GIMP, Krita, Photopea, Pixelmator Pro and Procreate: journeys, friction and tool matrices

[Photo editing research](../photo-editing-research.md) · source report, 2026-09-25 · baseline `dac76c20`

Web research gathered by an agent on 2026-09-25. Items with a link come from that source; view counts come from the Stack Exchange API. Reddit and some vendor pages refused automated access. Treat rankings as product judgment, not usage statistics.

---

## How I measured demand
- **View counts** came from the Stack Exchange API. I took the top 500 `gimp`-tagged questions on graphicdesign.SE and on superuser, plus the Photography.SE `gimp`, `photoshop` and `retouching` tags, and sorted them by views. I also ran keyword searches (clone, heal, warp, and so on).
- **Issue trackers:** GIMP GitLab and Photopea GitHub, sorted by popularity, reactions and comments.
- **Forums and docs:** discuss.pixls.us, gimp-forum.net, krita-artists, the vendor handbooks and release notes.
- **Blocked sources:** Reddit, photopea.com/learn, blog.photopea.com and Adobe helpx were blocked (403 or Cloudflare). Capability cells for those tools rely on search snippets, issue threads and my own knowledge of the apps. Those cells are marked † in the matrices.
- **Date:** everything is as of September 2026. Current versions are GIMP 3.2, Krita 5.3/6.0 and Pixelmator Pro 4 (Apple Creator Studio).

---

## (a) Ranked "how do I…" journeys, with evidence

View counts are SE views. "GD" is graphicdesign.SE, "SU" is superuser, "PH" is photo.SE.

1. **Remove a background or make it transparent** (including colour-to-alpha). This is the largest by far.
   - GD "Making the background of an image transparent in Gimp": **1.64M** views. https://graphicdesign.stackexchange.com/questions/5446
   - "Add transparency to an existing PNG": 243k. /questions/6449
   - "How to make a color transparent": 177k. /questions/36520
   - Photoshop "remove a specific color": 827k. /questions/32041
   - Photoshop "whiteness → transparency": 754k. /questions/21685
   - Sub-failures: "Color to alpha is not selectable" 80k + 50k, because the layer has no alpha channel (/questions/28058, /questions/113445). "PNGs not coming out transparent": 94k (/questions/39014).
2. **Select part of an image and move it** (the floating-selection problem).
   - SU "How do I use the Selection tool to move things in GIMP?": **294k**. https://superuser.com/questions/279725
   - GD "How to move a selection in a layer": 89k (/questions/25296).
   - "Moving a part of an image": 63k (/questions/5708).
   - SU "select and move an object": 54k.
3. **Copy/paste into a new layer or across documents** ("layer via copy").
   - Photoshop "copy layers from one doc to another": **1.01M**. /questions/1587
   - SU "copy a selection from one image to another": 60k.
   - GD "Gimp Pasted Layer Move Tool": 43k (/questions/13465).
   - "Disable floating layer mechanism in GIMP": 8.6k (/questions/55413). The poster calls the float "annoyingly contrived".
   - SU "Copy and paste copying empty space": 49k.
   - The same confusion runs the other way in Krita: "Copying a selection without adding a new layer": 41k (/questions/62039).
4. **Composite, overlay or put images side by side.**
   - GD "combine two images side-by-side": 180k (/questions/83446).
   - "Using an image as the transparency layer of another": 207k (/questions/8397).
   - PH "overlay two images exactly by scaling": 27k.
5. **Crop**: a single layer, to an aspect ratio, to content, or into tiles.
   - PH "How can I crop a single layer in Photoshop?": **422k**. https://photo.stackexchange.com/questions/30956
   - GD "GIMP: How to crop a layer": 80k.
   - "Crop a big picture into several small": 148k.
   - SU "Split image to multiple pages": 125k.
   - "Throw away outside transparent pixels" (crop to content): 33k.
   - Krita "fit canvas to the actual image": 17k.
6. **Resize or resample without losing quality, or export at a given size.**
   - Photoshop "increase size of JPEG without losing quality": 213k.
   - GD "Scale down without losing resolution": 95k.
   - SU "increase image resolution": 76k.
   - "Resizing image interactively": 66k.
   - "Export at specific dimensional size": 32k.
7. **Canvas size vs. layer size.**
   - GD "resize canvas and background layer": 87k.
   - SU "Fit Layer to Canvas Size": 44k.
   - "change size of transparent background": 34k.
   - GIMP GitLab #32 "Allow layer boundaries to be adjusted automatically" (30 notes). GIMP 3.0 added the "Expand Layers" paint option.
8. **Adjust only one layer; adjustment layers; non-destructive editing.**
   - Photoshop "adjustment layer affect only one layer": **406k** + 394k (/questions/2677, /questions/38916).
   - "adjustment layer that blurs all layers below": 236k.
   - PH "adjustment layer only to one layer": 84k.
   - GIMP "Level adjustment as layer, like Photoshop?": 27k (/questions/120401).
   - New for GIMP 3: "add a mask to the new non-destructive layer effects?" (/questions/167081).
9. **Layer masks**: from a layer or image, from a selection, applied to multiple layers.
   - Photoshop "create a layer mask from a layer": 265k.
   - Krita "add imported image to a layer mask": 58k (its top Krita question).
   - GIMP "apply a mask to multiple layers": 18k.
10. **Remove an object, person or text** (content-aware fill).
    - GD "replicate Photoshop's content-aware fill in GIMP for Windows": 26k (/questions/97525).
    - "Remove text from background": 44k.
    - PH "remove sweat spots": 66k.
    - "blend multiple photos to remove people": 32k.
    - GIMP GitLab #4762 "Add Heal Selection as a native feature" is the top heal issue (16 upvotes). It was closed in October 2025 with the label "Third-party Plug-ins". https://gitlab.gnome.org/GNOME/gimp/-/work_items/4762
    - Photopea #5516 "New Photoshop Remove Tool" has 59 comments.
    - A Lightroom request for "Photoshop-like clone/heal/content-aware brushes" has 237 replies.
    - Procreate Folio has a "Healing brush and stamp tool" request. https://folio.procreate.com/discussions/3/6/25844
11. **Perspective correction**: keystone, or straightening a document to a rectangle.
    - Photoshop "change the perspective of an image": **272k**.
    - GD "fix photo perspective and crop to a perfect rectangle": 18k (/questions/137304).
    - Photopea #5059 (21 comments) asks for Perspective Warp.
    - GIMP #2026 asks for H/V perspective correction.
12. **Change one colour to another, keeping feathered edges.**
    - SU "Change one colour to another": **304k**.
    - "swap colors": 130k.
    - GD "change color of object preserving feathering": 24k + 12.5k.
    - "colorize using reference color": 38k.
13. **Fade or feather edges, gradient to transparent, partial opacity.**
    - GD "Make image partially transparent": 174k.
    - SU "one side fade to transparent": 88k, plus 39k.
    - GD "blurring the edges of a layer": 68k.
14. **Annotate**: arrows, boxes, straight lines.
    - SU "draw a box in GIMP": 230k.
    - GD "insert arrows": 150k + 34k.
    - "straight line": 49k.
    - SU "stroke a path": 36k.
15. **Outline, stroke or border.**
    - GD "Make an outline with GIMP, like in Photoshop": 126k.
    - "add a simple border": 106k.
    - Krita "stroke effect like Photoshop": 5k.
16. **Black-and-white, threshold and scan cleanup.**
    - Photoshop "single layer to grayscale": 280k.
    - "pure black and white": 195k.
    - GD "old scanned documents into black and white": 68k.
    - PH "remove texture from scanned textured photo paper": 122k.
17. **Sharpen, or fix blur.**
    - GD "How to sharpen text in GIMP": 104k.
    - PH "fix an out-of-focus photo": 127k.
18. **Brighten part of a picture** (local exposure, dodge/burn).
    - GD "brighten a part of a picture with Gimp": 45k.
    - PH "Exposure setting in GIMP like Photoshop": 30k.
    - PH "edit exposure of two areas with smooth transition": 2.6k.
19. **Batch-apply an edit or record actions.**
    - SU "batch convert images": 176k.
    - GD "record action for automatic repeat": 59k.
    - PH "apply same processing to multiple photos": 50k.
20. **Save vs. export, formats and layers to files.**
    - Photoshop "can't save as PNG": 410k; "Can't save PSD as JPEG": 175k.
    - SU "Export layer to png file": 70k.
    - Krita "PNG or JPEG not saving": 14k.
    - GIMP's Save/Export split still generates tutorials. https://thegimptutorials.com/how-to-save-gimp-file-as-jpeg/
21. **Position layers numerically, nudge, align or centre.**
    - SU "Move a layer to specific X,Y": 112k.
    - GD "directly set layer positions": 66k.
    - SU "move layers one pixel with arrow keys": 15k.
22. **Select multiple layers.** GD "How to select more layers with GIMP?": 300k. GIMP 3.0 fixed this.
23. **Select by colour, magic wand, select subject.**
    - Photoshop "select all pixels matching an exact colour": 278k.
    - GD "stop Fuzzy Select from over-selecting": 10.6k.
    - Photopea issues for Select Subject and Remove Background mostly concern outages and ad-gating (#6031, #7645).
24. **Curve text or put text on a path.**
    - GD "curve text in an arc": 50k.
    - SU "style text along path": 12.8k.
    - gimp-forum "How Do I curve text?" https://www.gimp-forum.net/Thread-How-Do-I-curve-text
25. **Warp to a shape, mesh warp, wrap onto a sphere or cylinder.**
    - GD "warp an image to take shape of path": 32k.
    - PH "something like Photoshop's warp tool with open source": 25k.
    - GD "flat image to sphere": 23k.
    - pixls "GIMP equivalent of Photoshop's Transform-Warp": the user tried cage, warp and G'MIC and "can't get… the same speed and dexterity"; the answers said "use Krita's Mesh". https://discuss.pixls.us/t/looking-for-gimp-equivalent-of-photoshops-transform-warp/34923
    - GIMP #8017 "Mesh transform request" has been open since 2022, and there are duplicates (#12111, #15873).
26. **Spot heal and skin retouch.**
    - PH "remove shine from skin with GIMP": 31k.
    - GIMP #16124 "Healing tool looks like it is just cloning" (33 notes, closed as "Not a Bug").
    - pixls "Heal tool in GIMP very slow". https://discuss.pixls.us/t/heal-tool-in-gimp-very-slow/38466
27. **Match colour, white balance, vibrance.**
    - PH "Match Color in GIMP?": 54k.
    - "alter white balance": 27k.
    - "Vibrance equivalent": 21k. GIMP 3.2 finally added Vibrance.
28. **Noise reduction.** PH "filter away high ISO noise in Gimp": 25.5k.
29. **Panorama and auto-align.**
    - SU "Photomerge equivalent for Gimp?": 44k.
    - PH "Auto-Align plug-in: I. just. do. not. get. it.": 11k.
30. **View or edit EXIF.** GD 41k, SU 44k, PH 23k.
31. **Remove glare, shadows or reflections.**
    - GD "removing shadows on white background": 24k.
    - PH "flashes of light / glare": 38k.
32. **Restore old photos and clean scans.** SU "clean newspaper scan": 39k, plus the texture question in #16.
33. **Extend the canvas and fill the new edges.**
    - GD "extend image by adding blur to sides": 43k.
    - Photopea #4254: content-aware fill used to make an image taller shows seams and repeats.
    - GIMP #997 asks for content-aware fill on resize.
34. **Liquify, face and body reshaping.**
    - Photopea #3827 asks for a Liquify freeze mask; #5363 asks for Face-Aware Liquify.
    - GD "Photoshop Liquify alternative".
35. **Clone with scale, rotation or mirroring, or onto a separate layer.**
    - GD "GIMP Clone using different size, rotation, reflection": the poster must "duplicate, transform & re-select the clone source each time".
    - GD "heal an area… on a different layer".
    - GIMP #2079 asks for mirror cloning.
    - Photopea #5391 asks for a Clone Source panel.
36. **Puppet-warp a limb or object.**
    - GD "puppet warp equivalent in… Gimp?"
    - Photopea #1854 and #8114.
37. **A smart-object equivalent, or non-destructive transforms.**
    - GD "closest thing to Photoshop's smart object?": 31k.
    - pixls: a Recursive-Transform hack is used for NDE transforms in GIMP 3. https://discuss.pixls.us/t/non-destructive-transform-with-recursive-transform-gimp-3-0-4/55051
    - GIMP #13312. GIMP 3.2 added link layers.
38. **Red eye, including pets.** PH "dog's wrong colored eyes in flash photos": 21k.
39. **Blend two exposures or diff two images.**
    - GD "difference between two images as transparent": 63k.
    - PH "Double exposure with Gimp": 11k.
40. **Straighten a horizon or rotate by an arbitrary angle.** Low Q&A volume, probably because it is easy in most tools. It is still table stakes; GIMP's Measure tool has a "Straighten" button.

---

## (b) Friction points and power-user details that separate pleasant tools from painful ones

**Paste, float and layer targeting (the most-viewed GIMP pain)**
- GIMP 3.0 now pastes as a new layer. It floats only when pasting into a layer mask. https://developer.gimp.org/core/specifications/copy-paste/
- Floating layers still appear when you transform or move selected pixels. The Anchor (Ctrl+H) / To New Layer (Shift+Ctrl+N) state remains a trap.
- Photoshop users expect **Ctrl+J Layer via Copy** and **Ctrl+Shift+J Layer via Cut**, both in place.
- Krita users hit the opposite problem: paste always creates a layer (41k views).
- Lesson: offer explicit, well-named verbs. These are paste-as-layer, paste-in-place, paste-into-selection-as-masked-layer, and paste-into-current-layer.
- Also keep offsets on round-trips. GIMP's Paste in Place uses the copy-time offset, and external clipboard data lands at (0,0).

**Hidden state causes "tool silently does nothing"**
- Top GIMP threads come from invisible state:
  - no alpha channel, so erasing paints white and Color-to-Alpha is greyed out;
  - grayscale or indexed mode, so only black paints (64k, /questions/85548);
  - locked alpha, or the bucket shows a circle-slash (64k);
  - the Move tool grabs the layer under the cursor, not the active one (117k and 100k);
  - the selection outline persists (83k);
  - the layer boundary is smaller than the canvas.
- Lesson: when a tool cannot act, say why at the cursor and offer the fix ("Layer has no alpha — add?").

**Non-destructive editing (NDE)**
- **GIMP 3 filters are an fx stack on the layer.**
  - Re-edit, toggle, reorder and merge work.
  - One mask covers all effects on that layer. A developer said per-effect "editable masks is on our TODO list… one of the big regrets". https://discuss.pixls.us/t/nde-layer-workflow-in-gimp-3-0-masks/48935
  - Users duplicate the layer once per effect as a workaround.
  - GIMP 3.2 extended NDE filters to groups and channels. A pass-through group with filters acts as an adjustment layer. https://www.gimp.org/release-notes/gimp-3.2.html
  - Filters with an auxiliary input are still destructive (#11904).
  - GIMP 3.0 removed the menu icons that showed which filters are GEGL-based, so users can no longer tell NDE-capable filters from destructive ones. https://www.gimp-forum.net/Thread-Non-destructive-editing-in-Gimp3
- **NDE-by-default surprises users.** Blurring a Quick Mask needs "Merge Filters" in 3.x (https://discuss.pixls.us/t/quickmask-gimp2-feature-gone-in-gimp3/55484). Applying a mask with an un-merged filter changes the layer (#16182).
- **Pixelmator Pro:** using Repair bakes the layer's colour adjustments permanently. https://www.pixelmator.com/community/viewtopic.php?f=16&t=15854
- **Photopea:** the error "Smart Objects must be rasterised first" still blocks tools (issue #2381).
- **Procreate:** adjustments are destructive per layer. "Pencil" mode paints an adjustment locally, and there are Folio requests for adjustment layers (https://folio.procreate.com/discussions/3/6/13111, /44462).
- **Krita's model is the reference to copy.** Filter layers, filter masks, transform masks and clone layers each carry their own mask.

**Clone, heal and repair details**
- **Source setting relies on a modifier**, Ctrl-click in GIMP and Krita or Alt-click in Photoshop.
  - This hurts on tablets and touch. Krita "Duplicate Brush… (on a tablet)" has 19k views, and GD "Ctrl-Left-Click operations in GIMP on Mac OS X" has 4k.
  - Procreate uses a draggable source disc, with press-and-hold to lock it. https://help.procreate.com/procreate/handbook/adjustments/adjustments-clone
- **Alignment modes:**
  - GIMP: None, Aligned, Registered (layer-to-layer) and Fixed. https://docs.gimp.org/3.0/en/gimp-tool-clone.html
  - Photoshop: an Aligned toggle.
  - Pixelmator Pro: "Fix source position".
  - Krita: move the source per dab or per stroke, and reset before each stroke.
- **Sample scope:**
  - Photoshop: Current / Current & Below / All Layers, plus "ignore adjustment layers".
  - GIMP: Sample merged.
  - Krita: "Clone from all visible layers".
  - Pixelmator Pro: "Sample all layers".
  - Healing onto an empty retouch layer is the professional workflow, and it is often broken. See Photopea #3713 and #2508, and GD "heal… on a different layer".
- **Source overlay and transformed source:**
  - Photoshop's Clone Source panel offers 5 sources, offset, W/H scale, angle, flip, and an overlay with opacity, Clipped, Auto-Hide, Invert and blend mode. https://helpx.adobe.com/photoshop/desktop/repair-retouch/heal-clone/clone-source-panel.html
  - GIMP and Krita have no overlay, scale or rotate. Krita shows the source only if the cursor is set to brush outline.
  - Photopea added a clone preview (#1372), but a Clone Source panel is still requested (#5391).
- **Performance:** GIMP Heal crawls at brush sizes above about 100px on 6MP images because it runs on a single core. Raising spacing from 1 to 2 fixes it ("like magic"). Heal should be GPU and tile based.
- **Quality of the algorithm matters more than having the tool.**
  - Photopea's maintainer on content-aware fill: "I don't know how to make it better" (#4254).
  - Photopea's Puppet Warp Rigid/Distort modes behave identically: "I don't really know how to make Photopea produce the result" (#8114).
  - Krita Smart Patch is "much slower than Adobe's… too much blurring". https://www.creativebloq.com/reviews/krita
  - G'MIC Inpaint needs the hole painted a specific red, and the preview doesn't match the result. https://patdavid.net/2014/02/getting-around-in-gimp-gmic-inpainting-content-aware-fill/
- **Heal-mode semantics confuse users.** Photopea users want a "Replace" mode that keeps luminance (#4865). GIMP heal "looks like it is just cloning" (#16124).

**Transform details**
- **Commit and cancel:** Enter/Esc, plus clear on-canvas commit buttons, which matter on touch.
- **Reference point:** a 9-point pivot grid in Photoshop and Krita; a draggable pivot in GIMP.
- **Numeric entry** for X, Y, W, H, angle and skew, with a linked aspect ratio.
- **Interpolation choice:**
  - Photoshop: Nearest, Bilinear, Bicubic variants, Preserve Details.
  - GIMP: None, Linear, Cubic, NoHalo, LoHalo.
  - Procreate: Nearest (the default), Bilinear, Bicubic. https://help.procreate.com/procreate/handbook/transform/transform-interpolate
  - Krita: a filter dropdown.
- **Multi-layer and selection transforms**, plus "transform again".
- **No quality loss from repeated transforms**, via smart objects or transform masks.
- **Extras in GIMP:** a Corrective (backward) direction for undoing perspective, and clipping modes Adjust / Clip / Crop to result / Crop with aspect. There is also a preview-opacity slider, composition guides, and a Readjust button that resets handle sizes at the current zoom. https://docs.gimp.org/3.0/en/gimp-tools-transform.html
- **Modifier semantics are sacred.** Photoshop CC 2019 made proportional scaling the default, and the backlash forced a "Legacy Free Transform" preference. https://jkost.com/blog/2019/06/new-free-transform-preference-in-photoshop.html
- **Unified-transform handles are hard to discover** in GIMP (square, diamond and outlined-diamond handles). GIMP #4925 asks to be able to move guides while a transform is active.

**Selection and export**
- Users want to edit an existing marquee (25k views), set exact size and ratio (31k), see live selection dimensions (44k), and move the outline without the pixels (7.7k).
- GIMP's save/export split guards the layered file but confuses newcomers. Offer "Export" with clear format warnings, plus quick re-export.

---

## (c) Transform-mode matrix

Legend: ✓ native · ~ partial or workaround · ✗ absent · † inferred (the source page was blocked).

| Mode | GIMP 3.2 | Krita 5.3 | Photopea | Photoshop | Procreate | Pixelmator Pro 4 |
|---|---|---|---|---|---|---|
| Free/unified (scale, rotate, move) | ✓ Unified (Shift+T), plus separate Scale/Rotate | ✓ Free (5.3 adds bounding-box rotation) | ✓ Ctrl+T | ✓ | ✓ Freeform/Uniform | ✓ |
| Skew/shear | ✓ Shear tool and unified handles | ✓ | ✓ | ✓ | ✗ (only via Distort) | ✓† |
| Distort (corner pin) | ✓ Perspective / Handle Transform (1–4 handles) | ✓ Perspective | ✓ | ✓ | ✓ Distort | ✓ Distort† |
| Perspective | ✓, plus **Corrective** direction to un-keystone | ✓ (vanishing points) | ✓, plus Perspective Crop | ✓ | ~ via Distort | ✓† |
| Perspective Warp (multi-plane un-keystone) | ~ Corrective perspective handles one plane only | ✗ | ✓ recently; 2026 bug reports #8727, #8926 | ✓ | ✗ | ✗ |
| Warp presets (Arc, Flag, Bulge…) | ✗ | ✗ | ✓ presets, plus "Grid Warp" split† | ✓ 16 presets incl. Cylinder | ✗ | ✓ 12 presets + H/V/Bend sliders (Creator Studio only) |
| Mesh warp with grid density | ✗ (#8017 open since 2022) | ✓ Mesh: Bézier patches, rows/cols, split | ~ fixed 4×4 with splits† | ✓ Default/3×3/4×4/5×5/custom + split crosswise/V/H | ✓ Warp + "Advanced Mesh" | ✓ 3×3/4×4/5×5 + splits |
| Point warp (MLS) | ✗ | ✓ Warp: Rigid/Affine/Similitude, flexibility | ✗ | ✗ | ✗ | ✗ |
| Cage | ✓ (artifact history, #5267) | ✓ (preview/real granularity) | ✗† | ✗ | ✗ | ✗ |
| Puppet | ✗ | ✗ (Cage is the closest) | ✓ (modes don't differ, #8114) | ✓ (Rigid/Normal/Distort, density, expansion, pin depth) | ✗ | ✗ |
| Liquify | ✓ Warp Transform tool: move/grow/shrink/swirl/erase/smooth, abyss policy | ✓ Liquify mode: move/scale/rotate/offset/undo, wash/build-up (much faster in 5.3) | ✓ Filter > Liquify; no freeze mask (#3827) or face tool (#5363) | ✓ incl. freeze/thaw, Face-Aware | ✓ 7 modes incl. Reconstruct, Momentum | ✓ Warp/Bump/Pinch/Twirl |
| 3D rotate | ✓ 3D Transform | ~ (Free transform X/Y rotation) | ✗† | ✗ | ✗ | ✗ |
| Content-aware scale | ✗ (Liu Rescale plug-in) | ✗ | ✓ (#5200) | ✓ | ✗ | ✗ |
| Interpolation choice | ✓ 5 options | ✓ filter list | ✓† | ✓ | ✓ 3 options | ~† |
| Numeric entry / reference point | ~ numeric per tool dialog, draggable pivot; Unified shows only a matrix | ✓ numeric fields + 9-point anchor | ✓† | ✓ | ✗ (snapping only) | ✓ |
| NDE transform | ~ Recursive-Transform hack; 3.2 link/vector layers | ✓ transform masks | ✓ smart objects / puppet as smart filter | ✓ smart objects | ✗ | ~† |

Sources: Krita https://docs.krita.org/en/reference_manual/tools/transform.html · GIMP https://docs.gimp.org/3.0/en/gimp-tools-transform.html · Pixelmator Pro https://support.apple.com/en-au/guide/pixelmator-pro/md49514rvgv2/mac · Procreate https://help.procreate.com/procreate/handbook/adjustments/adjustments-liquify

---

## (d) Retouch-tool matrix

| Tool | GIMP 3.2 | Krita 5.3 | Photopea | Photoshop | Procreate | Pixelmator Pro |
|---|---|---|---|---|---|---|
| Clone | ✓ 4 alignment modes, sample merged, pattern source, cross-image; no overlay/scale/rotate | ✓ Clone brush engine: all visible layers, per-dab/per-stroke source; perspective option disabled | ✓ with preview; no Clone Source panel | ✓ + Clone Source panel (5 sources, scale/rotate/flip, overlay) | ✓ Adjustments > Clone: disc source, lock, any brush | ✓ sample all layers, fix source, source marker |
| Perspective clone | ✓ (two-step: modify perspective, then clone) | ✗ (disabled) | ✓ Vanishing Point | ✓ Vanishing Point | ✗ | ✗ |
| Healing (sampled) | ✓ Heal (slow at large sizes) | ✓ "Healing" checkbox on the clone brush | ✓ (bugs on empty layers) | ✓ diffusion, modes, sample scope | ✗ | ~ Repair |
| Spot heal (no source) | ✗ natively | ~ Smart Patch (slow, blurry) | ✓ + "Heal Selection" / "Remove with AI" buttons | ✓ Content-Aware / Create Texture / Proximity | ✗ | ✓ ML Repair brush, can target an empty layer |
| Patch | ✗ | ✗ | ✓ | ✓ Normal/Content-Aware | ✗ | ✗ |
| Content-aware fill | ~ Resynthesizer "Heal Selection" (third-party, ported to GIMP 3) and G'MIC Inpaint | ~ Smart Patch | ✓ Edit > Fill; weak on large fills (#4254) | ✓ + CAF workspace (sampling brush, colour/rotation/scale/mirror adaptation) | ✗ | ✓ Repair |
| Content-aware move | ✗ | ✗ | ✓ (#8968 bug) | ✓ | ✗ | ✗ |
| Red eye | ~ filter that needs a selection | ✗ | ✓† | ✓ | ✗ | ✓† |
| Dodge/Burn | ✓ (range + exposure) | ~ Filter brush / Dodge-Burn filters | ✓† | ✓ (range, exposure, protect tones) | ~ Pencil-mode adjustments | ✓ Lighten/Darken† |
| Sponge | ✗ | ~ Filter brush | ✓† | ✓ (vibrance option) | ~ | ✓† |
| Smudge | ✓ | ✓ Colour Smudge engine | ✓ | ✓ | ✓ | ✓ |
| Blur/sharpen brush | ✓ Convolve | ~ Filter brush | ✓ | ✓ | ~ Pencil-mode | ✓ Soften/Sharpen† |

Sources: Krita clone https://docs.krita.org/en/reference_manual/brushes/brush_engines/clone_engine.html · Krita Smart Patch https://docs.krita.org/en/reference_manual/tools/smart_patch.html · Resynthesizer for GIMP 3 https://github.com/bootchk/resynthesizer/wiki/Installing-Resynthesizer3-Plugins · Pixelmator Pro https://support.apple.com/guide/pixelmator-pro/repair-remove-and-clone-objects-in-images-pixc5f9d789e/mac and https://www.pixelmator.com/pro/retouching/ · Photopea issues https://github.com/photopea/photopea/issues

---

## Implications for Capy Canvas (short)
1. **Top differentiators on the evidence:**
   - Background removal and colour-to-alpha.
   - Obvious move/copy of selected pixels, with no float state and Ctrl+J-style verbs.
   - Per-layer adjustments with their own masks. This is Krita's model, and the one GIMP still lacks per effect.
   - Crop a single layer, or crop to content.
2. **A GPU painting app can win on retouch feel:**
   - brush-based heal, spot heal and content-aware fill that run fast at large radii;
   - a live, clipped source overlay with scale, rotate and flip;
   - sample scope "current & below" onto an empty layer;
   - modifier-free source setting for pen and touch, such as Procreate's disc.
3. **Transform gaps worth closing:** GIMP users go to Krita for mesh warp. Photoshop-style warp presets, puppet warp and multi-plane perspective warp are the other common reasons people leave free tools. Numeric entry, pivot, interpolation choice and NDE transforms (masks or smart layers) are table stakes.
4. **Guard against invisible-state failures:** missing alpha, locked alpha, wrong layer, and NDE filters not merged into masks. Explain the cause in context.
5. **Algorithm quality is the moat.** Photopea's maintainer has openly stalled on content-aware fill and puppet-warp modes, even though the UI for both exists.
