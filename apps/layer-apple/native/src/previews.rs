//! Owned picker atlases and live overview layout. No per-channel JSON or GPU wait.
use super::*;
use layer_render::FilterPreviewImage;

/// # Safety
/// Stateless geometry on any thread. Input is a readable NUL-terminated JSON
/// object containing a published ResolvedLayout, point, pointer kind and contact.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_edge_reach(json: *const c_char) -> u32 {
    #[derive(serde::Deserialize)]
    struct Request {
        layout: layer_ui::ResolvedLayout,
        position: [f32; 2],
        kind: layer_ui::PointerKind,
        contact: layer_ui::EdgeContact,
    }
    if json.is_null() { return 0 }
    std::panic::catch_unwind(|| {
        let request: Request = serde_json::from_str(unsafe { CStr::from_ptr(json) }.to_str().ok()?).ok()?;
        request.layout.edge_grab_at(request.position, request.kind, request.contact)
    }).ok().flatten().map_or(0, |grab| {
        1 | u32::from(grab.resizes(layer_ui::Axis::Vertical)) << 1
    })
}

/// # Safety
/// Serial owner call; JSON is an array of logical bounds/clip/order records.
/// The layout is immutable until the next call. No preview pixels cross the ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_navigator_placements(
    app: u64,
    json: *const c_char,
) -> i32 {
    let Some(mut lease_app)=handles::app(app) else {
        return -1;
    };
    let app=&mut *lease_app;
    app.perform(|app| {
        if json.is_null() {
            return Err("Missing Navigator geometry".into());
        }
        let source = unsafe { CStr::from_ptr(json) }
            .to_str()
            .map_err(|e| e.to_string())?;
        if source.len() > 16 * 1024 {
            return Err("Navigator geometry exceeds the layout transport limit".into());
        }
        let slots = serde_json::from_str(source).map_err(|e| e.to_string())?;
        if app.metal.navigators.set(slots)? {
            app.host.dirty = true;
        }
        Ok(0)
    })
    .unwrap_or(-1)
}

/// # Safety
/// Serial owner call; GlassLayout contains logical regions [x,y,w,h,tl,tr,br,bl,shape,strength],
/// arcs [cx,cy,radius,band,start_radians,sweep_radians,strength], color_bubbles
/// [cx,cy,wheel_radius,arc_radius,cap,end_angle_radians,strength], panel surfaces and DrawerConnection records.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_glass_regions(app: u64, json: *const c_char) -> i32 {
    let Some(mut lease_app)=handles::app(app) else {
        return -1;
    };
    let app=&mut *lease_app;
    app.perform(|app| {
        if json.is_null() {
            return Err("Missing glass geometry".into());
        }
        let source = unsafe { CStr::from_ptr(json) }.to_str().map_err(|e| e.to_string())?;
        if source.len() > 64 * 1024 {
            return Err("Glass geometry exceeds the layout transport limit".into());
        }
        let layout = serde_json::from_str(source).map_err(|e| e.to_string())?;
        if app.metal.glass.set(layout)? {
            app.host.dirty = true;
        }
        Ok(0)
    })
    .unwrap_or(-1)
}

/// # Safety
/// Stateless geometry on any thread. JSON is [Camera, documentExtent, viewport].
/// The returned JSON is owned and uses the ordinary string-free function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_navigator_geometry(json: *const c_char) -> u64 {
    unsafe { stateless_json(json, "Missing Navigator geometry", |source| {
        let (camera, document, viewport): (layer_ui::Camera, [u32; 2], [f32; 2]) =
            serde_json::from_str(source).map_err(|e| e.to_string())?;
        serde_json::to_value(layer_ui::NavigatorGeometry::new(&camera, document, viewport))
            .map_err(|e| e.to_string())
    }) }
}

pub struct CapyFilterPreviews {
    atlas: FilterPreviewImage,
    filters: CString,
}

#[derive(Default)]
#[repr(C)]
pub struct CapyFilterPreviewInfo {
    pub request: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub count: usize,
    pub filters: u64,
}

/// # Safety
/// Call on the serial editor owner. The returned allocation has no editor/GPU
/// references and can be read and freed on an image worker after editor teardown.
/// The shared query already retires stale requests without an editor error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_apple_take_filter_previews(
    app: u64,
) -> u64 {
    let Some(mut lease_app)=handles::app(app) else {
        return 0;
    };
    let app=&mut *lease_app;
    app.perform(|app| {
        let Some(atlas) = app.host.take_filter_preview_image() else {
            return Ok(0);
        };
        let filters =
            CString::new(serde_json::to_string(&atlas.filters).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        Ok(handles::store(CapyFilterPreviews {
            atlas,
            filters,
        }))
    })
    .unwrap_or(0)
}

/// # Safety
/// Previews must be live; output must be aligned and exclusively writable.
/// Metadata text is owned; pixel copies retain the source for each call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_filter_previews_read(
    previews: u64,
    output: *mut CapyFilterPreviewInfo,
) -> bool {
    if output.is_null() { return false; }
    unsafe {output.write(CapyFilterPreviewInfo::default());}
    let Some(lease_previews)=handles::shared::<CapyFilterPreviews>(previews) else {
        return false;
    };
    let previews=&*lease_previews;
    let image = &previews.atlas.image;
    unsafe { output.write(CapyFilterPreviewInfo {
        request: image.request_id,
        width: image.width,
        height: image.height,
        stride: image.stride,
        count: image.bytes.len(),
        filters: handles::text(previews.filters.clone()),
    }); }
    true
}

/// # Safety
/// Nonempty output is writable and exclusive for length bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_filter_previews_copy(previews:u64,offset:usize,output:*mut u8,length:usize)->bool {
    handles::shared::<CapyFilterPreviews>(previews).is_some_and(|previews|
        unsafe {layer_host::ffi::copy_bytes(&previews.atlas.image.bytes,offset,output,length)})
}

/// # Safety
/// Retire the handle after its worker calls finish.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn capy_filter_previews_free(previews: u64) {
    handles::free::<CapyFilterPreviews>(previews);
}
