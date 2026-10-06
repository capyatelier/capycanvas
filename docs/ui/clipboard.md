# Copy and paste

[Workspace and UI](README.md) · [Open and import](open-and-import.md) · [Canvas action bar](canvas-action-bar.md)

Copy, Cut and Copy Merged put pixels, or selected images, on the clipboard;
Paste, Paste in Place and Paste Into add them as a new layer. Every host offers
them.

| Command | Default | Result |
| --- | --- | --- |
| **Copy** | Ctrl+C | The active layer's own pixels, before its opacity, mask, blend mode and clipping, times the selection's coverage. Without a selection, the whole layer within the canvas. With Move or Transform on an image layer, the selected images, which paste as images. |
| **Copy Pixels** | none | The active layer's own pixels even where Copy would take images. |
| **Cut** | Ctrl+X | Copy, then Clear Selected on the same layer. It needs a selection and follows Clear Selected's rules. Selected images are removed once they are on the clipboard. |
| **Copy Merged** | Ctrl+Shift+C | The visible image, as an export would show it (visible paper included), times the coverage. |
| **Paste** | Ctrl+V | A copy from Capy Canvas lands where it was copied when that position is in view, otherwise centred in the view, with no handles. An image from another app opens the placement handles. |
| **Paste in Place** | Ctrl+Shift+V (Ctrl+Alt+V in the GIMP keymap) | Always at the copied position, with no handles. An image from another app is centred in the view at full size. |
| **Paste Into** | none (Ctrl+Alt+Shift+V in the Photoshop keymap) | A new image layer with a mask from the selection, which the mask consumes. The pasted pixels or images are centred on the selection's bounds, on whole pixels. It needs a selection. |

- **One layer, one step:** every paste adds one layer above the active layer's
  clipping stack and is one undo step; undoing Paste Into restores the selection.
- **The copied rectangle** is the selection's bounds on the canvas. Hidden
  pixels past the canvas are not copied.
- **Refusals:** Copy is unavailable on the paper, groups, effect and Selection
  Layers, in Quick Mask and while editing a mask, and when the selection misses
  the canvas; Cut also refuses locked and alpha-locked layers. Each gives its reason.
  Cutting or clearing pixels of an image layer offers Add Mask (or Edit Mask), New
  Paint Layer and Rasterize Layer instead.
- **Text fields keep their keys:** Ctrl+C, Ctrl+X and Ctrl+V in a focused text
  field copy and paste text, not pixels.
- **Edit menu and bar:** Edit lists Cut, Copy, Copy Pixels, Copy Merged, Paste,
  Paste in Place and Paste Into. The selection bar's Copy ▾ holds Copy, Copy Merged
  and Cut; with a selection tool, Copy already takes pixels.

## The clip

A copy is composed on a worker from an immutable snapshot, never on the UI
thread: the snapshot renders the rectangle band by band, the selection's GPU
coverage multiplies the rows, and one pass writes two outputs:

- a source image at the document's depth and colour space, which pastes read;
- an sRGB 8-bit PNG, which other apps read. High dynamic range drawings map it
  through their SDR rendition.

Copying an untouched photo whole (no selection, or Select All) keeps the photo's
original samples instead. Copies of more than 2 MP show the import-style
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
  drawing changed meanwhile, the pixels stay and a notice says so.

## Hosts

| Host | Writes | Recognizes its own copy |
| --- | --- | --- |
| GTK | A `ContentProvider` union of `image/png` and `application/x-capycanvas-clip` holding the nonce. | The clipboard offers the private type with the current nonce. |
| Web | `navigator.clipboard.write` with a `ClipboardItem` created synchronously in the key or click task, whose promises settle when the worker finishes; plus `web application/x-capycanvas-clip` where `ClipboardItem.supports` allows it. The raster worker encodes the clip. | The custom format holds the nonce; without custom formats, the page has not lost focus since its last copy. |
| Android | `cacheDir/clipboard/<nonce>.png` through a `FileProvider` URI in `ClipData.newUri`, with the nonce in `ClipDescription.extras`. Only the latest file is kept. | The clip description's nonce. Reading the description shows no clipboard toast. |
| Windows | A `DataPackage` with a `PNG` stream and `art.capycanvas.clip.nonce` holding the nonce. The copy workflow captures on the render owner, encodes on the document worker and spools the PNG, which the UI thread reads asynchronously before writing the clipboard. Only copies over 2 MP show progress. | The private format with the current nonce. Other apps that read only device-independent bitmaps do not see the copy, since converting it would decode on the UI thread. |
| macOS and iPadOS | One pasteboard item with a lazily provided `public.png` and `art.capycanvas.clip.nonce` holding the nonce (an `NSPasteboardItem` data provider, or an `NSItemProvider`). A project task (kind 8) captures on the owner and encodes on the file worker. | The private type with the current nonce; iPadOS checks for the type before reading it, so another app's content shows no paste prompt. On macOS, ⌘X, ⌘C and ⌘V reach a focused text field first. |

The shared parts are `crates/layer-ui/src/clipboard.rs` (commands, capture,
paste and Cut), `crates/layer-render-wgpu/src/snapshot/clip.rs` with
`SnapshotRenderer::selection_coverage_async`, `crates/layer-color/src/clip.rs`
(the one-pass writer) and `crates/layer-host/src/clipboard.rs` (`ClipTask`).

## Tests

- Shared: `crates/layer-ui/src/clipboard_tests.rs`, `crates/layer-color/src/clip.rs`
  and the GPU round trips in `crates/layer-host/src/clipboard.rs`.
- GTK: `native_clipboard_copy_paste_round_trips` in
  `apps/layer-linux/src/clipboard_tests.rs` (keyboard, Copy ▾ with mouse and
  touch, another app reading and writing through `wl-paste` and `wl-copy`,
  another drawing, Paste Into, Cut and a focused text field; run without
  `--tablet`, whose proxy does not forward clipboard requests) and
  `native_clipboard_copy_latency_24mp`.
- Web: `node apps/layer-web/test.mjs --headless --clipboard` with pen, touch and mouse.
- Android: `AndroidInteractionTest#clipboardCopyPasteAcrossDevices` (another app
  reads the URI) and `AndroidRasterTest#clipboardCopyLatency24mp`.
- macOS and iPadOS: `EditorLaunchTests/testPixelClipboard` (keyboard on macOS,
  where the test reads the PNG back from the pasteboard; the in-app Edit menu on
  iPadOS).
- Windows: `apps/layer-windows/scripts/exercise-clipboard.ps1` (keyboard Copy,
  Paste, Paste in Place and Cut, Copy Merged from the selection bar, an image
  written by another process and a focused text field).
