import test from 'node:test';
import assert from 'node:assert/strict';
import {writeDocumentFile} from './file-handles.js';

function fixture() {
  let bytes='original',staged,closed=0,aborted=0;
  const handle={async isSameEntry(other){return other===handle;},async createWritable(){return {
    async write(value){staged=value;},async close(){closed++;bytes=staged;},async abort(){aborted++;},
  };}};
  return {handle,get bytes(){return bytes;},get closed(){return closed;},get aborted(){return aborted;}};
}

test('Save over a live source waits for its existing Store copy before committing',async()=>{
  const f=fixture(),copy=Promise.withResolvers(),entered=Promise.withResolvers();let calls=0;
  const saving=writeDocumentFile(f.handle,'saved',[{id:7,handle:f.handle}],async id=>{assert.equal(id,7);calls++;entered.resolve();await copy.promise;});
  await Promise.race([entered.promise,saving]);
  assert.equal(calls,1);assert.equal(f.closed,0);assert.equal(f.bytes,'original');
  copy.resolve();await saving;
  assert.equal(f.closed,1);assert.equal(f.aborted,0);assert.equal(f.bytes,'saved');
});

for(const reason of ['quota exceeded','IO failed'])test(`Save-over refusal keeps the original file: ${reason}`,async()=>{
  const f=fixture();
  await assert.rejects(writeDocumentFile(f.handle,'saved',[{id:7,handle:f.handle}],async()=>{throw Error(reason);}),{message:reason});
  assert.equal(f.bytes,'original');assert.equal(f.closed,0);assert.equal(f.aborted,1);
});

test('Save As to a different entry does not wait for a package copy',async()=>{
  const f=fixture();await writeDocumentFile(f.handle,'saved',[{id:7,handle:{}}],()=>assert.fail('Unrelated source'));
  assert.equal(f.bytes,'saved');assert.equal(f.closed,1);
});
