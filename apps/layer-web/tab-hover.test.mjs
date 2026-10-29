import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';

export async function checkTabSurfaceCoverage({call,evaluate,settle}) {
  await evaluate(`(async()=>{
    const {tabHover,prepareTabPreview}=await import('./workspace-chrome.js');
    const root=document.createElement('div');root.id='tab-surface-fixture';
    Object.assign(root.style,{position:'fixed',left:'0',top:'0',width:'920px',height:'680px',zIndex:'2147483647',background:'rgb(255,0,255)',pointerEvents:'none'});
    for(const theme of ['light','dark'])for(const alpha of [1,.65])for(const drawer of [false,true])for(const fractional of [false,true])for(const dragged of [false,true]){
      const index=root.children.length,value=theme==='light'?237:65,color='rgba('+[value,value,value,alpha].join(',')+')',group=document.createElement('div');
      group.className=drawer?'content-drawer':'dock-group';group.dataset.sample=JSON.stringify({theme,alpha,drawer,fractional,dragged,value});
      Object.assign(group.style,{left:(24+index%4*220+(fractional?.25:0))+'px',top:(24+Math.floor(index/4)*80+(fractional?.25:0))+'px',width:'192px',height:'60px',borderRadius:'0',boxShadow:'none'});
      for(const [name,value]of [['--glass-panel',color],['--glass-strip',color],['--tab-bar-height','36px']])group.style.setProperty(name,value);
      group.innerHTML='<div class="panel-preview" style="border-radius:0"><nav class="dock-tabs"><div class="tab-list">'+Array.from({length:6},(_,i)=>'<button class="dock-tab icon-only-tab" aria-selected="'+(i===1)+'"></button>').join('')+'</div><button class="panel-grip"></button></nav><div class="panel-frame"></div></div>';
      if(drawer){group.innerHTML=group.innerHTML.replace('class="dock-tabs"','class="drawer-tabs" style="inset:0 0 auto;height:36px"').replace('class="tab-list"','class="drawer-tab-strip"');group.firstChild.style.background='transparent';}
      root.append(group);
    }
    for(const tab of root.querySelectorAll('.dock-tab')){const parent=tab.parentElement,next=tab.nextSibling;tab.remove();tabHover(tab,{hover_bottom:[0,0]});parent.insertBefore(tab,next);}
    document.body.append(root);
    await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
    for(const group of root.children){
      const sample=JSON.parse(group.dataset.sample),strip=group.querySelector('.tab-list,.drawer-tab-strip');if(sample.fractional)strip.scrollLeft=17.25;
      if(sample.dragged){
        const source=strip.querySelector('[aria-selected=true]'),preview=source.cloneNode(true),overlay=document.createElement('div'),rect=source.getBoundingClientRect(),origin=group.getBoundingClientRect();
        prepareTabPreview(preview);source.classList.add('dragged-tab-source');overlay.className='tab-slide-overlay';preview.classList.add('dragged-tab-preview');if(!sample.drawer)preview.classList.add('docked-tab-preview');
        Object.assign(overlay.style,{position:'absolute',left:'0',top:'0',width:strip.clientWidth+'px',height:'36px'});
        Object.assign(preview.style,{left:(rect.x-origin.x+12.25)+'px',top:'0',width:rect.width+'px',height:'36px'});overlay.append(preview);group.append(overlay);
      }
    }
  })()`);
  try {
    for(const round of [false,true]){
      if(round)await evaluate("(async()=>{const {tabHover}=await import('./workspace-chrome.js');document.querySelectorAll('#tab-surface-fixture .dock-tab').forEach(tab=>{tab.style.cornerShape='round';tab.style.setProperty('--surface-radius','9.72px');tabHover(tab,{hover_bottom:[1,1]})});})()");
      for(const unequal of [false,true]){
        await evaluate(`document.querySelectorAll('#tab-surface-fixture>[data-sample]').forEach(group=>{const sample=JSON.parse(group.dataset.sample);group.style.setProperty('--glass-strip','rgba('+[sample.value,sample.value,sample.value,${unequal}&&sample.alpha<1?.35:sample.alpha].join(',')+')')})`);
        await settle();
        const shot=(await call('Page.captureScreenshot',{format:'png',captureBeyondViewport:true,clip:{x:0,y:0,width:920,height:680,scale:1}})).data;
        const result=await evaluate(`(async()=>{
          const image=new Image();image.src='data:image/png;base64,${shot}';await image.decode();
          const canvas=new OffscreenCanvas(image.width,image.height),context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(image,0,0);
          const pixels=context.getImageData(0,0,image.width,image.height).data,scale=image.width/920,errors=[];let compared=0;
          for(const group of document.querySelectorAll('#tab-surface-fixture>[data-sample]')){
            const sample=JSON.parse(group.dataset.sample),rect=group.getBoundingClientRect(),selected=(group.querySelector('.tab-slide-overlay>.dock-tab')||group.querySelector('.dock-tab[aria-selected=true]')).getBoundingClientRect(),radius=${round?9.72:18},power=${round?2:4};
            const inside=(x,y)=>{
              if(y<selected.y||y>=selected.bottom)return false;
              if(x>=selected.x&&x<selected.right){if(y>=selected.y+radius)return true;const dx=x<selected.x+radius?selected.x+radius-x:x>selected.right-radius?x-selected.right+radius:0;return (dx/radius)**power+((selected.y+radius-y)/radius)**power<=1;}
              if(y<selected.bottom-6||x<selected.x-6||x>selected.right+6)return false;
              const dx=x<selected.x?x-selected.x+6:selected.right+6-x;return (dx/6)**power+((y-selected.bottom+6)/6)**power>=1;
            };
            for(let y=Math.ceil((rect.y+1)*scale);y<Math.floor((rect.y+35)*scale);y++)for(let x=Math.ceil((rect.x+9)*scale);x<Math.floor((rect.right-22)*scale);x++){
              const offset=(y*image.width+x)*4,actual=Array.from(pixels.slice(offset,offset+3));compared++;
              let coverage=1,boundary=false;
              if(${unequal}&&sample.alpha<1&&!sample.drawer){
                const corners=[-1,.5,2].flatMap(dx=>[-1,.5,2].map(dy=>[dx,dy])).map(([dx,dy])=>inside((x+dx)/scale,(y+dy)/scale));coverage=corners[0]?1:0;boundary=corners.some(value=>value!==corners[0]);
              }
              const alpha=${unequal}&&sample.alpha<1&&!sample.drawer ? .35+coverage*(sample.alpha-.35):sample.alpha,expected=[255,0,255].map(background=>Math.round(sample.value*alpha+background*(1-alpha)));
              const endpoints=[.35,sample.alpha].map(alpha=>[255,0,255].map(background=>Math.round(sample.value*alpha+background*(1-alpha))));
              if(actual.some((value,k)=>boundary?value<Math.min(endpoints[0][k],endpoints[1][k])-2||value>Math.max(endpoints[0][k],endpoints[1][k])+2:Math.abs(value-expected[k])>2)&&errors.length<12)errors.push({...sample,x:x/scale,y:y/scale,expected,actual});
            }
          }
          return{compared,errors,scale};
        })()`);
        assert.deepEqual(result.errors,[],`selected tab contours preserve complete surface coverage: ${JSON.stringify(result)}`);
        console.log(JSON.stringify({tabSurfaceCoverage:true,themes:['light','dark'],alpha:[1,.65],drawers:true,fractionalScroll:true,dragPreviews:true,round,unequal,...result}));
      }
    }
  } finally { await evaluate("(async()=>{const root=document.querySelector('#tab-surface-fixture');if(root){(await import('./workspace-chrome.js')).releaseTabSurfaces(root);root.remove()}})()"); }
}

export async function checkTabDragCorner({call,evaluate,settle}) {
  const saved=await evaluate('layerApp.state().workspace'),themeSaved=await evaluate('document.body.dataset.theme'),bodyStyle=await evaluate('document.body.getAttribute("style")');
  const fixture=structuredClone(saved),panels=['brushes','sizes','layers','properties'];
  Object.assign(fixture.layout,{bands:[{id:40,edge:'left',extent:280,root:{kind:'tabs',id:41,panels,active:'sizes',tab_style:'icon'}}],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],fit_height_groups:[],column_stacks:[],next_id:Math.max(42,fixture.layout.next_id)});
  fixture.zen_mode=false;
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();};
  let down=false,point;
  const input=async(type,p=point)=>{point=p;await call('Input.dispatchMouseEvent',{type:{down:'mousePressed',move:'mouseMoved',up:'mouseReleased'}[type],...p,button:'left',buttons:type==='up'?0:1,clickCount:1});down=type!=='up';};
  const sample=async clip=>{
    const shot=(await call('Page.captureScreenshot',{format:'png',clip:{x:clip.x,y:clip.y,width:40,height:40,scale:1}})).data;
    return evaluate(`(async()=>{const image=new Image();image.src='data:image/png;base64,${shot}';await image.decode();const canvas=new OffscreenCanvas(image.width,image.height),context=canvas.getContext('2d',{willReadFrequently:true});context.drawImage(image,0,0);const x=Math.floor((1.5+${clip.groupX}-${clip.x})*image.width/40),y=Math.floor((1.5+${clip.groupY}-${clip.y})*image.height/40);return Array.from(context.getImageData(x,y,1,1).data.slice(0,3))})()`);
  };
  try {
    for(const theme of ['light','dark'])for(const [panel,dx] of [['sizes',36],['layers',-36]]){
      await send({type:'restore_workspace',workspace:fixture});await send({type:'set_theme',theme});
      const geometry=await evaluate(`(()=>{const workspace=document.querySelector('#workspace'),group=document.querySelector('.dock-group[data-group="41"]'),tab=group.querySelector('.dock-tab[data-panel="${panel}"]'),g=group.getBoundingClientRect(),w=workspace.getBoundingClientRect(),t=tab.getBoundingClientRect(),backdrop=document.createElement('div');Object.assign(backdrop.style,{position:'absolute',left:g.x-w.x+'px',top:g.y-w.y+'px',width:g.width+'px',height:g.height+'px',background:'rgb(255,0,255)',zIndex:'0',pointerEvents:'none'});backdrop.id='tab-drag-corner-backdrop';workspace.append(backdrop);group.style.boxShadow='none';document.body.style.setProperty('--glass-strip','rgb(40,40,40)');document.body.style.setProperty('--glass-panel','rgb(80,80,80)');return{groupX:g.x,groupY:g.y,x:Math.floor(g.x),y:Math.floor(g.y),start:{x:t.x+t.width/2,y:t.y+t.height/2}}})()`);
      await settle();
      const before=await sample(geometry);
      assert.ok(before[0]>200&&before[1]<30&&before[2]>200,`${theme} ${panel}: the rounded corner exposes the backdrop: ${before}`);
      await input('down',geometry.start);await input('move',{x:geometry.start.x+dx,y:geometry.start.y});await settle();
      assert.equal(await evaluate("document.querySelectorAll('.tab-slide-overlay').length"),1,`${theme} ${panel}: tab slide starts`);
      const during=await sample(geometry);
      assert.deepEqual(during,before,`${theme} ${panel}: dragging preserves the group upper-left corner`);
      await input('up');await evaluate("document.querySelector('#tab-drag-corner-backdrop')?.remove()");
    }
  } finally {
    if(down)await input('up');
    await evaluate(`(()=>{document.querySelector('#tab-drag-corner-backdrop')?.remove();const style=${JSON.stringify(bodyStyle)};if(style===null)document.body.removeAttribute('style');else document.body.setAttribute('style',style)})()`);
    await send({type:'restore_workspace',workspace:saved});await send({type:'set_theme',theme:themeSaved});
  }
}

export async function checkTabHoverContours({call,evaluate,settle}) {
  await checkTabSurfaceCoverage({call,evaluate,settle});
  const dir='artifacts/panel-tab-hover/web';await mkdir(dir,{recursive:true});
  const send=async action=>{await evaluate(`layerApp.dispatch(${JSON.stringify(action)})`);await settle();await evaluate('new Promise(r=>setTimeout(r,260))');};
  const move=async point=>{await call('Input.dispatchMouseEvent',{type:'mouseMoved',...point});await settle();};
  const shot=async()=> (await call('Page.captureScreenshot',{format:'png'})).data;
  const saved=await evaluate('layerApp.state().workspace'),fixture=structuredClone(saved),panels=['brushes','sizes','layers','properties'];
  Object.assign(fixture.layout,{bands:[{id:40,edge:'left',extent:300,root:{kind:'tabs',id:41,panels,active:panels[0],tab_style:'icon'}}],floating:[],collapsed:[],column_scroll:[],fit_tab_groups:[],fit_height_groups:[],column_stacks:[],next_id:Math.max(42,fixture.layout.next_id)});fixture.zen_mode=false;
  const inspect=async(selector,active,index,clipped=false)=>{
    const geometry=await evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)}),r=n.getBoundingClientRect(),s=n.parentElement.getBoundingClientRect(),c=getComputedStyle(n);return{rect:{x:r.x,y:r.y,right:r.right,bottom:r.bottom},clip:{x:s.x,right:s.right},color:c.color,radii:[parseFloat(c.borderBottomLeftRadius),parseFloat(c.borderBottomRightRadius)],squircle:['squircle','superellipse(2)'].includes(c.cornerBottomLeftShape||c.cornerShape),published:[Number(n.dataset.hoverLeft),Number(n.dataset.hoverRight)]}})()`);
    const expected=index===active?[0,0]:[index===0?0:index-1===active?6:-6,index===panels.length-1?0:index+1===active?6:-6];
    assert.deepEqual(geometry.published,expected,'neighbor roles determine both lower corners');
    assert.deepEqual(geometry.radii,expected.map(v=>Math.max(0,v)));
    await move({x:800,y:500});const before=await shot();
    const r=geometry.rect,c=geometry.clip;
    await move({x:(Math.max(r.x,c.x)+Math.min(r.right,c.right))/2,y:r.bottom-18});
    assert.equal(await evaluate(`document.querySelector(${JSON.stringify(selector)}).matches(':hover')`),true);
    const after=await shot();
    const comparison=await evaluate(`(async()=>{
      const load=async s=>{const i=new Image();i.src='data:image/png;base64,'+s;await i.decode();const c=new OffscreenCanvas(i.width,i.height),x=c.getContext('2d',{willReadFrequently:true});x.drawImage(i,0,0);return x.getImageData(0,0,c.width,c.height).data};
      const a=await load(${JSON.stringify(before)}),b=await load(${JSON.stringify(after)}),g=${JSON.stringify(geometry)},roles=${JSON.stringify(expected)},selected=${index===active},r=g.rect,clip=g.clip,w=innerWidth,scale=devicePixelRatio,text=g.color.match(/[\\d.]+/g).slice(0,3).map(Number),power=g.squircle?4:2;
      const inside=(x,y)=>{
        if(selected)return false;
        if(x<clip.x||x>=clip.right||y<r.bottom-6||y>=r.bottom)return false;
        if(x>=r.x&&x<r.right){
          for(const [edge,side]of [[r.x,0],[r.right,1]])if(roles[side]>0){const radius=6,dx=side?x-(edge-radius):edge+radius-x,dy=y-(r.bottom-radius);if(dx>0&&dy>0&&(dx/radius)**power+(dy/radius)**power>1)return false;}
          return true;
        }
        const side=x<r.x?0:1,edge=side?r.right:r.x;
        if(roles[side]!==-6||Math.abs(x-edge)>6)return false;
        const dx=side?edge+6-x:x-edge+6,dy=y-r.bottom+6;
        return (dx/6)**4+(dy/6)**4>=1;
      };
      const errors=[];let compared=0,changed=0,untouched=0;
      for(const edge of [r.x,r.right])for(let py=Math.floor((r.bottom-6)*scale);py<Math.ceil(r.bottom*scale);py++)for(let px=Math.floor((edge-6)*scale);px<Math.ceil((edge+6)*scale);px++){
        if(px<0||px>=w*scale||py<0)continue;let coverage=0;for(let sy=0;sy<8;sy++)for(let sx=0;sx<8;sx++)coverage+=inside((px+(sx+.5)/8)/scale,(py+(sy+.5)/8)/scale)?1/64:0;
        const offset=(py*w*scale+px)*4;compared++;let error=0,delta=0;for(let k=0;k<3;k++){error=Math.max(error,Math.abs(b[offset+k]-(a[offset+k]*(1-.08*coverage)+text[k]*.08*coverage)));delta=Math.max(delta,Math.abs(b[offset+k]-a[offset+k]));}if(delta>1)changed++;else untouched++;
        const cx=(px+.5)/scale,cy=(py+.5)/scale,center=inside(cx,cy),boundary=(coverage>0&&coverage<1)||[[-1,-1],[-1,1],[1,-1],[1,1]].some(([dx,dy])=>inside(cx+dx/scale,cy+dy/scale)!==center);
        if(error>(boundary?5:2)&&errors.length<12)errors.push({x:px/scale,y:py/scale,coverage,before:[...a.slice(offset,offset+3)],after:[...b.slice(offset,offset+3)],error});
      }
      return{compared,changed,untouched,errors};})()`);
    assert.deepEqual(comparison.errors,[],`${JSON.stringify({active,index,clipped,expected})}: complete lower corner regions: ${JSON.stringify(comparison)}`);
    if(index===active)assert.equal(comparison.changed,0,'hovering the active tab preserves its feet');else assert.ok(comparison.changed>0);assert.ok(comparison.untouched>0,'active feet and pixels beyond the hover contour stay unchanged');
    return{expected,...comparison};
  };
  try{
    for(const fallback of [false,true])for(const theme of ['light','dark'])for(const drawer of [false,true]){
      await send({type:'restore_workspace',workspace:fixture});await send({type:'set_theme',theme});
      if(drawer){await send({type:'customize',action:{type:'set_column_collapsed',group:41,collapsed:true}});await send({type:'customize',action:{type:'set_column_drawers',column:41,drawers:true}});await evaluate(`document.querySelector('.collapsed-column [data-panel="brushes"]').click()`);await settle();await evaluate('new Promise(r=>setTimeout(r,300))');}
      if(fallback)await evaluate(`(()=>{const s=document.createElement('style');s.id='tab-hover-round-fixture';s.textContent='.dock-tab,.dock-tab::before,.dock-tab::after{corner-shape:round!important}.dock-tab{--corner-fit:.54}';document.head.append(s)})()`);
      const strip=drawer?'.drawer-tab-strip':'.dock-group[data-group="41"] .tab-list';let compared=0,pairs=0;
      const cacheWrites=await evaluate(`(async()=>{const n=document.querySelector(${JSON.stringify(strip)}+' .dock-tab'),original=n.style.setProperty;let writes=0;n.style.setProperty=function(...args){writes++;return original.apply(this,args)};try{const {tabHover}=await import('./workspace-chrome.js');tabHover(n,{hover_bottom:[Number(n.dataset.hoverLeft),Number(n.dataset.hoverRight)]});return writes}finally{n.style.setProperty=original}})()`);
      assert.equal(cacheWrites,0,'unchanged hover roles preserve cached CSS paths');
      for(const active of fallback?[1,2]:[0,1,2,3]){
        await send({type:'select_panel_tab',group:41,panel:panels[active]});
        for(let hover=0;hover<panels.length;hover++){if(fallback&&hover===active)continue;const result=await inspect(`${strip} [data-panel="${panels[hover]}"]`,active,hover);compared+=result.compared;pairs++;}
      }
      await send({type:'select_panel_tab',group:41,panel:'properties'});
      await evaluate(`Object.assign(document.querySelector(${JSON.stringify(strip)}).style,{maxWidth:'84px',flex:'none'});document.querySelector(${JSON.stringify(strip)}).scrollLeft=43`);await settle();
      for(const hover of [1,2])await inspect(`${strip} [data-panel="${panels[hover]}"]`,3,hover,true);
      await send({type:'select_panel_tab',group:41,panel:'brushes'});await evaluate(`document.querySelector(${JSON.stringify(strip)}).scrollLeft=43`);await settle();
      await inspect(`${strip} [data-panel="properties"]`,0,3,true);
      const capture=await shot();await writeFile(`${dir}/${theme}-${drawer?'drawer':'docked'}${fallback?'-round':''}-scrolled.png`,Buffer.from(capture,'base64'));
      console.log(JSON.stringify({theme,drawer,fallback,activeTabs:fallback?2:4,hoverPairs:pairs,compared,cacheWrites,fullLowerCorners:true,activeFeetUntouched:true,stripEnds:true,scrollClipping:true}));
      await evaluate("document.querySelector('#tab-hover-round-fixture')?.remove()");
    }
  }finally{await evaluate("document.querySelector('#tab-hover-round-fixture')?.remove()");await move({x:800,y:500});await send({type:'restore_workspace',workspace:saved});}
}
