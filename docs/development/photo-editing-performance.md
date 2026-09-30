# Photo editing performance and memory

[Developer guide](README.md) · [Performance targets](../PERFORMANCE_TARGETS.md) · [Known gaps](../performance/known-gaps.md) · [Photo editing M2–M4 record](../history/photo-editing-m2-m4.md)

Status: **open** (2026-09-29). Photo editing M2–M4 is feature-complete on GTK, Web and Android. The performance and memory gates below were not met. Measure by the [measuring rules](../performance/measuring.md) on each tier's reference tablet, record results in the tier tables, and delete this plan once each gate passes or moves to the [known gaps](../performance/known-gaps.md).

## 1. Memory on 24 MP photos (first)

Running out of memory is never acceptable, even on still frames. Whole-canvas work must be bounded: windowed or tiled, with results equal to whole-canvas ones.

**Gate:**
- On a 6 GB tablet (the XP-Pen), a 24 MP 8-bit Perceptual photo document settles under about 1.5 GB PSS.
- Frequency Separation, from preview to Apply, stays under about 2.5 GB, and the device keeps at least 1 GB available throughout.
- On GTK, the Radius preview adds under 200 MiB. Apply retains the new layers' pages plus at most about 300 MiB, and a second Apply adds only its own pages.
- Merge Down, Merge Visible, Flatten and Stamp Visible, which share the bake, stay within the same bounds.

**Implemented:** retired filter images release their bind groups; incremental
bakes capture bounded 1024 × 1024 regions and publish the layer only after the
last region. Display previews use the compositor's region and scale cache.
Android allocation blocks are 16 MiB. Frequency Separation bakes Low once and
constructs High from the quantized Low pages, including after eviction.

The XP-Pen completes the 24 MP radius-8 journey within the peak-memory and free
memory gates. On 2026-09-29, the two-page-refinement build peaks at 2,228 MiB PSS
with at least 1,619 MiB available in the memory diagnostic. Three separate runs
without PSS sampling take 10.14 / 10.29 / 10.80 seconds from Apply through drain,
with at least 1,602 MiB available. Required raster work keeps submission
backpressure; overlapping it can exceed the memory gate even when tracked
renderer storage remains bounded. Aim for 2–5 seconds:
the measured Gaussian kernel cost alone is about 0.81 seconds for 24 MP, with
additional composition, conversion, capture and publication work. Repeated
Apply and the complete merge/undo memory matrix remain to be qualified.

**Method:** measure on GTK first (per-process GPU memory from `nvidia-smi`, plus the renderer's page and memory metrics), and add a windowless test that fails if a bake's retained memory grows. On a tablet, sample available memory every second and force-stop the app below 1 GB. A tablet that runs out of memory reboots, and then waits for its PIN.

## 2. Brush and composite rates on Mali

- **Retouching brushes at 512 px** reach the moving-stroke rate on the mid and top reference tablets with the integrated compositor. See the [mid](../performance/mid-tier.md#retouching-with-the-integrated-compositor) and [top](../performance/top-tier.md#retouching-with-the-integrated-compositor) tables. Healing interruption and dependent painting follow section 3 below.
- **Soft brushes in Perceptual documents:** encoding and decoding each pixel a dab touches costs about 0.5 ms of GPU time per update on the Huion. A 512 px Airbrush drops from 81 to 76 updates/s.
- **Perceptual presentation and drags** cost 0.25–0.7 ms more GPU time on Mali. A Float32-accurate fitted curve cost more than `pow`. Try a lookup table, or an sRGB-encoded texture format for the composite where the document's curve is sRGB.
- **Top tier:** retouching has moving-stroke measurements; the complete ordinary-brush, drag, pan and zoom matrix remains unqualified.

## 3. Still frames that delay the next motion

- **Healing on large strokes** yields between bounded GPU batches, including native raster validation and conversion. Navigation and tool changes continue; dependent paint contacts keep their captured settings and wait in order. Capture publishes after the whole heal. See [responsiveness](../performance/responsiveness.md#healing-finalization) for measured latency and remaining gaps.
- **Short retouching strokes** commit their swept span only in their last frame, which lengthens a small heal's pen-up.
- **Merges on tablets** take one to two seconds on the Huion's canvas thread, and the Android UI then shows a 150–250 ms frame, probably the layer list.
- **Moving a Select All selection** on a 24 MP photo replays the commit in one frame at release.
- **A canvas size change** recomposes the whole display in its first frame. Recompose visible tiles first.
- **Copying on tablets:** a composed 24 MP copy spends most of its time reading the composite back from the GPU in bands.
- **Erase right after a stroke** rewrites every page of a layer that is still pending or holds wet state.

## 4. Frame rates on hosts

- **Web on tablets:** unchanged presents are skipped by the shared presenter. Requalify moving-frame rates on the reference tablets.
- **Liquify on Android:** a 240 Hz stylus stroke renders more often than its input rate. Check whether contacts are rendered more than once.
- **GTK Transform** on `photo24` presents below 120 fps. This was already so before M2.
- **GTK window resize** under the test runner's `color-mgmt` presents at 45–55 Hz in headless Mutter.
- **Pass Through groups:** checkpoints stay on a group's direct children, so a Pass Through group recomposes more of its surroundings than needed.

## 5. Cold shader readiness on GTK

The 61 MP navigation fixture has a 9504 × 6336 U16 ProPhoto document, 31 empty
paint layers and five effects, on a private 1600 × 1000 at 120 Hz Mutter display.
Three alternating old/current pairs on 2026-09-29 use a fresh private
`XDG_CACHE_HOME` and NVIDIA shader-cache path for every run. All six journeys
pass. Old readiness is 6.25 / 6.30 / 6.33 seconds; the two-page-refinement build
is 8.47 / 9.65 / 9.73 seconds. Renderer-owned storage falls from about 2,245 to
1,321 MiB. Neither storage figure is process PSS.

`startup_ready_ms` waits for all queued shaders, pending edits and frame work to
finish. It is not the time to the first visible photo and cannot directly qualify
that response target. The cold readiness regression remains open; attribute
document-critical compilation separately from optional background compilation
before changing startup requirements. Shared user shader caches cannot establish
a cold comparison. Records are
`artifacts/latency-investigation/two-25-private-startup-{old,current}-{0,1,2}.json`.
