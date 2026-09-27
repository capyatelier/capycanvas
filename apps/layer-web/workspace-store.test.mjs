import assert from "node:assert/strict";
import {readFile} from "node:fs/promises";

// Rust generates the same operation stream against SQLite and BrowserDatabase.
// Run it again here through actual IndexedDB transaction completion callbacks.
export async function checkWorkspaceStore({evaluate}) {
  const fixtures = JSON.parse(await readFile(process.env.CAPY_STORE_CONTRACT_FIXTURE || "/tmp/capy-workspace-store-contract.json", "utf8"));
  const result = await evaluate(`(async () => {
    const resources = performance.getEntriesByType('resource').map(r => r.name);
    const glue = await import(resources.find(n => /layer_web\\.[a-f0-9]+\\.js$/.test(n)) || './pkg/layer_web.js');
    const {createWorkspaceStore} = await import(resources.find(n => /workspace-store\\.[a-f0-9]+\\.js$/.test(n)) || './workspace-store.js');
    const name = 'capycanvas.contract.' + crypto.randomUUID();
    let now = 1000;
    const reduce = (snapshot, request, pending) => glue.workspace_database(snapshot, request, pending, now);
    const store = createWorkspaceStore(reduce, {name});
    const normalize = reply => {
      if (reply.type === 'list') reply.value.sort((a,b) => a.id.localeCompare(b.id));
      if (reply.type === 'pending') reply.value.sort((a,b) => a.operation_id.localeCompare(b.operation_id));
      return {ok:reply};
    };
    const results = [];
    try {
      for (const fixture of ${JSON.stringify(fixtures)}) {
        now = fixture.now;
        try { results.push(normalize(JSON.parse(await store.execute(JSON.stringify(fixture.request))))); }
        catch (e) { results.push({error:JSON.parse(e).kind}); }
      }
      // A cached read must still reject an invalid pending-delivery request.
      glue.workspace_database(undefined, '{"type":"list"}', false, now);
      let invalidPending;
      try { glue.workspace_database(undefined, '{"type":"list"}', true, now); }
      catch(e) { invalidPending=JSON.parse(e).kind; }
      // Successful individual requests do not acknowledge a transaction that
      // subsequently aborts. The previous database bytes must remain intact.
      let abortNext = true;
      const aborted = createWorkspaceStore((...args) => {
        const result = reduce(...args);
        if (abortNext && JSON.parse(args[1]).type === 'release') { abortNext = false; throw JSON.stringify({kind:'failed_write',message:'Injected abort after validation'}); }
        return result;
      }, {name});
      const first = ${JSON.stringify(fixtures[0].request.batch)};
      const list = await store.execute('{"type":"list"}');
      let abortError;
      try { await aborted.execute(JSON.stringify({type:'release', id:first.writes[0].id, owner:first.owner, fence:'1'})); }
      catch (e) { abortError = JSON.parse(e).kind; }
      const unchanged = list === await store.execute('{"type":"list"}');
      aborted.close();
      // Version upgrades close existing connections; a newer database must not
      // be recreated or silently replaced by this adapter.
      store.close();
      await new Promise((resolve,reject) => { const r=indexedDB.open(name,2); r.onsuccess=()=>{r.result.close();resolve();};r.onerror=()=>reject(r.error); });
      let upgradeError;
      try { await store.execute('{"type":"list"}'); } catch(e) { upgradeError = JSON.parse(e).kind; }
      const unavailable = createWorkspaceStore(reduce, {indexedDB:{open(){throw new DOMException('Storage disabled','SecurityError');}}});
      let unavailableError; try { await unavailable.execute('{"type":"list"}'); } catch(e) { unavailableError=JSON.parse(e).kind; }
      const quota = createWorkspaceStore(reduce, {indexedDB:{open(){throw new DOMException('Storage is full','QuotaExceededError');}}});
      let quotaError; try { await quota.execute('{"type":"list"}'); } catch(e) { quotaError=JSON.parse(e).kind; }
      return {results, invalidPending, abortError, unchanged, upgradeError, unavailableError, quotaError};
    } finally { store.close(); indexedDB.deleteDatabase(name); }
  })().catch(e=>{throw new Error(typeof e==='string'?e:(e?.stack||String(e)));})`);
  // Native serde_json::Value expands f32 to f64; direct Wasm JSON uses the
  // shortest f32 spelling. Compare the typed float values, retaining exact
  // string counters/fences and integer identities.
  const floats = value => JSON.parse(JSON.stringify(value, (_, v) => typeof v === "number" && !Number.isInteger(v) ? Math.fround(v) : v));
  assert.deepEqual(floats(result.results), floats(fixtures.map(f=>f.expected)));
  assert.equal(result.invalidPending, "invalid_data");
  assert.equal(result.abortError, "failed_write"); assert.ok(result.unchanged);
  assert.equal(result.upgradeError, "unsupported_schema");
  assert.equal(result.unavailableError, "unavailable"); assert.equal(result.quotaError, "storage_full");
  console.log(`PASS: ${fixtures.length} IndexedDB/SQLite contract cases, aborted transaction, newer database, unavailable storage and quota errors`);
}

export async function checkStaleWorkspaceStartup({evaluate, reload}) {
  const wait = async predicate => {
    for (let i = 0; i < 300; i++) { try { if (await evaluate(predicate)) return; } catch {} await new Promise(r => setTimeout(r, 100)); }
    throw Error(`Workspace startup timed out: ${await evaluate("layerApp.app.workspace_view()")}`);
  };
  const ready = 'window.layerApp?.startupTimes.complete != null && JSON.parse(layerApp.app.workspace_view())?.ready && !JSON.parse(layerApp.app.workspace_view()).busy';
  const snapshot = edit => `new Promise((resolve, reject) => {
    const open = indexedDB.open("capycanvas.workspaces", 1);
    open.onerror = () => reject(open.error);
    open.onsuccess = () => {
      const tx = open.result.transaction("workspace", "readwrite"), store = tx.objectStore("workspace"), read = store.get("database");
      let result;
      read.onsuccess = () => { const database = JSON.parse(read.result.snapshot); result = (${edit})(database); store.put({id: "database", snapshot: JSON.stringify(database)}); };
      tx.oncomplete = () => { open.result.close(); resolve(result); };
      tx.onerror = () => reject(tx.error);
    };
  })`;
  await wait(ready);
  const stale = await evaluate(snapshot(`database => Object.values(database.items).map(item => { item.entity.working.zen_mode = {}; return item.entity.id; })`));
  assert.ok(stale.length >= 3);
  await reload();
  await wait(ready);
  const view = JSON.parse(await evaluate("layerApp.app.workspace_view()"));
  assert.equal(view.error, null);
  assert.equal(view.id, "builtin:workspace:illustrator");
  assert.equal(await evaluate('document.querySelector(".canvas-notice-text")?.textContent'),
    "Saved workspaces couldn't be opened, so they were reset.");
  assert.equal(await evaluate('[...document.querySelectorAll("dialog")].some(d => d.open)'), false);
  const zen = await evaluate(snapshot(`database => Object.values(database.items).map(item => item.entity.working.zen_mode)`));
  assert.ok(zen.length >= 3 && zen.every(value => typeof value === "boolean"));
  await reload();
  await wait(ready);
  assert.equal(JSON.parse(await evaluate("layerApp.app.workspace_view()")).error, null);
  console.log(`PASS: startup replaced ${stale.length} unreadable workspaces and started without a dialog`);
}
