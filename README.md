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
are not copying anything in particular, but instead we want something better.

And this is worst on on Linux, where there are not many choices for
illustration software. Though, this app also happens to work on Windows, Mac,
iPad and Android, and web, so nobody is left behind.

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
were able to deliver the responsive painting and powerful paintbrush engine that
modern apps like Fresco and Rebelle.

Capycanvas's *Paint* workspace brings simulated physical media to digital illustration.


<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/paint-workspace-dark.webp">
  <img src="docs/assets/paint-workspace-light.webp" width="1920" height="1080" alt="An oil painting of a house by the sea at sunset in Paint, with brushes, colors and layers beside the canvas.">
</picture>

## Photo

Mobile devices are becoming more capable every day. It seems like OLED screens
are everywhere, and my 2-year-old phone saves its images in wide-gamut HDR
format by default. SRGB is a thing of the past.

To this date, the only painting app that properly supports wide gamut HDR is
Krita. But I feel the experience is still a bit lacking. And when you export
HDR images, you need control over the gain mapping, so that it still looks good
on SDR devices. And of course you also need the basics (proofing, effect
chains, the whole enchilada)

Capycanvas's *Photo* space will enable stunning visuals for a new generation of wide-gamut devices.

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/photo-workspace-dark.webp">
  <img src="docs/assets/photo-workspace-light.webp" width="1920" height="1080" alt="A terrarium photograph in Photo, with the Tonal range selection tool and Curves and Vibrance adjustment layers.">
</picture>

## Tools

Here’s what’s available across the editor. The linked guides cover the controls
and their limits.

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

We wanted to build the fastest brush and rendering engine we could. The goal is
120 frames per second and beyond, with brushes thousands of pixels wide. That
pushed us toward the way game engines work: build around the GPU, and do the
pixel work in shaders.

A large [wet brush](docs/engineering.md#brush-engine) has a lot to do. It has to
pick up color, carry paint and mix it into the next part of the stroke. We keep
that work on the GPU, along with
the layers it paints into. Pencils and ink take a different path: the shader
sweeps the tip along the stroke, with paper grain fixed to the canvas.

Then there’s the rest of the drawing. Hundreds of layers can hold a lot of
empty space, so we allocate paint storage in small tiles where marks exist.
[Edits update the affected regions](docs/engineering.md#incremental-composition).
Compatible adjustments [run together in one shader](docs/engineering.md#live-effects),
avoiding an intermediate image between every effect.

The engine is written in Rust and shared by every platform, including the
browser. Each [native app](docs/engineering.md#native-apps-and-shared-core) uses
its platform’s own controls. We wanted the same brushes everywhere, and an
interface that belongs on the device you’re using.

We’re still working toward the performance goals. The
[measurements](docs/PERFORMANCE_TARGETS.md#where-we-stand) show what meets them
and what doesn’t. Painting requires a hardware GPU.

The [engineering whitepaper](docs/engineering.md) follows the full design,
including [color and original images](docs/engineering.md#color-and-original-images)
and [writing new effects](docs/engineering.md#extending-the-engine).

## Contributing

We need artists to use Capy Canvas and tell us where it gets in their way. We
also need help with brushes, shaders and the interface. If you want to work on
any of that, we’d like to hear from you.

For a first shader project, the [Tent Blur example](examples/filters/tent-blur)
is a small effect defined in JSON and WGSL. Brush-engine work starts in the
[brush guide](docs/internals/brushes.md). Bring feedback and ideas to
[GitHub issues](https://github.com/capyatelier/capycanvas/issues).

## Try it or build it

Capy Canvas is in development. Linux and web are the most complete versions;
we’re bringing the other ports up to the same level. The
[download page](https://capycanvas.art/download/) lists available builds, and
the [browser editor](https://editor.capycanvas.art/) is there to try now.

To build from source, follow the guide for
[Linux](docs/development/linux.md), [Web](docs/development/web.md),
[Android](docs/development/android.md), [macOS and iPadOS](docs/development/apple.md)
or [Windows](docs/development/windows.md). The
[developer guide](docs/development/README.md) covers setup and checks.

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
