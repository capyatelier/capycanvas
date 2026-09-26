# Research: contextual canvas bars in other editors

[Photo editing research](../photo-editing-research.md) · source report, 2026-09-26 · baseline `5eb45a47`

Web research made by an agent on 2026-09-26. It covers contextual on-canvas bars in Photoshop, Clip Studio Paint, Procreate, Affinity Photo, Pixelmator Pro, Krita, GIMP, Photopea, Fresco, ibisPaint, Infinite Painter, Canva and FigJam, and the platform conventions from Apple, Microsoft, Android and WAI-ARIA. The report calls the component a "contextual canvas bar"; the [research record](../photo-editing-research.md) names it the **canvas action bar** and incorporates the findings in section 5. Line numbers can drift in later commits; verify before relying on one.

---

**Notes on sources**
- helpx.adobe.com and Canva help block automated fetches with a 403. I read the Adobe pages through Wayback Machine copies and give the live URL.
- Reddit could not be fetched, so community evidence comes from Adobe Community, CLIP STUDIO ASK, Krita Artists, GIMP GitLab and the Figma forum.
- "(snippet)" means I only saw a search-engine excerpt of an official page. "(inferred)" means my own conclusion.
- Three parallel sub-researches fed this report. I re-checked the most important claims myself: the Photoshop, Clip Studio Paint and Krita launchers, the Procreate selection bar, Microsoft's CommandBarFlyout, and the Apple Human Interface Guidelines (HIG).

---

## (a) Per product

### Photoshop desktop (Contextual Task Bar) and its siblings
**Contexts and actions**
- **Opened image or pixel layer:** Select Subject, Remove Background. Layers with a mask also get Harmonize (tutorial).
- **Blank canvas:** Import image, Generate image.
- **Active selection:**
  - Refine/Modify selection, Create mask, Fill selection (solid, gradient or pattern), Remove, Generative Fill.
  - After a generation: a model picker, Generate, and arrows to step through variations.
- **Transform (Ctrl/Cmd+T):** rotate clockwise/counter-clockwise, flip horizontal/vertical, **Done/Cancel**.
- **Crop:** Generative Expand. Straighten and ratio are on the bar in older docs; newer docs move them to the Options bar.
- **Mask (after Select and Mask):** add/subtract, View mode, Feather/Density.
- **Other tools:**
  - Gradient: presets, type, reverse, colour/opacity.
  - Shape: fill, stroke colour/width, stroke options.
  - Type: font, size, colour, alignment, bold/italic/underline.
- **Beta launch:** bars for open image, new document, type, selections, and Free Transform when a Smart Object is placed.
- Sources:
  - https://helpx.adobe.com/photoshop/using/contextual-task-bar.html
  - https://helpx.adobe.com/photoshop/desktop/get-started/learn-the-basics/boost-workflows-with-the-contextual-task-bar.html
  - https://helpx.adobe.com/photoshop/desktop/create-open-import-images/create-images/edit-images-with-generative-fill.html
  - https://www.photoshopessentials.com/basics/how-to-make-complex-selections-instantly-in-photoshop/
  - https://www.photoshopessentials.com/photo-editing/how-to-blend-anything-with-harmonize-in-photoshop-2026/
  - https://community.adobe.com/t5/photoshop-beta-discussions/contextual-task-bars-now-in-photoshop-beta/td-p/13658736

**Placement and behaviour**
- It floats on the canvas. The default spot is "bottom center of the document window," and it "hides itself until an appropriate object is selected" (tutorial).
- It "appears under your selection" (tutorial), and a user reports it "jumps to a point below the selected subject."
- Adobe staff: it "moves with you as you work on the canvas." Pinning "will hold your bar (and all subsequent bars) where it was placed … until un-pinned."
- You drag it by a handle on its left end. The ⋯ menu offers **Hide bar, Reset bar position, Pin bar position**, and these "are applied to all bars."
- Window > Contextual Task Bar toggles it, and you can assign a keyboard shortcut.
- Pinning became persistent across sessions in 25.x.
- **Customisation:** none.
- Sources:
  - https://www.teachucomp.com/how-to-use-the-contextual-task-bar-in-photoshop-instructions/
  - https://photoshoptrainingchannel.com/generative-fill-in-photoshop-the-ultimate-guide/
  - https://community.adobe.com/t5/photoshop-ecosystem-discussions/generative-fill-reset-bar-position/m-p/14164613
  - https://community.adobe.com/questions-700/contextual-task-bars-now-in-photoshop-beta-668168/index5.html
  - https://asktimgrey.com/2025/11/03/toggling-contextual-taskbar-visibility/
  - https://community.adobe.com/t5/photoshop-ecosystem-discussions/contextual-task-bar-pinned-position/m-p/14053297

**Photoshop Elements 2026**
- Defaults: Select Subject, Remove Background, Quick Actions.
- Transform and Place: rotate and flip.
- Smart Brush: New / Add / Subtract.
- Photo projects: rotate, zoom, replace, delete, **Done**.
- "In some workflows like Crop, Place, or Transform, the Contextual Task Bar remains visible to keep essential controls accessible." In other words, commit controls survive the hide setting.
- Source: https://helpx.adobe.com/photoshop-elements/using/contextual-task-bar.html

**Illustrator**
- It has per-object-type bars.
- "When you select the Touch type tool, the Contextual Task Bar disappears to let you use the tool's onscreen widgets."
- Source: https://helpx.adobe.com/illustrator/using/contextual-task-bar.html

**Photoshop on the web**
- Selection: Select more / Select less, then Generative fill, Create mask, Adjust, Invert selection, Content-aware fill, Deselect.
- Source: https://helpx.adobe.com/photoshop/web/edit-images/make-selections/select-subjects-automatically.html

**Photoshop on iPad**
- An "active selection properties" bar sits **at the bottom of the workspace**. It holds Deselect, Mask, Erase, Invert, Transform selection and Select similar. More (⋯) holds Refine edge, Cut and Copy merged. Refine edge is its own mode with Done/Cancel.
- Transform: Done/Cancel and flip sit "at the top"; the modes are scale, skew, perspective and distort.
- Sources:
  - https://helpx.adobe.com/photoshop/using/select-mask-on-ipad.html
  - https://helpx.adobe.com/photoshop/using/transforming-objects-ipad.html

**Photoshop mobile (phone)**
- Tap select. Its ⋯ menu holds Select all, Sample all layers, Invert, Clear. You commit with a checkmark.
- Source: https://helpx.adobe.com/photoshop/mobile/edit-images/make-selections/select-subject-of-an-image-with-tap-select.html

**Complaints**
- It is "always in the way," covers faces after AI generation, and covers content near the bottom of the document.
- It keeps "blinking on and off … had to hide it."
- The position resets when another image is opened.
- The pin uses display coordinates, so the bar shifts between monitors.
- Users cannot add Select and Mask to it.
- "Like how someone stands in front of you when you're trying to watch TV."
- **Praise:** the pin, and its value for pen workflows.
- Sources:
  - https://community.adobe.com/t5/idea-photoshop/permanent-pinning-contextual-task-bar/idi-p/13821865
  - https://community.adobe.com/questions-700/contextual-task-bars-now-in-photoshop-beta-668168/index1.html
  - https://community.adobe.com/t5/photoshop-ecosystem-discussions/contextual-task-bar-pinned-position/m-p/14053297

### Clip Studio Paint
**Selection Launcher**
- It is "the grey bar that appears at the bottom of a selection."
- **Defaults:** Deselect, Crop, Invert, Expand, Shrink, Delete, Clear outside selection, Cut and paste, Copy and paste, Move/Transform, Fill, New tone, Settings.
- **Show/hide:** View > Selection Launcher.
- **Moving it:** drag the handle at the bottom (added in Ver. 2.0). A moved position holds until a *new* selection, which puts it back in the default spot. Esc while dragging also resets it.
- **Customising:** fully customisable. You can add items from the main menu, pop-up palette, tools, auto actions and drawing colours, and change icons. On tablets you long-press where desktop uses right-click.
- Sources:
  - https://help.clip-studio.com/en-us/manual_en/330_selection/Selection_Launcher.htm
  - https://www.clipstudio.net/en/dl/release_note_old/v2/
  - https://www.clip-studio.com/site/gd_en/csp/userguide/csp_userguide/510_tool/510_tool_selct_launcher_setting.htm

**Transform launcher (Edit > Transform, including Mesh)**
- It shows only **OK / Cancel** and has its own View-menu toggle.
- Enter, a double-tap or Esc do the same.
- The transform **mode** (Scale, Free, Distort, Skew, Perspective, Mesh, Puppet Warp) is switched in the Tool Property palette, not on the launcher.
- Sources:
  - https://help.clip-studio.com/en-us/manual_en/360_transform/Transform_using_the_bounding_box.htm
  - https://help.clip-studio.com/en-us/manual_en/360_transform/Types_of_transformations.htm
  - https://help.clip-studio.com/en-us/manual_en/360_transform/Transform_using_the_Tool_Property_palette.htm

**Other launchers**
- An Object launcher appears below a selected 3D material.
- A text launcher appears "beneath the text" with OK/Cancel; Android adds undo/redo/cut/copy/paste.
- Sources:
  - https://help.clip-studio.com/en-us/manual_en/660_3d/Editing_a_3D_material.htm
  - https://help.clip-studio.com/en-us/manual_en/480_text/Adding_text.htm

**Simple Mode (tablet/phone)**
- The launcher is fixed at bottom centre.
- Move/Transform pops up Maintain ratio / Free Transform / Mesh.
- Source: https://tips.clip-studio.com/en-us/articles/9939

**Complaints**
- It is distracting, and users want to set its initial position.
- A tips author hides it because the handles, marquee and launcher "get in the way."
- Sources:
  - https://ask.clip-studio.com/en-us/detail?id=81296
  - https://ask.clip-studio.com/en-us/detail?id=108972
  - https://tips.clip-studio.com/en-us/articles/5932

### Procreate
**Selection toolbar**
- It sits **at the bottom of the screen**, not anchored to the selection.
- Modes: Automatic, Freehand, Rectangle, Ellipse.
- Tools: Add, Remove, Invert, Copy & Paste, Feather, Save & Load, Color Fill, Clear.
- Tapping another tool commits; re-tapping Selection cancels.
- Source: https://help.procreate.com/procreate/handbook/selections/selections-interface

**Transform toolbar**
- Also at the bottom of the screen.
- It holds Freeform / Uniform / Distort / Warp, Advanced Mesh (inside Warp), Snapping, Flip H/V, Rotate 45°, Fit to Screen, Interpolation and Reset.
- On the canvas there is a green rotate node and a yellow node that adjusts the bounding box.
- Re-tapping Transform commits.
- Sources:
  - https://help.procreate.com/procreate/handbook/transform/transform-interface-gestures
  - https://help.procreate.com/procreate/handbook/transform/transform-warp

**Liquify**
- Bottom menu with the mode, sliders (Size, Pressure, Distortion, Momentum), Reconstruct, Adjust and Reset.
- Source: https://help.procreate.com/procreate/handbook/adjustments/adjustments-liquify

**Adjustments**
- Layer vs Pencil is chosen in the top bar. You drag left or right on the canvas to change the value.
- Tapping the canvas opens Preview / Apply / Reset / Undo / Cancel.
- Source: https://help.procreate.com/procreate/handbook/adjustments/adjustments-interface

**Other**
- **Copy & Paste menu** (three-finger swipe down): Cut, Copy, Copy All, Duplicate, Cut & Paste, Paste. https://help.procreate.com/procreate/handbook/interface-gestures/copypaste
- **Clone:** a draggable source disc appears mid-screen; press and hold it to lock it. https://help.procreate.com/procreate/handbook/adjustments/adjustments-clone
- **QuickMenu:** radial, user-assigned, not contextual. https://help.procreate.com/procreate/handbook/interface-gestures/quickmenu
- **Procreate Dreams:** Select and Transform are toggled from the top bar, and painting implicitly commits a selection. https://help.procreate.com/dreams/handbook/draw-and-paint/select
- I could not verify any community complaints about Procreate.

### Affinity Photo 2 (and Affinity by Canva, v3)
**Desktop context toolbar**
- It is docked below the main toolbar (or floats in Separated mode), is "always visible," and "cannot be turned off."
- Selection Brush: Add/Subtract, Width, Snap, **Refine**.
- Crop: **Apply/Cancel**, Reset, Straighten, Rotate, presets.
- Mesh Warp: **Apply**, Source/Destination, Reset, Hide Mesh.
- Move: **"Hide Selection while Dragging."**
- Version 3 keeps the same model; I found no bar anchored to the object.
- Sources:
  - https://s3-eu-west-1.amazonaws.com/affinity-docs/help/photo/en-US.lproj/pages/Workspace/contextBar.html
  - https://affinity.help/photo2/en-US.lproj/pages/Tools/tools_crop.html
  - https://affinity.help/photo2/en-US.lproj/pages/Tools/tools_meshWarp.html
  - https://affinity.help/photo2/en-US.lproj/pages/Tools/tools_move.html
  - https://www.affinity.studio/help/workspace-context-bar/ (snippet)

**iPad**
- The context toolbar scrolls when it overflows. Refine is on it.
- The Quick Menu opens with a three-finger swipe or a long press. Its top row is Duplicate, Cut, Copy, Paste, Paste Style, Delete, plus nine context-sensitive, user-reassignable buttons.
- Complaint: the toolbar gets cut off in portrait.
- Sources:
  - https://affinity.help/photo2ipad/en-US.lproj/pages/Workspace/contextBar.html
  - https://affinity.help/designer2ipad/en-US.lproj/pages/Workspace/quickMenu.html
  - https://forum.affinity.serif.com/index.php?%2Ftopic%2F174741-context-toolbar-menu-is-cut-off-affinity-photo-v2-ipad-portrait-mode%2F=

### Pixelmator Pro
- **Mac:** options live in a side pane. Transform commits with "**Done at the bottom of the canvas**."
- **iPad:**
  - Selections get a toolbar at the bottom of the canvas with Invert, Deselect, and More (Add/Subtract/Intersect).
  - Crop: Apply at the bottom of the canvas.
  - Transform: Done at the bottom of the canvas.
- Sources:
  - https://support.apple.com/guide/pixelmator-pro/transform-a-layer-pix57476609d/mac
  - https://support.apple.com/en-au/guide/pixelmator-pro-ipad/pix6e781ba66/ipados
  - https://support.apple.com/en-au/guide/pixelmator-pro-ipad/pixb0ea7e75d/ipados

### Krita
**Selection Action Bar (new in 5.3 / 6.0, 2026)**
- Buttons: Select All, Deselect, Invert, Crop to Selection, Fill with Foreground Color, Copy to New Layer, and a drag handle.
- It is placed beneath the new selection, and dragging it is clamped to the canvas.
- 5.3.2 added a toggle in tool options, a context menu on the bar, and the same actions in the main menus.
- 5.3.3 added a **pin** that sticks the bar to the canvas sides.
- A proposed redesign would anchor it to a selection corner or side, switch orientation automatically, keep the offset across selections and restarts, draw an anchor line, and offer an offset reset.
- Sources:
  - https://docs.krita.org/en/user_manual/selections.html
  - https://krita.org/en/release-notes/krita-5-3-release-notes/
  - https://community.kde.org/GSoC/2025/StatusReports/Rossr
  - https://krita.org/en/posts/2026/krita-5.3.2-released/
  - https://krita.org/en/posts/2026/krita-5.3.3-released/

**Complaints**
- It "BLOCKS the canvas constantly" and appeared "unrequested."
- A user accidentally hit Crop while picking a brush in the overlapping pop-up palette.
- Its position doesn't persist.
- It doesn't follow moved selections.
- Touch targets are too small.
- Transform, Grow/Shrink and Feather are missing.
- A Drawpile developer asked for text labels, Deselect first, and no Select All because it "obliterates" the selection.
- Sources:
  - https://krita-artists.org/t/discussion-for-changes-to-new-feature-selection-actions-bar/178071
  - https://krita-artists.org/t/feedback-for-the-new-selection-action-bar-in-krita-5-3/141290

**Pop-up palette and transform**
- The pop-up palette spawns at the cursor on right-click.
- Transform has no buttons on the canvas: options are in the docker and a right-click menu, and Enter applies.
- Sources:
  - https://docs.krita.org/en/reference_manual/popup-palette.html
  - https://krita.org/en/posts/2018/krita-4-1-release-notes/

### GIMP 2.10 / 3.x
- **Overlay dialogs (primary source: the code):** the transform tools and Foreground Select show dialogs as overlays attached to the canvas's **top-right**.
  - They only attach when the canvas is more than 2× the dialog's width and more than 3× its height; otherwise they become a separate window.
  - A detach button makes the dialog permanently separate.
  - Buttons: Reset / Readjust / Cancel / an OK labelled per tool.
  - Sources:
    - https://gitlab.gnome.org/GNOME/gimp/-/raw/master/app/display/gimptoolgui.c
    - https://gitlab.gnome.org/GNOME/gimp/-/raw/master/app/tools/gimptransformgridtool.c
- **Maintainer issue:** the overlays are "often broken with tablets," can be invisible when zoomed in, and "often the GUI is in your way." https://gitlab.gnome.org/GNOME/gimp/-/issues/1262
- **Commit without buttons:** Crop commits with Enter or a click inside the rectangle; Foreground Select uses Enter. https://docs.gimp.org/3.0/en/gimp-tool-crop.html
- **Text editor:** the on-canvas text editor can be dragged out of the way as of 3.2. https://www.gimp.org/release-notes/gimp-3.2.html

### Photopea
- Free Transform: Enter/Esc, or commit and cancel in the **top options bar**.
- Refine Edge: a button in the top panel of the selection tools.
- I found no bar on the canvas.
- Sources:
  - https://www.photopea.com/learn/free-transform
  - https://www.photopea.com/learn/refine-edge

### Fresco, ibisPaint, Infinite Painter
**Fresco**
- A Selection Actions bar includes Erase and Transform selection "in the bottom bar," and there is a Mask Actions bar with Reveal/Hide.
- Transform: Done/Cancel at the top.
- The 7.3 (2026) move of the toolbars to the top drew backlash ("why … tools above what I'm drawing").
- Sources:
  - https://helpx.adobe.com/fresco/using/layer-masks.html
  - https://helpx.adobe.com/fresco/how-to/use-selections-make-artwork.html
  - https://helpx.adobe.com/fresco/using/free-transform-tool.html
  - https://community.adobe.com/questions-646/request-to-revert-the-ui-changes-in-the-latest-fresco-update-1560871

**ibisPaint**
- Lasso: a bar at the bottom with Set/Add/Subtract, Invert and Clear.
- Paste goes straight into Translate Scale.
- Transform modes: Translate Scale, Perspective and Mesh (with Division X/Y), plus interpolation. Commit with ✓/OK.
- Sources:
  - https://ibispaint.com/lecture/index.jsp?no=09&lang=en
  - https://ibispaint.com/lecture/index.jsp?no=52&lang=en
  - https://ibispaint.com/lecture/index.jsp?no=154&lang=en

**Infinite Painter**
- Transform bar: Cancel, Unlock Bounding Box, Mode (Basic/Anchor/Distort/Warp), Flip, Rotate 45°, Stamp, Confirm.
- Selection bar: tool picker, Add/Subtract, Cancel, Confirm. It "is scrollable; not all icons may be visible."
- The main toolbar can be dragged with two fingers and snaps to edges.
- Sources:
  - https://docs.infinitestudio.art/painter/transform/workspace/
  - https://docs.infinitestudio.art/painter/selections/workspace/
  - https://docs.infinitestudio.art/painter/studio/

### Figma, FigJam, Canva and platform conventions
**Canva**
- A floating toolbar "appears above or below the element," with lock and similar actions under More.
- Cmd/Ctrl+F1 focuses the toolbar.
- Sources: https://www.canva.com/help/add-elements/ and https://www.canva.com/help/screen-reader-editor/ (snippets)

**FigJam**
- Selected objects get an object toolbar with ⋯ (Group, Lock).
- The screen-reader region for it is skipped when nothing is selected.
- Sources:
  - https://help.figma.com/hc/en-us/articles/4502073572247-FigJam-for-iPad
  - https://help.figma.com/hc/en-us/articles/14477051168791-Use-FigJam-with-a-screen-reader

**Figma Design (UI3)**
- It uses a fixed bottom toolbar. 188 forum replies ask to move or dock it.
- Sources:
  - https://www.figma.com/blog/our-approach-to-designing-ui3/
  - https://forum.figma.com/suggest-a-feature-11/allow-us-to-dock-move-the-new-ui3-toolbar-7861/index4.html

**Apple HIG (edit menus)**
- It appears "above or below the insertion point or selection" depending on space.
- Show only relevant commands.
- Touch gets a compact horizontal row with a chevron to expand; pointer and keyboard get a vertical menu.
- Source: https://developer.apple.com/design/human-interface-guidelines/edit-menus

**Microsoft CommandBarFlyout**
- When it appears on its own (for example on selection), it opens collapsed with a ⋯ "see more" button and **does not take focus**.
- When opened on request (right-click), it opens expanded and takes focus.
- Common commands go in the primary set, and primary commands do not auto-overflow.
- Source: https://learn.microsoft.com/en-us/windows/apps/design/controls/command-bar-flyout

**Android floating ActionMode (from the AOSP source)**
- It hides while the content is moving (50 ms delay) and reappears afterwards.
- `hide()` lasts at most 3 s.
- Placement order: above if it fits, else below, else as high as possible; centred horizontally.
- Sources:
  - https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/master/core/java/com/android/internal/view/FloatingActionMode.java
  - https://raw.githubusercontent.com/aosp-mirror/platform_frameworks_base/master/core/java/com/android/internal/widget/floatingtoolbar/LocalFloatingToolbarPopup.java

**W3C WAI-ARIA toolbar pattern**
- One Tab stop for the whole toolbar, arrow keys move between controls, and it needs `aria-label`.
- Source: https://www.w3.org/WAI/ARIA/apg/patterns/toolbar/

---

## (b) Actions per context that at least two products share

**Layer selected, no selection**
- Select Subject and Remove Background: Photoshop, Elements, web, iPad. These are all Adobe.
- Duplicate / Cut / Copy / Paste / Delete: Procreate's copy-paste menu, Affinity iPad Quick Menu, Canva, FigJam.
- Transform entry.
- Product-specific: Harmonize, Quick Actions.

**Selection being built**
- New/Add/Subtract (plus Intersect): Procreate, ibisPaint, Elements Smart Brush, Photoshop web (more/less), Pixelmator iPad, Infinite Painter.
- Tool shape: Procreate, Infinite Painter.
- Confirm/Cancel: Infinite Painter, Photoshop mobile, GIMP Foreground Select.

**Completed selection**
- **Deselect:** Photoshop web/iPad, CSP, Krita, Pixelmator iPad, Fresco, Infinite Painter.
- **Invert:** Photoshop web/iPad/mobile, CSP, Krita, Procreate, Pixelmator, ibisPaint.
- **Fill:** Photoshop, CSP, Krita, Procreate.
- **Clear/Erase/Delete:** CSP, Photoshop iPad, Procreate, Fresco.
- **Copy/cut to new layer:** CSP, Krita, Procreate, Photoshop iPad.
- **Transform/move contents:** CSP, Fresco, Infinite Painter, Photoshop iPad (which transforms the selection outline).
- **Refine/Feather/Expand/Shrink:** Photoshop, Photoshop iPad, CSP, Procreate, Affinity.
- **Mask from selection:** Photoshop desktop/web/iPad, Fresco (snippet).
- **Crop to selection:** CSP, Krita.
- **Product-specific:** generative actions (Adobe), Save/Load (Procreate), New tone (CSP).

**Transform**
- **Commit/Cancel:** Photoshop, CSP, Fresco, Photoshop iPad, Pixelmator, Infinite Painter, ibisPaint, GIMP. Procreate commits by re-tapping the tool.
- **Flip H/V:** Photoshop, Elements, Procreate, Fresco, Photoshop iPad, Infinite Painter.
- **Rotate by a step:** 90° in Photoshop and Elements; 45° in Procreate and Infinite Painter.
- **Mode switch (Free/Uniform/Distort/Perspective/Warp/Mesh):**
  - On the bar in Procreate and Infinite Painter.
  - In a side panel in CSP, Fresco, Photoshop iPad and Krita.
- **Reset:** Procreate, GIMP.
- **Interpolation:** Procreate, ibisPaint.

**Mesh warp**
- Apply/Cancel: Affinity, CSP, ibisPaint.
- Reset: Affinity, Procreate.
- Grid density: ibisPaint Division X/Y. CSP's grid count is inferred, not verified.
- Hide mesh: Affinity only.

**Placed or pasted image**
- Enters transform straight away with commit/cancel plus rotate/flip: Photoshop (Smart Object place), Elements Place, ibisPaint paste, Infinite Painter import.
- GIMP 3 pastes to a new layer.

**Crop**
- Apply/Cancel: Affinity, Pixelmator, GIMP (Enter).
- Straighten and ratio: Photoshop, Affinity.
- Reset: Affinity.

**Retouch / clone source**
- Only Procreate's draggable, lockable source disc is verified. That is not enough evidence to call it a pattern (inferred).

**Mask editing**
- Add/subtract (Reveal/Hide) brush: Photoshop, Photoshop iPad, Fresco.
- View mode: Photoshop, Photoshop iPad.
- Feather, smoothing or density: Photoshop, Photoshop iPad.
- Done/Cancel: Photoshop iPad Refine edge.

**Liquify / adjustment preview**
- Apply, Reset and Cancel: Procreate Adjustments and Liquify, Affinity Mesh Warp, GIMP dialogs.
- Preview toggle: Procreate. Hide overlay: Affinity.
- A mode picker plus size/strength sliders: Procreate Liquify.

---

## (c) Consistent placement and behaviour rules

1. **Anchor below the object by default, and flip above when there is no room.**
   - Below: CSP ("bottom of a selection," "beneath the text"), Krita ("beneath the new selection"), Photoshop ("under your selection").
   - Flip: Apple HIG, Android and Canva go above or below depending on space.
   - Krita users asked to use the top when the selection is near the bottom edge.
2. **Keep it inside the visible canvas or viewport.** Krita clamps dragging to the canvas and Material says not to exceed the window edge. GIMP detaches the dialog into a window when the canvas is too small.
   - Fallback when the object fills the viewport: pin to a viewport edge (inferred). Krita's 5.3.3 edge pin and the fixed bottom bars on tablets support this.
3. **A fixed bottom bar is the norm on tablets and phones.** Procreate, Photoshop iPad, Pixelmator iPad, ibisPaint and CSP Simple Mode all use one.
   - Offer the anchored bar and a fixed-edge bar as one setting (inferred).
4. **Movable by a dedicated handle, with an explicit position lifetime.** Photoshop's handle is on the left, CSP's at the bottom, Krita's is a drag handle.
   - A moved position lasts until the next new selection in CSP, and until un-pinned in Photoshop, where it applies to all bars.
   - Every product has a reset (Photoshop menu, CSP Esc, Krita reset).
5. **Pin.** Photoshop and Krita both have one. Store it relative to the window, not the display.
6. **Global hide, with a menu item and a shortcut.** Photoshop, CSP, Krita and Illustrator all have one.
   - Commit and cancel must not disappear with it. Elements keeps the bar in Crop, Place and Transform, and CSP gives the transform launcher its own toggle.
   - Enter, Esc and double-tap always work as alternatives (CSP, GIMP, Photopea, Krita).
7. **Hide while things move.** Affinity has "Hide Selection while Dragging," and Android hides the bar while the content moves and restores it afterwards. Illustrator removes the bar when a tool has its own on-canvas widgets.
   - Debounce the reappearance so the bar doesn't flicker (Photoshop's "blinking" complaint).
8. **A small primary set plus a ⋯ overflow.** Photoshop, Photoshop iPad, Canva, FigJam, Microsoft and Apple all do this. Put pin/reset/hide in the overflow, as Photoshop does. Scrolling bars (Affinity iPad, Infinite Painter) hide actions.
9. **Mode switches appear on the bar only on touch-first apps.** Procreate and Infinite Painter put them there; desktop apps keep them in panels.
10. **Accessibility.** The bar appears without stealing focus (Microsoft). A shortcut moves focus to it (Canva Ctrl/Cmd+F1, FigJam F6). Inside, it is one Tab stop with arrow keys (ARIA).
11. **Customisation.** CSP lets you customise the whole launcher; Affinity iPad and Procreate let you reassign menus. The lack of it in Photoshop is a recurring complaint.

## (d) Pitfalls to avoid

- **Covering the work,** especially the lower part of the subject and results right after a generation (Photoshop, CSP, Krita, Adobe Express, Figma).
- **Jumping or flickering:** "popping everywhere," "blinking" (Photoshop), "moves unpredictably" (Krita).
- **Positions that don't persist** across images or selections, or pins tied to screen coordinates on multiple monitors (Photoshop, CSP, Krita).
- **Destructive actions within reach of other transient UI:** accidental Crop in Krita. Keep one-tap destructive actions undoable and away from overlapping popups (inferred), and leave out Select All.
- **Appearing unrequested with a toggle that's hard to find** (Krita).
- **Small or unlabelled icons on touch** (Krita, Drawpile). Only primary actions should be visible.
- **Overlays that break with tablets or disappear when zoomed** (GIMP #1262).
- **Bars cut off in portrait** (Affinity iPad).
- **A placement the user can't change,** e.g. above the drawing hand (Fresco 7.3 backlash, Figma UI3).
- **Leaving out the core next step:** Krita users complained that Transform is missing.
