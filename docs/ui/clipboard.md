# Copy and paste

[Workspace and UI](README.md) · [Open and import](open-and-import.md) · [Canvas action bar](canvas-action-bar.md)

Copy and Cut use the current pixel or image selection. Without either, they
copy the selected layers or folders, including their properties. Copy Merged
publishes the visible composite. Every host uses the shared commands below;
Apple uses Command in place of Ctrl.

| Command | Default | Result |
| --- | --- | --- |
| **Copy** | Ctrl+C | Selected image objects, or the active layer's own pixels through the pixel selection. Without either selection, the selected layers and folders, including masks, effects and positions. |
| **Copy Pixels** | none | The active layer's own pixels before opacity, mask, blend and clipping, even where Copy would take images or whole layers. |
| **Cut** | Ctrl+X | Copy first, then remove the copied images, selected paint pixels or whole layers after the system write succeeds. |
| **Copy Merged** | Ctrl+Shift+C | The visible image, including visible paper, multiplied by the pixel selection's coverage. |
| **Paste** | Ctrl+V | Retained content keeps its copied position, even off screen. The Photoshop preset centres it in the document. External images open placement handles at full pixel size, centred in the view (document centre in Photoshop). |
| **Paste to Shown Position** | Ctrl+Shift+V | Centre retained or external content in the visible canvas at full size, without placement handles. |
| **Paste in Place** | none; Photoshop Ctrl+Shift+V; GIMP Ctrl+Alt+V | Retained content keeps the copied position. External images start at the canvas origin, at full size, without handles. |
| **Paste at Cursor** | Ctrl+Alt+V; unbound in Photoshop and GIMP presets | Centre at the canvas pointer, or the view centre when no pointer is available. |
| **Paste as New Image** | Ctrl+Alt+N; GIMP Ctrl+Shift+V; Krita Ctrl+Shift+N | Open an unsaved drawing at the clipboard bounds with transparent paper. Retained layers, depth, colour, blending and image objects survive. External batches keep all images at full size on a canvas large enough for all of them. |
| **Paste Into** | Ctrl+Alt+Shift+V | Centre content on the pixel selection, on whole pixels, in a new image layer with that selection as its mask. Insert above the active layer or inside the active group. Consume the selection. |

Capy and Clip Studio Paint presets follow CSP's Ctrl+Shift+V placement command
and Ctrl+Shift+N for New Layer. New Window has no default chord. Paste Into uses
Photoshop's chord; Paste at Cursor uses Krita's chord. Paste as New Image keeps
Ctrl+Alt+N to leave New Layer intact.
User shortcut overrides remain in effect.

- **One edit:** each paste or cut is one undo step. Undoing Paste Into restores
  the selection. Pixel copies add a paint layer; selected image objects can join
  an editable image layer. Whole-layer copies add independent authored records
  and share immutable pixel resources. Only clipping/effect relationships whose
  targets are also copied are retained; other attachments are detached.
- **Starting from the clipboard:** ordinary Paste creates a clipboard-sized
  drawing on the untouched startup canvas. After editing, or in a drawing
  deliberately created, opened or restored, it adds to that drawing.
  Copied authored layers need their own save even when they contain no original
  photo; closing the new drawing asks to save it.
- **Bounds:** pixel selections copy their bounds on the canvas. Whole-layer and
  selected-image copies retain off-canvas content. Whole-layer PNG renditions
  include the composition frame and the copied layers' full rendering bounds.
- **Position:** the view centre or cursor is captured when Paste is requested,
  before clipboard delivery or decoding can move the view or pointer.
- **Refusals:** Copy Pixels and pixel-selection Copy require a paint or image
  layer. Whole-layer Cut refuses locked layers and dependencies that cannot be
  detached. Pixel Cut also refuses alpha lock. Copy/Cut without an active or
  selected layer are disabled; Copy Merged remains independent of layer focus.
  A locked destination group disables paste into that drawing, while Paste as
  New Image remains available. File operations and unfinished canvas gestures
  block clipboard commands consistently for pixels and objects.
- **Text fields keep their keys:** Ctrl+C, Ctrl+X and Ctrl+V edit native text.
  Shortcut recording captures the chord; settings and modal contexts protect
  the artwork. Browser image paste follows the shared bindings and gates, even
  without an asynchronous clipboard reader. Missing native delivery cancels
  the request instead of leaving the drawing busy.
- **Edit menu and bar:** the clipboard commands stay in the existing Edit menu.
  The selection bar's Copy ▾ holds Copy, Copy Merged and Cut.

## Current limits

Mask, Quick Mask and saved-selection editing do not yet accept clipboard
coverage. Copy/Cut and paste into the current drawing are disabled in those
contexts; Paste as New Image remains available. This prevents mask-focused
Paste from silently adding artwork. Cutting selected pixels from an image layer
still offers Add Mask (or Edit Mask), New Paint Layer and Rasterize Layer.

A pixel selection copies only the active paint/image layer. Multi-layer pixel
selections and selected regions of groups are not yet a structured clipboard.
Whole layers preserve their structure when the destination colour space and
depth match. A different colour space/depth or Paste Into uses the rendered
image with its explicit profile. Native data from other applications needs a
supported raster or image-file representation; text, SVG and foreign layer
formats are not imported as artwork.

## The clip

A copy is composed on a worker from an immutable snapshot, never on the UI
thread: the snapshot renders the rectangle band by band, the selection's GPU
coverage multiplies the rows, and one pass writes two outputs:

- a source image at the document's depth and colour space, which pastes read;
- an sRGB 8-bit PNG, which other apps read. High dynamic range drawings map it
  through their SDR rendition.

Copy Pixels without a selection, or pixel Copy with Select All, keeps an
untouched photo's original samples. Whole-layer copies retain those samples in
the authored layer graph and render a separate public PNG. Copies of more than 2 MP show the import-style
progress with Cancel.

The clip (`PixelClip`) belongs to the window, in `DocumentSessions`, so any of
its drawings can paste it; GTK keeps one clip for the whole application. Beside
the PNG, the system clipboard carries the clip's nonce. A paste whose clipboard
still carries that nonce reads the clip at full depth; anything else is an image
from another app.

- **Colour:** in a drawing with the same colour space and depth, the pasted layer
  holds document pixels; otherwise it keeps the clip as an original image with its
  explicit profile, converted like an opened photo.
- **Cut** captures, then erases once the host reports the copy written. If the
  drawing changed meanwhile, the source stays and a notice says so. A failed system write or cancellation cannot acknowledge Cut.

## Hosts

| Host | Writes | Recognizes its own copy |
| --- | --- | --- |
| GTK | A `ContentProvider` union of `image/png` and `application/x-capycanvas-clip` holding the nonce. | The clipboard offers the private type with the current nonce. |
| Web | `navigator.clipboard.write` with a `ClipboardItem` created synchronously in the key or click task, whose promises settle when the worker finishes; plus `web application/x-capycanvas-clip` where `ClipboardItem.supports` allows it. The raster worker encodes the clip. | The custom format holds the nonce. Without it, Paste reads the current system image; a retained copy and focus history do not establish ownership. |
| Android | `cacheDir/clipboard/<nonce>.png` through a `FileProvider` URI in `ClipData.newUri`, with the nonce in `ClipDescription.extras`. Only the latest file is kept. | The clip description's nonce. Reading the description shows no clipboard toast. |
| Windows | A `DataPackage` with a `PNG` stream, a standard Bitmap stream reference and `art.capycanvas.clip.nonce` holding the nonce. The copy workflow captures on the render owner, encodes on the document worker and spools the PNG, which the UI thread reads asynchronously before writing the clipboard. Only copies over 2 MP show progress. | The private format with the current nonce. The standard Bitmap representation lets consumers request the system bitmap format without an app-side UI-thread decode. |
| macOS and iPadOS | One pasteboard item with a lazily provided `public.png` and `art.capycanvas.clip.nonce` holding the nonce (an `NSPasteboardItem` data provider, or an `NSItemProvider`). A project task (kind 8) captures on the owner and encodes on the file worker. | The private type with the current nonce; iPadOS checks for the type before reading it, so another app's content shows no paste prompt. On macOS, ⌘X, ⌘C and ⌘V reach a focused text field first. |

GTK accepts copied local files through GDK's file list as well as encoded image
formats. The Web also accepts files supplied by a native Paste event, including
when the browser has no asynchronous clipboard reader; focused text fields keep
native text paste. Web clipboard items without a supported image and Android
items without an image URI do not discard neighboring image items. Windows falls
back to bitmap data when a StorageItems payload contains no files.

The shared parts are `crates/layer-ui/src/clipboard.rs` (commands, capture,
paste and Cut), `crates/layer-render-wgpu/src/snapshot/clip.rs` with
`SnapshotRenderer::selection_coverage_async`, `crates/layer-color/src/clip.rs`
(the one-pass writer) and `crates/layer-host/src/clipboard.rs` (`ClipTask`).

## Tests

- Shared: `crates/layer-ui/src/clipboard_tests.rs`, `crates/layer-color/src/clip.rs`
  the layer-import and undo tests in `crates/layer-core/src/authored/occurrence_edits.rs`,
  and the GPU round trips in `crates/layer-host/src/clipboard.rs`, including
  off-canvas group pixels and retained editable layers in new drawings.
- GTK: `native_clipboard_copy_paste_round_trips` in
  `apps/layer-linux/src/clipboard_tests.rs` (keyboard, Copy ▾ with mouse and
  touch, another app reading and writing through `wl-paste` and `wl-copy`,
  another drawing, Paste Into, Cut, a focused text field, copied file URIs and new-image tabs; run without
  `--tablet`, whose proxy does not forward clipboard requests) and
  `native_clipboard_copy_latency_24mp`.
- Web: `node apps/layer-web/test.mjs --headless --clipboard` with pen, touch and mouse, including internal and external new-image tabs. `color-controls-copy.test.mjs` checks denied or unavailable writes, late cancellation, stale ownership, mixed clipboard items and native paste events.
- Android: `AndroidInteractionTest#clipboardCopyPasteAcrossDevices` (another app
  reads the URI, mixed external items and external/internal new-image tabs with
  immediate Copy; run with `-e theme light` and `-e theme dark`) and
  `AndroidRasterTest#clipboardCopyLatency24mp`.
- macOS and iPadOS: `EditorLaunchTests/testPixelClipboard` (keyboard on macOS,
  where the test reads the PNG back from the pasteboard; the in-app Edit menu on
  iPadOS).
- Windows: `apps/layer-windows/scripts/exercise-clipboard.ps1` (keyboard Copy,
  Paste, Paste in Place and Cut, Copy Merged from the selection bar, standard
  Bitmap delivery, external images and file batches, internal pixel and image-object
  copies opened as new drawings, startup Paste, source-tab preservation, Unicode
  saves and a focused text field). Run with `-Theme dark` and `-Theme light`.
