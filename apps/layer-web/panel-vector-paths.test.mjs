import test from 'node:test';
import assert from 'node:assert/strict';
import {svgPath,panelVectorPaths} from './panel-vector-paths.js';

const command=(op,...points)=>[op,...points.map(p=>p*1024),...Array(6-points.length).fill(0)];
const rectangle=[command(0,0,0),command(1,10,0),command(1,10,20),command(1,0,20),command(3)];
const body=[command(0,0,0),command(1,20,0),command(2,22,0,24,2,24,4),command(1,0,4),command(3)];
const bridge=[command(0,-8,0),command(1,0,0),command(1,0,4),command(3)];

test('fixed commands preserve cubic controls, close operations and fractional offsets',()=>{
  assert.equal(svgPath(body,{x:0.125,y:-2.5}),'M0.125 -2.5 L20.125 -2.5 C22.125 -2.5 24.125 -0.5 24.125 1.5 L0.125 1.5 Z');
});

test('disjoint columns keep union coordinates, local original bodies and global flagged triangles',()=>{
  const a={id:11,bounds:{x:40,y:30,width:24,height:4},tabs_visible:true};
  const b={id:12,bounds:{x:450,y:80,width:24,height:4},tabs_visible:false};
  const layout={groups:[a,b]};
  const projection={sources:[{id:1,bounds:{x:20,y:20,width:10,height:20},commands:rectangle},{id:2,bounds:{x:490,y:60,width:10,height:20},commands:rectangle,reverse:true}],surfaces:[{column:1,group:11,commands:[...bridge,...body],triangles:[[0,0,1024,0,0,2048,5]]},{column:2,group:12,commands:body,triangles:[[-1024,0,0,0,0,1024,3]]}]};
  const result=panelVectorPaths(layout,projection);
  assert.deepEqual(result.columns.map(c=>c.bounds),[{x:20,y:20,width:44,height:20},{x:450,y:60,width:50,height:24}]);
  assert.deepEqual([...result.triangles],[40,30,41,30,40,32,5,449,80,450,80,450,81,3]);
  assert.equal(result.buffers[0],result.triangles);
  assert.equal(result.columns[0].receivers[0].body,svgPath(body));
  assert.equal(result.columns[0].receivers[0].header,svgPath(body,{x:20,y:10}));
  assert.equal(result.columns[0].receivers[0].tabs,true);assert.equal(result.columns[1].receivers[0].tabs,false);
  assert.ok(result.columns[1].d.startsWith('M40 20 L50 20 L50 0 L40 0 Z'));
  assert.equal(result.columns[1].source.d,svgPath(rectangle));
});

test('reversing the source reverses cubic endpoints and control points',()=>{
  const source={id:1,bounds:{x:0,y:0,width:24,height:4},commands:body,reverse:true};
  const result=panelVectorPaths({groups:[]},{sources:[source],surfaces:[]});
  assert.equal(result.columns[0].d,'M0 4 L24 4 C24 2 22 0 20 0 L0 0 Z');
  assert.equal(result.triangles.length,0);assert.deepEqual(result.columns[0].receivers,[]);
});
