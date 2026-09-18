# Phase 4: HDR display and export feedback

Follow-up to the user's report that an HDR-enabled desktop showed “Showing SDR”
and that Export was an awkward, long list. Work is in `~/code/capycanvas3` on
`capycanvas3`. No push is authorized before the next user review.

Source milestones: `367d7ee1` (HDR and export), `471ae553` (local GTK runtime).

## What changed

**HDR viewing.** The old host only requested Windows-scRGB. This desktop's
Wayland color manager v2 advertises parametric BT.2020 PQ but not scRGB (feature
7), so the app fell back to 8-bit SDR despite HDR being enabled. It now negotiates
PQ on a pass-through RGBA16Float surface when scRGB is unavailable. Shared Rust
presentation uses the same BT.2020 matrix and 203 cd/m² reference as interchange,
with Float32 shader processing. PQ is a bounded display derivative; colors outside
its signal gamut/range are limited for viewing, without changing editing samples.
SDR preview and print proofing still use the saved SDR rendition.

Compositor feedback now uses the compositor's reference white, wakes an idle
window, and refreshes the label and Preview SDR availability. Promotion from SDR
to HDR negotiates the surface in the existing window; reopening is unnecessary.
Renderer replacement retains the negotiated encoding and current viewing state.
Display Details distinguishes the compositor's hint from a physical brightness
measurement. It does not claim a 10,000-nit monitor when the compositor reports
the full PQ signal range.

**Export.** A native `AdwDialog` / `AdwNavigationView` replaces the oversized
alert and scroll form. The overview contains preview, dynamic range, format,
JPEG quality when applicable, and concise links to Size, Color & transparency,
Presets, and SDR Appearance. Back retains one draft; Choose file is on the
main page. All main-page controls fit without scrolling in the tested ordinary
window. Shorter windows can scroll this overview without exposing every setting.

- Size retains aspect-preserving fit, optional enlargement and print resolution
  metadata. Its summary shows actual output pixels.
- Color & transparency retains ICC selection, depth, background and optional
  8-bit dithering. Rendering intent is under Color conversion. The unavailable
  black-point control and routine profile-embedding explanation are removed.
- JPEG hides its fixed depth and cannot offer transparency. The native controls
  obtain available backgrounds from Rust's export draft rules, including CMYK
  delivery profiles; these changes do not add native CMYK documents.
- Presets uses standard Adwaita rows, with applicable Save/Update/Remove/Reset
  actions. Update and Remove name the saved preset even after edits show Custom.
  There is no oddly styled “Manage presets” expander or row of small buttons.
- HDR shows its fixed PNG / BT.2020 PQ / 16-bit / alpha contract. It hides SDR
  profile/depth/matte controls and the SDR Appearance link. A range failure shows
  the deliberate clipping choice beside the warning; strict export remains
  disabled. Preview state is structured, not inferred from translated UI text.
- SDR Appearance belongs to the SDR delivery flow and still edits the saved
  document rendition with live canvas preview. Export returns to its draft after
  Apply or Cancel. Preset maintenance and view toggles do not edit artwork.

## Validation evidence

Files are under `artifacts/color-m4/`. The recorded app and test SHA256 values,
source commit, runtime identity and final run results are in
`review/build-manifest.json`.

| Check | Evidence |
| --- | --- |
| Workspace/all-target compilation | `feedback-workspace-check-final.log` |
| Reference-white headroom policy | `feedback-headroom-unit.log` |
| Four GPU HDR tests, including CPU/PQ/scRGB/SDR/proof parity | `feedback-hdr-gpu.log` |
| Real desktop HDR promotion, label refresh, SDR preview and panel navigation | `feedback-desktop/test-fixed.log` and `feedback-desktop-final/test.log` |
| HDR edit/save/reopen/SDR and PQ PNG delivery | `feedback-journey-final.log` |
| Strict HDR range failure, explicit clipping, stale-check invalidation | `feedback-preflight-final.log` |
| Native PNG/TIFF/JPEG and embedded RGB/grayscale ICC delivery | `feedback-final-native_document_files.log` |
| CMYK-profile TIFF delivery and opaque-background choices | `feedback-cmyk-delivery.log` |
| Size/density, master preservation and cancelled-dialog release | `feedback-final-native_export_sizes_preserve_master_and_release_cancelled_dialogs.log` |
| Named preset save/update/remove/reset and remembered delivery | `feedback-final-native_export_presets_save_update_remove_reset_and_remember_after_delivery.log` |
| Panel navigation, preserved draft, overview without scrolling | `feedback-final-native_hdr_display_negotiation_and_export_navigation.log` |
| HDR GPU loss/recovery | `feedback-final-native_hdr_gpu_failure_recovery.log` |
| Print-proof setup/history/save/reopen/RGB export | `feedback-proof.log` |
| Saved ICC profile and untagged-source policy | `feedback-profiles.log` |
| Browser SDR workflows | `feedback-web-sdr.log` |
| Browser HDR rejection preserves the SDR drawing | `feedback-web-hdr-limits.log` |

The physical desktop test observed a negotiated PQ floating surface, an HDR label
and a working SDR Preview toggle. The preferred description reports the full PQ
range (10,000 cd/m²) and reference whites of 142/203 cd/m² in separate runs. These
are compositor hints, **not photometric measurements or measured monitor peaks**.
Read-only display state lists the attached Wacom and LG displays in color mode 1.
No OS display settings were changed. Calibration, cross-monitor moves, physical
pen/touch ergonomics, constrained/mobile devices and other hosts' HDR remain
unqualified. Browser HDR continues to be rejected explicitly.

## Large-document responsiveness

The existing 8192 × 7324 half-float ProPhoto workload ran at 3840 × 2160,
120 Hz, scale 2 using the same bundled GTK runtime for the retained baseline
and the revised build. Five camera phases issue 960 requests without waiting
for the preceding frame. Original samples and artwork preview revisions remain
unchanged by navigation.

| Input timing | Before p99 request → presentation | Revised p99 | Before/revised p99 worker CPU | Before/revised p99 GPU | Missed refresh slots before/revised |
| --- | --- | --- | --- | --- | --- |
| Free-running | 8.688 ms | 13.091 ms | 0.544 / 0.431 ms | 0.377 / 0.371 ms | 0 / 0 |
| Phase offset 0 | 11.193 ms | 9.134 ms | 0.542 / 0.470 ms | 0.413 / 0.378 ms | 0 / 1 |
| Phase offset 4 ms | 13.697 ms | 6.423 ms | 0.481 / 0.532 ms | 0.364 / 0.466 ms | 1 / 0 |

The initial end-to-end increase was investigated, not omitted. Input/refresh
alignment changes the result substantially across process/compositor launches;
the subsequent comparisons do not reproduce an increase. Worker CPU/GPU deltas
remain below the common 0.2 ms investigation threshold. These numbers do not
establish a physical pen latency bound or prove that every request arrives
within one 120 Hz refresh. All runs and unmatched/discarded feedback counts are
retained in `feedback-hdr60-*.summary.json`, with the requested phase recorded in
the associated JSON. The previous pre-UI executable is retained in
`performance/gtk-tests`; this comparison includes all later UI corrections.

The final stress case adds a 20-effect chain, a physical filter and concurrent
save/reopen plus a 480 MB TIFF export. It passed: save 100.4 ms, combined delivery
8.03 s, process peak RSS 1501.8 MiB (`time -v`) and sampled GPU allocation
2142 MiB. Sampling also saw an RSS high-water value of 1512.5 MiB; use the higher
observation when sizing memory. The previous stress record was 1558.8 MiB RSS
and 2046 MiB GPU. The new floating-point display surface increases graphics
allocation; this is not a constant-memory claim. Samples are taken about every
0.5 s and can miss transient GPU peaks. Evidence: `feedback-hdr60-stress.*`;
this virtual output tests resource cost and responsiveness, not physical HDR.

## GTK startup crash

The unmodified system GTK 4.22.4 again crashed before canvas creation. The core
has event type 27 (`GDK_PAD_GROUP_MODE`), a null `GdkEvent.surface`, and `r12=0`
at the surface mapped-state access. This replaces the earlier unproven theory
about the opening window's lifetime. Source confirms that the Wayland pad-mode
callback constructs this event from keyboard focus, which may be absent.

A local GTK runtime guards the missing surface before mapped-state access;
it does not disable tablet input or change window timing. The mode state has
already been retained in GTK's device bookkeeping. The HDR review launcher alone
selects this runtime. The system library remains unchanged. Build script, patch
and dependency notes are in [tools/build/gtk-review](../../../tools/build/gtk-review/README.md).
Source and LGPL license are retained alongside the review artifact.

Evidence: `review/desktop-pq-startup-backtrace.log`,
`review/desktop-pq-startup-event.log`, and `review/desktop-pq-fixed-runtime.log`.
The same review document then opened, rendered in PQ and exited cleanly.
Five consecutive launches through the final packaged launcher each captured
the drawing and exited with code 0 (`feedback-startup-final/results.json`).
The bundled-runtime desktop navigation test also passed; this does not claim
that every possible GTK/device failure is eliminated.
The older review binary also exhibits the large textured brush cursor visible in
some desktop screenshots; that overlay is not introduced by this display change.
Screenshots use the saved SDR appearance and cannot qualify emitted HDR luminance.

The system-runtime crash is still relevant for deployments that omit this fix.
The bundled runtime is a local review dependency, not an upstream GTK release.

Sources: [GTK event layout](https://github.com/GNOME/gtk/blob/4.22.4/gdk/gdkeventsprivate.h),
[GTK Wayland pad events](https://github.com/GNOME/gtk/blob/4.22.4/gdk/wayland/gdkseat-wayland.c),
[Wayland color management](https://wayland.app/protocols/color-management-v1), and
[Adwaita navigation](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/class.NavigationView.html).
