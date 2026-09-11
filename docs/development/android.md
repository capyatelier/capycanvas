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
