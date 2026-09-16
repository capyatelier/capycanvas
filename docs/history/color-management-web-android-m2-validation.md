# Web and Android milestone 2 integration

Work in progress after the merged GTK checkpoint. This record does **not**
qualify either port as complete. Apple and Windows remain unported.

## Native color controls checkpoint — 2026-09-15

Android now requests Float32 filtering/blending and constructs native SDR
renderers for startup, project replacement and recovery. Renderer forwarding
reports the actual document representation and tiled-source capability. The
shared numeric color form powers Android and browser effect/gradient editors;
changing input models without editing retains the exact tagged color. Gradient
previews sample the same encoded-document interpolation used by the shader.

Photo project construction is shared with GTK. Android Open detects native
archives versus JPEG/PNG/TIFF, retains original source samples/profile/depth, and
clears the save location for an imported photograph. The missing-profile Ask
flow, host color settings, richer delivery dialogs and full browser native
storage are still pending.

Validation logs are under `artifacts/color-m2/web-android/`:

- `shared-color-form.log`: shared round-trip and invalid-draft test passed.
- `package-color-controls.log`: browser packaging tests passed.
- `web-contract-check.log`: actual Wasm target checked.
- `android-photo-check.log`: actual Android ARM64 target checked with cargo-ndk.
- `android-photo-apks.log`: isolated debug app and instrumentation APK built.
- `tablet-native-sdr-controls.log`: `AndroidHostTest#nativeSdrTaggedColorsAndGradientEditor`
  passed on physical Wacom DTHA140 (`5ll21u1002931`), Android 15, Adreno 735. It
  paints, undoes/redoes, edits a Display P3 gradient stop, switches the numeric
  model and confirms the untouched gradient definition is unchanged.

Install/test commands (SDK root `$ANDROID_HOME`):

```sh
CARGO_NET_OFFLINE=true apps/layer-android/gradlew -p apps/layer-android --offline :app:assembleDebug :app:assembleDebugAndroidTest -PcapyAbi=arm64-v8a -PcapyApplicationId=art.capycanvas.colorm2
$ANDROID_HOME/platform-tools/adb -s 5ll21u1002931 install -r apps/layer-android/app/build/outputs/apk/debug/app-debug.apk
$ANDROID_HOME/platform-tools/adb -s 5ll21u1002931 install -r apps/layer-android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
$ANDROID_HOME/platform-tools/adb -s 5ll21u1002931 shell am instrument -w -e class art.capycanvas.AndroidHostTest#nativeSdrTaggedColorsAndGradientEditor art.capycanvas.colorm2.test/androidx.test.runner.AndroidJUnitRunner
```

The existing application was preserved; testing uses `art.capycanvas.colorm2`.
The requested 9504×6336 JPEG is in `/sdcard/Download/sony_a7r_v_29.jpg`. Host and
tablet SHA-256 match:
`3aac9c9b8b34c38a5e0121f16ad1ec806e92a19e15ee5e1128f36d987e888054`.

No tablet photo-navigation performance or browser precision claim is made at
this checkpoint. Final qualification requires actual 120 Hz navigation, native
save/reopen and profiled exports in both hosts, followed by the user's test.
