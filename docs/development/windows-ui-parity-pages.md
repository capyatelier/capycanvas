# Windows UI parity: remaining page helper adoption

[Windows development](windows.md)

The pages and chrome work adopts the helper contract in `692b310d5`.
Closed findings are removed. The remaining findings below need the shared
helper extensions and page adoption in M5.

## Prompt

Finish these findings in M5 after M4 is pushed. Read `AGENTS.md`,
`apps/layer-windows/AGENTS.md` and the helper commit messages and diffs first.
Use the assigned `capycanvas1` worktree and VM. M5 owns `ChoiceFeedback.cpp/.h`,
`NumberControl.cpp` and `PickerDialog.h`; keep `UiControls.h`, `ButtonFeedback.h`
and `NativeMenus.h` untouched. Add the entry-centering, compact-switch and dialog
layout helpers in ChoiceFeedback or PickerDialog. Extend existing APIs through
defaulted parameters or new ControlSurface values so other callers keep their
behavior. Remove each closed finding and delete this file when none remain.
Record dropped CHECK findings and new helper names/signatures in the commit
message, which must include "Windows UI parity".

`paletteResources()` is already merged once into Application resources.
Never merge it per page or per control. Use `opticalText(element, size)` and
`buttonCaption(control, text)` for retained captions. ContentDialog response
caption bindings must stay intact. Invalid numeric borders use
`CapyUi::InvalidFieldColor`; problem text uses the shared palette error role.

## Remaining findings

### PREF-7: Inline rename alignment and `alignNames` cleanup

Shared labels and button captions use A's optical correction. `fieldColors`
sets TextBox palette, font and border resources, but provides no corresponding
entry baseline correction. Keep the existing `WorkspaceManagerView::alignNames`
path until a shared entry helper replaces its baseline calculation; removing
it now would discard the rename/read alignment without a replacement.

### PREF-8: Compact switches

Application resources supply the palette colors. The contract has no compact
switch builder or resources that remove ToggleSwitch's reserved content column.
Preferences and shortcut per-tool switches still need a shared compact control
that puts the track at the row's suffix edge.

### PREF-16: Choice-row feedback and activation

Closed preference and keymap choices use transparent suffixes and symmetric
shared padding. `choiceColors` always adds its own hover and pressed fills,
with no row-suffix mode. Wrapping the choice in a feedback button stacks those
fills and disabled-content opacity. Keep native choice input until A supplies
one feedback owner for an activatable row, including its disabled content.

### HEAD-8: Header disabled-content opacity

Header surfaces stay opaque and editing/groups with variants stay enabled.
`9f34de048` adds a defaulted `disabledContentOpacity` parameter to
`buttonFeedback` and `buttonColors`. Pass `0.45` for header tools, including Zen
Capy, in M5. Do not stack an extra glyph opacity on the shared content opacity.

### HEAD-26: Shared document-dialog layout

Body captions use the shared label helper and response buttons inherit A's
34-DIP height, 16-DIP horizontal padding and Medium weight. The shared dialog
helper still lacks GTK's 20-DIP body padding and natural-width responses aligned
right. Keep this finding open until A supplies that dialog layout.

### HEAD-27: Shared canvas-bar and notice disabled policy

Per-site whole-button opacity is removed, preserving opaque tile surfaces and
one shared disabled-content dim. Adopt the `9f34de048` per-control parameter in
M5 with `0.45` for canvas-bar buttons and notice actions.

### HEAD-DLG-1: Composite numeric field fills

Plain drawing entries, Size choices and Export rows adopt their surface-aware
helpers. New drawing width/height and resolution still use the old outer `input` fill:
`fieldColors` has no transparent composite-entry surface, so changing the outer
fill would leave an opaque inner numeric box or stack two translucent fills.
The composite field must have one Drawing fill (`text` at 18/255) behind its
prefix, entry and unit. Size's complete numeric controls retain their shared
palette; NumberControl exposes no Dialog surface option, and page code must
not replace its internal entry resources after construction.

### HEAD-DLG-2: Transparent entries inside composite fields

A's field helper writes all TextBox background-state resources and has no
transparent variant. The detached numeric entry needs a shared transparent
surface for rest, hover, focus and disabled states so only the outer field paints.

### HEAD-DLG-9: Plain drawing-entry vertical insets

Detached numeric entries preserve NumberControl's computed 34-DIP optical insets;
only their horizontal insets are set by the page. Name, preset search and save-name
TextBoxes still need a shared vertical-centering helper to replace their literal
7-DIP top inset. `fieldColors` does not own padding or entry baseline geometry.

### HEAD-DLG-10: Bold drawing-choice captions

Drawing choices adopt the shared 34-DIP height, symmetric padding and optical
caption correction. `choiceColors` and `comboOptionText` explicitly set Normal
weight; the item TextBlock overrides the control's inherited weight. The helper
contract needs a drawing-caption weight option before GTK's Bold weight can
replace it without a local item or template fork.

### HEAD-DLG-33: Export switch rows

Export uses existing preference cards, shared numeric controls and labels, and
surface-aware ComboBoxes. Its existing boolean controls use `compactCheck`.
GTK's Enlarge and Remove location switch rows still require the missing shared
compact-switch builder from PREF-8.

## Validation

Use only `tools/windows-vm/windows-vm.py --vm capycanvas1`. Keep heavy output on
`/mnt/localssd`; keep the VM warm during a milestone and stop it when finished.
Run `check`, strict Clippy over SSH, and one full fixture round with the following
families and existing variants. After repairs, rerun the failed cases and every
fixture that exercises a file changed after that round:

- `header-settings settings-storage shortcut-page shortcuts manager manager-focus`
- `switcher switcher-drag header header-placement canvas-bar notice zoom-readout window`
- `documents size-dialogs localization:default`
- `hdr:screen-parity hdr:screen-parity-light` for the changed screen popup

M5 also runs `popup`, `button-feedback`, `sdk-feedback`, `header-settings`,
`documents` and `effects:default` in dark and light because panels share these
helpers. Use one batched build and fixture round.

Capture affected pages in light and dark with glass off and low. Include
expanded shortcut filters, profiles, header customization, tabs, canvas segments,
notices, zoom/screen menus, New drawing, Size and Export. Move keyboard focus off
the selected header editor item before capturing HEAD-9's 1-DIP 0.45 outline.
Retain raw and 4x ink crops for Scale's 26-DIP segment and a Preferences caption.
The existing settings-file recovery journey does not expose workspace-storage
Retry/Save as new/Backup; report that capture gap unless an existing private
workspace-recovery journey reaches it without a new production hook.

Run the full localization matrix only when text changes. Compare runtime failures
with the five exact baseline signatures in control `20261029-051821` and existing
matched `692b310d5` evidence. Run one targeted control list for any remaining
unexplained signatures before calling them pre-existing.
Never waive a regression or push on compilation alone. Preserve original fixture
assertions and deadlines, remove control worktrees by exact path, fetch/rebase,
and push the tested milestone to main without force.
