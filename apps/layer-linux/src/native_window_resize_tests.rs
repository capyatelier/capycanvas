use super::*;

#[test]
#[ignore = "private 120 Hz Mutter; workspace-motion.sh gtk --native-test=native_window_resize_presentations"]
fn native_window_resize_presentations() {
    let output = std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let app = native_test_app("art.capycanvas.WindowResizeMeasurement");
    let w = Workspace::new(&app);
    apply_fixture_theme(&w);
    w.window.set_default_size(1100, 760);
    let observed = Rc::new(Cell::new(None));
    let motion = gtk::EventControllerMotion::new();
    motion.set_propagation_phase(gtk::PropagationPhase::Capture);
    motion.connect_motion(glib::clone!(#[strong] observed, move |_, x, y| observed.set(Some([x as f32, y as f32]))));
    w.window.add_controller(motion);
    w.window.present();
    pump(1800);
    wait_workspaces(&w);
    until(|| w.gpu.borrow().is_some(), "canvas GPU ready");
    pump(1500);
    set_transparency(&w,"LAYER_NATIVE_RESIZE_TRANSPARENCY");
    let mut input = RemoteInput::new().settle_ms(0).timeout_secs(30);
    input.ready();
    let before_texture=crate::snapshot(&w);before_texture.save_to_png(output.join("before.png")).unwrap();
    std::fs::write(output.join("before-texture.json"),serde_json::to_vec_pretty(&resize_texture_metadata(&before_texture)).unwrap()).unwrap();
    pump(300);
    if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some(){input.perform(serde_json::json!([{"wait_ms":300},{"capture":"before-composited"}]));}
    let clock = w.window.frame_clock().unwrap();
    let surface=w.window.surface().unwrap();let monitor=surface.display().monitor_at_surface(&surface).unwrap();
    let runtime=serde_json::json!({"ui_thread_id":unsafe { libc::gettid() },"display":surface.display().name().to_string(),"surface_scale":surface.scale(),"widget_scale":w.window.scale_factor(),"monitor_connector":monitor.connector().map(|s|s.to_string()),"monitor_description":monitor.description().map(|s|s.to_string()),"monitor_geometry":[monitor.geometry().x(),monitor.geometry().y(),monitor.geometry().width(),monitor.geometry().height()],"monitor_refresh_millihz":monitor.refresh_rate(),"canvas_view_color":format!("{:?}",w.view_color()),"GTK":[gtk::major_version(),gtk::minor_version(),gtk::micro_version()],"renderer":w.window.renderer().unwrap().type_().name(),"GTK_DEBUG":std::env::var("GTK_DEBUG").ok(),"GSK_DEBUG":std::env::var("GSK_DEBUG").ok()});
    std::fs::write(output.join("runtime-metadata.json"),serde_json::to_vec_pretty(&runtime).unwrap()).unwrap();eprintln!("runtime: {runtime}");
    let worker_stats = ui_session(&w).engine().backend().stats.clone();
    let mut reports = Vec::new();
    let contacts=std::env::var("LAYER_NATIVE_RESIZE_CONTACTS").map_or(3,|value|value.parse::<usize>().unwrap());
    assert!((1..=3).contains(&contacts));
    for repeat in 0..=contacts {
        observed.set(None);
        input.perform(serde_json::json!([{"point":[799.,499.]},{"point":[800.,500.]}]));
        pump(100);
        let local = observed.get().expect("pointer inside private window");
        let origin = [800. - local[0], 500. - local[1]];
        let size = [w.window.width(), w.window.height()];
        let corner = [origin[0] + size[0] as f32 - 1., origin[1] + size[1] as f32 - 1.];
        input.perform(serde_json::json!([{"point":corner}]));
        pump(100);
        let frames = Rc::new(RefCell::new(Vec::new()));
        let last = Rc::new(Cell::new(size));
        let paint_start = Rc::new(Cell::new((Instant::now(),crate::timing::thread_cpu_ms(),glib::monotonic_time())));
        let before = clock.connect_before_paint(glib::clone!(#[strong] paint_start, move |_| paint_start.set((Instant::now(),crate::timing::thread_cpu_ms(),glib::monotonic_time()))));
        let after = clock.connect_after_paint(glib::clone!(#[strong] frames, #[strong] last, #[strong] paint_start, #[weak] w, move |clock| {
            let dimensions = [w.window.width(), w.window.height()];
            if last.replace(dimensions) != dimensions && let Some(timing) = clock.current_timings() {
                frames.borrow_mut().push((timing, dimensions, paint_start.get().0.elapsed().as_secs_f64()*1000.,crate::timing::thread_cpu_ms()-paint_start.get().1,[paint_start.get().2,glib::monotonic_time()]));
            }
        }));
        let mut events = vec![serde_json::json!({"down":true})];
        let count = if repeat == 0 { 500 } else { 1500 };
        for i in 0..count {
            let phase = (i as f32 * 0.004) % 2.;
            let triangle = if phase < 0.5 { phase * 2. } else if phase < 1.5 { 2. - phase * 2. } else { phase * 2. - 4. };
            events.push(serde_json::json!({"point":[corner[0]+triangle*80.,corner[1]+triangle*50.]}));
        }
        events.push(serde_json::json!({"down":false}));
        let step = input.step;
        { let mut stats = worker_stats.lock().unwrap(); stats.cpu.clear(); stats.frame_handler_cpu.clear(); stats.presented.clear(); }
        input.perform(serde_json::to_value(events).unwrap());
        clock.disconnect(after);
        clock.disconnect(before);
        pump(400);
        let trace: serde_json::Value = serde_json::from_slice(&std::fs::read(input.dir.join(format!("trace-{step}.json"))).unwrap()).unwrap();
        let trace = trace.as_array().unwrap();
        let begin = trace.first().unwrap()["ns"].as_i64().unwrap()/1000;
        let end = trace.last().unwrap()["ns"].as_i64().unwrap()/1000;
        let raw: Vec<_> = frames.borrow().iter().map(|(timing, dimensions, paint, cpu, window)|serde_json::json!({"before_paint_us":window[0],"after_paint_us":window[1],"frame_time_us":timing.frame_time(),"presentation_time_us":timing.presentation_time(),"complete":timing.is_complete(),"dimensions":dimensions,"paint_ms":paint,"paint_wall_ms":paint,"paint_thread_cpu_ms":cpu})).collect();
        let mut times: Vec<_> = frames.borrow().iter().filter(|(timing,_,_,_,_)|timing.is_complete() && timing.presentation_time()>0 && timing.frame_time()>=begin && timing.frame_time()<=end).map(|(timing,_,_,_,_)|timing.presentation_time()).collect();
        times.sort_unstable();
        times.dedup();
        let percentile = |mut values:Vec<f64>,p:usize| { values.sort_by(f64::total_cmp); values.get(values.len()*p/100).copied().unwrap_or(0.) };
        let gaps:Vec<_> = times.windows(2).map(|pair|(pair[1]-pair[0]) as f64/1000.).collect();
        let width_range = [frames.borrow().iter().map(|(_,d,_,_,_)|d[0]).min().unwrap_or(size[0]),frames.borrow().iter().map(|(_,d,_,_,_)|d[0]).max().unwrap_or(size[0])];
        let height_range = [frames.borrow().iter().map(|(_,d,_,_,_)|d[1]).min().unwrap_or(size[1]),frames.borrow().iter().map(|(_,d,_,_,_)|d[1]).max().unwrap_or(size[1])];
        let stats = worker_stats.lock().unwrap();
        let worker_cpu: Vec<_> = stats.cpu.iter().filter(|row|row[4]>=begin as f64*1000. && row[4]<=end as f64*1000.).copied().collect();
        let canvas_presents:Vec<_> = stats.presented.iter().filter(|row|row[3]==1 && row[1]>=begin as u64*1000 && row[1]<=end as u64*1000).copied().collect();
        std::fs::write(output.join(format!("canvas-{repeat}.json")),serde_json::to_vec_pretty(&serde_json::json!({"worker_cpu":worker_cpu,"presentations":canvas_presents,"frame_handler_cpu":stats.frame_handler_cpu})).unwrap()).unwrap();
        let mut report = serde_json::json!({"paint_cpu_p50_ms":percentile(frames.borrow().iter().map(|(_,_,_,cpu,_)|*cpu).collect(),50),"paint_cpu_p95_ms":percentile(frames.borrow().iter().map(|(_,_,_,cpu,_)|*cpu).collect(),95),"paint_timing":"before/after GDK paint; elapsedwall andOSthreadCPU; changingwindowdimensionsonly","canvas_presentations":canvas_presents.len(),"canvas_full_contact_hz":canvas_presents.len() as f64*1e6/(end-begin) as f64,"acquire_configure_p50_ms":percentile(worker_cpu.iter().map(|row|row[1]).collect(),50),"acquire_configure_p95_ms":percentile(worker_cpu.iter().map(|row|row[1]).collect(),95),"owner_frame_handler_p95_ms":percentile(stats.frame_handler_cpu.clone(),95),"canvas_extent":ui_session(&w).engine().document().composition().size,"gtk_renderer":w.window.renderer().unwrap().type_().name(),"priming":repeat==0,"repeat":repeat,"theme":std::env::var("CAPY_NATIVE_TEST_THEME").unwrap(),"gtk_version":[gtk::major_version(),gtk::minor_version(),gtk::micro_version()],"begin_us":begin,"end_us":end,"contact_seconds":(end-begin) as f64/1e6,"input_moves":count,"changed_paint_frames":frames.borrow().len(),"changed_completed_presentations":times.len(),"moving_hz":if times.len()>1 {(times.len()-1) as f64*1e6/(times.last().unwrap()-times[0]) as f64} else {0.},"full_contact_hz":times.iter().filter(|t|**t>=begin && **t<=end).count() as f64*1e6/(end-begin) as f64,"onset_ms":times.first().map(|t|(*t-begin) as f64/1000.),"tail_ms":times.last().map(|t|(end-*t) as f64/1000.),"gap_p99_ms":percentile(gaps.clone(),99),"gap_max_ms":gaps.iter().copied().reduce(f64::max),"paint_p50_ms":percentile(frames.borrow().iter().map(|(_,_,p,_,_)|*p).collect(),50),"paint_p95_ms":percentile(frames.borrow().iter().map(|(_,_,p,_,_)|*p).collect(),95),"paint_p99_ms":percentile(frames.borrow().iter().map(|(_,_,p,_,_)|*p).collect(),99),"width_range":width_range,"height_range":height_range,"window_origin":origin,"initial_size":size,"surface_scale":w.window.scale_factor(),"gdk_surface_scale":w.window.surface().unwrap().scale(),"gpu_timestamps":std::env::var("LAYER_PACING_GPU_TIMESTAMPS").unwrap_or_default()});
        drop(stats);
        if std::env::var_os("LAYER_NATIVE_RESIZE_RESOURCE_ONLY").is_some(){for key in ["moving_hz","full_contact_hz","canvas_full_contact_hz","gap_p99_ms","gap_max_ms","onset_ms","tail_ms"]{report.as_object_mut().unwrap().remove(key);}report["measurement"]=serde_json::json!("resources-and-paint-time");}
        std::fs::write(output.join(format!("frames-{repeat}.json")),serde_json::to_vec_pretty(&raw).unwrap()).unwrap();
        std::fs::copy(input.dir.join(format!("trace-{step}.json")),output.join(format!("inputs-{repeat}.json"))).unwrap();
        eprintln!("{report}");
        reports.push(report);
        std::fs::write(output.join("summary.json"),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
        assert!(width_range[1]-width_range[0]>100 && height_range[1]-height_range[0]>60,"native corner must resize outer window: {width_range:?} {height_range:?}");
        assert!(times.len()>20,"completed presentations while outer window changes");
        w.window.set_default_size(1100,760);
        pump(700);
    }
    crate::capture(&w, output.join("after.png").to_str().unwrap());
    input.finish();
    w.window.close();
    pump(100);
}

#[test]
#[ignore = "private Mutter; GTK_DEBUG=snapshot and native input required"]
fn native_window_resize_inventory() {
    assert!(std::env::var("GTK_DEBUG").unwrap_or_default().split([':', ',']).any(|flag|flag=="snapshot"));
    let output=std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    let app=native_test_app("art.capycanvas.WindowResizeInventory");
    let w=Workspace::new(&app);
    apply_fixture_theme(&w);
    w.window.set_default_size(1100,760);
    let observed=Rc::new(Cell::new(None));
    let motion=gtk::EventControllerMotion::new();
    motion.set_propagation_phase(gtk::PropagationPhase::Capture);
    motion.connect_motion(glib::clone!(#[strong] observed,move|_,x,y|observed.set(Some([x as f32,y as f32]))));
    w.window.add_controller(motion);
    w.window.present();pump(2200);wait_workspaces(&w);
    until(||w.gpu.borrow().is_some(),"inventory canvas ready");pump(1000);
    set_transparency(&w,"LAYER_NATIVE_RESIZE_TRANSPARENCY");
    let mut input=RemoteInput::new().settle_ms(0).timeout_secs(120);input.ready();
    input.perform(serde_json::json!([{"point":[799.,499.]},{"point":[800.,500.]}]));pump(100);
    let local=observed.get().expect("pointer inside private inventory window");
    let origin=[800.-local[0],500.-local[1]];
    let size=[w.window.width(),w.window.height()];
    let corner=[origin[0]+size[0] as f32-1.,origin[1]+size[1] as f32-1.];
    input.perform(serde_json::json!([{"point":corner}]));pump(100);
    let captured=Rc::new(Cell::new(0usize));
    let pending=Rc::new(Cell::new(false));
    let attempts=Rc::new(Cell::new(0usize));
    let paintable=gtk::WidgetPaintable::new(Some(&w.window));
    let last=Rc::new(Cell::new(size));
    let clock=w.window.frame_clock().unwrap();
    let handler=clock.connect_after_paint(glib::clone!(#[strong] captured,#[strong] pending,#[strong] attempts,#[strong] paintable,#[strong] last,#[strong] output,#[weak] w,move|clock|{
        let dimensions=[w.window.width(),w.window.height()];
        if last.replace(dimensions)!=dimensions && captured.get()<2 && !pending.get() && attempts.get()<8 {
            let index=captured.get();pending.set(true);attempts.set(attempts.get()+1);
            let frame=clock.frame_time();
            glib::idle_add_local_once(glib::clone!(#[strong] output,#[strong] captured,#[strong] pending,#[strong] paintable,#[weak] w,move||{
                pending.set(false);
                let snapshot=gtk::Snapshot::new();
                paintable.snapshot(&snapshot,w.window.width() as f64,w.window.height() as f64);
                let Some(node)=snapshot.to_node() else{return};
                captured.set(index+1);
                node.write_to_file(output.join(format!("resize-{index}.node"))).unwrap();
                let widgets=widgets(w.window.upcast_ref()).map(|widget|{
                    let mut path=Vec::new();let mut current=Some(widget.clone());
                    while let Some(item)=current {path.push(format!("{}#{}",item.type_().name(),item.widget_name()));current=item.parent();}
                    path.reverse();
                    let bounds=widget.compute_bounds(&w.window).map(|b|[b.x(),b.y(),b.width(),b.height()]);
                    serde_json::json!({"pointer":format!("{:p}",widget.as_ptr()),"type":widget.type_().name(),"name":widget.widget_name().to_string(),"css_classes":widget.css_classes().iter().map(|s|s.as_str()).collect::<Vec<_>>(),"ancestry":path,"bounds":bounds,"mapped":widget.is_mapped()})
                }).collect::<Vec<_>>();
                let mut rows=Vec::new();inventory_nodes(&node,None,&mut Vec::new(),&mut Vec::new(),&mut rows);
                let report=serde_json::json!({"diagnostic":"render tree captured from GtkWidgetPaintable in an idle callback after a real changed-dimension native resize paint; no timing claim","theme":std::env::var("CAPY_NATIVE_TEST_THEME").unwrap(),"gtk_version":[gtk::major_version(),gtk::minor_version(),gtk::micro_version()],"source_frame_time_us":frame,"source_dimensions":dimensions,"captured_dimensions":[w.window.width(),w.window.height()],"capture_time_us":glib::monotonic_time(),"surface_transform":w.window.surface_transform(),"surface_scale":w.window.surface().unwrap().scale(),"widgets":widgets,"nodes":rows});
                std::fs::write(output.join(format!("resize-{index}.json")),serde_json::to_vec_pretty(&report).unwrap()).unwrap();
            }));
        }
    }));
    let mut events=vec![serde_json::json!({"down":true})];
    for i in 0..500 {let t=i as f32/500.;events.push(serde_json::json!({"point":[corner[0]+t*150.,corner[1]+t*80.]}));}
    events.push(serde_json::json!({"down":false}));
    input.perform(serde_json::to_value(events).unwrap());pump(300);
    clock.disconnect(handler);
    assert!(captured.get()>=1,"at least one real changing-frame render tree");
    assert!(w.window.width()>size[0]+100 && w.window.height()>size[1]+50,"outer window resize gesture");
    input.finish();w.window.destroy();pump(100);
}

fn inventory_nodes(node:&gtk::gsk::RenderNode,parent:Option<usize>,labels:&mut Vec<String>,transforms:&mut Vec<gtk::gsk::Transform>,rows:&mut Vec<serde_json::Value>) {
    use gtk::gsk;
    assert!(rows.len()<100000,"bounded render tree inventory");
    let id=rows.len();
    let bounds=node.bounds();let mut world=bounds;
    for transform in transforms.iter().rev(){world=transform.transform_bounds(&world);}
    let mut row=serde_json::json!({"id":id,"parent":parent,"pointer":format!("{:p}",node.as_ptr()),"type":format!("{:?}",node.node_type()),"bounds":[bounds.x(),bounds.y(),bounds.width(),bounds.height()],"window_bounds":[world.x(),world.y(),world.width(),world.height()],"debug_ancestry":labels,"transforms":transforms.iter().map(|t|format!("{t:?}")).collect::<Vec<_>>()});
    if let Some(n)=node.downcast_ref::<gsk::MaskNode>(){row["mode"]=serde_json::json!(format!("{:?}",n.mask_mode()));row["mask_root_type"]=serde_json::json!(format!("{:?}",n.mask().node_type()));}
    if let Some(n)=node.downcast_ref::<gsk::CrossFadeNode>(){row["progress"]=serde_json::json!(n.progress());}
    if let Some(n)=node.downcast_ref::<gsk::BlurNode>(){row["radius"]=serde_json::json!(n.radius());}
    if let Some(n)=node.downcast_ref::<gsk::OpacityNode>(){row["opacity"]=serde_json::json!(n.opacity());}
    if let Some(n)=node.downcast_ref::<gsk::BlendNode>(){row["blend_mode"]=serde_json::json!(format!("{:?}",n.blend_mode()));}
    if let Some(n)=node.downcast_ref::<gsk::CompositeNode>(){row["operator"]=serde_json::json!(format!("{:?}",n.operator()));}
    if let Some(n)=node.downcast_ref::<gsk::IsolationNode>(){row["isolations"]=serde_json::json!(format!("{:?}",n.isolations()));}
    let texture=node.downcast_ref::<gsk::TextureNode>().map(|n|n.texture()).or_else(||node.downcast_ref::<gsk::TextureScaleNode>().map(|n|n.texture()));
    if let Some(texture)=texture {row["texture"]=serde_json::json!({"pointer":format!("{:p}",texture.as_ptr()),"type":texture.type_().name(),"extent":[texture.width(),texture.height()],"format":format!("{:?}",texture.format()),"color_state":format!("{:?}",texture.color_state())});}
    if let Some(n)=node.downcast_ref::<gsk::TextureScaleNode>(){row["filter"]=serde_json::json!(format!("{:?}",n.filter()));}
    let debug=node.downcast_ref::<gsk::DebugNode>().map(|n|n.message().to_string());
    if let Some(label)=&debug{row["debug_label"]=serde_json::json!(label);}
    rows.push(row);
    if let Some(label)=&debug{labels.push(label.clone());}
    let transform=node.downcast_ref::<gsk::TransformNode>().map(|n|n.transform());
    if let Some(t)=&transform{transforms.push(t.clone());}
    for child in node.children(){inventory_nodes(&child,Some(id),labels,transforms,rows);}
    if transform.is_some(){transforms.pop();}
    if debug.is_some(){labels.pop();}
}

#[test]
#[ignore = "private native Vulkan; legacy/current converters on the same raw workspace snapshot"]
fn native_window_scene_pixels() {
    let output=std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap());std::fs::create_dir_all(&output).unwrap();
    let app=native_test_app("art.capycanvas.WindowScenePixels");let w=Workspace::new(&app);apply_fixture_theme(&w);w.window.set_default_size(1100,760);w.window.present();pump(1800);wait_workspaces(&w);until(||w.gpu.borrow().is_some(),"scene canvas ready");pump(1500);
    let mut input=RemoteInput::new();input.ready();if std::env::var_os("LAYER_NATIVE_CAPTURE_DIR").is_some(){input.perform(serde_json::json!([{"wait_ms":300},{"capture":"window-native"}]));}
    let wrapper=w.window.content().unwrap();assert!(wrapper.is::<crate::squircle::Squircles>());let child=wrapper.first_child().unwrap();let paintable=gtk::WidgetPaintable::new(Some(&child));pump(100);
    let raw=crate::with_canvas_snapshot(&w,||{
        for _ in 0..8 {let snapshot=gtk::Snapshot::new();paintable.snapshot(&snapshot,child.width() as f64,child.height() as f64);if let Some(node)=snapshot.to_node(){return node}pump(30);}
        panic!("raw workspace paintable did not snapshot");
    });
    let scale=w.window.surface().unwrap().scale();let legacy=crate::squircle::native_window_reference::converted(&raw,scale);let candidate=crate::squircle::converted(&raw,scale);
    for (name,node) in [("raw",&raw),("legacy",&legacy),("candidate",&candidate)] {node.write_to_file(output.join(format!("{name}.node"))).unwrap();}
    let viewport=gtk::graphene::Rect::new(0.,0.,child.width() as f32*scale as f32,child.height() as f32*scale as f32);let renderer=w.window.renderer().unwrap();
    let float=gdk::MemoryTexture::new(1,1,gdk::MemoryFormat::R32g32b32a32Float,&glib::Bytes::from_owned(vec![0u8;16]),16);
    let actual=crate::snapshot(&w);actual.save_to_png(output.join("window.png")).unwrap();std::fs::write(output.join("window-texture.json"),serde_json::to_vec_pretty(&resize_texture_metadata(&actual)).unwrap()).unwrap();
    let mut reports=Vec::new();let mut accepted=true;
    for float_target in [false,true] {
        let render=|node:&gtk::gsk::RenderNode|{let snapshot=gtk::Snapshot::new();if float_target{snapshot.append_texture(&float,&viewport);}snapshot.scale(scale as f32,scale as f32);snapshot.append_node(node);let texture=renderer.render_texture(snapshot.to_node().unwrap(),Some(&viewport));let mut downloader=gdk::TextureDownloader::new(&texture);downloader.set_color_state(&gdk::ColorState::srgb());downloader.set_format(gdk::MemoryFormat::R8g8b8a8Premultiplied);let(bytes,stride)=downloader.download_bytes();let mut floats=gdk::TextureDownloader::new(&texture);floats.set_color_state(&gdk::ColorState::srgb_linear());floats.set_format(gdk::MemoryFormat::R32g32b32a32FloatPremultiplied);let(data,row)=floats.download_bytes();for y in 0..texture.height() as usize{for v in data[y*row..y*row+texture.width() as usize*16].chunks_exact(4){assert!(f32::from_ne_bytes(v.try_into().unwrap()).is_finite(),"finite whole-scene HDR sample");}}(bytes,stride,texture)};
        let a=render(&legacy);let b=render(&candidate);a.2.save_to_png(output.join(format!("legacy-float{float_target}.png"))).unwrap();b.2.save_to_png(output.join(format!("candidate-float{float_target}.png"))).unwrap();assert_eq!(a.1,b.1);let mut max_error=0;let mut changed=0;let mut over1=0;let mut interior_max=0;let mut edge_max=0;let mut interior_bad=0;let mut edge_bad=0;let mut bad=Vec::new();
        let width=a.2.width() as usize;let height=a.2.height() as usize;let contours=crate::squircle::native_window_reference::contour_edges(&raw,scale,width as i32,height as i32);let mut edge_mask=vec![0u8;width*height];
        for y in 0..height {for x in 0..width {
            let gradient=(0..3).any(|c|{let mut lo=255;let mut hi=0;for yy in y.saturating_sub(1)..=(y+1).min(height-1){for xx in x.saturating_sub(1)..=(x+1).min(width-1){let v=a.0[yy*a.1+xx*4+c];lo=lo.min(v);hi=hi.max(v);}}hi-lo>=2});
            let edge=contours[y*width+x]>0||gradient;edge_mask[y*width+x]=if edge{255}else{0};
            let e=(0..4).map(|c|a.0[y*a.1+x*4+c].abs_diff(b.0[y*b.1+x*4+c])).max().unwrap();max_error=max_error.max(e);changed+=usize::from(e>0);over1+=usize::from(e>1);
            if edge{edge_max=edge_max.max(e);edge_bad+=usize::from(e>16);}else{interior_max=interior_max.max(e);interior_bad+=usize::from(e>1);}
            if ((edge&&e>16)||(!edge&&e>1))&&bad.len()<32{bad.push(serde_json::json!({"pixel":[x,y],"edge":edge,"delta":e,"reference":&a.0[y*a.1+x*4..y*a.1+x*4+4],"candidate":&b.0[y*b.1+x*4..y*b.1+x*4+4]}));}
        }}
        let mask=gdk::MemoryTexture::new(width as i32,height as i32,gdk::MemoryFormat::R8g8b8a8,&glib::Bytes::from_owned(edge_mask.into_iter().flat_map(|v|[v,v,v,255]).collect::<Vec<_>>()),width*4);mask.save_to_png(output.join(format!("reference-edges-float{float_target}.png"))).unwrap();
        accepted&=interior_bad==0&&edge_bad==0;reports.push(serde_json::json!({"float_target":float_target,"max_error":max_error,"changed_pixels":changed,"pixels_over1":over1,"interior_max_error":interior_max,"edge_max_error":edge_max,"interior_violations":interior_bad,"edge_over16":edge_bad,"first_violations":bad,"finite_linear_float_samples":true,"edge_reference":"reference-only3x3RGB range>=2 or2-device-pixel Cairo stroke of original squircle contours; no candidate-derived edge allowance","legacy_texture":resize_texture_metadata(&a.2),"candidate_texture":resize_texture_metadata(&b.2)}));
    }
    let report=serde_json::json!({"theme":std::env::var("CAPY_NATIVE_TEST_THEME").unwrap(),"surface_scale":scale,"dimensions":[child.width(),child.height()],"targets":reports,"comparison":"same raw native workspace node; legacy/current conversion; includes managed canvas snapshot; test-only"});std::fs::write(output.join("comparison.json"),serde_json::to_vec_pretty(&report).unwrap()).unwrap();eprintln!("{report}");input.finish();w.window.close();pump(100);assert!(accepted,"same-workspace flat interiors<=1 and independently identified narrow edges<=16; see comparison.json");
}

fn resize_texture_metadata(texture:&gdk::Texture)->serde_json::Value {
    let state=texture.color_state();let cicp=state.create_cicp_params().map(|p|serde_json::json!({"primaries":p.color_primaries(),"transfer":p.transfer_function(),"matrix":p.matrix_coefficients(),"range":format!("{:?}",p.range())}));
    serde_json::json!({"format":format!("{:?}",texture.format()),"dimensions":[texture.width(),texture.height()],"color_state":format!("{state:?}"),"srgb":state.equivalent(&gdk::ColorState::srgb()),"srgb_linear":state.equivalent(&gdk::ColorState::srgb_linear()),"rec2100_pq":state.equivalent(&gdk::ColorState::rec2100_pq()),"rec2100_linear":state.equivalent(&gdk::ColorState::rec2100_linear()),"cicp":cicp})
}

#[test]
#[ignore = "assigned Cintiq HDR display; isolated profile; programmatic own-window resize without input injection"]
fn native_window_resize_programmatic_cintiq() {
    let output=std::path::PathBuf::from(std::env::var_os("LAYER_TEST_ARTIFACTS").unwrap());std::fs::create_dir_all(&output).unwrap();
    let app=native_test_app("art.capycanvas.CintiqResourceBudget");let w=Workspace::new(&app);apply_fixture_theme(&w);w.window.set_default_size(1100,760);w.window.present();pump(1800);wait_workspaces(&w);until(||w.gpu.borrow().is_some(),"programmatic canvas ready");pump(1000);
    let surface=w.window.surface().unwrap();let display=surface.display();let target=(0..display.monitors().n_items()).filter_map(|i|display.monitors().item(i).and_downcast::<gdk::Monitor>()).find(|m|m.connector().as_deref()==Some("DP-10")&&m.model().is_some_and(|s|s.contains("Cintiq"))).expect("assigned Cintiq DP-10 must be connected");
    if display.monitor_at_surface(&surface).as_ref()!=Some(&target){w.window.fullscreen_on_monitor(&target);pump(500);w.window.unfullscreen();w.window.set_default_size(1100,760);pump(700);}
    let monitor=display.monitor_at_surface(&surface).unwrap();assert_eq!(monitor,target,"own diagnostic window must be on assigned Cintiq");assert!((surface.scale()-2.).abs()<0.001,"Cintiq native scale must already be2; no display reconfiguration");
    let runtime=serde_json::json!({"ui_thread_id":unsafe { libc::gettid() },"method":"programmatic GtkWindow.set_default_size afterpaint; no injected input","display":display.name().to_string(),"monitor_connector":monitor.connector().map(|s|s.to_string()),"monitor_model":monitor.model().map(|s|s.to_string()),"monitor_description":monitor.description().map(|s|s.to_string()),"surface_scale":surface.scale(),"refresh_millihz":monitor.refresh_rate(),"canvas_view_color":format!("{:?}",w.view_color()),"renderer":w.window.renderer().unwrap().type_().name(),"GTK":[gtk::major_version(),gtk::minor_version(),gtk::micro_version()],"HDR_confirmation":"requires captured physical DisplayConfig BT.2100 mode and actual GTK swapchain format/colorspace; floating screenshot is not evidence"});std::fs::write(output.join("runtime-metadata.json"),serde_json::to_vec_pretty(&runtime).unwrap()).unwrap();eprintln!("runtime: {runtime}");
    let clock=w.window.frame_clock().unwrap();let mut reports=Vec::new();
    let contacts=std::env::var("LAYER_NATIVE_RESIZE_CONTACTS").ok().and_then(|s|s.parse::<usize>().ok()).unwrap_or(3).clamp(1,3);
    for repeat in 0..=contacts {
        let begin=glib::monotonic_time();let end=begin+if repeat==0{2_000_000}else{6_000_000};let records=Rc::new(RefCell::new(Vec::new()));let retained=Rc::new(RefCell::new(Vec::new()));let last=Rc::new(Cell::new([w.window.width(),w.window.height()]));let paint=Rc::new(Cell::new((Instant::now(),crate::timing::thread_cpu_ms(),glib::monotonic_time())));
        let before=clock.connect_before_paint(glib::clone!(#[strong] paint,move|_|paint.set((Instant::now(),crate::timing::thread_cpu_ms(),glib::monotonic_time()))));
        let after=clock.connect_after_paint(glib::clone!(#[strong] records,#[strong] retained,#[strong] last,#[strong] paint,#[weak] w,move|clock|{
            let dimensions=[w.window.width(),w.window.height()];if last.replace(dimensions)!=dimensions {if let Some(timing)=clock.current_timings(){retained.borrow_mut().push((timing,records.borrow().len()));}records.borrow_mut().push(serde_json::json!({"before_paint_us":paint.get().2,"after_paint_us":glib::monotonic_time(),"frame_time_us":clock.frame_time(),"dimensions":dimensions,"paint_wall_ms":paint.get().0.elapsed().as_secs_f64()*1000.,"paint_thread_cpu_ms":crate::timing::thread_cpu_ms()-paint.get().1}));}
            let now=glib::monotonic_time();if now<end {let phase=((now-begin) as f64/1e6)%2.;let triangle=if phase<0.5{phase*2.}else if phase<1.5{2.-phase*2.}else{phase*2.-4.};let mut width=(1100.+80.*triangle).round() as i32;let height=(760.+50.*triangle).round() as i32;if [width,height]==dimensions{width+=if width<1180{1}else{-1};}glib::idle_add_local_once(glib::clone!(#[weak] w,move||w.window.set_default_size(width,height)));}
        }));
        w.window.set_default_size(1101,760);while glib::monotonic_time()<end{pump(5);}clock.disconnect(after);clock.disconnect(before);pump(400);
        let deadline=Instant::now()+Duration::from_millis(600);while retained.borrow().iter().any(|(t,_)|!t.is_complete())&&Instant::now()<deadline{pump(5);}
        let mut times=Vec::new();{let mut rows=records.borrow_mut();for(t,index)in retained.borrow().iter(){rows[*index]["complete"]=serde_json::json!(t.is_complete());rows[*index]["presentation_time_us"]=serde_json::json!(t.presentation_time());if t.is_complete()&&t.presentation_time()>0&&t.frame_time()>=begin&&t.frame_time()<=end{times.push(t.presentation_time());}}}times.sort_unstable();times.dedup();
        let rows=records.borrow();let percentile=|key:&str,p:usize|{let mut a=rows.iter().map(|r|r[key].as_f64().unwrap()).collect::<Vec<_>>();a.sort_by(f64::total_cmp);a.get(a.len()*p/100).copied().unwrap_or(0.)};let widths=rows.iter().map(|r|r["dimensions"][0].as_i64().unwrap()).collect::<Vec<_>>();let heights=rows.iter().map(|r|r["dimensions"][1].as_i64().unwrap()).collect::<Vec<_>>();let range=|a:&Vec<i64>|[a.iter().copied().min().unwrap_or(0),a.iter().copied().max().unwrap_or(0)];let wr=range(&widths);let hr=range(&heights);
        let mut report=serde_json::json!({"measurement":"resources-and-paint-time","method":"programmatic-afterpaint-no-input","priming":repeat==0,"repeat":repeat,"theme":std::env::var("CAPY_NATIVE_TEST_THEME").unwrap(),"begin_us":begin,"end_us":end,"contact_seconds":(end-begin) as f64/1e6,"changed_paint_frames":rows.len(),"width_range":wr,"height_range":hr,"surface_scale":surface.scale(),"paint_p50_ms":percentile("paint_wall_ms",50),"paint_p95_ms":percentile("paint_wall_ms",95),"paint_cpu_p50_ms":percentile("paint_thread_cpu_ms",50),"paint_cpu_p95_ms":percentile("paint_thread_cpu_ms",95)});
        if std::env::var_os("LAYER_NATIVE_RESIZE_RESOURCE_ONLY").is_none(){let mut gaps=times.windows(2).map(|p|(p[1]-p[0])as f64/1000.).collect::<Vec<_>>();gaps.sort_by(f64::total_cmp);report["measurement"]=serde_json::json!("programmatic-completed-changing-window-presentations; probe-off only");report["changed_completed_presentations"]=serde_json::json!(times.len());report["retained_timings"]=serde_json::json!(retained.borrow().len());report["missing_frame_timings"]=serde_json::json!(rows.len()-retained.borrow().len());report["incomplete_timings"]=serde_json::json!(retained.borrow().iter().filter(|(t,_)|!t.is_complete()).count());report["moving_hz"]=serde_json::json!(if times.len()>1{(times.len()-1)as f64*1e6/(times[times.len()-1]-times[0])as f64}else{0.});report["full_contact_hz"]=serde_json::json!(times.iter().filter(|t|**t>=begin&&**t<=end).count()as f64*1e6/(end-begin)as f64);report["gap_p99_ms"]=serde_json::json!(gaps.get(gaps.len()*99/100));report["gap_max_ms"]=serde_json::json!(gaps.last());report["onset_ms"]=serde_json::json!(times.first().map(|t|(*t-begin)as f64/1000.));report["tail_ms"]=serde_json::json!(times.last().map(|t|(end-*t)as f64/1000.));assert!(times.len()>20,"completed presentations while own HDR window changes size");}
        std::fs::write(output.join(format!("frames-{repeat}.json")),serde_json::to_vec_pretty(&*rows).unwrap()).unwrap();eprintln!("{report}");reports.push(report);std::fs::write(output.join("summary.json"),serde_json::to_vec_pretty(&reports).unwrap()).unwrap();assert!(rows.len()>10&&wr[1]-wr[0]>100&&hr[1]-hr[0]>60,"programmatic outer window must actually change dimensions");drop(rows);w.window.set_default_size(1100,760);pump(500);
    }
    w.window.close();pump(100);
}
