# Design history

[Technical documentation](../README.md)

These records hold research and design decisions that explain why the current
design is the way it is, or that inform planned work. Their claims apply to when
they were written; check the code and the current guides before relying on them.
Progress notes, validation reports and other work records are not committed
([writing](../development/writing.md)).

## Research

- [Physical paint runtime cost](physical-paint-runtime-cost.md): calculations and
  break-even conditions for local brush updates and continuously flowing paint.
- [Capy file format: compatibility, containers and future artwork types](capy-format-foundation.md).
- [One authored graph for layers and nodes](authored-graph-research.md): artist workflows,
  stack semantics, compositor feasibility and proposed validation gates.
- [Photo editing: user journeys, gap audit and build list](photo-editing-research.md),
  with its [source reports](photo-editing-research).
- [PSD import and layer styles](psd-and-layer-styles-research.md): PhotoCraft reuse,
  compatibility gaps, style compositing and the proposed owner badge and Properties UI.
- [PSD import compatibility](psd-import-compatibility-research.md): what PSDs from
  Photoshop, Illustrator, Fresco and other apps contain, how about thirty importers
  degrade, where each PSD capability fits in Capy's model, the gaps ranked by
  impact and the decisions due before the format freeze, with its
  [source reports](psd-import-compatibility).
- [Vector drawing and editing](vector-drawing-research.md).
- [Vector layers: research, code audit and design](vector-layers-research.md):
  the object model, file format, rendering and tools for vector lines and fills,
  with its [source reports](vector-layers-research).
- [Object content extensions: format audits and later additions](object-content-extensions-research.md):
  whether the format grows into full SVG editing and the shapes, text and comic
  elements of other editors and painting apps, the reader fixes and model changes
  that made it so, and the slot each later feature uses, with its
  [source reports](object-content-extensions).
- [Vector layer performance](vector-performance-research.md): scale targets,
  lessons from Vello, Graphite, Inkscape, Chromium, WebRender and other editors,
  and the data structures, coverage kernel, caching, gesture and panel rules
  that keep large vector layers fast on the tier devices, with its
  [source reports](vector-performance-research).
- [Layers: illustration workflows and panel design](layers-research.md).
- [Professional illustration and manga gaps](illustration-manga-gap-audit.md):
  current capabilities and recommended behavior, with detailed daily-editing and
  enclosed-region coloring priorities.
- [Color palettes](color-palettes-research.md).

## Design records

- [Brush design records](brushes/README.md): accepted equations, authoring controls
  and decision history for brush families, beginning with analytic dry media.
- [Object layers: GA contract and long-term design](object-layer-ga-design.md):
  image objects, paint-grid constraints, scalable rendering, tools and format cleanup.
- [Layer clipping and effect attachment](layer-clipping-and-effects.md): common
  bases, owner-local effects, compact connections and shared drop planning.
- [Photo editing M2–M4](photo-editing-m2-m4.md): the decisions, design notes
  and steps of the quick wins, foundations and retouching milestones.
- [Pencil, charcoal and ink brush redesign](dry-media-brush-design.md).
- [GTK and Wayland canvas subsurface](wayland-subsurface-feasibility.md).
- [Large Vulkan allocations in bundled GTK](gtk-vulkan-allocation-reuse.md): why the
  runtime reuses large GSK allocations, the alternatives and upstream GTK's direction.
- [Vulkan rendering with native Windows presentation](windows-vulkan-presentation.md),
  a deferred candidate.
