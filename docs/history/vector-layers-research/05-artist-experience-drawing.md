# How artists actually use vector drawing, and what that means for the new-artist journey in Capy Canvas

[Vector layers research](../vector-layers-research.md) · source report, 2026-09-25

Researched 2026-09-25. Sources: about 60 Reddit threads with their top comments (r/ClipStudio, r/AdobeFresco, r/ProCreate, r/krita, r/ConceptsApp, r/PaintToolSAI, r/IbisPaint, r/Inkscape), about 80 YouTube tutorials (view counts scraped in September 2026) and their top comments, CLIP STUDIO TIPS/ASK, Adobe Community, Krita-Artists and vendor docs. YouTube transcripts were not available; I used chapter lists, descriptions and comments instead. The Reddit archive API hit a rate limit partway through, and r/webcomics, r/DigitalArt and r/learnart returned nothing. Comic and webtoon evidence therefore comes mostly from CSP sources.

---

## 0. Summary

- **"Vector" means two different things to users.** To illustrators and comic artists it means *editable ink*: pressure-sensitive brush strokes that stay editable afterwards (CSP "vector layer", SAI "linework layer"). To designers and hobbyists who cut or print, it means a *scalable file* (SVG/PDF/EPS for Cricut, screen print, logos). Most frustration comes from one tool promising both and delivering one.
- **The #1 "aha" in every popular tutorial is the same: erase an overlap up to the intersection.** Examples are CSP's Vector Eraser "Erase up to intersection" and Fresco's "Vector Trimming". Next come "swap the brush or colour of already-drawn lines", "thicken or thin a line after drawing" and "fill against the ink layer".
- **Discoverability is the biggest adoption blocker.** The official CSP vector video (1.3M views) has thousands of likes on comments like *"I've been using Clip Studio Paint like an idiot"* (1.2K) and *"You mean, I've been using CSP for nearly a year without having this information?"* (1.2K).
- **Recurring pain points:**
  - A normal eraser on a vector layer creates invisible transparent strokes.
  - The fill tool refuses to work on vector layers, and vector fills leave hairline gaps.
  - Vector lines look pixelated when zoomed in, so people ask whether it is "fake vector".
  - Lines look "sterile/too clean".
  - Lag with many hatching strokes.
  - Export loses line width (CSP SVG), or there is no SVG export at all (Fresco).
- **This is an opening for a Linux-first app.** CSP has no Linux build, and Krita's SVG vector layers are widely called "useless for most line art". Pressure-sensitive editable ink is a known, long-requested Krita gap.

---

## 1. Workflows artists actually follow

### A. Illustration line art in CSP (the canonical workflow)
1. **Sketch on a raster layer.** Set the layer colour to light blue or lower its opacity (ShannonJin TIPS, 42K views: https://tips.clip-studio.com/en-us/articles/3875; kmcarroll: https://kmcarroll.substack.com/p/art-process-comic-page).
2. **Ink on a new vector layer** with G-pen, Real G-pen, Turnip pen or Mapping pen. Pressure stays on. Stabilization is typically 20–30 (*"between 20 to 30"*, ShannonJin). Artists with tremors go up to 100 with "adjust by speed" off (https://reddit.com/r/ClipStudio/comments/1rghikm/).
3. **Clean overlaps** with the Vector Eraser set to "Erase up to intersection". A common hotkey setup is holding Shift on the pen for vector erase (Loading Artist: https://tips.clip-studio.com/en-us/articles/3569).
4. **Refine the lines:**
   - Correct Line Width: Thicken/Narrow, or "Fixed width" plus "Process whole line"; one example uses a value of 5.0 (https://tips.clip-studio.com/en-us/articles/7504).
   - Simplify Vector Line.
   - Pinch Vector Line.
   - Control Point tool ("Switch corner").
   - Connect Vector Line (graphixly: https://graphixly.com/blogs/news/easy-inking-with-vector-layers).
5. **Optionally swap the look of the whole layer.** With the Object tool, select all lines and change the brush tip or colour. This is a big reveal in the comments (Shyfoxx: *"GLOBALLY CHANGE BRUSH ON VECTOR LAYER OMG"*).
6. **Flats on a raster layer below.** Mark the ink as a Reference Layer, then use the fill tool with "Refer other layers", "Close gap", Area scaling "to darkest pixel" or "Fill up to vector paths" (the centre line). The Lasso Fill and Enclose-and-Fill tools are also common (CSP fill guide, 840K views: https://tips.clip-studio.com/en-us/articles/590; Art of Nemo, 252K views).
7. **Colour and shade on raster.** Shading uses a Multiply layer clipped to the flats and highlights use an Add Glow layer. **The ink stays vector until the end**, and some artists rasterize only at export (https://reddit.com/r/ClipStudio/comments/jrk14w/).
8. **Colour the lines themselves** with a clipped raster layer over the ink folder, or with the "line colouring" auto-action (https://reddit.com/r/ClipStudio/comments/1uxije2/). Top answer: *"I do all the lineart vectors black and then I color the lines by putting a layer on top clipping over it."*

### B. Comic / manga page
- Panels come from the **Frame Border** tool. It creates vector borders and masked folders that are editable afterwards (Scott Drummond, 155K views: https://youtu.be/FptnmJzH1Rk).
- **Balloons are vector objects** and are reshaped with the line-correction tools. Text comes from the text tool or the EX Story Editor.
- **Speed and focus lines** come from rulers (Parallel/Radial/Focus) or the "Effect line"/"Saturated line" figure tools. A Krita-Artists wishlist asks for *"these vector speed lines, in radial form… They help in time gain in manga or webtoon illustration"* (https://krita-artists.org/t/clipstudiopaint-artists-what-features-does-csp-have-that-you-wish-were-in-krita/47080).
- **Blacks are spotted on raster.** One manga pipeline: *"Vector layer is the best way to ink, use raster layer for the large black spots, and to finalize, with just one click of tone effect… it will automatically convert to screentones"* (469 upvotes: https://reddit.com/r/ClipStudio/comments/1o2188h/).
- The kmcarroll page process runs: pencils → vector inks (*"You can't actually flood fill on a vector layer and I'm so thankful for that"*) → raster shading → flats → Multiply shadows → Add Glow → balloons.

### C. Webtoon
From ArtHacks_avi (TIPS 11685 and https://youtu.be/qDEcRDkF6_k):
1. Set up a long canvas and adjust its length as you go.
2. Create panels with the Frame Border tool, with all panels sharing common layers.
3. Ink on vector: *"Vector layers and advanced brush stabilization make smooth lineart much easier."* Use Correct Line Width to *"thicken areas in shadow and thin lines hit by light"*.
4. Put 3D figures or backgrounds underneath.
5. Set the lineart folder as the Reference layer and fill.
6. Block in shadows with Lasso Fill.
7. Letter with the Story Editor and vector balloons.

Webtoon artists value vector mainly because they can **resize and reuse characters between panels** without degrading the line.

### D. Fresco scalable line art for merch, stickers, t-shirts and Cricut
From Chris Piascik (https://youtu.be/Mmfu3IO3onw, 274K views; https://youtu.be/uISPMwqWOMY) and LOINDAFLOW (https://youtu.be/ZN6dLCsMlUA):
1. Sketch with a pixel brush.
2. Ink with a vector brush such as Basic Round. For monoline work, turn pressure and velocity dynamics off (*"consistent monoline effect"*).
3. **Draw long lines past the intersections, then Vector Trim.** Put the Touch Shortcut in its secondary state and swipe across the overshoots.
4. Fill with the vector paint bucket or with Paint Inside against the reference layer.
5. Add texture with vector-captured textures made in Adobe Capture (*"Authentic VECTOR Texture Hack"*).
6. **Export a PDF, then open it in Affinity or Illustrator** to get SVG/EPS, because *"Adobe Fresco doesn't export SVG files"* (https://youtu.be/_5wKyW_nx9k, 2026).

Pros say the appeal is drawing *"without having to wrestle with the pen tool… mastering the fine art of bezier curves."*

### E. The hobbyist workaround when the app has no vector tools (Procreate/ibisPaint → trace)
1. Draw in Procreate.
2. Export a PNG.
3. Run Image Trace in Illustrator, Vector Magic or Cricut Design Space's PNG→SVG converter, then clean up by hand.

Tutorials for this are huge: CharleyPangus, 369K views (https://youtu.be/RAVlY_7qGSM); "Vectorize your Procreate Logo in Seconds", 180K. People complain that *"auto image trace will come out jagged and awkward"* (https://reddit.com/r/ProCreate/comments/etghpa/). Another example is a Cricut request to vectorize a late mother's handwriting for stickers (https://reddit.com/r/ProCreate/comments/1opig31/).

### F. Concepts (industrial and product design, architecture, landscape)
- Work on an infinite canvas with scale, measure, snap, and perspective or isometric grids.
- Sketch with the "wire" pen or with pressure pens.
- Reshape with **Nudge** (*"as easily as reshaping a piece of string"*; https://concepts.app/en/tutorials/nudge-tool/).
- Cut with Slice, then Select, move and rescale.
- Export DXF or PDF to CAD.

Designers value *"stylized pen strokes and… flexibility to play around with it in vector form"* (https://concepts.app/en/lp/industrial-designers/). A landscape designer uses it for *"site surveys and scale drawings"* (https://reddit.com/r/ConceptsApp/comments/q9dxlj/).

### G. Animation
Artists choose vector for **consistency between frames**:
- *"I tried animating with raster tools, but my wobblyness was in the way"* (https://reddit.com/r/krita/comments/1opikyg/).
- Fresco: *"I wanted that smooth clean linework that you can only get with vector brushes"* (https://reddit.com/r/AdobeFresco/comments/1k7v5ag/).
- A CSP animator's wish: *"Mainly just the vector layer that supports raster brushes is all I'm itching for"* (Krita-Artists 47080).

---

## 2. The de-facto curriculum (popular tutorials)

View counts as of September 2026.

| # | Tutorial (creator) | Views | What it teaches first |
|---|---|---|---|
| 1 | CLIP STUDIO PAINT useful features: Drawing with vectors (CLIP STUDIO official) https://youtu.be/j4UopyLEIYU | 1.32M | Processing finished ink: erase to intersection, width correction |
| 2 | How to draw vector illustrations in Adobe Fresco (Ashleigh Green, short) https://youtu.be/6uptaqf2ofQ | 1.21M | Choose a vector brush, get crisp scalable lines, send to Illustrator |
| 3 | Concepts "Learn to Draw Part 1" (Concepts/Lasse Pekkala) https://youtu.be/TOZxfVp_fSc | 1.88M | Drawing fundamentals in a vector sketchbook |
| 4 | How to draw FAST using CSP's "secret features" (Loading Artist) https://youtu.be/Uel2DS8L9zA | 585K | 1) Vector layer + Pinch, 2) Vector erase to intersection, 3) modifier keys, 4) hotkeys |
| 5 | How To Make Vector Art on Your iPad (Chris Piascik, Fresco) https://youtu.be/Mmfu3IO3onw | 274K | Sketch → vector ink → **Vector Trimming** → client products |
| 6 | Clip Studio Paint – Fill your ENTIRE LINEART in a snap (Art of Nemo) https://youtu.be/JXYINe_fCYY | 252K | Fill tools against line art |
| 7 | HOW TO: Making Lineart – My 8 Favourite Tips (kuroshiro) https://youtu.be/7g9UTbqZGiU | 244K | Messy-friendly line-art habits in CSP |
| 8 | Everything you need to get started w/ Adobe Fresco (Ten Hundred) https://youtu.be/XRtNFJKXIgw | 239K | Pixel/Live/Vector brush families |
| 9 | Clean Line Art! CSP Inking Tips for Beginners (Whyt Manga) https://youtu.be/jHYl5URkfm8 | 216K | Stabilization, pen choice, clean ink |
| 10 | 6 Tools in CSP to Make CLEAN & EASY Lineart (Shyfoxx) https://youtu.be/rYY8eBwFVqc | 139K | Vector erase, Object tool, global brush swap, width correction |
| 11 | How to Make Vector Art in Adobe Fresco (Chris Piascik) https://youtu.be/s7QocBjPwbs | 135K | Rough sketch → vector art without Bezier curves |
| 12 | How to create art using vector tools in Adobe Fresco (Adobe) https://youtu.be/PwpB5pz3f0w | 120K | Vector brushes and shapes |
| 13 | How to: Vector Tools (CLIP STUDIO official) https://youtu.be/pcV5k-yirpw | 111K | Curve tool + Object tool control points |
| 14 | How to use vector layers for easy line art in CSP (Spiral Heart, short) https://youtu.be/jPwcIVR3ba4 | 149K | Vector eraser on intersections |
| 15 | Vector Drawing in Krita 4: Review and Intro (GDQuest) https://youtu.be/YVe8Lt43mUs | 310K | Krita's SVG tools and their weaknesses |
| 16 | How I turn My Procreate Drawings Into Vector Graphics (CharleyPangus) https://youtu.be/RAVlY_7qGSM | 369K | Trace a raster drawing in Illustrator |
| 17 | How to draw with vector brushes in Adobe Fresco (Anya Kuvarzina) https://youtu.be/ddOM8eETyDA | 76K | Vector brush basics |
| 18 | Inking and using Vector Layers in CSP (ElectricAlice) https://youtu.be/deb3wLpjQTg | 74K | Early vector inking walkthrough |
| 19 | Using vectors to ink comics! (Jake Hercy, CSP official) https://youtu.be/EIKrzky8R6o | 55K | Panels, balloons, brushes. Comments complain it barely covers vectors. |
| 20 | Motion and Vector Trimming in Fresco (Adobe) https://youtu.be/58PPCCKgZCY | 53K | Trimming |
| 21 | Easier Line Art Using Vector Layers in CSP (Oyun Orka) https://youtu.be/Exs7pA_L4k4 (and TIPS 8842, 33K) | 48K | **Vector vs raster first (8 minutes)**, then line detail, raster↔vector conversion, changing the brush tip |
| 22 | How to ink and COLOR with vectors in CSP (Dadotronic, official) https://youtu.be/TPUnyiJyO0M | 47K | All-vector workflow for scalable posters and icons |
| 23 | This Feels Like a Cheat Code! (Piascik, Vector Trimming) https://youtu.be/EbN6t3QJBKE | 39K | Trimming only |
| 24 | How to Export SVG from Adobe Fresco (Piascik, 2026) https://youtu.be/_5wKyW_nx9k | 12K | PDF → Affinity → SVG workaround |

Courses and books:
- Skillshare: Lisk Feng, "Creative Digital Illustration: Learn to Use Adobe Fresco".
- Domestika: Kyle T. Webster, Fresco animated illustration.
- LinkedIn Learning: "Designing Characters Using Adobe Fresco", with the lesson "Create crisp line art using vector brushes"; also Deke's Techniques #867, "Vector brushes in Fresco".
- Packt/Coursera, *Learn Clip Studio Paint*: Chapter 10 is "Vector Layers and the Material Palette", which places vector as an intermediate topic.
- Concepts' free "Learn to Draw" series.

**Implied curriculum order:**
1. What vector vs raster means ("won't lose quality when you resize").
2. How to make a vector layer and ink over the sketch.
3. **Erase or trim to intersection.**
4. Thicken or thin lines, and simplify them.
5. Reshape by grabbing lines (Pinch/Nudge/control points).
6. Swap the brush or colour of existing lines.
7. Fill on a separate layer referencing the ink.
8. Export and scaling, taught much later and mostly by Fresco and merch creators.

---

## 3. Favourite tools and settings

**CSP:**
- **Vector Eraser → "Erase up to intersection"**, with "Refer all layers" optional. The other modes are "Erase touched area" and "Erase whole line".
- **Correct Line Width** (Thicken/Narrow/Fixed, "Process whole line"). Ctrl+Shift+W is one shortcut people mention (TIPS 7572).
- **Simplify Vector Line**, **Pinch Vector Line** and **Redraw Vector Line Width**.
- **Control Point** tool (move/add/delete, switch corner, width/opacity per point).
- **Object tool** to change brush tip, size or colour of selected strokes.
- **Continuous Curve / Curve tools** with texture. Upvoted 211 times: *"No more redrawing the line 1000 times or messing with stabilizers"* (https://reddit.com/r/ClipStudio/comments/1rb4kuq/).
- **Vector Magnet.** People turn it **off** when overlapping lines go squiggly (https://reddit.com/r/ClipStudio/comments/138wm7t/).
- **"Sharp angles"** pen option. It only exists in tool settings and cannot be changed on existing lines, which frustrates people (https://reddit.com/r/ClipStudio/comments/1rlpube/).
- Fill: "Refer other layers", **Close gap**, Area scaling "to darkest pixel", **"Fill up to vector paths"** (the centre line).
- Stabilization 20–30 as the common default; 90–100 for tremor or ultra-clean ink.

**Fresco:**
- **Vector Trimming.** A Fresco developer commented: *"You can also draw over multiple lines when trimming to trim many line segments at once."*
- Vector brushes: Basic Round/Taper/Flat, with Smoothing (more smoothing means more lag), and pressure/velocity toggles.
- Paint Inside / reference layers.
- Two-colour vector outline brushes for lettering.
- Experimental **Simplify** to reduce document complexity.

**Concepts:** Nudge (push or pull a line like string), Slice (delete vector points), "Hard eraser" vs "Soft mask", snap/auto-complete, the wire tool at 0.5 mm with 5% smoothing (https://reddit.com/r/ConceptsApp/comments/1stphcg/).

**SAI (the precursor):** "Linework layer" with pen, curve, edit and **weight** (pressure-edit) tools. The SAI curve tool gets requested in CSP (https://reddit.com/r/ClipStudio/comments/1421ggp/).

**Procreate (the benchmark for feel):** StreamLine plus **QuickShape** hold-to-snap. Procreate's official short on it has 983K views, and artsytsaa "Perfect line art" has 926K. The shaky-hands thread wants CSP to copy it: *"If you're thinking of lines turning into perfect curves… by holding long, that's a feature that is not yet in CSP"* (https://reddit.com/r/ClipStudio/comments/1rghikm/).

---

## 4. Pain points, with quotes

1. **Surprises when erasing.**
   - A normal eraser or transparent colour on a vector layer *draws* invisible lines. *"PSA: Do NOT use the normal eraser with your vector drawings… the lower line is made invisible by overlaying 'Transparent Vector Line'"* (https://reddit.com/r/ClipStudio/comments/sfglm6/). Others: *"when I erase lines… [they] become invisible vector lines as well"* (b9mqwk); *"massive random vector lines keep showing up"* (1mldnec); erased lines reappearing after a move or merge; merging vector layers loses pieces (1lgh8s9). One user quit: *"one of the reasons i ditched vector layers for lineart."*
   - The vector eraser also catches beginners off guard: *"Why's my eraser doing this? It doesn't do it on raster layers"* (1v99ozy). It is greyed out on raster layers (1jve4ay).
   - Concepts: *"it's not a traditional eraser, it's more like a mask"* (gx8yj2).
   - Fresco: *"I can't erase on a vector layer"* (qvx75z).
2. **Fill confusion.**
   - CSP: the Fill tool, Gradient and Blend are disabled on vector layers (official docs). The same questions recur: *"Is it possible to turn a vector layer into raster… want to fill it with color"* (jrk14w); *"worried… lack of fill bucket"* (1v0ztp8).
   - Fresco vector fill leaves **hairline gaps or white lines**: *"The paint bucket and the wand tool needs an Expand by 1-5 pixels and call it 'Overfill'"* (https://reddit.com/r/AdobeFresco/comments/1ozi6kp/). Also *"leaves an outline between the vector brush and the fill"* (j439gy); *"Cannot fill vector layer with a pixel reference layer"* popup (1rvcn8x); vector fill exported as a pixel layer (1o0w3ll); white gaps appearing after reopening a document (1wh07ky); an unanswered Adobe Community thread (https://community.adobe.com/t5/fresco-discussions/the-paint-bucket-tool-in-vector-format-is-not-filling-to-the-edge-of-the-reference-layer/td-p/15613542).
3. **"Why are my vector lines pixelated?" and "Is it fake vector?"** CSP rasterizes vector at the canvas resolution. *"I've been told that Vector Lines are supposed to be smooth… Why are my lines so pixelated?"* (25 comments, 1mifxlo); *"is it true that CSP uses simulated vector layers?"* (1pbxlya); Flash/Animate users expect "vector brushes" (137lof8). This leads into long DPI arguments (*"I also hate that CSP has DPI listed as 'resolution'"*).
4. **Too clean, sterile, loses the brush.**
   - *"it feels a bit sterile"* (91 upvotes, 1rb4kuq).
   - *"There is something stiff about vector layer… vector makes it a bit too clean"* (ju7pmy).
   - *"They feel like they were done on a computer, too perfect"* (Affinity/Fresco vectors, https://reddit.com/r/ProCreate/comments/1o6lf1a/).
   - *"using vector layers makes you lose the uniqueness of a brush tip"* (l0c5fo); *"the 'fun' of the brush disappears"* (qkz6ds).
   - Krita: freehand paths give a *"sterile look"* (https://krita-artists.org/t/vector-layer-for-primary-line-art-layer/4173).
5. **Texture, blending and painting don't work.** *"if you wanted to blend two colors together, it won't be possible"* (TIPS 8842); *"Vectors are simply not well suited to simulating physical media"* (1pbxlya). Fresco users need a "texture hack" to get grain.
6. **Performance and file bloat with many strokes.**
   - *"if you hatch… many lines, sometimes the app lags"* (l0c5fo).
   - *"resource-intensive if you have a style with a lot of small lines"* (ju7pmy).
   - A comic done in vector: *"It really requires a powerful PC… lines lag sometimes and not come out the way I drew them"* (1v0ztp8).
   - Fresco: *"the vector brushes are lagging so bad Fresco is unusable"* (y0y9u4). Smoothing adds lag.
   - CSP TIPS: *"Painting with brushes creates excessive control points, making files heavy."*
7. **Fiddly control points.**
   - *"Make a vector layer and then what? The only new thing I see… is the vector eraser"* (9nnzwe).
   - *"sometimes it feels like it's just easier to redraw a line than vector it into place"* (l0c5fo).
   - *"editing a vector line for shape is quite fiddly"* (Krita).
   - Pinch is *"very tricky"* (1ms1rnb); Animate users miss drag-to-bend (14fure6); Krita users can't select a line with one click (krita-artists 130698).
8. **Export disappointments.**
   - CSP SVG: *"only the line shape is recorded, so the color, brush tip shape, and line thickness cannot be transferred"* (official docs); *"don't include… line tapering"*; no EPS (1ugzpug).
   - Fresco has no SVG export and no vector import (1t4nlqj).
   - Concepts Android export is *"total chaos"* in SVG, DXF and PDF (1q0a8x9).
   - Text won't become a solid vector for SVG (1o2f9dq).
9. **Krita's vector layers are weak for ink.** *"useless for most line art… SVG doesn't support 'pen pressure'"* (https://www.virtualcuriosities.com/articles/1464/). The "Irinuki" pressure-on-vector and "G-pen for Krita's vector layer" requests remain unimplemented (krita-artists 120451, 121714). Wolthera's design study concluded CSP and SAI treat strokes as *"first-class"* with per-node width and brush-engine rendering (https://wolthera.info/2021/10/study-of-editable-strokes-for-inking/).
10. **Mobile auto-magic can misfire.** ibisPaint creates a vector layer automatically for each stroke: *"every time I make a stroke one is created and it bothers me"* (27 likes). The top comment (206) is someone realizing they had picked the vector brush by accident. Another reviewer: *"the vector update is so disappointing… wish… filling in the freehand parts"* (https://youtu.be/GtuKr_RtjFU).

**What beginners misunderstand:**
- "Vector means I can zoom forever" (CSP renders at canvas resolution).
- "Why not use vector for everything, including colouring?" (l0c5fo, ju7pmy).
- "Why can't I fill or blend on it?"
- "Erasing works the same as on raster."
- "DPI equals quality."
- "CSP vectors will open in Illustrator."
- "Vector brush" means the Flash style of filled shapes.

---

## 5. The new-artist journey: what makes or breaks adoption

- **Break: invisibility.** Vector sits in a layer-type menu with a jargon name. Users find it years later through a video: *"I've been using Clip Studio Paint like an idiot"* (1.2K likes); *"Vector Layers are such a powerful feature! But among my fellow Western artists, no one uses them"* (248); *"I stubbornly used raster layers for my entire artist career"* (140).
- **Break: the first erase or fill.** The first time the eraser or bucket misbehaves, many beginners write vector off.
- **Make: seeing an overlap vanish.** Commenters on the CSP official video (like counts in brackets):
  - *"just the part of erasing passed the intersection… where has this been my whole life???"* (396)
  - *"flips table now I don't have to worry erasing over and over"* (471)
  - Fresco: *"Omg this tip is SOOOO SATISFYING"*; *"so fun to zap those lines."*
- **Make: re-skinning finished ink.** *"GLOBALLY CHANGE BRUSH ON VECTOR LAYER OMG… I've been redrawing all my line art over and over"*; *"I've been making each line on a new layer and then merging so I can adjust them 😭."*
- **Make: vector as accessibility.** Shaky hands, carpal tunnel and "not confident with vectors" (1rghikm, Shyfoxx comment with 65 likes). Editable ink is a correction tool, not only a format.
- **Make: familiarity of feel ("draw like Procreate, get vector").** *"I can't tell you how many Google searches I've done looking for a program that allows me to draw like I can on Procreate, but in vector form"* (Piascik comment). Another: *"Procreate is great… but the lack of vectors is a modern graphic designer's nightmare"* (122).
- **What beginners reach for first:**
  1. A smooth pen with stabilization.
  2. An eraser.
  3. A fill bucket.
  4. Resize or transform (to fix proportions).
  5. Undo.

  Vector wins when steps 2–4 are *better* than on raster and never worse.

---

## 6. Where vector matters: hobbyists vs pros

- **Hobbyists:**
  - Cricut, vinyl and sticker cutting (single-colour SVG; Heather Cash's Affinity/Vector Q iPad tutorials, 13K views).
  - Handwriting-to-sticker projects.
  - Coloring books for kids (https://reddit.com/r/ClipStudio/comments/1ms1rnb/).
  - Fan art with clean anime lines.
  - Laser engraving (Piascik mentions engraved wood graphics).
  - Everyone who wants to "resize without losing quality".
- **Pro illustrators and designers:**
  - Logos, t-shirt and screen-print art, and client deliverables that require SVG/EPS (1ugzpug, 1v0ztp8; LOINDAFLOW, Piascik).
  - Posters at unknown sizes.
  - Nickelodeon uses Fresco vectors (Domestika).
- **Comic and manga pros:** here vector is about **editability and speed**, not scale: clean overlaps, adjust weight, reuse panels and characters, vector frames and balloons, and output to print tones.
- **Animators:** consistent line between frames, and redrawing less.
- **Industrial and architectural designers:** precision, scale, iteration (Nudge) and DXF/PDF out.

---

## 7. Top-20 feature wishlist

Ranked by how many distinct sources in this sample (threads, videos, articles; about 140 reviewed) raise the need.

1. **Erase or trim to intersection** (CSP vector erase, Fresco trim, Concepts request): about 30
2. **Pressure-sensitive strokes with the same brushes as raster, editable afterwards**: about 20
3. **Fill against ink with no gaps** (reference layer, close gap, fill to centre line, overfill): about 18
4. **Adjust line width after drawing** (thicken/thin/fixed, per-node taper): about 16
5. **Stabilization plus hold-to-straighten or shape snap**: about 14
6. **Transform or resize without quality loss**: about 13
7. **Swap brush, size or colour of existing strokes (single or all)**: about 12
8. **Reshape by grabbing the line** (pinch, nudge, Animate-style bend): about 11
9. **Real vector export** (SVG/PDF/EPS keeping width, colour and fills; cutting-ready): about 11
10. **Keep a hand-drawn, textured look on vector lines**: about 10
11. **Eraser that behaves intuitively on vector layers** (no invisible strokes): about 9
12. **Curve/Bezier drawing tools (SAI-style curve, continuous curve)**: about 8
13. **Simplify or smooth strokes with fewer points** (also for performance): about 8
14. **Good performance with thousands of hatching strokes**: about 8
15. **Comic tooling as vector** (panel borders, balloons, speed/focus lines): about 7
16. **Connect or join lines, snapping to endpoints (magnet), with an easy off switch**: about 6
17. **Line colour tools** (colour-hold, recolour lines, clip colour to ink): about 5
18. **Raster↔vector conversion or tracing**: about 5
19. **Merge, rasterize and flatten reliably**: about 4
20. **Vector support in symmetry, rulers and animation (onion skin)**: about 4

---

## 8. Recommendations for Capy Canvas

**Naming and placement**
- Call it **"Ink layer"** with the subtitle **"Editable lines · resize, reshape, trim overlaps"**, and show "Vector" as a secondary label. SAI's "Linework layer" works well as a name; "Vector" alone attracts designers who expect Illustrator and confuses beginners.
- In the Add-Layer menu, put **Paint** and **Ink** side by side at the top level with icons.
- In the brush picker, mark inking pens with a small "editable" badge.
- **Do not auto-create a layer per stroke** (ibisPaint backlash). One safe exception: when a user picks an inking pen while on a sketch layer, offer a one-tap "Ink on a new Ink layer?" prompt, with "always" and "never" options.

**Defaults that avoid the classic traps**
- **The eraser on an Ink layer always edits geometry.** Never create transparent strokes. Offer three visible modes on the eraser chip: *Cut* (touched part), *Trim to intersection* (**default**) and *Whole line*.
- **Pressure is on by default** with a G-pen-like pen. Include a one-tap "Monoline" preset (pressure off, round caps) for stickers, logos and Cricut.
- **Stabilizer default is moderate** (around the CSP 20–30 range) with speed-adaptive behaviour off as an option. Add **hold-to-straighten/snap** (the QuickShape expectation).
- **Magnet or auto-join is off by default** (it caused "squiggly overlaps").
- **Render Ink layers at view resolution** so that zooming in stays crisp. This one change removes the "fake vector / pixelated" confusion and delivers an instant wow.
- **Fill on an Ink layer is never an error.** Offer "Fill on a new colour layer below (uses this ink as boundary)". Defaults: close-gap on, **fill under lines to the centre line**, and a small overfill so there are no hairline halos.
- Keep texture: render Ink strokes with the **same brush engine** as raster, and store per-node width, opacity and angle (the CSP/SAI model, not SVG-only paths).

**Onboarding moments**
1. **First stroke on an Ink layer:** show a toast, "These lines stay editable", with three 3-second loops: trim an overlap, thicken a line, swap the pen.
2. **First time the eraser crosses an intersection:** a one-time coach mark, "Trim to intersection is on. Hold to erase just the touched part."
3. **First zoom above 400%:** a subtle hint, "Ink stays sharp at any zoom."
4. **First fill attempt:** the auto-created flats layer described above, with the explanation.
5. **Selecting lines:** make tap-to-select a stroke obvious. Show "Pen / Size / Color" on the selection bar so the "global brush swap" aha is one tap, not buried in tool properties.
6. **Starter templates:**
   - "Comic page": Sketch (blue, 30%), Ink, Flats (referencing Ink), Shade (Multiply, clipped), Frames (vector), Text.
   - "Sticker / cut file": monoline Ink with SVG export preset.
   - "Webtoon": long canvas plus panels.

**Tools to ship first, in curriculum order:**
1. Trim/erase modes
2. Width correct (brush to thicken or thin; whole-line fixed)
3. Grab-to-reshape (Nudge-style pull, better than raw control points)
4. Simplify
5. Change pen or colour of selection
6. Reference fill
7. Curve tool
8. Join lines

**Performance:** batch-render and cache strokes, and simplify automatically on commit. Heavy hatching must not lag, because lag feeds the "vector is slow" folklore.

**Export**
- **SVG/PDF export that preserves width**, by expanding strokes to outlines.
- A **"Cut-ready" preset**: single colour, union of overlaps, remove tiny specks, no raster.
- Warn honestly when textured strokes will rasterize.
- Allow SVG import. People repeatedly ask for this in Fresco and Concepts.

**Positioning:** "Pressure-sensitive, editable ink on Linux" fills a gap. CSP has no Linux build, Krita's vector layers are SVG shapes without pressure, and Krita-Artists threads show demand from CSP refugees. Treat Ink layers as an accessibility feature too: correct instead of redraw.
