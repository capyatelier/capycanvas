# Float32 HDR layers: parity scope

Status: scoped follow-on work; not implemented by the SDR appearance update.

The product reason is parity with image editors that support 32-bit floating
point images. Users should be able to keep Float32 image data through ordinary
layered editing and interchange, without having to justify a specialist workflow.
GIMP exposes floating-point precision separately from its internal Float32
processing ([encoding documentation](https://docs.gimp.org/3.0/en/gimp-image-encoding.html)).

## User contract

- Add **32-bit float HDR** alongside 16-bit float HDR in New Drawing and Document
  Properties. The chosen document precision governs editable raster layers;
  independently tagged placed originals retain their source precision. Arbitrary
  per-layer precision switches are not needed for the initial parity release.
- Use the same tools, layers, masks, RGB spaces, HDR viewing and SDR appearance.
  Retain the existing 203 cd/m² reference-white convention. Float32 is a storage
  precision choice, not a new color space or a different tone mapper.
- Preserve committed Float32 data in save/reopen, undo/redo, recovery and device
  restoration. Promotion from Float16 is exact but cannot recover lost detail.
  Demotion is explicit, reports unsupported ranges and never occurs silently.
- Document finite-value, signed RGB, alpha and overflow behavior. Decide and test
  mask precision explicitly; the current HDR path uses integer16 mask storage.
- Expose the mode only on qualified hosts/devices. Unsupported devices explain
  the limitation without narrowing the document to Float16.

## Reuse and required work

The current shared renderer already processes native artwork in RGBA32Float
working tiles. Reuse that renderer, bounded tile caches, asynchronous publication,
history and recovery architecture. No separate Float32 application or renderer.

1. Extend `SampleDepth`, `PixelDescriptor`, source interpretations and host/FFI
   contracts. Float samples are currently assumed to have 16 bits.
2. Add exact Float32 backing/serialization, upload, readback and publication.
   Remove half quantization only for this mode; retain the Float16 contract.
   Audit masks, source caches, transforms, brush accumulation and effect stores.
3. Make tool ranges and validation depend on the document contract. Color entry,
   histogram ranges, exposure/curves and GPU validation currently contain half
   limits. Define overflow failure before publishing any affected tiles; Float32
   storage does not make every arithmetic operation safe at every finite input.
4. Provide a bounded RGB/RGBA Float32 OpenEXR import/export route with explicit
   primaries, white interpretation and alpha association. Start with ordinary
   flat images and lossless compression, not deep/multipart EXR. The format
   supports HALF and FLOAT channels ([OpenEXR introduction](https://openexr.com/en/latest/TechnicalIntroduction.html)).
   Native `.capy` remains the layered format. Keep PQ PNG and deliberate SDR
   delivery; neither is an exact Float32 interchange format.
5. Audit capability requirements, cancellation, memory accounting, worker queues
   and device loss. Existing Float32 filtering/blending requirements must remain
   explicit; implement any alternate GPU path against the same precision contract.

## Qualification and delivery

One uncompressed 60 MP RGBA plane grows from 480 MB to 960 MB. This is a payload
calculation, not a measured process budget. Existing working planes are already
Float32, so total application memory does not automatically double. Measure cold
backing, staging, history, concurrent export/save and steady/peak CPU/GPU memory.

Require exact committed samples across native save, reopen, history and recovery;
operation-specific tolerances against Float64 references for edited values;
subnormals, signed values, low alpha, long chains and failed/cancelled operations;
and real HDR/SDR delivery round trips. Qualify representative 24/45/60 MP layered
documents, sustained painting and p95/p99 input latency on supported native and
browser devices. Missing hardware evidence remains a release limitation.

Deliver as three reviewable milestones: storage/history contract; complete
editing and Float32 interchange; host/device qualification and runnable builds.
This is separate from phase 4's existing Float16 acceptance. RAW development,
layered PSD, native CMYK, OCIO/ACES and deep EXR remain outside this scope.
