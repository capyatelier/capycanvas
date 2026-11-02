import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {launchChrome} from '../../cdp.mjs';
import {checkWorkspaceStore} from '../../../apps/layer-web/workspace-store.test.mjs';

const chrome = await launchChrome(['--headless=new', '--disable-gpu', '--disable-software-rasterizer']);
try {
  await chrome.attachPage();
  await chrome.call('Page.enable');
  await chrome.call('Runtime.enable');
  const loaded = chrome.once('Page.loadEventFired');
  await chrome.call('Page.navigate', {url: process.argv[2]});
  await loaded;
  assert.equal(await chrome.evaluate('globalThis.compatReady'), true);
  assert.equal(await chrome.evaluate('document.querySelectorAll("canvas").length'), 0);
  const adapter = await chrome.evaluate('(async()=>{const adapter=await navigator.gpu?.requestAdapter();return adapter ? true : null})()');
  assert.equal(adapter, null, 'GPU-free checks must have no WebGPU adapter');
  const gpu = await chrome.call('SystemInfo.getInfo', {}, null);
  console.log('PASS: Chrome --disable-gpu, no GPU adapter', JSON.stringify(gpu.gpu.featureStatus));
  await checkWorkspaceStore(chrome);
  const released = await readFile(new URL('../../../crates/layer-workspace/tests/compat/1.0.15/browser.json', import.meta.url), 'utf8');
  const upgraded = await chrome.evaluate(`(async()=>{
    const glue=await import('./pkg/layer_web.js');
    const {createWorkspaceStore}=await import('./workspace-store.js');
    const name='capycanvas.compat-upgrade.'+crypto.randomUUID();
    const database=await new Promise((resolve,reject)=>{
      const request=indexedDB.open(name,1);
      request.onupgradeneeded=()=>request.result.createObjectStore('workspace',{keyPath:'id'});
      request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);
    });
    const records=values=>new Promise((resolve,reject)=>{
      const tx=database.transaction('workspace',values?'readwrite':'readonly');
      const table=tx.objectStore('workspace');
      for(const value of values||[])table.put(value);
      const read=table.getAll();tx.oncomplete=()=>resolve(read.result);tx.onabort=()=>reject(tx.error);
    });
    const original=${JSON.stringify(released)};
    await records([{id:'database',snapshot:original}]);
    const store=createWorkspaceStore((...args)=>glue.workspace_database(...args),{name});
    try {
      for(const request of [{type:'reopen'},{type:'editing',language:'en'},{type:'switcher'},{type:'adopted'}])
        await store.execute(JSON.stringify(request));
      const rows=await records();
      return {kept:rows.find(row=>row.id==='database')?.snapshot===original,
        marker:!!rows.find(row=>row.id==='version')?.value.version,
        copy:rows.some(row=>row.id.startsWith('snapshot:')&&row.snapshot===original)};
    } finally {store.close();database.close();indexedDB.deleteDatabase(name);}
  })()`);
  assert.deepEqual(upgraded, {kept:true,marker:true,copy:true});
  assert.deepEqual(chrome.errors, []);
  console.log('PASS: released IndexedDB startup keeps the database and writes its version marker and copy');
} finally {
  await chrome.close();
}
