// SPDX-License-Identifier: MIT OR Apache-2.0
// node apps/layer-linux/bench/summarize.mjs <received.json | pacing.json> [...]
import {readFileSync} from 'node:fs';
import {gunzipSync} from 'node:zlib';

function distribution(samples) {
    if (!samples.length) return null;
    const sorted = [...samples].sort((a, b) => a - b);
    const percentile = p => +sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * p) - 1)].toFixed(3);
    return {count: sorted.length, median: percentile(.5), p95: percentile(.95), p99: percentile(.99), max: percentile(1)};
}

const groups = new Map();
for (const path of process.argv.slice(2)) {
    const bytes = readFileSync(path);
    const json = JSON.parse(path.endsWith('.gz') ? gunzipSync(bytes) : bytes);
    for (const r of Array.isArray(json) ? json : [json]) {
        const name = (r.events ? 'NativePan' : r.brush) + (r.gpu_timestamps === false ? ' (no GPU queries)' : '')
            + (r.event_loop === 'native_wait' ? ' (native event wait)' : '');
        if (!groups.has(name)) groups.set(name, {runs: [], samples: {}});
        const group = groups.get(name);
        const add = (key, samples) => (group.samples[key] ??= []).push(...samples);
        for (const key of ['input_cpu', 'input_handler_cpu', 'frame_handler_cpu', 'gtk_paint_cpu', 'main_dispatch_cpu', 'main_dispatch_thread_cpu', 'wake_lateness'])
            add(`${key}_ms`, r[key] ?? []);
        add('gpu_ms', r.worker_gpu.map(v => v[1]));
        add('worker_cpu_ms', r.worker_cpu.map(v => v[3]));
        for (const [i, phase] of ['compose', 'encode', 'submit', 'feedback', 'present'].entries())
            add(`worker_${phase}_ms`, (r.worker_cpu_stages ?? []).map(v => v[i + 1]));
        const threadCpu = r.worker_thread_cpu ?? [];
        add('worker_thread_cpu_ms', threadCpu.map(v => v.slice(1).reduce((a, b) => a + b, 0)));
        for (const [i, phase] of ['acquire', 'compose', 'encode', 'submit', 'feedback', 'present'].entries())
            add(`worker_${phase}_thread_cpu_ms`, threadCpu.map(v => v[i + 1]));
        const pan = r.events?.filter(e => e[2] === 1);
        const active = !pan || !pan.length ? () => true : t => t >= pan[0][0] && t <= pan.at(-1)[0];
        // Exclude setup/focus movements and genuinely idle time from pan FPS.
        const presented = r.canvas_presentation.filter(p => p[3] && active(p[1])).sort((a, b) => a[1] - b[1]);
        add('presentation_interval_ms', presented.slice(1).map((p, i) => (p[1] - presented[i][1]) / 1e6));
        const queued = new Map(r.worker_cpu.map(p => [p[0], p[4]]));
        add('enqueue_to_presentation_ms', presented.filter(p => queued.has(p[0])).map(p => (p[1] - queued.get(p[0])) / 1e6));
        const rate = list => list.length < 2 ? 0 : (list.length - 1) * 1e9 / (list.at(-1) - list[0]);
        const run = {path, presented: presented.length, presented_fps: +rate(presented.map(p => p[1])).toFixed(3), discarded: r.canvas_presentation.filter(p => !p[3]).length};
        if (r.input_hz !== undefined) {
            run.synthetic_input_events = r.input_events;
            run.synthetic_input_hz = +r.input_hz.toFixed(3);
        }
        if (pan?.length) {
            run.pan_events = pan.length;
            run.pointer_hz = +rate(pan.map(e => e[0])).toFixed(3);
            add('event_interval_ms', pan.slice(1).map((p, i) => (p[0] - pan[i][0]) / 1e6));
            // GDK's event clock is unsigned 32-bit monotonic milliseconds.
            add('event_age_ms', pan.map(e => ((e[0] / 1e6 - e[1] + 2 ** 31) % 2 ** 32) - 2 ** 31));
        }
        group.runs.push(run);
    }
}
console.log(JSON.stringify(Object.fromEntries([...groups].map(([name, group]) => [name, {
    runs: group.runs, ...Object.fromEntries(Object.entries(group.samples).map(([key, samples]) => [key, distribution(samples)])),
}])), null, 2));
