function reversed(commands) {
  const edges=[];let point;
  for(const command of commands) {
    const [op,...p]=command;
    if(op===0)point=p.slice(0,2);
    else if(op===1||op===2) {edges.push({command,point});point=p.slice(op===1?0:4,op===1?2:6);}
  }
  return [[0,...point,0,0,0,0],...edges.reverse().map(({command:c,point:p})=>
    c[0]===1?[1,...p,0,0,0,0]:[2,c[3],c[4],c[1],c[2],...p]),[3,0,0,0,0,0,0]];
}

export function svgPath(commands,offset={x:0,y:0}) {
  return commands.map(c=>{
    const count=c[0]===2?6:c[0]===3?0:2;
    return ['M','L','C','Z'][c[0]]+c.slice(1,1+count).map((v,i)=>
      Math.round((v/1024+(i%2?offset.y:offset.x))*1000)/1000).join(' ');
  }).join(' ');
}

export function panelVectorPaths(layout,projection) {
  const groups=new Map(layout.groups.map(g=>[g.id,g]));
  const triangles=new Float32Array(projection.surfaces.reduce((n,s)=>n+(s.triangles?.length||0)*7,0));
  let index=0;
  const columns=projection.sources.map(source=>{
    const surfaces=projection.surfaces.filter(s=>s.column===source.id);
    const bodies=surfaces.map(s=>groups.get(s.group).bounds);
    const left=Math.min(source.bounds.x,...bodies.map(b=>b.x)),top=Math.min(source.bounds.y,...bodies.map(b=>b.y));
    const right=Math.max(source.bounds.x+source.bounds.width,...bodies.map(b=>b.x+b.width));
    const bottom=Math.max(source.bounds.y+source.bounds.height,...bodies.map(b=>b.y+b.height));
    const offset=b=>({x:b.x-left,y:b.y-top});
    const parts=[svgPath(source.reverse?reversed(source.commands):source.commands,offset(source.bounds))];
    const receivers=surfaces.map(s=>{
      const group=groups.get(s.group),b=group.bounds;
      parts.push(svgPath(s.commands,offset(b)));
      for(const triangle of s.triangles||[])for(let i=0;i<7;i++)
        triangles[index++]=i===6?triangle[i]:triangle[i]/1024+(i%2?b.y:b.x);
      const original=s.commands.findLastIndex(c=>c[0]===0),body=s.commands.slice(original);
      return {group:s.group,bounds:b,tabs:group.tabs_visible,body:svgPath(body),header:svgPath(body,offset(b))};
    });
    return {id:source.id,bounds:{x:left,y:top,width:right-left,height:bottom-top},d:parts.join(' '),
      source:{bounds:source.bounds,d:svgPath(source.commands)},receivers};
  });
  return {columns,triangles,buffers:[triangles]};
}
