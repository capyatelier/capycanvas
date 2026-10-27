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
        if node.is::<gsk::BorderNode>()||node.is::<gsk::OutsetShadowNode>()||node.is::<gsk::InsetShadowNode>() {return converter.convert(node)}
        node.clone()
    }
    visit(node,&mut Converter{scale,..Converter::default()})
}
