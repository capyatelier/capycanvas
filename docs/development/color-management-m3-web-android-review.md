# Web and Android print proofing review

Review builds and raw evidence are in `artifacts/color-m3-web-android/`.
The [validation record](../history/color-management-web-android-m3-validation.md)
describes the tested flows, performance, failures and untested cases. GTK's
accepted workflow is preserved; Web/Android user acceptance is still pending.

## Run

Android: **Capy Canvas Proof** is installed on the connected Wacom tablet as
`art.capycanvas.proof`, separate from the normal app. The ARM64 debug review APK
is `artifacts/color-m3-web-android/review/capycanvas-proof.apk`.

Web: the static distribution is `dist/capycanvas/`, also archived as
`artifacts/color-m3-web-android/review/capycanvas-web.tar.gz`.
Serve the directory on localhost:

```sh
python3 -m http.server 8132 --bind 127.0.0.1 --directory dist/capycanvas
```

Open `http://127.0.0.1:8132/`. While the tablet is connected, the configured
`adb reverse tcp:8132 tcp:8132` makes that address work in tablet Chrome too.
The site can be installed as a PWA. No public deployment is required. The archive
includes notices and fingerprinted runtime assets, without test ICCs or photos.

For a portable example, open `artifacts/color-m3-web-android/proof-portable.capy`.
It is a synthetic P3/U16 drawing made in Web, embedding just its active CMYK proof.
A copy is in the tablet's `Download/CapyCanvasProof/` folder. View toggles start off
when the file opens. Use your printer/paper profile to judge your actual workflow.

## Review

1. In a new drawing, choose **View → Proof Colors**. First use opens **Proof
   Setup**; Cancel must leave the drawing unchanged. The commands are grouped
   with **Proof Setup…** and **Gamut Warning**. With a keyboard, use `Ctrl+Alt+P`
   and `Ctrl+Shift+Y`; `Ctrl+Y` remains redo.
2. Open the single **Proof profile** picker. Choose a standard/saved target or
   **Add Profile…**. **Manage Profiles…** is in the picker. Defaults are relative
   intent, BPC and black-ink simulation; absolute intent disables BPC. Try
   **Colors only**, **Black ink**, and **Paper and ink**, then Apply.
3. Compare canvas and Navigator with Proof Colors and Gamut Warning. Turning both
   off hides proof status. The viewing toggles must not mark the drawing modified.
   On Android, the visible proof status also opens setup.
4. With an embedded profile, select a replacement. The original must stay under
   **Document Profile** throughout setup. Cancel must not add it to Saved Profiles.
   Apply must preserve it locally before changing the recipe. Undo/redo the recipe.
   Hide/remove local library entries and check that the embedded target still works.
5. Edit while proofing, undo/redo the edit, save and reopen. Reopening retains the
   recipe but resets viewing toggles. Export using an independently chosen delivery
   profile. Proof/warning overlays must not appear in exports, samples or histograms.
6. Cancel a preparation, background/resume the app, and rotate Android. The drawing
   and recipe must remain usable. Note any unexpected stall, missing status,
   altered color, or lost profile and the action that triggered it.

The 61 MP measurement shows a performance limit: tablet Web proof navigation had
about 33 ms p95 callback intervals, and Apply included an 80 ms main-thread task.
Native preparation measured 308 ms; Web 916 ms. Neither physical-print matching
nor sustained 120 Hz proof navigation is claimed. Full details are in validation.

## Rebuild and reproduce

```sh
bash apps/layer-web/build.sh
node apps/layer-web/package.mjs
ANDROID_HOME=/home/babymastodon/Android/Sdk apps/layer-android/gradlew \
  -p apps/layer-android :app:assembleDebug :app:assembleDebugAndroidTest \
  -PcapyAbi=arm64-v8a -PcapyApplicationId=art.capycanvas.proof \
  -PcapyAppLabel='Capy Canvas Proof'
```

The package needs the tools documented in [Web packaging](web-packaging.md).
This workspace used `LAYER_RESVG=/tmp/capy-audit-tools/bin/resvg` and
`LAYER_CARGO_ABOUT=/tmp/capy-audit-tools/bin/cargo-about`.

For the Web journey, serve a local CMYK profile at `/pkg/proof-cmyk.icc` and a
Display P3 matrix ICC at `/pkg/proof-p3.icc`. Test fixtures are deliberately not
in the distribution. Then run:

```sh
LAYER_WEB_URL=http://127.0.0.1:8130 node apps/layer-web/test.mjs --proof
adb -s 5ll21u1002931 forward tcp:9230 localabstract:chrome_devtools_remote
node apps/layer-web/proof-tablet.test.mjs TEST_TAB_ID journey
node apps/layer-web/proof-tablet.test.mjs TEST_TAB_ID performance
```

The tablet CLI only touches the explicitly selected test tab. `performance` needs
the documented 9504×6336 photo at `/pkg/proof-photo61mp.jpg`; it saves three normal
and three proof runs. `memory` opens/prepares the same fixture with a short idle
period for an external sampler, without the navigation timing loop. Override
fixture URLs with `LAYER_PHOTO_URL`, `LAYER_PROOF_URL`, `LAYER_PROOF_ORIGINAL_URL`;
use `LAYER_CDP_URL` for a different forwarded debugger port. An optional third
CLI argument changes the output directory. No flags or unrelated tabs are changed.

For native tests, install the matching test APK, push the CMYK and synthetic
portable file into `/data/local/tmp`, and run:

```sh
adb -s 5ll21u1002931 shell am instrument -w -r \
  -e class art.capycanvas.AndroidRasterTest#proofSetupCompareEditPortabilityExportAndRecovery \
  -e proofProfile /data/local/tmp/capy-proof-cmyk.icc \
  -e proofPortableFile /data/local/tmp/capy-proof-portable.capy \
  art.capycanvas.proof.test/androidx.test.runner.AndroidJUnitRunner
```

`AndroidPhotoNavigationBenchmarkTest` accepts `-e photoBenchmark true` and the
optional `-e proofProfile …`. Its documented photo fixture goes in the isolated
app's `files/photo-benchmark.jpg`; generated reports go to its external files
folder. Sample `dumpsys meminfo` and `/proc/meminfo` in a separate run, keeping raw
first/warm runs and whole-browser attribution limits visible.
