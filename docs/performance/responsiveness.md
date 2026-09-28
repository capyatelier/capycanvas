# Responsiveness

[Performance targets](../PERFORMANCE_TARGETS.md)

These interactions produce one change rather than continuous motion, so they are
measured as latency. The limits are the same on every tier unless a cell says
otherwise.

| Interaction | Limit | Low | Mid | Top |
| --- | --- | --- | --- | --- |
| Pen down → first submitted canvas update | 2 frames: 33 / 22 / 17 ms | Input queueing 5–13 ms (debug build; `68820ad8`, 2026-09-26) | | Navigation → first pen submission 6.7–11.0 ms ([buffered navigation](../development/android-buffered-navigation-20260921.md), 2026-09-22) |
| Tap or press → visible response: buttons, tools, menus | 100 ms | | | |
| Transform or placement press → first moving frame | 100 ms | | 22 ms for a selection Distort, 47 ms for a photo handle (`debff77d`, `fe605aa3`, 2026-09-27) | |
| Undo or redo of one 1024 px stroke | 250 ms | | | |
| Magic Wand or Color Select on the tier canvas | 250 ms | | | |
| Tonal selection, warm mask on the tier canvas | 350 ms | | Huion: 244–266 ms on 61 MP ([tonal performance](../development/tonal-performance.md), 2026-09-24) | |
| Filter preview after a parameter change | 100 ms p95 | | | |
| Command search open or query → drawn | 50 ms p95 | | Huion: 20.5 ms (`9b830b42`, 2026-09-25) | |
| Warm launch → canvas ready for a stroke | 2 s | | Huion: workspace ready 1.72–1.80 s ([shader readiness](../development/shared-shader-readiness.md), 2026-09-25) | |
| Cold launch with an empty shader cache → ready | 10 s | About 18 s in instrumented runs | Huion: 3.50 s to all shaders; workspace 2.34 s ([shader readiness](../development/shared-shader-readiness.md), 2026-09-25) | |
| Open the tier photo → first frame | 3 / 4 / 6 s | | | |
