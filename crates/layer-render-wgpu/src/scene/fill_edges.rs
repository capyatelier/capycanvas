use super::*;

pub(super) const RADIUS:u32=crate::flood::UNDERFILL_HALF_PIXELS.div_ceil(2);
const HALO:u32=RADIUS;
const SIDE:u32=PAGE_SIZE+2*HALO;
pub(super) struct Capture {
    source:artwork::Capture,
    color:(wgpu::Texture,wgpu::TextureView),
    lines:(wgpu::Texture,wgpu::TextureView),
    cover:(wgpu::Texture,wgpu::TextureView),
    next:(wgpu::Texture,wgpu::TextureView),
    seed:(wgpu::Texture,wgpu::TextureView),
    line_seeds:wgpu::Texture,
    external:Vec<bool>,
    mask:wgpu::Buffer,
    precision:Option<Precision>,
    top:wgpu::Texture,
    left:wgpu::Texture,
    row:Option<u32>,
    previous:Option<[u32;2]>,
    top_columns:std::collections::BTreeSet<u32>,
    columns:std::collections::BTreeSet<u32>,
}
pub(super) struct Precision {params:wgpu::Buffer}
impl Precision {
    fn new(r:&WgpuRasterizer,input:Option<(&layer_core::FillEdges,SourceTarget)>)->Self {
        use wgpu::util::DeviceExt;
        let working=r.device.working_space();
        let (depth,space)=input.map(|(edges,target)| {
            let scene=edges.snapshot.view();
            if let Some(base)=scene.paint_base(target)
                && let layer_core::color::ColorProfile::Builtin(space)=base.image.interpretation.profile {
                (base.image.interpretation.depth,space)
            }else{(scene.composition().color.depth,scene.composition().color.space)}
        }).unwrap_or((layer_core::color::SampleDepth::F32,working));
        let transform=working.linear_transform(space);
        let mut bytes=Vec::with_capacity(96);
        for column in [0,1,2].map(|column|[transform[0][column] as f32,transform[1][column] as f32,transform[2][column] as f32,0.]) {
            for value in column {bytes.extend(value.to_ne_bytes());}
        }
        let half=r.device.working_format()==wgpu::TextureFormat::Rgba16Float;
        for value in [if depth.is_float(){0}else{depth.maximum()},space.shader_code(),if half{10}else{23},if half{24}else{149}] {bytes.extend(value.to_ne_bytes());}
        let extent=input.map(|(edges,target)| {
            let scene=edges.snapshot.view();let size=scene.composition().size;let offset=scene.target_offset(target);
            [size[0] as i32,size[1] as i32,offset[0] as i32,offset[1] as i32]
        }).unwrap_or([0;4]);
        for value in extent {bytes.extend(value.to_ne_bytes());}
        for value in [u32::from(input.is_some_and(|(edges,_)|edges.colour_on_solid)),0,0,0] {bytes.extend(value.to_ne_bytes());}
        let params=r.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {label:Some("fill source precision"),contents:&bytes,usage:wgpu::BufferUsages::UNIFORM});
        Self {params}
    }
}
pub(super) fn layout(device:&PipelineDevice)->wgpu::BindGroupLayout {
    crate::bindings::layout(device,"fill edge inputs",&[
        crate::bindings::texture(0,wgpu::ShaderStages::FRAGMENT,false),
        crate::bindings::buffer(1,wgpu::ShaderStages::FRAGMENT,wgpu::BufferBindingType::Storage {read_only:true},false,None),
        crate::bindings::texture(2,wgpu::ShaderStages::FRAGMENT,false),
        crate::bindings::texture(3,wgpu::ShaderStages::FRAGMENT,false),
        crate::bindings::buffer(4,wgpu::ShaderStages::FRAGMENT,wgpu::BufferBindingType::Uniform,false,None),
    ])
}
pub(super) fn binding(r:&WgpuRasterizer,color:&wgpu::TextureView,lines:&wgpu::TextureView,cover:&wgpu::TextureView,mask:&wgpu::Buffer,precision:Option<&Precision>)->wgpu::BindGroup {
    let empty=precision.is_none().then(||Precision::new(r,None));let precision=precision.or(empty.as_ref()).unwrap();
    crate::bindings::group(&r.device,"fill edge inputs",&r.scene_pipelines.edges,[
        wgpu::BindingResource::TextureView(color),mask.as_entire_binding(),wgpu::BindingResource::TextureView(lines),wgpu::BindingResource::TextureView(cover),
        precision.params.as_entire_binding(),
    ])
}
pub(super) fn shader()->String {
    let selection=include_str!("../selection_clip.wgsl");
    compose_wgsl(&[
        &selection.replace("@group(1) @binding(1)","@group(2) @binding(1)").replace("brush_selection","fill_selection"),
        include_str!("../fill_unmix.wgsl"),
        include_str!("../fill_edges.wgsl"),
    ]).into_owned()
}
impl Capture {
    fn new(r:&WgpuRasterizer,extent:[u32;2],lines:usize)->Self {
        Self {source:Default::default(),color:create_color_target(&r.device,[SIDE,SIDE+1],"fill source halo"),
            lines:create_color_target(&r.device,[SIDE;2],"fill line alpha"),next:create_color_target(&r.device,[SIDE,SIDE+1],"fill line source"),
            cover:create_color_target(&r.device,[SIDE;2],"fill lower line alpha"),
            seed:create_color_target(&r.device,[1;2],"fill seed"),mask:r.unclipped.clone(),precision:None,
            line_seeds:create_color_target(&r.device,[1,lines.max(1) as u32],"fill line seeds").0,external:Vec::new(),
            top:create_color_target(&r.device,[extent[0],HALO],"fill original row border").0,
            left:create_color_target(&r.device,[HALO,SIDE],"fill original page border").0,
            row:None,previous:None,top_columns:Default::default(),columns:Default::default()}
    }
    fn preserve(&mut self,coordinate:[u32;2],crop:PixelRect,extent:[u32;2],encoder:&mut submission::CommandEncoder) {
        if self.row!=Some(coordinate[1]) {
            self.top_columns=if self.row.is_some_and(|row|row+1==coordinate[1]) {std::mem::take(&mut self.columns)}else {Default::default()};
            self.columns.clear();self.previous=None;self.row=Some(coordinate[1]);
        }
        let core=page_rect(coordinate).intersect(PixelRect::full(extent));
        if crop.min_y()<core.min_y() {
            for column in crop.min_x()/PAGE_SIZE..crop.max_x().div_ceil(PAGE_SIZE) {
                if !self.top_columns.contains(&column) {continue;}
                let left=crop.min_x().max(column*PAGE_SIZE);let right=crop.max_x().min((column+1)*PAGE_SIZE);
                copy(encoder,&self.top,&self.color.0,[left,0],[left-crop.min_x(),0],[right-left,core.min_y()-crop.min_y()]);
            }
        }
        if self.previous.is_some_and(|previous|previous[1]==coordinate[1] && previous[0]+1==coordinate[0]) {
            copy(encoder,&self.left,&self.color.0,[0;2],[0;2],[core.min_x()-crop.min_x(),crop.height()]);
        }
        if core.max_x()<extent[0] {
            copy(encoder,&self.color.0,&self.left,[core.max_x()-HALO-crop.min_x(),0],[0;2],[HALO,crop.height()]);
        }
        if core.max_y()<extent[1] {
            copy(encoder,&self.color.0,&self.top,[core.min_x()-crop.min_x(),core.max_y()-HALO-crop.min_y()],[core.min_x(),0],[core.width(),HALO]);
        }
        self.previous=Some(coordinate);self.columns.insert(coordinate[0]);
    }
    fn packet<'a>(snapshot:&'a layer_core::SceneSnapshot,scope:&'a SceneScope,offset:[i64;2],extent:[u32;2],packet:FramePacket<'a>)->FramePacket<'a> {
        let scene=snapshot.view().with_scope(scope).with_offset64(offset.map(|v|-(v as f64)));
        FramePacket {scene,document_extent:extent,selection_overlays:None,inspect_mask:None,dabs:&[],dab_batches:&[],restore_rasters:&[],
            reset_layers:false,commit_rasters:false,composite_all:false,time_seconds:snapshot.context.elapsed,blend_space:scene.composition().blend,..packet}
    }
}
fn copy(encoder:&mut submission::CommandEncoder,source:&wgpu::Texture,destination:&wgpu::Texture,from:[u32;2],to:[u32;2],size:[u32;2]) {
    encoder.copy_texture_to_texture(wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:from[0],y:from[1],z:0},..source.as_image_copy()},
        wgpu::TexelCopyTextureInfo {origin:wgpu::Origin3d {x:to[0],y:to[1],z:0},..destination.as_image_copy()},
        wgpu::Extent3d {width:size[0],height:size[1],depth_or_array_layers:1});
}
fn coverage(r:&mut WgpuRasterizer,selection:Option<&layer_core::Selection>,extent:[u32;2],encoder:&mut submission::CommandEncoder)->Result<wgpu::Buffer,GpuRasterError> {
    let Some(selection)=selection else{return Ok(r.unclipped.clone());};
    if selection.affine==layer_core::Affine::IDENTITY && !selection.inverted
        && let layer_core::SelectionShape::Pixels(pixels)=&selection.shape {return Ok(r.selection_clip.pixel_buffer(&r.device,pixels));}
    r.selection_clip.prepare(&r.device,encoder,extent,&Arc::new(selection.clone()))?;
    let source=r.selection_clip.buffer.as_ref().unwrap();
    let copy=r.device.create_buffer(&wgpu::BufferDescriptor {label:Some("fill edge coverage"),size:source.size(),usage:wgpu::BufferUsages::STORAGE|wgpu::BufferUsages::COPY_DST,mapped_at_creation:false});
    encoder.copy_buffer_to_buffer(source,0,&copy,0,source.size());Ok(copy)
}
fn band_intersects(operation:&layer_core::RasterOperation,coordinate:[u32;2],extent:[u32;2],halo:u32)->bool {
    let RasterOperationKind::Fill {underfill:Some(selection),..}=&operation.kind else {return false;};
    let region=page_rect(coordinate).expand(halo,extent);
    if !selection.inverted && selection.affine.0[..4]==[1.,0.,0.,1.]
        && let layer_core::SelectionShape::Pixels(pixels)=&selection.shape {
        let offset=[selection.affine.0[4],selection.affine.0[5]];
        if offset.iter().all(|value|value.fract()==0.) {
            let bounds=DocRect::from(region).translated(offset.map(|value|-(value as i64))).in_frame(pixels.extent());
            return pixels.coverage_bounds_in([bounds.min_x(),bounds.min_y(),bounds.max_x(),bounds.max_y()])!=[0;4];
        }
    }
    !region.intersect(pixel_rect(selection.bounds(),extent)).is_empty()
}
pub(crate) fn read_bounds(operation:&layer_core::RasterOperation,bounds:PixelRect,extent:[u32;2])->PixelRect {
    if matches!(operation.kind,RasterOperationKind::Fill {edges:Some(_),underfill:Some(_),..}) {bounds.expand(HALO,extent)}else{bounds}
}

type Texture=(wgpu::Texture,wgpu::TextureView);
struct LineCapture<'a> {
    source:&'a mut artwork::Capture,next:&'a Texture,seeds:&'a wgpu::Texture,lines:&'a wgpu::TextureView,cover:&'a wgpu::TextureView,
}
impl LineCapture<'_> {
    fn capture(&mut self,scene:&mut Scene,r:&mut WgpuRasterizer,packet:FramePacket<'_>,line:&layer_core::FillLine,index:usize,crop:PixelRect,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        scene.jobs.push(Job::Clear(self.next.1.clone(),wgpu::Color::TRANSPARENT));scene.encode_jobs(r,encoder)?;
        self.source.region_into(r,packet,crop,&self.next.0,encoder)?;
        copy(encoder,&self.seeds,&self.next.0,[0,index as u32],[0,SIDE],[1;2]);
        let mut data=[0.;32];data[..8].copy_from_slice(&[0.,0.,SIDE as f32,SIDE as f32,SIDE as f32,SIDE as f32,0.,0.]);
        data[8..12].copy_from_slice(&[23.,1.,1.,0.]);
        scene.jobs.push(Job::Draw {target:self.lines.clone(),sources:[self.next.1.clone(),r.empty_view.clone(),r.empty_view.clone(),r.empty_paper_view.clone()],
            data,over:true,clip:None,source_target:None,edges:None});scene.encode_jobs(r,encoder)?;
        if line.fill_above {
            scene.jobs.push(Job::Draw {target:self.cover.clone(),sources:[self.next.1.clone(),r.empty_view.clone(),r.empty_view.clone(),r.empty_paper_view.clone()],
                data,over:true,clip:None,source_target:None,edges:None});scene.encode_jobs(r,encoder)?;
        }
        Ok(())
    }
}

impl Scene {
    fn fill_target(&mut self,r:&mut WgpuRasterizer,target:SourceTarget,region:PixelRect,texture:&Texture,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        let mut tiles=Vec::new();let mut leases=Vec::new();
        for coordinate in page_coordinates(region) {
            let tile=r.raw_layer_tile(target,coordinate,encoder)?;
            if let Some(tile)=&tile {leases.extend(r.source_tiles.borrow().lease(&tile.view));}
            tiles.push((coordinate,tile));
        }
        if tiles.is_empty() || tiles.iter().any(|(_,tile)|tile.is_none()) {
            self.jobs.push(Job::Clear(texture.1.clone(),wgpu::Color::TRANSPARENT));self.encode_jobs(r,encoder)?;
        }
        for (coordinate,tile) in tiles {
            let Some(tile)=tile else {continue;};let overlap=page_rect(coordinate).intersect(region);
            copy(encoder,&tile.texture,&texture.0,[overlap.min_x()-coordinate[0]*PAGE_SIZE,overlap.min_y()-coordinate[1]*PAGE_SIZE],
                [overlap.min_x()-region.min_x(),overlap.min_y()-region.min_y()],[overlap.width(),overlap.height()]);
        }
        Ok(())
    }
    pub(super) fn prepare_fill_edges(&mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,target:SourceTarget,index:u32,edges:&layer_core::FillEdges,operation:&layer_core::RasterOperation,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        let extent=packet.scene.target_extent(target);let offset=packet.scene.target_offset(target);
        let mut capture=self.fill_capture.take().unwrap_or_else(||Box::new(Capture::new(r,extent,edges.lines.len())));
        let prepared=r.fill_edges_preflight.as_ref().and_then(|state|state.input(target,index).map(|input|(state.line_seeds[input].clone(),state.inputs[input].external.clone())));
        capture.external=prepared.as_ref().map_or_else(||vec![false;edges.lines.len()],|(_,external)|external.clone());
        let [x,y]=[edges.position.x,edges.position.y].map(|v|v.max(0.) as u32);
        let region=PixelRect::new(x,y,x+1,y+1).intersect(PixelRect::full(extent));
        let result=(|| {
            for (index,line) in edges.lines.iter().enumerate() {
                if capture.external[index] {
                    copy(encoder,&prepared.as_ref().unwrap().0,&capture.line_seeds,[0,index as u32],[0,index as u32],[1;2]);
                }else {
                    capture.source.region_into(r,Capture::packet(&line.snapshot,&line.scope,offset,extent,packet),region,&capture.seed.0,encoder)?;
                    copy(encoder,&capture.seed.0,&capture.line_seeds,[0;2],[0,index as u32],[1;2]);
                }
            }
            self.fill_target(r,target,region,&capture.seed,encoder)?;
            capture.mask=coverage(r,operation.coverage.selection.as_ref(),extent,encoder)?;
            capture.precision=Some(Precision::new(r,Some((edges,target))));
            Ok(())
        })();
        self.fill_capture=Some(capture);result
    }
    pub(super) fn fill_edge_page(&mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,target:SourceTarget,index:u32,operation:&layer_core::RasterOperation,coordinate:[u32;2],encoder:&mut submission::CommandEncoder)->Result<Option<(wgpu::BindGroup,[f32;4])>,GpuRasterError> {
        let RasterOperationKind::Fill {edges:Some(edges),..}=&operation.kind else {unreachable!();};
        let extent=packet.scene.target_extent(target);let offset=packet.scene.target_offset(target);
        if !band_intersects(operation,coordinate,extent,HALO) {return Ok(None);}
        self.encode_jobs(r,encoder)?;
        let mut capture=self.fill_capture.take().unwrap();
        let crop=page_rect(coordinate).expand(HALO,extent);
        let result=(|| {
            self.fill_target(r,target,crop,&capture.color,encoder)?;
            capture.preserve(coordinate,crop,extent,encoder);
            if !band_intersects(operation,coordinate,extent,0) {return Ok(None);}
            encoder.copy_texture_to_texture(capture.seed.0.as_image_copy(),wgpu::TexelCopyTextureInfo {texture:&capture.color.0,
                origin:wgpu::Origin3d {x:0,y:SIDE,z:0},mip_level:0,aspect:wgpu::TextureAspect::All},wgpu::Extent3d {width:1,height:1,depth_or_array_layers:1});
            let prepared=r.fill_edges_preflight.as_ref().and_then(|state|state.pages.iter().find(|(key,_,_)|*key==(target,index,coordinate)))
                .map(|(_,lines,cover)|(lines.0.clone(),cover.0.clone()));
            if let Some((lines,cover))=&prepared {
                copy(encoder,lines,&capture.lines.0,[0;2],[0;2],[SIDE;2]);copy(encoder,cover,&capture.cover.0,[0;2],[0;2],[SIDE;2]);
            }else {
                self.jobs.push(Job::Clear(capture.lines.1.clone(),wgpu::Color::TRANSPARENT));self.encode_jobs(r,encoder)?;
                self.jobs.push(Job::Clear(capture.cover.1.clone(),wgpu::Color::TRANSPARENT));self.encode_jobs(r,encoder)?;
            }
            for (index,line) in edges.lines.iter().enumerate() {
                if prepared.is_some() && capture.external[index] {continue;}
                LineCapture {source:&mut capture.source,next:&capture.next,seeds:&capture.line_seeds,lines:&capture.lines.1,cover:&capture.cover.1}
                    .capture(self,r,Capture::packet(&line.snapshot,&line.scope,offset,extent,packet),line,index,crop,encoder)?;
            }
            Ok(Some((binding(r,&capture.color.1,&capture.lines.1,&capture.cover.1,&capture.mask,capture.precision.as_ref()),[crop.min_x() as f32,crop.min_y() as f32,crop.width() as f32,crop.height() as f32])))
        })();
        self.fill_capture=Some(capture);result
    }
}

struct Input {
    target:SourceTarget,index:u32,operation:layer_core::RasterOperation,external:Vec<bool>,damage:PixelRect,
}
enum Step {Seed(usize),Page(usize,[u32;2])}
pub(crate) struct FillEdgesPreflight {
    owner:u64,revision:u64,color:layer_core::color::DocumentColor,preview:Option<(u64,u64,SourceTarget)>,inputs:Vec<Input>,steps:Vec<Step>,next:usize,
    capture:artwork::Capture,scene:Option<Box<Scene>>,seed:Texture,scratch:Texture,line_seeds:Vec<wgpu::Texture>,
    pages:Vec<((SourceTarget,u32,[u32;2]),Texture,Texture)>,failure:Option<GpuRasterError>,
}
impl FillEdgesPreflight {
    pub fn matches(&self,packet:FramePacket<'_>,color:layer_core::color::DocumentColor)->bool {
        self.owner==packet.scene.owner() && self.revision==packet.scene.revision() && self.color==color
    }
    pub fn pending(&self)->bool {self.failure.is_none() && self.next<self.steps.len()}
    fn input(&self,target:SourceTarget,index:u32)->Option<usize> {self.inputs.iter().position(|input|input.target==target && input.index==index)}
    fn advance(&mut self,r:&mut WgpuRasterizer,packet:FramePacket<'_>,encoder:&mut submission::CommandEncoder)->Result<(),GpuRasterError> {
        if self.scene.is_none() {self.scene=Some(Scene::new(r)?);}
        while let Some(step)=self.steps.get(self.next) {
            let (Step::Seed(input)|Step::Page(input,_))=*step;
            let input=&self.inputs[input];let extent=packet.scene.target_extent(input.target);let offset=packet.scene.target_offset(input.target);
            let RasterOperationKind::Fill {edges:Some(edges),..}=&input.operation.kind else {unreachable!();};
            let scene=self.scene.as_mut().unwrap();
            match *step {
                Step::Seed(id)=> {
                    let [x,y]=[edges.position.x,edges.position.y].map(|value|value.max(0.) as u32);
                    let region=PixelRect::new(x,y,x+1,y+1).intersect(PixelRect::full(extent));
                    for (index,line) in edges.lines.iter().enumerate().filter(|(index,_)|input.external[*index]) {
                        self.capture.region_into(r,Capture::packet(&line.snapshot,&line.scope,offset,extent,packet),region,&self.seed.0,encoder)?;
                        copy(encoder,&self.seed.0,&self.line_seeds[id],[0;2],[0,index as u32],[1;2]);
                    }
                }
                Step::Page(id,coordinate)=> {
                    let lines=create_color_target(&r.device,[SIDE;2],"prepared fill lines");let cover=create_color_target(&r.device,[SIDE;2],"prepared fill cover");
                    scene.jobs.push(Job::Clear(lines.1.clone(),wgpu::Color::TRANSPARENT));
                    scene.jobs.push(Job::Clear(cover.1.clone(),wgpu::Color::TRANSPARENT));scene.encode_jobs(r,encoder)?;
                    for (index,line) in edges.lines.iter().enumerate().filter(|(index,_)|input.external[*index]) {
                        LineCapture {source:&mut self.capture,next:&self.scratch,seeds:&self.line_seeds[id],lines:&lines.1,cover:&cover.1}
                            .capture(scene,r,Capture::packet(&line.snapshot,&line.scope,offset,extent,packet),line,index,page_rect(coordinate).expand(HALO,extent),encoder)?;
                    }
                    self.pages.push(((input.target,input.index,coordinate),lines,cover));
                }
            }
            self.next+=1;
        }
        Ok(())
    }
}
impl WgpuRasterizer {
    pub(crate) fn prepare_fill_edges(&mut self,packet:FramePacket<'_>)->bool {
        if self.snapshot_worker {return true;}
        let keys:Vec<_>=packet.dab_batches.iter().filter_map(|batch| {
            let DabBatchKind::RasterOperation(index)=batch.kind else {return None;};
            let operation=packet.scene.operations(batch.target)?.get(index as usize)?;
            matches!(operation.kind,RasterOperationKind::Fill {edges:Some(_),underfill:Some(_),..}).then_some((batch.target,index))
        }).chain(self.fill_preview.iter().filter(|fill|matches!(fill.operation.kind,RasterOperationKind::Fill {edges:Some(_),underfill:Some(_),..})).map(|fill|(fill.target,u32::MAX))).collect();
        if keys.is_empty() {self.fill_edges_preflight=None;return true;}
        let preview=self.fill_preview.as_ref().map(layer_render::FillPreview::key);
        let mut state=if let Some(state)=self.fill_edges_preflight.take().filter(|state|state.preview==preview
            && state.inputs.iter().map(|input|(input.target,input.index)).eq(keys.iter().copied())) {state}else {
            let edited:Vec<_>=packet.dab_batches.iter().map(|batch|batch.target).chain(self.fill_preview.iter().map(|fill|fill.target)).collect();
            let inputs:Vec<_>=packet.dab_batches.iter().filter_map(|batch| {
                let DabBatchKind::RasterOperation(index)=batch.kind else {return None;};
                Some((batch.target,index,packet.scene.operations(batch.target)?.get(index as usize)?,batch_pixel_rect(batch,packet.scene.target_extent(batch.target))))
            }).chain(self.fill_preview.iter().map(|fill|(fill.target,u32::MAX,&fill.operation,pixel_rect(fill.operation.bounds(packet.scene.target_extent(fill.target)),packet.scene.target_extent(fill.target)))))
                .filter_map(|(target,index,operation,damage)| {
                    let RasterOperationKind::Fill {edges:Some(edges),underfill:Some(_),..}=&operation.kind else {return None;};
                    let external=edges.lines.iter().map(|line|!crate::snapshot::capture_targets(line.snapshot.view().with_scope(&line.scope),&line.scope).iter().any(|target|edited.contains(target))).collect();
                    Some(Input {target,index,operation:operation.clone(),external,damage})
                }).collect();
            let steps=inputs.iter().enumerate().filter(|(_,input)|input.external.iter().any(|external|*external)).flat_map(|(id,input)| {
                let extent=packet.scene.target_extent(input.target);
                std::iter::once(Step::Seed(id)).chain(page_coordinates(input.damage).filter(move |coordinate|band_intersects(&input.operation,*coordinate,extent,0)).map(move |coordinate|Step::Page(id,coordinate)))
            }).collect::<Vec<_>>();
            let bytes=(steps.iter().filter(|step|matches!(step,Step::Page(..))).count() as u64).checked_mul(2*u64::from(SIDE).pow(2)*u64::from(self.device.working_format().block_copy_size(None).unwrap()));
            let failure=bytes.ok_or(GpuRasterError::SourceWorkingSetExceeded).and_then(|bytes|self.admit_preparation_bytes(self.projected_restore_bytes(),bytes)).err();
            let line_seeds=inputs.iter().map(|input|create_color_target(&self.device,[1,input.external.len().max(1) as u32],"prepared fill line seeds").0).collect();
            FillEdgesPreflight {owner:packet.scene.owner(),revision:packet.scene.revision(),color:self.document_color(),preview,inputs,steps,next:0,capture:Default::default(),scene:None,
                seed:create_color_target(&self.device,[1;2],"prepared fill seed"),scratch:create_color_target(&self.device,[SIDE,SIDE+1],"prepared fill line source"),line_seeds,pages:Vec::new(),failure}
        };
        if state.pending() {
            let mut encoder=submission::CommandEncoder::new(&self.device,&Default::default());
            let result=self.drain_image_decodes(&mut encoder).and_then(|()|state.advance(self,packet,&mut encoder));
            if let Err(error)=result && !matches!(error,GpuRasterError::DeferredObjectWork) {state.failure=Some(error);}
            if encoder.has_work() {self.uploads.finish(&encoder);self.last_submission=Some(encoder.submit(&self.queue));}
        }
        let ready=!state.pending();self.fill_edges_preflight=Some(state);ready
    }
    pub(crate) fn take_fill_edges_failure(&mut self)->Option<GpuRasterError> {self.fill_edges_preflight.as_mut().and_then(|state|state.failure.take())}
}
