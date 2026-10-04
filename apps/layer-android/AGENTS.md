# Android client

Start with the [Android guide](../../docs/development/android.md).

- Reserve a tablet and run device commands through `tools/devices/devices.py run`
  ([devices](../../docs/development/devices.md)).
- Install under your own `-PcapyApplicationId` (`tools/devices/devices.py appid`);
  never uninstall or clear `art.capycanvas`. `run.sh` installs
  `art.capycanvas.dev`; `run.sh test` uses your own ID.
- An instrumentation run's exit status proves nothing; read `OK` or `FAILURES`.
