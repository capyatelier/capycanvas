use super::*;
use super::erase::{Edge, Edges, crossing_stops, path_edges};
use std::collections::{HashMap, VecDeque};

pub struct FaceLine<'a> { pub path: &'a PathItem, pub offset: [f64; 2] }
pub struct VectorFace { pub geometry: PathGeometry, pub boundary: BTreeSet<usize> }
#[derive(Clone)]
struct Line { edges: Arc<[Edge]>, source: usize, other: Option<usize>, base: u32, closed: bool }
struct Link { ends: [usize; 2], points: Vec<[f64; 2]>, sources: BTreeSet<usize> }
struct Cycle { points: Vec<[f64; 2]>, sources: BTreeSet<usize>, component: usize, area: f64 }
struct Cache { paths: Vec<Option<std::ops::Range<usize>>>, lines: Vec<Line>, trees: Vec<Edges>, next: usize, maxima: Vec<f64>, lengths: HashMap<usize, Box<[f64]>> }
impl Cache {
    fn new(lines:&[FaceLine<'_>])->Self {
        Self {paths:vec![None;lines.len()],lines:Vec::new(),trees:Vec::new(),next:0,lengths:HashMap::new(),maxima:lines.iter().map(|s|
            s.path.style.strokes().map(|stroke|stroke.width*0.5*stroke.profile.as_ref().map_or(1.,|p|p.max_factor())).fold(0.,f64::max)).collect()}
    }
    fn length(&self,line:&Line,index:usize)->f64 {
        if line.edges.len()>1 {self.lengths[&line.edges[0].subpath][index]}else if index==0 {0.}else {
            let edge=line.edges[0];(edge.b[0]-edge.a[0]).hypot(edge.b[1]-edge.a[1])
        }
    }
    fn near(&self,a:[f64;2],b:[f64;2],active:Option<&[usize]>,emit:&mut impl FnMut(&Edge)) {
        match active {
            None=>for tree in &self.trees {tree.near(a,b,emit);},
            Some(active)=>for tree in &self.trees {tree.near(a,b,&mut |edge|if active[edge.subpath]!=usize::MAX {emit(edge);});},
        }
    }
}


fn inside(points: &[[f64; 2]], p: [f64; 2]) -> bool {
    points.iter().zip(points.iter().cycle().skip(1)).filter(|(a,b)|
        (a[1]>p[1])!=(b[1]>p[1]) && a[0]+(p[1]-a[1])*(b[0]-a[0])/(b[1]-a[1])>p[0]).count()%2!=0
}
fn bounds(points: impl Iterator<Item=[f64;2]>) -> [f64;4] {
    points.fold([f64::INFINITY,f64::INFINITY,f64::NEG_INFINITY,f64::NEG_INFINITY],|b,p|
        [b[0].min(p[0]),b[1].min(p[1]),b[2].max(p[0]),b[3].max(p[1])])
}
fn radius(source:&FaceLine<'_>,base:u32,place:f64)->f64 {
    source.path.style.strokes().map(|s|s.width*0.5*s.profile.as_ref().map_or(1.,|p| {
        let width=p.at(base+place.floor() as u32,place.fract());width[0].max(width[1])
    })).fold(0.,f64::max)
}
fn translated(mut edge:Edge,offset:[f64;2])->Edge {
    let map=|p:[f64;2]|[p[0]+offset[0],p[1]+offset[1]];
    edge.a=map(edge.a);edge.b=map(edge.b);edge.segment=Segment {from:map(edge.segment.from),control:edge.segment.control.map(map),to:map(edge.segment.to)};edge
}
fn at(edges:&[Edge],place:f64)->[f64;2] {
    let index=edges.partition_point(|edge|edge.base+edge.to<place).min(edges.len()-1);
    let edge=edges[index];edge.segment.point((place-edge.base).clamp(edge.from,edge.to))
}
fn piece(edges:&[Edge],from:f64,to:f64)->Vec<[f64;2]> {
    let mut points=vec![at(edges,from)];
    points.extend(edges.iter().filter(|e|e.base+e.to>from && e.base+e.to<to).map(|e|e.b));
    points.push(at(edges,to));points
}

struct End {line:usize,end:bool,point:[f64;2],direction:[f64;2],radius:f64}
struct Choice {distance:f64,line:usize,place:f64,point:[f64;2],end:Option<usize>}
fn choose(nearest:&mut Option<Choice>,candidate:Choice) {
    if nearest.as_ref().is_none_or(|old| {
        let tie=(candidate.distance-old.distance).abs()<=0.05;
        if tie && candidate.end.is_some()!=old.end.is_some() {candidate.end.is_some()}else{candidate.distance<old.distance}
    }) {*nearest=Some(candidate);}
}
fn ray_hit(e:[f64;2],d:[f64;2],edge:&Edge)->Option<(f64,f64)> {
    let v=[edge.b[0]-edge.a[0],edge.b[1]-edge.a[1]];let q=[edge.a[0]-e[0],edge.a[1]-e[1]];
    let cross=|a:[f64;2],b:[f64;2]|a[0]*b[1]-a[1]*b[0];let denominator=cross(d,v);
    if denominator.abs()<1e-12 {
        if cross(q,d).abs()>1e-9 {return None;}
        return [(q[0]*d[0]+q[1]*d[1],0.),((edge.b[0]-e[0])*d[0]+(edge.b[1]-e[1])*d[1],1.)]
            .into_iter().filter(|(s,_)|*s>0.).min_by(|a,b|a.0.total_cmp(&b.0));
    }
    let (s,u)=(cross(q,v)/denominator,cross(q,d)/denominator);
    (s>0. && (0. ..=1.).contains(&u)).then_some((s,u))
}

fn graph(sources:&[FaceLine<'_>],selected:&[usize],point:Option<[f64;2]>,reach:f64,cache:&mut Cache)->Vec<VectorFace> {
    let mut lines=Vec::new();let mut added=Vec::new();
    for &source in selected {
        if cache.paths[source].is_none() {
            let start=cache.lines.len();let mut base=0;
            for subpath in sources[source].path.geometry.subpaths() {
                let edges=path_edges(subpath,cache.next,0.05).into_iter().map(|e|translated(e,sources[source].offset)).collect::<Vec<_>>();
                if !edges.is_empty() {
                    added.extend(&edges);cache.next+=1;
                    if !subpath.closed && edges.len()>1 {
                        let lengths=std::iter::once(0.).chain(edges.iter().scan(0.,|length,edge| {
                            *length+=(edge.b[0]-edge.a[0]).hypot(edge.b[1]-edge.a[1]);Some(*length)
                        })).collect();cache.lengths.insert(edges[0].subpath,lengths);
                    }
                    cache.lines.push(Line {edges:edges.into(),source,other:None,base,closed:subpath.closed});
                }
                base+=subpath.segment_count() as u32;
            }
            cache.paths[source]=Some(start..cache.lines.len());
        }
        lines.extend(cache.lines[cache.paths[source].as_ref().unwrap().clone()].iter().cloned());
    }
    if lines.is_empty() {return Vec::new();}
    if !added.is_empty() {cache.trees.push(Edges::new(added));}
    let mut lookup=vec![usize::MAX;cache.next];
    for (id,line) in lines.iter().enumerate() {lookup[line.edges[0].subpath]=id;}
    let active=(lines.len()!=cache.next).then_some(lookup.as_slice());
    let maxima=&cache.maxima;
    let maximum=selected.iter().map(|i|maxima[*i]).fold(0.,f64::max);
    let isolated=lines.iter().map(|line| {
        if line.closed || line.edges.len()!=1 {return false;}
        let edge=line.edges[0];let reach=maxima[line.source]+maximum+0.5+0.05;let mut other=false;
        cache.near([edge.a[0].min(edge.b[0])-reach,edge.a[1].min(edge.b[1])-reach],
            [edge.a[0].max(edge.b[0])+reach,edge.a[1].max(edge.b[1])+reach],active,&mut |edge|other|=edge.subpath!=line.edges[0].subpath);
        !other
    }).collect::<Vec<_>>();
    let mut joins=Vec::new();let mut cuts=vec![Vec::new();lines.len()];let mut joined=vec![[false;2];lines.len()];
    for (id,line) in lines.iter().enumerate().filter(|(id,line)|!line.closed && !isolated[*id]) {
        for end in [false,true] {
            let edge=if end {line.edges.last().unwrap()}else{&line.edges[0]};
            let place=edge.base+if end {edge.to}else{edge.from};let e=if end {edge.b}else{edge.a};
            let reach=radius(&sources[line.source],line.base,place);let search=reach+maximum+0.5;
            let mut nearest:Option<(f64,usize,f64,[f64;2])>=None;
            cache.near([e[0]-search,e[1]-search],[e[0]+search,e[1]+search],active,&mut |candidate| {
                let other_id=lookup[candidate.subpath];let other=&lines[other_id];
                let d=[candidate.b[0]-candidate.a[0],candidate.b[1]-candidate.a[1]];let square=d[0]*d[0]+d[1]*d[1];
                let u=if square==0. {0.}else{((e[0]-candidate.a[0])*d[0]+(e[1]-candidate.a[1])*d[1])/square}.clamp(0.,1.);
                let distance=(candidate.a[0]+d[0]*u-e[0]).hypot(candidate.a[1]+d[1]*u-e[1]);
                if distance>reach+maxima[other.source]+0.5+0.05 || nearest.as_ref().is_some_and(|n|distance>n.0+0.05) {return;}
                let (t,distance)=super::points::closest_segment(candidate.segment,e,candidate.from,candidate.to);
                let place=candidate.base+t;let q=candidate.segment.point(t);
                let limit=reach+radius(&sources[other.source],other.base,place)+0.5;
                if candidate.subpath==line.edges[0].subpath {
                    let i=candidate.index;let along=cache.length(line,i)+(q[0]-candidate.a[0]).hypot(q[1]-candidate.a[1]).min(cache.length(line,i+1)-cache.length(line,i));
                    if (if end {cache.length(line,line.edges.len())-along}else{along})<=2.*limit {return;}
                }
                if distance<=limit
                    && nearest.as_ref().is_none_or(|n|distance<n.0) {
                    nearest=Some((distance,other_id,place,q));
                }
            });
            if let Some((distance,other,place,q))=nearest {
                joined[id][usize::from(end)]=true;cuts[other].push(place);
                if distance>1e-9 {joins.push((line.source,lines[other].source,e,q));}
            }
        }
    }
    if reach>0. {
        let mut ends=Vec::new();let mut end_ids=vec![[None;2];lines.len()];
        for (id,line) in lines.iter().enumerate().filter(|(_,line)|!line.closed) {
            for end in [false,true] {
                if joined[id][usize::from(end)] {continue;}
                let edge=if end {line.edges.iter().rev().find(|e|e.a!=e.b)}else{line.edges.iter().find(|e|e.a!=e.b)};
                let Some(edge)=edge else{continue;};
                let (a,b)=if end {(edge.b,edge.a)}else{(edge.a,edge.b)};
                let length=(a[0]-b[0]).hypot(a[1]-b[1]);
                let place=if end {line.edges.last().unwrap().base+line.edges.last().unwrap().to}else{line.edges[0].base+line.edges[0].from};
                let point=at(&line.edges,place);end_ids[id][usize::from(end)]=Some(ends.len());
                ends.push(End {line:id,end,point,direction:[(a[0]-b[0])/length,(a[1]-b[1])/length],radius:radius(&sources[line.source],line.base,place)});
            }
        }
        let choices=ends.iter().enumerate().map(|(id,end)| {
            let mut nearest=None;let e=end.point;let d=end.direction;
            cache.near([e[0]-reach,e[1]-reach],[e[0]+reach,e[1]+reach],active,&mut |edge| {
                let line_id=lookup[edge.subpath];let line=&lines[line_id];
                for other_end in [false,true] {
                    if if other_end {edge.index+1!=line.edges.len()}else{edge.index!=0} {continue;}
                    let Some(other)=end_ids[line_id][usize::from(other_end)].filter(|other|*other!=id) else{continue;};
                    let q=ends[other].point;let delta=[q[0]-e[0],q[1]-e[1]];let distance=delta[0].hypot(delta[1]);
                    let dot=delta[0]*d[0]+delta[1]*d[1];
                    if distance>0. && distance<=reach && dot>=0. && 2.*dot*dot>=distance*distance {
                        let place=if other_end {line.edges.last().unwrap().base+line.edges.last().unwrap().to}else{line.edges[0].base+line.edges[0].from};
                        choose(&mut nearest,Choice {distance,line:line_id,place,point:q,end:Some(other)});
                    }
                }
                if let Some((s,_))=ray_hit(e,d,edge) && s>0. && s<=reach {
                    let hit=[e[0]+d[0]*s,e[1]+d[1]*s];
                    let (t,_)=super::points::closest_segment(edge.segment,hit,edge.from,edge.to);let place=edge.base+t;
                    let point=edge.segment.point(t);let delta=[point[0]-e[0],point[1]-e[1]];let distance=delta[0].hypot(delta[1]);
                    if distance>reach || delta[0]*d[0]+delta[1]*d[1]<=0. {return;}
                    if line_id==end.line {
                        let along=cache.length(line,edge.index)+(point[0]-edge.a[0]).hypot(point[1]-edge.a[1]).min(cache.length(line,edge.index+1)-cache.length(line,edge.index));
                        if (if end.end {cache.length(line,line.edges.len())-along}else{along})<=2.*(end.radius+reach) {return;}
                    }
                    choose(&mut nearest,Choice {distance,line:line_id,place,point,end:None});
                }
            });nearest
        }).collect::<Vec<_>>();
        for (id,choice) in choices.iter().enumerate() {
            let Some(choice)=choice else{continue;};let end=&ends[id];
            if let Some(other)=choice.end && (other<id || choices[other].as_ref().is_none_or(|c|c.end!=Some(id))) {continue;}
            joined[end.line][usize::from(end.end)]=true;cuts[choice.line].push(choice.place);
            if let Some(other)=choice.end {joined[ends[other].line][usize::from(ends[other].end)]=true;}
            joins.push((lines[end.line].source,lines[choice.line].source,end.point,choice.point));
        }
    }
    for (source,other,a,b) in joins {
        let segment=Segment {from:a,control:[a,b],to:b};let subpath=cache.next+lines.len();
        lines.push(Line {edges:vec![Edge {a,b,segment,from:0.,to:1.,base:0.,subpath,index:0}].into(),source,other:Some(other),base:0,closed:false});
        cuts.push(Vec::new());
    }
    let flattened=lines.iter().enumerate().map(|(i,l)|if isolated.get(i)==Some(&true) && !joined[i].iter().any(|joined|*joined) && cuts[i].is_empty() {Vec::new()}else{l.edges.to_vec()}).collect::<Vec<_>>();
    let crossings=crossing_stops(&flattened,&[]);
    let mut vertices=Vec::<[f64;2]>::new();let mut cells=HashMap::<[i64;2],usize>::new();
    let mut links=Vec::<Link>::new();let mut duplicate=HashMap::<Vec<[i64;2]>,usize>::new();
    for (id,line) in lines.iter().enumerate() {
        if line.other.is_none() && !line.closed && !joined[id].iter().any(|joined|*joined) && cuts[id].is_empty() && crossings[id].is_empty() {continue;}
        cuts[id].extend(&crossings[id]);cuts[id].extend([line.edges[0].base+line.edges[0].from,line.edges.last().unwrap().base+line.edges.last().unwrap().to]);
        cuts[id].sort_by(f64::total_cmp);cuts[id].dedup_by(|a,b|(*a-*b).abs()<1e-9);
        for stops in cuts[id].windows(2) {
            let mut points=piece(&line.edges,stops[0],stops[1]);
            let mut ends=[0;2];
            for (end,p) in [points[0],*points.last().unwrap()].into_iter().enumerate() {
                let key=p.map(|v|(v*GRID).round() as i64);
                let found=(-1..=1).flat_map(|y|(-1..=1).map(move|x|[key[0]+x,key[1]+y])).filter_map(|key|cells.get(&key).copied())
                    .find(|&i|(vertices[i][0]-p[0]).hypot(vertices[i][1]-p[1])<=1./GRID);
                ends[end]=found.unwrap_or_else(|| {let i=vertices.len();vertices.push(p);cells.insert(key,i);i});
            }
            if ends[0]==ends[1] && points.len()<4 {continue;}
            points[0]=vertices[ends[0]];*points.last_mut().unwrap()=vertices[ends[1]];
            let key_points=if ends[0]<=ends[1] {points.clone()}else{points.iter().rev().copied().collect()};
            let key=key_points.iter().map(|p|p.map(|v|(v*GRID).round() as i64)).collect::<Vec<_>>();
            let mut provenance=BTreeSet::from([line.source]);provenance.extend(line.other);
            if let Some(&i)=duplicate.get(&key) {links[i].sources.extend(provenance);}
            else {duplicate.insert(key,links.len());links.push(Link {ends,points,sources:provenance});}
        }
    }
    let mut outgoing=vec![Vec::new();vertices.len()];
    for (i,link) in links.iter().enumerate() {outgoing[link.ends[0]].push(2*i);outgoing[link.ends[1]].push(2*i+1);}
    let mut degree=outgoing.iter().map(Vec::len).collect::<Vec<_>>();let mut live=vec![true;links.len()];
    let mut queue:VecDeque<_>=degree.iter().enumerate().filter(|(_,n)|**n<=1).map(|(i,_)|i).collect();
    while let Some(v)=queue.pop_front() {
        for &half in &outgoing[v] {let edge=half/2;if !live[edge] {continue;}live[edge]=false;
            for end in links[edge].ends {degree[end]-=1;if degree[end]==1 {queue.push_back(end);}}
        }
    }
    let direction=|half:usize| {let points=&links[half/2].points;
        let (a,b)=if half.is_multiple_of(2) {(points[0],points[1])}else{(points[points.len()-1],points[points.len()-2])};
        (b[1]-a[1]).atan2(b[0]-a[0])};
    for list in &mut outgoing {list.retain(|h|live[h/2]);list.sort_by(|a,b|direction(*a).total_cmp(&direction(*b)).then(a.cmp(b)));}
    let mut components=vec![usize::MAX;vertices.len()];let mut component=0;
    for v in 0..vertices.len() {if outgoing[v].is_empty() || components[v]!=usize::MAX {continue;}components[v]=component;queue.push_back(v);
        while let Some(v)=queue.pop_front() {for &half in &outgoing[v] {let other=links[half/2].ends[1-half%2];if components[other]==usize::MAX {components[other]=component;queue.push_back(other);}}}component+=1;
    }
    let mut cycles=Vec::<Cycle>::new();let mut cycle_of=vec![usize::MAX;links.len()*2];
    for first in 0..links.len()*2 {
        if !live[first/2] || cycle_of[first]!=usize::MAX {continue;}
        let mut half=first;let mut points=Vec::new();let mut provenance=BTreeSet::new();let id=cycles.len();
        loop {
            if cycle_of[half]!=usize::MAX {break;}cycle_of[half]=id;
            let link=&links[half/2];provenance.extend(&link.sources);
            if half.is_multiple_of(2) {points.extend(link.points[..link.points.len()-1].iter().copied());}
            else{points.extend(link.points[1..].iter().rev().copied());}
            let end=link.ends[1-half%2];let list=&outgoing[end];let reverse=list.iter().position(|h|*h==(half^1)).unwrap();
            half=list[(reverse+list.len()-1)%list.len()];if half==first {break;}
        }
        let area=points.iter().zip(points.iter().cycle().skip(1)).map(|(a,b)|a[0]*b[1]-b[0]*a[1]).sum::<f64>()*0.5;
        cycles.push(Cycle {points,sources:provenance,component:components[links[first/2].ends[0]],area});
    }
    let outers=if let Some(point)=point {
    let mut hits=Vec::new();
    for (index,link) in links.iter().enumerate().filter(|(i,_)|live[*i]) {
        for pair in link.points.windows(2) {let (a,b)=(pair[0],pair[1]);if (a[1]>point[1])==(b[1]>point[1]) {continue;}
            let x=a[0]+(point[1]-a[1])*(b[0]-a[0])/(b[1]-a[1]);if x<point[0] {continue;}
            let left=(b[0]-a[0])*(point[1]-a[1])-(b[1]-a[1])*(point[0]-a[0]);
            hits.push((x,2*index+usize::from(left<0.)));
        }
    }
    hits.sort_by(|a,b|a.0.total_cmp(&b.0));let mut excluded=BTreeSet::new();
    let outer=hits.into_iter().find_map(|(_,half)| {let id=cycle_of[half];let cycle=&cycles[id];
        if excluded.contains(&cycle.component) {return None;}
        if cycle.area>0. && inside(&cycle.points,point) {Some(id)}else{excluded.insert(cycle.component);None}
    });
    outer.into_iter().collect::<Vec<_>>()
    }else{cycles.iter().enumerate().filter(|(_,c)|c.area>0.).map(|(i,_)|i).collect()};
    outers.into_iter().filter_map(|outer| {
    let face=&cycles[outer];
    let holes=cycles.iter().enumerate().filter(|(_,c)|c.component!=face.component && c.area<0. && inside(&face.points,c.points[0])).map(|(i,_)|i).collect::<Vec<_>>();
    let holes=holes.iter().copied().filter(|&i|!holes.iter().any(|&j|j!=i && inside(&cycles[j].points,cycles[i].points[0]))).collect::<Vec<_>>();
    let mut boundary=face.sources.clone();let mut rings=vec![outer];rings.extend(holes);
    let subpaths=rings.into_iter().map(|i| {boundary.extend(&cycles[i].sources);
        let mut anchors=cycles[i].points.iter().copied().map(Anchor::corner).collect::<Vec<_>>();anchors.dedup_by(|a,b|a.point()==b.point());
        if anchors.first().zip(anchors.last()).is_some_and(|(a,b)|a.point()==b.point()) {anchors.pop();}
        Subpath::new(&anchors,true)
    }).collect();
    Some(VectorFace {geometry:PathGeometry::new(subpaths).ok()?,boundary})
    }).collect()
}

pub fn vector_face(lines:&[FaceLine<'_>],point:[f64;2],reach:Option<u32>)->Option<VectorFace> {
    let mut cache=Cache::new(lines);
    let paths=line_bounds(lines);
    let all=bounds(paths.iter().flat_map(|b|[[b[0],b[1]],[b[2],b[3]]]));
    let limit=[(point[0]-all[0]).abs(),(point[0]-all[2]).abs(),(point[1]-all[1]).abs(),(point[1]-all[3]).abs()].into_iter().fold(0.,f64::max)+1.;
    if !limit.is_finite() {return None;}
    let attempts=reach.map_or_else(||vec![0,16,32,48],|r|vec![r]);
    for reach in attempts {let mut half=128_f64;
    loop {
        let window=[point[0]-half,point[1]-half,point[0]+half,point[1]+half];
        let selected=selected_lines(lines,&paths,window);
        if let Some(face)=graph(lines,&selected,Some(point),f64::from(reach),&mut cache).into_iter().next() {let b=face.geometry.hull();if b[0]>window[0] && b[1]>window[1] && b[2]<window[2] && b[3]<window[3] {return Some(face);}}
        if half>=limit {break;}half=(half*2.).min(limit);
    }}None
}

fn line_bounds(lines:&[FaceLine<'_>])->Vec<[f64;4]> {
    lines.iter().map(|line| {let b=line.path.bounds();[b[0]+line.offset[0],b[1]+line.offset[1],b[2]+line.offset[0],b[3]+line.offset[1]]}).collect()
}
fn selected_lines(lines:&[FaceLine<'_>],paths:&[[f64;4]],window:[f64;4])->Vec<usize> {
    paths.iter().enumerate().filter(|(i,b)|!lines[*i].path.hidden && lines[*i].path.style.strokes().next().is_some()
        && b[0]<=window[2] && b[2]>=window[0] && b[1]<=window[3] && b[3]>=window[1]).map(|(i,_)|i).collect()
}
pub fn vector_faces(lines:&[FaceLine<'_>],window:[f64;4])->Vec<VectorFace> {
    let selected=selected_lines(lines,&line_bounds(lines),window);
    graph(lines,&selected,None,0.,&mut Cache::new(lines)).into_iter().filter(|face| {
        let b=face.geometry.hull();b[0]>=window[0] && b[1]>=window[1] && b[2]<=window[2] && b[3]<=window[3]
    }).collect()
}
