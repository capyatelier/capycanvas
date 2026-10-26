# New drawing

[Workspace and UI](README.md)

New drawing asks for a sheet the way an art store sells one: a size in the
unit the artist thinks in, a resolution, a color mode and a paper. A fresh workspace on every platform
opens on a 4:3 landscape sheet with the current monitor's height in device pixels.
It uses the monitor's PPI, with a minimum of 144 and a fallback of 144 when unknown,
sRGB 8-bit color, Cold press paper at 100% Texture visibility and Natural white.
The selected brush and drawing color are restored from the saved editing state.
New users start with Pencil and drawing color #2E2E2E (30% lightness). Pencil's
default size is 9 px and Pointy Pencil's is 6 px; saved custom sizes are preserved.
If display dimensions are unavailable, the sheet is 1,440 × 1,080 px.

## What artists see

- **Main page.** A preview of the sheet at its proportions, color and grain,
  captioned with the matching preset and the orientation, and how many layers
  this device can hold at that size. Beside it: Name, Size (W and H, a link
  that keeps their ratio, the unit, portrait or landscape, and Resolution with
  common choices), Color (space, depth, blending) and Paper (the texture, with
  its Texture visibility in fainter text below its name, and the paper color).
  Presets ⌃, Cancel and Create sit in the footer; clicking the preview opens
  Presets too. The grain darkens and lightens the paper color with transparent
  black and white marks; hidden paper keeps the grain at 35% opacity.
- **Units.** Millimetres, centimetres and inches keep the sheet's physical size
  when the resolution changes; pixels keep the pixel size. Dragging W or H
  lands on whole pixels and millimetres, tenths of a centimetre and eighths of
  an inch; typed values keep their precision.
- **Limits.** A sheet larger than the GPU accepts, or one that leaves no memory
  for a layer, shows why instead of the size line and cannot be created. Common
  resolutions that would cross a limit are disabled.
- **Presets.** Search, category pills, tiles with a sheet thumbnail, name, size
  and details, and a summary of the picked preset in the footer, whose texture
  name is followed by its visibility, as in "Extra rough 150 %, Natural white". Recent
  comes first once a preset was used; Your presets appears once one is saved.
  Save As… names the current sheet into a category; your own presets have
  Rename and Delete. Select applies the picked preset.
- **Preset orientation.** General sheets, sketch pads, canvases and art prints
  start landscape. Pocket books, journals, botanical plates, cards, bookmarks,
  scrolls, comics pages, zines, posters and portrait device/social formats keep
  portrait. Leaf, Reed screen and Toilet paper keep their physical proportions.
  Square presets stay square, and This screen follows the display's orientation.
- **Paper.** The Choose Texture grid with No paper first, the built-in papers,
  then imported papers, and an Import… tile last. Texture visibility, a 0–300%
  field like W and H, sits at the left of the footer and is disabled for No
  paper. Use Texture applies the paper and its visibility; Cancel keeps both.
  Light and Shows through paint start at their defaults.
- **Texture visibility in presets.** Screens, comics pages, print sizes and
  Blueprint start at 0%: their drawings are reproduced, so a lit sheet would
  print over the real one. Physical papers start at 100%, the relief their
  brushes feel. Newsprint and Chalkboard start at 50%. The rough Full sheet,
  Extra rough, the handmade Journal and the surfaces whose structure is the
  point, Barn board, Cracked earth, Crumpled page, Sidewalk and Toilet paper,
  start at 150% so their surface reads.
- **Paper color.** The Edit Color form without its copy buttons and eyedropper,
  a Color / Transparent choice, and the Paper Colors palette with + and Delete.
  Transparent keeps the Paper layer but hides it.

## Model

All state and rules are shared Rust. Hosts present the view and forward input.

| Part | Where |
| --- | --- |
| Setups, units, built-in presets, paper colors, page text, preset catalog | [`new_drawing.rs`](../../crates/layer-ui/src/new_drawing.rs) |
| The open dialog, its drawers, actions and the Create order | [`new_drawing_dialog.rs`](../../crates/layer-ui/src/new_drawing_dialog.rs) |
| Saved presets, recents, the last custom sheet and the paper palette | `NewDocumentSettings` in [`document_creation.rs`](../../crates/layer-ui/src/document_creation.rs) |
| Device memory for layer capacity | `CanvasRenderer::memory_headroom`, from Vulkan, Metal or DXGI budgets |

An unreported Metal working-set recommendation supplies no GPU budget; sheet
admission uses measured process or system headroom. Optional GPU caches retain
their zero budget without a driver recommendation.

The dialog opens with the last used preset or custom sheet. Built-in preset
identities, saved setups and recents are stored in settings; `This screen`
takes the display's pixels and density from the host.

## Host contract

At startup, hosts capture the current display before creating the blank session.
GTK waits for monitor assignment and Apple waits for the canvas window. Native
bridges and Web pass `ScreenInfo` to the shared startup constructor. Physical
display density is optional: browsers and
iPadOS report pixels without a known PPI. The shared constructor applies the
144 PPI minimum and fallback.

1. New drawing raises the `document` request `new`. The host dispatches
   `UiAction::NewDrawing { action: Open { screen, untitled } }` with the
   display it shows the dialog on and the next drawing's numeric identity
   (`DocumentSessions::upcoming_id`, `upcoming_id` in the tab view). The session
   formats the untitled placeholder in its active language, including while
   the dialog is open; an entered name remains literal.
2. While `UiState.new_drawing` is present, the host presents it and sends a
   `NewDrawingAction` for every input. It renders again whenever the HOST
   region changes.
3. The host keeps its own color editor for the paper color, like Edit Color,
   and reports each value with `PaperColorValue`. Everything else, including
   when Transparent switches back to Color, is decided by the session.
4. `Create` validates against the device's current limits, records the preset
   as recent and leaves a `NewDrawingOrder` with the setup, the name, the paper
   material and the limits. The host builds the drawing from it on its worker
   (`OpenEnvironment::new_drawing` on native hosts) for the pending New request.
   Adoption names the drawing with `inherit_new_drawing_name`.
5. `Back` leaves preset naming, clears a nonempty preset search, then closes the
   drawer. `Cancel`, or `Back` on the main page, closes the dialog; the host completes
   the New request as cancelled.

Android forwards both Escape and the native dialog's Back callback to the shared
`Back` action.

GTK is the reference presentation: [`new_drawing.rs`](../../apps/layer-linux/src/new_drawing.rs)
and its `.new-drawing-*` styles. Drawers slide up over the page; inputs are
34 px high with 6 px between controls, 10 px between rows and 14 px between
sections.

## Checks

- `cargo test --locked -p layer-ui new_drawing` covers presets, units, limits,
  the dialog's actions and the Create order without a window.
- `native_startup_canvas_uses_current_monitor` checks the startup sheet, Pencil
  and drawing color on the private GTK display in both themes.
- `native_new_drawing_dialog_review` walks the GTK dialog in both themes.
- Web's `--new-drawing` and Android's
  `startupCanvasUsesCurrentDisplayInBothThemes` check startup display dimensions,
  Pencil size and drawing color; their New drawing journeys cover both themes.
- Native host tests cover every platform's screen sizing and PPI fallback. Apple
  launch configuration and Windows shared settings tests cover the bridge paths.
