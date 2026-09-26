import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {mkdtemp, realpath, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';

function client(send, {timeout = 30000, onEvent = () => {}} = {}) {
  const pending = new Map(), waiters = new Set(), errors = [];
  let sequence = 0;
  const cdp = {errors, session: undefined, detail: () => ''};
  cdp.report = message => {
    errors.push(message);
    if (process.env.LAYER_TEST_VERBOSE) console.error(message);
  };
  cdp.receive = message => {
    if (message.id) {
      const job = pending.get(message.id);
      if (!job) return;
      pending.delete(message.id); clearTimeout(job.timer);
      message.error ? job.reject(Error(`${job.method}: ${JSON.stringify(message.error)}`)) : job.resolve(message.result);
      return;
    }
    if (message.method === 'Runtime.exceptionThrown') {
      const details = message.params.exceptionDetails;
      cdp.report(details.exception?.description || details.exception?.value || details.text);
    } else if (message.method === 'Page.javascriptDialogOpening' && message.params.type === 'beforeunload')
      cdp.call('Page.handleJavaScriptDialog', {accept: true}).catch(() => {});
    for (const waiter of waiters) if (waiter.method === message.method) waiter.resolve(message.params);
    onEvent(message);
  };
  cdp.call = (method, params = {}, sessionId = cdp.session) => new Promise((resolve, reject) => {
    const id = ++sequence;
    const timer = setTimeout(() => { pending.delete(id); reject(Error(`CDP timeout: ${method}${cdp.detail()}`)); },
      typeof timeout === 'function' ? timeout(method) : timeout);
    pending.set(id, {resolve, reject, timer, method});
    try { send({id, method, params, ...(sessionId ? {sessionId} : {})}); }
    catch (error) { pending.delete(id); clearTimeout(timer); reject(error); }
  });
  cdp.once = (method, limit = 60000) => {
    const waiter = {method};
    const event = new Promise((resolve, reject) => {
      const timer = setTimeout(() => { waiters.delete(waiter); reject(Error(`Timed out waiting for ${method}`)); }, limit);
      waiter.resolve = params => { waiters.delete(waiter); clearTimeout(timer); resolve(params); };
    });
    waiters.add(waiter);
    event.catch(() => {});
    return event;
  };
  cdp.evaluate = async expression => {
    const result = await cdp.call('Runtime.evaluate', {expression, awaitPromise: true, returnByValue: true});
    const details = result.exceptionDetails;
    if (details) throw Error(details.exception?.description || details.exception?.value || details.text);
    return result.result.value;
  };
  cdp.settle = () => cdp.evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))');
  cdp.fail = reason => {
    for (const job of pending.values()) { clearTimeout(job.timer); job.reject(Error(reason)); }
    pending.clear();
  };
  return cdp;
}

export async function connectTab(endpoint, match, options) {
  let tab;
  for (const deadline = Date.now() + 10000; !tab && Date.now() < deadline;) {
    tab = (await (await fetch(`${endpoint}/json/list`, {signal: AbortSignal.timeout(10000)})).json()).find(match);
    if (!tab) await new Promise(resolve => setTimeout(resolve, 100));
  }
  if (!tab?.webSocketDebuggerUrl) throw Error(`Open the dedicated test tab at ${endpoint} first`);
  const socket = new WebSocket(tab.webSocketDebuggerUrl);
  const cdp = client(message => {
    if (socket.readyState !== WebSocket.OPEN) throw Error('CDP disconnected');
    socket.send(JSON.stringify(message));
  }, options);
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(Error('Chrome connection timed out')), 10000);
    socket.onopen = () => { clearTimeout(timer); resolve(); };
    socket.onerror = error => { clearTimeout(timer); reject(error); };
  });
  socket.onmessage = event => cdp.receive(JSON.parse(event.data));
  socket.onclose = () => cdp.fail('CDP disconnected');
  cdp.close = async () => { socket.close(); cdp.fail('CDP closed'); };
  return cdp;
}

export async function launchChrome(args, {executable = process.env.CHROME || 'google-chrome', ...options} = {}) {
  const root = await realpath(tmpdir());
  const profile = await mkdtemp(join(root, 'capy-chrome-'));
  const chrome = spawn(executable, ['--remote-debugging-pipe', `--user-data-dir=${profile}`, '--no-first-run',
    '--no-default-browser-check', '--password-store=basic', ...args, 'about:blank'],
    {stdio: ['ignore', 'ignore', 'pipe', 'pipe', 'pipe']});
  const cdp = client(message => chrome.stdio[3].write(JSON.stringify(message) + '\0'), options);
  let stderr = '', buffer = '';
  cdp.detail = () => stderr && `: ${stderr.slice(-2000)}`;
  chrome.stderr.on('data', data => {
    stderr = (stderr + data).slice(-4000);
    if (process.env.LAYER_TEST_VERBOSE) process.stderr.write(data);
  });
  chrome.stdio[4].on('data', data => {
    for (buffer += data; buffer.includes('\0'); buffer = buffer.slice(buffer.indexOf('\0') + 1))
      cdp.receive(JSON.parse(buffer.slice(0, buffer.indexOf('\0'))));
  });
  chrome.on('exit', () => cdp.fail('Chrome exited'));
  cdp.attachPage = async () => {
    const {targetId} = await cdp.call('Target.createTarget', {url: 'about:blank'}, null);
    cdp.session = (await cdp.call('Target.attachToTarget', {targetId, flatten: true}, null)).sessionId;
    return targetId;
  };
  cdp.close = async () => {
    if (chrome.exitCode === null && chrome.signalCode === null) {
      const exited = once(chrome, 'exit');
      await cdp.call('Browser.close', {}, null).catch(() => chrome.kill());
      await exited;
    }
    if (await realpath(profile).catch(() => null) === profile)
      await rm(profile, {recursive: true, force: true, maxRetries: 8, retryDelay: 250});
  };
  return cdp;
}
