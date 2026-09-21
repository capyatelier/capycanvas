// Analyze bounded host timing and DXGI statistics, never label it photon latency.
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
export function distribution(values) {
  const v = values.filter(Number.isFinite).sort((a, b) => a - b);
  if (!v.length) return null;
  const p = q => v[Math.floor(q * (v.length - 1))];
  return {count: v.length, mean_ms: v.reduce((a, b) => a + b, 0) / v.length,
    p50_ms: p(.5), p95_ms: p(.95), p99_ms: p(.99), max_ms: v.at(-1)};
}
function csv(file) {
  const rows = fs.readFileSync(file, 'utf8').trim().split(/\r?\n/);
  const keys = rows.shift().split(',');
  return rows.filter(Boolean).map(line => Object.fromEntries(line.split(',').map((v, i) => [keys[i], Number(v)])));
}
export function analyze(directory) {
  const meta = JSON.parse(fs.readFileSync(path.join(directory, 'capture.json'), 'utf8').replace(/^\uFEFF/, ''));
  const prefix = path.join(directory, `latency-${meta.surface.window_id}`);
  if (JSON.parse(fs.readFileSync(prefix + '-status.json', 'utf8')).overflow) throw Error('Trace overflow; reject this run');
  const inputs = csv(prefix + '-input.csv'), consumed = csv(prefix + '-consumed.csv'), frames = csv(prefix + '-frames.csv');
  if (!inputs.length || !consumed.length) throw Error('No pen input reached the render owner');
  if (inputs.length !== consumed.length || new Set(consumed.map(p => p.sequence)).size !== inputs.length
      || new Set(inputs.map(p => p.sequence)).size !== inputs.length) throw Error('Incomplete pen consumption; reject this run');
  const injectionPath = path.join(directory, 'injected.csv');
  const injected = fs.existsSync(injectionPath) ? csv(injectionPath) : null;
  if (injected && injected.length !== inputs.length) throw Error('Injection/receipt count mismatch');
  const offsets = [];
  const timedInputs = inputs.map((p, i) => {
    if (!injected) return p;
    const source = injected[i], before = source.qpc_before * (1e9 / meta.qpc_frequency);
    const after = source.qpc_after * (1e9 / meta.qpc_frequency);
    if (source.index !== i || !Number.isFinite(before) || !Number.isFinite(after) || after < before
        || (i && source.qpc_before <= injected[i - 1].qpc_before)
        || (i && p.sequence <= inputs[i - 1].sequence)) throw Error('Invalid injection correspondence');
    // Windows-generated injected timestamps have millisecond quantization.
    // Pair complete ordered histories and check each reported timestamp against
    // its injection interval; never fit an offset or hide missing samples.
    if (p.sample_ns < before - 2e6 || p.sample_ns > after + 2e6) throw Error('Injection timestamp correspondence mismatch');
    offsets.push((p.sample_ns - before) / 1e6);
    return {...p, sample_ns: before};
  });
  const bySequence = new Map(timedInputs.map(p => [p.sequence, p])), byFrame = new Map(frames.map(f => [f.frame, f]));
  const actual = frames.filter(f => f.stats_error === 0 && f.displayed_present > 0 && f.sync_qpc > 0);
  const samples = new Map();
  for (const f of actual) samples.set(f.sync_refresh, f.sync_qpc);
  const sync = [...samples].sort((a, b) => a[0] - b[0]);
  const periods = sync.slice(1).map(([r, t], i) => (t - sync[i][1]) * 1000 / meta.qpc_frequency / (r - sync[i][0]));
  const period = distribution(periods)?.p50_ms;
  const validPeriod = period > 4 && period < 50;
  // SyncInterval=0 can flip during a refresh. Its refresh-start timestamp is
  // not that flip's timestamp and must not be reported as exact display latency.
  const refreshTimestampApplicable = meta.surface.present_mode === 'Fifo';
  const displayed = new Map(), observed = new Map();
  if (validPeriod && refreshTimestampApplicable) for (const f of actual) {
    const time = f.sync_qpc * 1000 / meta.qpc_frequency + (f.present_refresh - f.sync_refresh) * period;
    if (!displayed.has(f.displayed_present)) displayed.set(f.displayed_present, time);
  }
  for (let i = 0; i + 1 < frames.length; i++) {
    const f = frames[i], next = frames[i + 1];
    // The next acquisition starts AFTER this frame's statistics query. This
    // is a conservative upper bound on when the host observed that present ID,
    // not an exact display timestamp. It includes any intervening owner work.
    if (next.acquire_start_ns < f.render_end_ns) throw Error('Inconsistent timestamp order');
    if (!f.stats_error && f.displayed_present > 0 && !observed.has(f.displayed_present)) {
      observed.set(f.displayed_present, next.acquire_start_ns / 1e6);
    }
  }
  const delivery = [], queue = [], submit = [], display = [], observation = [], latest = new Map();
  let unmatched = 0;
  for (const c of consumed) {
    const p = bySequence.get(c.sequence), f = byFrame.get(c.frame);
    if (!p || !f) { unmatched++; continue; }
    if (!f.last_present || byFrame.get(c.frame - 1)?.last_present === f.last_present) throw Error('Consumed input has no new present');
    delivery.push((p.arrival_ns - p.sample_ns) / 1e6);
    queue.push((f.render_start_ns - p.arrival_ns) / 1e6);
    submit.push((f.render_end_ns - p.sample_ns) / 1e6);
    if (!latest.has(c.frame) || latest.get(c.frame).sample_ns < p.sample_ns) latest.set(c.frame, p);
    const shown = displayed.get(f.last_present), seen = observed.get(f.last_present);
    // Never substitute a later frame for an unobserved or dropped present.
    if (shown !== undefined) display.push(shown - p.sample_ns / 1e6);
    if (seen !== undefined) observation.push(seen - p.sample_ns / 1e6);
  }
  if (unmatched) throw Error('Consumed pen input has no completed host frame');
  if (delivery.some(v => v < -.1) || queue.some(v => v < 0) || display.some(v => v < -.1)
      || observation.some(v => v < -.1)) throw Error('Inconsistent timestamp domains or presentation mapping');
  const active = frames.filter(f => latest.has(f.frame));
  const newest = map => distribution(active.flatMap(f => {
    const t = map.get(f.last_present);
    return t === undefined ? [] : [t - latest.get(f.frame).sample_ns / 1e6];
  }));
  return {
    scope: 'OS-injected pen to DXGI-reported presentation, excluding physical digitizer, scanout position and panel response',
    timestamp_origin: injected ? 'QPC immediately before each OS injection call' : 'WinUI pointer timestamp',
    pointer_timestamp_offset_ms: injected ? {min: Math.min(...offsets), max: Math.max(...offsets)} : null,
    process_id: meta.process_id, diameter: meta.diameter, present_mode: meta.surface.present_mode,
    no_vsync_wait: meta.surface.no_vsync_wait ?? false,
    inputs: inputs.length, consumed: consumed.length, unmatched,
    stats_errors: [...new Set(frames.map(f => f.stats_error))], valid_stats: actual.length,
    observed_displayed_frames: observed.size, refresh_period_ms: validPeriod ? period : null,
    refresh_timestamp_applicable: refreshTimestampApplicable,
    delivery_ms: distribution(delivery), owner_queue_ms: distribution(queue), input_to_frame_return_ms: distribution(submit),
    host_frame_ms: distribution(active.map(f => (f.render_end_ns - f.render_start_ns) / 1e6)),
    acquire_wait_ms: distribution(active.map(f => (f.acquired_ns - f.acquire_start_ns) / 1e6)),
    input_to_dxgi_display_ms: distribution(display), newest_input_to_dxgi_display_ms: newest(displayed),
    display_matched_inputs: display.length,
    input_to_presentation_observation_bound_ms: distribution(observation),
    newest_input_to_presentation_observation_bound_ms: newest(observed),
    observation_matched_inputs: observation.length,
  };
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const result = analyze(process.argv[2]);
  fs.writeFileSync(path.join(process.argv[2], 'summary.json'), JSON.stringify(result, null, 2) + '\n');
  console.log(JSON.stringify(result, null, 2));
}
