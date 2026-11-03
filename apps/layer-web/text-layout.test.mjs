import assert from 'node:assert/strict';
import test from 'node:test';
import {middleEllipsis} from './text-layout.js';

test('middle ellipsis keeps both ends and complete graphemes within measured space',()=>{
  const measure=text=>[...new Intl.Segmenter(undefined,{granularity:'grapheme'}).segment(text)].length;
  assert.equal(middleEllipsis('painting.capy',100,measure),'painting.capy');
  assert.equal(middleEllipsis('painting.capy',7,measure),'pai…apy');
  assert.equal(middleEllipsis('e\u0301🎨日本語.capy',5,measure),'e\u0301🎨…py');
  assert.equal(middleEllipsis('painting.capy',0,measure),'');
  assert.equal(middleEllipsis('painting.capy',.5,measure),'');
});
