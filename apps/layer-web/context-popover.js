import {addScrollbar} from './scrollbars.js';

const MARGIN=0, ARROW=12, RADIUS=15;

export function contextPopoverLayout({anchor,size,viewport}) {
  const x=viewport.x??0,y=viewport.y??0;
  const below=Math.max(0,y+viewport.height-MARGIN-anchor.y-anchor.height-ARROW);
  const above=Math.max(0,anchor.y-y-MARGIN-ARROW);
  const side=size.height<=below||below>=above?'bottom':'top';
  const maxHeight=side==='bottom'?below:above,height=Math.min(size.height,maxHeight);
  const width=Math.min(size.width,Math.max(0,viewport.width-2*MARGIN));
  const tip=anchor.x+anchor.width/2;
  const left=Math.max(x+MARGIN,Math.min(tip-width/2,x+viewport.width-width-MARGIN));
  const top=side==='bottom'?anchor.y+anchor.height+ARROW:anchor.y-ARROW-height;
  const arrowTip=Math.max(0,Math.min(tip-left,width));
  const corner=Math.min(RADIUS+ARROW,width/2);
  return {left,top:Math.max(y+MARGIN,Math.min(top,y+viewport.height-height-MARGIN)),maxHeight,side,
    arrowTip,arrowBase:Math.max(corner,Math.min(arrowTip,width-corner))};
}

export function createContextPopover({element,classes='',popover='manual'}) {
  const root=element('div',`panel-context-menu ${classes}`.trim()),body=element('div','context-menu-body');
  root.popover=popover;root.setAttribute('role','menu');root.append(body);
  let scroll;
  root.addEventListener('toggle',event=>{
    if(event.newState==='open')scroll??=addScrollbar(body,root);
    else {scroll?.disposeScrollbar();scroll=null;}
  });
  const clearHighlight=()=>body.querySelector('[data-menu-initial]')?.removeAttribute('data-menu-initial');
  body.addEventListener('pointermove',clearHighlight);
  body.addEventListener('focusin',clearHighlight);
  body.addEventListener('scroll',clearHighlight);
  function place(anchor,initial=false) {
    const view=globalThis.visualViewport;
    const viewport={x:view?.offsetLeft??0,y:view?.offsetTop??0,width:view?.width??innerWidth,height:view?.height??innerHeight};
    root.style.maxWidth=`${Math.max(0,viewport.width-2*MARGIN)}px`;
    body.style.maxHeight='';
    const size=root.getBoundingClientRect();
    const layout=contextPopoverLayout({anchor,size,viewport});
    body.style.maxHeight=`${layout.maxHeight}px`;
    root.style.left=`${layout.left}px`;root.style.top=`${layout.top}px`;
    root.dataset.side=layout.side;
    root.style.setProperty('--menu-arrow-tip',`${layout.arrowTip}px`);
    root.style.setProperty('--menu-arrow-base',`${layout.arrowBase}px`);
    if(initial){clearHighlight();body.querySelector('button:not(:disabled)')?.setAttribute('data-menu-initial','');}
    return layout;
  }
  return {root,body,place};
}
