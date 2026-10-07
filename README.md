<p align="center">
  <a href="https://capycanvas.art/">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="docs/assets/capy-dark.svg">
      <source media="(prefers-color-scheme: light)" srcset="docs/assets/capy-light.svg">
      <img src="docs/assets/capy-light.svg" width="96" height="96" alt="Capy Canvas">
    </picture>
  </a>
</p>

<h1 align="center">Capy Canvas</h1>

<p align="center">Linux · Windows · macOS · iPad · Android · Web</p>

<p align="center">
  <strong><a href="https://capycanvas.art/download/">Download</a></strong> ·
  <a href="https://editor.capycanvas.art/">Try in your browser</a> ·
  <a href="https://capycanvas.art/docs/">Artist guide</a> ·
  <a href="docs/engineering.md">Engineering</a>
</p>

Capy Canvas is free, GPU-accelerated painting and photo editing software. We
want to improve upon the ideas of the past, and build something better.

The gap in creative software is most stark Linux, where there are not many good
options for artists. Though, we think we can help people on Windows, Mac, iPad and
Android, and web, so nobody is left behind.

We want artists to build their own tools. We envision a world where painters
build their own brush engine, and photographers write their effect shaders.
And while software may be AI generated, the tools should be used by humans.
Artists shouldn't have to wait for a software company to decide that your idea is worth
building.

That is why Capy Canvas is open source and forever free, and we want
this to be a community of builders and creatives.

## Sketch

My first foray into the digital art world (like many others) was with Procreate
over a decade ago. Back when the apple pencil first came out, it was magic.
Even though the processor was slow by today's standards, the hardware and
software was so heavily optimized that it gave a real pen-to-paper feeling.

This was the first time a drawing app was ever fully optimized for the
hardware, using predictive pen tracking and GPU powered brush+rendering
engines. Then they dropped a clean and minimalist UI on top of it, which as
become ubiquitious across all drawing apps.

Capycanvas's *Sketch* workspace is a tribute to our roots. The place where
everyone starts out.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/sketch-workspace-dark.webp">
  <img src="docs/assets/sketch-workspace-light.webp" width="1920" height="1080" alt="An ink drawing of a train beneath a large tree in Sketch, with the drawing filling the screen and tools at the edges.">
</picture>

## Paint

Once you get into proper comic/manga work, the simple tools don't cut it anymore.
Lasso fill tools exist for a reason, and you need to be using masks left and right
to keep the shapes clean.

There are many tools out there that solve this problem, CSP and medibang are
often what people learn first. And they are great, easy-to-use and
inuitive software. All you have to do is follow the process and it usually
turns out OK.

While they are true workhorses, they are slow, built for an era where all
rendering and compositing was done on the CPU instead of the GPU. So they never
were able to match the powerful, realistic paintbrush engines in modern apps
like Fresco and Rebelle.

Capycanvas's *Paint* workspace brings simulated physical media to digital illustration.


<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/paint-workspace-dark.webp">
  <img src="docs/assets/paint-workspace-light.webp" width="1920" height="1080" alt="An oil painting of a house by the sea at sunset in Paint, with brushes, colors and layers beside the canvas.">
</picture>

## Photo

Mobile devices are becoming more capable every day. It seems like OLED screens
are everywhere, and my 2-year-old phone saves its images in P3 HDR
format by default. SRGB is a thing of the past.

To this date, the only painting app that properly supports wide gamut HDR is
Krita. But I feel the experience is still a bit lacking. And when you export
HDR and wide-gamut images online, you need control over the gain mapping, so
that it still looks good on SDR devices. And of course you also need the basics
(proofing, effect chains, the whole enchilada)

Capycanvas's *Photo* workspace enables stunning visuals for a new generation of wide-gamut screens.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/photo-workspace-dark.webp">
  <img src="docs/assets/photo-workspace-light.webp" width="1920" height="1080" alt="A terrarium photograph in Photo, with the Tonal range selection tool and Curves and Vibrance adjustment layers.">
</picture>

## Tools

| Area | Tools and operations |
| --- | --- |
| [Brushes](https://capycanvas.art/docs/drawing/brush-tools/) | Pencil, charcoal, ink, bristle, watercolor, oil and airbrush; pressure and tilt, paper grain, textured tips and color mixing. |
| [Layers](https://capycanvas.art/docs/layers/types/) | Groups, clipping layers, layer masks, blend modes, editable color and gradient fills, and selection layers. |
| [Selections](https://capycanvas.art/docs/selections/tools/) | Rectangle, ellipse, lasso, polygon, contiguous color and tonal range; Quick Mask, feathering and saved selections. |
| [Effects](https://capycanvas.art/docs/filters/how-filters-apply/) | Linked non-destructive effect chains, adjustment layers and effect masks; Curves, Levels, color grading, blur, sharpening, halftone and painterly effects. |
| [Retouching](https://capycanvas.art/docs/retouch/clone-heal/) | Clone Stamp, Healing and Spot Healing, dodge and burn, blending and liquify. |
| [Color](https://capycanvas.art/docs/color-management/color-spaces/) | sRGB, Display P3, Adobe RGB and ProPhoto RGB; ICC profiles, 8/16-bit SDR, 16/32-bit float HDR, print proofing and gamut warnings. |
| [Drawing](https://capycanvas.art/docs/drawing/fill/) and [transforms](https://capycanvas.art/docs/transform/move-transform/) | Reference-layer fills, Enclose and Fill, gradients, rulers, crop, resize, perspective and mesh warp. |
| [Workspace](https://capycanvas.art/docs/customize/workspaces/) | Docked or floating panels, tab groups, configurable toolbars and shortcuts, saved layouts and Zen mode. |

## Building the engine

We wanted to build the fastest brush and rendering engine we could, aiming for
120+ frames per second with brushes thousands of pixels wide. And we want that
response with physical paint, live effects and HDR too.

We took our cue from game engines. The [brush engine](docs/engineering.md#brush-engine)
and layer compositor are built around shaders, on top of `wgpu`. Wet paint stays
on the GPU as the brush picks it up and mixes it into the next part of the stroke.
The [effects](docs/engineering.md#live-effects) run there too, and compatible
adjustments can share a shader instead of each writing out another image.

Of course a fast brush is only part of it. There may be hundreds of layers to
combine after every stroke. We [update the parts that changed](docs/engineering.md#incremental-composition)
and keep the rest. And a layer with a few marks on it only needs paint storage
for those areas. This matters when the same engine has to fit on a tablet.

We also want a brush or effect someone builds on Linux to work on the other
platforms. The [shared engine is written in Rust](docs/engineering.md#native-apps-and-shared-core)
and compiles for all of them, including the browser. Each native app uses its
platform's own controls around that engine.

There is still performance work to do. The [measurements](docs/PERFORMANCE_TARGETS.md#where-we-stand)
show where we meet the targets and where we don't. The
[engineering whitepaper](docs/engineering.md) goes into the design, including
[color and HDR](docs/engineering.md#color-and-original-images).

## Contributing

The idea of artists building their own tools is something we want people to
actually try. For an effect shader, the [Tent Blur example](examples/filters/tent-blur)
is a place to start. It defines a filter and its controls in JSON and WGSL,
without adding a Rust kernel. The [extension guide](docs/engineering.md#extending-the-engine)
covers how to load it. Building a new brush engine involves the shared code;
the [brush guide](docs/internals/brushes.md#changing-a-brush) points to that.

We also need help with interface design and testing the app with actual drawings.
If a tool gets in the way of how you work, [tell us](https://github.com/capyatelier/capycanvas/issues)
what you were trying to do.

## Development

Capy Canvas is in development. Linux and web are the most complete versions;
we're bringing the other ports up to the same level. Painting requires a hardware
GPU.

The [download page](https://capycanvas.art/download/) lists available builds.
To build from source, choose a platform in the
[developer guide](docs/development/README.md#build-a-client). It also covers the
checks to run when making changes.

## Package layout

<details>
<summary>Shared Rust crates and platform clients</summary>

Shared code lives in `crates/`, native clients and the browser app in `apps/`.

| Package | Responsibility |
| --- | --- |
| [`layer-core`](crates/layer-core/README.md) | Artwork, undoable edits and `.capy` files. |
| [`layer-engine`](crates/layer-engine/README.md) | Pen input, brush dynamics and stroke generation. |
| [`layer-ui`](crates/layer-ui/README.md) | Shared tools, commands, editor state and layout. |
| [`layer-workspace`](crates/layer-workspace) | Saved workspace layouts. |
| [`layer-color`](crates/layer-color) | Color profiles and photo codecs. |
| [`layer-render`](crates/layer-render/README.md) | Engine-to-renderer contract. |
| [`layer-render-wgpu`](crates/layer-render-wgpu/README.md) | GPU painting, effects and composition. |
| [`layer-host`](crates/layer-host/README.md) | Shared native integration. |
| [`layer-bench`](crates/layer-bench/README.md) | GPU benchmarks and brush previews. |

</details>

## License and branding

Original code and non-brand assets are licensed under [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), your choice. Contributions to those parts are
submitted under both licenses unless agreed otherwise.

The Capy Canvas and Capy Atelier names and capybara artwork have separate
[branding terms](BRANDING.md). Modified versions need their own branding unless
you have permission. Dependencies retain their own licenses; see
[third-party notices](THIRD_PARTY_NOTICES.md).
