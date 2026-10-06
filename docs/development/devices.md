# Test devices

[Developer guide](README.md)

Performance targets are measured on Android tablets, and several sessions share
them. This guide lists the devices, how to reserve one, and how to test on it
without disturbing anyone else. Shared machine resources are in
[environment](environment.md).

## Tablets

| Name | Model | Role | GPU |
| --- | --- | --- | --- |
| `tcl` | TCL TAB 11 Gen 2 (9465X) | Low tier reference | Mali-G52 MC2 |
| `movinkpad11` | Wacom MovinkPad 11 (DTHA116) | Mid tier reference | Mali-G57 MC2 |
| `movinkpad14` | Wacom MovinkPad Pro 14 (DTHA140) | Top tier reference | Adreno 735 |
| `huion` | Huion Kamvas Pad 12 (KP1202) | Pen input testing | Mali-G57 |
| `xppen` | XP-Pen Magic Note Pad (MNP1095) | Pen input testing | Mali-G57 |

[Tier hardware](../performance/hardware.md) has the full specifications.
Device quirks that affect tests:

- The MovinkPad 11 panel presents at 60 Hz, not 90 Hz, and its EGL has no
  `buffer_age`, so the Android UI redraws every frame in full.
- The TCL needs about 18 s to start from a cold shader cache; waits shorter than
  that time out on the first run after an install.
- The XP-Pen's `libgui` throttles FIFO presents on an older fence than AOSP and
  crashes when shared and FIFO presentation alternate. The Android host adds one
  frame of navigation latency on MediaTek Android 14 devices to avoid it; a
  navigation crash seen only on the XP-Pen points there first.
- The Huion launcher force-stops an app it sees updated; wait a few seconds after
  `adb install` before starting instrumentation.
- An install can stall on a Play Protect dialog. These are test devices:
  `adb shell settings put global verifier_verify_adb_installs 0` turns the check off.

## Reserving a tablet

A tablet serves one session at a time. [`tools/devices/devices.py`](../../tools/devices/devices.py)
records reservations and serializes commands:

```bash
tools/devices/devices.py list
tools/devices/devices.py reserve --tier mid --note "canvas bar frame cost"
tools/devices/devices.py reserve movinkpad14 --hours 8
tools/devices/devices.py run movinkpad11 -- adb install -r app-debug.apk
tools/devices/devices.py release
```

- **Reserve before use and release when done.** A reservation names its owner,
  the worktree directory by default, and expires after `--hours` (4 by default).
  Subagents working in the same worktree share it.
- **Pick by tier, not by name**, unless you were assigned a tablet. If the user
  assigns one, use only that one. If they say a tablet belongs to someone else,
  leave it alone even when `list` shows it free.
- **Run every device command through `run`.** It holds the tablet's lock
  (`/tmp/capy-<serial>.lock`, the same file `flock` users take), waits while
  another command runs, and requires a current reservation by this owner. Inside
  the command, `adb` targets the tablet through `ANDROID_SERIAL`, and
  `CAPY_ANDROID_SERIAL`, `CAPY_APPLICATION_ID`, `CAPY_WEB_PORT` and
  `CAPY_CDP_PORT` are set.
- Build outside `run`; hold the lock only while the device is in use.

## Keeping installs apart

- Install experiments and test builds under your own application ID:
  `-PcapyApplicationId=$(tools/devices/devices.py appid) -PcapyAppLabel=<label>`.
  The test package is `<id>.test`, and `devices.py run` exports the same ID as
  `$CAPY_APPLICATION_ID`. `apps/layer-android/run.sh` installs `art.capycanvas.dev`
  and is for single-user work on an emulator or your own device; `run.sh test`
  uses your own ID.
- Never uninstall `art.capycanvas` or `art.capycanvas.editor` or clear their
  data; they hold an artist's drawings. Never touch another session's application IDs.
- A `connectedDebugAndroidTest` run uninstalls the tested app afterwards,
  deleting its data. Always give it your own `-PcapyApplicationId`, as
  `run.sh test` does.
- Before each run, check that none of your app's processes are still running.
  At the end, uninstall your own IDs and remove your own port forwards
  (`adb forward --remove tcp:$CAPY_CDP_PORT`, `adb reverse --remove tcp:$CAPY_WEB_PORT`).
- Chrome on a tablet uses the device's real profile. Use your own origin and
  port, open your own tab, and never drive a tab you did not open:

  ```bash
  LAYER_WEB_PORT=$CAPY_WEB_PORT ./apps/layer-web/run.sh
  tools/devices/devices.py run movinkpad11 -- sh -c 'adb reverse tcp:$CAPY_WEB_PORT tcp:$CAPY_WEB_PORT &&
    adb forward tcp:$CAPY_CDP_PORT localabstract:chrome_devtools_remote &&
    adb shell am start -a android.intent.action.VIEW -d http://127.0.0.1:$CAPY_WEB_PORT/ -p com.android.chrome'
  ```

  `curl --max-time 5 http://127.0.0.1:$CAPY_CDP_PORT/json/list` lists the tabs.
  The [Web guide](web.md) runs the device journeys from there.

## Windows

- **VMs.** [`tools/windows-vm/windows-vm.py`](windows-vm.md) runs one Windows 11
  VM per worktree, named after the worktree. Commands that use a VM lock it; pass
  `--vm` to use another. VMs render with WARP, so they check builds and UI
  fixtures, never performance.
- **Physical PC.** A Windows 11 Home laptop (Intel Core i7-1255U, Iris Xe, 60 Hz
  panel) is the only Windows GPU and pen hardware. It cannot measure 120 Hz
  painting, and Windows Home has no Sandbox for clean package installs. It is not
  reachable from the Linux machine; work that needs it runs there.

## Apple

Apple builds and tests need an Apple Silicon Mac with Xcode; there is no remote
Mac. The reference hardware is an M2 Pro Mac mini, whose display presents at
90 Hz, and a 13-inch M4 iPad Pro at 120 Hz. On Linux, only the `layer-apple`
Rust bridge tests run.

Use `--platform apple` with the same reservation tool. It discovers paired
physical iPads through Xcode's `devicectl` and shares the reservation and command
locks used by the Android runner. When several iPads are paired, select one by
its serial.

```bash
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer tools/devices/devices.py --platform apple list
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer tools/devices/devices.py --platform apple reserve ipad
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer tools/devices/devices.py --platform apple run ipad -- \
  sh -c 'xcrun devicectl device info details --device "$CAPY_APPLE_DEVICE_ID"'
DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer tools/devices/devices.py --platform apple release ipad
```

Apple commands require a current reservation. The runner exports
`CAPY_APPLE_DEVICE_ID` and the owner's isolated `CAPY_APPLE_BUNDLE_ID`; pass the
latter as an `xcodebuild` build setting when building the app.

- The iPad must be awake and unlocked, with Settings > Developer > Enable UI
  Automation on.
- Never replace or clear an installed `art.capycanvas.CapyCanvas`,
  `art.capycanvas.CapyCanvas.dev` or older `art.capycanvas.apple.ipad`/`.mac`
  app; they may hold an artist's drawings. Build test and benchmark apps under
  your own `CAPY_APPLE_BUNDLE_ID` and give test launches their own
  `CAPY_STORAGE_DIR` ([Apple guide](apple.md)). A free Personal Team profile
  limits how many apps a device may hold, and the XCTest runner counts as one.
- There is no iPad hardware keyboard, second Mac display, 120 Hz Mac display or
  working iCloud Drive account. Leave those checks unverified; don't ask for them.
- Keep device UDIDs, team IDs and signing details out of commits.
