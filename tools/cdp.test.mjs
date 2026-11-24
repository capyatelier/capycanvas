import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {once} from 'node:events';
import {mkdtemp, readdir, readFile, realpath, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {connectBrowser, launchChrome, prepareWorkers} from './cdp.mjs';

test('worker observers precede module execution and close rejects event waiters', async t => {
  const server = createServer((request, response) => {
    response.setHeader('Content-Type', request.url === '/worker.js' ? 'text/javascript' : 'text/html');
    response.end(request.url === '/worker.js'
      ? "globalThis.__observedAtStart=[...(globalThis.__observers??[])];globalThis.__workerStarted(JSON.stringify(__observedAtStart));"
      : '<!doctype html><title>Worker preparation</title>');
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  let cdp, first, second;
  t.after(async () => {
    try {
      await Promise.allSettled([first?.close(), second?.close()]);
      if (cdp) await cdp.close();
    } finally {
      server.closeAllConnections();
      await new Promise(resolve => server.close(resolve));
    }
  });
  cdp = await launchChrome(['--headless=new'], {timeout: 180000});
  first = prepareWorkers(cdp, async session => {
    await cdp.call('Runtime.addBinding', {name: '__workerStarted'}, session);
    const value = await cdp.call('Runtime.evaluate', {
      expression: "(globalThis.__observers??=[]).push('first')",
    }, session);
    assert.equal(value.exceptionDetails, undefined);
  });
  second = prepareWorkers(cdp, async session => {
    const value = await cdp.call('Runtime.evaluate', {
      expression: "(globalThis.__observers??=[]).push('second')",
    }, session);
    assert.equal(value.exceptionDetails, undefined);
  });
  await cdp.attachPage();
  await cdp.call('Page.enable');
  const loaded = cdp.once('Page.loadEventFired', 180000);
  const url = `http://127.0.0.1:${server.address().port}`;
  await cdp.call('Page.navigate', {url});
  await loaded;
  await Promise.all([first.start(), second.start()]);
  const started = cdp.once('Runtime.bindingCalled', 180000);
  await cdp.call('Runtime.evaluate', {
    expression: `globalThis.worker=new Worker(${JSON.stringify(url + '/worker.js')},{type:'module'});true`,
  });
  const observed = await started;
  assert.equal(observed.name, '__workerStarted');
  assert.deepEqual(JSON.parse(observed.payload).sort(), ['first', 'second']);
  await Promise.all([first.check(), second.check()]);
  await first.close();first = undefined;
  await second.close();second = undefined;
  const waiter = cdp.once('CapyNeverEmittedEvent', 180000);
  const rejected = assert.rejects(waiter, /Chrome exited|CDP closed/);
  await cdp.close();cdp = undefined;
  await rejected;
});

test('a caller-owned profile outlives the browser', async () => {
  const profile = await mkdtemp(join(await realpath(tmpdir()), 'capy-profile-test-'));
  try {
    const cdp = await launchChrome(['--headless=new', '--disable-gpu'], {profile, timeout: 180000});
    await cdp.call('Browser.getVersion', {}, null);
    await cdp.close();
    assert.ok((await readdir(profile)).includes('Local State'));
  } finally { await rm(profile, {recursive: true, force: true}); }
});

test('a browser connection opens pages in an isolated context', async () => {
  const profile = await mkdtemp(join(await realpath(tmpdir()), 'capy-profile-test-'));
  const chrome = await launchChrome(['--headless=new', '--disable-gpu', '--remote-debugging-port=0'], {profile, timeout: 180000});
  let browser;
  try {
    let port;
    for (const deadline = Date.now() + 30000; !port && Date.now() < deadline; await new Promise(resolve => setTimeout(resolve, 100)))
      port = (await readFile(join(profile, 'DevToolsActivePort'), 'utf8').catch(() => '')).split('\n')[0];
    browser = await connectBrowser(`http://127.0.0.1:${port}`, {timeout: 180000});
    const {browserContextId} = await browser.call('Target.createBrowserContext', {}, null);
    const targetId = await browser.attachPage(browserContextId);
    assert.equal((await browser.call('Target.getTargetInfo', {targetId}, null)).targetInfo.browserContextId, browserContextId);
    assert.equal(await browser.evaluate('location.href'), 'about:blank');
    await browser.call('Target.disposeBrowserContext', {browserContextId}, null);
  } finally {
    await browser?.close();
    await chrome.close();
    await rm(profile, {recursive: true, force: true});
  }
});
