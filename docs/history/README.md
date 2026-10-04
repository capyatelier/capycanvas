# Design history

[Technical documentation](../README.md)

These records hold research and design decisions that explain why the current
design is the way it is, or that inform planned work. Their claims apply to when
they were written; check the code and the current guides before relying on them.
Progress notes, validation reports and other work records are not committed
([writing](../development/writing.md)).

## Research

- [Capy file format: compatibility, containers and future artwork types](capy-format-foundation.md).
- [One authored graph for layers and nodes](authored-graph-research.md): artist workflows,
  stack semantics, compositor feasibility and proposed validation gates.
- [Photo editing: user journeys, gap audit and build list](photo-editing-research.md),
  with its [source reports](photo-editing-research).
- [Vector drawing and editing](vector-drawing-research.md).
- [Vector layers: tool subset, stroke storage and new-artist journey](vector-layers-research.md),
  with its [source reports](vector-layers-research).
- [Layers: illustration workflows and panel design](layers-research.md).
- [Color palettes](color-palettes-research.md).

## Design records

- [Layer clipping and effect attachment](layer-clipping-and-effects.md): common
  bases, owner-local effects, compact connections and shared drop planning.
- [Photo editing M2–M4](photo-editing-m2-m4.md): the decisions, design notes
  and steps of the quick wins, foundations and retouching milestones.
- [Pencil, charcoal and ink brush redesign](dry-media-brush-design.md).
- [GTK and Wayland canvas subsurface](wayland-subsurface-feasibility.md).
- [Vulkan rendering with native Windows presentation](windows-vulkan-presentation.md),
  a deferred candidate.
