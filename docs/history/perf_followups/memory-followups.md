# Memory follow-ups after the total-memory work

[Design history](../README.md)

This record lists the memory work that was designed or written but not shipped
before the release cut, with the evidence, the agreed design and what is left to
verify. Its claims apply to when it was written; check the code first.

Code for most items exists as local commits in the shared clone, named below by
branch and hash. They are not on `origin`; rebase them onto current `main` before
use. Test names are the regression tests each change was written against.

## Already on main

Context for the items below. Each landed alone with its regression test.

| Change | Commit |
| --- | --- |
| Decoded-sample charge leaves on trim; zero-page stroke starts; GTK heap trim; 32 MiB read cache | `85b13139d` and its three parents |
| Refusals expire against the view and motion of the next pass | `9c8ee1c91` |
| A modified page's spare counts as evictable | `7dbb8d3be` |
| Import coverage settles on an owner's answer (K1) | `10059ca0a` |
| Coverage cancellation wakes the wait; watch wakes after unlock | `980d17233` |
| Pen-down tiles kept and warmed after idle | `94f946c44` |
| JPEG import gets one allowance and a need-sized reservation | `e41113a3e` |
| Drawing allocations charged once across Authored, History, captures and parked tabs (M7) | `ba02aac00` |

The ledger contract all follow-ups must keep: one charge per unique live
allocation; serialized ledger operations; pins, floors, grants and accepted
reclaims kept apart from physical bytes; wakes delivered after locks are released;
a wait only on a producer that always settles; one bounded reclaim, then grant or
a visible refusal.

## Measurement notes

- **RSS on dual-GPU laptops.** Main renders on the GPU that drives the display
  (`55f1dde16`). On a Lenovo ThinkPad P43s that is the Intel UHD 620, whose GPU
  memory is shared system RAM and appears in process RSS; 1.0.15 used the Quadro
  P520's dedicated memory. Compare RSS plus dedicated VRAM: G-Pen +5% and undo
  −33% against 1.0.15, not the 2–3.6× that RSS alone suggests.
- **Allocator reserve.** gpu-allocator keeps partly used blocks. Reserved minus live
  Vulkan bytes can be 50–450 MiB and depends on the mix of allocation sizes.
- **Device runs need a matched parent** and at least two trials per arm; single runs
  of first-dab latency vary by ±15 ms on the Huion Kamvas Pad 12.

## Items

| Item | Kind | Code | Blocked on |
| --- | --- | --- | --- |
| [M5: floors kept out of pins](#m5-floors-kept-out-of-pins) | Accounting | `b6-a` `96f617a34` | Stroke-lane rewrite |
| [L4: imports wait for retiring bytes](#l4-imports-wait-for-retiring-bytes) | Liveness | `b6-a` `4e9783b8f` (partial) | Retirement receipts |
| [M8: Web arena and GPU limits](#m8-web-arena-and-gpu-limits) | Accounting | `b6-a` `50c71b5fb` (partial) | A sizing decision |
| [K4: capture minimum at pen-down](#k4-capture-minimum-at-pen-down) | Overcommit | none | Stroke-lane rewrite |
| [W3: half-float exact pages](#w3-half-float-exact-pages) | Memory saving | `b6-stack` `c4f9d329a` | Device rows |
| [W8: native settled paint](#w8-native-settled-paint) | Memory saving | `b6-stack` `af21b941e` | Device rows |
| [W11: release finer pages after a zoom](#w11-release-finer-pages-after-a-zoom) | Memory saving | `b6-stack` `731040532` | Device rows |
| [W4: parked drawing's first-stroke pages](#w4-parked-drawings-first-stroke-pages) | Memory saving | `b6-stack` `e1970cf7d` | Tests |
| [W9: decoded samples sized to the view](#w9-decoded-samples-sized-to-the-view) | Memory saving | partial pick only | A repro first |
| [Q1: Refinement protection window](#q1-refinement-protection-window) | Cache policy | `b6-stack` `6df404d6a` | Tests |
| [Allocator host blocks](#allocator-host-blocks) | Memory saving | `b6-stack` `4f2b5cd57` | Device rows |
| [PNG need-sized import](#png-need-sized-import) | Import limits | `b6-stack` `275161936` | Tests |
| [Smaller accounting gaps](#smaller-accounting-gaps) | Accounting | none | — |

### M5: floors kept out of pins

**Problem.** Renderer owners report policy floors (display working floor, raster
`WORKING_BYTES`, staging floor, the decoded-sample floor) as pins, and the ledger
clamps physical up to pins. On unified memory a fresh renderer books about 136 MiB
it has not allocated (63 MiB Scene for 0.5 MiB actually held), and three renderers
pin 429 MiB against a 402 MiB limit.

**Design.** Pins are only bytes held. Floors move to demand; the physical-up-to-pins
clamps go; import reservations leave RAM pinned; debt becomes physical above the
limit. A floor that cannot be met after one bounded reclaim is refused, never
clamped and never pinned past the limit. The owner accepted that a refused display
floor leaves the view coarse.

**Why it is held.** `96f617a34` fails four tests that pass on main, all through the
stroke lane's lending, which assumes floors are pinned:
`a_lane_that_no_release_can_wake_asks_for_its_own_frame_until_room_returns`,
`a_stroke_lane_that_nothing_can_free_refuses_its_edit_once_and_ends_the_wait`,
`a_color_cache_grant_keeps_unpinned_pages_beside_pinned_stroke_pages` and
`a_tablet_budget_below_the_photo_keeps_publishing_strokes_and_their_ink`. Land it
with the lane rewrite that replaces lending (`lent`, `lend`, `draw`,
`yield_to_lane`). Expected test updates under the coarse-view decision include
`a_drained_display_pin_covers_one_full_stack_unit_of_decoded_inputs`.

An earlier attempt, `5c587c9ba`, pinned the display tier floor instead. On the
Wacom MovinkPad Pro 14 with RAM over its limit the display still sat below its
floor, and one Galaxy Tab S7 run rebooted the device; do not revive it.

### L4: imports wait for retiring bytes

**Problem.** K1's coverage drops a reclaim request once the owner's published use
is within its grant, and refuses an import when no request is open. Bytes still
retiring sit in the transient Other row, which no one asks, so an import can be
refused moments before retirement would cover it.

**Design.** Keep an accepted request open until the owner answers or drops
(`4e9783b8f` removes the `occupied>granted` retain condition and the fixed-sum
refusal). Settle on the GPU retirement receipt once retiring charges are carried
through retirement. The release fix that lands first makes parked renderers and
`StoragePressure` decline or answer, so open requests always end.

**Tests.** `an_owner_that_moves_its_accepted_bytes_to_another_row_keeps_the_import_waiting_for_its_answer`;
a new test where an import waits for an accepted retirement and opens when it
settles.

### M8: Web arena and GPU limits

**Problem.** Each Web instance samples linear memory once and uses the wasm32
address ceiling as the process limit, and registers its GPU as unified with no
heap. GPU bytes therefore consume the RAM allowance that protects wasm allocations,
even on a discrete adapter, and the 384 MiB floor can exceed the remaining arena.
The page and each raster worker have separate ledgers.

**Done in `50c71b5fb`.** A per-instance arena bounds only RAM (headroom, coverage,
working RAM, the RAM resident cap) with no floor; the device keeps a shared-system
share; the arena is re-sampled at copy admission and receipt. Test:
`memory_budget::arena_tests::a_wasm_arena_bounds_only_ram_and_takes_no_floor`.

**Left.** The page should reserve a claim for each worker job and the worker seed
its budget from it. The claim size needs a decision per job kind (read, write,
snapshot, colour conversion, output): a claim equal to the page's headroom trims
the page's display cache during every snapshot; a small claim makes the worker
refuse large opens.

### K4: capture minimum at pen-down

**Problem.** `admit_capture_window` overcommits its minimum when refused, so pen-up
can push memory past the limit.

**Design (owner).** Never delay the first ink. Reserve the committed edit's minimum
capture window at pen-down in the lane admission without blocking the first dab; if
the reclaim has not settled by pen-up, keep today's bounded overcommit rather than
delay or drop the stroke. A basic pen on a small canvas is never refused or paused.
A refusal after dabs were admitted commits the stroke at its last admitted dab.

**Size.** About +45/−15 production in `memory.rs`, `native_edit.rs`, `staging.rs`
and `lib.rs`, plus about 30 if fills, transforms and filters reserve at their own
preparation step.

### W3: half-float exact pages

**What it does.** Exact display pages of half-float drawings, including pivot
halves, stay in half float, so every display page has one allocation size.
Captures that need a half writer before it compiles go through the compose kernel
that startup already compiles, so the first frames and the first dab never wait.

**Evidence.** Earlier versions saved 90–140 MiB PSS with a photo open on the Huion
Kamvas Pad 12, but cost 68–91 MiB of reserved Vulkan with the photo parked, from
mixed 512 KiB and 1 MiB pages spreading over allocator blocks. On the TCL TAB 11
Gen 2 the half writers delayed exact Fit by 204 ms and the immediate dab by 119 ms.
The owner accepted the half pivot precision (0.11 mean code error on an opaque
backdrop, translucent rows within 0.25).

**Verify.** `startup::tests::a_half_float_drawing_draws_and_captures_before_its_half_capture_writers_compile`,
`liquify::field_tests::fit_persistent_preview_preserves_untouched_prior_compression_in_same_tile`,
`delivered_pixel_tests`. Device rows: TCL cold-open first present, exact Fit and
immediate dab; Huion PSS and reserved Vulkan after a stroke and with the photo
parked.

### W8: native settled paint

**What it does.** Settled paint pages demote to native GPU codes (75% smaller at
U8, 50% at U16). Pages in the pen-down set are never demoted and are promoted in
the idle warm, so the dab's frame promotes nothing. A view that reads a demoted
page asks for its promotion at the next frame instead of decoding the backing on
the CPU.

**Evidence.** The first version added 16–29 ms to a first dab on earlier paint on
the TCL and 1.1 s to a wide-stroke pinch to sharp on the Huion. Demotion must
forget material bindings and the per-pass evictable-paint figure.

**Verify.** The eleven `tests::cold_paint` tests named in `af21b941e`; TCL first dab
on earlier paint; Huion pinch to sharp after a wide stroke.

### W11: release finer pages after a zoom

**What it does.** Once a settled view's cells are exact, finer display pages that
the current zoom cannot reach are released. The fix in `731040532` diverts cells to
the plan-level halves compose only for an edited pivot; the first version built
halves at every open and stroke.

**Verify.** The four contact and two `memory_tests` halves tests, and the five
refinement release tests. Device row: live GPU 12 s after a stroke on the 12 MP
photo at Fit, then a pinch to 100% and time to sharp. On the Huion the first version
never released, because the display never reached full exactness; recheck after
the refusal-expiry fix on main.

### W4: parked drawing's first-stroke pages

**What it does.** A parked drawing keeps at most 16 MiB of first-stroke pages. Its
window test sampled `display_bytes()` before the renderer settled, so the baseline
varied between 57 and 138 MiB; the test now drives frames until
`next_frame() != Work && settled()` and samples `warm_bytes()`.

**Verify.** `window::tests::a_stroked_photo_parks_its_window_and_16_mib_of_stroke_pages_and_presents_on_return`
several times; `warm::tests`.

### W9: decoded samples sized to the view

**Problem.** DecodedSamples demands a fixed 512 MiB and trims only to its grant. An
earlier version sized it to its consumers and trimmed when settled, but on the TCL
the return from 100% to Fit never reached exact within 60 s, pans re-read the photo
119–535 times, and the dab after a pan was 36 ms slower.

**Design.** Keep the view's tiles plus a one-tile pan ring, nearest first within the
sample grant (on a 32-layer 4248 × 2832 drawing at 100% that set is about 700 MiB, so
it needs the byte bound), warmed once per view like the pen-down warm; do not keep
decodes used only to restore a page after upload.

**First step.** Reproduce the return-to-Fit stall without a window under a tight
budget. Reading the code points at the preparing-limit eviction in `retain()`
combined with idle release between refinement frames, not the settled trim, which
cannot run while the view is approximate.

### Q1: Refinement protection window

`6df404d6a` deletes the `Refinement && latest.required()` clause from
`Demand::outranks`, which protected a once-visible page from Refinement forever.
The recency clause already protects anything a higher class stamped this epoch or
the last. Release keeps whole-array accounting, which is the honest figure because
an array frees only when empty. Verify against the three seeded-eviction tests from
`587c783f2`, which guard against thrash on a tight display budget.

### Allocator host blocks

`4f2b5cd57` keeps 16 MiB device blocks and uses 4 MiB host-visible blocks through a
vendored wgpu host-range field. 4 MiB blocks everywhere saved 40 MiB with a photo
parked on the Huion but tripled `vkAllocateMemory` calls on the TCL (1,028 to
2,568), added 52 ms to the dab after a pan and 34 ms to the worst pinch frame gap.
The Huion saving came from host blocks. A rule based on heap size would choose small
blocks on the TCL, where they hurt. Verify on the TCL and the Huion.

### PNG need-sized import

`275161936` gives PNG the JPEG treatment: the need from IHDR and a pre-image-data
chunk scan (expanded palette, tRNS and low depths, Adam7 frames), cumulative stage
checks, and a reservation sized to the need. Other formats still check per buffer
against fixed shares (a third for decode, an eighth for the source).

### Smaller accounting gaps

- **Display trim.** `memory.rs` trims against `grant + retiring`, but retired store
  pages have already left `display.used()` for the transient Other owner, so the
  trim holds back by that amount until the poll thread destroys them.
- **Decoded-sample demand** is last-writer-wins between renderers sharing the cache.
- **Artwork captures** build a fresh inventory each time, so their scalar metadata is
  charged per capture; fixing it needs per-record metadata identities.
- **Tab exchange** is two ledger transactions, unpark then park; a trim that finishes
  between them can publish a parked row early (an over-count only).
- **Pending tile sizes** are estimates (`MAX_COMPRESSED_TILE_BYTES`).
- **Source arrays** are charged per slot while a 16 MiB array frees only when empty.
- **Clone on the Galaxy Tab S7** raised live Vulkan by about 683 MB, most of it
  RasterPool and Scene rather than retouch pages; the RasterPool part did not
  reproduce without a window.

### Liquify on the Galaxy Note 10

On `80168fc83`, lmkd killed the app on the Galaxy Note 10 (SM-N970U1) during
Liquify on a 12 MP photo in a cold pass: 164 MB RSS plus 209 MB swap, with OS
available memory down to 291 MB against a minimum of 864 MB on 1.0.15. An earlier
build ran the same step without a kill. It has not been rerun on the phone since
the Liquify originals admission, the import changes and the retouch charge landed.
Rerun the cold pass on the phone; if it still dies, trace app PSS and GPU during
Liquify and bound the step to the device.
