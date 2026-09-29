# Photo editing performance and memory

[Developer guide](README.md) · [Performance targets](../PERFORMANCE_TARGETS.md) · [Known gaps](../performance/known-gaps.md) · [Photo editing M2–M4 record](../history/photo-editing-m2-m4.md)

Status: **open** (2026-09-28). Photo editing M2–M4 is feature-complete on GTK, Web and Android. The performance and memory gates below were not met. Measure by the [measuring rules](../performance/measuring.md) on each tier's reference tablet, record results in the tier tables, and delete this plan once each gate passes or moves to the [known gaps](../performance/known-gaps.md).

## 1. Memory on 24 MP photos (first)

Running out of memory is never acceptable, even on still frames. Whole-canvas work must be bounded: windowed or tiled, with results equal to whole-canvas ones.

**Gate:**
- On a 6 GB tablet (the XP-Pen), a 24 MP 8-bit Perceptual photo document settles under about 1.5 GB PSS.
- Frequency Separation, from preview to Apply, stays under about 2.5 GB, and the device keeps at least 1 GB available throughout.
- On GTK, the Radius preview adds under 200 MiB. Apply retains the new layers' pages plus at most about 300 MiB, and a second Apply adds only its own pages.
- Merge Down, Merge Visible, Flatten and Stamp Visible, which share the bake, stay within the same bounds.

**State on main:** a 24 MP Frequency Separation runs the XP-Pen out of memory and reboots it. Known causes:
- Bind-group caches (`RecentBindings` in `scene.rs`) keep retired filter images alive. Every bake window and every windowed display filter therefore stays resident, which undoes the windowing.
- Mali driver memory grows with the number of command buffers queued at once.
- The Radius preview (`CanvasEngine::set_layer_preview`) blurs the whole layer instead of the displayed windows.
- On Android, the GPU allocator's 128–256 MiB blocks strand memory.
- Decoded photo tiles stay cached during bakes.
- Working pages are Float32, so every dense 24 MP layer costs 384 MiB of GPU memory. This is architectural; size it as its own task.

**Known trade-off:** fixes for the first five causes reach the gate on the Huion and the XP-Pen. However, bounding submissions in flight on every device slows GTK `native_frame_pacing` Transform on `photo24`. Keep the bound on Mali, and allow more batches in flight where memory allows.

**Also:** the 24 MP Frequency Separation Apply took 20–27 s in one frame on the Huion. That is close to the workspace's 30 s ownership lease, and one run failed with "Workspace ownership needs recovery". Spread bakes across frames.

**Method:** measure on GTK first (per-process GPU memory from `nvidia-smi`, plus the renderer's page and memory metrics), and add a windowless test that fails if a bake's retained memory grows. On a tablet, sample available memory every second and force-stop the app below 1 GB. A tablet that runs out of memory reboots, and then waits for its PIN.

## 2. Brush and composite rates on Mali

- **Retouching brushes at 512 px** (Clone Stamp, Healing, Spot Healing) complete 57–62 updates/s on the mid tier against a target of 90. Their soft edge keeps a wide band of pixels in the dab loop.
- **Soft brushes in Perceptual documents:** encoding and decoding each pixel a dab touches costs about 0.5 ms of GPU time per update on the Huion. A 512 px Airbrush drops from 81 to 76 updates/s.
- **Perceptual presentation and drags** cost 0.25–0.7 ms more GPU time on Mali. A Float32-accurate fitted curve cost more than `pow`. Try a lookup table, or an sRGB-encoded texture format for the composite where the document's curve is sRGB.
- **Top tier unverified:** M4's tablet gate is that clone, heal and ordinary strokes, drags, pan and zoom hold 120 fps on the MovinkPad Pro 14. It was never measured, because that tablet was reserved for another session.

## 3. Still frames that delay the next motion

- **Healing on large strokes** runs in one pen-up frame: about 0.4 s for a 1 MP Healing stroke and 0.6 s for Spot Healing on the Huion, and seconds for 4 MP. Split the pyramid levels and sweeps across frames, holding back the raster capture until the heal finishes.
- **Short retouching strokes** commit their swept span only in their last frame, which lengthens a small heal's pen-up.
- **Merges on tablets** take one to two seconds on the Huion's canvas thread, and the Android UI then shows a 150–250 ms frame, probably the layer list.
- **Moving a Select All selection** on a 24 MP photo replays the commit in one frame at release.
- **A canvas size change** recomposes the whole display in its first frame. Recompose visible tiles first.
- **Copying on tablets:** a composed 24 MP copy spends most of its time reading the composite back from the GPU in bands.
- **Erase right after a stroke** rewrites every page of a layer that is still pending or holds wet state.

## 4. Frame rates on hosts

- **Web on tablets** redraws the whole WebGPU canvas every animation frame. Skip unchanged presents.
- **Liquify on Android:** a 240 Hz stylus stroke renders more often than its input rate. Check whether contacts are rendered more than once.
- **GTK Transform** on `photo24` presents below 120 fps. This was already so before M2.
- **GTK window resize** under the test runner's `color-mgmt` presents at 45–55 Hz in headless Mutter.
- **Pass Through groups:** checkpoints stay on a group's direct children, so a Pass Through group recomposes more of its surroundings than needed.
