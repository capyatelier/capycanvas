# Project validation checkpoint

[History](README.md) · [Current project format](../reference/project-format.md)

This records the initial GTK and shared codec validation. Statements about other
ports describe that checkpoint; current coverage is in the
[platform guide](../platforms/README.md).

- Core round trips cover all 40 filters and advanced brush/mask histories.
- Rejection tests cover corrupt/truncated streams, budgets, missing/duplicate or
  wrongly typed assets, invalid selections, IDs, history order and group depth.
- Engine tests compare live, active and committed replay with feedback on/off
  and coalesced input, including idle frames.
- `cargo test -p layer-render-wgpu --test project` compares incremental live
  drawing against save/reopen on fresh GPU renderers. Imported transparency,
  wet brushes, applied/live masks, groups, gradients, figures, transforms,
  clipped multipass filters, curves/gradient lookup and animation match exactly
  on the Vulkan test workstation, including subsequent wet painting.
- Optional `CAPY_PROJECT_CAPTURES` writes generated test PNGs for inspection;
  use a directory under ignored `artifacts/`, never commit those captures.

The native `workspace::tests::native_document_files` test exercises GTK document
dialogs, cancellation, malformed files, Save As, PNG export, fresh-window GPU
reopening and close-after-save. It uses the toolkit's file-chooser fallback on
an isolated Wayland display; desktop portal-provider interaction is not automated.
Shared tests cover failed/overlapping writes, undo checkpoints and edits during
save.

The GPU project workload also passes on Metal. The Apple bridge suite separately
checks both iPad and Mac session configurations: import, textured painting,
applied mask, transform, project round trip, exact fresh-GPU pixels, then new
painting and undo. Source access shares immutable storage. The fixture uses
real editor actions, with no OS-menu automation.

Apple file dialogs, save cancellation, unsaved-work behavior and physical
iPad project UI/replay are not covered by those codec tests. Browser and Android
project workflows also remain. The independent historical filter-reference
discrepancy remains open as documented in [runtime-filters.md](../reference/runtime-filters.md).
