// node analyze-live.mjs WIDE_BRUSH_JSON [WIDE_BRUSH_JSON ...]
// GPU samples are the app's rolling window, not per-stroke timestamps.
import {readFileSync} from 'node:fs';
import {gunzipSync} from 'node:zlib';
function stats(values) {
  const v = [...values].sort((a, b) => a - b);
  if (!v.length) return {count: 0};
  const q = p => v[Math.ceil(p * v.length) - 1];
  return {count: v.length, p50: q(.5), p95: q(.95), p99: q(.99), max: v.at(-1)};
}
const reports = process.argv.slice(2).map(path => {
  const bytes = readFileSync(path);
  const report = JSON.parse(path.endsWith('.gz') ? gunzipSync(bytes) : bytes);
  return {path, algorithm: report.algorithm, preset: report.preset,
    turns_per_second: report.turns_per_second,
    runs: report.runs.map((r, stroke) => {
      const t = r.timeline;
      const index = name => t.frame_fields.indexOf(name);
      const frames = t.frames;
      const inputs = t.inputs;
      const start = index('start_ns'), callback = index('cpu_callback_ns');
      const duration = frames.length ? (frames.at(-1)[start] + frames.at(-1)[callback] - frames[0][start]) / 1e9 : 0;
      const phases_ms = Object.fromEntries(t.frame_fields.filter(n => n.endsWith('_ns') &&
        !['vsync_ns', 'start_ns', 'expected_presentation_ns'].includes(n)).map(name =>
        [name.replace(/_ns$/, ''), stats(frames.map(f => f[index(name)] / 1e6))]));
      // Input-worker completion to the next callback's return. This includes
      // scheduling/CPU waits, but is NOT GPU completion or nib-to-photon latency.
      const workerToCallback = inputs.flatMap(input => {
        const ready = input[2] + input[3];
        const next = frames.find(f => f[start] >= ready);
        return next ? [(next[start] + next[callback] - ready) / 1e6] : [];
      });
      return {stroke, failure: r.failure, frames: frames.length, duration_s: duration,
        callbacks_per_second: r.failure || !duration ? null : frames.length / duration,
        submitted_contacts_cumulative: Number(r.renderer_stats.rows?.find(row => row.label === 'Dabs')?.value.replaceAll(',', '')),
        input_events: inputs.length, input_samples: inputs.reduce((n, i) => n + i[4], 0),
        worker_queue_ms: stats(inputs.map(i => (i[2] - i[1]) / 1e6)),
        worker_to_callback_return_ms: stats(workerToCallback),
        rolling_cpu_ms: stats(r.renderer_stats.samples),
        rolling_gpu_ms: stats(r.renderer_stats.gpu_samples),
        phases_ms, swept_counts: r.swept_counts,
        resident_bytes: r.tracked_canvas_bytes, process_pss_bytes: r.process_pss_bytes,
        process_mappings: r.process_mappings};
    })};
});
console.log(JSON.stringify(reports, null, 2));
