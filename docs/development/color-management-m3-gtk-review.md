# GTK print proofing: manual review

The GTK implementation is ready for app review. Automated qualification and its
measured limits are in the [acceptance record](../history/color-management-gtk-m3-validation.md).
Manual acceptance is **pending your confirmation**. Other platforms and HDR have
not been enabled by this work.

## Run the review build

From the repository root:

```sh
bash artifacts/color-m3/profile-picker/review/launch.sh
```

The launcher opens a separate app session with its own settings, workspace,
profile library and recovery directory. Existing app windows and their drawings
stay open. Review files and crash recovery remain under
`artifacts/color-m3/profile-picker/review/state/`; normal Save As writes to your chosen location.
`artifacts/color-m3/profile-picker/review/build.json` records the source and executable identity.
Saved profiles from the previous review session have been copied into this one.

## Review the print journey

1. Open a photograph or drawing. For a quick prepared example, open
   `artifacts/color-m3/gtk-journey/variant.capy`, which contains the tested CMYK
   recipe and a small edited drawing. Reopening starts in normal viewing.
2. Choose **View → Soft Proof Setup** (`Ctrl+Alt+Shift+P`). Open the single
   **Proof profile** picker. Select a saved profile or use **Add profile…** to
   choose an `.icc`/`.icm` file; it is saved to the library and selected immediately.
   Re-adding the same file reuses its entry. Standard color spaces have their own
   group. The profile's name supplies the proof indicator automatically.
3. Choose rendering intent, black point compensation and **Print simulation**:
   **Colors only**, **Black ink**, or **Paper and ink**. Paper and ink includes
   both simulations; absolute intent disables BPC. **Manage saved profiles…**
   lives inside the picker. Removing a library entry leaves a selected or embedded
   profile intact. The same picker is used for source interpretation and delivery,
   with independent selections.
4. Choose **Prepare and Apply**. Check the visible **Proof: target** indicator.
   The canvas and Navigator should change together. Try paper/ink settings on
   both a neutral ramp and saturated colors. Preparing a complex profile can
   take several seconds; Cancel leaves the saved setup unchanged.
5. Compare with **View → Soft Proof** (`Ctrl+Alt+P`) and **Gamut Warning**
   (`Ctrl+Shift+G`). Warning gray is a viewing overlay. Toggling must leave the
   document's saved/dirty state unchanged. These shortcuts preserve `Ctrl+Y` redo.
6. Use **Save As** for a print variant, then paint or adjust colors while proofing.
   Check undo/redo for both the edit and a changed proof setup. Reopen the saved
   variant: the recipe must remain available, with temporary view toggles off.
7. Export using the lab's **explicit delivery profile** and requested file/depth
   settings. A CMYK proof can accompany an sRGB or Adobe RGB delivery. Importing
   a proof target does not choose an export profile. Compare exports made with
   proof/warnings on and off; simulation and warning gray must never enter them.
8. Try cancelling setup, removing it and undoing removal. An invalid or unsupported
   ICC must produce an actionable error rather than display a substituted target.

The test corpus uses `/usr/share/color/icc/krita/cmyk.icm` and downloaded WhiteWall
and ICC targets under `artifacts/color-m3/references/`. Use the profile supplied
for your actual printer/paper or lab when judging the workflow.

## Limits to keep visible during review

- Proofing supports bounded SDR RGB artwork in all four working spaces and both
  integer depths. Extended composed values are clamped for this preview and
  marked by Gamut Warning. Exact artwork and export retain their normal contract.
- Supported targets are usable bidirectional ICC v2/v4 RGB matrix/LUT or CMYK LUT
  image profiles. Profiles exceeding memory or interpolation limits fail explicitly.
- Gamut decisions near the documented threshold are ambiguous. A warning is not
  a measurement of physical print error.
- Calibrated-display matching and physical print comparison are unverified.
  The performance record includes cold preparation, first-use latency and brief
  save/export contention. Dirty-image regeneration remains a separate limitation.

After this review, confirm acceptance or describe the behavior to adjust. This
handoff stops here; it does not authorize another platform or HDR work.
