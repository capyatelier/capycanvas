# Web and Android phase 4 qualification

This branch starts at `59aff3df` (origin/main, 2026-09-18). The requested review
covers Web and Android's integration of the current shared Rust HDR contracts
and reviewed GTK workflows. It does not qualify physical HDR presentation or
change the global milestone's other-host gates.

## Budgets declared before large-document measurements

Run release Wasm in ordinary hardware WebGPU Chrome, and the release Rust core
inside the isolated Android debug host (`art.capycanvas.hdr`). Use the attached
Wacom tablet on its current power/display settings; capture model, OS, browser,
GPU, memory, battery and thermal state with the results. Do not infer physical
pen latency from injected input or frame cadence.

Workloads: sparse 4K HDR painting; dense 24, 45 and 60 MP HDR content; repeated
navigation/pen contacts; SDR appearance edits; histogram; cancellation; native
save/reopen and export contention. Record cold and warm results separately.
Use three repeated interaction runs where supported. Preserve precision when
rejecting a workload that exceeds the host's admission budget.

| Measurement | Desktop Chrome | Tablet Chrome / native Android |
| --- | --- | --- |
| Warm input submission p95 / p99 | 16.7 / 33.4 ms | 33.4 / 66.7 ms |
| Maximum event-loop heartbeat gap during background work | 100 ms | 200 ms |
| Cancellation to idle | 1 s | 2 s |
| Cold document ready, 24 / 45 / 60 MP | 30 / 45 / 60 s | 45 / 60 / 90 s |
| Peak application process memory, ordinary / concurrent delivery | 3 / 4 GiB | 2 / 3 GiB |
| Accounted renderer allocation, steady | 2 GiB | 1.5 GiB |

Memory figures must distinguish measured process residency, renderer accounting
and unavailable browser/driver allocations. Browser JS heap is not total memory.
For unchanged SDR paths, investigate a reproducible p95/p99 regression above
`max(5%, 0.2 ms)` against the runnable parent and existing fixed baseline. Missing
measurements, missed presentation feedback, untested large combinations and
sustained thermal coverage remain outstanding; they are not implied passes.

## Review record

The final validation report links runnable packages, exact source/build hashes,
commands, screenshots, numerical checks and measured limits. Browser file-picker
handles may be supplied by the harness; codecs, workers, WebGPU, storage and
input dispatch must be real. Native tablet tests must exercise JNI, Vulkan,
Compose and Android touch/stylus dispatch. Physical pen feel and physical HDR
luminance require human/hardware review beyond those automation paths.
