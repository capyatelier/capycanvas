use super::*;

#[test]
#[ignore = "private Vulkan GTK renderer; cached shadow coverage and finite HDR values"]
fn native_compact_shadow_pixels() {
    let app=crate::workspace::tests::native_test_app("art.capycanvas.CachedShadowPixels");
    let window=adw::ApplicationWindow::new(&*app);window.set_default_size(640,480);window.present();crate::workspace::tests::pump(200);
    let renderer=window.renderer().unwrap();
    let float=gdk::MemoryTexture::new(1,1,gdk::MemoryFormat::R32g32b32a32Float,&glib::Bytes::from_owned(vec![0u8;16]),16);
    let render=|node:&gsk::RenderNode,viewport:&graphene::Rect,float_target:bool|{
        let snapshot=gtk::Snapshot::new();if float_target{snapshot.append_texture(&float,viewport);}snapshot.append_node(node);
        let texture=renderer.render_texture(snapshot.to_node().unwrap(),Some(viewport));
        let mut download=gdk::TextureDownloader::new(&texture);download.set_format(gdk::MemoryFormat::R8g8b8a8Premultiplied);let (bytes,stride)=download.download_bytes();
        download.set_format(gdk::MemoryFormat::R32g32b32a32FloatPremultiplied);let (floats,float_stride)=download.download_bytes();
        (bytes,stride,floats,float_stride,texture)
    };
    let mut converter=Converter::default();let mut comparisons=0;let mut max_byte_error=0;let mut max_float_error=0f32;
    for scale in [1.,1.25,2.] {for [width,height] in [[36.,36.],[36.3,36.7],[222.,36.],[242.,42.],[40.,40.],[80.,20.],[512.,36.],[513.,36.],[36.,512.]] {
        for (dx,dy,spread,blur) in [(0.,8.,0.,24.),(-2.,3.,1.,6.),(2.,-3.,-1.,12.),(0.3,0.7,0.4,3.2)] {
            for color in [gdk::RGBA::new(0.,0.,0.,0.4),gdk::RGBA::new(0.8,0.3,0.6,0.7)] {
                let outline=gsk::RoundedRect::new(graphene::Rect::new(2.25,3.5,width,height),graphene::Size::new(9.72,9.72),graphene::Size::new(6.48,6.48),graphene::Size::new(3.24,3.24),graphene::Size::new(0.,0.));
                let inner=squircle_rect(&outline);let mut outer=inner;outer.offset(dx,dy);outer.shrink(-spread,-spread,-spread,-spread);converter.scale=scale as f64;
                let geometry=ShadowGeometry::new(converter.scale,&outer,&inner,blur);let minimum=(2*geometry.inset+1) as f32/scale;
                if outer.bounds().width()>=minimum && outer.bounds().height()>=minimum {continue}
                let reference:gsk::RenderNode=gsk::MaskNode::new(gsk::BlurNode::new(gsk::MaskNode::new(gsk::ColorNode::new(&color,outer.bounds()),converter.mask(&outer,None),gsk::MaskMode::Alpha),blur),converter.mask(&inner,None),gsk::MaskMode::InvertedAlpha).upcast();
                let candidate:gsk::RenderNode=gsk::OutsetShadowNode::new(&outline,&color,dx,dy,spread,blur).upcast();
                for context in 0..3 {for float_target in [false,true] {
                    let wrap=|node:&gsk::RenderNode|{
                        let node=match context {1=>gsk::ClipNode::new(node,&graphene::Rect::new(width*0.2,-7.,width*0.65,height+21.)).upcast(),2=>gsk::MaskNode::new(node,gsk::ColorNode::new(&gdk::RGBA::new(0.2,0.4,0.6,0.3),&graphene::Rect::new(-40.,-40.,width+100.,height+100.)),gsk::MaskMode::InvertedAlpha).upcast(),_=>node.clone()};
                        gsk::TransformNode::new(node,Some(&gsk::Transform::new().translate(&graphene::Point::new(0.3,0.7)))).upcast()
                    };
                    let physical=|node:gsk::RenderNode|gsk::TransformNode::new(node,Some(&gsk::Transform::new().scale(scale,scale))).upcast();
                    let expected=physical(wrap(&reference));let actual=physical(converter.frame(&wrap(&candidate),scale as f64));let area=expected.bounds().union(&actual.bounds());let viewport=graphene::Rect::new(area.x().floor(),area.y().floor(),(area.x()+area.width()).ceil()-area.x().floor(),(area.y()+area.height()).ceil()-area.y().floor());
                    let a=render(&expected,&viewport,float_target);let b=render(&actual,&viewport,float_target);assert_eq!((a.1,a.3,a.4.width(),a.4.height()),(b.1,b.3,b.4.width(),b.4.height()));
                    let mut byte_error=0u8;let mut float_error=0f32;
                    for y in 0..a.4.height() as usize {for x in 0..a.4.width() as usize*4 {
                        byte_error=byte_error.max(a.0[y*a.1+x].abs_diff(b.0[y*b.1+x]));
                        let i=y*a.3+x*4;let j=y*b.3+x*4;let av=f32::from_ne_bytes(a.2[i..i+4].try_into().unwrap());let bv=f32::from_ne_bytes(b.2[j..j+4].try_into().unwrap());assert!(av.is_finite()&&bv.is_finite());if x%4==3 { assert!((-0.001..=1.001).contains(&bv),"shadow alpha remains bounded"); }float_error=float_error.max((av-bv).abs());
                    }}
                    max_byte_error=max_byte_error.max(byte_error);max_float_error=max_float_error.max(float_error);
                    if comparisons==0 {if let Some(dir)=std::env::var_os("LAYER_TEST_ARTIFACTS"){let dir=std::path::PathBuf::from(dir);std::fs::create_dir_all(&dir).unwrap();a.4.save_to_png(dir.join("shadow-reference.png")).unwrap();b.4.save_to_png(dir.join("shadow-candidate.png")).unwrap();}}
                    comparisons+=1;
                }}
            }
        }
    }}
    eprintln!("compact shadow comparisons {comparisons}; visual reference deltas byte{max_byte_error} float{max_float_error}");window.destroy();crate::workspace::tests::pump(100);
}
