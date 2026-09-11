# Non-Destructive Filter Layers + WGSL Shader Subsystem

> Historical design or validation record. Statements about completion and remaining
> work describe the recorded checkpoint. Start with the [current technical guides](../README.md).

Research background supplied for the filter project. The shipped contract and
its current scope are documented in [Filters and programmable effects](adjustments-implementation.md);
the broader graph/editor proposals below are not all implemented.

## Purpose

This document summarizes the design direction for a drawing application's non-destructive filter system and a WGSL-based custom shader subsystem.

The intended architecture should support:

- Built-in non-destructive filter / adjustment layers.
- Built-in GPU effects implemented using the same underlying shader infrastructure.
- User-authored custom WGSL effects.
- Custom effects that operate either as:
  - **filters** over existing image content, or
  - **procedural generators** that produce image content from scratch.
- A future in-app custom shader editor.
- Safe persistence of custom WGSL inside drawing documents.
- A future render graph capable of multi-pass and compute-based effects without inventing a new shader language.

The application already uses **Rust + `wgpu` + WGSL**.

The current security priority is specifically **remote code execution (RCE)** from malicious drawing files or embedded shaders.

**Denial of service, GPU hangs, pathological shader runtime, and resource exhaustion are explicitly not a current security priority.**

---

# 1. Product Model

The top-level concept should be an **Effect Layer**, not merely a "Shader Layer."

An Effect Layer has:

- inputs,
- parameters,
- an implementation,
- dependency / invalidation metadata,
- an output image,
- normal layer properties such as opacity, blend mode, visibility, and masks.

The implementation may be:

- built-in WGSL,
- user-authored WGSL,
- native Rust,
- a future ML/inference implementation,
- another internal backend.

Conceptually:

```text
EffectLayer
│
├── Inputs
├── Parameters
├── Dependency information
├── Implementation
│   ├── WGSL
│   ├── Native
│   └── Future ML / other backend
└── Output
```

This keeps the document/layer model independent of the implementation technology.

---

# 2. Non-Destructive Layer Types

The application should support non-destructive equivalents of common adjustment/filter workflows.

Typical adjustment-style effects include:

- Brightness / Contrast
- Levels
- Curves / Tone Curve
- Hue / Saturation / Luminosity
- Color Balance
- Gradient Map
- Posterization
- Invert
- Threshold / Binarization
- LUT / color transforms

Traditional image-processing filters can include:

- Blur
- Sharpen
- Convolution
- Emboss
- Edge detection
- Morphology
- Noise
- Pixelation
- Dithering
- Halftoning
- Chromatic aberration
- Distortion
- Displacement
- Stylization
- Texture / grain effects

A filter layer should behave like a live effect:

- parameters remain editable,
- it can be reordered,
- visibility can be toggled,
- opacity/blend mode remain editable,
- masks remain editable,
- removing the layer restores the unchanged source beneath it.

This is analogous to the useful parts of Photoshop Smart Filters and Krita Filter Layers / Filter Masks.

---

# 3. Two Shader Effect Categories

The WGSL subsystem should support at least two conceptual effect types.

## 3.1 Filter Effect

Reads the composited image below the layer and produces a transformed image.

```text
layers below
    ↓
source texture
    ↓
WGSL effect
    ↓
effect output
```

Examples:

- blur,
- sharpen,
- color grading,
- halftone,
- edge detection,
- chromatic aberration,
- displacement.

---

## 3.2 Generator Effect

Produces image content without requiring a source image.

```text
canvas coordinates + parameters
             ↓
          WGSL
             ↓
       generated pixels
```

Examples:

- procedural gradients,
- noise,
- checkerboards,
- paper textures,
- grids,
- screentones,
- mathematical patterns,
- procedural backgrounds.

The same shader package/runtime should support both.

---

# 4. Do Not Invent a Shader Language

User shaders should be **standard WGSL**.

Do not create a custom shader syntax, DSL, or WGSL dialect.

Avoid constructs such as:

```text
@param strength slider(0, 1)
@canvas_texture source
```

Instead use:

```text
effect.wgsl
manifest.json
```

or an equivalent packaged format.

WGSL remains ordinary WGSL that can be parsed, validated, syntax-highlighted, and edited with existing tooling.

The application defines only a **host ABI / contract**.

---

# 5. WGSL Host Contract

The host defines stable resource bindings and semantics.

A simple filter might receive:

```text
@group(0)
binding 0 = source texture
binding 1 = source sampler
binding 2 = canvas/effect context

@group(1)
binding 0 = effect parameter uniform buffer

@group(2)
bindings = optional user-supplied textures/resources

@group(3)
reserved for future versions
```

The exact layout is implementation-defined, but it should be:

- stable,
- documented,
- versioned,
- independent from internal renderer implementation details.

Example context:

```wgsl
struct EffectContext {
    output_size: vec2<u32>,
    source_size: vec2<u32>,
    // Future coordinate transforms can be added in later ABI versions.
};
```

A filter shader can remain completely normal WGSL:

```wgsl
struct Params {
    strength: f32,
};

@group(0) @binding(0)
var source: texture_2d<f32>;

@group(0) @binding(1)
var source_sampler: sampler;

@group(1) @binding(0)
var<uniform> params: Params;

@fragment
fn fs_main(
    @builtin(position) position: vec4<f32>
) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(source));
    let uv = position.xy / size;
    let c = textureSample(source, source_sampler, uv);
    return vec4<f32>(c.rgb * params.strength, c.a);
}
```

---

# 6. Manifest / Metadata

WGSL alone is insufficient to describe UI and execution semantics.

A separate manifest should describe things such as:

- effect API version,
- effect type (`filter` or `generator`),
- entry point(s),
- parameters,
- parameter UI metadata,
- auxiliary texture inputs,
- dependency radius,
- pass graph,
- intermediate texture sizes,
- required capabilities.

Example:

```json
{
  "apiVersion": 1,
  "type": "filter",
  "entryPoint": "fs_main",
  "parameters": {
    "strength": {
      "type": "float",
      "default": 1.0,
      "min": 0.0,
      "max": 2.0
    },
    "radius": {
      "type": "float",
      "default": 5.0,
      "min": 0.0,
      "max": 100.0,
      "unit": "px"
    }
  },
  "dependency": {
    "type": "radius",
    "pixels": 100
  }
}
```

The manifest is **not a shader language**. It is host metadata and execution configuration.

---

# 7. Parameter Model

WGSL can determine parameter names/types, but not good UI.

For example:

```wgsl
struct Params {
    radius: f32,
    amount: f32,
    enabled: u32,
};
```

This does not communicate:

```text
radius:
  widget = slider
  min = 0
  max = 100
  unit = px
```

Recommended design:

- WGSL defines actual shader types.
- Manifest optionally defines:
  - labels,
  - ranges,
  - defaults,
  - units,
  - slider/checkbox/color/dropdown widgets,
  - enum choices,
  - asset pickers.

If the manifest is absent or incomplete, generic controls can be inferred.

Examples:

```text
f32          → numeric field
bool/u32     → checkbox when declared as boolean metadata
vec3/vec4    → vector control
vec4<f32>    → optional color control when tagged as color
```

---

# 8. Color and Alpha Contract

Define this early and explicitly.

Shader authors must know whether pixels are:

- sRGB encoded or linear,
- premultiplied or straight alpha,
- SDR or HDR,
- in document color space or a normalized working space.

A strong default is:

```text
linear floating-point RGBA
premultiplied alpha
```

with document/profile conversion handled around the effect subsystem.

This avoids effects behaving differently depending on document profile and prevents common blur/compositing errors.

---

# 9. Coordinate Spaces

The shader API should define coordinate semantics clearly.

Potential spaces:

- texture UV,
- source pixel coordinates,
- output pixel coordinates,
- layer coordinates,
- canvas/document coordinates,
- future transformed coordinates,
- optional physical/DPI coordinates.

Questions that must have defined answers:

- Does a 10 px checker pattern transform with the layer?
- Does blur radius use source pixels or canvas pixels?
- What happens if the layer is scaled?
- How do effects behave under rotation/skew/perspective?

Eventually the context may expose transformation matrices.

---

# 10. Sampling and Boundary Behavior

Filters frequently sample neighboring pixels.

Standardize behavior for sampling outside the source bounds:

- transparent,
- clamp,
- repeat,
- mirror.

Where possible this can map to WebGPU sampler behavior.

For arbitrary pixel-offset sampling, shader authors need predictable source dimensions and coordinate conventions.

---

# 11. Tile / Dirty-Region Dependencies

This is especially important in a drawing app.

For a simple color effect:

```text
changed source region
→ same output region becomes dirty
```

For a blur with radius 100:

```text
changed source region
→ changed region expanded by 100 px becomes dirty
```

Therefore effects should declare their dependency footprint.

Examples:

```json
{
  "dependency": {
    "type": "radius",
    "pixels": 100
  }
}
```

or:

```json
{
  "dependency": {
    "type": "fullFrame"
  }
}
```

Possible dependency modes:

```text
local
radius(N pixels)
fullFrame
future/custom/internal
```

This metadata drives tile invalidation and caching.

Do not try to infer arbitrary texture sampling radius from WGSL automatically.

---

# 12. Multi-Pass Effects

A single fragment shader is enough for many effects, but not all.

Example: efficient Gaussian blur.

```text
source
  ↓
horizontal blur
  ↓
temporary texture
  ↓
vertical blur
  ↓
output
```

Bloom may require:

```text
source
  ↓
threshold
  ↓
downsample
  ↓
blur
  ↓
upsample
  ↓
composite
```

Therefore the long-term shader subsystem should support a **declarative pass graph**.

Example:

```json
{
  "passes": [
    {
      "kind": "render",
      "entryPoint": "blur_horizontal",
      "input": "source",
      "output": "tmp"
    },
    {
      "kind": "render",
      "entryPoint": "blur_vertical",
      "input": "tmp",
      "output": "result"
    }
  ]
}
```

The executable language remains WGSL.

---

# 13. Variable Resolution Intermediate Textures

Some effects should run parts of their pipeline at reduced resolution.

Examples:

- bloom,
- large-radius blur,
- frequency separation,
- image pyramids,
- coarse-to-fine algorithms.

The pass graph should eventually support:

```text
full resolution
1/2 resolution
1/4 resolution
fixed dimensions
```

The host owns and validates these allocations.

User WGSL should not arbitrarily allocate GPU resources.

---

# 14. Compute Shaders

Fragment-only effects cover a large initial feature set.

A future version may expose standard WGSL compute shaders for:

- histograms,
- reductions,
- prefix sums,
- large kernels,
- workgroup-shared algorithms,
- morphology,
- simulations,
- certain denoisers,
- iterative processing.

Conceptually:

```text
Effect
  ├── render pass
  ├── compute pass
  ├── render pass
  └── output
```

Compute should probably be **out of scope for v1 custom shaders**, even if built-in effects use it internally.

A fragment-only public API reduces implementation surface while still enabling many useful effects.

---

# 15. Global Statistics Effects

Some effects need information about the entire image.

Examples:

- auto levels,
- auto exposure,
- histogram equalization,
- average luminance,
- min/max detection,
- global palette analysis.

These cannot be implemented as a single independent pixel transformation.

Example pipeline:

```text
source
  ↓
compute histogram
  ↓
derive parameters
  ↓
apply transform
```

This is another reason to treat the long-term architecture as an **effect graph**, not merely a fragment shader.

---

# 16. Iterative Effects

Some algorithms repeatedly execute a pass:

```text
A → B → C → ... → convergence
```

Examples include certain:

- diffusion algorithms,
- distance transforms,
- morphological algorithms,
- simulations.

The effect graph may eventually support a host-controlled fixed repeat count.

Avoid letting user shaders dynamically construct an arbitrary render graph.

Keep execution structure declarative and host-controlled.

---

# 17. Stateful / Time-Dependent Effects

Persistent state is not needed for traditional Smart Filter-like behavior.

Potential future uses include:

- wet paint simulation,
- reaction-diffusion,
- animation,
- temporal effects.

Distinguish:

```text
Pure effect:
input → output
```

from:

```text
Stateful effect:
input + previous state → output + next state
```

Prefer pure effects initially because they are easier to:

- cache,
- serialize,
- undo,
- parallelize,
- export reproducibly.

Time/animation should be a later addition.

---

# 18. Auxiliary Inputs

Effects should be able to receive explicit auxiliary resources.

Examples:

```text
source + displacement texture → distortion
source + grain texture        → film effect
source + LUT                  → color grading
```

Potential resource categories:

- sampled 2D textures,
- samplers,
- uniform buffers,
- later storage buffers,
- later storage textures.

Avoid arbitrary access to unrelated document layers or GPU resources.

Every resource should be explicitly provided by the host.

---

# 19. Built-In Filters Should Use the Same Foundation

Where practical, built-in effects should run through the same Effect Layer / effect-graph abstraction.

Examples:

```text
Curves        → built-in WGSL effect
Levels        → built-in WGSL effect
Blur          → built-in multi-pass WGSL effect
Halftone      → built-in WGSL effect
Custom effect → user WGSL effect
```

Benefits:

- one caching system,
- one parameter system,
- one mask/blending system,
- one document representation,
- one preview pipeline,
- less duplicated renderer logic,
- easier migration from built-in effect to editable shader example.

Not every future effect must be WGSL. For example:

```text
AI Denoise      → ML runtime
Content-aware   → native/ML implementation
Camera pipeline → specialized native implementation
```

These should still fit the same Effect Layer abstraction.

---

# 20. Custom Shader Editor

A future editor can expose the exact same shader package used by documents.

Recommended editor capabilities:

- WGSL syntax highlighting,
- compiler diagnostics,
- live preview,
- parameter editing,
- manifest editing,
- source/reference texture selection,
- generator/filter mode,
- reset/disable effect,
- possibly example templates.

Suggested templates:

```text
New Shader
  → Filter
  → Generator
  → Color Transform
  → Neighbor-Sampling Filter
  → Procedural Pattern
```

The editor should not require users to understand internal renderer bindings beyond the documented ABI.

---

# 21. Sharing / Shader Presets

Custom shader effects should be shareable independently of documents.

Example package:

```text
my-halftone-effect/
├── shader.wgsl
├── manifest.json
└── optional-assets/
```

or a single application-specific package containing the same data.

This lets most users consume effects as presets:

```text
Add Effect Layer
  → Manga Halftone
  → RGB Split
  → Paper Grain
  → VHS
  → Custom...
```

Only advanced users need to edit WGSL.

---

# 22. Embedding Shader Source in Drawing Files

It is reasonable—and desirable—to embed WGSL inside the drawing document.

Example:

```text
document
├── layer tree
├── effect manifests
├── WGSL source
├── effect parameters
├── optional shader assets
├── flattened preview
└── thumbnail
```

Benefits:

- portable files,
- no "missing plugin" problem,
- reproducible effects,
- effects survive sharing,
- exact source used by the artwork is preserved.

Store source, not trusted compiled GPU binaries.

Persist:

```text
WGSL source
manifest
API version
parameter values
asset references/assets
```

Do not treat embedded SPIR-V, DXIL, Metal bytecode, or vendor binaries as authoritative user content.

Compile WGSL locally through the normal `wgpu` path.

---

# 23. Security Scope: RCE Only

The current security concern is:

> Can a malicious drawing document containing attacker-controlled WGSL cause arbitrary native code execution on the user's machine?

The following are explicitly **out of scope for the current threat model**:

- GPU hangs,
- infinite/expensive loops,
- rendering stalls,
- resource exhaustion,
- large allocations,
- excessive shader compilation time,
- device loss,
- denial of service generally.

Those may matter later, but they should not drive the initial custom-shader architecture.

The important distinction is:

> WGSL should be treated as untrusted active content, but it is not equivalent to embedding arbitrary native code or a general scripting language.

---

# 24. RCE Risk Characterization

For the proposed stack:

```text
malicious drawing
      │
      │ embedded WGSL
      ▼
document / manifest parser
      │
      ▼
Naga parser + validator
      │
      ▼
wgpu
      │
      ▼
MSL / HLSL / SPIR-V or backend-native representation
      │
      ▼
OS/vendor shader compiler
      │
      ▼
GPU driver
      │
      ▼
GPU
```

The RCE risk is best characterized as:

> **Low-probability but real. Significantly safer than native plugins, embedded JavaScript/Lua with host access, arbitrary SPIR-V/DXIL, or vendor-native shader binaries. Not equivalent to a security sandbox on native desktop by itself.**

The major residual RCE surfaces are:

1. application-owned parsing / unsafe code,
2. Naga / `wgpu`,
3. native platform shader compilers and graphics drivers.

---

# 25. Why Standard WGSL + `wgpu` Is a Good Baseline

`wgpu` and WebGPU are designed around the assumption that shader content may be untrusted.

For user-controlled shader source, the intended path is:

```text
WGSL
  ↓
Naga parses
  ↓
Naga validates
  ↓
wgpu creates validated pipeline
  ↓
native backend
```

This is materially safer than passing attacker-controlled native shader bytecode directly to a graphics driver.

The strongest rule is:

> **Documents may provide WGSL source only. They may not provide trusted or passthrough GPU bytecode.**

---

# 26. `wgpu` Rule: Use Only the Normal Checked Shader Path

For document-provided WGSL, use only the normal safe shader-module creation path.

Conceptually:

```rust
device.create_shader_module(...)
```

Do not route untrusted document shaders through any trusted, unchecked, or passthrough path.

Avoid APIs whose purpose is to:

- bypass Naga,
- skip normal validation,
- trust caller-provided invariants,
- feed native shader forms directly to the backend.

Architecturally, custom shader handling should be encapsulated so this is hard to violate accidentally.

Example:

```text
CustomShaderRuntime
      │
      └── only exposes validated WGSL module creation
```

Do not rely only on code-review convention.

---

# 27. Do Not Accept Native Shader Bytecode From Documents

Do not allow drawing files or shader packages to supply:

- SPIR-V binaries,
- DXIL,
- DXBC,
- Metal libraries / binary IR,
- vendor-native shader binaries,
- serialized driver pipeline caches.

Instead store:

```text
WGSL source
manifest
parameter values
assets
API version
```

Then compile locally through the normal `wgpu`/Naga path.

This preserves the strongest validation layer between malicious document content and the native driver/compiler stack.

---

# 28. Application-Owned `unsafe` Is a More Important RCE Concern Than Shader Complexity

Shader complexity is primarily a DoS issue.

For RCE, focus review effort on the path:

```text
document bytes
   ↓
document parser
   ↓
manifest parser
   ↓
WGSL string
   ↓
wgpu safe API
```

The ideal property is:

> There is little or no application-owned `unsafe` reachable from attacker-controlled shader/document metadata.

Be especially cautious around:

- raw GPU handles,
- `wgpu-hal`,
- Vulkan/Metal/D3D interop,
- external-memory handles,
- manual buffer pointer manipulation,
- native FFI,
- custom shader compiler integrations,
- shared-memory/image interop.

Keep those systems away from arbitrary document-controlled parameters whenever possible.

---

# 29. Keep the Public Host ABI Narrow

A narrow host ABI reduces the amount of the graphics stack an attacker can exercise.

A practical public v1:

```text
WGSL fragment shader
+
source sampled texture
+
sampler
+
uniform parameters
+
canvas/effect context
+
optional bounded auxiliary sampled textures
+
host-created render target
```

The document does **not** control:

- arbitrary bind group layouts,
- arbitrary GPU feature enabling,
- arbitrary native resources,
- arbitrary command-buffer construction,
- arbitrary device creation,
- arbitrary pipeline caches,
- native shader bytecode.

This is useful for both architecture and RCE containment.

---

# 30. Fragment-Only Public v1 Is a Sensible RCE Reduction

A fragment-only public API still enables a large set of effects:

- Curves
- Levels
- color transforms
- LUTs
- posterization
- halftones
- dithering
- convolution
- blur
- sharpen
- edge detection
- distortion
- displacement
- chromatic aberration
- procedural textures
- pixelation
- stylization

It also gives custom shaders a simpler execution envelope:

```text
host-created source
      ↓
user fragment shader
      ↓
host-created target
```

Compute, storage resources, arbitrary work graphs, and advanced GPU features can be added later if concrete use cases justify the larger attack surface.

This is not about DoS; it is about minimizing native driver/compiler/API surface reachable from untrusted documents.

---

# 31. Resource Limits Are Not a Current RCE Requirement

Because denial of service is currently out of scope, limits such as these are not security-critical for the present RCE threat model:

- maximum loop count,
- shader instruction complexity,
- sampling count,
- render time,
- pass runtime,
- GPU watchdog handling,
- strict memory quotas.

Reasonable parser/document sanity limits may still be useful engineering hygiene, but they should not be mistaken for primary RCE mitigations.

The RCE-focused controls are instead:

```text
standard WGSL only
validated wgpu path
no native shader bytecode
narrow host ABI
minimal unsafe
OS sandboxing where practical
```

---

# 32. Desktop vs Web / Mobile

Web, iOS, and Android generally provide strong process/application sandboxing around the application already.

Native desktop builds deserve more attention because a successful process-level RCE may otherwise inherit broader user privileges.

The important question is not:

```text
Can WGSL directly call the filesystem?
```

It cannot.

The important question is:

```text
If a shader compiler / driver vulnerability gives native code execution,
what privileges does the compromised process have?
```

---

# 33. macOS RCE Hardening

For macOS, **App Sandbox** is high-value.

Desired outcome:

```text
shader/compiler exploit
        ↓
RCE in drawing app process
        ↓
still constrained by App Sandbox
```

The sandbox can reduce access to:

- arbitrary filesystem locations,
- user data outside granted scopes,
- network depending on entitlements,
- unrelated system capabilities.

If practical in the future, an XPC/helper process with fewer entitlements gives a stronger boundary:

```text
Main app
   │
   │ IPC
   ▼
Shader helper
   - GPU access
   - no general filesystem
   - no network
   - minimal entitlements
```

But this is hardening, not necessarily a v1 requirement.

---

# 34. Windows RCE Hardening

A normal Win32 process should not be assumed to be sandboxed.

If the app architecture/distribution permits it, investigate:

- AppContainer,
- least-privilege application isolation,
- restricted capabilities.

Desired result:

```text
shader/compiler exploit
        ↓
RCE in app/helper
        ↓
restricted token/capability environment
```

This is especially valuable if custom shader documents are expected to be downloaded and opened frequently from untrusted sources.

A separate shader helper under a stronger sandbox is better still, but not required to justify the feature.

---

# 35. Linux / GTK RCE Hardening

GTK does not provide a security sandbox.

Potential whole-app or worker isolation options include:

- Flatpak,
- namespaces,
- bubblewrap,
- seccomp,
- Landlock where applicable.

For an RCE-only threat model, the main goal is to reduce what a compromised process can access:

```text
no arbitrary filesystem
no secrets
no unnecessary network
no unrelated process access
```

The graphics stack still needs to be reachable for GPU execution.

---

# 36. Separate Shader Worker Process

A dedicated worker provides the strongest userspace RCE containment:

```text
Main app
   │
   │ IPC
   ▼
Shader worker
   │
   ├── Naga
   ├── wgpu
   └── native graphics stack
```

If an exploit gains native code execution in the shader path:

```text
RCE
 ↓
worker process only
 ↓
OS sandbox boundary
```

This is materially stronger than compiling/running untrusted WGSL in the main process.

However, it introduces a significant practical issue:

> Efficient cross-process GPU texture sharing is not uniformly exposed through a simple portable high-level `wgpu` API.

Possible approaches:

- CPU/shared-memory image transfer,
- backend-specific external texture sharing,
- keep GPU execution in-process.

Because of this complexity, a helper process should be viewed as **optional hardening**, not necessarily a prerequisite for shipping custom WGSL.

---

# 37. Recommended RCE Posture for v1

For a conventional desktop drawing app, a defensible first release is:

```text
✓ embed standard WGSL in documents
✓ allow custom filter shaders
✓ allow procedural generator shaders
✓ use normal validated wgpu shader creation
✓ rely on Naga validation
✓ never accept native shader bytecode from documents
✓ never use trusted/passthrough shader paths for document content
✓ keep custom shader host ABI narrow
✓ minimize application-owned unsafe in the shader/document path
✓ keep wgpu/Naga current
✓ use whole-app OS sandboxing where practical
✓ consider a dedicated shader helper later as additional hardening
```

This is a substantially lower-risk model than arbitrary native plugins.

---

# 38. Do Custom Shaders Need an Explicit "Enable" Warning?

Not necessarily.

If the application has:

- standard WGSL only,
- normal `wgpu` validation,
- a narrow host ABI,
- no native bytecode,
- no dangerous application-owned unsafe path,
- normal application sandboxing where practical,

then automatically compiling embedded WGSL can be a reasonable product choice.

An explicit "this document contains shaders; enable?" warning is more appropriate if:

- the desktop app is broadly unsandboxed,
- custom shader execution is unusually privileged,
- native shader passthrough exists,
- documents can invoke arbitrary plugins/native code,
- the application automatically processes arbitrary files from hostile sources.

For the proposed restricted WGSL model, user friction from a macro-style warning may not be justified.

---

# 39. Do Not Execute Embedded Shaders in Thumbnail / Indexing Helpers

Even though DoS is out of scope, this remains valuable from an RCE perspective.

Do not compile embedded WGSL merely because the OS requests:

- a thumbnail,
- metadata,
- Quick Look preview,
- file indexing,
- folder previews.

Instead embed:

```text
thumbnail
flattened preview
```

in the drawing file.

The lightweight file-preview path should parse only enough data to retrieve safe pre-rendered imagery.

This avoids exposing thumbnail/indexing helper processes to attacker-controlled shader compilers unnecessarily.

---

# 40. Security Trust Boundary Summary

The intended trust model is:

```text
UNTRUSTED
drawing file
WGSL source
manifest values
auxiliary image assets

TRUSTED / HOST CONTROLLED
document parser implementation
WGSL ABI
wgpu device
bind groups
textures/resources
render target
command encoding
feature selection
native application code
```

The user shader is allowed to compute pixels.

It is not allowed to control the native runtime.

---

# 41. Document Versioning

Every embedded effect should declare an API version.

Example:

```json
{
  "apiVersion": 1
}
```

Do not rely on your internal renderer implementation staying stable forever.

The ABI should be treated as a public compatibility contract.

Possible evolution:

```text
Effect API v1
  fragment filters
  generators
  uniforms
  sampled textures

Effect API v2
  multi-pass
  temporary targets

Effect API v3
  compute
  storage resources

Effect API v4
  state/time/animation
```

Old documents remain tied to their declared semantics.

---

# 42. Recommended Initial Architecture

A practical v1:

```text
EffectLayer
│
├── type
│   ├── filter
│   └── generator
│
├── implementation
│   ├── built-in effect ID
│   └── embedded WGSL
│
├── parameters
├── manifest
├── masks / layer properties
└── output
```

Public custom WGSL v1:

```text
standard WGSL fragment shader
+
fixed app-defined bindings
+
manifest
+
one render target
+
source texture for filters
+
uniform parameters
+
optional sampled textures
```

Host responsibilities:

```text
document parsing
parameter UI
texture allocation
render target allocation
binding creation
color management
tile invalidation
WGSL validation
error handling
security policy
```

---

# 43. Suggested Staged Rollout

## Phase 1 — Internal Effect Foundation

Implement:

- Effect Layer abstraction,
- source texture input,
- generator mode,
- parameter schema,
- masking/blending/opacity,
- tile dependency metadata,
- fixed color/alpha semantics,
- built-in effects using WGSL where practical.

Ship built-in effects first.

---

## Phase 2 — Embedded Custom WGSL

Expose:

- standard WGSL fragment shaders,
- fixed host ABI,
- manifest,
- shader compilation diagnostics,
- document embedding,
- shader disable/failure state,
- flattened preview/thumbnail.

Security requirements:

- normal checked `wgpu` shader path,
- WGSL only,
- no native shader bytecode,
- narrow host ABI,
- no attacker-controlled application `unsafe`,
- whole-app sandboxing where practical.

---

## Phase 3 — Shader Editor + Presets

Add:

- syntax highlighting,
- live preview,
- diagnostics,
- parameter manifest editing,
- filter/generator templates,
- import/export of shader packages,
- shareable effect presets.

---

## Phase 4 — Multi-Pass Effect Graph

Add host-declared:

- multiple passes,
- intermediate textures,
- scaled intermediate resolution.

Enable efficient blur, bloom, pyramids, etc.

---

## Phase 5 — Compute / Advanced GPU Effects

Only after real use cases justify it:

- compute WGSL,
- storage resources,
- histogram/reduction effects,
- more sophisticated effect graphs.

Treat this as a larger public GPU capability surface and review the RCE implications again before exposing it to untrusted documents.

---

## Phase 6 — Optional Stateful / Temporal Effects

Potential future support:

- time,
- persistent textures,
- animation,
- simulations.

Treat separately from pure non-destructive filters because state complicates:

- caching,
- undo,
- export,
- determinism,
- serialization.

---

# 44. Key Architectural Principles

## Principle 1: WGSL is the executable language

Do not invent a shader language.

---

## Principle 2: The host contract is versioned

Bindings, coordinate systems, color semantics, and execution behavior are public API.

---

## Principle 3: Effect Layer is the fundamental abstraction

WGSL is one implementation technology.

---

## Principle 4: Built-ins and custom shaders share infrastructure

Avoid separate filter and custom-shader architectures.

---

## Principle 5: Effects are pure whenever possible

Prefer:

```text
inputs + parameters → output image
```

This enables deterministic rendering, caching, undo, and export.

---

## Principle 6: The host controls resources

Shaders do not own the native rendering runtime or arbitrary document resources.

---

## Principle 7: Embedded WGSL is untrusted active content

But it is lower risk than arbitrary native plugins because it is parsed and validated through the WebGPU/`wgpu` security model.

---

## Principle 8: The GPU is not itself an RCE sandbox

The residual native attack surface is:

- Naga,
- `wgpu`,
- backend translators,
- platform shader compilers,
- graphics drivers.

Desktop OS sandboxing contains the consequences of an exploit.

---

## Principle 9: Start with fragment-only custom effects

This covers a large amount of artistic functionality while keeping the public GPU surface narrow.

---

## Principle 10: DoS is currently out of scope

Do not overcomplicate v1 around loop limits, watchdogs, shader complexity scoring, or other availability protections unless product needs change.

---

# 45. RCE-Focused Implementation Checklist

Before exposing custom shaders publicly, confirm:

- [ ] Effect Layer abstraction is independent of WGSL.
- [ ] Filter and generator modes share the same runtime.
- [ ] WGSL is standard WGSL; no custom shader DSL.
- [ ] ABI/bind groups are documented and versioned.
- [ ] Color space and alpha semantics are fixed.
- [ ] Coordinate-space semantics are fixed.
- [ ] Parameter schema is separate from WGSL syntax.
- [ ] User shaders receive only host-provided resources.
- [ ] Drawing files provide WGSL source only.
- [ ] Drawing files cannot provide SPIR-V/DXIL/native shader binaries.
- [ ] User shaders use only the normal validated `wgpu` shader-module path.
- [ ] No trusted/passthrough shader API is reachable from document content.
- [ ] Application-owned `unsafe` is minimized in document/shader code paths.
- [ ] Raw GPU/native backend interop is isolated from attacker-controlled metadata.
- [ ] Shader errors do not make document loading fatal.
- [ ] Embedded shaders do not execute inside thumbnail/indexing helpers.
- [ ] Documents store source + manifest + API version.
- [ ] Public v1 is preferably fragment-only.
- [ ] Compute/storage capabilities are treated as a later API/security review.
- [ ] `wgpu` and Naga are kept current.
- [ ] macOS App Sandbox is used where practical.
- [ ] Windows application isolation is considered where practical.
- [ ] Linux distribution sandboxing such as Flatpak is considered where practical.
- [ ] Dedicated shader worker remains an optional future hardening path.

Out of scope for this checklist:

- [ ] GPU DoS prevention.
- [ ] shader runtime budgets.
- [ ] loop complexity analysis.
- [ ] resource exhaustion protection beyond normal engineering sanity checks.

---

# 46. Condensed Target Architecture

```text
                         Drawing Document
                               │
          ┌────────────────────┼────────────────────┐
          │                    │                    │
       Paint Layers        Effect Layers         Preview
                               │
                     ┌─────────┴─────────┐
                     │                   │
                  Built-in            Custom
                     │                   │
                Effect ID          Embedded WGSL
                     │                   │
                     └─────────┬─────────┘
                               │
                         Effect Runtime
                               │
          ┌────────────────────┼─────────────────────┐
          │                    │                     │
      Parameters          Dependency Info       Pass Graph
          │                    │                     │
          └────────────────────┼─────────────────────┘
                               │
                      Host-Controlled ABI
                               │
                     Standard WGSL only
                               │
                     Normal wgpu validation
                               │
                              Naga
                               │
                  Metal / D3D12 / Vulkan
                               │
                              GPU
```

On native desktop, optionally wrap the application or shader worker in the strongest practical OS sandbox.

---

# 47. Primary Design Decision

The central architecture can be summarized as:

> Build a general non-destructive **Effect Layer** system whose GPU implementation is a versioned, host-controlled **standard WGSL runtime**. Use it for built-in filters today, embedded custom filter/generator shaders next, and a future shader editor and render graph later.

For security:

> Treat embedded WGSL as untrusted active content, but rely on the intended `wgpu`/Naga/WebGPU validation model rather than inventing a custom shader language. For RCE, the essential protections are **WGSL-only input, normal checked `wgpu` APIs, a narrow host ABI, minimal application-owned `unsafe`, current dependencies, and OS sandboxing on desktop where practical**.

This provides a strong capability-to-risk tradeoff and should not block custom shaders from being a first-class document feature.
