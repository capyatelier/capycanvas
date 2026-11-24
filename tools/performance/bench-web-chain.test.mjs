import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import {
  PRESENTED, CLOCK_MARK, canvasDragPoint, completions, documentNeeds, fillPoint, frameRows, gesturePoints, guaranteeSize, idleTimedOut, memoryRows,
  motionMs, pageClock, panelDragPoint, presentedTimes, sampleCount, selectSteps, stepRecord, stopPageOnTimeout, strokePoint,
  strokePressure, surfaceOf, traceEvents,
} from './bench-web-chain.mjs';

const chain = JSON.parse(await readFile(new URL('./bench-chain.json', import.meta.url)));
const area = { x: 100, y: 50, w: 1000, h: 600 };
const near = (actual, expected) => actual.forEach((value, index) => assert.ok(Math.abs(value - expected[index]) < 1e-9, `${actual} != ${expected}`));

test('motion follows the chain frames at the measured refresh unless a step sets its duration', () => {
  assert.equal(motionMs({}, chain, 120), 1250);
  assert.equal(motionMs({ duration_ms: 1000 }, chain, 120), 1000);
  assert.equal(sampleCount(1250, 200, 4), 63);
  assert.deepEqual([1e7, 3e7, 6e7].map(guaranteeSize), [1024, 1536, 2048]);
});

test('stroke paths match the Android chain geometry', () => {
  near(strokePoint(area, 'zigzag', 0, 1000), [300, 230]);
  near(strokePoint(area, 'zigzag', 1, 1000), [300, 470]);
  near(strokePoint(area, 'zigzag', .5, 1000), [900, 350]);
  near(strokePoint(area, 'ellipse', 0, 1000), [900, 350]);
  near(strokePoint(area, 'ellipse', .25, 1000), [600, 470]);
  assert.equal(strokePressure(0, 1000), .25);
  assert.equal(strokePressure(.5, 1000), 1);
});

test('two-finger gestures stay symmetric around their moving centre', () => {
  const [left, right] = gesturePoints(area, 'pan', .5, 0);
  near([(left[0] + right[0]) / 2, left[1]], [900, 350]);
  const pinch = gesturePoints(area, 'pinch', .5, 0);
  assert.ok(Math.abs(pinch[1][0] - pinch[0][0] - 2 * .12 * 600 * 2.5) < 1e-9);
  const rotate = gesturePoints(area, 'rotate', .5, 0);
  assert.ok(Math.abs(Math.atan2(rotate[1][1] - rotate[0][1], rotate[1][0] - rotate[0][0]) - Math.PI / 3) < 1e-9);
  assert.ok(gesturePoints(area, 'pinch', 1, 1)[1][0] > gesturePoints(area, 'pinch', 1, 0)[1][0]);
});

test('drag paths start at their anchor and the panel drag returns to its drop', () => {
  near(canvasDragPoint(area, 'arc', 0, [600, 350]), [600, 350]);
  near(canvasDragPoint(area, 'arc', 1, [600, 350]), [600 + .3 * 600, 350]);
  near(canvasDragPoint(area, 'loop', 0, null), [600, 200]);
  near(panelDragPoint(0, [10, 10], [500, 300], 40, [20, 30]), [10, 10]);
  near(panelDragPoint(.5, [10, 10], [500, 300], 40, [20, 30]), [500, 220]);
  near(panelDragPoint(1, [10, 10], [500, 300], 40, [20, 30]), [20, 30]);
  near(panelDragPoint(1, [10, 10], [500, 300], 40, null), [10, 10]);
});

test('fill taps outside the drawing move onto it', () => {
  const camera = { zoom: .5, rotation: 0, flipped: [false, false], translation: [100, 100] }, view = { x: 0, y: 0, scale: 1 };
  const drawing = { width: 800, height: 400 };
  near(fillPoint([300, 200], area, camera, drawing, view), [300, 200]);
  near(fillPoint([700, 200], area, camera, drawing, view), [300 + 400 * .9 * 200 / 400, 200]);
  assert.throws(() => fillPoint([5000, 200], { x: 0, y: 0, w: 100, h: 100 }, camera, drawing, view), /does not intersect/);
});

test('step records carry the shared pass schema and each kind its surface', () => {
  assert.deepEqual(Object.keys(stepRecord({ id: 'undo', do: 'repeat', surface: 'canvas' }, 5)),
    ['id', 'status', 'reason', 'input', 'begin_ns', 'end_ns', 'surface', 'motion', 'contacts', 'completions', 'settled_ns', 'latency', 'extra']);
  const surfaces = Object.fromEntries(chain.steps.map(step => [step.id, surfaceOf(step)]));
  assert.equal(surfaces['ui.slider'], 'window');
  assert.equal(surfaces['heavy.object'], 'canvas');
  assert.equal(surfaces.save, 'window');
  assert.equal(surfaces['ui.open'], 'window');
  assert.equal(surfaces['switch.resume'], 'canvas');
  assert.equal(surfaces['draw.pencil'], 'canvas');
});

test('a page transport timeout stops later page requests while browser cleanup and unrun rows survive', async () => {
  const failure = Error('CDP timeout: Runtime.evaluate: renderer did not answer');
  const sent = [];
  const cdp = {
    session: 'page-session',
    async call(method, _params, session) {
      sent.push([method, session]);
      if (method === 'Browser.getVersion') throw Error('CDP timeout: Browser.getVersion');
      if (method === 'Runtime.evaluate') throw failure;
    },
  };
  const pageTimeout = stopPageOnTimeout(cdp);
  const wanted = new Set(['open.vector', 'vector.move', 'close']);
  const [start, ...tail] = selectSteps(chain.steps, wanted);
  assert.equal(start.id, 'open.vector');
  await assert.rejects(cdp.call('Browser.getVersion', {}, null), /CDP timeout: Browser.getVersion/);
  assert.equal(pageTimeout(), null);
  await cdp.call('Page.enable');
  await assert.rejects(cdp.call('Runtime.evaluate'), error => error === failure);
  for (const method of ['Runtime.evaluate', 'Page.handleJavaScriptDialog', 'Page.removeScriptToEvaluateOnNewDocument'])
    await assert.rejects(cdp.call(method), error => error === failure);
  for (const method of ['Tracing.end', 'Target.disposeBrowserContext', 'Browser.close']) await cdp.call(method, {}, null);
  assert.deepEqual(sent, [['Browser.getVersion', null], ['Page.enable', 'page-session'],
    ['Runtime.evaluate', 'page-session'], ['Tracing.end', null],
    ['Target.disposeBrowserContext', null], ['Browser.close', null]]);
  const rows = tail.map(step => stepRecord(step, 10, pageTimeout().message));
  assert.deepEqual(rows.map(row => [row.id, row.status, row.reason, row.end_ns]), [
    ['vector.move', 'error', `not run: ${failure.message}`, 10], ['close', 'error', `not run: ${failure.message}`, 10],
  ]);
});

test('an idle timeout stops the pass whether or not the page error carries its stack', () => {
  assert.ok(idleTimedOut('Timed out waiting for idle before draw.pen'));
  assert.ok(idleTimedOut('Error: Timed out waiting for idle before draw.pen\n    at check (<anonymous>:12:9)'));
  assert.ok(!idleTimedOut('Error: Timed out waiting for draw.pen to open'));
  assert.ok(!idleTimedOut(null));
});

test('steps need the open document, their own document and their tabs', () => {
  assert.deepEqual(documentNeeds({ do: 'launch' }, null), []);
  assert.deepEqual(documentNeeds({ do: 'stroke' }, 'photo'), ['photo']);
  assert.deepEqual(documentNeeds({ do: 'repeat', tabs: ['photo', 'heavy'] }, 'heavy'), ['heavy', 'photo']);
  assert.deepEqual(documentNeeds({ do: 'save', document: 'heavy' }, 'heavy'), ['heavy']);
  assert.deepEqual(documentNeeds({ do: 'close' }, 'vector'), []);
  assert.deepEqual(selectSteps(chain.steps, new Set(['launch', 'nav.pan'])).map(step => step.id), ['launch', 'nav.pan']);
  assert.throws(() => selectSteps(chain.steps, new Set(['nav.spin'])), /Unknown steps: nav.spin/);
  assert.deepEqual([...new Set(chain.steps.map(step => step.do))].sort(),
    ['close', 'drag', 'gesture', 'launch', 'open', 'repeat', 'resume', 'save', 'scrub', 'stroke', 'tap']);
});

test('canvas frames are the first presented frame after each submission', () => {
  const events = traceEvents(JSON.stringify({ traceEvents: [
    { name: PRESENTED, ts: 1000 }, { name: PRESENTED, ts: 1008 }, { name: PRESENTED, ts: 1008 }, { name: PRESENTED, ts: 1016 },
    { name: CLOCK_MARK, ts: 2000, pid: 77, args: { data: { startTime: 1.5 } } }, { name: 'other', ts: 1 }] }));
  assert.deepEqual(presentedTimes(events), [1e6, 1.008e6, 1.016e6]);
  assert.deepEqual(pageClock(events), { pid: 77, offset_ns: 2e6 - 1.5e6 });
  const { evidence, rows } = frameRows([0.999e6, 1.001e6, 1.002e6, 1.02e6], presentedTimes(events));
  assert.equal(evidence, 'presented');
  assert.deepEqual(rows.filter(row => row.surface === 'canvas').map(row => row.t_ns), [1e6, 1.008e6]);
  assert.equal(rows.filter(row => row.surface === 'window').length, 3);
  assert.deepEqual(frameRows([5, 6], []), { evidence: 'submitted', rows: [{ t_ns: 5, surface: 'canvas' }, { t_ns: 6, surface: 'canvas' }] });
  assert.equal(pageClock([]), null);
});

test('completions keep updates inside the stroke that consumed newer input', () => {
  const rows = [{ complete_ns: 5, input_ns: 1 }, { complete_ns: 10, input_ns: 2 }, { complete_ns: 12, input_ns: 2 },
    { complete_ns: 14, input_ns: null }, { complete_ns: 20, input_ns: 3 }, { complete_ns: 40, input_ns: 4 }];
  assert.deepEqual(completions(rows, 8, 30), [{ complete_ns: 10, input_ns: 2 }, { complete_ns: 20, input_ns: 3 }]);
});

test('memory rows add the page renderer to the GPU process', () => {
  const samples = [{ t_ns: 1, os_available_kib: 9, renderers: { 5: 100, 6: 50 }, gpu_process_rss_kib: 30, gpu_kib: 2048 }];
  assert.deepEqual(memoryRows(samples, 5), [{ t_ns: 1, os_available_kib: 9, renderer_rss_kib: 100, gpu_process_rss_kib: 30, gpu_kib: 2048, app_rss_kib: 130 }]);
  assert.deepEqual(memoryRows(samples, null), [{ t_ns: 1, os_available_kib: 9, gpu_process_rss_kib: 30, gpu_kib: 2048 }]);
});
