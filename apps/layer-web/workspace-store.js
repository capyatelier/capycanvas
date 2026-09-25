// IndexedDB transport only. The synchronous Wasm reducer owns validation,
// fencing, counters, receipts, names, migration and retention policy.
export const ownerLock = "capy-workspace-owner:";
export function createWorkspaceStore(reduce, { name = "capycanvas.workspaces", indexedDB = globalThis.indexedDB, locks = globalThis.navigator?.locks } = {}) {
  let database, opening;
  async function liveOwners() {
    if (!locks) return [null, 0];
    const at = Date.now(), { held = [], pending = [] } = await locks.query();
    return [[...held, ...pending].flatMap(({ name }) => name?.startsWith(ownerLock) ? [name.slice(ownerLock.length)] : []), at];
  }
  const error = e => JSON.stringify({ kind: e?.name === "QuotaExceededError" ? "storage_full" :
    e?.name === "VersionError" ? "unsupported_schema" : "unavailable", message: e?.message || String(e) });
  function open() {
    if (database) return Promise.resolve(database);
    if (opening) return opening;
    opening = new Promise((resolve, reject) => {
      let request;
      try { request = indexedDB.open(name, 1); } catch (e) { reject(error(e)); return; }
      request.onupgradeneeded = () => { request.result.createObjectStore("workspace", { keyPath: "id" }); };
      request.onblocked = () => reject(error(new Error("Close older CapyCanvas tabs to finish opening workspace storage.")));
      request.onerror = () => reject(error(request.error));
      request.onsuccess = () => {
        const connection = request.result; database = connection;
        connection.onversionchange = () => { connection.close(); if (database === connection) database = null; };
        connection.onclose = () => { if (database === connection) database = null; };
        resolve(connection);
      };
    }).finally(() => { opening = null; });
    return opening;
  }
  const readOnly = new Set(["list", "load", "raw", "receipt", "binding", "legacy_import", "pending", "reopen", "switcher", "workspace_order"]);
  async function transaction(request, pending = false) {
    const db = await open(), command = JSON.parse(request), [live, liveAt] = await liveOwners();
    return new Promise((resolve, reject) => {
      let tx, response, failure;
      const readonly = !pending && (readOnly.has(command.type) || command.type === "maintenance" && !command.apply);
      try {
        tx = db.transaction("workspace", readonly ? "readonly" : "readwrite", { durability: "strict" });
        const store = tx.objectStore("workspace"), read = store.get("database");
        read.onsuccess = () => {
          try {
            const result = reduce(read.result?.snapshot, request, pending, Date.now(), live, liveAt);
            response = result.response;
            if (!readonly) store.put({ id: "database", snapshot: result.snapshot });
          } catch (e) { failure = typeof e === "string" ? e : error(e); tx.abort(); }
        };
        tx.oncomplete = () => resolve(response);
        tx.onabort = () => reject(failure || error(tx.error || new Error("The workspace transaction was interrupted. Your changes have been kept.")));
        tx.onerror = () => {}; // onabort reports the complete transaction's result.
      } catch (e) { reject(error(e)); }
    });
  }
  return {
    async execute(request) {
      if (JSON.parse(request).type === "commit") await transaction(request, true);
      return transaction(request);
    },
    close() { database?.close(); database = null; },
  };
}

// Keep validation/serialization of retained history off the editor thread too.
// The worker uses the same Wasm policy and the same IndexedDB adapter as tests.
export function createWorkspaceClient(url, { onSettled = () => {}, module, preload = false } = {}) {
  let worker, sequence = 0, failed;
  const pending = new Map();
  function start() {
    worker = new Worker(url, {type:"module", name:"workspace-storage"}); failed = null;
    worker.onmessage = ({data}) => { const p = pending.get(data.id); if (!p) return; pending.delete(data.id); data.error ? p.reject(data.error) : p.resolve(data.response); };
    if (module) worker.postMessage({module});
    worker.onerror = e => {
      failed = JSON.stringify({kind:"unavailable",message:e.message || "Workspace storage stopped. Retry to reconnect."});
      for (const p of pending.values()) p.reject(failed); pending.clear(); worker.terminate(); worker = null;
    };
  }
  // Overlap worker instantiation with construction of the editor controls.
  if (module || preload) start();
  return {
    initialize(compiledModule) { module = compiledModule; worker?.postMessage({module}); },
    execute(request) {
      return new Promise((resolve,reject) => {
        try {
          if (failed && JSON.parse(request).type !== "reopen") { reject(failed); return; }
          if (!worker) start();
          const id = ++sequence; pending.set(id,{resolve,reject}); worker.postMessage({id,request});
        } catch(e) { reject(JSON.stringify({kind:"unavailable",message:String(e)})); }
      }).finally(onSettled);
    },
  };
}
