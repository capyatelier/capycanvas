import test from 'node:test';
import assert from 'node:assert/strict';
import {createPanelVectorQueue} from './panel-vector-queue.js';

function fixture() {
  const jobs=[],commits=[],errors=[];
  const enqueue=createPanelVectorQueue(layout=>new Promise((resolve,reject)=>jobs.push({layout,resolve,reject})),e=>errors.push(e));
  return {jobs,commits,errors,send(id,layoutOnly=true,open=true){enqueue({id,collapsed:[{open:open?{}:null}]},layoutOnly,(layout,result,only)=>commits.push({id:layout.id,result,only}));},async finish(index,result=index){jobs[index].resolve(result);await Promise.resolve();await Promise.resolve();}};
}

test('motion has one in-flight projection and keeps only the latest pending pose without starving publication',async()=>{
  const f=fixture();f.send(0,false);
  for(let i=1;i<=20;i++)f.send(i);
  assert.equal(f.jobs.length,1);
  await f.finish(0);
  assert.deepEqual(f.commits,[{id:0,result:0,only:false}]);
  assert.equal(f.jobs[1].layout.id,20);
  for(let i=21;i<=40;i++)f.send(i);
  await f.finish(1);
  assert.equal(f.commits[1].id,20);assert.equal(f.jobs[2].layout.id,40);
  await f.finish(2);assert.equal(f.commits[2].id,40);
});

test('a full update invalidates old results and remains full when pending motion replaces it',async()=>{
  const f=fixture();f.send(0,false);f.send(1,false);f.send(2);
  await f.finish(0);assert.deepEqual(f.commits,[]);assert.equal(f.jobs[1].layout.id,2);
  await f.finish(1);assert.deepEqual(f.commits,[{id:2,result:1,only:false}]);
});

test('closing clears immediately and cancels pending and in-flight publication',async()=>{
  const f=fixture();f.send(0,false);f.send(1);f.send(2,true,false);
  assert.equal(f.commits.length,1);assert.equal(f.commits[0].id,2);assert.deepEqual(f.commits[0].result.columns,[]);
  await f.finish(0);assert.equal(f.commits.length,1);assert.equal(f.jobs.length,1);
  f.send(3,false);await f.finish(1);assert.equal(f.commits[1].id,3);
});

test('stale failures are ignored and current failures drain the newest pose',async()=>{
  const f=fixture();f.send(0,false);f.send(1,false);f.jobs[0].reject(Error('obsolete'));
  await Promise.resolve();await Promise.resolve();assert.deepEqual(f.errors,[]);assert.equal(f.jobs.length,2);
  f.send(2);const error=Error('current');f.jobs[1].reject(error);
  await Promise.resolve();await Promise.resolve();assert.deepEqual(f.errors,[error]);assert.equal(f.jobs[2].layout.id,2);
  await f.finish(2);assert.equal(f.commits[0].id,2);
});

test('the first completed full update clears the full requirement on pending motion',async()=>{
  const f=fixture();f.send(0,false);f.send(1);
  await f.finish(0);await f.finish(1);
  assert.deepEqual(f.commits.map(c=>c.only),[false,true]);
});

test('repeated poses still prepare on the worker',async()=>{
  const f=fixture();f.send(0,false);await f.finish(0);f.send(0);
  assert.equal(f.jobs.length,2);assert.equal(f.commits.length,1);
  await f.finish(1);assert.deepEqual(f.commits.map(c=>c.only),[false,true]);
});

test('reentrant closure publishes after the matching layout and shape commit',async()=>{
  const jobs=[],commits=[];
  const send=createPanelVectorQueue(layout=>new Promise(resolve=>jobs.push({layout,resolve})),assert.fail);
  send({collapsed:[{open:{}}]},false,()=>{
    send({collapsed:[]},false,(_,result,only)=>commits.push({columns:result.columns,only}));
    commits.push('shape');
  });
  jobs[0].resolve({columns:[]});await Promise.resolve();await Promise.resolve();
  assert.deepEqual(commits,['shape',{columns:[],only:false}]);
});

test('a failed commit reports its error and preserves the full content requirement',async()=>{
  const jobs=[],errors=[],commits=[];
  const send=createPanelVectorQueue(layout=>new Promise(resolve=>jobs.push({layout,resolve})),error=>errors.push(error));
  const layout={collapsed:[{open:{}}]},error=Error('commit failed');
  send(layout,false,()=>{throw error;});jobs[0].resolve({columns:[]});
  await Promise.resolve();await Promise.resolve();assert.deepEqual(errors,[error]);
  send(layout,true,(_,result,only)=>commits.push(only));jobs[1].resolve({columns:[]});
  await Promise.resolve();await Promise.resolve();assert.deepEqual(commits,[false]);
});

test('a successful outer commit cannot acknowledge a reentrant full update',async()=>{
  const jobs=[],commits=[];
  const send=createPanelVectorQueue(layout=>new Promise(resolve=>jobs.push({layout,resolve})),assert.fail);
  const layout=id=>({id,collapsed:[{open:{}}]});
  send(layout(0),false,()=>send(layout(1),false,(_,result,only)=>commits.push(only)));
  jobs[0].resolve({columns:[]});await Promise.resolve();await Promise.resolve();
  jobs[1].resolve({columns:[]});await Promise.resolve();await Promise.resolve();
  assert.deepEqual(commits,[false]);
});
