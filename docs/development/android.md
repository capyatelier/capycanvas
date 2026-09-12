# Android development

[Developer guide](README.md) · [Platform integration](../platforms/README.md)

The Android client uses Kotlin and Jetpack Compose for the editor UI. A Rust JNI
bridge connects it to `NativeHost`, and the shared Vulkan renderer presents into
a `SurfaceView`.

## Prerequisites

Install Java 17 or newer, Rust, Android Studio or the Android command-line tools,
and these SDK packages:

- Android SDK platform 37 and Build Tools 37.0.0.
- NDK 29.0.14206865 and Platform Tools.
- The Android Emulator and a tablet system image if testing without a device.

The exact versions are set in
[`app/build.gradle.kts`](../../apps/layer-android/app/build.gradle.kts). Install
these through Android Studio's SDK Manager. Set `ANDROID_HOME` if the SDK is not
at `$HOME/Android/Sdk`; the launcher uses that path by default.

Prepare the Rust targets and JNI build tool:

```bash
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk --locked
```

For emulator use, create a tablet AVD named `medium_tablet` in Device Manager,
or select another existing name with `CAPY_ANDROID_AVD`. Enable hardware graphics.
The app's minimum Android API is 29; the compile SDK and emulator OS need not have
the same version.

## Build and run

```bash
bash apps/layer-android/run.sh
```

The launcher starts the configured emulator if necessary, builds for its ABI,
installs the debug APK and opens the app. The default serial is `emulator-5554`.
For an already connected device, select its serial:

```bash
CAPY_ANDROID_SERIAL=DEVICE_SERIAL bash apps/layer-android/run.sh
```

`DEVICE_SERIAL` is the value shown by `adb devices`. `run.sh headless` starts an
emulator without a window; `run.sh test` runs instrumented tests. The Gradle
wrapper supplies Gradle and builds the Rust library through `cargo-ndk`.

## How the host works

Compose renders shared tool and workspace models. Android collects `MotionEvent`
history and available predictions, while a dedicated render owner handles the
session and Vulkan work. `Choreographer` supplies frame timing. Surface recreation,
backgrounding and input cancellation need Android lifecycle handling.

[Document transport](../../apps/layer-android/app/src/main/java/art/capycanvas/Documents.kt)
uses Android's Storage Access Framework. Kotlin opens document-provider locations;
Rust processes projects using file descriptors and background work. Providers
control their own destination behavior, so local-filesystem atomic replacement
cannot be assumed for every URI.

## Validation status

The [feature-parity record](../history/android-feature-parity.md) and
[Android implementation record](../history/android-implementation.md) describe
completed checkpoints and outstanding checks. Emulator UI tests do not establish
physical stylus accuracy, thermal behavior or high-refresh presentation. Use a
real tablet to validate those properties.

## Focused device tests and debugging

Run from the repository root with the SDK's `platform-tools` on `PATH`. Enable
USB debugging and authorize the development computer on the device. Select an
attached device or running emulator, then build and install without clearing data:

```bash
adb devices -l
export CAPY_ANDROID_SERIAL=DEVICE_SERIAL
CAPY_TEST_ABI=$(adb -s "$CAPY_ANDROID_SERIAL" shell getprop ro.product.cpu.abi | tr -d '\r')
(cd apps/layer-android && ./gradlew :app:assembleDebug :app:assembleDebugAndroidTest :app:lintDebug "-PcapyAbi=$CAPY_TEST_ABI")
adb -s "$CAPY_ANDROID_SERIAL" install -r apps/layer-android/app/build/outputs/apk/debug/app-debug.apk
adb -s "$CAPY_ANDROID_SERIAL" install -r apps/layer-android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
adb -s "$CAPY_ANDROID_SERIAL" shell am instrument -w -e class art.capycanvas.AndroidWorkspaceSwitcherTest art.capycanvas.test/androidx.test.runner.AndroidJUnitRunner
```

Use `ClassName#methodName` in the fully qualified test selector for one regression.
Read the instrumentation result (`OK` or `FAILURES`); the shell exit status alone
does not establish success. Start with
[`AndroidInteractionTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidInteractionTest.kt)
for drawers and drag geometry, or
[`AndroidWorkspaceManagerTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidWorkspaceManagerTest.kt)
and [`AndroidWorkspaceSwitcherTest`](../../apps/layer-android/app/src/androidTest/java/art/capycanvas/AndroidWorkspaceSwitcherTest.kt)
for persistence, menus and window lifecycle. Workspace-manager tests use isolated
SQLite stores; other tests can edit live document/settings state, so save user work
before running them. Do not uninstall the app or clear its storage to reset a test.

```bash
mkdir -p artifacts/android
adb -s "$CAPY_ANDROID_SERIAL" logcat -d -v threadtime > artifacts/android/logcat.txt
adb -s "$CAPY_ANDROID_SERIAL" exec-out screencap -p > artifacts/android/screen.png
adb -s "$CAPY_ANDROID_SERIAL" pull /sdcard/Android/data/art.capycanvas/files/validation artifacts/android/
adb -s "$CAPY_ANDROID_SERIAL" shell am start -n art.capycanvas/.MainActivity
```

The validation directory contains captures from tests that produce them. Trace
input in [`WorkspaceInput.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceInput.kt)
and [`WorkspaceRows.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/WorkspaceRows.kt),
then host publication in [`CanvasHost.kt`](../../apps/layer-android/app/src/main/java/art/capycanvas/CanvasHost.kt).
Shared drag/drop policy and history live in `crates/layer-ui`; workspace storage
and ownership live in `crates/layer-workspace`.

Interaction tests dispatch typed mouse/touch/stylus `MotionEvent`s through native
views; `AndroidInteractionTest` optionally accepts `-e systemInput true` where OS
injection is supported. Neither substitutes for physical pen testing. When adding
held-contact tests, use `runOnMainSync` and bounded condition polling: global idle
waits can hang during a held gesture. Test focus loss with a real window and send
keyboard events through system dispatch so Android leaves touch mode correctly.
