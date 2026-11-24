import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {mkdtemp, realpath, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';

function client(send, {timeout = 30000, onEvent = () => {}} = {}) {
  const pending = new Map(), waiters = new Set(), errors = [];
  let sequence = 0;
  const cdp = {errors, session: undefined, detail: () => '', timeout: method => typeof timeout === 'function' ? timeout(method) : timeout};
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
    if (message.method === 'Target.detachedFromTarget')
      for (const waiter of waiters) if (waiter.session === message.params.sessionId) waiter.reject(Error('Worker detached'));
    for (const waiter of waiters) if (waiter.method === message.method && (waiter.session === undefined || waiter.session === message.sessionId)) waiter.resolve(message.params);
    onEvent(message);
  };
  cdp.call = (method, params = {}, sessionId = cdp.session) => new Promise((resolve, reject) => {
    const id = ++sequence;
    const timer = setTimeout(() => { pending.delete(id); reject(Error(`CDP timeout: ${method}${cdp.detail()}`)); },
      cdp.timeout(method));
    pending.set(id, {resolve, reject, timer, method});
    try { send({id, method, params, ...(sessionId ? {sessionId} : {})}); }
    catch (error) { pending.delete(id); clearTimeout(timer); reject(error); }
  });
  cdp.once = (method, limit = 60000, session) => {
    const waiter = {method, session};
    const event = new Promise((resolve, reject) => {
      const timer = setTimeout(() => { waiters.delete(waiter); reject(Error(`Timed out waiting for ${method}`)); }, limit);
      waiter.resolve = params => { waiters.delete(waiter); clearTimeout(timer); resolve(params); };
      waiter.reject = error => { waiters.delete(waiter); clearTimeout(timer); reject(error); };
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
  cdp.attachPage = async browserContextId => {
    const {targetId} = await cdp.call('Target.createTarget', {url: 'about:blank', ...(browserContextId ? {browserContextId} : {})}, null);
    cdp.session = (await cdp.call('Target.attachToTarget', {targetId, flatten: true}, null)).sessionId;
    return targetId;
  };
  cdp.fail = reason => {
    for (const job of pending.values()) { clearTimeout(job.timer); job.reject(Error(reason)); }
    pending.clear();
    for (const waiter of [...waiters]) waiter.reject(Error(reason));
    waiters.clear();
  };
  return cdp;
}

const workerPreparations = new WeakMap();
export function prepareWorkers(cdp, install) {
  let state = workerPreparations.get(cdp);
  if (!state) {
    state = {installers: new Set(), pending: new Set(), detached: new Set(), failures: [], started: false, starting: null};
    workerPreparations.set(cdp, state);
    const receive = cdp.receive;
    cdp.receive = message => {
      receive(message);
      if (message.method === 'Target.detachedFromTarget') state.detached.add(message.params.sessionId);
      if (message.method !== 'Target.attachedToTarget' || message.params.targetInfo.type !== 'worker' || !state.installers.size) return;
      const attached = message.params, session = attached.sessionId;
      const task = (async () => {
        let breakpoint, runIfSent = false, runIfCompleted = false;
        try {
          await cdp.call('Runtime.enable', {}, session);
          if (!attached.waitingForDebugger) {
            for (const installer of state.installers) await installer(session, attached);
            return;
          }
          await cdp.call('Debugger.enable', {}, session);
          breakpoint = (await cdp.call('Debugger.setInstrumentationBreakpoint', {instrumentation: 'beforeScriptExecution'}, session)).breakpointId;
          const paused = cdp.once('Debugger.paused', cdp.timeout('Runtime.evaluate'), session);
          runIfSent = true;
          await cdp.call('Runtime.runIfWaitingForDebugger', {}, session);
          runIfCompleted = true;
          const boundary = await paused;
          if (boundary.reason !== 'instrumentation') throw Error('Unexpected worker pause: ' + boundary.reason);
          for (const installer of state.installers) await installer(session, attached);
        } catch (error) {
          if (!state.detached.has(session)) state.failures.push(error);
        } finally {
          if (!state.detached.has(session) && attached.waitingForDebugger) {
            if (!runIfSent) {
              runIfSent = true;
              try { await cdp.call('Runtime.runIfWaitingForDebugger', {}, session); runIfCompleted = true; }
              catch (error) { if (!state.detached.has(session)) state.failures.push(error); }
            }
            for (const [method, params] of [
              ['Debugger.removeBreakpoint', {breakpointId: breakpoint}],
              ['Debugger.resume', {}], ['Debugger.disable', {}],
            ]) {
              if (method === 'Debugger.removeBreakpoint' && !breakpoint) continue;
              if (method === 'Debugger.resume' && !runIfCompleted) continue;
              try { await cdp.call(method, params, session); }
              catch (error) { if (!state.detached.has(session)) state.failures.push(error); }
            }
          }
        }
      })();
      state.pending.add(task);task.finally(() => state.pending.delete(task));
    };
  }
  state.installers.add(install);
  const check = async () => {
    while (state.pending.size) await Promise.all([...state.pending]);
    if (state.failures.length) throw new AggregateError(state.failures, 'Worker preparation failed');
  };
  return {
    start() {
      if (!state.starting) state.starting = cdp.call('Target.setAutoAttach', {autoAttach: true, waitForDebuggerOnStart: true, flatten: true, filter: [{type: 'worker'}, {exclude: true}]}).then(() => { state.started = true; });
      return state.starting;
    },
    check,
    async close() {
      try { await check(); }
      finally {
        state.installers.delete(install);
        if (!state.installers.size && state.started) {
          await cdp.call('Target.setAutoAttach', {autoAttach: false, waitForDebuggerOnStart: false, flatten: true});
          state.started = false;
          state.starting = null;
        }
      }
    },
  };
}

async function connectSocket(url, options) {
  const socket = new WebSocket(url);
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

export async function connectTab(endpoint, match, options) {
  let tab;
  for (const deadline = Date.now() + 10000; !tab && Date.now() < deadline;) {
    tab = (await (await fetch(`${endpoint}/json/list`, {signal: AbortSignal.timeout(10000)})).json()).find(match);
    if (!tab) await new Promise(resolve => setTimeout(resolve, 100));
  }
  if (!tab?.webSocketDebuggerUrl) throw Error(`Open the dedicated test tab at ${endpoint} first`);
  return connectSocket(tab.webSocketDebuggerUrl, options);
}

export async function connectBrowser(endpoint, options) {
  const {webSocketDebuggerUrl} = await (await fetch(`${endpoint}/json/version`, {signal: AbortSignal.timeout(10000)})).json();
  if (!webSocketDebuggerUrl) throw Error(`${endpoint} exposes no browser target`);
  return connectSocket(webSocketDebuggerUrl, options);
}

export async function launchChrome(args, {executable = process.env.CHROME || 'google-chrome', profile: kept, ...options} = {}) {
  const profile = kept ?? await mkdtemp(join(await realpath(tmpdir()), 'capy-chrome-'));
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
  cdp.close = async () => {
    if (chrome.exitCode === null && chrome.signalCode === null) {
      const exited = once(chrome, 'exit');
      await cdp.call('Browser.close', {}, null).catch(() => chrome.kill());
      await exited;
    }
    for (const stream of chrome.stdio) stream?.destroy();
    if (!kept && await realpath(profile).catch(() => null) === profile)
      await rm(profile, {recursive: true, force: true, maxRetries: 8, retryDelay: 250});
  };
  return cdp;
}
