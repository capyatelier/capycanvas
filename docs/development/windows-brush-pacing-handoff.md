# Windows brush pacing handoff

[Windows development](windows.md)

Restore the Windows brush frame rate that 5e196eb79 lost. Delete this file in
the commit that lands the fix.

On a ThinkPad P43s (i7-8565U, Intel UHD 620 and Quadro P520, built-in 60 Hz
1920×1080 panel at 150%), a 9 px Pencil stroke submits about **41 canvas
frames per second** at 12f9a7749. At 622a1fef6 (Nov 10) it submits
**97–128**. Each frame now takes less CPU than before. The drop comes from
the render thread sleeping at least 16 ms after every frame.

## Cause

Line numbers are at 13281b9b4.

1. `FramePacing::ready()` (`apps/layer-windows/native/src/frame_pacing.rs:11`)
   used to block in `device.poll(PollType::Wait)` until the previous frame's
   GPU work finished. Since 5e196eb79 it returns `false` at once while that
   work is in flight. `submitted()` (`:16-19`) sets the flag from
   `gpu.on_gpu_done` and then calls the host wake.
2. `capy_acquire` (`apps/layer-windows/native/src/host.rs:1248-1251`) returns
   0 when pacing is not ready. With `IN_FLIGHT = 1`, the next acquire comes
   about 1 ms after Present. The previous frame is still on the GPU at that
   point (about 6–8 ms per frame here), so nearly every second acquire
   returns 0.
3. `apps/layer-windows/CanvasWindow.cpp:1425-1429` handles 0 with
   `wake.wait_for(lock, 16ms, [&]{return closing||resize;})`. This code is
   unchanged since 622a1fef6. It rarely ran before because pacing seldom
   returned 0.
4. The completion wake reaches the services callback
   (`CanvasWindow.cpp:1293-1297`), which sets `servicesReady` and calls
   `notify_one()`. The predicate ignores `servicesReady`, so the thread goes
   back to sleep until the full 16 ms has passed.

## Measurements

The harness is `tools/performance/windows-pen-latency.ps1 -Executable <exe>
-Project <.capy> -OutputDirectory <dir> -Diameter 9 -BrushPreset Pencil
-Seconds 6 -SkipPresentMon`, run unchanged on both builds. Projects are
`small-768x512.capy` and `large-6144x4096.capy`. Each run has three measured
contacts; the small canvas got two runs per build and the large canvas one.
Both builds used the same surface: Immediate present, Bgra8UnormSrgb, maximum
frame latency 1, 1920×1008 viewport. The stage columns come from
`latency-*-frames.csv`:

- Pacing wait: `pacing_ns`.
- Frame: `acquire_start_ns` to `render_end_ns`.
- Idle: from `render_end_ns` to the next frame's `acquire_start_ns`.

Values are ranges of per-contact p50s unless marked otherwise.

| Build, canvas | Submitted FPS | Interval p50 / p99 ms | Pacing wait ms | Frame ms | Idle ms (min) | Input→submit ms |
| --- | --- | --- | --- | --- | --- | --- |
| 622a1fef6, 768×512 | 107.8–128.3 | 7.5–8.9 / 12.2–19.7 | 3.5–4.0 | 6.1–7.6 | 1.0–1.1 (0.44) | 7.8–9.6 |
| 622a1fef6, 6144×4096 | 97.4–110.3 | 8.7–9.6 / 14.0–16.7 | 4.2–4.6 | 7.6–8.6 | 1.0–1.1 (0.41) | 9.2–10.6 |
| 12f9a7749, 768×512 | 41.4–42.0 | 23.6–24.3 / 28.9–33.3 | 0.00 | 1.8–2.9 | 20.6–22.2 (13.5) | 13.1–14.2 |
| 12f9a7749, 6144×4096 | 39.5–40.5 | 24.4–24.7 / 32.6–36.3 | 0.00 | 2.6–2.8 | 21.6–21.8 (16.5) | 14.5–15.2 |

Idle gaps under 12 ms: 0 of 2,579 frames at 12f9a7749, and 7,142 of 7,144 at
622a1fef6. The new build runs at the same rate on both canvases, which points
to a fixed sleep rather than GPU load. The raw captures are on that laptop in
`artifacts/windows/pencil9-fps-20261114`.

## Check while fixing

- `capy_acquire` also returns 0 for surface `Timeout` and `Occluded`. Keep a
  timed retry for those cases; a pacing wait has to end on the completion
  wake.
- The completion itself may be observed late:
  - `crates/layer-render-wgpu/src/device_poll.rs:287` parks the
    `capy-gpu-poll` thread for 1 ms between `Poll` calls while the queue is
    busy.
  - On Windows, that park rounds up to the process timer resolution.
  - After the fix, the idle gap should come down to about GPU time minus CPU
    time, with no steps at timer ticks.
- Each completion now sets `servicesReady`. The next loop iteration then
  probably runs `capy_poll_services` and publishes a snapshot on every frame
  (`CanvasWindow.cpp:1364-1390`). This hasn't been measured.
- Navigation, object drags and animations use the same acquire path and
  probably have the same ceiling. Only Pencil was measured.

## Done when

- The harness above on the fixed build, on both projects, on this laptop
  shows:
  - submitted FPS back to the 622a1fef6 range;
  - idle p50 near 1 ms, with no 16 ms floor;
  - interval p99 at or below 16.7 ms.
- `node tools/performance/windows-pen-report.mjs <dir>` accepts each run.
- A regression test fails without the fix.
- The [checks for the change](testing.md) and the
  [diff review](../COMMIT_GUIDE.md#review-the-diff) pass.

Not established: 39e1a26b0, the commit just before 5e196eb79, was not built or
bisected; there is no ETW profile; presented FPS was not measured; other hosts
were not checked.
