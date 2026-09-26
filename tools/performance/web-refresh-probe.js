// Injected by web-refresh.mjs before page code. Observation only unless probes
// are explicitly enabled; all times are milliseconds since navigation.
export function installRefreshProbe({ duration, probes, uiOnly, skipCatalog, quietCompiler, earlyRecovery }) {
  const trace = window.refreshTrace = {
    calls: [], gpu: [], workers: [], longtasks: [], frames: [], inputs: [],
    states: [], dialogs: [], probes: [], errors: [],
  };
  const cleanup = [];
  trace.dispose = () => { while (cleanup.length) cleanup.pop()(); };
  const now = () => performance.now();
  const active = () => now() < duration;
  if (uiOnly) {
    const descriptor = Object.getOwnPropertyDescriptor(navigator, 'gpu');
    Object.defineProperty(navigator, 'gpu', { value: undefined, configurable: true });
    cleanup.push(() => { if (descriptor) Object.defineProperty(navigator, 'gpu', descriptor); else delete navigator.gpu; });
  }
  const observer = new PerformanceObserver(list => {
    if (active()) trace.longtasks.push(...list.getEntries().map(e => ({ start: e.startTime, duration: e.duration })));
  });
  observer.observe({ type: 'longtask', buffered: true });
  cleanup.push(() => observer.disconnect());
  let previousFrame, frameRequest;
  function frame(time) {
    if (!active()) return;
    if (previousFrame !== undefined) trace.frames.push({ start: time, interval: time - previousFrame });
    previousFrame = time;
    frameRequest = requestAnimationFrame(frame);
  }
  frameRequest = requestAnimationFrame(frame);
  cleanup.push(() => cancelAnimationFrame(frameRequest));
  function listen(target, type, listener, options) {
    target.addEventListener(type, listener, options);
    cleanup.push(() => target.removeEventListener(type, listener, options));
  }
  function wrap(object, name, output, describe = () => ({})) {
    const original = object?.[name];
    if (typeof original !== 'function') return;
    cleanup.push(() => { object[name] = original; });
    object[name] = function (...args) {
      if (!active()) return original.apply(this, args);
      const row = { name, start: now(), ...describe(args) };
      output.push(row);
      try {
        const result = original.apply(this, args);
        row.sync = now() - row.start;
        if (result instanceof Promise) result.then(
          () => { row.end = now(); },
          error => { row.end = now(); row.error = String(error); },
        );
        else row.end = now();
        return result;
      } catch (error) { row.sync = now() - row.start; row.error = String(error); throw error; }
    };
  }
  for (const name of ['createShaderModule', 'createRenderPipeline', 'createComputePipeline', 'createRenderPipelineAsync', 'createComputePipelineAsync']) {
    wrap(window.GPUDevice?.prototype, name, trace.gpu, ([d]) => ({ label: d.label, bytes: d.code?.length }));
  }
  wrap(window.GPU?.prototype, 'requestAdapter', trace.gpu);
  wrap(window.GPUAdapter?.prototype, 'requestDevice', trace.gpu);
  wrap(window.GPUQueue?.prototype, 'onSubmittedWorkDone', trace.gpu);
  const warn = console.warn;
  console.warn = (...args) => { if (active()) trace.errors.push(args.map(String).join(' ')); warn.apply(console, args); };
  cleanup.push(() => { console.warn = warn; });
  const workerRequests = new WeakMap();
  const post = Worker.prototype.postMessage;
  Worker.prototype.postMessage = function (message, ...rest) {
    const operation = typeof message?.request === 'string' ? `workspace:${JSON.parse(message.request).type}` : message?.request?.operation;
    if (active() && operation) {
      if (!workerRequests.has(this)) {
        const requests = new Map(); workerRequests.set(this, requests);
        listen(this, 'message', ({ data }) => {
          const row = requests.get(data.id);
          if (row) { row.end = now(); row.error = data.error; requests.delete(data.id); }
        });
      }
      const row = { operation, start: now() };
      workerRequests.get(this).set(message.id, row); trace.workers.push(row);
    }
    return post.call(this, message, ...rest);
  };
  cleanup.push(() => { Worker.prototype.postMessage = post; });
  const show = HTMLDialogElement.prototype.showModal;
  HTMLDialogElement.prototype.showModal = function () {
    if (active()) trace.dialogs.push({ start: now(), title: this.querySelector('h1,h2')?.textContent || this.getAttribute('aria-label') || this.id });
    return show.call(this);
  };
  cleanup.push(() => { HTMLDialogElement.prototype.showModal = show; });
  const contacts = new Set(); let quietAt = 0;
  for (const type of ['pointerdown', 'pointerup', 'pointercancel', 'pointermove', 'keydown', 'wheel']) {
    listen(window, type, e => {
      if (type === 'pointerdown') contacts.add(e.pointerId);
      if (type === 'pointerup' || type === 'pointercancel') contacts.delete(e.pointerId);
      if (type !== 'pointermove' || e.buttons) quietAt = now() + 500;
      if (active() && type !== 'pointermove') trace.inputs.push({ type, start: now(), eventTime: e.timeStamp, target: e.target.id || e.target.tagName, pointer: e.pointerType });
    }, { capture: true, passive: true });
  }
  listen(window, 'blur', () => { contacts.clear(); quietAt = now() + 500; });
  listen(window, 'error', e => trace.errors.push(String(e.error || e.message)));
  listen(window, 'unhandledrejection', e => trace.errors.push(String(e.reason)));
  let attached = false;
  function attach() {
    if (attached || !window.layerApp) return;
    attached = true;
    // Experimental ablations, never production fixes. Skipping the catalog can
    // shift compilation to the first effect/preview use; measure that separately.
    if (skipCatalog) {
      const original = layerApp.app.load_filter_library;
      cleanup.push(() => { layerApp.app.load_filter_library = original; });
      layerApp.app.load_filter_library = () => ({ regions: 0, canvas_wake: false });
    }
    if (quietCompiler) {
      const compile = layerApp.app.compile_startup_step.bind(layerApp.app);
      cleanup.push(() => { layerApp.app.compile_startup_step = compile; });
      layerApp.app.compile_startup_step = async () => {
        while (active() && layerApp.app.brush_ready() && (contacts.size || now() < quietAt || document.querySelector('dialog[open],details[open]'))) {
          await new Promise(resolve => setTimeout(resolve, 50));
        }
        return compile();
      };
    }
    for (const name of ['frame', 'compile_startup_step', 'load_filter_library', 'attach_gpu', 'prepare_document', 'adopt_document', 'capture_tab_recovery', 'workspace_tick']) {
      wrap(layerApp.app, name, trace.calls);
    }
    for (const name of ['startRecovery', 'autosave']) wrap(layerApp.documents, name, trace.calls);
    if (earlyRecovery) {
      Promise.resolve().then(() => layerApp.documents.startRecovery())
        .catch(error => trace.errors.push(`Early recovery experiment: ${error}`));
    }
  }
  const mark = performance.mark.bind(performance);
  performance.mark = (...args) => { if (args[0] === 'capy.startup.ui') attach(); return mark(...args); };
  cleanup.push(() => { performance.mark = mark; });
  let previousState;
  const poll = setInterval(() => {
    if (!active()) { clearInterval(poll); return; }
    attach();
    if (!attached) return;
    const state = layerApp.state(), workspace = JSON.parse(layerApp.app.workspace_view());
    const sample = {
      workspace: workspace?.ready, workspaceBusy: workspace?.busy,
      gpu: layerApp.app.gpu_ready(), brush: layerApp.app.brush_ready(),
      complete: layerApp.startupTimes.complete != null, filters: state.filter_load.pending,
      park: layerApp.app.document_park_ready(), documentBusy: state.document_file.busy,
      commands: Object.fromEntries(state.commands.filter(c => ['settings','new_document','open_document','export_document','document_properties'].includes(c.id)).map(c => [c.id, c.enabled])),
    };
    const key = JSON.stringify(sample);
    if (key !== previousState) { previousState = key; trace.states.push({ start: now(), ...sample }); }
  }, 250);
  cleanup.push(() => clearInterval(poll));
  // Deliberately separate passive runs from UI probes: opening Settings pauses
  // compilation and must never be reported as an undisturbed startup timing.
  if (probes) {
    let due = now() + 500, busy = false;
    const probe = setInterval(async () => {
      if (!active()) { clearInterval(probe); return; }
      const late = Math.max(0, now() - due); due = now() + 500;
      if (busy || !attached || document.querySelector('dialog[open]')) return;
      const summary = [...document.querySelectorAll('#header details > summary')].find(n => n.getBoundingClientRect().width > 0);
      if (!summary) return;
      busy = true;
      const row = { start: now(), late, complete: layerApp.startupTimes.complete != null };
      trace.probes.push(row);
      try {
        summary.click(); row.sync = now() - row.start;
        await new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r)));
        row.present = now() - row.start;
        row.open = summary.parentElement.open;
        row.items = summary.parentElement.querySelectorAll('button').length;
        summary.parentElement.open = false;
      } catch (error) { row.error = String(error); }
      finally { busy = false; }
    }, 500);
    cleanup.push(() => clearInterval(probe));
  }
}
