# Vector path editors: how real people use and learn them (research for Capy Canvas vector layers)

[Vector layers research](../vector-layers-research.md) · source report, 2026-09-25

Date: 2026-09-25. Sources: Adobe Community, Inkscape Forum (mostly via search snippets because the forum returns HTTP 403 to fetchers), Affinity/Procreate/Figma forums, Hacker News, TypeDrawers, Krita Artists, Engraver's Cafe, course syllabi (Domestika, Peachpit, Affinity Revolution, Logos By Nick, Envato Tuts+, Dansky), vendor docs, and Reddit. Reddit blocks the WebSearch tool, so Reddit posts come from the Pullpush Reddit archive API (post titles and selftext only; comment bodies were not available). Popularity estimates are my own judgment from this evidence, not survey data.

## TL;DR

- **Most hobbyists who touch vectors aren't illustrators. They make things.** Cricut/Silhouette, laser, sticker, embroidery and print-on-demand users want *closed, welded, low-node shapes plus an offset border, exported as SVG, DXF or PDF*. Cricut alone reported ~5.9M active users and 3.09M paid subscribers at the end of 2025 ([Cricut Q4 2025](https://investor.cricut.com/news-releases/news-release-details/cricut-inc-reports-fourth-quarter-and-full-year-2025-financial)).
- **Every serious curriculum teaches the pen tool late.** Shapes, selection, transforms and booleans come first. Adobe's own book has "Make a Logo with Shapes" at lesson 3, the Curvature/Pencil tools at lesson 6 and the Pen tool at lesson 7.
- **The top struggle is Bézier handles.** Next come booleans that fail silently (groups, text, strokes), stroke vs fill vs "expand", open vs closed paths, holes/compound paths, and auto-trace that "node bombs".
- **Shape Builder is the single most loved feature.** After it: booleans, snapping, trace, offset, the Width tool, the Curvature tool, and live/parametric corners.
- **Painters want CSP-style vector layers,** meaning pressure strokes whose centerline and width stay editable, with a vector eraser that erases up to intersections. They also want a clean trace or "sketch to vector" path to SVG. Procreate still has no vectors, and "add vector layers" is a recurring request.

---

## 1. Use cases: who does what

| Use case | Who | Hobby vs pro weight | What they actually need |
|---|---|---|---|
| **Cutting-machine files** (Cricut, Silhouette: vinyl decals, HTV shirts, cake toppers, cards) | Crafters, Etsy sellers, teachers, parents | **Very high hobby**; the largest vector-adjacent population | Closed paths, **weld/union** so overlaps don't cut through each other, no duplicate/stacked paths, low node count, **offset** borders, text converted to outlines, SVG. Cricut: Offset "works best with images that have closed paths"; "Weld Offsets… merges offsets for all selected objects, so they function as a single layer" ([Cricut](https://cricut.com/blog/offset-design-space/)). |
| **Stickers** (print-then-cut, kiss-cut, die-cut) | Illustrators, Etsy/Redbubble sellers | High hobby, medium pro | Offset path / contour cut line around raster or vector art, welded into one outline, holes filled. Often with Silhouette's `Offset`, Inkscape's `Linked Offset`, or Illustrator's `Offset Path + Unite` ([Silhouette School](https://www.silhouetteschoolblog.com/2022/05/how-to-make-white-border-for-stickers.html), [Inkscape Forum cut line for stickers](https://inkscape.org/forums/cutplot/making-contourcut-line-for-stickers/), [JoAnna Seiter](https://www.joannaseiter.com/tips-tutorials/2021/2/23/creating-an-offset-for-stickers-in-adobe-illustrator-method-2)) |
| **Laser, CNC, plotter, 3D-print extrusion** | Makers (Glowforge, xTool, LightBurn), pen-plotter artists | High hobby | Stroke-only cut paths, hairline widths, **no overlapping or double lines**, closed contours, DXF plus plain SVG, **centerline trace** for engraving line art ([Craftgineer](https://craftgineer.com/blog/inkscape-vs-illustrator-laser-cnc), [SVGVector guide](https://www.svgvector.com/blog/svg-for-laser-cutting-guide.html)). Reddit: "the laser cutter will just follow the vectors" ([r/AdobeIllustrator](https://reddit.com/r/AdobeIllustrator/comments/ys0xyj/)); a laser-engraved guitar body project in [r/Inkscape](https://reddit.com/r/Inkscape/comments/1t4vkjd/). |
| **Machine embroidery** | Hobby digitizers | Niche but passionate | Clean closed fills and satin columns. [Ink/Stitch](https://inkstitch.org/) is an Inkscape extension; "trace bitmap… to make a embroidery design and it's not working" ([r/Inkscape](https://reddit.com/r/Inkscape/comments/ot279w/)). |
| **Logos / brand marks** | Freelancers, students, small businesses; a whole Fiverr economy of $5–10 "vectorize my logo" gigs ([Fiverr](https://www.fiverr.com/gigs/logo-vectorize)) | **Pro-dominant**, also a common hobby first project | Geometric construction, booleans, Shape Builder, type to outlines, precise curves. "How do I make the 'fold'… without just using a pen tool and trying to eye the curvature?" ([r/AdobeIllustrator](https://reddit.com/r/AdobeIllustrator/comments/1trj8fi/)) |
| **Icons / UI / mockups** | UI designers (now mostly Figma) | Pro | Pixel grid, snapping, booleans, corner radius, vector networks |
| **Flat / "vector" illustration, characters, posters** | Illustrators, students | Mixed | Pen/curvature, Shape Builder, Live Paint/fill regions, gradients, Width tool |
| **Hand lettering / typography vectorization** | Letterers (often Procreate-first) | Mixed | Trace or redraw with good point placement (extrema, H/V handles) ([Scannerlicker](https://scannerlicker.net/writings/2014/10/01/bezier-ocd-or-why-you-should-know-about-point-placement/), [Envato Tuts+](https://design.tutsplus.com/tutorials/hand-lettering-how-to-vector-your-letterforms--cms-23248)) |
| **Print-on-demand / surface pattern** | Procreate/Fresco artists | High hobby / semi-pro | Scalable art: "Large format artwork is king when it comes to quality and printing onto items like shower curtains and wall art" ([Lisa Glanz](https://www.lisaglanz.com/blog/how-to-use-procreate-with-illustrator-convert-your-digital-drawings-to-vector/)); repeat patterns, recolor |
| **Infographics, diagrams, technical drawings** | Scientists, engineers, educators | Medium, mostly Inkscape | Connectors, align/distribute, text, precise dimensions |
| **Comics / manga line art** | CSP users | High hobby | Vector layers for inking (control points, line-width correction, intersection eraser). This is not SVG output. |

**Rough ranking for a free, Linux-first painting app's audience:** (1) painters wanting clean, scalable line art or shapes; (2) sticker and cut-file makers; (3) logos/lettering; (4) flat illustration; (5) laser/plotter; far behind: UI/infographics. Inkscape reviewers list "vector graphics" at 72% ([G2](https://g2.com/products/inkscape/reviews)). Inkscape's forum has a dedicated "Using Inkscape with Cutters/Plotters" board, so fabrication is a first-class use.

---

## 2. The standard learning curriculum

### Representative tutorials and courses (in rough popularity/prominence)

1. *Adobe Illustrator Classroom in a Book 2025*, Brian Wood (Adobe Press). Lessons: Work Area → Selecting → **Make a Logo with Shapes** → **Editing & Combining Shapes and Paths** → Transforming → **Basic Drawing Tools (Curvature, Pencil)** → **Pen Tool** → Color/Live Paint → Type → Layers → Gradients/Blends/Patterns → Brushes → Effects → Time-savers → Images/masks → Sharing ([Peachpit](https://www.peachpit.com/store/adobe-illustrator-classroom-in-a-book-2025-release-9780135376720))
2. Domestika, *Adobe Illustrator for Beginners*, Tina Touli. Workspace → Doc → **Shape & Selection** → Transform → **Pen** → **Fill & Stroke** → **Cutting & Merging** → Aligning → Color/Gradients/Patterns/Layers → Brush/Puppet/"Imperfect shapes"/**Convert artwork into vector** → Type (convert text to shapes) → 3D → Export ([Domestika](https://www.domestika.org/en/courses/1334-adobe-illustrator-for-beginners/course))
3. Dansky, *Illustrator First Class* (core tools → color/strokes → **drawing & shape-building** → type → images/masks/**tracing** → brushes → advanced → export), plus the free YouTube *30 Days to Learn Adobe Illustrator* ([course](https://www.dansky.com/courses/illustrator-first-class), [playlist](https://www.youtube.com/playlist?list=PLRz3V_3lp2-LgUGlveQ4yen3aG6P4pOqb))
4. Logos By Nick, *Inkscape Master Class* (80+ videos): Select/Snapping → Export → Groups, **Fill & Stroke**, **Working With Strokes**, Clipping, Align → **What Is A Path?** → Path Operations → **Offsetting** → **Tracing Bitmaps** → Path Effects → Text → Node editing / Shape Builder / Bezier Pen ([logosbynick.com/inkscape](https://logosbynick.com/inkscape/)), plus the free [Inkscape Beginner Tutorials playlist](https://www.youtube.com/playlist?list=PLynG8gQD-n8BMplEVZVsoYlaRgqzG1qc4)
5. Logos By Nick, *How To Use The Pen Tool in Illustrator: The Complete Guide*: straight lines → Shift-constrain → add/remove points → curves → curves + corners → reposition anchors ([link](https://logosbynick.com/how-to-use-the-pen-tool-in-illustrator/))
6. Logos By Nick, *7 Reasons Why Union Is Not Working In Inkscape* ([link](https://logosbynick.com/union-is-not-working-in-inkscape/))
7. Affinity Revolution, *Affinity Designer for Beginners*: Layers → Move → **Shapes** → Color → Mountain project (shapes only) → **Curves chapter: Pen → Node → Shapes→Curves → Corner tool → Combining shapes** → Fill/Stroke/Appearance → Snapping → Power Duplicate → Text → Pixel persona texture ([link](https://courses.affinityrevolution.com/p/affinity-designer-for-beginners))
8. Envato Tuts+, *Complete Affinity Designer Beginners Guide* (16 parts: Interface → Artboard → Move → **Pen & Node** → **Corner** → Pencil → Clip → Vector Brush → Parametric shapes → Swatches → … → Compound shapes) ([link](https://design.tutsplus.com/series/complete-affinity-designer-beginners-guide--cms-1345))
9. **The Bézier Game**, Mark MacKay/Method of Action: trace shapes "using as few nodes as possible", scored against an "ideal solution". Touch-only users are redirected to **The Boolean Game** ([bezier.method.ac](https://bezier.method.ac/), [boolean.method.ac](https://boolean.method.ac/), [Adobe EdEx](https://edex.adobe.com/teaching-resources/bezier-game-a-game-to-help-you-master-the-pen-tool))
10. Envato Tuts+, *Find the Pen Tool Difficult? Try This Workaround* (click straight points, then round them with Live Corners) ([link](https://design.tutsplus.com/tutorials/find-the-pen-tool-difficult-try-this-adobe-illustrator-cc-workaround--cms-24365))
11. Skillshare, *Vector Basics: Mastering the Illustrator Pen Tool with Fun Results!*, Tim Eggert ([link](https://www.skillshare.com/en/classes/vector-basics-mastering-the-illustrator-pen-tool-with-fun-results/129287307))
12. Skillshare, *Pen Tool Perfection in Adobe Illustrator*, Xhico ([link](https://www.skillshare.com/en/classes/pen-tool-perfection-in-adobe-illustrator-expert-tips-for-beginners/1657042425))
13. Skillshare, *Part 1: Adobe Illustrator Pen Tool Vector Art For Hand Letterers*, Melanie Greenwood ([link](https://www.skillshare.com/en/classes/Part-1-Adobe-Illustrator-Pen-Tool-Vector-Art-For-Hand-Letterers/1019695852))
14. Skillshare, *Vectorize your Drawings! From Procreate to Vector in Adobe Illustrator*, Jesse LeDoux ([link](https://www.skillshare.com/en/classes/vectorize-your-drawings-from-procreate-to-vector-in-adobe-illustrator/154437419))
15. Skillshare, *Surface Pattern Workflow: Adobe Fresco to Adobe Illustrator*, Amy Bradley ([link](https://www.skillshare.com/en/classes/surface-pattern-workflow-adobe-fresco-to-adobe-illustrator/1514054140))
16. CreativeLive, *Live Paint & Image Trace*, Jason Hoppe ([link](https://www.creativelive.com/class/adobe-illustrator-cc-the-complete-guide-jason-hoppe/lessons/live-paint-image-trace))
17. YouTube: *#10MinSkills – How to vectorize hand lettering with Illustrator's Pen tool* ([link](https://www.youtube.com/watch?v=ltwEgnTfdMY)); *How to Use the Adobe Illustrator Pen Tool for Beginners* ([link](https://www.youtube.com/watch?v=JAVfB30vc9E)); Satori Graphics *Adobe Illustrator Tools Tutorial* (Shape Builder) ([link](https://www.youtube.com/watch?v=XkK01vGP-C0)); *Affinity Designer Pen Tool and Node Tool Tutorial – Beginner to Advanced* ([link](https://www.youtube.com/watch?v=3iowyjv8ezA))
18. Official Inkscape tutorials: Basic → Shapes → Advanced (node editing, booleans, offsets, simplify) → Tracing ([Shapes](https://inkscape.org/doc/tutorials/shapes/tutorial-shapes.html), [Advanced](https://www.inkscape.org/doc/tutorials/advanced/tutorial-advanced.html)); *Inkscape Beginners' Guide*, Pen tool modes Bézier/Spiro/BSpline ([readthedocs](https://inkscape-manuals.readthedocs.io/en/latest/pen-tool.html))
19. Scannerlicker, *Bézier OCD, or Why You Should Know About Point Placement*: nodes at extrema, "Keep smooth nodes' handles straight vertically or horizontally" ([link](https://scannerlicker.net/writings/2014/10/01/bezier-ocd-or-why-you-should-know-about-point-placement/))
20. Crafter-oriented: Dinosaur Mama *Inkscape Nodes, A Beginners Guide for Cricut Users* ([link](https://dinosaurmama.com/post/inkscape-nodes/)); Silhouette School *Sticker Tutorial for Beginners* ([link](https://www.silhouetteschoolblog.com/2014/09/making-custom-silhouette-stickers-101.html)); iPad Calligraphy *Convert Procreate Art to Vector (free auto trace)* ([link](https://ipadcalligraphy.com/procreate/convert-procreate-vector/))
21. Linearity Academy / blog, *How to Vectorize a Hand Drawing on iPad* (Auto Trace modes: Sketch / Illustration / Photo / Basic Shapes) ([link](https://www.linearity.io/blog/how-to-vectorize-a-hand-drawing/))

### The canonical teaching order (the consensus across these)

1. Workspace, navigation, document/artboard
2. **Selection and move/transform** (selection vs direct selection is taught very early)
3. **Primitive shapes** and a *shapes-only mini project* (logo, mountain, badge)
4. **Fill vs stroke**, color, swatches
5. **Combining shapes**: Pathfinder/booleans → **Shape Builder**
6. Align/distribute, snapping, smart guides
7. **Curvature/Pencil** (gentle drawing), then the **Pen tool** (straight → curves → corners → handles)
8. **Node editing**: convert smooth/corner, add/delete, corner tool / live corners
9. Strokes in depth: caps/joins, **Width tool**, **outline stroke / stroke to path**
10. Layers, groups, **clipping masks**, compound paths
11. Type, then **type to outlines**
12. **Image Trace / Trace Bitmap** ("convert artwork into vector")
13. Brushes, patterns, gradients, effects
14. Export (SVG/PDF/PNG; cut or print prep)

The notable pattern: **shapes plus booleans produce a satisfying result before anyone touches a Bézier handle.** Affinity Revolution's first practice project is a "Mountain" built only from shapes. Adobe puts Curvature *before* the Pen tool.

---

## 3. What beginners struggle with most (ranked by frequency across sources)

**1. The pen tool and Bézier handles.**
- "Drawing smooth curves with the pen tool is literally doing my head in, HEEEEELP!" ([Adobe Community](https://community.adobe.com/t5/illustrator-discussions/drawing-smooth-curves-with-the-pen-tool-is-literally-doing-my-head-in-heeeeelp/m-p/4835257)). The expert reply there explains users expect "clickClickBend" (place points, then bend segments), which Illustrator supports poorly.
- "It's specifically using the handle bar function which provides a road block for people" ([Envato Tuts+](https://design.tutsplus.com/tutorials/find-the-pen-tool-difficult-try-this-adobe-illustrator-cc-workaround--cms-24365)).
- "Creating curved lines that follow the precise path that you want them to follow" is the hardest part ([Logos By Nick](https://logosbynick.com/how-to-use-the-pen-tool-in-illustrator/)).
- Figma's founder on why: "Changing the shape of a curve involves dragging a control handle off in space instead of dragging the curve directly" ([Figma blog](https://www.figma.com/blog/introducing-vector-networks/)).
- Reddit: "basically a noob, can't even trace the image with the pen tool" ([r/AdobeIllustrator](https://reddit.com/r/AdobeIllustrator/comments/1ngsb8d/)). Even an experienced user writes: "really stretching the limits of how smooth and natural I can get this thing… without brute forcing path creation with the pen tool" ([r/AdobeIllustrator](https://reddit.com/r/AdobeIllustrator/comments/1mp7y4t/)).
- Pros care about point *quality*: extrema, H/V handles, balanced handles ([Scannerlicker](https://scannerlicker.net/writings/2014/10/01/bezier-ocd-or-why-you-should-know-about-point-placement/)). Type designers say Illustrator's drawing "peaked at version 8 (1998)" ([TypeDrawers](https://typedrawers.com/discussion/1626/should-we-tell-adobe-how-bad-the-illustrator-ui-is-for-drawing-with-beziers)).

**2. Booleans and Shape Builder "don't work" or give surprising results.**
- Logos By Nick's list of causes: "Path functions… will not work on objects that are grouped together"; converted text "is a grouping of individual letters"; union needs a fill area, and with only strokes "there's nothing there to merge with"; clipped, masked, filtered and cloned objects "are not paths" ([Logos By Nick](https://logosbynick.com/union-is-not-working-in-inkscape/)).
- The Inkscape Forum has many near-identical threads: "Union Not Working", "Union just won't", "Path difference is not working?" ([forum](https://inkscape.org/forums/questions/union-not-working/)). The status bar's "One object is not a path" is the only clue.
- Reddit: "When I… do Path->Union it removes the rounded caps off my lines and I don't know why" ([r/Inkscape](https://reddit.com/r/Inkscape/comments/1mmp2vn/)); "when I want to unite two objects… their width gets thinner" ([r/Inkscape](https://reddit.com/r/Inkscape/comments/13yolgt/)); "my strokes aren't connected meaning I can't use the shape builder" (97 upvotes) ([r/AdobeIllustrator](https://reddit.com/r/AdobeIllustrator/comments/10fcc7d/)).

**3. Stroke vs fill, and "expand / outline stroke / stroke to path".**
- A stroke is an *appearance*, not geometry. Users can't boolean it, a cutter ignores its width, and "Stroke to Path" vs "Object to Path" is confusing. Threads include "Stroke to Path doesn't work", "Stroke to path not working as intended" and "Text will not stroke to path" ([Inkscape Forum](https://inkscape.org/forums/questions/stroke-to-path-doesn-t-work/)), plus Adobe's "Expand Appearance vs Expand vs Outline Stroke" confusion ([Jason Hoppe](https://www.jasonhoppe.com/blog/adobe-illustator-expand-appearance-expand-and-outline-stroke)).
- Painters get bitten from the other side: "my vector lines turning into filled compound paths" when Fresco art reaches Illustrator ([Adobe Community](https://community.adobe.com/t5/fresco-discussions/vector-lines-turn-to-filled-compound-paths-from-fresco-to-illustrator/td-p/12534525)).

**4. Strokes not scaling with the object.** Illustrator's `Scale Strokes & Effects` is off by default, so line art gets relatively fatter when shrunk and thinner when enlarged. This is a perennial tip video topic ([IllustratorHow](https://illustratorhow.com/scale-stroke-proportionally/)). Inkscape adds its own twist: "the width of the line depends on the zoom level" for pressure freehand ([r/Inkscape](https://reddit.com/r/Inkscape/comments/1mjw16l/)).

**5. Open vs closed paths.**
- Filling an open pen path fills "haphazardly" ([Adobe Community](https://community.adobe.com/t5/illustrator-discussions/filling-wrong-section/td-p/13304059)).
- Live Paint and bucket fills fail on gaps: "draw a line to close the gap" ([MakeUseOf](https://www.makeuseof.com/how-to-vectorize-colorize-procreate-drawing-with-adobe-illustrator/)).
- Cricut offsets need closed paths.
- Traced line art: "all the black lines are opened and not true lines" ([r/AdobeIllustrator](https://reddit.com/r/AdobeIllustrator/comments/1qqtw2u/)); "Can't join nodes… Did a trace bitmap" ([r/Inkscape](https://reddit.com/r/Inkscape/comments/1q05eoy/)).
- Inkscape pressure freehand: "Disappearing closed freehand paths" ([r/Inkscape](https://reddit.com/r/Inkscape/comments/oiwxim/)).

**6. Holes, compound paths and fill rules.**
- "Used shape builder and pathfinder but still can't create empty space in B-holes" ([r/AdobeIllustrator](https://reddit.com/r/AdobeIllustrator/comments/13kale9/)).
- Under nonzero winding, a hole appears only if the inner path "goes in the opposite direction" ([Supreme Graphics](https://www.supremegraphics.com/resources/the-ideas-collection/understanding-compound-paths/)). Figma calls this "confusing 'winding number' logic" ([Figma](https://www.figma.com/blog/introducing-vector-networks/)).

**7. Auto-trace output: too many nodes, double lines, blobs.**
- "I… got 522 nodes where a clean version of the same art is about 59. it's miserable to edit… worse if the file's going to a cutter since the machine stutters along every micro-segment" ([r/AdobeIllustrator](https://reddit.com/r/AdobeIllustrator/comments/1ularqw/)).
- "Note to self: don't try to break apart a traced bitmap when there are 288,644 nodes in it. (Six hours so far.)" ([r/Inkscape](https://reddit.com/r/Inkscape/comments/1smh8vf/)).
- "image trace gives accurate results but it node bombs" ([r/AdobeIllustrator](https://reddit.com/r/AdobeIllustrator/comments/1uufuwk/)).
- Line art traces as double outlines: "A tracer took your single stroke, treated it as a thin shape, and drew a path around each edge" ([Perfect Vector](https://perfectvector.com/blog/svg-cutting-double-lines)).
- "a traced circle is a wobbly approximation of a circle" ([Perfect Vector](https://perfectvector.com/blog/inkscape-trace-bitmap)); halftones come out "very blobby" ([r/AdobeIllustrator](https://reddit.com/r/AdobeIllustrator/comments/1nxcfl4/)).
- Simplify is global: "I don't think you can simplify a section of a path without simplifying the whole path" ([InkscapeForum](https://alpha.inkscape.org/vectors/www.inkscapeforum.com/viewtopic5286.html?t=3995)).
- Crafter: "The nodes were endless because I didn't understand what they did within Cricut" ([Dinosaur Mama](https://dinosaurmama.com/post/inkscape-nodes/)).

**8. Groups, layers and "entered group" state.** Inkscape users save a file while inside a group, and then booleans fail ([Inkscape Forum](https://inkscape.org/forums/questions/trouble-with-union/)). HN users list broken conventions: "shift-to-lock aspect ratio… control-plus to zoom, and double-click to enter a group" ([HN](https://news.ycombinator.com/item?id=14534575)), and "I really want to learn to use inkscape well, but just can't grok the interface."

**9. Invisible objects and mode traps.** These include fill and stroke both none, 0% opacity, outline display mode, and a leftover Power Stroke LPE ([Inkscape Forum](https://inkscape.org/forums/questions/shapes-are-displaying-stroke-but-not-fill/)). There is also "How to I remove this shape builder? I clicked it by accident" ([r/AdobeIllustrator](https://reddit.com/r/AdobeIllustrator/comments/yi0yhq/)).

**10. Hairline gaps between vector fill and line.** "paint bucket Fill leaving a gap"; "white lines when using fill" ([r/AdobeFresco](https://reddit.com/r/AdobeFresco/comments/j439gy/), [r/AdobeFresco](https://reddit.com/r/AdobeFresco/comments/1ozi6kp/)).

**11. Cost and complexity for makers.** "$276 per year for software that preps your cut files is a hard sell"; Inkscape's "tool names don't always match what you'd expect, and some common operations require multiple steps" ([Craftgineer](https://craftgineer.com/blog/inkscape-vs-illustrator-laser-cnc)).

---

## 4. Most-loved features (ranked) and shortcuts

1. **Shape Builder** (Illustrator; Inkscape 1.3+; Affinity; Figma Draw 2025). "What's the Illustrator feature you learned way too late…? I'll go first: Shape Builder instead of suffering with Pathfinder like a caveman" ([r/AdobeIllustrator](https://reddit.com/r/AdobeIllustrator/comments/1pd54ns/)). Figma forum requesters call it the reason they still keep Illustrator for icons ([Figma forum](https://forum.figma.com/t/shape-builder-tool-feature-request/3758)). Inkscape shipped it in 1.3, and it was one of the most requested features ([Inkscape 1.3](https://inkscape.org/news/2023/07/23/inkscape-launches-version-13-focus-organizing-work/), [GitLab #1550](https://gitlab.com/inkscape/inbox/-/issues/1550)).
2. **Pathfinder / booleans** (union, subtract, intersect, divide). Taught in every course; Figma Draw touts "Improved boolean operations" ([Figma Draw](https://www.figma.com/blog/introducing-figma-draw/)).
3. **Snapping, smart guides, align/distribute.** These get dedicated early lessons ("Master Snapping", "Aligning & Distributing (16:03)").
4. **Auto-trace** (Image Trace; Trace Bitmap with brightness, centerline and multicolor modes; Linearity Auto Trace; Illustrator's newer **Sketch to Vector**). "I am a 26 year Adobe Illustrator user and my favorite tool is the Pen Tool. But… this amazing Sketch to Vector… this is all I want in life, help with my sketches to clean art!" ([r/AdobeIllustrator](https://reddit.com/r/AdobeIllustrator/comments/1ux9052/)). Vectorizer.AI (about $9.99/mo) is widely cited as best-in-class ([Perfect Vector](https://perfectvector.com/blog/best-ai-vectorizers-2026)).
5. **Offset path** (Inkscape Dynamic/Linked Offset, Ctrl+J / Ctrl+Alt+J; Illustrator Offset Path). Essential for stickers and cut borders ([Custom Stickers NZ](https://customstickers.co.nz/blog/adobe-illustrator-tips-sticker-printing-offset-paths/)).
6. **Width tool / variable-width strokes / Power Stroke.** "You can get really organic, hand-drawn line effects without having to expand strokes or mess with shapes" ([Adobe Community](https://community.adobe.com/questions-652/my-favorite-hidden-feature-in-illustrator-the-width-tool-815158)). Figma added variable width in 2025.
7. **Curvature tool** (Illustrator) and Inkscape BSpline/Spiro modes. The beginner favorite, because it previews the segment and needs no handles ([CreativePro](https://creativepro.com/illustrator-how-to-use-the-curvature-pen-tool-for-easier-drawing/)).
8. **Live / parametric corners**: Affinity's Corner tool (non-destructive, several corner types, "bake" later), Illustrator's Live Corners (users complain they're "too easy to break") ([Envato Tuts+](https://design.tutsplus.com/tutorials/how-to-use-the-corner-tool-in-affinity-designer--cms-108797), [Illustrator UserVoice](https://illustrator.uservoice.com/forums/333657-illustrator-feature-requests/suggestions/34915786-live-corners-should-be-more-live)).
9. **Inkscape LPEs** (Power Stroke, BSpline, Corners, Pattern Along Path, Envelope), clones and tiled clones, and **Simplify (Ctrl+L)** ([Logos By Nick Master Class](https://logosbynick.com/inkscape/)).
10. **Figma vector networks and the bend tool.** "drag the curve around directly. The editor will automatically figure out where to place the control handles"; "the paint bucket tool can toggle the fill for any enclosed region" ([Figma](https://www.figma.com/blog/introducing-vector-networks/)). HN: "Vector networks feel like the correct way to draw an object rather than the hack". The caveat: no per-node smooth/corner type, and Figma lacks a curvature pen ([HN](https://news.ycombinator.com/item?id=39246585)).
11. **Clip Studio vector layer tools**: the vector eraser "Erase up to intersection", Correct Line Width, Simplify, Connect lines, control-point editing ([CSP Tips](https://tips.clip-studio.com/en-us/articles/600)). "I switched from krita to csp (Painfully)… for vector layers and vector eraser" ([r/ClipStudio](https://reddit.com/r/ClipStudio/comments/14qxpv9/)).
12. **Illustrator Simplify slider** (2020, auto-simplify with live preview) ([Adobe](https://helpx.adobe.com/illustrator/desktop/draw-shapes-and-paths/modify-paths/auto-simplify-paths.html)); Live Paint for coloring line art.

**Most-used shortcuts**
- **Illustrator:**
  - Tools: V select, A direct select, P pen, Shift+~ curvature, M/L rect/ellipse, Shift+M Shape Builder, Shift+W width.
  - Fill/stroke: X toggle, Shift+X swap.
  - Objects: Ctrl+G / Ctrl+Shift+G group/ungroup, Ctrl+D transform again, Alt-drag duplicate, Ctrl+J join, Ctrl+8 compound path, Ctrl+7 clipping mask, Ctrl+Y outline view.
  - Modifiers: hold Shift to constrain, Alt with pen to convert/break handles, Space to pan.
  ([Adobe](https://helpx.adobe.com/illustrator/using/default-keyboard-shortcuts.html), [Academy Class](https://academyclass.com/blog/illustrator-keyboard-shortcuts-cheat-sheet/))
- **Inkscape:**
  - Tools: S select, N node, B pen, P pencil, R/E rect/ellipse.
  - Path ops: Ctrl++ union, Ctrl+- difference, Ctrl+* intersection, Ctrl+/ division, Ctrl+K / Ctrl+Shift+K combine / break apart, Ctrl+( / ) inset/outset, Ctrl+J dynamic offset, Ctrl+L simplify, Shift+Ctrl+C object to path, Ctrl+Alt+C stroke to path.
  - Selection: Alt+click select under.
- **Affinity:** V move, A node, P pen, N pencil, C corner, Ctrl+Enter convert to curves.

---

## 5. The painter-who-wants-vector crossover

**Who and why.** Procreate, CSP, Fresco and Krita artists need vectors for:
- print-on-demand and large prints
- Cricut stickers and decals ("print stickers on my cricut of my mother's handwriting" ([r/ProCreate](https://reddit.com/r/ProCreate/comments/1opig31/)))
- laser engraving
- logos for clients, and recoloring or repeat patterns
- clean animation line work ("my wobblyness was in the way. So I used the vector tools" ([r/krita](https://reddit.com/r/krita/comments/1opikyg/)))

Procreate has no vectors. "Hear me out, ProCreate should add vector layers" (169 upvotes, 50 comments) ([r/ProCreate](https://reddit.com/r/ProCreate/comments/1ny1b7e/)), and Procreate Folio has "Adding Vector Layer + Vector Eraser" and "Will vector ever be explored?" threads ([Folio](https://folio.procreate.com/discussions/3/6/34606)). Fresco fans: "Being able to freely sketch while maintaining vector format has been a game changer" ([r/AdobeFresco](https://reddit.com/r/AdobeFresco/comments/1l1avv8/)).

**Current workflows (all multi-app):**
1. Procreate: ink a clean line layer (no texture, 300 dpi+) → PNG → Illustrator Image Trace → Expand → Live Paint to color → fix gaps → export ([Lisa Glanz](https://www.lisaglanz.com/blog/how-to-use-procreate-with-illustrator-convert-your-digital-drawings-to-vector/), [MakeUseOf](https://www.makeuseof.com/how-to-vectorize-colorize-procreate-drawing-with-adobe-illustrator/), [Engraver's Cafe](https://engraverscafe.com/threads/converting-procreate-drawings-to-vector.24084/))
2. Procreate → free Adobe Capture "Shapes" → SVG ([iPad Calligraphy](https://ipadcalligraphy.com/procreate/convert-procreate-vector/))
3. Upload to Vectorizer.AI, or pay a Fiverr "vectorize/redraw" gig
4. Redraw by hand with the pen tool over a locked template. Experts recommend this for logos ([Adobe Community](https://community.adobe.com/t5/illustrator-discussions/created-logo-in-procreate-but-how-to-vectorize/m-p/12437398))
5. Fresco vector brushes → "Open a copy" in Illustrator. It's one-way: "Moving from Fresco to Illustrator is easy, the opposite isn't" ([Adobe Community](https://community.adobe.com/questions-646/is-a-vector-fresco-illustrator-workflow-possible-308280))

**Pain points:**
- **Texture loss:** "A logo is simple. No grain, no special brushes, no raster effects."
- **Fidelity loss:** "The drawing seems to lose a lot of integrity when I choose the live trace option"; shading becomes "a big black blob" ([Engraver's Cafe](https://engraverscafe.com/threads/converting-procreate-drawings-to-vector.24084/))
- **Lines become shapes:** double outlines, and Fresco "Vector in Fresco has always created shapes rather than lines"
- **Node explosions** and no *local* simplify
- **Gaps** that block fills, and **hairline seams** between fill and line
- **Vector erasers that behave oddly:** CSP's normal eraser creates invisible "transparent vector lines" ([r/ClipStudio](https://reddit.com/r/ClipStudio/comments/sfglm6/)). Concepts' eraser feels "choppy… I get that it's vector" ([r/ConceptsApp](https://reddit.com/r/ConceptsApp/comments/gx8yj2/))
- **Painting-app vector UIs feel second-class:** Krita artists ask for simple things like "Thickness can definitely be improved with a slider" ([Krita Artists](https://krita-artists.org/t/working-with-krita-on-vector-layers/185160))
- **"Coloring with vectors is cool, but hard"** ([r/AdobeFresco](https://reddit.com/r/AdobeFresco/comments/xstvbr/))

**What the ideal combined app does:**
1. **Pressure-brush strokes on a vector layer stored as a centerline plus a width profile**, like CSP or a Width tool. Keep them as strokes while drawing (so recolor, reshape, width edits and the intersection eraser all work), then offer **"Convert to shapes"** that outputs clean outlines with few nodes. Keep **centerline export** for plotters, lasers and single-line cutting.
2. **Stroke smoothing that yields few nodes** (fit, then simplify), plus a **node-count badge**. **Local simplify** by lasso or brush ("smooth tool").
3. **Vector eraser modes:** erase touched, **erase to intersection**, whole stroke.
4. **Fill regions like Live Paint or Figma's paint bucket,** with automatic **gap closing** (tolerance) and fills that **tuck under the line** so there are no hairline seams.
5. **"Merge same-color overlaps"** (blob-brush behavior) to produce one welded shape for cutting and stickers.
6. **Trace raster layer** with modes for silhouette, **centerline**, color posterize and sketch-to-vector, a detail/node slider with live preview, and "keep source" for comparison. Warn about anti-aliased or JPEG sources (the #1 cause of node bombs).
7. **Keep texture as a separate raster layer** clipped over clean vector art, so the artist gets "clean logo" and "textured illustration" versions from one file.
8. **Sticker/cut pipeline:** one-click **offset border (welded, holes filled, rounded corners)**, a "cut-ready" check (open paths, overlaps, duplicate or stacked paths, tiny segments, unconverted strokes/text), and SVG, PDF and DXF export with correct stroke-vs-fill semantics.

---

## 6. New-user journey recommendations for Capy Canvas

**Principles from the evidence.** Start from what painters already do (draw with a brush). Deliver a "wow, it's editable" moment in the first two minutes. Teach **shapes plus merge before Béziers**. Make the invisible state (open/closed, stroke vs fill, grouped, hole direction) visible. Never let an operation fail silently.

### First hour (suggested guided path, skippable)

1. **0–3 min: "Make it a vector layer."** Add a vector layer from the layer panel and draw with the *same* brushes. Tap a stroke to select it, then drag, recolor and change its width. Scaling is on by default so strokes scale with the object.
2. **3–10 min: Tidy.** Vector eraser in "to intersection" mode on overlapping hair or leaf lines, like CSP. A Smooth brush over a wobbly segment. A node-count chip shows "12 points".
3. **10–20 min: Shapes plus merge** (sticker or badge project). Circle, rounded rect and star, then **Shape Builder**: drag across pieces to merge, Alt/Option-drag to remove. Add a bucket fill for a region. This mirrors every curriculum's "logo with shapes".
4. **20–30 min: Bend, don't handle.** Select a stroke or shape and drag *the curve itself* (Figma bend / Inkscape segment drag). Double-tap a point to toggle smooth or corner. Handles are hidden until requested.
5. **30–40 min: Trace your sketch.** Import a photo or sketch layer, then "Trace" (Sketch / Silhouette / Centerline presets) with a detail slider. Or draw over it with the **curvature pen** (click points, with segments previewed). Show node count versus a "clean" target.
6. **40–50 min: Make it real.** Add an offset border (welded) and **Export**. Presets: "Sticker (PNG + cut line SVG)", "Cricut/Silhouette SVG", "Laser SVG/DXF", "Print PDF". A cut-ready check lists fixable issues, each with a *Fix* button.
7. **50–60 min: Optional Pen Tool challenge.** A built-in Bézier-game-style exercise that ghosts target shapes and scores against the fewest nodes. Make it **touch/pen friendly**: bezier.method.ac requires a keyboard and sends touch users elsewhere.

### Tool exposure tiers

- **Tier 1 (visible by default):** vector brush/pencil, select/move, direct-edit (nodes), shapes, Shape Builder, fill bucket, vector eraser, curvature pen, trace, offset border, export presets.
- **Tier 2 (one tap away):** classic Bézier pen, booleans menu (union, subtract, intersect, exclude, divide), corner rounding, width tool, simplify, align/distribute, snapping options, text plus "convert to shapes".
- **Tier 3 (advanced):** fill rule, compound path make/release, path direction, clipping masks, stroke caps/joins/miter, per-node handle modes, DXF options.

### Teaching Bézier curves gently

- Default the pen to **curvature mode** (points only, with a live preview of the next segment). Add a toggle to the classic pen for people coming from Illustrator.
- Let users **drag a segment directly** to bend it, and auto-place handles (the Figma bend tool). Support the "clickClickBend" model users already expect.
- Make **auto-smooth nodes** the default (Affinity "smart" nodes). Converting to corner is one gesture (double-tap).
- **Snap handles to horizontal/vertical, and snap points to extrema** by default when near. This teaches good point placement implicitly (the Scannerlicker/Bézier Game rules).
- Show a subtle **"too many points" hint** and a one-tap Simplify that previews the result.
- Give on-screen modifier chips on touch devices (constrain, break handle, add point) instead of relying on Alt/Shift.

### Defaults that prevent the common mistakes

- **Open paths get stroke only; closing a path adds fill.** Never render a haphazard fill on an open path. Clearly highlight open endpoints, and add an "auto-close within N px" option.
- **Scale strokes with objects: ON** by default (painter mental model), with a visible toggle.
- **Booleans and Shape Builder should just work:**
  - auto-ungroup and convert text
  - offer "use stroke outlines" when inputs are strokes
  - preserve rounded caps by outlining first
  - in the rare real failure, say *why* in plain words with a Fix button (never a bare "One object is not a path")
- **Holes:** any merge or subtract that yields an inner contour makes a real hole. Avoid exposing winding direction, and use even-odd for traced output.
- **Plain-language names:** "Convert line to shape" (not "Expand Appearance" or "Stroke to Path"), "Merge", "Cut out", "Border/offset".
- **Show state:** a status chip for "open path / closed / 3 subpaths / 1,240 points / stroke only / grouped". No hidden "inside group" mode that persists after save.
- **Fills tuck under line art** (fill expands slightly beneath the stroke) to avoid hairline seams.
- **Traces clean up by default:** "ignore white" and "merge adjacent same-color regions" are ON, and the detail slider defaults to low-node output.
- **Export semantics are explicit:** "cut" exports centerlines or outlines as chosen; "print" outlines strokes; text is converted automatically, with a warning.
- **One-step undo** for every compound operation (trace + expand + merge counts as one undo).

### Differentiation opportunities (from gaps in competitors)

- A free, cross-platform **"paint then vectorize in one app"** flow. Procreate lacks it; Fresco is one-way and Adobe-bound; Krita's vector layers feel secondary; Inkscape's pressure drawing is awkward (zoom-dependent width, device setup).
- A **maker-first export and "cut-ready" linting**, which the $276/yr Illustrator doesn't prioritize and Inkscape buries.
- **Touch-native node editing and practice.** Existing pen-tool training assumes a mouse and keyboard.
