# Android UI before canvas shaders

Android now shows a neutral gray launch background, then the workspace controls
over the theme's gray surround while the canvas GPU initializes. This path has no
dependency on the application's Vulkan shaders. Drawing still follows the staged
canvas/brush readiness rules.

The previous window and Compose fallback backgrounds were dark gray, but the
`SurfaceView` exposed black when it was created without a buffer. Android documents
that a [SurfaceView punches through its containing window](https://developer.android.com/reference/android/view/SurfaceView),
so a background behind that view does not cover the empty surface. The old host
also withheld the correctly sized toolbar/panel layout until `Native.attach`
returned from GPU initialization.

The fix publishes the catalog before shader-resource I/O and publishes the sized
workspace before GPU attachment. An opaque Compose layer covers the SurfaceView
until the first submitted frame's GPU work completes. Readiness belongs to each
surface, and the UI checks its generation before removing the cover; a previous
surface's completion cannot expose an empty replacement. The one-time completion
check polls without waiting on the UI thread. Canvas rendering continues through
the existing native SurfaceView compositor layer.

The system splash, window background, and initial Compose fallback use the same
neutral gray (`#808080`). Once the model is available, the placeholder uses the
current theme's surround color.

Measured on the USB-connected Wacom MovinkPad 14, Android 15, September 10, 2026.
The final ARM64 debug APK was built from isolated commit `bcda425`, containing fix
`42d06dd` and the other agents' published milestones. Uncommitted shared-renderer
work was excluded from this final build.

| Measurement | Final, empty app shader cache | Final, warm app shader cache |
| --- | ---: | ---: |
| Recorded black canvas interval | 0 ms | 0 ms |
| Initial gray window draw finishes | 379 ms | 342 ms |
| First workspace draw (header/placeholder) | 1,255 ms | 1,157 ms |
| First canvas buffer submitted | 1,524 ms | 1,305 ms |
| UI receives first-buffer GPU completion | 1,761 ms | 1,354 ms |
| All shader work ready | 4,734 ms | 2,274 ms |

Times use process birth as the origin. Window/workspace draw markers and GPU
submission/completion markers are distinct from physical display presentation.
The black interval was checked separately in compositor screen recordings:
every decoded frame's central 16×16 pixels was inspected. The original installed
APK showed **131 ms of black canvas**; neither final recording contained a black
canvas frame. These are diagnostic samples, not statistical startup benchmarks.
The original video and original Perfetto capture were separate launches.

Both final launches started a fresh process. The empty-cache run removed only
the application's generated `cache/shader-pipelines` directory; the driver cache
was not reset. The app's cold Compose/model initialization remains visible in the
roughly 1.2 seconds to the first workspace draw. This change removes the black
interval and shader dependency of the UI, rather than eliminating all startup CPU
work.

`AndroidFirstUiTest` holds the real canvas worker immediately before Vulkan device
creation. With that worker still held, it verifies the visible placeholder, opens
the View menu, and reads the actual compositor screenshot to check the gray
canvas. The screenshot shows panels, the open menu, zero pipelines, zero canvas
frames, and GPU unavailable. Releasing the gate brings up the rendered canvas;
activity recreation also completes with the replacement surface ready.

The final APK passed all five targeted device tests: the no-GPU UI test, drawing
and navigation during speculative compilation, stylus drawing with undo/redo,
touch history/cancellation with surface recovery, and camera publication timing.
ARM64 APK/test builds and `lintDebug` passed (zero lint errors; existing warnings).
The user's original preferences were restored byte-for-byte before final startup
recordings, and the final APK was left installed and running.

Local evidence is under `artifacts/android/`: `gray-ui-final-{cold,warm}.mp4`,
matching Perfetto traces and logcat files, `gray-ui-results.json`,
`startup-without-gpu.png`, `gray-ui-final-device-tests.txt`, and
`gray-ui-final-build.txt`. `record-gray-ui.py` records a fresh process with video
and Perfetto; `analyze-gray-ui.py` extracts the timing markers and black intervals.

APK: `capy-canvas-gray-startup-arm64-debug.apk`.
SHA-256: `d4399b5de3b722f2ed88e5328189b480ed48015c2461351d8965bdc15cafb250`.
