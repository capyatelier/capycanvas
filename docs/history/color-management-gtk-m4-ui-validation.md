# Phase 4 GTK UI corrections

This follows the [UI audit](../development/color-management-m4-ui-review.md),
after the user requested implementation before another review. Work lives in
`~/code/capycanvas3`, branch `capycanvas3`; nothing is pushed.

## User flows and changes

- **Create/open and edit HDR:** New Drawing adds **HDR drawing**. Edit Color
  defaults to **Linear RGB** in HDR documents. A compact **Brightness (EV)**
  control and Edit Color button sit above the color wheel; changing hue retains
  above-white brightness. Black or wholly nonpositive colors still use precise
  numeric entry. Brightness readouts retain values outside the slider's editing
  range; no display projection changes the stored color. Positive HDR intensity
  alone is no longer reported as an out-of-gamut chromaticity.
- **Judge the SDR version:** Document Properties, Export and Proof Setup open
  the same **SDR Appearance** utility window. Exposure, Contrast and Highlights
  preview live, with Compare saved appearance, Reset, Cancel and Apply. Drafts
  never enter save/recovery or document history. Apply is one undoable edit.
- **Understand the view:** **View → Preview SDR** appears beside proofing for
  HDR documents and is enabled only with HDR presentation available and proofing
  off. The footer distinguishes HDR, a requested SDR preview, and Showing SDR.
  Its button opens display details, including the reopen requirement when an
  SDR-created surface cannot present a subsequently converted HDR drawing.
  The status remains usable with the optional zoom/rotation readout hidden.
- **Adjust and inspect:** Curves mark SDR white and their linear range, and put
  curve space/range in Advanced. Old masters receive the new labels too. SDR
  curves hide irrelevant HDR options. Histograms use fitted stop axes, mark
  SDR white and separate logarithmic counts from the brightness axis. Additional
  counts and explanations live in Details.
- **Deliver HDR or SDR:** Export puts Dynamic range and the actual output
  contract first. HDR is one PNG route: BT.2020 PQ, 16-bit, retained transparency.
  Advanced offers explicit clipping; irrelevant profile/depth/matte/ICC options
  are hidden. A single SDR preview avoids comparing two identical mapped images.
  A full-output-size asynchronous range check gates HDR export; the writer
  validates again. Returning from SDR Appearance preserves export choices and
  captures the accepted recipe. SDR delivery and mapped proofing continue to
  share the saved appearance. Preset maintenance is collapsed; the TIFF preset
  says Further editing (SDR) for HDR documents.

No reference-white preference, global HDR workspace, OS HDR switch, extra HDR
format or gain-map promise was added. Exact source/document depth labels now
separate integer SDR from floating HDR.

## Validation

The machine and original baseline remain documented in
[the phase 4 validation record](color-management-gtk-m4-validation.md). New logs,
images and delivered files are under `artifacts/color-m4/ui-*`. This correction
uses the existing shared Rust transforms, storage and bounded worker model.

Final code milestone: `7dd22e4b`.

| Check | Result / evidence |
| --- | --- |
| Shared core, color and UI tests | 622 passed, 7 existing ignored; `ui-shared-tests-final.log` |
| Workspace and all-target compilation | Passed; existing unrelated warnings; `ui-workspace-check-final.log` |
| Native acceptance | Eight passed; `ui-accepted/results.json` |
| GPU HDR numerical regressions | Four passed: half publication, float flattening, exposure/curves and presentation/proof mapping; `ui-hdr-gpu-tests.log` |
| Browser SDR workflows | Passed: conversion, undo/reopen, six photo corrections, source workflows, ICC, presets and delivery; `ui-browser-workflows.log` |
| Browser unsupported HDR behavior | Passed; drawing remains usable after rejection; `ui-browser-hdr-limits.log` |
| Screenshot inspection | SDR Appearance, color entry, curves, histogram, HDR export and footer; `ui-accepted/*.png` |

Native acceptance uses a fresh output directory and separate GTK processes. It
opens the independent FFmpeg PQ fixture; edits exposure/curves and paints HDR;
checks sampling, cancellation, comparison/reset, one-step undo/redo, recovery
snapshot exclusion of the draft, save/reopen, HDR and SDR delivery; applies SDR
Appearance from Export and retains the range choice. Range rejection disables
export until explicit clipping is enabled, and changing it invalidates the old
success. Other cases cover destination preservation, SDR drawing presets,
resizing/presets, histogram lifecycle, and proof/save/reopen/RGB export.

The first integration run found and fixed an export choice reset after returning
from SDR Appearance. Screenshot review found and fixed the zero-height status
HUD, excess preview allocation and buried export range controls. Two obsolete
histogram test assumptions were updated for collapsed Details and shorter status
copy. One intermediate rerun stopped at overwrite confirmation for its earlier
artifact; final acceptance uses a fresh directory. Failed intermediate logs are
retained; final acceptance is `ui-accepted/results.json`.

### Large documents and baseline comparison

The final executable reran 8192×7324 (60 MP) HDR navigation at 3840×2160, scale 2,
120 Hz. An initial observation had request-to-present p99 **13.546 ms**, zero
missed refresh slots and 1096.9 MiB peak process RSS. This observation is
retained as `ui-hdr60.*`; it is not replaced by a more favorable repeat.

A subsequent same-session comparison of the preserved pre-UI executable and
this build produced request-to-present p99 **8.753 / 9.193 ms**, with **0 / 0**
missed refresh slots. Worker CPU p99 was **0.480 / 0.481 ms** and GPU p99
**0.354 / 0.393 ms**. These worker deltas are below the common 0.2 ms allowance.
Logs: `ui-hdr60-before_ui.*`, `ui-hdr60-after_ui.*`. This pair is a supplemental
check, not a replacement for the original repeated M2/parent baselines. The
variation in end-to-end timing remains visible in the record.

The 60 MP stress run with 30 effects, a physical blur and concurrent save/export
passed. Save took **123 ms**; save/reopen plus a **480 MB SDR TIFF** took
**8.08 s**. Whole-process peak RSS was **1558.8 MiB** and sampled
per-process graphics residency peaked at **2046 MiB**, versus 2042 MiB in the
previous stress qualification. Its 202 MB half-float master reopens. Evidence:
`ui-hdr60-stress.json`, `.time.txt`, `.memory.json`, `.capy` and `.tif`.
Memory sampling is supplemental and may miss short peaks; it is not a latency
baseline. The new HDR preflight uses the same cancellable row path as the writer;
the native UI rejection/cancellation check uses a small fixture, not a separate
60 MP preflight benchmark.

The review package's `build-manifest.json` identifies source and binary hashes.

## Desktop launch follow-up

The final review executable also reproduced the earlier desktop startup fault.
Three ordinary launch attempts exited before a canvas appeared; a subsequent
capture launch terminated with SIGSEGV in `gdk_surface_handle_event` in the
system `libgtk-4.so.1`. The backtrace reaches it through GDK event dispatch and
`g_application_run`; it does not establish an application-side root cause.
Evidence: `review/desktop-startup-backtrace.log` and
`review/desktop-startup-instructions.log`. The same stack was already present in
the pre-UI build's `review/startup-coredump.txt`; Vulkan alone is not a complete
mitigation. The isolated Wayland acceptance runs above still pass.

A diagnostic desktop launch with `G_MESSAGES_DEBUG=all G_ENABLE_DIAGNOSTIC=1`
opened the review drawing and stayed running. Diagnostic logging is **not** a
validated fix and was not added to the launcher. Desktop startup reliability
remains a release blocker; the open window can be used for feature feedback.
No GTK library replacement, timing workaround, clipping or precision reduction
was introduced. See the upstream
[GTK event dispatch implementation](https://github.com/GNOME/gtk/blob/4.22.4/gdk/gdksurface.c#L2790)
for the library frame referenced by the local backtrace.

## Qualification limits

Physical HDR display output, calibration, mixed-HDR-monitor moves, constrained
or mobile devices, touch/pen ergonomics and thermal/battery behavior remain
unqualified. Native automation exercises GTK controls, pen publication, software
input/presentation and GPU math on the recorded desktop GPU; screenshots are SDR.
Browser coverage is SDR editing plus explicit HDR rejection, not browser HDR.
The native utility window keeps the canvas visible but blocks editing while its
draft is open. The brightness slider edits −16 to +15 EV; Linear RGB entry covers
the supported signed half range, and sampling/readouts preserve extended values.
HDR curve processing-range changes are document edits, not graph zoom.

Interchange remains noninterlaced 16-bit RGB/RGBA PQ PNG. HLG, gain maps,
HDR HEIF/AVIF/TIFF/EXR, RAW development, layered PSD, native CMYK, OCIO/ACES and
full Float32 documents remain outside this delivery. The storage/reference-white
contract and the original physical-device gaps are unchanged.
