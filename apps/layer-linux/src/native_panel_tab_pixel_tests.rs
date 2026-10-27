use super::*;

#[test]
#[ignore = "private native Vulkan reference: native tab regions: interior color, geometry, finite HDR and reported edge antialiasing"]
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
    let mut comparisons=0;let mut reports=Vec::new();
    for (space,state) in [("srgb",gdk::ColorState::srgb()),("pq",gdk::ColorState::rec2100_pq()),("linear",gdk::ColorState::rec2100_linear())] {
        for (width,selected_width) in [(72,36),(73,37),(100,70),(222,192)] {for scale in [1.,1.25,2.] {for origin in [[0.,0.],[0.25,0.5]] {
            for (background_alpha,panel_alpha) in [(1.,0.45),(0.6,0.45),(0.6,1.)] {for hover_enabled in [false,true] {
                let bounds=graphene::Rect::new(0.,0.,width as f32,36.);let selected=graphene::Rect::new(12.,0.,selected_width as f32,36.);let hover_rect=graphene::Rect::new((width-31) as f32,0.,30.,36.);let panel=gdk::RGBA::new(0.8,0.2,0.6,panel_alpha);let bg=gdk::RGBA::new(0.15,0.35,0.5,background_alpha);let hover_color=gdk::RGBA::new(0.7,0.4,0.2,if hover_enabled{0.08}else{0.});
                let mask=gtk::Snapshot::new();{let cr=mask.append_cairo(&bounds);selected_outline(&cr,&selected,36.);cr.fill().unwrap();}
                let mut background=vec![paint(bg,&state,&bounds)];
                if hover_enabled {let h=gtk::Snapshot::new();let cr=h.append_cairo(&bounds);cr.set_source_rgba(hover_color.red().into(),hover_color.green().into(),hover_color.blue().into(),hover_color.alpha().into());hover(&cr,&hover_rect,[-6,0]);cr.fill().unwrap();drop(cr);background.push(h.to_node().unwrap());}
                let mask_node=mask.to_node().unwrap();
                let reference=masked_fill(gsk::ContainerNode::new(&background).upcast(),paint(panel,&state,&bounds),mask_node.clone());
                let hover_node=background.get(1).cloned();
                let mut paths=CornerPaths::new();
                let candidate=tab_background(&bounds,Some(&selected),&paint(bg,&state,&bounds),&paint(panel,&state,&bounds),hover_node.as_ref(),crate::squircle::PixelGrid{scale:[scale;2],offset:[origin[0]*scale,origin[1]*scale]},&mut paths);
                for context in 0..3 {for float_target in [false,true] {
                    let wrap=|node:&gsk::RenderNode|{let node=match context{1=>gsk::OpacityNode::new(node,0.7).upcast(),2=>gsk::ClipNode::new(node,&graphene::Rect::new(2.3,3.7,(width-5) as f32,29.)).upcast(),_=>node.clone()};gsk::TransformNode::new(node,Some(&gsk::Transform::new().translate(&graphene::Point::new(origin[0],origin[1])))).upcast()};
                    let a=wrap(&reference);let b=crate::squircle::converted(&wrap(&candidate),scale as f64);let physical=|node:&gsk::RenderNode|gsk::TransformNode::new(node,Some(&gsk::Transform::new().scale(scale,scale))).upcast();let a=physical(&a);let converted_candidate=physical(&b);let raw_candidate=physical(&wrap(&candidate));let legacy_node=a.clone();let viewport=graphene::Rect::new(0.,0.,(width as f32*scale+2.).ceil(),(36.*scale+2.).ceil());let a=render(&a,&viewport,&state,float_target);let b=render(&converted_candidate,&viewport,&state,float_target);let mut byte_error=0;let mut float_error=0f32;
                    let native_bg=render(&physical(&wrap(&gsk::ContainerNode::new(&background).upcast())),&viewport,&state,float_target);
                    let native_fg=render(&physical(&wrap(&paint(panel,&state,&bounds))),&viewport,&state,float_target);
                    let coverage_node=gsk::TransformNode::new(&mask_node,Some(&gsk::Transform::new().translate(&graphene::Point::new(origin[0],origin[1]))));
                    let coverage=render(&physical(coverage_node.upcast_ref()),&viewport,&state,false);
                    let mut interior_byte_error=0u8;let mut interior_float_error=0f32;let mut interior_pixels=0;let mut edge_pixels=0;let mut worst_interior=serde_json::Value::Null;
                    let (width_px,height_px)=(a.4.width() as usize,a.4.height() as usize);
                    for y in 0..height_px {for x in 0..width_px {
                        let alpha=coverage.0[y*coverage.1+x*4+3];
                        let lx=(x as f32+0.5)/scale-origin[0];let ly=(y as f32+0.5)/scale-origin[1];
                        let strip_interior=lx>=bounds.x()+2./scale && lx<=bounds.x()+bounds.width()-2./scale && ly>=bounds.y()+2./scale && ly<=bounds.y()+bounds.height()-2./scale;
                        let clip_interior=context!=2||(lx>=2.3+2./scale&&lx<=(width-5) as f32+2.3-2./scale&&ly>=3.7+2./scale&&ly<=32.7-2./scale);
                        let uniform=strip_interior&&clip_interior&&(alpha==0||alpha==255)&&(y.saturating_sub(2)..=(y+2).min(height_px-1)).all(|yy|(x.saturating_sub(2)..=(x+2).min(width_px-1)).all(|xx|coverage.0[yy*coverage.1+xx*4+3]==alpha));
                        let native=if alpha==255{&native_fg}else{&native_bg};
                        if uniform{interior_pixels+=1}else{edge_pixels+=1}
                        for c in 0..4 {
                            let ai=y*a.1+x*4+c;let bi=y*b.1+x*4+c;
                            byte_error=byte_error.max(a.0[ai].abs_diff(b.0[bi]));
                            let i=y*a.3+(x*4+c)*4;let j=y*b.3+(x*4+c)*4;
                            let av=f32::from_ne_bytes(a.2[i..i+4].try_into().unwrap());let bv=f32::from_ne_bytes(b.2[j..j+4].try_into().unwrap());assert!(av.is_finite()&&bv.is_finite());float_error=float_error.max((av-bv).abs());
                            if uniform {
                                let delta=native.0[y*native.1+x*4+c].abs_diff(b.0[bi]);if delta>interior_byte_error{worst_interior=serde_json::json!({"pixel":[x,y],"logical_center":[lx,ly],"mask_alpha":alpha,"channel":c,"native":native.0[y*native.1+x*4+c],"candidate":b.0[bi],"delta":delta});}interior_byte_error=interior_byte_error.max(delta);
                                let ni=y*native.3+(x*4+c)*4;let nv=f32::from_ne_bytes(native.2[ni..ni+4].try_into().unwrap());assert!(nv.is_finite());interior_float_error=interior_float_error.max((nv-bv).abs());
                            }
                        }
                    }}
                    reports.push(serde_json::json!({"space":space,"width":width,"scale":scale,"origin":origin,"background_alpha":background_alpha,"foreground_alpha":panel_alpha,"hover":hover_enabled,"context":context,"float_target":float_target,"legacy_max_byte_delta":byte_error,"legacy_max_float_delta":float_error,"interior_max_byte_delta_native_color":interior_byte_error,"interior_max_float_delta_native_color":interior_float_error,"interior_pixels":interior_pixels,"edge_pixels":edge_pixels,"worst_interior":worst_interior}));
                    if interior_byte_error>1||interior_float_error>1./255.+1./2048. {if let Some(dir)=std::env::var_os("LAYER_TEST_ARTIFACTS"){let dir=std::path::PathBuf::from(dir);std::fs::create_dir_all(&dir).unwrap();a.4.save_to_png(dir.join("tab-reference.png")).unwrap();b.4.save_to_png(dir.join("tab-candidate.png")).unwrap();legacy_node.write_to_file(dir.join("tab-legacy.node")).unwrap();raw_candidate.write_to_file(dir.join("tab-before-converter.node")).unwrap();converted_candidate.write_to_file(dir.join("tab-after-converter.node")).unwrap();let raw=render(&raw_candidate,&viewport,&state,float_target);raw.4.save_to_png(dir.join("tab-before-converter.png")).unwrap();native_bg.4.save_to_png(dir.join("tab-native-background.png")).unwrap();native_fg.4.save_to_png(dir.join("tab-native-foreground.png")).unwrap();coverage.4.save_to_png(dir.join("tab-mask-coverage.png")).unwrap();std::fs::write(dir.join("tab-comparison.json"),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();}}
                    assert!(interior_byte_error<=1&&interior_float_error<=1./255.+1./2048.,"tab interior {space},width{width},scale{scale},origin{origin:?},bg{background_alpha},panel{panel_alpha},hover{hover_enabled},context{context},float{float_target}: byte{interior_byte_error},float{interior_float_error}; legacy including AA/clamped HDR byte{byte_error},float{float_error}");comparisons+=1;
                }}
            }}
        }}}
    }
    if let Some(dir)=std::env::var_os("LAYER_TEST_ARTIFACTS"){std::fs::write(std::path::PathBuf::from(dir).join("tab-comparison.json"),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();}
    eprintln!("tab region comparisons {comparisons}");window.destroy();crate::workspace::tests::pump(100);
}

fn masked_fill(background: gtk::gsk::RenderNode, foreground: gtk::gsk::RenderNode, mask: gtk::gsk::RenderNode) -> gtk::gsk::RenderNode {
    let outside = gtk::gsk::MaskNode::new(background, &mask, gtk::gsk::MaskMode::InvertedAlpha);
    let inside = gtk::gsk::MaskNode::new(foreground, mask, gtk::gsk::MaskMode::Alpha);
    let average = gtk::gsk::CrossFadeNode::new(outside, inside, 0.5);
    let restore_alpha = gtk::graphene::Matrix::from_float([
        1., 0., 0., 0.,
        0., 1., 0., 0.,
        0., 0., 1., 0.,
        0., 0., 0., 2.,
    ]);
    gtk::gsk::ColorMatrixNode::new(average, &restore_alpha, &gtk::graphene::Vec4::zero()).upcast()
}


fn selected_outline(cr: &gtk::cairo::Context, tab: &gtk::graphene::Rect, bottom: f32) {
    let radius = gtk::graphene::Size::new(layer_ui::SURFACE_RADIUS, layer_ui::SURFACE_RADIUS);
    let square = gtk::graphene::Size::new(0., 0.);
    let mut shape = gtk::gsk::RoundedRect::new(*tab, radius, radius, square, square);
    shape.normalize();
    crate::squircle::rounded_rect(cr, &shape);
    feet(cr, tab, bottom);
}
