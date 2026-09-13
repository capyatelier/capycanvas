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
      // Exercise default recovery through real IndexedDB commits/aborts, with
      // the exact unknown ColorState field found in development workspaces.
      const recoveryName = name + '.recovery', defaultId = 'builtin:workspace:painter';
      const recovery = createWorkspaceStore(reduce, {name:recoveryName});
      const seed = structuredClone(first);
      seed.operation_id = crypto.randomUUID();
      const included = structuredClone(seed.writes[0]);
      included.metadata = JSON.parse(included.metadata_json);
      included.id = defaultId; included.metadata.builtin = true; included.metadata.name = 'Painter';
      included.metadata_json = JSON.stringify(included.metadata);
      seed.writes.push(included);
      const customId = seed.writes[0].id;
      const raw = async change => {
        const connection = await new Promise((resolve,reject) => {
          const request=indexedDB.open(recoveryName,1);
          request.onsuccess=()=>resolve(request.result); request.onerror=()=>reject(request.error);
        });
        try { return await new Promise((resolve,reject) => {
          const tx=connection.transaction('workspace',change?'readwrite':'readonly'), store=tx.objectStore('workspace');
          const read=store.get('database'); let snapshot;
          read.onsuccess=()=>{snapshot=JSON.parse(read.result.snapshot);if(change){change(snapshot);store.put({id:'database',snapshot:JSON.stringify(snapshot)});}};
          tx.oncomplete=()=>resolve(snapshot);tx.onabort=()=>reject(tx.error);
        }); } finally { connection.close(); }
      };
      let recovered, resetAbort, resetUnchanged, resetDurable, customUnchanged, customError, ownedError;
      try {
        await recovery.execute(JSON.stringify({type:'commit',batch:seed}));
        const before=await raw();
        const damaged=await raw(s=>{s.items[defaultId].entity.working.colors.shape='wheel';});
        await recovery.execute('{"type":"list"}');
        const claim={type:'claim',id:defaultId,owner:seed.owner,reset_invalid_default:'web'};
        try { await recovery.execute(JSON.stringify({...claim,owner:{id:'another-window',epoch:'another-session'}})); }
        catch(e) { ownedError=JSON.parse(e).kind; }
        const abortReset=createWorkspaceStore((...args)=>{
          const result=reduce(...args);
          if(JSON.parse(args[1]).type==='claim')throw JSON.stringify({kind:'failed_write',message:'Abort after default reset'});
          return result;
        },{name:recoveryName});
        try { await abortReset.execute(JSON.stringify(claim)); } catch(e) { resetAbort=JSON.parse(e).kind; }
        finally { abortReset.close(); }
        resetUnchanged=JSON.stringify(await raw())===JSON.stringify(damaged);
        recovered=JSON.parse(await recovery.execute(JSON.stringify(claim))).value;
        const saved=await raw();
        customUnchanged=JSON.stringify(saved.items[customId])===JSON.stringify(before.items[customId]);
        recovery.close();
        resetDurable=JSON.stringify(JSON.parse(await recovery.execute(JSON.stringify(claim))).value)===JSON.stringify(recovered);
        await raw(s=>{s.items[customId].entity.working.colors.shape='wheel';});
        try { await recovery.execute(JSON.stringify({...claim,id:customId})); } catch(e) { customError=JSON.parse(e).kind; }
      } finally { recovery.close(); indexedDB.deleteDatabase(recoveryName); }
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
      return {results, abortError, unchanged, upgradeError, unavailableError, quotaError,
        recovered,resetAbort,resetUnchanged,resetDurable,customUnchanged,customError,ownedError};
    } finally { store.close(); indexedDB.deleteDatabase(name); }
  })().catch(e=>{throw new Error(typeof e==='string'?e:(e?.stack||String(e)));})`);
  // Native serde_json::Value expands f32 to f64; direct Wasm JSON uses the
  // shortest f32 spelling. Compare the typed float values, retaining exact
  // string counters/fences and integer identities.
  const floats = value => JSON.parse(JSON.stringify(value, (_, v) => typeof v === "number" && !Number.isInteger(v) ? Math.fround(v) : v));
  assert.deepEqual(floats(result.results), floats(fixtures.map(f=>f.expected)));
  assert.equal(result.abortError, "failed_write"); assert.ok(result.unchanged);
  assert.equal(result.upgradeError, "unsupported_schema");
  assert.equal(result.unavailableError, "unavailable"); assert.equal(result.quotaError, "storage_full");
  assert.equal(result.ownedError,"owned_elsewhere");
  assert.equal(result.resetAbort,"failed_write"); assert.ok(result.resetUnchanged);
  assert.ok(result.resetDurable); assert.ok(result.customUnchanged);
  assert.equal(result.customError,"invalid_data");
  assert.equal(result.recovered.entity.id,"builtin:workspace:painter");
  assert.equal(result.recovered.entity.working.colors.shape,"circle");
  assert.deepEqual(result.recovered.generations,{metadata:"2",layout:"2",working:"2"});
  console.log(`PASS: ${fixtures.length} IndexedDB/SQLite contract cases, aborted transaction, newer database, unavailable storage and quota errors`);
  console.log('PASS: corrupt default recovery, reset transaction rollback, live ownership, reopen persistence and custom workspace preservation');
}
