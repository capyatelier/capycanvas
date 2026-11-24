# Display consistency and performance follow-ups

[Design history](../README.md)

This records open work left after the November 2026 investigation into stale and flickering tiles under a wide pen, and the display-consistency audit that followed. Each item has enough context to start from. Line numbers are approximate as of `origin/main` on 2026-11-24. Check the code before relying on them.

## Background

A 2000 px G-Pen over a 61 MP photo showed tile-shaped areas behind the leading edge that flickered back to the photo, or stayed wrong until pen-up, on GTK and Android. Four separate causes were fixed: a budget break, a refused contact, a retracted tip, and the reduced-preview saturation check that read the wrong texel. A static audit and fault-injected tests then found more members of the same class.

**The class of bug: display inconsistency.** At some presented frame, part of the image disagrees with the document state the renderer has already accepted (authored raster plus current preview), outside approved approximations, and the renderer does not know it. It shows up in four ways:
- **silent staleness:** a region shows an older state while treated as current;
- **mixed states:** one frame combines regions from different document states;
- **reversion:** a region returns to content older than what an earlier frame presented;
- **unbounded lag:** a known-stale region is never scheduled for repair.

**The four obligations every stage must keep:**

| Obligation | What it requires |
| --- | --- |
| **V: value** | Each write computes the declared operator on its declared inputs. A partial write proves its kept complement still meets the contract. |
| **D: dependencies** | Each cached result records every input it read, including destination pixels it kept or patched. Reuse needs equality since that baseline. |
| **C: truthful publication** | Coverage, quality and "current" claims attach to the actual write, and become visible only on successful submission. A stand-in is current, or pending and never older than what that region last presented. |
| **L: bounded repair** | Every visible provisional has an owner and a runnable step or live wake, and is repaired within a bound. |

The presented frame equals the reference by induction over frames, then over the stages within a frame, with earlier presented output as an input edge. The argument assumes:
- every input is declared;
- storage stays valid while a label refers to it;
- GPU reads and writes are ordered;
- discard, completion and host acknowledgement are truthful;
- an in-place update counts as a new version;
- approximation contracts are stated per operation.

Approximations do not compose automatically. A nonlinear blend over a filter halo is a counterexample.

## Tests that guard the class

`scene::scale::tests::display_oracle` (`crates/layer-render-wgpu/src/scene/scale/display_oracle_tests.rs`) replays seeded host-shaped packet streams. It compares every **presented** frame with a level-0 reference render. The streams use persistent and preview batches in one packet, retracted predictions, self-crossing strokes, every zoom level and pans.

**Seeded faults:**
- held compiles;
- evicted sources;
- refused cells;
- a discarded frame;
- a view move mid-stroke;
- held decodes across a pan.

**Each frame must satisfy:**
- **never older than shown:** a texel never returns to an older accepted state on the same support;
- **bounded repair:** within 8 frames after a fault clears;
- **no wake-less stall:** the renderer never waits while still mismatched with nothing outstanding;
- **no mislabelled cell:** no shown cell carries a label that doesn't match its inputs.

**Adding a brush** is one `Brush` constant and one case line. Wet and Watercolor are excluded until their rewrite.

**Mutation check.** These deliberate bugs are caught: a dropped invalidation, a discarded receipt accepted, an older stand-in selected, a suppressed wake, and a hidden contact cell.


## Release gate, 2026-11-24

Candidate `1390c5db3` against control `9529ec478`:

| Check | Result |
| --- | --- |
| Display oracle | 15/15 in 3 serial runs and 4 parallel copies |
| Renderer library, full | 3 candidate-only failures. All were test drift or races, fixed in `45fd84f84`, `1b2755475` and `1390c5db3`. 6 more passed alone. |
| `layer-engine` | 296 pass |
| `layer-host` | 2 failures, also on the control (follow-up 13) |
| GTK journeys | 12 journeys in light and dark: candidate 24/24, control 23/24. No candidate-only failures. |
| Windows fixtures | 61/61 on both builds: smoke, canvas editing, Navigator, layers, idle brush, touch, Enclose Fill |
| Tablet ABBA | One row below control in both orders: MovinkPad 11 G-Pen 512 px at 100%, 120.9 → 116.7 updates/s, GPU composition +19%, traced to the per-frame overview absorption in `878556e86`. Pro 14 at 100% was −8.9% completed but unchanged fresh. Every other row was within spread. Fixed by `f4bccd601`: the absorption now runs only when the overview is handed over. A MovinkPad 11 and Pro 14 recheck is pending. |
| Settled canvas waking its host | A periodic memory sample woke idle hosts about every 0.5 s, since `5e196eb79`. Fixed by `274fef634`, which forwards ledger wakes only when the display owes the ledger. |

## Open follow-ups

### 1. Exact refinement over an incomplete Clone page
- **Symptom:** a zoomed-out page built over a Clone page whose donor was cold can be committed as `Accuracy::Exact`. `exact_capture` in `scene/scale/refinement.rs` commits Exact without checking that its capture's inputs were complete.
- **Repro:** use the `committed_miss` helper in `retouch_sources_tests.rs`, zoom out, and run idle refinement.
- **Candidate:** wrap the capture in `Scene::certified`. If it is incomplete, commit Display and wait on `Input::Name`. But the cell then never settles while the incomplete page stays, and four existing cold-donor tests (Fill edge band, Frequency Detail, transform, Heal tone) time out in `submit_settled`.
- **Decision needed:** either accept an unsettled display until stroke replay, with an idle-stopping test driver, or add a store state for "refined but not exact for this key".
- **Impact:** rare. It needs a cold Clone donor, and the replay after pen-up rewrites the page.

### 2. A bake from a native capture of incomplete pixels
- **Symptom:** native capture (`raster/native_edit.rs` ~599–626) encodes a working page into a backing tile without its completeness bit. A later decode counts as complete (`source_access.rs` ~89), so a merge or bake from it publishes complete pixels.
- **Fix needs:** the backing tile to remember the bit, for example a completeness field on `RasterTile::pending_from_live` in layer-core read when decoded, or a renderer registry of incomplete captures. Refusing to capture deadlocks, because the replay that repairs the page needs frames.
- **Repro:** the cold-donor setup, mark the low layer's raster pending so native capture encodes it, then merge into the photo layer.
- **Impact:** rare, and self-repairing after replay.

### 3. Watercolor near strokes are 7–15% slower than before the audit fixes
- **Measured** (ABBA, control `9c2b6c3ea`), in completed updates/s:

| Tablet | Watercolor Wash | Wet Watercolor |
| --- | --- | --- |
| TCL | 7.47 → 6.31 | 7.39 → 6.31 |
| MovinkPad 11 | 17.8 → 15.1 | 17.3 → 14.7 |
| Pro 14 | 104.8 → 96.5 | 100.7 → 93.8 |

- **Cause:** the wet-edge fix reads the near wetness band on every composite pixel: 10 loads per pixel against 6.
- **Tried:** a tiled compute composite sharing those reads through workgroup memory. It was bit-identical but 22–47% slower on Mali and Adreno.
- **Untested:** splitting transport into halves or quadrants only where reach needs it, and reusing an already-loaded wetness value.
- **Plan:** the Wet and Watercolor engines are being rewritten. Fold this into the rewrite. If the rewrite slips past the following release, decide whether to ship the regression, or try a per-page wetness occupancy summary that proves a halo uniform.

### 4. Opaque Gouache about 5% slower
The control read zeros for paint beyond the 3×3 pages around its last contact. Correct far-tap reads cost one sliver gather per exchange. Parity would need a wider exchange binding layout: more sampled-texture slots, or a second bind group. Accepted as a correctness cost.

### 5. A resting pen keeps the display busy
- **Symptom:** while a pen rests in contact with nothing to repair, the display reports `Runnable` every frame, even on a fresh renderer.
- **Suspected cause:** exactness refinement stays pending during contact but never runs.
- **Impact:** GPU and battery waste while the pen rests.
- **Next step:** confirm with `outstanding_terms()` in a resting-pen fixture.

### 6. Pen-down on a layer mask may still re-decode after idle
The idle keep set (capycanvas8's `94f946c44`) covers paint targets only. A pen-down on a mask target gets no warm 3×3 and can re-decode one tile. The A set (tiles under a 1:1 view) can also sit above the floor with many layers; admission still reclaims it.

### 7. A tight-memory Navigator stall (capycanvas7's liveness work)
A Navigator-class refusal's need never enters the display demand (`refused_bytes` counts only required classes). With no room, the Navigator waits forever (`Blocked(Residency)`), and settle never completes.

**Agreed rule:** count the refused need in demand when the device has room. Otherwise treat the refusal as final and idle the Navigator, blank or showing its last picture, until a release wakes it.

The RED is the display-oracle case `g_pen_half_tight_budget` (G-Pen at 50%, 256 MB budget). Re-add it to the oracle once fixed.

### 8. Fast-stroke landing slivers on Flow brushes
After `57cf0707b`, Flow brushes land at full width, and normal-speed strokes lose no ink. Fast strokes (3×) still leave slivers of up to about 4 px² for a frame, a radius or more behind the tip. The full fix eases the landing in from the previous tip's position and width.

### 9. Oracle coverage gaps
- **Memory pressure:** waits for item 7.
- **Geometry:** fractional zoom and rotation.
- **Approximation contract:** the 0.2 tolerance at dab edges in reduced cells hides edge-only errors. Each reduced stage needs a stated contract.
- **Brush shader correctness:** the reference shares the brush shaders, so wrong brush values everywhere would pass. Add small asymmetric-image tests that compare each partial operation with a full evaluation.
- **Bigger documents:** the stand-in mutation is caught only through the held-decode pan schedule.

### 10. Structural refactors
These come from the audit, ranked by value against cost. They make the obligations hold by construction rather than per site.
1. **Every partial write or skip carries a baseline.** Done for reduced-effect change maps (`36678b7f1`). Apply the same rule to contact crops.
2. **Publication is a submission-bound transaction.** Coverage, claims, damage and completion become encoder-local and are promoted on submit, then rolled back on discard. Pieces are done: `7873485bd`, `d3d83e153`, `af15b8400`, `f38171388`. Remove the remaining drop-as-success flags and early revision bumps.
3. **One dependency declaration per graph result.** Derive keys, damage, contact witnesses and repair edges from it, replacing the separate shown, written and current checks.
4. **Choose the final picture against what the host last accepted.** A stand-in must dominate that on overlapping support. `878556e86` covers the overview case.
5. **Derive frame demand from visible debt.** Each provisional has an owner, age and wake; the idle checks read the same set.

**Sufficiency check:** build a small transition model with independently specified bad outcomes. Search for a trace that satisfies V, D, C and L but is still bad, then remove one obligation at a time and require a counterexample.

### 11. Test suite health and environment
- **The red suite:** about 150 renderer tests fail on main, many from other work, for example `5e196eb79`, `64740e6f5` and `3cd6519ec`. Every change then needs a control build and a diff, which roughly doubles test time. Triage, then fix or delete.
- **Event-order harness:** add deterministic hooks to `test_support`, to gate one decode, hold one pipeline, refuse one cell's memory or discard a frame. Reproductions then take minutes.
- **Measured and rejected:** `opt-level = 2` for the vendored wgpu crates in test builds sped the oracle by only about 6%. Rust 1.100 already links with its bundled `rust-lld`.
- **Not installed:** `cargo-nextest` (per-test timeouts) and `sccache` need network or package installs.

### 12. Low-confidence audit hypotheses, not reproduced
- reversion during a crop-only redraw or a camera-word change;
- glass-blur damage edges;
- held transitions at a half-fade switch;
- object mip wakes;
- refinement starvation under continuous dirtying.

Each has a deciding experiment in the audit notes. Treat them as leads, not bugs.

### 13. Pre-existing failures found by the release gate
- **Undo-integrity tests are test drift, not product bugs.** These four have failed since they were introduced in `5e196eb79`:
  - `undo_then_overlapping_stroke_waits_for_selected_nonempty_restore`
  - `redo_then_undo_and_overlapping_stroke_use_only_selected_history`
  - `undo_then_zoom_and_overlapping_stroke_use_only_selected_history`
  - `ui_held_restore::a_real_ui_contact_waiting_eight_seconds_after_undo_keeps_every_sample_and_raster_byte`

  They assert that a selected restore stays pending while its display read is gated. Since `5e196eb79`, Undo publishes the selected backing and materializes it when a stroke needs those cells. After release, every case matched the replay's committed tile bytes and pixels at 25% and 100%. Rewrite the tests against the current mechanism.
- **`layer-host`, failing on the control too:**
  - `header::tests::native_capture_uses_shared_preview_and_never_persists_motion_or_cancellation`: the geometry reply now carries a 7-entry `roles` array (control and title_bar per item) where the test expects `[]`;
  - `tests::a_windows_host_whose_stroke_waits_on_memory_frames_only_when_its_renderer_asks`: panics with "the stroke waits on memory".
- **GTK startup capture under default color management** reads `[54,54,54]` where `[51,51,51]` is expected, on both builds and in both themes. SDR capture passes.
- **GTK Enclose Fill menu assertion:** a flake on both builds.
- **GTK cold-store journeys need recovery enabled.** With it disabled, they stop at "The latest drawing changes are still being prepared".

## Dropped after testing
- **A skipped cell counted as a finished transition:** a refused cell is pending, shows its last pixels, and is repaired.
- **No wake when the stand-in compile finishes:** no blocked hole showing a stand-in with nothing else pending could be built.
- **A forced re-render used up by a failed attempt:** the analysis guide identity in the display key repairs it.
- **A no-op encode reported as drawn:** every host checks pipeline readiness before encoding, and zoom is clamped.
