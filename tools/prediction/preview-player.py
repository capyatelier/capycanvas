#!/usr/bin/env python3
"""Create a standalone preview player from replay-bank.py --analyze output.

Optionally compare an earlier frame export using identical input/query clocks.
This displays recorded centerlines and width probes, not the original brush raster.
"""
import argparse
import json
from pathlib import Path
import numpy as np
from analysis import read_recording
from flicker_metrics import load_trace, surface, interpolate
from history_metrics import validate_pair

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('directory',type=Path)
parser.add_argument('--before',type=Path,help='Earlier frames.jsonl export')
args=parser.parse_args()
root=args.directory;contacts=read_recording(root/'records.jsonl')
current=load_trace(root/'frames.jsonl',contacts)
frames=[current];names=['Smooth Motion']
if args.before:
    before=load_trace(args.before,contacts);validate_pair(before,current)
    frames.insert(0,before);names.insert(0,'Previous revision')
signals=dict(np.load(root/'analysis'/'signals.npz'))
metrics=[('gap_24','Tracking gap at 24 ms display delay'),
         ('flash_tip','Unproductive tip correction'),('flash_body','Unproductive body correction'),
         ('retreat_retreat_tip','Preview retraction'),('correction_tip','Correction shock'),
         ('off_path_tip','Off-path prediction')]
episodes=[];chosen=[]
for metric,description in metrics:
    count=0
    ranking=np.nan_to_num(signals[metric],nan=-1)
    for index in np.argsort(ranking)[::-1]:
        if ranking[index]<=0:break
        f=current[index];cid=f['contact'];now=f['frame_us'];selected_query=f['query']
        if any(c==cid and abs(t-now)<400000 for c,t in chosen):continue
        chosen.append((cid,now));count+=1
        indices=[i for i,g in enumerate(current) if g['contact']==cid and now-150000<=g['frame_us']<=now+200000]
        m=f['transform'];truth=contacts[cid]['samples'];truth=truth[(truth[:,0]>=current[indices[0]]['latest_us']-60000)&(truth[:,0]<=current[indices[-1]]['target_us']+60000)]
        xy=surface(truth[:,1:3],m);payload=[];bounds=[xy]
        for i in indices:
            f=current[i];actual=surface(f['actual_recent'][:,1:3],m)
            previews=[surface(stream[i]['curve'][:,1:3],m) for stream in frames]
            pen=surface(interpolate(contacts[cid]['samples'],f['frame_us']+np.array([0,8,16,24])*1000),m)
            bounds.extend(previews)
            payload.append({'t':f['frame_us']/1000,'query':f['query'],'real':actual.round(3).tolist(),
                            'preview':[p.round(3).tolist() for p in previews],
                            'pen':[p.round(3).tolist() if np.isfinite(p).all() else None for p in pen]})
        allpoints=np.concatenate(bounds);bounds=[allpoints.min(axis=0).tolist(),allpoints.max(axis=0).tolist()]
        episodes.append(dict(label=f'Contact {cid}, query {selected_query}: {description}',truth=xy.round(3).tolist(),frames=payload,bounds=bounds,peak=now/1000))
        if count==5:break
if not episodes:raise SystemExit('No measurable error episodes')
html='''<!doctype html><meta charset="utf-8"><title>Stroke preview comparison</title>
<style>body{font:15px system-ui;background:#f4f2ec;color:#252722;margin:24px}h1{font-size:22px}button,select,input{font:inherit;margin:5px;padding:5px}#views{display:grid;grid-template-columns:repeat(__COUNT__,minmax(0,1fr));gap:12px}.view{background:white;border:1px solid #ccd0c8;border-radius:8px;padding:10px}canvas{width:100%;height:420px;display:block}#seek{width:min(850px,75vw)}p{max-width:1000px;line-height:1.5}small{color:#53594e}</style>
<h1>Prediction geometry across successive frames</h1>
<p>Orange: disposable preview. Blue: already measured ink. Dotted gray: eventual stroke. Magenta cross: true pen position at the selected common display clock. The input and query clocks match; adaptive predictors can use shorter forecast times. Width controls are geometric probes, not the original brush material.</p>
<div><label>Episode <select id="episode"></select></label><button id="play">Play</button><button id="back">Previous frame</button><button id="next">Next frame</button></div>
<div><label>Display delay <select id="delay"><option value="0">0 ms</option><option value="1">8 ms</option><option value="2">16 ms</option><option value="3" selected>24 ms</option></select></label><label>Speed <select id="speed"><option value=".25">¼×</option><option value=".5">½×</option><option value="1" selected>1×</option></select></label><label>Zoom <select id="zoom"><option value=".5">½×</option><option value="1" selected>Fit</option><option value="2">2×</option><option value="4">4×</option></select></label><label>Width <select id="width"><option>2</option><option selected>8</option><option>32</option></select> physical px</label></div>
<div id="views">__VIEWS__</div>
<input id="seek" type="range" min="0" step="1"><span id="time"></span>
<p><small>Frame times are recorded prediction queries. Display delay is a probe, not measured latency. GPU presentation, brush texture, opacity and final pen-up compositing were not recorded. Camera and scale stay fixed within an episode so viewport motion cannot hide oscillation. At high zoom, use the frame controls to inspect the visible part.</small></p>
<script>const DATA=__DATA__;
const episode=document.querySelector('#episode'),seek=document.querySelector('#seek'),play=document.querySelector('#play'),speed=document.querySelector('#speed'),zoom=document.querySelector('#zoom'),width=document.querySelector('#width'),delay=document.querySelector('#delay'),canvases=[...document.querySelectorAll('canvas')];
let frame=0,running=false,start=0;for(let i=0;i<DATA.length;i++)episode.add(new Option(DATA[i].label,i));
function stop(){running=false;play.textContent='Play'}
function selected(){return DATA[+episode.value]}
function draw(){let e=selected(),f=e.frames[frame];seek.value=frame;document.querySelector('#time').textContent=`query ${f.query} · ${(f.t-e.peak).toFixed(1)} ms from selected error`;
 canvases.forEach((canvas,i)=>{let r=canvas.getBoundingClientRect(),dpr=devicePixelRatio||1;canvas.width=r.width*dpr;canvas.height=r.height*dpr;let ctx=canvas.getContext('2d');ctx.scale(dpr,dpr);ctx.fillStyle='#fff';ctx.fillRect(0,0,r.width,r.height);let [lo,hi]=e.bounds;let scale=Math.min((r.width-40)/Math.max(hi[0]-lo[0],40),(r.height-40)/Math.max(hi[1]-lo[1],40))*+zoom.value;ctx.translate(r.width/2,r.height/2);ctx.scale(scale,scale);ctx.translate(-(lo[0]+hi[0])/2,-(lo[1]+hi[1])/2);ctx.lineCap=ctx.lineJoin='round';
 function line(points,color,w,dashed=false){if(points.length<2)return;ctx.strokeStyle=color;ctx.lineWidth=w;ctx.setLineDash(dashed?[3/scale,4/scale]:[]);ctx.beginPath();points.forEach((p,k)=>k?ctx.lineTo(...p):ctx.moveTo(...p));ctx.stroke()}
 line(e.truth,'#81877f',1.2/scale,true);line(f.real,'#286f89',+width.value);line(f.preview[i],'#e77424',+width.value);ctx.setLineDash([]);let tip=f.preview[i].at(-1);if(tip){ctx.beginPath();ctx.arc(...tip,2/scale,0,2*Math.PI);ctx.fillStyle='#973a00';ctx.fill();}let pen=f.pen[+delay.value];if(pen){ctx.strokeStyle='#b01897';ctx.lineWidth=2/scale;ctx.beginPath();ctx.moveTo(pen[0]-5/scale,pen[1]);ctx.lineTo(pen[0]+5/scale,pen[1]);ctx.moveTo(pen[0],pen[1]-5/scale);ctx.lineTo(pen[0],pen[1]+5/scale);ctx.stroke()}});}
function reset(){stop();frame=0;seek.max=selected().frames.length-1;draw()}
function tick(t){if(!running)return;let e=selected(),elapsed=(t-start)*+speed.value+e.frames[0].t;while(frame+1<e.frames.length&&e.frames[frame+1].t<=elapsed)frame++;draw();if(frame==e.frames.length-1)stop();else requestAnimationFrame(tick)}
play.onclick=()=>{if(running){stop();return}if(frame==selected().frames.length-1)frame=0;running=true;play.textContent='Pause';start=performance.now()-(selected().frames[frame].t-selected().frames[0].t)/+speed.value;requestAnimationFrame(tick)};
seek.oninput=()=>{stop();frame=+seek.value;draw()};episode.onchange=reset;document.querySelector('#back').onclick=()=>{stop();frame=Math.max(0,frame-1);draw()};document.querySelector('#next').onclick=()=>{stop();frame=Math.min(selected().frames.length-1,frame+1);draw()};zoom.onchange=width.onchange=delay.onchange=draw;speed.onchange=stop;window.onresize=draw;reset();</script>'''
html=html.replace('__COUNT__',str(len(names))).replace('__VIEWS__',''.join(f'<div class="view">{name}<canvas></canvas></div>' for name in names))
(root/'preview-player.html').write_text(html.replace('__DATA__',json.dumps(episodes,separators=(',',':'))))
print(root/'preview-player.html')
