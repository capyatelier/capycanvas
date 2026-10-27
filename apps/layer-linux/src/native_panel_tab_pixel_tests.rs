use super::*;

#[test]
#[ignore = "private native Vulkan reference: legacy Cairo selected mask versus persistent worker texture"]
fn native_panel_tab_alpha_pixels() {
    use gtk::{gdk,gsk,graphene};
    let app=crate::workspace::tests::native_test_app("art.capycanvas.TabAlphaPixels");
    let window=adw::ApplicationWindow::new(&*app);window.set_default_size(640,480);window.present();crate::workspace::tests::pump(200);
    let renderer=window.renderer().unwrap();let float=gdk::MemoryTexture::new(1,1,gdk::MemoryFormat::R32g32b32a32Float,&glib::Bytes::from_owned(vec![0u8;16]),16);
    let paint=|color:gdk::RGBA,state:&gdk::ColorState,bounds:&graphene::Rect|->gsk::RenderNode {
        if state.equivalent(&gdk::ColorState::srgb()){return gsk::ColorNode::new(&color,bounds).upcast()}
        let bytes=[color.red(),color.green(),color.blue(),color.alpha()].into_iter().flat_map(f32::to_ne_bytes).collect::<Vec<_>>();
        let texture=gdk::MemoryTextureBuilder::new().set_width(1).set_height(1).set_stride(16).set_format(gdk::MemoryFormat::R32g32b32a32Float).set_color_state(state).set_bytes(Some(&glib::Bytes::from_owned(bytes))).build();
        gsk::TextureNode::new(&texture,bounds).upcast()
    };
    let render=|node:&gsk::RenderNode,viewport:&graphene::Rect,state:&gdk::ColorState,float_target:bool|{
        let snapshot=gtk::Snapshot::new();if float_target{snapshot.append_texture(&float,viewport);}snapshot.append_node(node);let texture=renderer.render_texture(snapshot.to_node().unwrap(),Some(viewport));
        let mut d=gdk::TextureDownloader::new(&texture);d.set_color_state(&gdk::ColorState::srgb());d.set_format(gdk::MemoryFormat::R8g8b8a8Premultiplied);let(bytes,stride)=d.download_bytes();d.set_color_state(state);d.set_format(gdk::MemoryFormat::R32g32b32a32FloatPremultiplied);let(floats,float_stride)=d.download_bytes();(bytes,stride,floats,float_stride,texture)
    };
    let mut comparisons=0;
    for (space,state) in [("srgb",gdk::ColorState::srgb()),("pq",gdk::ColorState::rec2100_pq()),("linear",gdk::ColorState::rec2100_linear())] {
        for width in [100,222] {for scale in [1.,1.25,2.] {for origin in [[0.,0.],[0.25,0.5]] {
            for (background_alpha,panel_alpha) in [(1.,0.45),(0.6,0.45),(0.6,1.)] {for hover_enabled in [false,true] {
                let bounds=graphene::Rect::new(0.,0.,width as f32,36.);let selected=graphene::Rect::new(12.,0.,(width-30) as f32,36.);let hover_rect=graphene::Rect::new((width-31) as f32,0.,30.,36.);let panel=gdk::RGBA::new(0.8,0.2,0.6,panel_alpha);let bg=gdk::RGBA::new(0.15,0.35,0.5,background_alpha);let hover_color=gdk::RGBA::new(0.7,0.4,0.2,if hover_enabled{0.08}else{0.});
                let mask=gtk::Snapshot::new();{let cr=mask.append_cairo(&bounds);selected_outline(&cr,&selected,36.);cr.fill().unwrap();}
                let mut background=vec![paint(bg,&state,&bounds)];
                if hover_enabled {let h=gtk::Snapshot::new();let cr=h.append_cairo(&bounds);cr.set_source_rgba(hover_color.red().into(),hover_color.green().into(),hover_color.blue().into(),hover_color.alpha().into());hover(&cr,&hover_rect,[-6,0]);cr.fill().unwrap();drop(cr);background.push(h.to_node().unwrap());}
                let reference=masked_fill(gsk::ContainerNode::new(&background).upcast(),paint(panel,&state,&bounds),mask.to_node().unwrap());
                let path=crate::squircle::CachedMask::new(&bounds,|cr|selected_outline(cr,&selected,36.));
                let key=[selected.x(),selected.y(),selected.width(),selected.height(),bounds.x(),bounds.y(),bounds.width(),bounds.height(),36.,scale].map(f32::to_bits);
                path.set_texture(raster_mask(key));assert!(path.ready());
                let mut new_background=vec![paint(bg,&state,&bounds)];
                if hover_enabled {let h=gtk::Snapshot::new();let cr=h.append_cairo(&bounds);cr.set_source_rgba(hover_color.red().into(),hover_color.green().into(),hover_color.blue().into(),hover_color.alpha().into());hover(&cr,&hover_rect,[-6,0]);cr.fill().unwrap();drop(cr);new_background.push(h.to_node().unwrap());}
                let new_background=gsk::ContainerNode::new(&new_background).upcast();
                let candidate=masked_fill(new_background,paint(panel,&state,&bounds),path.node());
                for context in 0..3 {for float_target in [false,true] {
                    let wrap=|node:&gsk::RenderNode|{let node=match context{1=>gsk::OpacityNode::new(node,0.7).upcast(),2=>gsk::ClipNode::new(node,&graphene::Rect::new(2.3,3.7,(width-5) as f32,29.)).upcast(),_=>node.clone()};gsk::TransformNode::new(node,Some(&gsk::Transform::new().translate(&graphene::Point::new(origin[0],origin[1])))).upcast()};
                    let a=wrap(&reference);let b=crate::squircle::converted(&wrap(&candidate),scale as f64);let physical=|node:&gsk::RenderNode|gsk::TransformNode::new(node,Some(&gsk::Transform::new().scale(scale,scale))).upcast();let a=physical(&a);let b=physical(&b);let viewport=graphene::Rect::new(0.,0.,(width as f32*scale+2.).ceil(),(36.*scale+2.).ceil());let a=render(&a,&viewport,&state,float_target);let b=render(&b,&viewport,&state,float_target);let mut byte_error=0;let mut float_error=0f32;
                    for y in 0..a.4.height() as usize {for x in 0..a.4.width() as usize*4 {byte_error=byte_error.max(a.0[y*a.1+x].abs_diff(b.0[y*b.1+x]));let i=y*a.3+x*4;let j=y*b.3+x*4;let av=f32::from_ne_bytes(a.2[i..i+4].try_into().unwrap());let bv=f32::from_ne_bytes(b.2[j..j+4].try_into().unwrap());assert!(av.is_finite()&&bv.is_finite());float_error=float_error.max((av-bv).abs());}}
                    if byte_error>1||float_error>1./255.+1./2048. {if let Some(dir)=std::env::var_os("LAYER_TEST_ARTIFACTS"){let dir=std::path::PathBuf::from(dir);std::fs::create_dir_all(&dir).unwrap();a.4.save_to_png(dir.join("tab-reference.png")).unwrap();b.4.save_to_png(dir.join("tab-candidate.png")).unwrap();}}
                    assert!(byte_error<=1&&float_error<=1./255.+1./2048.,"tab {space},width{width},scale{scale},origin{origin:?},bg{background_alpha},panel{panel_alpha},hover{hover_enabled},context{context},float{float_target}: byte{byte_error},float{float_error}");comparisons+=1;
                }}
            }}
        }}}
    }
    eprintln!("tab alpha comparisons {comparisons}");window.destroy();crate::workspace::tests::pump(100);
}
