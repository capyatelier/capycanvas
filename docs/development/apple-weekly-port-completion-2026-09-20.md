# Apple weekly feature port — 2026-09-20

Follow-up to the [weekly audit](apple-weekly-port-audit-2026-09-20.md).
The existing local HDR work was preserved in `60f67452`, then reconciled with
current main on `apple/week-parity-20260920` in the original Capy Canvas checkout.

## Implemented on macOS and iPadOS

| Audit gap | Result |
| --- | --- |
| Multiple drawings | Shared Rust `DocumentSessions` now owns each drawing's history, tools, camera, save checkpoint and recovery. Native tabs adapt to a compact selector, support independent order undo/redo, keyboard cycling, close decisions, ordered multi-file Open and title-area drops. |
| Resource lifetime | Inactive sessions are boxed, retain immutable backing, release their GPU renderer, and spill above the shared RAM budget. Switching waits for current and history raster captures. Camera generations reject input queued for a previous drawing. |
| Existing HDR integration | Retained the local F16/EDR, live Off/SDR/Print Proof, intensity, histogram/curve and PQ PNG implementation while reconciling newer shared APIs. |
| Float32 and EXR | Creation, precision conversion, inspection, picker depth/range and OpenEXR export are exposed. OpenEXR delivers document-linear Float32 RGB and alpha. |
| Gain-map delivery | JPEG and AVIF output use the shared codecs, format-specific quality/transparency options, correct native file types/extensions and actual encode/decode comparisons with selectable SDR bases. |
| Local-tone analysis | Keeps compatible GPU guides while drawing; cancels pending analysis during interaction and publishes replacement guides at idle boundaries without downloading/reuploading the guide. |
| Edit Color and Proof layout | HDR editing carries document depth and rendition through paint/property/gradient entry points, defaults to Linear RGB, automatically separates intensity, groups numeric fields and shows contiguous Base/Adjusted swatches. Proof modes use a neutral native button row. |
| Display status | Reports actual HDR output, mapped SDR, preparation or analysis errors; accessible Display Details explains headroom and artwork reference white. |
| Smaller presentation gaps | Translucent outlined docking body targets preserve insertion markers; protected Paper has explicit help and accessibility guidance. |

Native tab styling follows the Apple UI while keeping the shared ordering,
selection and close behavior. Tab strips and explicit grips drag after native
movement slop. Compact row bodies use immediate mouse pickup and held touch/pen
pickup; the retained native adapter preserves scrolling and same-contact menus.

## Validation

The regression work covers both Apple platform policies on real Metal, plus
native Swift owner/file execution and actual Mac/physical M4 iPad UI journeys.
Artifacts are retained under ignored `artifacts/apple-week-parity/`.

The tab tests exposed and fixed an undo-backing readiness race, large by-value
session moves on native worker stacks, lost initial brush settings, stale camera
input generations, unnecessary Navigator placement clearing, recovery during
New/Open and a queued external-Open admission race. The shared Unix spill failure
test now uses a read-only descriptor instead of Linux-only `/dev/full`.

A separate physical Pencil sensor qualification, calibrated luminance comparison,
and ten-minute performance matrix are outside these automated checks. The new
controls use the existing device-aware AppKit/UIKit adapters; physical finger
and mouse UI checks do not establish physical Pencil sensor behavior.
