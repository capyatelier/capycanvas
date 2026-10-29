//! Native workspace projections and asynchronous preview packets.
use crate::previews::CapyPreview;
use layer_host::NativeHost;
use serde_json::{Value, json};
use std::ffi::{CString, c_char};
use layer_host::handles::{self,Handle};
use std::sync::{Arc,Mutex};

struct LayoutInput {
    input:layer_ui::LayoutResolveInput<'static>,
    resolved:Mutex<Option<Arc<layer_ui::ResolvedLayout>>>,
}

/// # Safety
/// Input is readable NUL-terminated UTF-8 layout JSON during this worker call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_layout_input_prepare(input:*const c_char)->u64 {
    std::panic::catch_unwind(|| {
        let layout=unsafe {crate::input::with_text(input,|text| {
            if text.len()>256*1024 {return Err("Panel layout is too large".into());}
            serde_json::from_str(text).map_err(|error|error.to_string())
        })}.ok()?;
        handles::insert_shared(Arc::new(LayoutInput {input:layout,resolved:Mutex::new(None)})).ok().map(Handle::raw)
    }).ok().flatten().unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn capy_layout_input_free(layout:u64) {
    let _=Handle::from_raw(layout).and_then(handles::release::<LayoutInput>);
}

/// # Safety
/// Input is readable, unchanged NUL-terminated JSON of native panel measurements during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_layout_input_measure(layout:u64,input:*const c_char)->u64 {
    std::panic::catch_unwind(|| {
        let input=unsafe {crate::input::read_json(input)}.ok()?;
        let source=Handle::from_raw(layout).and_then(handles::shared::<LayoutInput>).ok()?;
        let input=source.input.with_panel_measurements(input).ok()?;
        handles::insert_shared(Arc::new(LayoutInput {input,resolved:Mutex::new(None)})).ok().map(Handle::raw)
    }).ok().flatten().unwrap_or(0)
}

fn resolve_layout(input:u64,width:f32,height:f32)->Result<Arc<layer_ui::ResolvedLayout>,String> {
    if ![width,height].into_iter().all(|v|v.is_finite() && v>0.) {return Err("Invalid workspace viewport".into());}
    let input=Handle::from_raw(input).and_then(handles::shared::<LayoutInput>).map_err(|error|error.to_string())?;
    let mut cached=input.resolved.lock().map_err(|error|error.to_string())?;
    if cached.as_ref().is_none_or(|layout|layout.viewport!=[width,height]) {
        *cached=Some(Arc::new(input.input.resolve([width,height])));
    }
    Ok(Arc::clone(cached.as_ref().unwrap()))
}

fn layout_packet(result:Result<Value,String>)->u64 {
    let packet=match result {
        Ok(result)=>json!({"result":result,"error":null}),
        Err(error)=>json!({"result":null,"error":error}),
    };
    CString::new(packet.to_string()).map_or(0,crate::owned::text)
}

#[unsafe(no_mangle)]
pub extern "C" fn capy_layout_resolve(input:u64,width:f32,height:f32)->u64 {
    layout_packet(std::panic::catch_unwind(||resolve_layout(input,width,height).map(|layout|json!(layout.as_ref())))
        .unwrap_or_else(|_|Err("Workspace layout failed".into())))
}

/// # Safety
/// Input is readable NUL-terminated UTF-8 JSON during this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_drawer_resolve(layout:u64,width:f32,height:f32,input:*const c_char)->u64 {
    let result=std::panic::catch_unwind(|| {
        unsafe {crate::input::with_text(input,|source| {
            if source.len()>256*1024 {return Err("Drawer geometry request is too large".into());}
            let layout=Handle::from_raw(layout).and_then(handles::shared::<LayoutInput>).map_err(|error|error.to_string())?;
            layout.input.drawer_geometry([width,height],source)
        })}
    }).unwrap_or_else(|_|Err("Drawer geometry failed".into()));
    layout_packet(result)
}

/// # Safety
/// Input is readable NUL-terminated UTF-8 JSON for the duration of this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_panel_bridges(layout:u64,width:f32,height:f32,input: *const c_char) -> u64 {
    let result=std::panic::catch_unwind(|| {
        unsafe {crate::input::with_text(input, |text| {
            if text.len()>256*1024 {return Err("Panel geometry request is too large".into());}
            let measure=serde_json::from_str(text).map_err(|error|error.to_string())?;
            resolve_layout(layout,width,height)?.measured_panel_bridges(measure).map(|frame|json!(frame))
        })}
    }).unwrap_or_else(|_|Err("Panel geometry failed".into()));
    layout_packet(result)
}

fn request(json: &str) -> Result<Value, String> {
    if json.len() > 8192 {
        return Err("Workspace query is too large".into());
    }
    let value: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    match value.get("type").and_then(Value::as_str) {
        Some(
            "context"
            | "panel_handle_target"
            | "drop"
            | "workspace_drag_preview"
            | "layer_drop"
            | "image_layer_drop"
            | "drawer"
            | "layer_drawer_sizing"
            | "bubble_motion"
            | "bubble_grab"
            | "can_take_bubble"
            | "bubble_wheel_color"
            | "drawer_toolbar"
            | "expansion"
            | "renderer_stats"
            | "application_menu"
            | "application_menu_hover"
            | "application_link"
            | "header"
            | "palette_menu"
            | "palette_reorder_preview"
            | "palette_action"
            | "swatch_sheet"
            | "reveal_panel"
            | "canvas_bar_layout"
            | "canvas_bar_menu"
            | "canvas_bar_choice_menu"
            | "zoom_menu"
            | "action_tooltip"
            | "workspace_name_validation"
            | "profile_library_matches"
            | "stroke_recording",
        ) => Ok(value),
        _ => Err("Unsupported workspace query".into()),
    }
}

#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum StampQuery {
    ToolbarStamp { context: layer_ui::ToolbarContext, #[serde(default)] known_request: Option<u64> },
    BubbleStamp { #[serde(default)] known_request: Option<u64> },
    Scopes { #[serde(default)] size: Option<[u32; 2]> },
}

pub fn query(host: &mut NativeHost, json: &str) -> Result<CapyPreview, String> {
    match serde_json::from_str(json) {
        Ok(StampQuery::ToolbarStamp { context, known_request }) => return stamp(host, context, known_request),
        Ok(StampQuery::BubbleStamp { known_request }) => {
            let context = host.session.state().toolbar_context();
            return stamp(host, context, known_request);
        }
        Ok(StampQuery::Scopes { size }) => return crate::scopes::query(host.session.state(), size),
        Err(_) => {}
    }
    let (result, error) = match request(json).and_then(|value| host.query(value)) {
        Ok(result) => (result, Value::Null),
        Err(error) => (Value::Null, Value::String(error)),
    };
    CapyPreview::packet(json!({"result":result,"error":error}), Vec::new())
}

fn stamp(host: &mut NativeHost, context: layer_ui::ToolbarContext, known_request: Option<u64>) -> Result<CapyPreview, String> {
    match host.session.toolbar_stamp(context, known_request) {
            Ok(Some(stamp)) => CapyPreview::packet(
                json!({"result":{"size":stamp.size,"extent":stamp.extent,"request_id":stamp.request_id,"pending":stamp.pending},"error":null}),
                stamp.alpha,
            ),
            Ok(None) => CapyPreview::packet(json!({"result":null,"error":null}), Vec::new()),
            Err(error) => CapyPreview::packet(json!({"result":null,"error":error}), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use crate::test_support::*;
    use super::*;
    #[test]
    fn prepared_drawer_geometry_matches_shared_input_without_mutating_session() {
        let mut host=NativeHost::new(layer_ui::Platform::Windows).unwrap();
        host.dispatch(layer_ui::UiAction::RestoreWorkspace {workspace:Box::new(layer_ui::WorkspaceState::default())}).unwrap();
        host.resize(1600,1000,1.).unwrap();
        let panel=layer_ui::Panel::Toolbar;
        let tile=host.session.state().workspace.layout.panel(panel).unwrap().tiles().iter()
            .find(|tile|tile.control==layer_ui::ToolbarControl::Color).unwrap().id;
        host.dispatch(layer_ui::UiAction::ActivateTile {panel,tile}).unwrap();
        let revision=host.session.engine().document().revision;
        let state=host.session.state();let model=state.customization.drawer.as_ref().unwrap();
        let request=json!({"model":model,"column_drawers":state.customization.column_drawers,"tiles":[],
            "heights":vec![360.;model.columns.len()],"toolbars":true}).to_string();
        let expected=host.session.layout_resolve_input().drawer_geometry(host.logical,&request).unwrap();
        let source=CString::new(json!(host.session.layout_resolve_input()).to_string()).unwrap();
        let input=unsafe {capy_layout_input_prepare(source.as_ptr())};assert_ne!(input,0);
        let request=CString::new(request).unwrap();
        let read=|handle|serde_json::from_str::<Value>(crate::owned::take_text(handle).to_str().unwrap()).unwrap();
        let reply=read(unsafe {capy_drawer_resolve(input,1600.,1000.,request.as_ptr())});
        assert_eq!(reply["result"],expected);assert!(reply["error"].is_null());
        assert!(read(unsafe {capy_drawer_resolve(0,1600.,1000.,request.as_ptr())})["error"].is_string());
        assert!(read(unsafe {capy_drawer_resolve(input,1600.,1000.,std::ptr::null())})["error"].is_string());
        assert!(read(unsafe {capy_drawer_resolve(input,0.,1000.,request.as_ptr())})["error"].is_string());
        assert_eq!(host.session.engine().document().revision,revision);
        capy_layout_input_free(input);
    }

    #[test]
    fn measured_layout_input_retains_source_and_matches_shared_session() {
        let mut host=NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let document_revision=host.session.engine().document().revision;
        let input=CString::new(json!(host.session.layout_resolve_input()).to_string()).unwrap();
        let original=unsafe {capy_layout_input_prepare(input.as_ptr())};assert_ne!(original,0);
        let before=resolve_layout(original,1400.,900.).unwrap();
        let request=before.measurement_requests.iter().find(|request|request.identity.panel==layer_ui::Panel::Histogram).unwrap().clone();
        let context=layer_ui::PanelMetricContext {generation:1,scale:1.,text_scale:1.,style:"native-test".into()};
        let measurements=layer_ui::PanelMeasurementBatch {context:context.clone(),reports:vec![layer_ui::PanelMeasurementReport {
            request:request.clone(),context,tab_width:Some(110.),body:Some(layer_ui::PanelBodyMeasurement {
                actual_width:request.width,content_height:385.,scroll:None,
            }),
        }]};
        let input=CString::new(json!(measurements).to_string()).unwrap();
        let prepared=unsafe {capy_layout_input_measure(original,input.as_ptr())};assert_ne!(prepared,0);
        assert!(host.session.state().workspace.layout.measurements.is_empty());
        host.session.dispatch(layer_ui::UiAction::MeasurePanels {measurements}).unwrap();
        assert_eq!(json!(resolve_layout(prepared,1400.,900.).unwrap().as_ref()),json!(host.session.layout([1400.,900.])));
        assert!(Arc::ptr_eq(&before,&resolve_layout(original,1400.,900.).unwrap()));
        assert_eq!(host.session.engine().document().revision,document_revision);
        let invalid=c"[{\"panel\":\"histogram\",\"tab_width\":0,\"content_height\":-1}]";
        assert_eq!(unsafe {capy_layout_input_measure(original,invalid.as_ptr())},0);
        assert_eq!(unsafe {capy_layout_input_measure(original,std::ptr::null())},0);
        capy_layout_input_free(prepared);capy_layout_input_free(original);
    }
    fn metadata(packet: CapyPreview) -> Value {
        let owned=crate::previews::store(packet).unwrap();
        let result=crate::previews::copied_metadata(owned);
        assert_eq!(crate::previews::capy_preview_length(owned),0);
        crate::previews::capy_preview_free(owned);result
    }

    #[test]
    fn windows_panel_bridges_reuse_owned_layouts_for_current_measurements() {
        let mut host=NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let revision=host.session.engine().document().revision;
        let layout=host.session.layout([1400.,900.]);
        let groups:Vec<_>=layout.groups.iter().map(|g|json!([g.id,g.bounds])).collect();
        let columns:Vec<_>=layout.collapsed.iter().map(|c|json!({"column":c.id,"bounds":c.bounds,"content":c.content,
            "icons":c.groups.iter().flat_map(|g|&g.icons).map(|i|json!([i.panel,i.bounds])).collect::<Vec<_>>()})).collect();
        let query=json!({"layout_input":host.session.layout_resolve_input(),"measure":{"groups":groups,"columns":columns}});
        let prepare=|layout:&Value| {
            let input=CString::new(layout.to_string()).unwrap();
            let context=std::thread::spawn(move||unsafe {capy_layout_input_prepare(input.as_ptr())}).join().unwrap();
            assert_ne!(context,0);context
        };
        let context=prepare(&query["layout_input"]);
        let resolve=|context,viewport:[f32;2]| {
            let packet=capy_layout_resolve(context,viewport[0],viewport[1]);
            serde_json::from_str::<Value>(crate::owned::take_text(packet).to_str().unwrap()).unwrap()
        };
        let project=|context,request:&Value| {
            let input=CString::new(request.to_string()).unwrap();
            let packet=unsafe {capy_panel_bridges(context,1400.,900.,input.as_ptr())};
            assert_ne!(packet,0);
            serde_json::from_str::<Value>(crate::owned::take_text(packet).to_str().unwrap()).unwrap()
        };
        let initial=project(context,&query["measure"]);
        assert!(initial["error"].is_null());
        for viewport in [[1400.,900.],[640.,480.],[1700.,1100.]] {
            let projected=resolve(context,viewport);
            assert!(projected["error"].is_null());
            assert_eq!(projected["result"],json!(host.session.layout(viewport)));
        }
        let previous=resolve_layout(context,1700.,1100.).unwrap();
        for viewport in [[0.,900.],[-1.,900.],[f32::NAN,900.],[1400.,f32::INFINITY]] {
            assert!(resolve(context,viewport)["error"].is_string());
        }
        assert!(Arc::ptr_eq(&previous,&resolve_layout(context,1700.,1100.).unwrap()));
        assert_eq!(project(context,&query["measure"]),initial);
        resolve(context,[1400.,900.]);
        let cached=resolve_layout(context,1400.,900.).unwrap();
        resolve(context,[1400.,900.]);
        assert!(Arc::ptr_eq(&cached,&resolve_layout(context,1400.,900.).unwrap()));
        let result=project(context,&query["measure"]);
        assert!(result["error"].is_null());
        let expected=layout.clone().measured_panel_bridges(serde_json::from_value(query["measure"].clone()).unwrap()).unwrap();
        assert_eq!(result["result"],json!(expected));
        let mut partial=query["measure"].clone();partial["groups"]=json!([]);
        assert!(project(context,&partial)["error"].is_null());
        assert_eq!(project(context,&query["measure"]),result);
        assert!(Arc::ptr_eq(&cached,&resolve_layout(context,1400.,900.).unwrap()));
        assert_eq!(json!(cached.as_ref()),resolve(context,[1400.,900.])["result"]);
        let actual=result["result"]["groups"].as_array().unwrap();
        assert_eq!(actual.len(),layout.groups.len());
        for group in &layout.groups {
            let projected=actual.iter().find(|g|g["group"]==group.id).unwrap();
            assert_eq!(projected["bounds"],json!(group.bounds));
            assert_eq!(projected["radii"].as_array().unwrap().len(),4);
        }
        let prepared=layer_ui::PreparedWorkspace::new(layer_ui::WorkspaceCapture::from_template(
            &layer_ui::WorkspacePreset::Painter.layout(layer_ui::Platform::Windows)).unwrap()).unwrap();
        host.session.adopt_workspace(prepared).unwrap();
        host.resize(720,1200,1.5).unwrap();
        assert_ne!(json!(host.session.layout([1400.,900.])),json!(layout));
        assert_eq!(project(context,&query["measure"]),result);
        let mut retired=query.clone();
        retired["measure"]["groups"].as_array_mut().unwrap()
            .push(json!([u32::MAX,{"x":0,"y":0,"width":1,"height":1}]));
        assert_eq!(project(context,&retired["measure"]),result);
        assert!(!layout.groups.is_empty());
        for mutation in 0..2 {
            let mut invalid=query.clone();
            if mutation==0 {
                let duplicate=invalid["measure"]["groups"][0].clone();
                invalid["measure"]["groups"].as_array_mut().unwrap().push(duplicate);
            } else {
                invalid["measure"]["groups"][0][1]["width"]=json!(-1);
            }
            let rejected=project(context,&invalid["measure"]);
            assert!(rejected["result"].is_null());
            assert_eq!(rejected["error"],"Invalid panel measurements");
        }
        let mut moved=query.clone();
        moved["measure"]["groups"][0][1]["x"]=json!(123.);
        let moved_result=project(context,&moved["measure"]);
        assert!(moved_result["error"].is_null());
        assert_ne!(moved_result,result);
        assert_eq!(project(context,&query["measure"]),result);
        for invalid in [json!({}),json!({"layout":null,"measure":{}})] {
            assert!(project(context,&invalid)["result"].is_null());
            assert!(project(context,&invalid)["error"].is_string());
        }
        let null=unsafe {capy_panel_bridges(context,1400.,900.,std::ptr::null())};
        assert!(serde_json::from_str::<Value>(crate::owned::take_text(null).to_str().unwrap()).unwrap()["error"].is_string());
        let mut revised=query["layout_input"].clone();
        revised["layout"]["bands"]=json!([]);revised["layout"]["floating"]=json!([]);
        revised["layout"]["column_stacks"]=json!([]);
        revised["column_open"]=json!([]);
        let newer=prepare(&revised);
        assert_ne!(resolve(newer,[1400.,900.]),resolve(context,[1400.,900.]));
        assert_ne!(project(newer,&query["measure"]),result);
        assert_eq!(project(context,&query["measure"]),result);
        let retained=Handle::from_raw(context).and_then(handles::shared::<LayoutInput>).unwrap();
        capy_layout_input_free(context);
        assert!(project(context,&query["measure"])["error"].is_string());
        assert!(resolve(context,[1400.,900.])["error"].is_string());
        assert_eq!(json!(retained.input.resolve([1400.,900.]).measured_panel_bridges(serde_json::from_value(query["measure"].clone()).unwrap()).unwrap()),result["result"]);
        let reused=prepare(&query["layout_input"]);
        resolve(reused,[1400.,900.]);
        assert_ne!(reused,context);
        assert_eq!(project(reused,&query["measure"]),result);
        assert!(project(context,&query["measure"])["error"].is_string());
        capy_layout_input_free(newer);capy_layout_input_free(reused);
        assert_eq!(unsafe {capy_layout_input_prepare(std::ptr::null())},0);
        let invalid=CString::new("{}").unwrap();
        assert_eq!(unsafe {capy_layout_input_prepare(invalid.as_ptr())},0);
        assert!(request(&json!({"type":"panel_bridges"}).to_string()).is_err());
        assert_eq!(revision,host.session.engine().document().revision);
        assert!(host.error.is_none());
    }

    #[test]
    fn renderer_stats_distinguish_submissions_and_retire_surface_feedback() {
        let mut host=NativeHost::new(layer_ui::Platform::Windows).unwrap();
        host.set_localization(layer_ui::Localizer::shared(layer_ui::UiLanguage::English));
        let observer=host.canvas_frame_rate.clone();
        observer.submitted();
        let packet=metadata(super::query(&mut host,r#"{"type":"renderer_stats"}"#).unwrap());
        let rows=packet["result"]["rows"].as_array().unwrap();
        let submitted=rows.iter().find(|row|row["label"]=="Submitted FPS").unwrap();
        assert_eq!(submitted["value"],"1");
        assert_eq!(rows.iter().find(|row|row["label"]=="Frames").unwrap()["value"],"0");
        host.reset_canvas_frame_rate();
        observer.presented(1,1);
        let packet=metadata(super::query(&mut host,r#"{"type":"renderer_stats"}"#).unwrap());
        let rows=packet["result"]["rows"].as_array().unwrap();
        assert_eq!(rows.iter().find(|row|row["label"]=="FPS").unwrap()["value"],"—");
        assert!(!rows.iter().any(|row|row["label"]=="Submitted FPS"));
    }

    #[test]
    fn prompt_validation_and_profile_search_packets_use_current_shared_policy() {
        let mut host=NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let before=host.session.engine().document().clone();
        let entry=layer_ui::profile_library::ProfileEntry {
            id:"a".repeat(64),bytes:2048,name:String::new(),channels:Some(layer_core::color::ProfileChannels::Gray),profile:None,issue:None,
        };
        for language in layer_ui::UiLanguage::ALL {
            let localization=layer_ui::Localizer::shared(language);
            host.set_localization(localization.clone());
            for name in ["", "  ", "bad\nname", "Painting"] {
                let request=json!({"type":"workspace_name_validation","name":name});
                let actual=metadata(query(&mut host,&request.to_string()).unwrap());
                assert!(actual["error"].is_null(),"{actual}");
                assert_eq!(actual["result"],json!(layer_ui::workspace_name_validation(name,&localization)));
            }
            for text in [entry.display_name(&localization),entry.details(&localization),"A".repeat(64),"not a profile".into()] {
                let request=json!({"type":"profile_library_matches","entry":entry,"query":text});
                let actual=metadata(query(&mut host,&request.to_string()).unwrap());
                assert!(actual["error"].is_null(),"{actual}");
                assert_eq!(actual["result"],json!(entry.matches(&text,&localization)));
            }
            assert_authored_eq(host.session.engine().document(),&before);
        }
    }
    #[test]
    fn bubble_queries_forward_shared_policy_without_document_edits() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let before = host.session.engine().document().clone();
        for request in [
            json!({"type":"bubble_motion","now_ns":100,"reduced":true}),
            json!({"type":"bubble_grab","grab":{"kind":"header","id":9999},"held":false}),
            json!({"type":"can_take_bubble","grab":{"kind":"header","id":9999},"position":[320,240],"viewport":[1280,800]}),
            json!({"type":"bubble_wheel_color"}),
        ] {
            let expected = host.query(request.clone()).unwrap();
            let actual = metadata(query(&mut host, &request.to_string()).unwrap());
            assert!(actual["error"].is_null(), "{actual}");
            assert_eq!(actual["result"], expected);
            assert_authored_eq(host.session.engine().document(), &before);
        }
        let malformed = metadata(query(&mut host, r#"{"type":"bubble_grab","grab":{"kind":"header","id":9999}}"#).unwrap());
        assert!(malformed["error"].is_string());
        assert_authored_eq(host.session.engine().document(), &before);
    }
    #[test]
    fn layer_drop_query_validates_epoch_and_preserves_document_and_history() {
        use layer_ui::{LayerAction, Platform, UiAction};
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        host.dispatch(UiAction::Layer {
            action: LayerAction::New {
                group: true,
                clipped: false,
            },
        })
        .unwrap();
        let group = host.session.engine().document().working.occurrence.map(layer_ui::occurrence_token).unwrap();
        let epoch = host.session.state().document_file.epoch;
        let before = host.session.engine().document().clone();
        for requested_epoch in [epoch, epoch + 1] {
            let reply = metadata(query(&mut host, &json!({
                "type":"layer_drop", "epoch":requested_epoch, "id":1, "target":group, "fraction":0.5, "surface":"row"
            }).to_string()).unwrap());
            assert!(reply["error"].is_null());
            assert_eq!(reply["result"]["epoch"], epoch);
            assert_eq!(reply["result"]["target"], if requested_epoch == epoch { json!(group) } else { Value::Null });
            assert_eq!(
                reply["result"]["position"],
                if requested_epoch == epoch {
                    json!("into")
                } else {
                    Value::Null
                }
            );
            assert_authored_eq(host.session.engine().document(),&before);
        }
        host.dispatch(UiAction::Invoke {
            command: layer_ui::CommandId::Undo,
        })
        .unwrap();
        assert!(
            host.session
                .engine()
                .document()
                .scene().occurrence(layer_ui::occurrence_handle(group).unwrap())
                .is_none()
        );
    }

    #[test]
    fn toolbar_stamp_returns_alpha_bytes_and_rejects_stale_context() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let gpu=layer_render_wgpu::WgpuRasterizer::new_native_headless(
            host.session.engine().document().composition().color).unwrap();
        host.session.replace_renderer(layer_host::Renderer(Some(gpu.into()))).unwrap();
        let context = host.session.state().toolbar_context();
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(15);
        let owned=loop {
            let packet=query(&mut host,&json!({"type":"toolbar_stamp","context":context}).to_string()).unwrap();
            let owned=crate::previews::store(packet).unwrap();
            let reply=crate::previews::copied_metadata(owned);
            assert!(reply["error"].is_null(),"{reply}");
            if !reply["result"].is_null() && reply["result"]["pending"]==false {break owned;}
            crate::previews::capy_preview_free(owned);
            assert!(std::time::Instant::now()<deadline,"GPU toolbar stamp did not complete: {reply}");
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        {
            let reply=crate::previews::copied_metadata(owned);
            assert!(reply["error"].is_null(), "{reply}");
            let size = reply["result"]["size"].as_u64().unwrap() as usize;
            assert!(reply["result"]["extent"].as_f64().unwrap() > 0.);
            let bytes=crate::previews::copied_bytes(owned);let count=bytes.len();
            assert_eq!(count, size * size);
            assert!(bytes.iter().any(|&a| a > 0));
            let alpha = bytes;
            let (bubble, bubble_metadata) = loop {
                let bubble = crate::previews::store(query(&mut host, r#"{"type":"bubble_stamp"}"#).unwrap()).unwrap();
                let bubble_metadata=crate::previews::copied_metadata(bubble);
                assert!(bubble_metadata["error"].is_null(), "{bubble_metadata}");
                if bubble_metadata["result"]["pending"] == false { break (bubble, bubble_metadata); }
                crate::previews::capy_preview_free(bubble);
                assert!(std::time::Instant::now() < deadline, "GPU bubble stamp did not complete: {bubble_metadata}");
                std::thread::sleep(std::time::Duration::from_millis(1));
            };
            for field in ["size", "extent", "pending"] {
                assert_eq!(bubble_metadata["result"][field], reply["result"][field]);
            }
            assert!(bubble_metadata["result"]["request_id"].is_u64());
            let bubble_bytes=crate::previews::copied_bytes(bubble);let bubble_count=bubble_bytes.len();
            assert_eq!(bubble_count, count);
            assert_eq!(bubble_bytes, alpha);
            crate::previews::capy_preview_free(bubble);
            let cached = metadata(query(&mut host, &json!({"type":"bubble_stamp","known_request":bubble_metadata["result"]["request_id"]}).to_string()).unwrap());
            assert!(cached["error"].is_null());
            assert_eq!(cached["result"]["request_id"], bubble_metadata["result"]["request_id"]);
            crate::previews::capy_preview_free(owned);
        }
        let mut stale = context;
        stale.generation += 1;
        let reply = metadata(
            query(&mut host, &json!({"type":"toolbar_stamp","context":stale}).to_string()).unwrap(),
        );
        assert!(reply["result"].is_null());
        assert!(reply["error"].is_string());
    }

    #[test]
    fn native_startup_waits_for_workspace_adoption_and_saved_workspaces_remain_authoritative() {
        use layer_ui::{Platform, UiAction, WorkspaceState};
        let mut host = NativeHost::new(Platform::Windows).unwrap();
        assert!(!host.session.state().workspace_ready);
        assert!(host.session.state().requests.is_empty());
        let saved = WorkspaceState::default();
        host.dispatch(UiAction::RestoreWorkspace {
            workspace: Box::new(saved.clone()),
        })
        .unwrap();
        assert_eq!(host.session.state().workspace, saved);
        assert!(host.session.state().requests.is_empty());
    }
    #[test]
    fn titlebar_measurements_publish_native_snapshot_insets() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        host.session.begin_workspace_transition().unwrap();
        for insets in [[0., 138., 48.], [0., 92., 32.], [0.; 3]] {
            host.dispatch(layer_ui::UiAction::MeasureTitlebar { insets }).unwrap();
            let snapshot: Value = serde_json::from_slice(&host.take_update_bytes().unwrap().unwrap()).unwrap();
            assert_eq!(snapshot["titlebar_insets"], json!(insets));
        }
        host.session.end_workspace_transition();
    }
    #[test]
    fn workspace_queries_follow_shared_policy_without_mutating_the_document() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let revision = host.session.engine().document().revision;
        for query in [
            json!({"type":"application_menu","menu":"edit"}),
            json!({"type":"application_link","link":"website"}),
            json!({"type":"application_link","link":"source_code"}),
            json!({"type":"renderer_stats"}),
            json!({"type":"panel_handle_target","item":{"kind":"panel","panel":"layers"}}),
            json!({"type":"drawer","column":null,"heights":[],"progress":1}),
            json!({"type":"expansion","panel":"toolbar","heights":[0,420],"progress":1}),
            json!({"type":"header","request":{"op":"geometry","model":host.session.state().workspace.layout.header.projected_for(layer_ui::Platform::Windows),"editing":false,"documents":1,"width":1400,"insets":[0,0],"metrics":[]}}),
            json!({"type":"swatch_sheet","query":"","current":layer_core::color::RgbColor::WHITE}),
            json!({"type":"swatch_sheet","query":"zzz","current":layer_core::color::RgbColor::WHITE}),
        ] {
            let expected = host.query(query.clone()).unwrap();
            let result = metadata(super::query(&mut host, &query.to_string()).unwrap());
            assert_eq!(result["result"], expected);
            assert!(result["error"].is_null());
        }
        assert_eq!(revision, host.session.engine().document().revision);
    }
    #[test]
    fn windows_tab_capture_and_preview_use_frozen_shared_insertion() {
        use layer_ui::{Bounds, ContactPhase, DockItem, Panel, TabHit, UiAction};
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        host.resize(986, 658, 1.).unwrap();
        let group = host
            .session
            .layout([986., 658.])
            .groups
            .into_iter()
            .find(|g| g.panels.contains(&Panel::ToolSettings))
            .unwrap();
        let clip = Bounds {
            height: 36.,
            ..group.bounds
        };
        let tabs = vec![
            TabHit {
                group: group.id,
                index: 0,
                bounds: Bounds { width: 80., ..clip },
            },
            TabHit {
                group: group.id,
                index: 1,
                bounds: Bounds {
                    x: clip.x + 80.,
                    width: 120.,
                    ..clip
                },
            },
        ];
        let press = [clip.x + 2., clip.y + 18.];
        let item = DockItem::Panel {
            panel: Panel::ToolSettings,
        };
        let action = |phase, position| UiAction::DragWorkspace {
            item,
            phase,
            position,
            viewport: [986., 658.],
            tabs: tabs.clone(),
        };
        let down: crate::actions::Action = serde_json::from_value(json!({
            "windows_tab_drag":{"tabs":tabs,"clip":clip},
            "action":action(ContactPhase::Down,press)
        }))
        .unwrap();
        down.dispatch(&mut host).unwrap();
        let moved = [press[0] + 65., press[1]];
        host.dispatch(action(ContactPhase::Move, moved)).unwrap();
        let query = json!({"type":"workspace_drag_preview","item":item,"tabs":tabs,"position":moved});
        let result = metadata(super::query(&mut host, &query.to_string()).unwrap());
        assert!(result["error"].is_null());
        assert_eq!(result["result"]["tab"]["insertion"], 2);
        assert_eq!(result["result"]["tab"]["offsets"][1]["x"], -80.);
        // Release without an additional motion uses the identical partition.
        host.dispatch(action(ContactPhase::Up, moved)).unwrap();
        assert_eq!(
            host.session
                .state()
                .workspace
                .layout
                .group_panels(group.id)
                .unwrap(),
            &[Panel::Sizes, Panel::ToolSettings]
        );
        let ended = metadata(super::query(&mut host, &query.to_string()).unwrap());
        assert!(ended["result"]["tab"].is_null());
    }
    #[test]
    fn optional_queries_reject_mutations_oversize_and_stale_geometry_without_failing_host() {
        let mut host = NativeHost::new(layer_ui::Platform::Windows).unwrap();
        let revision = host.session.engine().document().revision;
        for query in [
            r#"{"type":"load_filter_package"}"#.to_owned(),
            r#"{"type":"invoke","command":"new_layer"}"#.to_owned(),
            r#"{"type":"drawer_toolbar","panel":"layers","width":0,"height":480}"#.to_owned(),
            " ".repeat(8193),
            "{".to_owned(),
        ] {
            let result = metadata(super::query(&mut host, &query).unwrap());
            assert!(result["result"].is_null());
            assert!(result["error"].is_string());
        }
        assert_eq!(revision, host.session.engine().document().revision);
        assert!(
            metadata(super::query(&mut host, r#"{"type":"renderer_stats"}"#).unwrap())["error"]
                .is_null()
        );
    }
}
