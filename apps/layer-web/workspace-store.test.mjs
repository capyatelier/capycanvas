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
    const reduce = (snapshot, request, pending, _now, _live, _at, compatibility) => glue.workspace_database(snapshot, request, pending, now, undefined, 0, compatibility);
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
  await checkWorkspaceSnapshots(evaluate, fixtures[0].request);
}

async function checkWorkspaceSnapshots(evaluate, commit) {
  const released = await readFile(new URL("../../crates/layer-workspace/tests/compat/1.0.15/browser.json", import.meta.url), "utf8");
  const result = await evaluate(`(async () => {
    const resources = performance.getEntriesByType('resource').map(r => r.name);
    const glue = await import(resources.find(n => /layer_web\\.[a-f0-9]+\\.js$/.test(n)) || './pkg/layer_web.js');
    const {createWorkspaceStore} = await import(resources.find(n => /workspace-store\\.[a-f0-9]+\\.js$/.test(n)) || './workspace-store.js');
    const name = 'capycanvas.snapshots.' + crypto.randomUUID();
    const connection = await new Promise((resolve,reject) => {
      const open = indexedDB.open(name,1);
      open.onupgradeneeded = () => open.result.createObjectStore('workspace',{keyPath:'id'});
      open.onsuccess = () => resolve(open.result); open.onerror = () => reject(open.error);
    });
    const records = async (values = []) => new Promise((resolve,reject) => {
      const tx = connection.transaction('workspace',values.length ? 'readwrite' : 'readonly'), store = tx.objectStore('workspace');
      for (const value of values) store.put(value);
      const read = store.getAll();
      tx.oncomplete = () => resolve(read.result.sort((a,b) => a.id.localeCompare(b.id)));
      tx.onabort = () => reject(tx.error);
    });
    const encoded = value => JSON.stringify(value,(_key,value) => typeof value==='bigint' ? value.toString() : value);
    const original = ${JSON.stringify(released)}, legacy = 'legacy unreadable bytes';
    await records([{id:'database',snapshot:original},{id:'unreadable',snapshot:legacy},{id:'version',value:{version:'1.0.14',unreadable_identity:null,copies:[]}}]);
    const reduce = (...args) => glue.workspace_database(...args);
    const store = createWorkspaceStore(reduce,{name});
    let aborted, quota;
    try {
      await store.execute('{"type":"reopen"}');
      const first = await records(), copies = first.filter(row => row.id.startsWith('snapshot:'));
      await store.execute('{"type":"reopen"}');
      const repeated = await records();
      for (const version of ['99.0.0',...Array(10).fill('1.0.14')]) {
        const marker = (await records()).find(row => row.id==='version');
        marker.value.version = version; await records([marker]);
        await store.execute('{"type":"reopen"}');
      }
      const beforeAdoption = (await records()).filter(row => row.id.startsWith('snapshot:')).length;
      await store.execute('{"type":"adopted"}');
      const kept = await records();
      const marker = kept.find(row => row.id==='version'); marker.value.version='99.0.0'; await records([marker]);
      const beforeAbort = await records();
      aborted = createWorkspaceStore((...args) => {
        const result = reduce(...args);
        return {...result,get snapshot() { throw JSON.stringify({kind:'failed_write',message:'Abort after queued version copy'}); }};
      },{name});
      let abortError;
      try { await aborted.execute('{"type":"reopen"}'); } catch(error) { abortError=JSON.parse(error).kind; }
      const unchanged = encoded(beforeAbort)===encoded(await records());
      let attempts = 0;
      quota = createWorkspaceStore(reduce,{name,indexedDB:{open(...args) {
        const request = indexedDB.open(...args);
        request.addEventListener('success',() => {
          const db = request.result, transaction = db.transaction.bind(db);
          db.transaction = (...args) => {
            const tx = transaction(...args), objectStore = tx.objectStore.bind(tx);
            tx.objectStore = (...args) => {
              const store = objectStore(...args), put = store.put.bind(store);
              store.put = row => {
                if (row.id.startsWith('snapshot:')) { attempts++; throw new DOMException('Copy quota exhausted','QuotaExceededError'); }
                return put(row);
              };
              return store;
            };
            return tx;
          };
        });
        return request;
      }}});
      await quota.execute(${JSON.stringify(JSON.stringify(commit))});
      const afterQuota = await records();
      const quotaMarkerUnchanged = encoded(afterQuota.find(row=>row.id==='version'))===encoded(beforeAbort.find(row=>row.id==='version'));
      const quotaSaved = !!JSON.parse(afterQuota.find(row=>row.id==='database').snapshot).items[${JSON.stringify(commit.batch.writes[0].id)}];
      quota.close();
      await store.execute('{"type":"reopen"}');
      const quotaRetried = (await records()).filter(row=>row.id.startsWith('snapshot:')).length===kept.filter(row=>row.id.startsWith('snapshot:')).length+1;
      const restoreMarker = (await records()).find(row=>row.id==='version');
      restoreMarker.value.version='';
      const restoredCopy = kept.find(row => row.snapshot===original);
      await records([{id:'database',snapshot:restoredCopy.snapshot},restoreMarker]);
      await store.execute('{"type":"reopen"}');
      const restored = (await records()).find(row => row.id==='database').snapshot===original;
      await store.execute('{"type":"adopted"}');
      const restoredCount = (await records()).filter(row => row.id.startsWith('snapshot:')).length;
      await store.execute(${JSON.stringify(JSON.stringify(commit))});
      const beforeRestore=await records(), sourceCopy=beforeRestore.find(row=>row.id.startsWith('snapshot:')&&row.snapshot===original);
      const restoring={type:'restore_snapshot',id:sourceCopy.id};
      let restoreAttempts=0,restoreQuotaError;
      quota=createWorkspaceStore((...args)=>{
        const result=reduce(...args);
        if(JSON.parse(args[1]).type==='restore_snapshot'){restoreAttempts++;throw JSON.stringify({kind:'storage_full',message:'Injected restore copy failure'});}
        return result;
      },{name});
      try{await quota.execute(JSON.stringify(restoring));}catch(error){restoreQuotaError=JSON.parse(error).kind;}
      const restoreUnchanged=encoded(await records())===encoded(beforeRestore);
      await store.execute(JSON.stringify(restoring));
      const afterRestore=await records(),currentCopy=afterRestore.filter(row=>row.id.startsWith('snapshot:')&&!beforeRestore.some(old=>old.id===row.id));
      const replacedKept=currentCopy.length===1&&currentCopy[0].snapshot===beforeRestore.find(row=>row.id==='database').snapshot;
      const sourceKept=afterRestore.find(row=>row.id===sourceCopy.id).snapshot===sourceCopy.snapshot;
      const restoredItemMissing=!JSON.parse(afterRestore.find(row=>row.id==='database').snapshot).items[${JSON.stringify(commit.batch.writes[0].id)}];
      return {copies:copies.map(row => row.snapshot).sort(),repeated:encoded(first)===encoded(repeated),
        beforeAdoption,kept:kept.filter(row => row.id.startsWith('snapshot:')).length,
        metadata:kept.find(row => row.id==='version').value.copies.length,
        abortError,unchanged,attempts,quotaMarkerUnchanged,quotaSaved,quotaRetried,restored,restoredCount,restoreQuotaError,restoreAttempts,restoreUnchanged,replacedKept,sourceKept,restoredItemMissing};
    } finally { store.close(); aborted?.close(); quota?.close(); connection.close(); indexedDB.deleteDatabase(name); }
  })()`);
  assert.deepEqual(result.copies, [released, "legacy unreadable bytes"].sort());
  assert.ok(result.repeated);
  assert.equal(result.beforeAdoption, 13);
  assert.equal(result.kept, 10); assert.equal(result.metadata, 10);
  assert.equal(result.abortError, "failed_write"); assert.ok(result.unchanged);
  assert.equal(result.attempts, 1); assert.ok(result.quotaMarkerUnchanged); assert.ok(result.quotaSaved); assert.ok(result.quotaRetried);
  assert.ok(result.restored); assert.equal(result.restoredCount, 10);
  assert.equal(result.restoreQuotaError,'storage_full');assert.equal(result.restoreAttempts,1);assert.ok(result.restoreUnchanged);
  assert.ok(result.replacedKept);assert.ok(result.sourceKept);assert.ok(result.restoredItemMissing);
  console.log("PASS: Web version copies preserve released and legacy bytes, survive downgrade, prune after adoption, roll back queued copies on abort and restore by hand");
}

export async function checkStaleStorageStartup({evaluate, reload}) {
  const wait = async predicate => {
    for (let i = 0; i < 300; i++) { try { if (await evaluate(predicate)) return; } catch {} await new Promise(r => setTimeout(r, 100)); }
    throw Error(`Startup timed out: ${await evaluate("layerApp.app.workspace_view()")}`);
  };
  const ready = 'window.layerApp?.startupTimes.complete != null && JSON.parse(layerApp.app.workspace_view())?.ready && !JSON.parse(layerApp.app.workspace_view()).busy';
  const restart = async () => {
    await evaluate('(async()=>{await layerApp.documents.startRecovery();await layerApp.documents.autosave(true)})()');
    await reload();
    await wait(ready);
    await evaluate('layerApp.documents.startRecovery()');
  };
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
  const workspaceRecord = id => `new Promise((resolve, reject) => {
    const open = indexedDB.open("capycanvas.workspaces", 1);
    open.onerror = () => reject(open.error);
    open.onsuccess = () => {
      const read = open.result.transaction("workspace").objectStore("workspace").get(${JSON.stringify(id)});
      read.onsuccess = () => { open.result.close(); resolve(read.result?.snapshot ?? null); };
      read.onerror = () => reject(read.error);
    };
  })`;
  const keptProfile = `new Promise((resolve, reject) => {
    const open = indexedDB.open("capy-color-preferences");
    open.onerror = () => reject(open.error);
    open.onsuccess = () => {
      const read = open.result.transaction("profiles").objectStore("profiles").get("kept-profile");
      read.onsuccess = () => { open.result.close(); resolve(read.result ? Array.from(read.result) : null); };
      read.onerror = () => reject(read.error);
    };
  })`;
  const olderColorPreferences = `new Promise((resolve, reject) => {
    const removed = indexedDB.deleteDatabase("capy-color-preferences");
    removed.onblocked = () => reject(new Error("color preferences are open"));
    removed.onerror = () => reject(removed.error);
    removed.onsuccess = () => {
      const open = indexedDB.open("capy-color-preferences", 1);
      open.onupgradeneeded = () => { open.result.createObjectStore("values"); open.result.createObjectStore("profiles"); };
      open.onerror = () => reject(open.error);
      open.onsuccess = () => {
        const tx = open.result.transaction(["values", "profiles"], "readwrite"), values = tx.objectStore("values");
        values.put(new TextEncoder().encode('{"destinations":[null,null,null,null],"named":[]}'), "export-presets");
        values.put({hidden: ["not a list"]}, "profile-menu-hidden");
        tx.objectStore("profiles").put(new Uint8Array([1, 2, 3]), "kept-profile");
        tx.oncomplete = () => { open.result.close(); resolve(true); };
        tx.onerror = () => reject(tx.error);
      };
    };
  })`;
  await wait(ready);
  await evaluate('layerApp.documents.startRecovery()');
  await wait('(() => { const workspace = JSON.parse(layerApp.app.workspace_view()); return !workspace.dirty && !workspace.saving; })()');
  const stale = await evaluate(snapshot(`database => Object.values(database.items).map(item => { item.entity.content = {}; item.claim = null; return item.entity.id; })`));
  assert.ok(stale.length >= 3);
  const unreadable = await evaluate(workspaceRecord("database"));
  const previousCopy = await evaluate(workspaceRecord("unreadable"));
  await evaluate(`localStorage.setItem("layer.preferences.v1", JSON.stringify({...layerApp.state().settings, pan_speed: 2, tip_lock: true, prediction_algorithm: "kalman", zoom_speed: "fast"}))`);
  await evaluate(`sessionStorage.setItem("capy.workspace.owner", JSON.stringify({id: 7, epoch: "stale", extra: true}))`);
  assert.equal(await evaluate(olderColorPreferences), true);
  await restart();
  const view = JSON.parse(await evaluate("layerApp.app.workspace_view()"));
  assert.equal(view.error, null);
  assert.ok(!stale.includes(view.id));
  assert.equal(await evaluate("layerApp.state().notice?.text"),"This version of Capy Canvas can't open your saved workspaces. Update Capy Canvas to open them.");
  assert.equal(await evaluate('document.querySelector("#status").textContent'), "");
  assert.equal(await evaluate('[...document.querySelectorAll("dialog")].some(d => d.open)'), false);
  assert.equal(await evaluate(workspaceRecord("unreadable")), previousCopy);
  const retained = JSON.parse(await evaluate(workspaceRecord("database")));
  for (const id of stale) assert.deepEqual(retained.items[id], JSON.parse(unreadable).items[id]);
  assert.equal(Object.keys(retained.items).length, stale.length + 1);
  assert.equal(await evaluate("layerApp.state().settings.pan_speed"), 2);
  assert.equal(await evaluate("layerApp.state().settings.zoom_speed"), 1);
  const zen = await evaluate(snapshot(`database => Object.values(database.items).map(item => item.entity.working.zen_mode)`));
  assert.ok(zen.length >= 3 && zen.every(value => typeof value === "boolean"));
  assert.equal(await evaluate("(async () => Array.isArray(await layerApp.app.profile_library('list')))()"), true);
  assert.equal(await evaluate("(async () => (await layerApp.app.load_export_dialog()) instanceof Uint8Array)()"), true);
  assert.deepEqual(await evaluate(keptProfile), [1, 2, 3], "Upgrading color preferences keeps stored profiles");
  await wait('!layerApp.documents.busy() && layerApp.state().commands.find(c => c.id === "export_document")?.enabled');
  await evaluate("layerApp.dispatch({type: 'invoke', command: 'export_document'})");
  await wait('!!document.querySelector(\'dialog[open] .dropdown[aria-label="Format"]\')');
  await evaluate(`[...document.querySelectorAll("dialog[open]")].forEach(d => d.close())`);
  await evaluate(`new Promise((resolve, reject) => {
    const open = indexedDB.open("capy-color-preferences", 2);
    open.onerror = () => reject(open.error);
    open.onsuccess = () => {
      const tx = open.result.transaction("values", "readwrite"), values = tx.objectStore("values");
      values.put('{"destinations":[]}', "export-presets");
      values.put({hidden: 7}, "profile-menu-hidden");
      tx.oncomplete = () => { open.result.close(); resolve(true); };
      tx.onerror = () => reject(tx.error);
    };
  })`);
  assert.equal(await evaluate("(async () => Array.isArray(await layerApp.app.profile_library('list')))()"), true);
  assert.equal(await evaluate("(async () => (await layerApp.app.load_export_dialog()) instanceof Uint8Array)()"), true);
  await restart();
  assert.equal(JSON.parse(await evaluate("layerApp.app.workspace_view()")).error, null);
  assert.equal(await evaluate("!!layerApp.state().notice"), false);
  assert.equal(JSON.parse(await evaluate("layerApp.app.workspace_view()")).id, view.id);
  const restarted = JSON.parse(await evaluate(workspaceRecord("database")));
  assert.equal(Object.keys(restarted.items).length, stale.length + 1);
  for (const id of stale) assert.deepEqual(restarted.items[id], JSON.parse(unreadable).items[id]);
  assert.equal(await evaluate('document.querySelector("#status").textContent'), "");
  assert.equal(await evaluate("layerApp.state().settings.pan_speed"), 2);
  const released = JSON.parse(await readFile(new URL("../../crates/layer-workspace/tests/compat/1.0.15/browser.json", import.meta.url), "utf8"));
  await evaluate(snapshot(`database => { Object.assign(database, ${JSON.stringify(released)}); return true; }`));
  await reload();
  await wait(ready);
  assert.equal(await evaluate('layerApp.state().color_library.palettes.some(p => p.name === "Release palette" && p.swatches.some(s => s.name === "Release blue"))'), true);
  assert.equal(await evaluate('layerApp.state().brush.diameter'), 37);
  assert.equal(await evaluate('!!layerApp.state().notice'), false);
  console.log(`PASS: startup read stale preferences, kept ${stale.length} unreadable workspaces and created one persistent workspace, upgraded older color preferences without losing data, and opened Export without a message`);
}
