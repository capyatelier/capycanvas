import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import test from "node:test";

const source = readFileSync(new URL("app.js", import.meta.url), "utf8");
function harness() {
  const calls = [];
  let current = 0;
  const context = {
    scheduled: false,
    state: { settings_open: false },
    startupTimes: { complete: 1 },
    pending: [],
    performance: { now: () => current },
    app: { frame(...args) { calls.push(args); return {}; } },
    flushWorkspacePresentation() {},
    applyChange() {},
    refreshStartup() {},
    scheduleCompiler() {},
    wake() {},
    stopGpu(error) { throw error; },
  };
  runInNewContext(source.slice(
    source.indexOf("const frameIntervals = []"),
    source.indexOf("function refreshStartup()"),
  ), context);
  return {
    tick(time, now = time + 2) {
      current = now;
      context.frame(time);
      return calls.at(-1);
    },
  };
}

test("prediction uses current input time and follows a 90 Hz display despite missed frames", () => {
  const h = harness(), interval = 1000 / 90;
  for (let i = 0; i < 40; i++) h.tick(i * interval);
  const [now, presentation] = h.tick(42 * interval, 42 * interval + 5);
  assert.equal(now, 42 * interval + 5);
  assert.ok(Math.abs(presentation - 43 * interval) < .01);
  const late = h.tick(43 * interval, 45 * interval + 1);
  assert.equal(late[0], 45 * interval + 1);
  assert.ok(late[1] > late[0] && late[1] - late[0] <= interval + .01);
});

test("display estimate adapts after moving to a 60 Hz screen and excludes idle gaps", () => {
  const h = harness();
  for (let i = 0; i < 40; i++) h.tick(i * 1000 / 120);
  for (let i = 0; i < 40; i++) h.tick(1000 + i * 1000 / 60);
  const [, presentation] = h.tick(1000 + 40 * 1000 / 60);
  assert.ok(Math.abs(presentation - (1000 + 41 * 1000 / 60)) < .01);
  const idle = h.tick(10000, 10002);
  assert.ok(idle[1] > 10002 && idle[1] < 10020);
});
