# Photoshop and Affinity Photo: user journeys and power-user details

[Photo editing research](../photo-editing-research.md) · source report, 2026-09-25 · baseline `dac76c20`

Web research gathered by an agent on 2026-09-25. Items with a link come from that source; view counts come from the Stack Exchange API. Reddit and some vendor pages refused automated access. Treat rankings as product judgment, not usage statistics.

---

**Method:** I pulled evidence from community.adobe.com threads and feature requests, the Affinity forum, Adobe's help pages (helpx.adobe.com) and its task landing pages on adobe.com, plus tutorial sites and channels (PiXimperfect, PHLEARN, Photoshop Training Channel (PTC), Julieanne Kost, James Ritson) and reviews.

**Evidence limits:**
- Reddit (r/photoshop, r/AffinityPhoto, r/photography) could not be fetched, so the "popularity" evidence comes from Adobe and Affinity forums, Adobe's own SEO task pages and tutorial sites.
- No site publishes per-video view counts, so the ranking is a judgment. It weighs beginner search demand, forum volume and how central each task is to professional retouching.
- For scale: PiXimperfect has about 5M subscribers, over 1,100 tutorials and 675M views ([piximperfect.com](https://www.piximperfect.com/)). PTC has over 2M subscribers.

**Current baseline:**
- **Photoshop 2026** ships Harmonize (automatic color, light and shadow matching for composites), on-device Select Subject and Remove Background, a Remove tool that finds distractions, Generative Fill with partner models, Topaz-powered Generative Upscale, and a new Color & Vibrance adjustment layer with Temperature/Tint ([PhotoshopCAFE](https://photoshopcafe.com/whats-new-in-photoshop-2026-full-release-overview/), [Fstoppers](https://fstoppers.com/photoshop/best-updates-hidden-inside-photoshop-2026-715565)).
- **Affinity:** version 2.6 added on-device Select Subject and an Object Selection tool; the model download is opt-in ([Digital Production](https://digitalproduction.com/2025/02/26/affinity-2-6-machine-learning-tools-to-the-rescue/)). Since October 2025, "Affinity by Canva" is free and puts Generative Fill/Expand, Portrait Blur and Colorize behind a Canva subscription ([CG Channel](https://www.cgchannel.com/2025/10/check-out-canvas-new-perpetually-free-affinity-software/)).

---

## Tier 1: must-have journeys (1–12)

**1. Remove an unwanted object, person or distraction**
- **Photoshop:** Remove tool / Spot Healing (small items), Content-Aware Fill workspace (large areas), Generative Fill as a fallback.
- **Affinity:** Inpainting Brush, Edit > Inpaint on a selection, Patch tool.
- **Pleasant vs painful:**
  - Retouch on an empty layer with "Sample All Layers".
  - Remove tool: remove-after-each-stroke toggle and an on/off switch for generative mode.
  - Content-Aware Fill: paintable sampling-area overlay, rotation/scale/mirror adaptation, output to a new layer.
  - Affinity's inpainting "repeats patterns" and breaks down on busy backgrounds.
- **Evidence:**
  - [PHLEARN "How to Remove Anything"](https://phlearn.com/tutorial/how-to-remove-anything-photo-photoshop/) (20 scenarios)
  - [Adobe CAF page](https://www.adobe.com/products/photoshop/content-aware-fill.html) and [Remove tool help](https://helpx.adobe.com/photoshop/using/remove-tool.html)
  - [Fstoppers: Remove tool finds distractions](https://fstoppers.com/photoshop/photoshops-new-remove-tool-can-find-and-erase-general-distractions-automatically-902035)
  - [Adobe thread "How do I remove this person"](https://community.adobe.com/t5/photoshop-ecosystem-discussions/how-do-i-remove-this-person-using-photoshop/td-p/9994899)
  - [Affinity "Content Aware" thread](https://forum.affinity.serif.com/index.php?%2Ftopic%2F71950-content-aware%2F=) and [XDA on missing AI content-aware fill](https://www.xda-developers.com/photoshop-features-not-available-in-affinity-photo/)
  - A 237-reply Lightroom request for "Photoshop-like clone/heal/content-aware brushes" ([link](https://community.adobe.com/feature-requests-564/p-more-photoshop-like-clone-healing-content-aware-brushes-666552/index3.html))

**2. Cut out a subject and remove or replace the background**
- **Photoshop:** Select Subject or the Remove Background quick action, then Select and Mask, then output to a layer mask, then drop in a new background.
- **Affinity:** Select Subject / Object Selection (2.6) or Selection Brush, then Refine Selection, then output as a mask.
- **Pleasant vs painful:**
  - A single click that gives a mask, not deleted pixels.
  - Output choices: selection, mask, or new layer with mask.
  - Affinity's ML selection "struggles with hair and thin edges" ([Lenscraft](https://lenscraft.co.uk/photo-editing-tutorials/affinity-photo-ai-tools/)).
- **Evidence:**
  - [helpx: remove background](https://helpx.adobe.com/photoshop/desktop/repair-retouch/remove-objects-fill-space/remove-background-in-your-images.html) and [Adobe "change background color"](https://www.adobe.com/products/photoshop/change-background-color.html)
  - [Adobe thread "Which better way to remove background"](https://community.adobe.com/t5/photoshop-ecosystem-discussions/which-better-way-to-remove-background-from-photo/td-p/10441151)
  - [DPS: Affinity background removal](https://digital-photography-school.com/remove-background-affinity-photo/)

**3. Mask hair or fur against a busy background**
- **Photoshop:** Select and Mask with Refine Hair, Refine Edge Brush, Smart Radius and Object-Aware mode, plus Decontaminate Colors. Channel masking or Calculations as the fallback, or painting hair back with a custom hair brush.
- **Affinity:** Refine Selection's Matte brush, or channels.
- **Pleasant vs painful:**
  - View modes: onion skin, overlay, on black, on white.
  - Being able to re-refine an existing mask.
  - Decontaminate Colors is called "horrible and destructive"; the expert fix is a clipped Color-mode paint layer.
  - Users disliked Select and Mask's modal workspace enough to want the old Refine Edge dialog back.
- **Evidence:**
  - PTC [Advanced Hair Masking](https://photoshoptrainingchannel.com/advanced-hair-masking/) and [Channel Masking](https://photoshoptrainingchannel.com/channel-masking-photoshop/)
  - Adobe threads: ["tricky removal of background behind hair"](https://community.adobe.com/questions-712/please-help-tricky-removal-of-background-behind-hair-1074141), [decontaminate colors issues](https://community.adobe.com/t5/photoshop-ecosystem-discussions/select-and-mask-decontaminate-colors-issues/m-p/9179430), ["Hate the new Select and Mask tool!"](https://community.adobe.com/questions-712/hate-the-new-select-and-mask-tool-1117387), ["Refine Edge has Returned"](https://community.adobe.com/questions-712/refine-edge-has-returned-1125035)

**4. Clean up blemishes, dust and sensor spots**
- **Photoshop:** Spot Healing (Content-Aware / Proximity Match / Create Texture), Healing Brush, Patch tool, all on an empty layer.
- **Affinity:** Blemish Removal, Healing Brush, Patch.
- **Pleasant vs painful:** "Sample: Current & Below" plus the ignore-adjustment-layers toggle; Diffusion setting; Replace mode.
- **Evidence:** [PTC "Remove Blemishes: 2 New Methods"](https://photoshoptrainingchannel.com/remove-blemishes-photoshop/) (featured). [Fstoppers' six must-master features](https://fstoppers.com/post-production/six-photoshop-features-all-photographers-must-master-586083) include Spot Healing and Patch.

**5. Clone-stamp structured detail (edges, patterns, perspective)**
- **Photoshop:** Clone Stamp on an empty layer, Alt-click to set the source, the Aligned option, and the Clone Source panel.
- **Affinity:** Clone Brush with Aligned, sources (Current Layer / Current & Below / Layers Beneath / Global), rotation, scale and flip.
- **Pleasant vs painful:**
  - Source overlay that can be clipped to the brush and auto-hidden while painting.
  - Rotate, scale and nudge the source by keyboard (Alt+Shift+< > [ ] and arrow keys); up to 5 saved sources; cloning from another document.
  - The top confusion is "clone does nothing on a new layer" (sample mode set to Current).
  - Users want a "sample selected layers only" mode for frequency-separation layers; Affinity's "Layers Beneath" covers this.
- **Evidence:**
  - Adobe threads ["Clone Stamp not working in a new layer"](https://community.adobe.com/t5/photoshop-ecosystem-discussions/clone-stamp-tool-not-working-in-a-new-layer-photoshop-2022-and-cropping-issue/td-p/12777858) and ["won't work on layers"](https://community.adobe.com/questions-712/clone-stamp-tool-won-t-work-on-layers-1101272)
  - [Julieanne Kost's 10 clone/heal tips](https://jkost.com/blog/2021/12/10-tips-for-the-clone-stamp-and-healing-brush-tools-in-photoshop.html) and [PTC clone shortcuts](https://photoshoptrainingchannel.com/clone-stamp-tool-shortcuts/)
  - [Selected-layers mode request](https://community.adobe.com/t5/photoshop-ecosystem-ideas/clone-stamp-tool-selected-layers-mode/idi-p/14434134) and [Affinity cloning help](https://affinity.help/photo2/English.lproj/pages/Retouching/retouching_cloningHealing.html)
  - Affinity request ["Current Layer & Below as default for Inpainting and Patch"](https://forum.affinity.serif.com/index.php?/topic/200962-current-layer-below-as-default-for-inpainting-and-patch-tools/)

**6. Global tone and color correction with adjustment layers**
- **Photoshop:** Curves, Levels, Hue/Saturation, Color Balance and Vibrance adjustment layers, with masks and clipping.
- **Affinity:** the same adjustments exist as adjustment layers.
- **Pleasant vs painful:** Everything re-editable. The mask lives on the adjustment layer itself. Clip-to-layer. Presets.
- **Evidence:**
  - [PHLEARN's top-10 essentials](http://phlearn.com/tutorial/aaron-must-know-tools-and-techniques/) list adjustment layers, layer masks, clipping masks and Camera Raw.
  - Missing adjustment layers was the defining GIMP complaint ([Cambridge in Colour](https://www.cambridgeincolour.com/forums/thread1259.htm)). GIMP only fixed this with non-destructive filters in 3.0 and 3.2 ([AlternativeTo](https://alternativeto.net/news/2026/3/gimp-3-2-introduces-non-destructive-and-vector-layers-more-brushes-and-other-enhancements)).

**7. Local adjustments by painting or selecting masks**
- **Photoshop:** make a selection, add an adjustment layer (the selection becomes its mask), paint the mask; Adjustment Brush; Color Range.
- **Affinity:** the same, plus live Luminosity, Hue-range and Band-pass masks.
- **Pleasant vs painful:** Mask density and feather sliders; Alt-click to view the mask; X to swap colors.
- **Evidence:** [PHLEARN top-10](http://phlearn.com/tutorial/aaron-must-know-tools-and-techniques/) and [Noble Desktop's beginner features](https://blog.nobledesktop.com/photoshop-features-beginners-should-know).

**8. Frequency-separation skin retouching**
- **Photoshop:** manual build: duplicate twice, blur the low layer, Apply Image on the high layer (different settings for 8-bit and 16-bit), set Linear Light. Heal or clone on the high layer, mix or blur on the low layer.
- **Affinity:** Filters > Frequency Separation, a one-click dialog with live preview and a choice of Gaussian, Median or Bilateral separation.
- **Pleasant vs painful:** Photoshop users rely on actions to automate the setup; Affinity's built-in, previewable filter is a delight.
- **Evidence:**
  - [PHLEARN Frequency Separation](https://phlearn.com/tutorial/amazing-power-frequency-separation-retouching-photoshop/) and [Affinity blog](https://www.affinity.studio/blog/frequency-separation-explained)
  - [Adobe "favorite retouching techniques" thread](https://community.adobe.com/questions-712/what-are-your-favorite-retouching-techniques-in-photoshop-1178865)

**9. Dodge and burn for shape and depth**
- **Photoshop:** a 50% gray layer set to Soft Light or Overlay, or paired Curves layers, painted at low flow.
- **Affinity:** Dodge/Burn brushes with tonal range, or the gray-layer method.
- **Pleasant vs painful:** Low-flow brush with pen pressure. The gray layer causes desaturation or discoloration on dark skin. Softening an overdone pass afterwards.
- **Evidence:** Adobe threads ["Dodge & Burn discoloration"](https://community.adobe.com/questions-712/dodge-burn-discoloration-1081088) and ["too hard, can I soften them?"](https://community.adobe.com/questions-712/dodge-and-burn-too-hard-can-i-soften-them-1064187); [Fstoppers six features](https://fstoppers.com/post-production/six-photoshop-features-all-photographers-must-master-586083).

**10. Remove a color cast or fix white balance**
- **Photoshop:** Curves or Levels gray-point eyedropper, Camera Raw filter white balance, Match Color > Neutralize, and the Color & Vibrance adjustment layer's Temperature/Tint (2026).
- **Affinity:** White Balance adjustment with a picker.
- **Pleasant vs painful:** Click-a-neutral eyedropper; a sample-size option for the eyedropper.
- **Evidence:** [helpx: remove color cast](https://helpx.adobe.com/ph_fil/photoshop/how-to/remove-unwanted-color-cast.html), [Adobe "Set Gray Point in Curves"](https://community.adobe.com/t5/photoshop/set-gray-point-in-curves/td-p/12119505), [DPReview thread](https://www.dpreview.com/forums/thread/4716965).

**11. Crop and straighten a tilted horizon**
- **Photoshop:** Crop tool Straighten (draw a line), Content-Aware fill of the empty corners, Delete Cropped Pixels toggle.
- **Affinity:** Crop tool Straighten.
- **Pleasant vs painful:** Content-Aware crop greys out in Classic mode. Curved horizons need Lens Correction or warping. A reviewer calls Affinity's crop "clunky and inaccurate".
- **Evidence:** [helpx content-aware crop](https://helpx.adobe.com/photoshop/desktop/crop-resize-transform/crop-straighten/apply-content-aware-fill-while-cropping-images.html), [Adobe "Cropping with Content Aware"](https://community.adobe.com/questions-712/cropping-with-content-aware-1057925), ["Fixing a curved horizon"](https://community.adobe.com/questions-712/fixing-a-curved-horizon-not-a-crooked-one-a-curved-one-1162185), [Perishable Press](https://perishablepress.com/switching-photoshop-affinity-photo/).

**12. Fix perspective and converging verticals**
- **Photoshop:** Camera Raw Geometry (Upright / Guided), Lens Correction, Edit > Transform > Perspective, Perspective Warp.
- **Affinity:** Perspective tool or live Perspective filter, Develop lens corrections.
- **Pleasant vs painful:** Guided lines; choosing a resampling that avoids softening the stretched top; keeping the edit re-editable.
- **Evidence:** Adobe threads ["Vertical Perspective correction, degradation"](https://community.adobe.com/t5/photoshop-ecosystem-discussions/vertical-perspective-correction-degradation-of-image/td-p/9538400), ["Tools to perspective correct"](https://community.adobe.com/questions-712/tools-to-perspective-correct-1170080), ["Perspective Warp"](https://community.adobe.com/questions-712/perspective-warp-1067410).

## Tier 2: very common journeys (13–30)

**13. Replace the sky**
- **Photoshop:** Edit > Sky Replacement (edge shift and fade, foreground lighting and color adjustment), or Select Sky then mask.
- **Affinity:** manual: select the sky, mask it, use Blend Ranges, then match color.
- **Pleasant vs painful:** Users want sky settings to be saveable.
- **Evidence:** [Adobe sky-replacement page](https://www.adobe.com/products/photoshop/sky-replacement.html); Affinity requests [2020 "auto sky à la Luminar"](https://forum.affinity.serif.com/index.php?/topic/120633-feature-request-automatic-sky-replacement-a-la-luminar/); [Adobe "save settings on sky replacements"](https://community.adobe.com/feature-requests-713/save-settings-on-sky-replacements-653766).

**14. Blur the background (fake shallow depth of field)**
- **Photoshop:** Blur Background quick action, Lens Blur with a depth map, Neural Depth Blur.
- **Affinity:** Lens Blur or Field Blur live filters with a mask; Portrait Blur in Affinity by Canva.
- **Evidence:** PTC's "Most Popular" list includes ["How To Blur Backgrounds"](https://photoshoptrainingchannel.com/how-to-blur-backgrounds-in-photoshop/); [helpx quick action](https://helpx.adobe.com/photoshop/using/quick-actions/blur-background.html).

**15. Change the color of an object (clothing, car) while keeping texture**
- **Photoshop:** Hue/Saturation adjustment layer (Colorize) with a mask, a Color-blend paint layer, or Replace Color.
- **Affinity:** HSL or Recolour adjustment with a mask, Select Sampled Colour.
- **Pleasant vs painful:** Going to much lighter or darker colors loses detail; Replace Color is "crude".
- **Evidence:** Adobe threads ["recolor selection but keep texture"](https://community.adobe.com/questions-716/how-to-recolor-selection-but-keep-texture-1198151) and ["change color of a shirt to light color without losing details"](https://community.adobe.com/questions-712/how-to-change-color-of-a-shirt-to-light-color-without-losing-details-1170367); [helpx](https://helpx.adobe.com/photoshop/desktop/adjust-color/selective-color-adjustments/replace-object-colors-by-applying-a-hue-or-saturation-adjustment.html).

**16. Composite a subject into a new scene and match color and light**
- **Photoshop:** cut out, then clipped Curves per channel, Match Color, Blend If, painted shadows, Harmonize.
- **Affinity:** Blend Ranges, clipped adjustments.
- **Evidence:** PiXimperfect ["Advanced Color Matching Process"](https://www.youtube.com/watch?v=XUD7_JTG_BM) and ["Blend Subject with Background"](https://www.youtube.com/watch?v=4nvtS5i75bE); Adobe thread ["Why placing one image into another looks unnatural"](https://community.adobe.com/t5/photoshop/why-placing-one-image-into-another-looks-unnatural/td-p/10883384); [Harmonize](https://www.adobe.com/learn/photoshop/web/blend-subjects-with-harmonize).

**17. Make a set of photos match each other in color**
- **Photoshop:** Image > Adjustments > Match Color, Camera Raw presets, exported LUTs.
- **Affinity:** LUT adjustment, copy/paste adjustments.
- **Evidence:** Adobe thread ["easiest way to make two or more photos match on colour balance"](https://community.adobe.com/questions-712/what-is-easiest-way-to-make-two-or-more-photos-same-subject-match-on-colour-balance-1080651); [helpx Match Color](https://helpx.adobe.com/photoshop/desktop/adjust-color/selective-color-adjustments/match-color-between-two-images.html); [PHLEARN](https://phlearn.com/tutorial/match-color-two-photos-photoshop/).

**18. Color grading or creating a "look"**
- **Photoshop:** Color Lookup (LUTs), Gradient Map, Selective Color, split-channel Curves, Camera Raw color-grading wheels.
- **Affinity:** LUT, Split Toning, Gradient Map, Selective Colour.
- **Evidence:** [Fstoppers complete guide](https://fstoppers.com/education/complete-guide-frequency-separation-dodging-and-burning-and-color-grading-485517); color grading is on [PHLEARN's top-10](http://phlearn.com/tutorial/aaron-must-know-tools-and-techniques/).

**19. Reshape faces or bodies with Liquify**
- **Photoshop:** Filter > Liquify (Face-Aware, Freeze/Thaw mask, Pin Edges, Show Backdrop, load last mesh), as a smart filter.
- **Affinity:** Liquify persona.
- **Pleasant vs painful:**
  - Pros ask for: a toolbar tool instead of a dialog, "repeat same move on other layers" (Ctrl+F), fast mesh save/load for 2GB+ files, and Alt+right-drag brush resize.
  - Face-Aware silently needs the GPU.
- **Evidence:** ["P: Pro Retouchers need better Liquify"](https://community.adobe.com/feature-requests-713/p-pro-retouchers-need-better-liquify-653496/index4.html) (5+ pages); ["Face-aware not working due to GPU"](https://community.adobe.com/questions-712/liquify-face-aware-not-working-due-to-gpu-1071340).

**20. Warp an object to fit a surface (mockups, bending limbs)**
- **Photoshop:** Warp transform (custom grid, split), Puppet Warp, Perspective Warp, applied to a smart object.
- **Affinity:** Mesh Warp tool or live Mesh Warp, Perspective, Deform.
- **Evidence:** [Affinity "proper Puppet Warp" request](https://forum.affinity.serif.com/index.php?%2Ftopic%2F185611-feature-request-proper-puppet-warp-tool-like-photoshop%2F=); [Affinity mockup workaround](https://www.naxeem.com/articles/create-and-use-photoshop-like-smart-objects-for-mockups-in-affinity-photo/); [Puppet Warp mockup thread](https://community.adobe.com/t5/photoshop-ecosystem-discussions/puppet-warp-tool-hides-important-parts-of-image-when-making-mockups/m-p/14514432).

**21. Blend bracketed exposures (HDR, window pulls)**
- **Photoshop:** Merge to HDR Pro or Camera Raw HDR, then hand-blending with luminosity masks (TK and Raya panels).
- **Affinity:** New HDR Merge, Tone Mapping persona, Select Tonal Range and luminosity live masks.
- **Evidence:** [TK Luminosity Mask panel](https://exchange.adobe.com/apps/cc/83b6b487/tk-luminosity-mask); [DPS exposure blending](https://digital-photography-school.com/exposure-blending-using-luminosity-masks-tutorial/); Adobe ["Real Estate HDR Window Pulls"](https://community.adobe.com/questions-675/real-estate-photography-hdr-window-pulls-953265).

**22. Stitch a panorama**
- **Photoshop:** Photomerge, or Auto-Align followed by Auto-Blend (Panorama), then Content-Aware fill of the edges.
- **Affinity:** New Panorama, with editable per-source masks before rendering.
- **Evidence:** ["Photomerge not creating blending masks"](https://community.adobe.com/t5/photoshop-ecosystem-discussions/photomerge-not-creating-blending-masks/td-p/10412761); ["Photomerge is problematic and needs to be enhanced"](https://community.adobe.com/t5/photoshop-ecosystem-ideas/photomerge-is-problematic-and-needs-to-be-enhanced/idc-p/13799782).

**23. Focus-stack macro or landscape frames**
- **Photoshop:** Load Files into Stack, Auto-Align, Auto-Blend (Stack).
- **Affinity:** New Focus Merge, with a Sources panel for cloning in detail from a specific frame.
- **Evidence:** Adobe bug ["Focus stacking auto-blend poor quality"](https://community.adobe.com/t5/photoshop-ecosystem-bugs/p-focus-stacking-auto-blend-layers-poor-quality/idi-p/12250428) and ["Horrible Results"](https://community.adobe.com/questions-712/focus-stacking-in-photoshop-horrible-results-1174885). Fstoppers praises Affinity's HDR, focus and pano tools ([review](https://fstoppers.com/reviews/affinity-photo-25-imperfect-perfect-alternative-photoshop-it-depends-669573)).

**24. Remove moving tourists with a median stack**
- **Photoshop:** stack the frames, convert to a smart object, Stack Mode > Median.
- **Affinity:** New Stack with the Median operator.
- **Evidence:** [PTC stack-mode tutorial](https://photoshoptrainingchannel.com/remove-tourists-stack-mode/).

**25. Swap heads or eyes in a group photo**
- **Photoshop:** stack the frames, Auto-Align, then mask the better face through.
- **Evidence:** ["Fixing group photos with auto align"](https://community.adobe.com/questions-712/fixing-group-photos-in-photoshop-with-auto-align-1084654); ["Automated head swap"](https://community.adobe.com/t5/photoshop/automated-head-swap/td-p/10470297); [AI head-swap request](https://community.adobe.com/t5/photoshop-ecosystem-ideas/p-ai-head-swap-between-photos/idi-p/15042790); [Adobe face-swap page](https://www.adobe.com/products/photoshop/face-swap.html).

**26. Restore an old, damaged photo**
- **Photoshop:** Dust & Scratches plus a mask or History Brush, clone and heal, Neural Photo Restoration and Colorize.
- **Evidence:** [Adobe restoration page](https://www.adobe.com/products/photoshop/old-photo-restoration.html); threads ["Help with photo restoring"](https://community.adobe.com/t5/photoshop-ecosystem-discussions/help-with-photo-restoring/td-p/15068312) and ["restore a vintage photo"](https://community.adobe.com/t5/photoshop-ecosystem-discussions/how-to-restore-a-vintage-photo/m-p/11199121).

**27. Enlarge for print without stretching or blur**
- **Photoshop:** Image Size with Preserve Details 2.0, Super Resolution, Generative Upscale; Content-Aware Scale.
- **Evidence:** PTC's featured ["Resize WITHOUT Stretching"](https://photoshoptrainingchannel.com/how-to-resize-an-image-without-stretching-it-in-photoshop/); ["Increasing photo size with minimal loss"](https://community.adobe.com/questions-712/increasing-photo-size-with-minimal-loss-of-detail-1172781); ["Upscaling within Photoshop"](https://community.adobe.com/feature-requests-713/upscaling-within-photoshop-655357).

**28. Develop RAW before pixel editing, re-editable later**
- **Photoshop:** Camera Raw as a smart filter or raw smart object.
- **Affinity:** Develop persona, now with embedded or linked RAW layers.
- **Evidence:** Camera Raw is #1 on [PHLEARN's top-10](http://phlearn.com/tutorial/aaron-must-know-tools-and-techniques/). [Fstoppers](https://fstoppers.com/reviews/affinity-photo-25-imperfect-perfect-alternative-photoshop-it-depends-669573) calls Affinity's RAW "basic" and its noise reduction "muddy". Photopea's RAW has only exposure, temperature/tint and contrast ([Aiarty](https://www.aiarty.com/knowledge-base/photopea-vs-photoshop.htm)).

**29. Batch resize, convert and export**
- **Photoshop:** File > Scripts > Image Processor, Actions + Batch or Droplets, Export As / Quick Export.
- **Affinity:** Macros plus New Batch Job, Export persona slices.
- **Evidence:** ["batch resize export"](https://community.adobe.com/t5/photoshop-ecosystem-discussions/batch-resize-export/td-p/14500212); ["I need to batch resize 20 images"](https://community.adobe.com/questions-712/i-need-to-batch-resize-20-images-i-need-pngs-ps2017-1060803); ["Batch Processing and the new Export As"](https://community.adobe.com/t5/photoshop-ecosystem-discussions/batch-processing-and-the-new-quot-export-as-quot-in-photoshop-cc/td-p/8923076). Actions are one of Digital Camera World's [top-5 most-used tools](https://www.digitalcameraworld.com/features/these-are-the-5-photoshop-tools-i-use-the-most-and-5-i-rarely-touch).

**30. Export for web with correct colors**
- **Photoshop:** Export As with Convert to sRGB and Embed Color Profile checked.
- **Pleasant vs painful:** The profile checkbox being off by default causes "dull" exports. Affinity users miss Save for Web and a default export folder.
- **Evidence:** ["Colors look dull in export"](https://community.adobe.com/t5/photoshop-ecosystem-discussions/colors-look-dull-in-export/td-p/11526128); ["export as sRGB jpg colors wrong"](https://community.adobe.com/questions-712/ps-export-as-srgb-jpg-colors-wrong-1162359); ["washed out on export"](https://community.adobe.com/questions-712/colours-get-dulled-down-and-washed-out-on-photoshop-export-1086850).

## Tier 3: important secondary journeys (31–40)

**31. Apply filters non-destructively and re-edit them later**
- **Photoshop:** Smart Objects plus Smart Filters (with a filter mask and blend options).
- **Affinity:** Live Filter layers (masks, opacity, blend modes), but not every filter is available live.
- **Evidence:** [Digital Camera World on Affinity Live Filters](https://www.digitalcameraworld.com/tutorials/master-live-filters-in-affinity-photo-and-create-re-editable-non-destructive-effects); ["Filters as adjustment layers" request](https://community.adobe.com/feature-requests-713/filters-as-adjustment-layers-656369).

**32. Target tones with Blend If or luminosity masks**
- **Photoshop:** Layer Style > Blend If, with Alt-drag to split the sliders.
- **Affinity:** Blend Ranges curves.
- **Evidence:** PiXimperfect ["Blend If Explained"](https://www.youtube.com/watch?v=ysShyX50r6U); Affinity forum ["Blend If compared"](https://forum.affinity.serif.com/index.php?/topic/34244-blend-if-compared-photoshop-on1-raw-and-affinity/).

**33. Sharpen for output**
- High Pass on an Overlay layer (Affinity: live High Pass), Smart Sharpen, selective sharpening of eyes and lashes.
- **Evidence:** the [favorite-techniques thread](https://community.adobe.com/questions-712/what-are-your-favorite-retouching-techniques-in-photoshop-1178865).

**34. Reduce noise**
- Camera Raw Denoise (AI); Affinity Denoise filter and Develop noise reduction.
- **Evidence:** the [Fstoppers Affinity review](https://fstoppers.com/reviews/affinity-photo-25-imperfect-perfect-alternative-photoshop-it-depends-669573) ("muddy").

**35. Extend the canvas or change aspect ratio**
- Content-Aware crop fill, Generative Expand, Content-Aware Scale.
- **Evidence:** Adobe Learn's "reframe with Generative Expand" tutorial.

**36. Whiten teeth and brighten eyes**
- Hue/Saturation (desaturate yellows) with a mask, dodging.
- **Evidence:** Adobe Learn's teeth-whitening tutorial.

**37. Remove glare from glasses**
- Lasso the glare, adjustment layer, clone and heal.
- **Evidence:** [Adobe glasses-glare page](https://www.adobe.com/products/photoshop/remove-glasses-glare.html).

**38. Remove stray and flyaway hairs**
- Healing Brush, Clone Stamp, Remove tool.
- **Evidence:** [Adobe stray-hair page](https://www.adobe.com/products/photoshop/remove-stray-hair.html).

**39. Convert to black and white**
- Black & White adjustment with per-color channel sliders, Gradient Map, Channel Mixer. Affinity has equivalent adjustments.

**40. Build a background shadow for a cutout (product or portrait)**
- Paint a shadow on a Multiply layer, or duplicate-and-transform the subject silhouette; Harmonize can now generate shadows.
- **Evidence:** [PhotoshopCAFE 2026 overview](https://photoshopcafe.com/whats-new-in-photoshop-2026-full-release-overview/).

---

## Power-user details that forum posts complain about when missing

1. **Adjustment layers, clipping masks and layer masks** are the baseline. GIMP's lack of them was its #1 complaint for years ([Cambridge in Colour](https://www.cambridgeincolour.com/forums/thread1259.htm)). Krita users ask for Photoshop-style Photo Filter adjustments ([Krita Artists](https://krita-artists.org/t/is-there-a-photo-filter-adjustment-layer-equivilent-in-krita/94036)).
2. **Smart Objects.** They give lossless re-scaling, Smart Filters with a filter mask, and Stack Modes. Liquify and Camera Raw can run as smart filters. Affinity has only embedded documents, and complex mockup warps break ([naxeem](https://www.naxeem.com/articles/create-and-use-photoshop-like-smart-objects-for-mockups-in-affinity-photo/)). Photopea's layer, smart-object and RAW tooling trails Photoshop ([Aiarty](https://www.aiarty.com/knowledge-base/photopea-vs-photoshop.htm)).
3. **A sample-mode switch on every retouch tool** (Clone, Heal, Spot Heal, Patch, Remove, Inpaint). Options: Current / Current & Below / All Layers, plus Ignore Adjustment Layers, so retouching can go on an empty layer. Wished for: "selected layers only". Affinity adds "Layers Beneath" and a cross-document "Global" source. Users want per-tool defaults.
4. **Clone-source ergonomics:**
   - Overlay (opacity, clipped to the brush, auto-hide).
   - Rotate, scale, flip and nudge the source from the keyboard.
   - 5 saved sources; cloning between documents.
   - Ctrl+H to hide the crosshair.
   - Healing Diffusion, Replace mode and legacy/real-time healing ([Kost](https://jkost.com/blog/2021/12/10-tips-for-the-clone-stamp-and-healing-brush-tools-in-photoshop.html)).
   - Lightroom users complain its heal tool can't handle overlapping strokes and picks bad sources ([request](https://community.adobe.com/feature-requests-564/p-more-photoshop-like-clone-healing-content-aware-brushes-666552/index3.html)).
5. **Refine-edge workflow:**
   - Refine Hair / Smart Radius, Refine Edge brush, Decontaminate Colors.
   - View modes (overlay, on black, on white, black-and-white mask).
   - Output to mask or new layer with mask; re-refining an existing mask.
   - A non-modal or lightweight option (users forced the old Refine Edge back via Shift-click).
6. **Channel and luminosity tools:** Ctrl-click a channel to load it as a selection, Apply Image, Calculations, Color Range by luminance or skin tones, 16-bit luminosity masks. Third-party panels (TK, Raya Pro) exist because users want this in one click.
7. **Blend If / Blend Ranges** with split, feathered sliders per channel.
8. **Liquify pro needs:** Face-Aware; Freeze/Thaw; Pin Edges; Show Backdrop; Reconstruct; repeat last mesh on another layer; fast on 2GB+ files; toolbar tool rather than dialog; GPU fallback.
9. **Multi-frame merges with manual override:** split Photomerge into Auto-Align and Auto-Blend; editable blend masks. Affinity's focus-merge Sources panel and pre-render pano mask editing are praised.
10. **Automation:** Actions, Batch, Droplets, Image Processor, Export As / Quick Export. Affinity Macros and Batch Job cover the core, but users miss saved workspaces, a default export folder and Save for Web ([Perishable Press](https://perishablepress.com/switching-photoshop-affinity-photo/)).
11. **Color management:** embed profile on export, convert to sRGB, soft-proofing, 16-bit and wide-gamut working spaces. The "dull export" threads are perennial.
12. **Crop:** straighten line, content-aware corner fill, crop beyond the canvas, Delete Cropped Pixels toggle. XDA lists Affinity's crop-beyond-canvas and path-from-selection as missing ([XDA](https://www.xda-developers.com/photoshop-features-not-available-in-affinity-photo/)).
13. **Generative and ML baseline users now expect:** on-device Select Subject/Sky/People, Remove tool with distraction detection, Generative Fill/Expand, AI denoise and upscale, Harmonize. Affinity's inpainting is judged weaker than AI fill ([XDA](https://www.xda-developers.com/photoshop-features-not-available-in-affinity-photo/), [Lenscraft](https://lenscraft.co.uk/photo-editing-tutorials/affinity-photo-ai-tools/)).
14. **Brush and pen ergonomics** (Alt+right-drag to resize or change hardness, low-flow pressure painting for dodge-and-burn and masks), plus a History Brush / snapshots (Affinity's Undo Brush) for selectively reverting.
15. **Large-file performance and GPU fallbacks:** Face-Aware Liquify needing a GPU is a recurring support thread.

**Most important to cover first:** journeys 1–12 and 16, 19, 21, 27 and 29, plus details 1–5. They appear in both the beginner and the pro evidence.
