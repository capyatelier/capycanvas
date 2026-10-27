use super::*;

pub(crate) fn converted(node: &gsk::RenderNode, scale: f64) -> gsk::RenderNode {
    fn visit(node: &gsk::RenderNode, converter: &mut Converter) -> gsk::RenderNode {
        if let Some(n)=node.downcast_ref::<gsk::ContainerNode>() {
            return gsk::ContainerNode::new(&(0..n.n_children()).map(|i|visit(&n.child(i),converter)).collect::<Vec<_>>()).upcast();
        }
        if let Some(n)=node.downcast_ref::<gsk::DebugNode>() {
            if n.message()==KEEP_ROUND {return node.clone()}
            if n.message()=="capy-cached-mask" {return visit(&n.child().downcast_ref::<gsk::CrossFadeNode>().unwrap().start_child(),converter)}
            return gsk::DebugNode::new(visit(&n.child(),converter),n.message()).upcast();
        }
        if let Some(n)=node.downcast_ref::<gsk::TransformNode>() {return gsk::TransformNode::new(visit(&n.child(),converter),Some(&n.transform())).upcast()}
        if let Some(n)=node.downcast_ref::<gsk::OpacityNode>() {return gsk::OpacityNode::new(visit(&n.child(),converter),n.opacity()).upcast()}
        if let Some(n)=node.downcast_ref::<gsk::ClipNode>() {return gsk::ClipNode::new(visit(&n.child(),converter),&n.clip()).upcast()}
        if let Some(n)=node.downcast_ref::<gsk::RoundedClipNode>() {
            let clip=n.clip();let child=visit(&n.child(),converter);
            return if clip.is_rectilinear(){gsk::RoundedClipNode::new(child,&clip).upcast()}
                else {gsk::MaskNode::new(child,converter.mask(&squircle_rect(&clip),None),gsk::MaskMode::Alpha).upcast()};
        }
        if let Some(n)=node.downcast_ref::<gsk::MaskNode>() {return gsk::MaskNode::new(visit(&n.source(),converter),visit(&n.mask(),converter),n.mask_mode()).upcast()}
        if let Some(n)=node.downcast_ref::<gsk::ColorMatrixNode>() {return gsk::ColorMatrixNode::new(visit(&n.child(),converter),&n.color_matrix(),&n.color_offset()).upcast()}
        if let Some(n)=node.downcast_ref::<gsk::BlurNode>() {return gsk::BlurNode::new(visit(&n.child(),converter),n.radius()).upcast()}
        if let Some(n)=node.downcast_ref::<gsk::CrossFadeNode>() {return gsk::CrossFadeNode::new(visit(&n.start_child(),converter),visit(&n.end_child(),converter),n.progress()).upcast()}
        if let Some(n)=node.downcast_ref::<gsk::BlendNode>() {return gsk::BlendNode::new(visit(&n.bottom_child(),converter),visit(&n.top_child(),converter),n.blend_mode()).upcast()}
        if let Some(n)=node.downcast_ref::<gsk::IsolationNode>() {return gsk::IsolationNode::new(visit(&n.child(),converter),n.isolations()).upcast()}
        if let Some(n)=node.downcast_ref::<gsk::ShadowNode>() {return gsk::ShadowNode::new(visit(&n.child(),converter),&(0..n.n_shadows()).map(|i|n.shadow(i)).collect::<Vec<_>>()).upcast()}
        if let Some(shadow)=node.downcast_ref::<gsk::OutsetShadowNode>() {
            let outline=shadow.outline();
            if !outline.is_rectilinear() && shadow.blur_radius()>0. {
                let inner=squircle_rect(&outline);let mut outer=inner;outer.offset(shadow.dx(),shadow.dy());outer.shrink(-shadow.spread(),-shadow.spread(),-shadow.spread(),-shadow.spread());
                let geometry=ShadowGeometry::new(converter.scale,&outer,&inner,shadow.blur_radius());let minimum=(2*geometry.inset+1) as f32/converter.scale as f32;
                if outer.bounds().width()<minimum || outer.bounds().height()<minimum {
                    return legacy_compact_shadow(converter,&outer,&inner,&shadow.color(),shadow.blur_radius());
                }
            }
        }
        if node.is::<gsk::BorderNode>()||node.is::<gsk::OutsetShadowNode>()||node.is::<gsk::InsetShadowNode>() {return converter.convert(node)}
        node.clone()
    }
    visit(node,&mut Converter{scale,..Converter::default()})
}

fn legacy_compact_shadow(converter:&mut Converter,shape:&gsk::RoundedRect,outline:&gsk::RoundedRect,color:&gdk::RGBA,blur:f32)->gsk::RenderNode {
    let source=gsk::MaskNode::new(gsk::ColorNode::new(color,shape.bounds()),converter.mask(shape,None),gsk::MaskMode::Alpha);
    gsk::MaskNode::new(gsk::BlurNode::new(source,blur),converter.mask(outline,None),gsk::MaskMode::InvertedAlpha).upcast()
}

#[cfg(test)]
#[path = "native_compact_shadow_pixel_tests.rs"]
mod compact_shadow_pixels;

pub(crate) fn contour_edges(node:&gsk::RenderNode,scale:f64,width:i32,height:i32)->Vec<u8> {
    fn outline(cr:&cairo::Context,rect:&gsk::RoundedRect) {
        let matrix=cr.matrix();let pixels=matrix.xx().hypot(matrix.yx()).max(matrix.xy().hypot(matrix.yy())).max(1e-9);
        cr.set_line_width(4./pixels);rounded_rect(cr,&squircle_rect(rect));cr.stroke().unwrap();
    }
    fn visit(node:&gsk::RenderNode,cr:&cairo::Context) {
        cr.save().unwrap();
        if let Some(n)=node.downcast_ref::<gsk::DebugNode>() && n.message()==KEEP_ROUND {cr.restore().unwrap();return}
        if let Some(n)=node.downcast_ref::<gsk::TransformNode>() {let(a,b,c,d,e,f)=n.transform().to_2d();cr.transform(cairo::Matrix::new(a.into(),b.into(),c.into(),d.into(),e.into(),f.into()));}
        if let Some(n)=node.downcast_ref::<gsk::RoundedClipNode>() {let rect=n.clip();outline(cr,&rect);rounded_rect(cr,&squircle_rect(&rect));cr.clip();}
        if let Some(n)=node.downcast_ref::<gsk::ClipNode>() {let r=n.clip();cr.rectangle(r.x().into(),r.y().into(),r.width().into(),r.height().into());cr.clip();}
        if let Some(n)=node.downcast_ref::<gsk::BorderNode>() {outline(cr,&n.outline());}
        if let Some(n)=node.downcast_ref::<gsk::OutsetShadowNode>() {outline(cr,&n.outline());}
        if let Some(n)=node.downcast_ref::<gsk::InsetShadowNode>() {outline(cr,&n.outline());}
        for child in node.children(){visit(&child,cr);}
        cr.restore().unwrap();
    }
    let mut surface=cairo::ImageSurface::create(cairo::Format::A8,width,height).unwrap();
    let cr=cairo::Context::new(&surface).unwrap();cr.scale(scale,scale);cr.set_source_rgba(1.,1.,1.,1.);visit(node,&cr);drop(cr);
    let stride=surface.stride() as usize;let data=surface.data().unwrap();let mut edges=Vec::with_capacity(width as usize*height as usize);
    for y in 0..height as usize{edges.extend_from_slice(&data[y*stride..y*stride+width as usize]);}edges
}
