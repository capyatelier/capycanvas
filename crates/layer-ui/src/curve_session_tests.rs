#[test]
fn curve_key_repeat_commits_one_undo_with_matching_key_release() {
    let mut app=session(Platform::Gtk);
    app.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"curves".into()}}).unwrap();
    let control=app.state.layer_properties.controls.iter().find(|c|matches!(c.value,layer_core::EffectValue::Curve(_))).unwrap();
    let key=control.key.clone();
    let layer=occurrence_token(app.engine.document().working.occurrence.unwrap());
    app.dispatch(UiAction::Effect{action:EffectAction::Set{layer,key:key.clone(),value:layer_core::EffectValue::Curve(vec![[0.,0.],[0.5,0.5],[1.,1.]])}}).unwrap();
    let epoch=app.state.layer_properties.epoch;
    app.dispatch(UiAction::Effect{action:EffectAction::CurveSelectPoint{layer,key:key.clone(),epoch,index:Some(1)}}).unwrap();
    let before=app.engine.document().clone();let checkpoint=app.engine.checkpoint();
    let press=|key_event:&str,pressed,repeat|UiAction::Effect{action:EffectAction::CurveKey{layer,key:key.clone(),epoch,key_event:key_event.into(),pressed,repeat,modifiers:Modifiers::default()}};
    app.dispatch(press("ArrowUp",true,false)).unwrap();
    for _ in 0..5 {app.dispatch(press("ArrowUp",true,true)).unwrap();}
    assert_eq!(app.engine.checkpoint(),checkpoint);
    app.dispatch(press("ArrowLeft",false,false)).unwrap();
    assert_eq!(app.engine.checkpoint(),checkpoint);
    app.dispatch(press("ArrowUp",false,false)).unwrap();
    let after=app.engine.document().clone();assert_ne!(after.artwork,before.artwork);
    app.engine.undo().unwrap();assert_eq!(app.engine.document().artwork,before.artwork);
    app.engine.redo().unwrap();assert_eq!(app.engine.document().artwork,after.artwork);
}
#[test]
fn stale_curve_number_down_cannot_open_a_gesture_or_change_history() {
    let mut app=session(Platform::Gtk);
    app.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"curves".into()}}).unwrap();
    let control=app.state.layer_properties.controls.iter().find(|c|matches!(c.value,layer_core::EffectValue::Curve(_))).unwrap();
    let key=control.key.clone();let epoch=control.curve.as_ref().unwrap().epoch;
    let layer=occurrence_token(app.engine.document().working.occurrence.unwrap());
    app.dispatch(UiAction::Effect{action:EffectAction::SelectPage{layer,page:"red".into()}}).unwrap();
    let before=app.engine.document().clone();let checkpoint=app.engine.checkpoint();
    app.dispatch(UiAction::Effect{action:EffectAction::Gesture{phase:ContactPhase::Down,action:Box::new(EffectAction::CurveNumber{layer,key,epoch,axis:crate::CurveAxis::Output,operation:NumericOperation::Value{value:0.7}})}}).unwrap();
    assert!(app.effect_gesture.is_none());assert_eq!(app.engine.checkpoint(),checkpoint);
    assert_eq!(app.engine.document(),&before);
}

#[test]
fn selecting_a_curves_owner_mask_does_not_publish_owner_pages_or_curve_controls() {
    let mut app = session(Platform::Gtk);
    app.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "curves".into() } }).unwrap();
    let id = occurrence_token(app.engine.document().working.occurrence.unwrap());
    assert_eq!(app.state.layer_properties.pages.len(), 4);
    app.dispatch(UiAction::Layer { action: LayerAction::AddMask { id, replace: false } }).unwrap();
    app.dispatch(UiAction::Layer { action: LayerAction::Select { id, mask: true } }).unwrap();
    let layer_core::SourceTarget::Coverage(mask)=app.engine.document().active_target().unwrap() else {panic!("owner mask must be coverage")};
    let owner=occurrence_handle(id).unwrap();
    assert_eq!(app.engine.document().scene().mask(owner).unwrap().0.source,mask);
    assert_eq!(app.engine.document().working.occurrence,Some(owner));
    assert_eq!(app.state.layer_properties.layer, Some(id));
    assert!(app.state.layer_properties.pages.is_empty());
    assert!(app.state.layer_properties.page.is_none());
    assert!(app.state.layer_properties.controls.iter().all(|control| control.curve.is_none()));
    app.dispatch(UiAction::Layer { action: LayerAction::Select { id, mask: false } }).unwrap();
    assert_eq!(app.state.layer_properties.pages.len(), 4);
}

#[test]
fn quick_mask_properties_replace_owner_pages_and_invalidate_stale_curve_epoch() {
    let mut app = session(Platform::Gtk);
    app.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "curves".into() } }).unwrap();
    let control = app.state.layer_properties.controls.iter().find(|control| control.curve.is_some()).unwrap();
    let key = control.key.clone();
    let epoch = app.state.layer_properties.epoch;
    let layer = occurrence_token(app.engine.document().working.occurrence.unwrap());
    invoke(&mut app, CommandId::QuickMask);
    assert_eq!(app.state.layer_properties.layer, Some(0));
    assert!(app.state.layer_properties.pages.is_empty());
    assert!(app.state.layer_properties.page.is_none());
    assert_ne!(app.state.layer_properties.epoch, epoch);
    let before = app.engine.document().clone();
    let revision=app.state.revision;let checkpoint=app.engine.checkpoint();
    let change=app.dispatch(UiAction::Effect { action: EffectAction::CurveSelectPoint { layer, key, epoch, index: Some(0) } }).unwrap();
    assert_eq!(change.regions,0);assert_eq!(app.state.revision,revision);assert_eq!(app.engine.checkpoint(),checkpoint);
    assert_eq!(app.engine.document(), &before);
    assert!(app.effect_gesture.is_none());
}

#[test]
fn unchanged_close_curve_contacts_and_numbers_preserve_bits_history_and_redo() {
    let mut app = session(Platform::Gtk);
    app.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "curves".into() } }).unwrap();
    let layer = occurrence_token(app.engine.document().working.occurrence.unwrap());
    let key = app.state.layer_properties.controls.iter().find(|control| control.curve.is_some()).unwrap().key.clone();
    let points = vec![[0., 0.], [0.4, 0.1], [0.4f32.next_up(), 0.12345679], [0.9, 0.7], [1., 1.]];
    let set = |points| UiAction::Effect { action: EffectAction::Set { layer, key: key.clone(), value: layer_core::EffectValue::Curve(points) } };
    app.dispatch(set(points.clone())).unwrap();
    let mut edited = points.clone();
    edited[2][1] = 0.3;
    app.dispatch(set(edited)).unwrap();
    invoke(&mut app, CommandId::Undo);
    let before = app.engine.document().clone();
    let checkpoint = app.engine.checkpoint();
    let epoch = app.state.layer_properties.epoch;
    let extent = [255., 255.];
    let point = [points[2][0] * extent[0], (1. - points[2][1]) * extent[1]];
    for offset in [[0., 0.], [3., 2.]] {
        let point = [point[0] + offset[0], point[1] + offset[1]];
        for phase in [ContactPhase::Down, ContactPhase::Up] {
            app.dispatch(UiAction::Effect { action: EffectAction::CurveContact { layer, key: key.clone(), epoch, phase, point, extent } }).unwrap();
            assert!(app.engine.document().artwork == before.artwork, "an unchanged contact preserves the exact curve");
        }
    }
    for axis in [crate::CurveAxis::Input, crate::CurveAxis::Output] {
        let curve = app.state.layer_properties.controls.iter().find(|control| control.key == key).unwrap().curve.as_ref().unwrap();
        let value = match axis { crate::CurveAxis::Input => curve.input.as_ref().unwrap().value, crate::CurveAxis::Output => curve.output.as_ref().unwrap().value };
        for operation in [NumericOperation::Value { value }, NumericOperation::Format] {
            for phase in [ContactPhase::Down, ContactPhase::Up] {
                app.dispatch(UiAction::Effect { action: EffectAction::Gesture { phase, action: Box::new(EffectAction::CurveNumber { layer, key: key.clone(), epoch, axis, operation: operation.clone() }) } }).unwrap();
                assert_eq!(app.engine.document().artwork, before.artwork);
            }
        }
    }
    assert_eq!(app.engine.checkpoint(), checkpoint);
    assert!(app.engine.can_redo());
    invoke(&mut app, CommandId::Redo);
    let effect = app.engine.document().scene().effect(occurrence_handle(layer).unwrap()).unwrap();
    assert_eq!(effect.value(&key), Some(&layer_core::EffectValue::Curve(vec![[0., 0.], [0.4, 0.1], [0.4f32.next_up(), 0.3], [0.9, 0.7], [1., 1.]])));
}

#[test]
fn close_curve_output_edits_and_vertical_keys_preserve_input_bits() {
    let mut app = session(Platform::Gtk);
    app.dispatch(UiAction::Effect { action: EffectAction::Insert { effect: "curves".into() } }).unwrap();
    let layer = occurrence_token(app.engine.document().working.occurrence.unwrap());
    let key = app.state.layer_properties.controls.iter().find(|control| control.curve.is_some()).unwrap().key.clone();
    let points = vec![[0., 0.], [0.4, 0.1], [0.4f32.next_up(), 0.12345679], [0.9, 0.7], [1., 1.]];
    app.dispatch(UiAction::Effect { action: EffectAction::Set { layer, key: key.clone(), value: layer_core::EffectValue::Curve(points.clone()) } }).unwrap();
    for numeric in [true, false] {
        let epoch = app.state.layer_properties.epoch;
        app.dispatch(UiAction::Effect { action: EffectAction::CurveSelectPoint { layer, key: key.clone(), epoch, index: Some(2) } }).unwrap();
        if numeric {
            app.dispatch(UiAction::Effect { action: EffectAction::CurveNumber { layer, key: key.clone(), epoch, axis: crate::CurveAxis::Output, operation: NumericOperation::Value { value: 127.5 } } }).unwrap();
        } else {
            for pressed in [true, false] {
                app.dispatch(UiAction::Effect { action: EffectAction::CurveKey { layer, key: key.clone(), epoch, key_event: "ArrowUp".into(), pressed, repeat: false, modifiers: Modifiers::default() } }).unwrap();
            }
        }
        let layer_value = app.engine.document().scene().effect(occurrence_handle(layer).unwrap()).unwrap();
        let layer_core::EffectValue::Curve(edited) = layer_value.value(&key).unwrap() else { panic!("curve required") };
        assert_eq!(edited[2][0].to_bits(), points[2][0].to_bits());
        assert_ne!(edited[2][1], points[2][1]);
        invoke(&mut app, CommandId::Undo);
        assert_eq!(app.engine.document().scene().effect(occurrence_handle(layer).unwrap()).unwrap().value(&key), Some(&layer_core::EffectValue::Curve(points.clone())));
    }
}

#[test]
fn generic_properties_number_gesture_commits_once_and_cancel_noop_preserve_redo() {
    let mut app=session(Platform::Gtk);
    app.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"brightness_contrast".into()}}).unwrap();
    let control=app.state.layer_properties.controls.iter().find(|c|matches!(c.value,layer_core::EffectValue::Number(_))).unwrap();
    let key=control.key.clone();let layer=occurrence_token(app.engine.document().working.occurrence.unwrap());
    let layer_core::EffectValue::Number(initial)=control.value else{unreachable!()};
    let gesture=|phase,operation|UiAction::Effect{action:EffectAction::Gesture{phase,action:Box::new(EffectAction::Number{layer,key:key.clone(),operation})}};
    let before=app.engine.document().clone();let checkpoint=app.engine.checkpoint();
    app.dispatch(gesture(ContactPhase::Down,NumericOperation::Value{value:f64::from(initial)})).unwrap();
    assert!(app.effect_gesture.is_some());
    for position in [0.6,0.7,0.8] {app.dispatch(gesture(ContactPhase::Move,NumericOperation::Position{position})).unwrap();}
    assert_eq!(app.engine.checkpoint(),checkpoint);
    assert_ne!(app.engine.document().artwork,before.artwork);
    app.dispatch(gesture(ContactPhase::Up,NumericOperation::Position{position:0.8})).unwrap();
    let after=app.engine.document().clone();
    app.engine.undo().unwrap();assert_eq!(app.engine.document().artwork,before.artwork);
    app.engine.redo().unwrap();assert_eq!(app.engine.document().artwork,after.artwork);
    app.engine.undo().unwrap();let checkpoint=app.engine.checkpoint();
    app.dispatch(gesture(ContactPhase::Down,NumericOperation::Value{value:f64::from(initial)})).unwrap();
    app.dispatch(gesture(ContactPhase::Move,NumericOperation::Position{position:0.65})).unwrap();
    app.dispatch(gesture(ContactPhase::Cancel,NumericOperation::Value{value:f64::from(initial)})).unwrap();
    assert!(app.effect_gesture.is_none());assert_eq!(app.engine.checkpoint(),checkpoint);
    assert_eq!(app.engine.document().artwork,before.artwork);
    app.dispatch(gesture(ContactPhase::Down,NumericOperation::Value{value:f64::from(initial)})).unwrap();
    app.dispatch(gesture(ContactPhase::Up,NumericOperation::Value{value:f64::from(initial)})).unwrap();
    assert_eq!(app.engine.checkpoint(),checkpoint);
    app.engine.redo().unwrap();assert_eq!(app.engine.document().artwork,after.artwork);
}

#[test]
fn queued_curve_contact_release_then_key_uses_the_same_published_epoch() {
    let mut app=session(Platform::Android);
    app.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"curves".into()}}).unwrap();
    let layer=occurrence_token(app.engine.document().working.occurrence.unwrap());
    let key=app.state.layer_properties.controls.iter().find(|c|c.curve.is_some()).unwrap().key.clone();
    let original=vec![[0.,0.],[0.5,0.5],[1.,1.]];
    app.dispatch(UiAction::Effect{action:EffectAction::Set{layer,key:key.clone(),value:layer_core::EffectValue::Curve(original.clone())}}).unwrap();
    let epoch=app.state.layer_properties.epoch;
    for phase in [ContactPhase::Down,ContactPhase::Up] {
        app.dispatch(UiAction::Effect{action:EffectAction::CurveContact{layer,key:key.clone(),epoch,phase,point:[127.5,127.5],extent:[255.,255.]}}).unwrap();
    }
    assert_eq!(app.engine.document().scene().effect(occurrence_handle(layer).unwrap()).unwrap().value(&key),Some(&layer_core::EffectValue::Curve(original.clone())));
    for pressed in [true,false] {
        app.dispatch(UiAction::Effect{action:EffectAction::CurveKey{layer,key:key.clone(),epoch,key_event:"ArrowUp".into(),pressed,repeat:false,modifiers:Modifiers::default()}}).unwrap();
    }
    let value=app.engine.document().scene().effect(occurrence_handle(layer).unwrap()).unwrap().value(&key).unwrap();
    let layer_core::EffectValue::Curve(points)=value else{unreachable!()};
    assert!((points[1][1]-(0.5+1./255.)).abs()<f32::EPSILON,"queued key must edit rather than be rejected as stale");
    invoke(&mut app,CommandId::Undo);assert_eq!(app.engine.document().scene().effect(occurrence_handle(layer).unwrap()).unwrap().value(&key),Some(&layer_core::EffectValue::Curve(original.clone())));
    for pressed in [true,false] {app.dispatch(UiAction::Effect{action:EffectAction::CurveKey{layer,key:key.clone(),epoch,key_event:"ArrowUp".into(),pressed,repeat:false,modifiers:Modifiers::default()}}).unwrap();}
    assert_eq!(app.engine.document().scene().effect(occurrence_handle(layer).unwrap()).unwrap().value(&key),Some(&layer_core::EffectValue::Curve(original)));
    assert!(app.engine.can_redo(),"external Undo still invalidates the old published epoch");
}

#[test]
fn queued_curve_numeric_axis_change_keeps_the_published_epoch_for_each_gesture() {
    let mut app=session(Platform::Android);
    app.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"curves".into()}}).unwrap();
    let layer=occurrence_token(app.engine.document().working.occurrence.unwrap());
    let key=app.state.layer_properties.controls.iter().find(|c|c.curve.is_some()).unwrap().key.clone();
    let original=vec![[0.,0.],[0.5,0.5],[1.,1.]];
    app.dispatch(UiAction::Effect{action:EffectAction::Set{layer,key:key.clone(),value:layer_core::EffectValue::Curve(original.clone())}}).unwrap();
    let epoch=app.state.layer_properties.epoch;
    app.dispatch(UiAction::Effect{action:EffectAction::CurveSelectPoint{layer,key:key.clone(),epoch,index:Some(1)}}).unwrap();
    let gesture=|phase,axis,value|UiAction::Effect{action:EffectAction::Gesture{phase,action:Box::new(EffectAction::CurveNumber{layer,key:key.clone(),epoch,axis,operation:NumericOperation::Value{value}})}};
    app.dispatch(gesture(ContactPhase::Down,crate::CurveAxis::Output,0.5)).unwrap();
    app.dispatch(gesture(ContactPhase::Move,crate::CurveAxis::Output,0.6)).unwrap();
    app.dispatch(gesture(ContactPhase::Up,crate::CurveAxis::Output,0.6)).unwrap();
    app.dispatch(gesture(ContactPhase::Down,crate::CurveAxis::Input,0.5)).unwrap();
    assert!(app.effect_gesture.is_some(),"next native key gesture must retain its published control epoch");
    app.dispatch(gesture(ContactPhase::Move,crate::CurveAxis::Input,0.55)).unwrap();
    app.dispatch(gesture(ContactPhase::Up,crate::CurveAxis::Input,0.55)).unwrap();
    let get=|app:&UiSession<Recorder>|app.engine.document().scene().effect(occurrence_handle(layer).unwrap()).unwrap().value(&key).unwrap().clone();
    assert_eq!(get(&app),layer_core::EffectValue::Curve(vec![[0.,0.],[0.55,0.6],[1.,1.]]));
    invoke(&mut app,CommandId::Undo);assert_eq!(get(&app),layer_core::EffectValue::Curve(vec![[0.,0.],[0.5,0.6],[1.,1.]]));
    invoke(&mut app,CommandId::Undo);assert_eq!(get(&app),layer_core::EffectValue::Curve(original));
    app.dispatch(gesture(ContactPhase::Down,crate::CurveAxis::Input,0.5)).unwrap();
    assert!(app.effect_gesture.is_none(),"external Undo must refuse the original native epoch");
    assert!(app.engine.can_redo());
}

#[test]
fn removing_an_interior_curve_point_clears_selection_instead_of_selecting_its_neighbor() {
    for detached in [false,true] {
        let mut app=session(Platform::Android);
        app.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"curves".into()}}).unwrap();
        let layer=occurrence_token(app.engine.document().working.occurrence.unwrap());
        let key=app.state.layer_properties.controls.iter().find(|c|c.curve.is_some()).unwrap().key.clone();
        let original=vec![[0.,0.],[0.25,0.2],[0.75,0.8],[1.,1.]];
        app.dispatch(UiAction::Effect{action:EffectAction::Set{layer,key:key.clone(),value:layer_core::EffectValue::Curve(original.clone())}}).unwrap();
        let epoch=app.state.layer_properties.epoch;
        app.dispatch(UiAction::Effect{action:EffectAction::CurveSelectPoint{layer,key:key.clone(),epoch,index:Some(1)}}).unwrap();
        if detached {
            for (phase,point) in [(ContactPhase::Down,[63.75,204.]),(ContactPhase::Move,[63.75,340.]),(ContactPhase::Up,[63.75,340.])] {
                app.dispatch(UiAction::Effect{action:EffectAction::CurveContact{layer,key:key.clone(),epoch,phase,point,extent:[255.,255.]}}).unwrap();
            }
        } else {
            app.dispatch(UiAction::Effect{action:EffectAction::CurveRemoveAt{layer,key:key.clone(),epoch,point:[63.75,204.],extent:[255.,255.],point_count:None}}).unwrap();
        }
        let control=app.state.layer_properties.controls.iter().find(|c|c.key==key).unwrap();
        assert_eq!(control.value,layer_core::EffectValue::Curve(vec![[0.,0.],[0.75,0.8],[1.,1.]]));
        assert_eq!(control.curve.as_ref().unwrap().selected,None,"removed point must not retarget the surviving neighbor (detached={detached})");
        invoke(&mut app,CommandId::Undo);
        assert_eq!(app.engine.document().scene().effect(occurrence_handle(layer).unwrap()).unwrap().value(&key),Some(&layer_core::EffectValue::Curve(original)));
    }
}

#[test]
fn queued_empty_curve_double_tap_preserves_the_first_insert_and_removes_only_published_points() {
    let mut app=session(Platform::Android);
    app.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"curves".into()}}).unwrap();
    let layer=occurrence_token(app.engine.document().working.occurrence.unwrap());
    let key=app.state.layer_properties.controls.iter().find(|c|c.curve.is_some()).unwrap().key.clone();
    let epoch=app.state.layer_properties.epoch;
    let contact=|phase|UiAction::Effect{action:EffectAction::CurveContact{layer,key:key.clone(),epoch,phase,point:[127.5,127.5],extent:[255.,255.]}};
    app.dispatch(contact(ContactPhase::Down)).unwrap();
    app.dispatch(contact(ContactPhase::Up)).unwrap();
    let inserted=app.engine.document().artwork.clone();
    let checkpoint=app.engine.checkpoint();
    let remove=|point_count|UiAction::Effect{action:EffectAction::CurveRemoveAt{layer,key:key.clone(),epoch,point:[127.5,127.5],extent:[255.,255.],point_count}};
    app.dispatch(remove(Some(2))).unwrap();
    assert_eq!(app.engine.document().artwork,inserted);
    assert_eq!(app.engine.checkpoint(),checkpoint,"stale first-tap count must add no history");
    invoke(&mut app,CommandId::Undo);
    assert_eq!(app.state.layer_properties.controls.iter().find(|c|c.key==key).unwrap().value,layer_core::EffectValue::Curve(vec![[0.,0.],[1.,1.]]));
    invoke(&mut app,CommandId::Redo);
    for point_count in [Some(3),None] {
        let epoch=app.state.layer_properties.epoch;
        app.dispatch(UiAction::Effect{action:EffectAction::CurveRemoveAt{layer,key:key.clone(),epoch,point:[127.5,127.5],extent:[255.,255.],point_count}}).unwrap();
        assert_eq!(app.state.layer_properties.controls.iter().find(|c|c.key==key).unwrap().value,layer_core::EffectValue::Curve(vec![[0.,0.],[1.,1.]]));
        invoke(&mut app,CommandId::Undo);
        assert_eq!(app.engine.document().artwork,inserted);
    }
}

#[test]
fn language_refresh_preserves_curve_page_selected_knot_epoch_and_redo() {
    let mut app=session(Platform::Gtk);
    app.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"curves".into()}}).unwrap();
    let layer=occurrence_token(app.engine.document().working.occurrence.unwrap());
    app.dispatch(UiAction::Effect{action:EffectAction::SelectPage{layer,page:"red".into()}}).unwrap();
    let key=app.state.layer_properties.controls.iter().find(|c|c.curve.is_some()).unwrap().key.clone();
    let points=vec![[0.,0.],[0.4,0.12345679],[1.,1.]];
    app.dispatch(UiAction::Effect{action:EffectAction::Set{layer,key:key.clone(),value:layer_core::EffectValue::Curve(points.clone())}}).unwrap();
    let mut changed=points;changed[1][1]=0.7;
    app.dispatch(UiAction::Effect{action:EffectAction::Set{layer,key:key.clone(),value:layer_core::EffectValue::Curve(changed)}}).unwrap();
    invoke(&mut app,CommandId::Undo);
    let epoch=app.state.layer_properties.epoch;
    app.dispatch(UiAction::Effect{action:EffectAction::CurveSelectPoint{layer,key:key.clone(),epoch,index:Some(1)}}).unwrap();
    let before=app.engine.document().clone();let checkpoint=app.engine.checkpoint();
    let coordinate=app.state.layer_properties.controls.iter().find(|c|c.key==key).unwrap().curve.as_ref().unwrap().output.clone();
    assert!(app.set_localization(Localizer::shared(UiLanguage::Japanese)));
    assert_eq!(app.state.layer_properties.page.as_deref(),Some("red"));
    assert_eq!(app.state.layer_properties.epoch,epoch);
    let curve=app.state.layer_properties.controls.iter().find(|c|c.key==key).unwrap().curve.as_ref().unwrap();
    assert_eq!(curve.selected,Some(1));assert_eq!(curve.output,coordinate);
    assert_eq!(app.engine.document(),&before);assert_eq!(app.engine.checkpoint(),checkpoint);assert!(app.engine.can_redo());
}

#[test]
fn curve_and_numeric_contacts_are_busy_until_their_native_release() {
    let mut app=session(Platform::Gtk);
    app.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"curves".into()}}).unwrap();
    let layer=occurrence_token(app.engine.document().working.occurrence.unwrap());
    let key=app.state.layer_properties.controls.iter().find(|c|c.curve.is_some()).unwrap().key.clone();
    app.dispatch(UiAction::Effect{action:EffectAction::Set{layer,key:key.clone(),value:layer_core::EffectValue::Curve(vec![[0.,0.],[0.5,0.5],[1.,1.]])}}).unwrap();
    let epoch=app.state.layer_properties.epoch;
    app.dispatch(UiAction::Effect{action:EffectAction::CurveSelectPoint{layer,key:key.clone(),epoch,index:Some(1)}}).unwrap();
    for pressed in [true,false] {
        app.dispatch(UiAction::Effect{action:EffectAction::CurveKey{layer,key:key.clone(),epoch,key_event:"ArrowUp".into(),pressed,repeat:false,modifiers:Modifiers::default()}}).unwrap();
        assert_eq!(app.localization_input_busy(),pressed);
    }
    let epoch=app.state.layer_properties.epoch;
    for phase in [ContactPhase::Down,ContactPhase::Up] {
        app.dispatch(UiAction::Effect{action:EffectAction::CurveContact{layer,key:key.clone(),epoch,phase,point:[127.5,127.5],extent:[255.,255.]}}).unwrap();
        assert_eq!(app.localization_input_busy(),phase==ContactPhase::Down);
    }
    app.dispatch(UiAction::Effect{action:EffectAction::Insert{effect:"brightness_contrast".into()}}).unwrap();
    let layer=occurrence_token(app.engine.document().working.occurrence.unwrap());
    let control=app.state.layer_properties.controls.iter().find(|c|matches!(c.value,layer_core::EffectValue::Number(_))).unwrap();
    let key=control.key.clone();let layer_core::EffectValue::Number(value)=control.value else{unreachable!()};
    for phase in [ContactPhase::Down,ContactPhase::Up] {
        app.dispatch(UiAction::Effect{action:EffectAction::Gesture{phase,action:Box::new(EffectAction::Number{layer,key:key.clone(),operation:NumericOperation::Value{value:f64::from(value)}})}}).unwrap();
        assert_eq!(app.localization_input_busy(),phase==ContactPhase::Down);
    }
}
