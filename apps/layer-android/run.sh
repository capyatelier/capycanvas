#!/usr/bin/env bash
set -euo pipefail
capy_android_dir=$(cd -- "$(dirname -- "$0")" && pwd)
capy_repo_dir=$(cd -- "$capy_android_dir/../.." && pwd)
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/29.0.14206865"
export PATH="$ANDROID_HOME/platform-tools:$PATH"
export ANDROID_SERIAL="${CAPY_ANDROID_SERIAL:-emulator-5554}"
capy_avd="${CAPY_ANDROID_AVD:-medium_tablet}"
capy_mode="${1:-run}"
if [[ "$capy_mode" != run && "$capy_mode" != test && "$capy_mode" != headless ]]; then
    echo "Usage: $0 [run|headless|test]" >&2
    exit 2
fi
if [[ ! -x "$ANDROID_HOME/platform-tools/adb" || ! -d "$ANDROID_NDK_HOME" ]]; then
    echo "Install the Android SDK and NDK r29 first; see docs/android-implementation.md." >&2
    exit 1
fi
if ! adb get-state >/dev/null 2>&1; then
    if ! "$ANDROID_HOME/emulator/emulator" -list-avds | rg -Fxq "$capy_avd"; then
        echo "Create the tablet first: android --sdk=\"$ANDROID_HOME\" emulator create medium_tablet" >&2
        exit 1
    fi
    mkdir -p "$capy_repo_dir/target/android-tools"
    capy_emulator_args=(-avd "$capy_avd" -gpu host -no-snapshot -no-audio)
    if [[ "$capy_mode" == headless || "$capy_mode" == test ]]; then
        capy_emulator_args+=(-no-window)
    fi
    "$ANDROID_HOME/emulator/emulator" "${capy_emulator_args[@]}" >"$capy_repo_dir/target/android-tools/emulator.log" 2>&1 &
fi
capy_deadline=$((SECONDS + 180))
until [[ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == 1 ]]; do
    if ((SECONDS > capy_deadline)); then
        echo "Emulator did not boot; see target/android-tools/emulator.log." >&2
        exit 1
    fi
    sleep 1
done
capy_abi=$(adb shell getprop ro.product.cpu.abi | tr -d '\r')
if [[ "$capy_mode" == test ]]; then
    "$capy_android_dir/gradlew" -p "$capy_android_dir" :app:connectedDebugAndroidTest \
        "-PcapyAbi=$capy_abi" -Pandroid.injected.androidTest.leaveApksInstalledAfterRun=true
else
    "$capy_android_dir/gradlew" -p "$capy_android_dir" :app:assembleDebug "-PcapyAbi=$capy_abi"
    adb install -r "$capy_android_dir/app/build/outputs/apk/debug/app-debug.apk"
    adb shell am start -n art.capycanvas/.MainActivity
fi
