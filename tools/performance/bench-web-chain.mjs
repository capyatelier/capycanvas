export const PEN_HZ = 200;
export const TOUCH_HZ = 240;
export const ALIASES = { transform: 'scale_rotate', apply: 'apply_transform', magic_wand: 'auto_select' };
export const WORKSPACES = { photo: 'builtin:workspace:photographer' };
export const PRESENTED = 'Display::FrameDisplayed';
export const CLOCK_MARK = 'capy-bench-clock';

export function motionMs(step, chain, refreshHz) {
  return step.duration_ms ?? (step.motion_frames ?? chain.motion_frames ?? 150) * 1000 / refreshHz;
}

export function guaranteeSize(pixels) {
  return pixels < 20_000_000 ? 1024 : pixels < 40_000_000 ? 1536 : 2048;
}

export function sampleCount(durationMs, hz, contacts = 1) {
  return Math.max(1, Math.round(durationMs * hz / 1000 / contacts));
}

export function at(area, fx, fy) {
  return [area.x + area.w * fx, area.y + area.h * fy];
}

export function strokePressure(progress, durationMs) {
  return .625 - .375 * Math.cos(2 * Math.PI * progress * durationMs / 1000);
}

export function strokePoint(area, path, progress, durationMs) {
  const t = progress * durationMs / 1000;
  return path === 'zigzag'
    ? at(area, .5 + .3 * (1 - 4 * Math.abs(t % 1 - .5)), .5 + .2 * (2 * progress - 1))
    : at(area, .5 + .3 * Math.cos(2 * Math.PI * t), .5 + .2 * Math.sin(2 * Math.PI * t));
}

export function gesturePoints(area, kind, progress, flick) {
  const wave = Math.sin(Math.PI * progress), radius = .12 * Math.min(area.w, area.h);
  const [x, y] = at(area, .5 + (kind === 'pan' ? .3 * wave : 0), .5);
  const spread = radius * (kind === 'pinch' ? 1 + 1.5 * wave : 1) * (1 + .6 * flick);
  const angle = kind === 'rotate' ? Math.PI / 3 * wave : 0;
  return [-1, 1].map(d => [x + d * spread * Math.cos(angle), y + d * spread * Math.sin(angle)]);
}

export function canvasDragPoint(area, path, progress, start) {
  if (path === 'loop') return at(area, .5 + .25 * Math.sin(2 * Math.PI * progress), .5 - .25 * Math.cos(2 * Math.PI * progress));
  const radius = .15 * Math.min(area.w, area.h);
  return [start[0] + radius * (1 - Math.cos(Math.PI * progress)), start[1] - radius * Math.sin(Math.PI * progress)];
}

export function lerp(a, b, t) {
  return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
}

export function panelDragPoint(progress, start, float, radius, drop) {
  if (progress < .25) return lerp(start, float, progress / .25);
  if (progress < .75) {
    const q = 2 * Math.PI * (progress - .25) / .5;
    return [float[0] + radius * Math.sin(q), float[1] - radius * (1 - Math.cos(q))];
  }
  return lerp(float, drop ?? start, (progress - .75) / .25);
}

export function sweepPoint(start, progress, horizontal, extent) {
  const offset = extent * Math.sin(Math.PI * progress);
  return horizontal ? [start[0] + offset, start[1]] : [start[0], start[1] - offset];
}

export function fillPoint(point, area, camera, drawing, view) {
  const { zoom, rotation, flipped, translation } = camera;
  const x = drawing.width / 2 * (flipped[0] ? -zoom : zoom), y = drawing.height / 2 * (flipped[1] ? -zoom : zoom);
  const cos = Math.cos(rotation), sin = Math.sin(rotation);
  const center = [view.x + (translation[0] + cos * x - sin * y) * view.scale, view.y + (translation[1] + sin * x + cos * y) * view.scale];
  const dx = point[0] - center[0], dy = point[1] - center[1];
  const reach = Math.max(Math.abs((cos * dx + sin * dy) / (x * view.scale)), Math.abs((cos * dy - sin * dx) / (y * view.scale)));
  if (reach < 1) return point;
  const projected = [center[0] + dx * .9 / reach, center[1] + dy * .9 / reach];
  if (projected[0] < area.x || projected[0] > area.x + area.w || projected[1] < area.y || projected[1] > area.y + area.h)
    throw Error('Fill point does not intersect the visible drawing');
  return projected;
}

export function surfaceOf(step) {
  if (step.surface) return step.surface;
  if (['save', 'close'].includes(step.do)) return 'window';
  return step.do === 'drag' && !/^(canvas|object:)/.test(step.target) ? 'window' : 'canvas';
}

export function documentNeeds(step, opened) {
  return [...new Set([...(['launch', 'resume', 'close'].includes(step.do) ? [] : [opened]), step.document, ...(step.tabs ?? [])]
    .filter(Boolean))];
}

export function selectSteps(steps, wanted) {
  if (!wanted) return steps;
  const known = new Set(steps.map(step => step.id)), unknown = [...wanted].filter(id => !known.has(id));
  if (unknown.length) throw Error(`Unknown steps: ${unknown.join(', ')}`);
  return steps.filter(step => wanted.has(step.id));
}

export function stopPageOnTimeout(cdp) {
  const send = cdp.call;
  let timeout = null;
  cdp.call = async (method, params, session = cdp.session) => {
    if (session == null) return send(method, params, session);
    if (timeout) throw timeout;
    try { return await send(method, params, session); }
    catch (error) {
      if (error.message?.startsWith('CDP timeout: ')) timeout ??= error;
      throw error;
    }
  };
  return () => timeout;
}

export const idleTimedOut = reason => /^(Error: )?Timed out waiting for idle/.test(reason ?? '');

export function stepRecord(step, beginNs, stopped = null) {
  return { id: step.id, status: stopped ? 'error' : 'ok', reason: stopped ? `not run: ${stopped}` : null,
    input: 'real', begin_ns: beginNs, end_ns: stopped ? beginNs : null, surface: surfaceOf(step),
    motion: [], contacts: [], completions: [], settled_ns: null, latency: [], extra: {} };
}

export function traceEvents(text) {
  const trace = JSON.parse(text);
  return Array.isArray(trace) ? trace : trace.traceEvents ?? [];
}

export function presentedTimes(events) {
  return [...new Set(events.filter(event => event.name === PRESENTED).map(event => Math.round(event.ts * 1000)))].sort((a, b) => a - b);
}

export function pageClock(events) {
  const mark = events.find(event => event.name === CLOCK_MARK && event.args?.data?.startTime != null);
  return mark ? { pid: mark.pid, offset_ns: Math.round(mark.ts * 1000 - mark.args.data.startTime * 1e6) } : null;
}

function firstAtOrAfter(times, value) {
  let low = 0, high = times.length;
  while (low < high) {
    const middle = (low + high) >> 1;
    if (times[middle] < value) low = middle + 1; else high = middle;
  }
  return times[low];
}

export function frameRows(submitted, presented) {
  if (!presented.length) return { evidence: 'submitted', rows: submitted.map(t_ns => ({ t_ns, surface: 'canvas' })) };
  const canvas = [...new Set(submitted.map(t => firstAtOrAfter(presented, t)).filter(t => t !== undefined))];
  return { evidence: 'presented', rows: [...canvas.map(t_ns => ({ t_ns, surface: 'canvas' })), ...presented.map(t_ns => ({ t_ns, surface: 'window' }))]
    .sort((a, b) => a.t_ns - b.t_ns || a.surface.localeCompare(b.surface)) };
}

export function completions(rows, from, to) {
  const kept = [];
  let newest = -Infinity;
  for (const row of rows.toSorted((a, b) => a.complete_ns - b.complete_ns)) {
    if (row.complete_ns < from || row.complete_ns > to || row.input_ns == null || row.input_ns <= newest) continue;
    newest = row.input_ns;
    kept.push({ complete_ns: row.complete_ns, input_ns: row.input_ns });
  }
  return kept;
}

export function memoryRows(samples, rendererPid) {
  return samples.map(({ t_ns, os_available_kib, renderers, gpu_process_rss_kib, gpu_kib }) => {
    const renderer = rendererPid == null ? null : renderers[rendererPid] ?? null;
    const row = { t_ns, os_available_kib, renderer_rss_kib: renderer, gpu_process_rss_kib, gpu_kib };
    if (renderer != null && gpu_process_rss_kib != null) row.app_rss_kib = renderer + gpu_process_rss_kib;
    return Object.fromEntries(Object.entries(row).filter(([, value]) => value != null));
  });
}
