mod enclose_fill_checks {
    use super::*;
    use layer_core::{Selection, SelectionMode, SelectionPixels, authored::SceneScope};
    use layer_render::{RegionRequest, RegionResult};
    use std::sync::Arc;

    fn contact(s: &mut UiSession<Recorder>, phase: PenPhase, point: [f32; 2]) {
        pen_at(s, 1, phase, point);
        assert!(s.state.host_error.is_none(), "{:?}", s.state.host_error);
    }

    fn enclose(s: &mut UiSession<Recorder>) {
        for (phase, point) in [
            (PenPhase::Down, [20., 30.]),
            (PenPhase::Move, [180., 30.]),
            (PenPhase::Move, [180., 190.]),
            (PenPhase::Up, [20., 190.]),
        ] {
            contact(s, phase, point);
        }
    }

    fn reply(s: &mut UiSession<Recorder>, request: &RegionRequest) -> Arc<SelectionPixels> {
        let pixels = Arc::new(SelectionPixels::bytes([8, 1], [0, 0, 8, 1], vec![0xffffffff; 2]).unwrap());
        s.renderer_mut().region_reply = Some(RegionResult { colour_on_solid: false, gaps: None, underfill: None,
            request_id: request.request_id, pixels: pixels.clone(),
            placement: layer_core::Affine::IDENTITY, tonal_sample: None,
        });
        pixels
    }

    fn fill_edges(operation:&layer_core::RasterOperationKind)->&layer_core::FillEdges {
        let layer_core::RasterOperationKind::Fill {edges:Some(edges),..}=operation else {panic!("Fill edge metadata")};edges
    }

    #[test]
    fn fill_press_classifies_reference_stacking_and_visible_group_leaves() {
        for platform in Platform::ALL {
            let mut s=session(platform);let below=s.engine.document().working.occurrence.unwrap();
            layer(&mut s,LayerAction::New {group:false,clipped:false});let leaf=s.engine.document().working.occurrence.unwrap();
            layer(&mut s,LayerAction::New {group:true,clipped:false});let group=s.engine.document().working.occurrence.unwrap();
            layer(&mut s,LayerAction::Drop {id:occurrence_token(leaf),target:occurrence_token(group),fraction:0.5,surface:LayerDropSurface::Row});
            layer(&mut s,LayerAction::New {group:false,clipped:false});let target=s.engine.document().working.occurrence.unwrap();
            layer(&mut s,LayerAction::Drop {id:occurrence_token(target),target:occurrence_token(group),fraction:0.,surface:LayerDropSurface::Row});
            layer(&mut s,LayerAction::New {group:false,clipped:false});let hidden=s.engine.document().working.occurrence.unwrap();
            layer(&mut s,LayerAction::Drop {id:occurrence_token(hidden),target:occurrence_token(group),fraction:0.5,surface:LayerDropSurface::Row});
            layer(&mut s,LayerAction::Visibility {id:occurrence_token(hidden),value:false});
            layer(&mut s,LayerAction::Select {id:occurrence_token(target),mask:false});
            layer(&mut s,LayerAction::New {group:false,clipped:false});let above=s.engine.document().working.occurrence.unwrap();
            s.set_references([below,group,above].into()).unwrap();
            s.dispatch(UiAction::SetLayerOpacity {id:Some(occurrence_token(leaf)),opacity:0.5}).unwrap();
            let scene=s.engine.document().scene();assert_eq!(scene.parent(leaf),Some(group));assert_eq!(scene.parent(hidden),Some(group));
            let position=|id|scene.order().iter().position(|layer|*layer==id).unwrap();
            assert!(position(above)<position(target)&&position(target)<position(leaf)&&position(leaf)<position(below));
            for (working,source,expected,hide_group) in [
                (target,CommandId::SelectionReference,vec![(above,false),(leaf,true),(below,true)],false),
                (below,CommandId::SelectionReference,vec![(above,false),(leaf,false)],false),
                (target,CommandId::SelectionEditing,vec![],false),
                (target,CommandId::SelectionReference,vec![(above,false),(below,true)],true),
                (target,CommandId::SelectionVisible,vec![(above,false),(leaf,true),(below,true)],false),
            ] {
                layer(&mut s,LayerAction::Visibility {id:occurrence_token(group),value:!hide_group});
                layer(&mut s,LayerAction::Select {id:occurrence_token(working),mask:false});
                for tool in [CommandId::Fill,CommandId::EncloseFill] {
                    invoke(&mut s,tool);invoke(&mut s,source);invoke(&mut s,CommandId::EdgesLineMiddle);
                    if tool==CommandId::Fill {contact(&mut s,PenPhase::Down,[40.,60.]);}else{enclose(&mut s);}
                    s.frame(20_000_000,20_000_000).unwrap();
                    let request=s.renderer_mut().region_requests.last().unwrap().clone();reply(&mut s,&request);s.frame(30_000_000,30_000_000).unwrap();
                    let mode=match source {
                        CommandId::SelectionEditing=>layer_render::RegionColourMode::ColourOnSolid,
                        CommandId::SelectionVisible=>layer_render::RegionColourMode::Target(s.engine.document().drawing_target().unwrap()),
                        _=>layer_render::RegionColourMode::AlphaBorder,
                    };
                    assert_eq!(request.colour_mode,mode);
                    let operation=if tool==CommandId::Fill {s.renderer_mut().fill_preview.as_ref().unwrap().operation.clone()}
                        else{s.engine.document().target_operations(s.engine.document().drawing_target().unwrap()).unwrap().last().unwrap().clone()};
                    let edges=fill_edges(&operation.kind);
                    assert_eq!(edges.position,Point {x:request.position[0] as f32,y:request.position[1] as f32});assert_eq!(edges.tolerance,request.tolerance);
                    let actual=edges.lines.iter().map(|line| {
                        let SceneScope::Members(members)=&line.scope else {panic!("reference leaves")};assert_eq!(members.len(),1);
                        if members[0]==leaf {assert_eq!(line.snapshot.view().occurrence(leaf).unwrap().opacity,if source==CommandId::SelectionReference {1.}else{0.5},"{platform:?} {tool:?} {source:?}");}
                        (members[0],line.fill_above)
                    }).collect::<Vec<_>>();
                    assert_eq!(actual.len(),expected.len(),"{platform:?} {tool:?} {source:?} hidden group {hide_group}: {actual:?}");
                    assert_eq!(actual.into_iter().collect::<std::collections::BTreeSet<_>>(),expected.iter().copied().collect());
                    assert_eq!(s.engine.document().scene().occurrence(leaf).unwrap().opacity,0.5);
                    if tool==CommandId::Fill {contact(&mut s,PenPhase::Cancel,[40.,60.]);s.frame(40_000_000,40_000_000).unwrap();}
                    else{s.frame(40_000_000,40_000_000).unwrap();invoke(&mut s,CommandId::Undo);s.frame(50_000_000,50_000_000).unwrap();}
                }
            }
        }
    }

    #[test]
    fn fill_reveal_waits_for_preparation_and_starts_with_a_fresh_frame() {
        for platform in Platform::ALL {
            let mut s=session(platform);invoke(&mut s,CommandId::Fill);
            contact(&mut s,PenPhase::Down,[40.,60.]);s.frame(20_000_000,20_000_000).unwrap();
            let request=s.renderer_mut().region_requests.last().unwrap().clone();reply(&mut s,&request);
            s.frame(30_000_000,30_000_000).unwrap();
            s.renderer_mut().settling=true;
            s.frame(2_000_000_000,2_000_000_000).unwrap();
            assert_eq!(s.renderer_mut().fill_reveal.unwrap().radius,0.);
            s.frame(2_050_000_000,2_050_000_000).unwrap();assert_eq!(s.renderer_mut().fill_reveal.unwrap().radius,0.);
            s.renderer_mut().settling=false;s.frame(2_100_000_000,2_100_000_000).unwrap();assert_eq!(s.renderer_mut().fill_reveal.unwrap().radius,0.);
            let change=s.frame(2_150_000_000,2_150_000_000).unwrap();
            assert!(s.renderer_mut().fill_reveal.unwrap().radius>0.);assert!(change.needs_present());
            assert!(!s.engine.can_undo());contact(&mut s,PenPhase::Cancel,[40.,60.]);s.frame(2_160_000_000,2_160_000_000).unwrap();
            assert!(s.renderer_mut().fill_reveal.is_none());assert!(!s.engine.can_undo());
        }
    }

    #[test]
    fn fill_press_previews_before_release_and_cancel_preserves_history() {
        for platform in Platform::ALL {
            let mut s=session(platform);invoke(&mut s,CommandId::Fill);
            let before=s.engine.document().artwork.clone();let checkpoint=s.engine.checkpoint();
            contact(&mut s,PenPhase::Down,[40.,60.]);s.frame(1,1).unwrap();
            let request=s.renderer_mut().region_requests.last().unwrap().clone();
            assert!(request.fill.is_some());
            reply(&mut s,&request);s.frame(2,2).unwrap();
            assert!(s.renderer_mut().fill_preview.is_some());
            assert_eq!(s.engine.document().artwork,before);assert_eq!(s.engine.checkpoint(),checkpoint);
            assert!(s.renderer_mut().pending_operations.is_empty());
            contact(&mut s,PenPhase::Cancel,[40.,60.]);s.frame(3,3).unwrap();
            assert!(s.renderer_mut().fill_preview.is_none());
            assert_eq!(s.engine.document().artwork,before);assert_eq!(s.engine.checkpoint(),checkpoint);
            assert!(!s.engine.can_undo());
        }
    }

    #[test]
    fn fill_hold_requests_the_gap_stop_and_cancel_restores_the_option() {
        for platform in Platform::ALL {
            let mut s=session(platform);invoke(&mut s,CommandId::Fill);
            contact(&mut s,PenPhase::Down,[100.,160.]);s.frame(20_000_000,20_000_000).unwrap();
            let request=s.renderer_mut().region_requests.last().unwrap().clone();reply(&mut s,&request);
            s.renderer_mut().region_reply.as_mut().unwrap().gaps=Some(layer_render::RegionGapMasks {next:None,active:2,line_bounded:true,seals:Default::default()});
            s.frame(520_000_000,520_000_000).unwrap();
            let pill=s.state.fill_gesture.as_ref().unwrap().pill.as_ref().unwrap();
            assert!(pill.bounds.y+pill.bounds.height<s.state.fill_gesture.as_ref().unwrap().tip[1]);
            assert_eq!(pill.fill_end,s.state.fill_gesture.as_ref().unwrap().tip[0]);
            let end=Point {x:pill.track[1],y:s.state.fill_gesture.as_ref().unwrap().tip[1]};
            let saved=s.editing_state();let mut movement=event(&s,2,PenPhase::Move,1.);movement.surface_position=end;s.pen(movement);s.frame(530_000_000,530_000_000).unwrap();
            assert_eq!(s.renderer_mut().region_requests.len(),2);assert_eq!(s.region_tools.gap_reach,Some(64));
            assert_eq!(s.renderer_mut().region_requests.last().unwrap().fill.as_ref().unwrap().reach,Some(64));
            let mut same=event(&s,3,PenPhase::Move,1.);same.surface_position=end;s.pen(same);s.frame(535_000_000,535_000_000).unwrap();
            assert_eq!(s.renderer_mut().region_requests.len(),2);
            let latest=s.renderer_mut().region_requests.last().unwrap().clone();reply(&mut s,&latest);
            s.renderer_mut().region_reply.as_mut().unwrap().gaps=Some(layer_render::RegionGapMasks {next:None,active:4,line_bounded:true,seals:Default::default()});
            s.frame(536_000_000,536_000_000).unwrap();assert!(s.state.fill_gesture.as_ref().unwrap().pill.is_some());
            let mut same=event(&s,4,PenPhase::Move,1.);same.surface_position=end;s.pen(same);s.frame(537_000_000,537_000_000).unwrap();
            assert_eq!(s.renderer_mut().region_requests.len(),2);
            assert_eq!(s.editing_state(),saved);
            contact(&mut s,PenPhase::Cancel,[160.,160.]);s.frame(540_000_000,540_000_000).unwrap();
            assert_eq!(s.region_tools.gap_reach,None);assert!(s.region_tools.refinement.close_gaps);assert!(!s.engine.can_undo());
        }
    }

    #[test]
    fn fill_restores_its_gap_stop_without_relabeling_auto_in_other_region_tools() {
        for platform in Platform::ALL {
            let mut s=session(platform);
            let mut editing=s.editing_state();editing.region_gap_reach=Some(64);s.restore_editing(editing).unwrap();
            for command in [CommandId::Fill,CommandId::EncloseFill,CommandId::AutoSelect,CommandId::Fill] {
                invoke(&mut s,command);
                let option=s.state.tool_extra.iter().find(|option|matches!(option,ToolOption::Choice {id:"close-gaps",..})).unwrap();
                let ToolOption::Choice {items,..}=option else{unreachable!()};
                assert_eq!(items[0].label.as_ref(),if command==CommandId::Fill {"64 px"}else{"Auto"});
                assert_eq!(s.editing_state().region_gap_reach,Some(64));
            }
        }
    }

    #[test]
    fn fill_seal_hit_amends_but_a_neighbouring_region_outside_every_hit_radius_fills_normally() {
        for platform in Platform::ALL {for hit in [true,false] {
            let mut s=session(platform);invoke(&mut s,CommandId::Fill);
            contact(&mut s,PenPhase::Down,[40.,60.]);s.frame(20_000_000,20_000_000).unwrap();
            let request=s.renderer_mut().region_requests.last().unwrap().clone();reply(&mut s,&request);
            s.renderer_mut().region_reply.as_mut().unwrap().gaps=Some(layer_render::RegionGapMasks {next:None,active:2,line_bounded:true,
                seals:vec![layer_render::RegionSeal {id:7,start:[100,80],bend:None,end:[104,80],reach:32}].into()});
            s.frame(30_000_000,30_000_000).unwrap();contact(&mut s,PenPhase::Up,[40.,60.]);s.frame(40_000_000,40_000_000).unwrap();s.frame(50_000_000,50_000_000).unwrap();
            let first=s.engine.checkpoint();assert!(s.engine.can_undo());
            let point=if hit {[102.,80.]}else{[150.,90.]};
            contact(&mut s,PenPhase::Down,point);s.frame(60_000_000,60_000_000).unwrap();
            let request=s.renderer_mut().region_requests.last().unwrap().clone();
            assert_eq!(request.fill.as_ref().unwrap().excluded_seals.as_ref(),if hit {&[7][..]}else{&[][..]});
            assert_eq!(request.position,if hit {[40,60]}else{[150,90]});
            reply(&mut s,&request);s.frame(70_000_000,70_000_000).unwrap();contact(&mut s,PenPhase::Up,point);
            s.frame(80_000_000,80_000_000).unwrap();s.frame(90_000_000,90_000_000).unwrap();
            assert_ne!(s.engine.checkpoint(),first);invoke(&mut s,CommandId::Undo);
            assert_eq!(s.engine.can_undo(),!hit);
            if !hit {assert_eq!(s.engine.checkpoint(),first);invoke(&mut s,CommandId::Undo);assert!(!s.engine.can_undo());}
        }}
    }

    #[test]
    fn fill_slides_keep_only_the_newest_request_before_release() {
        for platform in Platform::ALL {for line in [false,true] {
            let mut s=session(platform);invoke(&mut s,CommandId::Fill);
            contact(&mut s,PenPhase::Down,[100.,160.]);s.frame(20_000_000,20_000_000).unwrap();
            let request=s.renderer_mut().region_requests.last().unwrap().clone();reply(&mut s,&request);
            if line {s.renderer_mut().region_reply.as_mut().unwrap().gaps=Some(layer_render::RegionGapMasks {next:None,active:2,line_bounded:true,seals:Default::default()});}
            s.frame(520_000_000,520_000_000).unwrap();
            assert!(s.state.fill_gesture.as_ref().unwrap().pill.is_some());
            let view=s.state.fill_gesture.as_ref().unwrap();let track=view.pill.as_ref().unwrap().track;
            let first=if line {Point{x:track[0]+0.75*(track[1]-track[0]),y:view.tip[1]}}else{on_surface(&s,Point{x:120.,y:160.})};
            let end=if line {Point{x:track[1],y:view.tip[1]}}else{on_surface(&s,Point{x:150.,y:160.})};
            let mut movement=event(&s,2,PenPhase::Move,1.);movement.surface_position=first;s.pen(movement);s.frame(530_000_000,530_000_000).unwrap();
            let older=s.renderer_mut().region_requests.last().unwrap().clone();
            for (sequence,phase) in [(3,PenPhase::Move),(4,PenPhase::Up)] {let mut event=event(&s,sequence,phase,1.);event.surface_position=end;s.pen(event);}
            assert!(!s.engine.can_undo());reply(&mut s,&older);s.frame(540_000_000,540_000_000).unwrap();
            assert!(!s.engine.can_undo());let newest=s.renderer_mut().region_requests.last().unwrap().clone();
            assert_ne!(older.request_id,newest.request_id);
            if line {assert_eq!(older.fill.as_ref().unwrap().reach,Some(48));assert_eq!(newest.fill.as_ref().unwrap().reach,Some(64));}
            else{assert!(newest.tolerance>older.tolerance);}
            reply(&mut s,&newest);s.frame(550_000_000,550_000_000).unwrap();s.frame(560_000_000,560_000_000).unwrap();
            assert!(s.engine.can_undo());assert_eq!(s.region_tools.tolerance,newest.tolerance);
            if line {assert_eq!(s.region_tools.gap_reach,Some(64));}
            let edges=fill_edges(&s.engine.backend().pending_operations.last().unwrap().1.kind);
            assert_eq!(edges.tolerance,newest.tolerance);assert_eq!(edges.position,Point {x:newest.position[0] as f32,y:newest.position[1] as f32});
            invoke(&mut s,CommandId::Undo);assert!(!s.engine.can_undo());
        }}
    }

    #[test]
    fn fill_drag_coalesces_motion_and_waits_for_every_crossed_region_in_one_entry() {
        for platform in Platform::ALL {
            let mut s=session(platform);invoke(&mut s,CommandId::Fill);
            contact(&mut s,PenPhase::Down,[40.,60.]);s.frame(20_000_000,20_000_000).unwrap();
            let first=s.renderer_mut().region_requests.last().unwrap().clone();
            contact(&mut s,PenPhase::Move,[100.,60.]);contact(&mut s,PenPhase::Move,[180.,60.]);contact(&mut s,PenPhase::Up,[180.,60.]);
            reply(&mut s,&first);s.frame(30_000_000,30_000_000).unwrap();s.frame(40_000_000,40_000_000).unwrap();
            let second=s.renderer_mut().region_requests.last().unwrap().clone();
            assert_ne!(first.request_id,second.request_id);assert_eq!(second.fill.as_ref().unwrap().trail.as_ref().unwrap().points.len(),4);
            assert!(!s.engine.can_undo());reply(&mut s,&second);
            s.renderer_mut().region_reply.as_mut().unwrap().gaps=Some(layer_render::RegionGapMasks {next:Some([121,60]),active:0,line_bounded:true,seals:Default::default()});
            s.frame(50_000_000,50_000_000).unwrap();s.frame(60_000_000,60_000_000).unwrap();
            let last=s.renderer_mut().region_requests.last().unwrap().clone();assert_eq!(last.position,[121,60]);assert!(!s.engine.can_undo());
            reply(&mut s,&last);
            s.renderer_mut().region_reply.as_mut().unwrap().gaps=Some(layer_render::RegionGapMasks {next:None,active:0,line_bounded:true,seals:Default::default()});
            s.frame(70_000_000,70_000_000).unwrap();s.frame(80_000_000,80_000_000).unwrap();
            let edges=fill_edges(&s.engine.backend().pending_operations.last().unwrap().1.kind);
            assert_eq!(edges.position,Point {x:121.,y:60.});assert_eq!(edges.tolerance,last.tolerance);
            assert!(s.engine.can_undo());invoke(&mut s,CommandId::Undo);assert!(!s.engine.can_undo());
        }
    }

    #[test]
    fn fill_quick_touch_taps_keep_both_released_contacts_in_order() {
        for (platform,mode) in Platform::ALL.into_iter().flat_map(|platform|FingerDrawing::ALL.map(move |mode|(platform,mode))) {
            let mut s=session(platform);s.state.settings.finger_drawing=mode;invoke(&mut s,CommandId::Fill);
            let pointer=|id,phase,x|UiInput::Pointer {id,kind:PointerKind::Touch,button:PointerButton::Primary,position:[x,240.],time_ns:id*1_000_000,phase};
            for (id,x) in [(41,200.),(42,400.)] {
                for phase in [ContactPhase::Down,ContactPhase::Up] {assert!(!s.input(pointer(id,phase,x)).unwrap().paint);}
            }
            s.frame(50_000_000,50_000_000).unwrap();let first=s.renderer_mut().region_requests.last().unwrap().clone();
            reply(&mut s,&first);
            for now in 51..56 {s.frame(now*1_000_000,now*1_000_000).unwrap();}
            assert_eq!(s.renderer_mut().region_requests.len(),2);let second=s.renderer_mut().region_requests.last().unwrap().clone();
            assert_ne!(first.position,second.position);assert_ne!(first.request_id,second.request_id);
            reply(&mut s,&second);s.frame(60_000_000,60_000_000).unwrap();s.frame(70_000_000,70_000_000).unwrap();
            invoke(&mut s,CommandId::Undo);assert!(s.engine.can_undo());invoke(&mut s,CommandId::Undo);assert!(!s.engine.can_undo());
        }
    }

    #[test]
    fn fill_touch_computes_on_press_and_cancels_on_a_second_finger() {
        for (platform,mode) in Platform::ALL.into_iter().flat_map(|platform|FingerDrawing::ALL.map(move |mode|(platform,mode))) {
            let mut s=session(platform);s.state.settings.finger_drawing=mode;invoke(&mut s,CommandId::Fill);
            let pointer=|id,phase|UiInput::Pointer {id,kind:PointerKind::Touch,button:PointerButton::Primary,position:[200.,240.],time_ns:10_000_000,phase};
            assert!(!s.input(pointer(41,ContactPhase::Down)).unwrap().paint);s.frame(20_000_000,20_000_000).unwrap();
            let request=s.renderer_mut().region_requests.last().unwrap().clone();reply(&mut s,&request);s.frame(30_000_000,30_000_000).unwrap();
            assert!(s.renderer_mut().fill_preview.is_none());assert!(!s.engine.can_undo());
            s.input(pointer(42,ContactPhase::Down)).unwrap();s.input(pointer(41,ContactPhase::Up)).unwrap();s.input(pointer(42,ContactPhase::Up)).unwrap();
            s.frame(40_000_000,40_000_000).unwrap();assert!(s.renderer_mut().fill_preview.is_none());assert!(!s.engine.can_undo());
            s.input(pointer(43,ContactPhase::Down)).unwrap();s.frame(50_000_000,50_000_000).unwrap();
            let request=s.renderer_mut().region_requests.last().unwrap().clone();reply(&mut s,&request);s.frame(60_000_000,60_000_000).unwrap();
            let mut moved_up=pointer(43,ContactPhase::Up);if let UiInput::Pointer {position,..}=&mut moved_up {*position=[400.,240.];}
            s.input(moved_up).unwrap();s.frame(70_000_000,70_000_000).unwrap();assert!(!s.engine.can_undo());
            s.input(pointer(44,ContactPhase::Down)).unwrap();s.frame(80_000_000,80_000_000).unwrap();
            let request=s.renderer_mut().region_requests.last().unwrap().clone();reply(&mut s,&request);s.frame(90_000_000,90_000_000).unwrap();
            s.input(pointer(44,ContactPhase::Up)).unwrap();s.frame(100_000_000,100_000_000).unwrap();assert!(s.engine.can_undo());
        }
    }

    #[test]
    fn fill_release_commits_before_the_next_tool_or_setting_change() {
        for platform in Platform::ALL {for tool in [true,false] {
            let mut s=session(platform);invoke(&mut s,CommandId::Fill);
            contact(&mut s,PenPhase::Down,[40.,60.]);s.frame(20_000_000,20_000_000).unwrap();
            let request=s.renderer_mut().region_requests.last().unwrap().clone();contact(&mut s,PenPhase::Up,[40.,60.]);
            if tool {invoke(&mut s,CommandId::Brush);}else{s.dispatch(UiAction::SetToolSetting {id:"tolerance".into(),value:0.2}).unwrap();}
            assert!(matches!(s.layer_interaction.tool,LayerCanvasTool::Region {fill:true,..}));assert_eq!(s.region_tools.tolerance,0.1);
            reply(&mut s,&request);
            for now in 30..35 {s.frame(now*1_000_000,now*1_000_000).unwrap();}
            if tool {assert_eq!(s.layer_interaction.tool,LayerCanvasTool::Paint);}else{assert_eq!(s.region_tools.tolerance,0.2);}
            assert!(s.engine.can_undo());
            invoke(&mut s,CommandId::Undo);assert!(!s.engine.can_undo());
        }}
    }

    #[test]
    fn fill_release_keeps_its_press_request_and_commits_one_entry_when_ready() {
        for platform in Platform::ALL {
            let mut s=session(platform);invoke(&mut s,CommandId::Fill);
            contact(&mut s,PenPhase::Down,[40.,60.]);s.frame(1,1).unwrap();
            let request=s.renderer_mut().region_requests.last().unwrap().clone();
            contact(&mut s,PenPhase::Up,[40.,60.]);s.frame(2,2).unwrap();
            assert_eq!(s.renderer_mut().region_requests.len(),1);assert!(!s.engine.can_undo());
            reply(&mut s,&request);s.frame(3,3).unwrap();s.frame(4,4).unwrap();
            assert_eq!(s.renderer_mut().region_requests.len(),1);assert!(s.engine.can_undo());
            invoke(&mut s,CommandId::Undo);assert!(!s.engine.can_undo());
        }
    }

    #[test]
    fn fill_siblings_stay_flat_in_toolbar_header_drawers_and_restored_state() {
        for platform in Platform::ALL {
            for control in [ToolbarControl::Command { command: CommandId::Fill },
                ToolbarControl::ToolSlot { slot: ToolSlotId::PhotoFill },
                ToolbarControl::ToolSlot { slot: ToolSlotId::Fill }] {
                let (mut s, panel, ids) = group_fixture(platform, &[control]);
                let tile = DrawerAnchor::Tile { panel, tile: ids[0] };
                let header = add_group_header(&mut s, control);
                let commands = &[CommandId::Fill, CommandId::LassoFill, CommandId::EncloseFill, CommandId::PaintUnfilled][..];
                let check = |s: &UiSession<Recorder>, view: &ToolSetView, anchor: DrawerAnchor| {
                    let siblings: Vec<_> = view.groups.iter().chain(&view.subtools).filter_map(|item|
                        if let UiAction::ChooseToolVariant { anchor: origin, variant: ToolVariant::Command { command } } = item.action {
                            assert_eq!(origin, anchor);
                            Some((command, item.selected))
                        } else { None }).collect();
                    assert_eq!(siblings.iter().map(|(command, _)| *command).collect::<Vec<_>>(), commands);
                    assert_eq!(siblings.iter().filter(|(_, selected)| *selected).count(), 1);
                    assert!(siblings.iter().all(|(command, selected)| *selected == s.command(*command).selected));
                };
                for anchor in [tile, header] {
                    for &command in commands {
                        let variant = ToolVariant::Command { command };
                        s.dispatch(slot_choice(&s, anchor, variant)).unwrap();
                        check(&s, &s.state().tool_set, anchor);
                        activate_slot(&mut s, anchor);
                        let drawer = s.state().customization.drawer.as_ref().unwrap();
                        assert_eq!(drawer.anchor, anchor);
                        check(&s, drawer.tool_set.as_ref().unwrap(), anchor);
                        if matches!(command,CommandId::EncloseFill|CommandId::PaintUnfilled) {
                            for source in [CommandId::SelectionVisible, CommandId::SelectionEditing, CommandId::SelectionReference] {
                                let options = s.state().tool_options();
                                let ToolOption::Choice { items, .. } = options.iter().find(|option|
                                    matches!(option, ToolOption::Choice { id: "selection-source", .. })).unwrap() else { unreachable!() };
                                assert_eq!(items.len(), 3);
                                let action = items.iter().find(|item| item.action == UiAction::Invoke { command: source }).unwrap().action.clone();
                                s.dispatch(UiAction::ToolbarEdit { context: s.state().toolbar_context(), action: Box::new(action) }).unwrap();
                                assert!(s.command(source).selected);
                                check(&s, s.state().customization.drawer.as_ref().unwrap().tool_set.as_ref().unwrap(), anchor);
                            }
                        }
                        assert_eq!(selected_slot_variant(&s, tile), variant);
                        assert_eq!(selected_slot_variant(&s, header), variant);
                        activate_slot(&mut s, anchor);
                        assert!(s.state().customization.drawer.is_none());
                    }
                }
                invoke(&mut s, CommandId::Hand);
                let mut restored = session(platform);
                restored.restore_editing(s.editing_state()).unwrap();
                restored.adopt_workspace(PreparedWorkspace::new(s.capture_workspace().unwrap()).unwrap()).unwrap();
                let enclose = ToolVariant::Command { command: CommandId::PaintUnfilled };
                assert_eq!(selected_slot_variant(&restored, tile), enclose);
                assert_eq!(selected_slot_variant(&restored, header), enclose);
                activate_slot(&mut restored, tile);
                assert!(restored.command(CommandId::PaintUnfilled).selected);
                assert!(restored.command(CommandId::SelectionReference).selected);
            }
        }
    }

    #[test]
    fn reference_documents_prepare_fill_on_open_without_choosing_the_tool() {
        for platform in Platform::ALL {
            let mut s=session(platform);
            assert!(s.renderer_mut().region_preparation.is_none());
            let reference=s.engine.document().working.occurrence.unwrap();
            s.set_references([reference].into()).unwrap();
            let document=s.engine.document().clone();
            let mut opened=UiSession::new(Recorder::default(),document,[1000,1000],platform).unwrap();
            assert_eq!(opened.layer_interaction.tool,LayerCanvasTool::Paint);
            let preparation=opened.renderer_mut().region_preparation.clone().unwrap();
            assert_eq!(preparation.reference_count,Some(1));
            assert!(preparation.refinement.close_gaps);
            assert_eq!(preparation.refinement.edges,layer_render::RegionEdges::LineMiddle);
            assert!(!preparation.enclosed);
            contact(&mut opened,PenPhase::Down,[40.,60.]);
            assert!(!opened.renderer_mut().shader_idle.0);
            assert!(opened.renderer_mut().region_preparation.is_some());
            contact(&mut opened,PenPhase::Up,[60.,60.]);
            opened.frame(1,1).unwrap();
        }
    }

    #[test]
    fn fill_preparation_follows_window_use_and_keeps_each_drawings_options() {
        for platform in Platform::ALL {
            for command in [CommandId::Fill,CommandId::EncloseFill] {
                let mut s=session(platform);
                invoke(&mut s,command);
                invoke(&mut s,CommandId::SelectionEditing);
                invoke(&mut s,CommandId::CloseGapsOff);
                invoke(&mut s,CommandId::EdgesGrowShrink);
                s.dispatch(UiAction::SetToolSetting {id:"edge_offset".into(),value:3.}).unwrap();
                invoke(&mut s,CommandId::Hand);
                let editing=s.editing_state();
                s.restore_editing(editing).unwrap();
                let preparation=s.renderer_mut().region_preparation.clone().unwrap();
                assert_eq!(preparation.enclosed,command==CommandId::EncloseFill);
                assert!(!preparation.refinement.close_gaps);
                assert_eq!(preparation.refinement.edges,layer_render::RegionEdges::GrowShrink(3));
                assert!(matches!(preparation.source,layer_render::RegionSource::Scene {scope:SceneScope::Raw(_),..}));
                let mut next=session(platform);
                assert!(next.renderer_mut().region_preparation.is_none());
                next.inherit_window_state(&s).unwrap();
                let preparation=next.renderer_mut().region_preparation.clone().unwrap();
                assert_eq!(preparation.enclosed,command==CommandId::EncloseFill);
                assert!(preparation.refinement.close_gaps);
                assert_eq!(preparation.refinement.edges,layer_render::RegionEdges::LineMiddle);
                assert_eq!(preparation.reference_count,None);
                s.inherit_window_state(&next).unwrap();
                let preparation=s.renderer_mut().region_preparation.as_ref().unwrap();
                assert_eq!(preparation.refinement.edges,layer_render::RegionEdges::GrowShrink(3));
                assert!(matches!(preparation.source,layer_render::RegionSource::Scene {scope:SceneScope::Raw(_),..}));
            }
        }
    }

    #[test]
    fn region_tools_default_to_available_references_and_keep_source_choices_separate() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            invoke(&mut s, CommandId::EncloseFill);
            assert!(s.command(CommandId::SelectionVisible).selected);
            assert!(ToolSlotId::Fill.variants().iter().any(|variant| variant.command() == CommandId::EncloseFill));
            assert!(ToolSlotId::PhotoFill.variants().iter().any(|variant| variant.command() == CommandId::EncloseFill));
            let checkpoint = s.engine.checkpoint();
            invoke(&mut s, CommandId::SelectionReference);
            enclose(&mut s);
            s.frame(1, 1).unwrap();
            assert!(s.renderer_mut().region_requests.is_empty());
            assert_eq!(notice_text(&s), Some(s.localization().text(MessageId::RESOURCES_REFERENCE_TOOL_MARK_FIRST).as_ref()));
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert!(!s.engine.can_undo());
            invoke(&mut s, CommandId::SelectionEditing);
            invoke(&mut s, CommandId::Fill);
            assert!(s.command(CommandId::SelectionVisible).selected);
            invoke(&mut s, CommandId::SelectionReference);
            invoke(&mut s, CommandId::EncloseFill);
            assert!(s.command(CommandId::SelectionEditing).selected);
            invoke(&mut s, CommandId::Fill);
            assert!(s.command(CommandId::SelectionReference).selected);
        }
    }

    #[test]
    fn enclose_fill_and_bucket_share_controls_and_source_commands_on_every_platform() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            let reference = s.engine.document().working.occurrence.unwrap();
            layer(&mut s, LayerAction::New { group: false, clipped: false });
            s.set_references([reference].into()).unwrap();
            for command in [CommandId::EncloseFill, CommandId::Fill] {
                invoke(&mut s, command);
                assert!(s.command(command).selected);
                invoke(&mut s, CommandId::EdgesLineMiddle);
                assert_eq!(s.state.tool_settings.iter().map(|setting| setting.id).collect::<Vec<_>>(),
                    ["grain_depth", "tolerance", "opacity"]);
                invoke(&mut s, CommandId::CloseGapsOff);
                invoke(&mut s, CommandId::EdgesGrowShrink);
                for (id, value) in [("tolerance", 0.25), ("edge_offset", -2.)] {
                    s.dispatch(UiAction::SetToolSetting { id: id.into(), value }).unwrap();
                }
                for (source_command, scope) in [
                    (CommandId::SelectionVisible, SceneScope::All),
                    (CommandId::SelectionEditing, SceneScope::Raw(s.engine.document().active_target().unwrap())),
                    (CommandId::SelectionReference, SceneScope::Members(vec![reference].into())),
                ] {
                    assert!(s.command(source_command).enabled);
                    invoke(&mut s, source_command);
                    assert!(s.command(command).selected);
                    assert!(s.command(source_command).selected);
                    assert_eq!(s.state.tool_actions.iter().any(|action| action.command == CommandId::StopAtColors), command==CommandId::Fill && source_command == CommandId::SelectionReference);
                    assert_eq!(s.command(CommandId::StopAtColors).enabled, command==CommandId::Fill && source_command == CommandId::SelectionReference);
                    if command == CommandId::EncloseFill { enclose(&mut s); }
                    else {
                        contact(&mut s, PenPhase::Down, [40., 60.]);
                        contact(&mut s, PenPhase::Up, [40., 60.]);
                    }
                    s.frame(1, 1).unwrap();
                    let request = s.renderer_mut().region_requests.last().unwrap().clone();
                    assert_eq!(request.enclosure.is_some(), command == CommandId::EncloseFill);
                    assert!(request.contiguous);
                    assert_eq!(request.tolerance, 0.25);
                    assert!(!request.refinement.close_gaps);
                    assert_eq!(request.refinement.edges, layer_render::RegionEdges::GrowShrink(-2));
                    assert!(request.refinement.smooth_edges);
                    assert_eq!(request.source.sources().len(), if command==CommandId::EncloseFill || source_command == CommandId::SelectionReference { 2 } else { 1 });
                    assert_eq!(request.fill_areas,(command==CommandId::EncloseFill).then_some(FillAreas::Transparent));
                    let layer_render::RegionSource::Scene { scope: actual, .. } = &request.source.sources()[0] else { panic!("scene source"); };
                    assert_eq!(actual, &scope);
                    assert!(s.cancel_layer_gesture().unwrap());
                    reply(&mut s, &request);
                    s.frame(2, 2).unwrap();
                }
            }
            invoke(&mut s, CommandId::StopAtColors);
            contact(&mut s, PenPhase::Down, [40., 60.]);
            contact(&mut s, PenPhase::Up, [40., 60.]);
            s.frame(3, 3).unwrap();
            assert_eq!(s.renderer_mut().region_requests.last().unwrap().source.sources().len(), 1);
            assert!(s.cancel_layer_gesture().unwrap());
            invoke(&mut s, CommandId::EncloseFill);
            assert!(s.command(CommandId::SelectionReference).selected);
        }
    }

    #[test]
    fn enclose_fill_keeps_camera_geometry_and_intersects_selection_after_detection() {
        let mut s = session(Platform::Gtk);
        select(&mut s, rectangle([80., 50., 120., 100.]));
        let previous = s.engine.document().working.selection.clone();
        invoke(&mut s, CommandId::EncloseFill);
        invoke(&mut s, CommandId::SelectionEditing);
        s.state.camera.zoom = 2.;
        s.state.camera.rotation = 0.4;
        s.sync_camera();
        contact(&mut s, PenPhase::Down, [20., 30.]);
        contact(&mut s, PenPhase::Move, [180., 30.]);
        contact(&mut s, PenPhase::Move, [180., 190.]);
        s.frame(2, 2).unwrap();
        assert!(s.renderer_mut().region_requests.is_empty());
        let mut overlay = Vec::new();
        s.append_layer_overlay(&mut overlay);
        assert!(overlay.len() >= 2);
        assert_eq!(s.engine.document().working.selection, previous);
        contact(&mut s, PenPhase::Up, [20., 190.]);
        s.frame(3, 3).unwrap();
        let request = s.renderer_mut().region_requests.last().unwrap().clone();
        let enclosure = request.enclosure.unwrap();
        assert_eq!(enclosure.contours().len(), 1);
        for (actual, expected) in enclosure.contours()[0].iter().zip([[20., 30.], [180., 30.], [180., 190.], [20., 190.]]) {
            assert!((actual.x - expected[0]).abs() < 0.001 && (actual.y - expected[1]).abs() < 0.001);
        }
        assert!(request.limit.is_none());
        let refinement = request.selection.unwrap();
        assert_eq!(refinement.mode, SelectionMode::Intersect);
        assert_eq!(refinement.previous.as_deref(), previous.as_ref());
        assert_eq!(s.engine.document().working.selection, previous);
    }

    #[test]
    fn enclose_fill_result_uses_atomic_paint_history_and_captured_opacity() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            let target = s.engine.document().active_target().unwrap();
            let original = s.engine.document().target_raster(target).unwrap().identity();
            invoke(&mut s, CommandId::EncloseFill);
            invoke(&mut s, CommandId::SelectionEditing);
            s.dispatch(UiAction::SetToolSetting { id: "opacity".into(), value: 0.25 }).unwrap();
            let checkpoint = s.engine.checkpoint();
            enclose(&mut s);
            assert_eq!(s.engine.checkpoint(), checkpoint);
            assert!(s.engine.document().target_operations(target).unwrap().is_empty());
            s.frame(1, 1).unwrap();
            let request = s.renderer_mut().region_requests.last().unwrap().clone();
            s.dispatch(UiAction::SetToolSetting { id: "opacity".into(), value: 0.75 }).unwrap();
            let pixels = reply(&mut s, &request);
            s.renderer_mut().region_reply.as_mut().unwrap().underfill = Some(pixels.clone());
            s.frame(2, 2).unwrap();
            let operations = s.engine.document().target_operations(target).unwrap();
            assert_eq!(operations.len(), 1);
            assert_eq!(operations[0].coverage.selection, Some(Selection::pixels(pixels)));
            let layer_core::RasterOperationKind::Fill { color, underfill, .. } = &operations[0].kind else { panic!("fill operation"); };
            assert_eq!(color[3], 0.25);
            assert_eq!(*underfill, operations[0].coverage.selection);
            assert!(s.engine.document().working.selection.is_none());
            s.frame(3, 3).unwrap();
            let committed = s.engine.document().target_raster(target).unwrap().identity();
            assert_ne!(original, committed);
            invoke(&mut s, CommandId::Undo);
            s.frame(4, 4).unwrap();
            assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), original);
            assert!(!s.engine.can_undo());
            invoke(&mut s, CommandId::Redo);
            s.frame(5, 5).unwrap();
            assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), committed);
        }
    }

    #[test]
    fn enclose_fill_abandoned_paths_and_stale_results_leave_no_history() {
        for cancellation in 0..6 {
            let mut s = session(Platform::Gtk);
            invoke(&mut s, CommandId::EncloseFill);
            invoke(&mut s, CommandId::SelectionEditing);
            let checkpoint = s.engine.checkpoint();
            let target = s.engine.document().active_target().unwrap();
            if cancellation < 3 {
                contact(&mut s, PenPhase::Down, [20., 30.]);
                contact(&mut s, PenPhase::Move, [180., 30.]);
                match cancellation {
                    0 => contact(&mut s, PenPhase::Cancel, [180., 190.]),
                    1 => { assert!(key(&mut s, "escape", true, false, false).handled); }
                    _ => { s.input(UiInput::Blur).unwrap(); }
                }
                contact(&mut s, PenPhase::Up, [20., 190.]);
            } else {
                enclose(&mut s);
                if cancellation == 3 { invoke(&mut s, CommandId::Brush); }
                else {
                    s.frame(1, 1).unwrap();
                    let request = s.renderer_mut().region_requests.last().unwrap().clone();
                    if cancellation == 4 { invoke(&mut s, CommandId::Brush); }
                    else { s.dispatch(UiAction::SetToolSetting { id: "tolerance".into(), value: 0.4 }).unwrap(); }
                    reply(&mut s, &request);
                }
            }
            s.frame(2, 2).unwrap();
            assert!(s.layer_interaction.path.is_empty());
            assert_eq!(s.engine.checkpoint(), checkpoint, "cancellation {cancellation}");
            assert!(!s.engine.can_undo());
            assert!(s.engine.document().target_operations(target).unwrap().is_empty());
            assert!(!s.region_tools.busy());
            assert!(s.renderer_mut().region_requests.is_empty() || cancellation >= 4);
        }
    }

    #[test]
    fn enclose_fill_new_enclosure_supersedes_pending_detection_without_losing_its_reply() {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::EncloseFill);
        invoke(&mut s, CommandId::SelectionEditing);
        let target = s.engine.document().active_target().unwrap();
        enclose(&mut s);
        s.frame(1, 1).unwrap();
        let first = s.renderer_mut().region_requests.last().unwrap().clone();
        enclose(&mut s);
        s.frame(2, 2).unwrap();
        assert_eq!(s.renderer_mut().region_requests.len(), 1);
        reply(&mut s, &first);
        s.frame(3, 3).unwrap();
        assert!(s.engine.document().target_operations(target).unwrap().is_empty());
        assert!(!s.engine.can_undo());
        assert_eq!(s.renderer_mut().region_requests.len(), 2);
        let second = s.renderer_mut().region_requests.last().unwrap().clone();
        assert_ne!(first.request_id, second.request_id);
        reply(&mut s, &second);
        s.frame(4, 4).unwrap();
        assert_eq!(s.engine.document().target_operations(target).unwrap().len(), 1);
        s.frame(5, 5).unwrap();
        invoke(&mut s, CommandId::Undo);
        s.frame(6, 6).unwrap();
        assert!(!s.engine.can_undo());
    }

    #[test]
    fn enclose_fill_undo_waits_for_completed_detection_and_preserves_redo() {
        let mut s = session(Platform::Gtk);
        invoke(&mut s, CommandId::EncloseFill);
        invoke(&mut s, CommandId::SelectionEditing);
        let target = s.engine.document().active_target().unwrap();
        let original = s.engine.document().target_raster(target).unwrap().identity();
        enclose(&mut s);
        s.frame(1, 1).unwrap();
        let request = s.renderer_mut().region_requests.last().unwrap().clone();
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), original);
        reply(&mut s, &request);
        for tick in 2..=5 { s.frame(tick, tick).unwrap(); }
        assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), original);
        assert!(!s.engine.can_undo());
        assert!(s.engine.can_redo());
        invoke(&mut s, CommandId::Redo);
        s.frame(6, 6).unwrap();
        assert_ne!(s.engine.document().target_raster(target).unwrap().identity(), original);
        invoke(&mut s, CommandId::Undo);
        s.frame(7, 7).unwrap();
        assert_eq!(s.engine.document().target_raster(target).unwrap().identity(), original);
    }

    #[test]
    fn enclose_fill_stationary_contacts_and_empty_results_preserve_redo() {
        let mut s = session(Platform::Gtk);
        layer(&mut s, LayerAction::New { group: false, clipped: false });
        invoke(&mut s, CommandId::Undo);
        invoke(&mut s, CommandId::EncloseFill);
        invoke(&mut s, CommandId::SelectionEditing);
        let checkpoint = s.engine.checkpoint();
        contact(&mut s, PenPhase::Down, [40., 60.]);
        contact(&mut s, PenPhase::Move, [40., 60.]);
        contact(&mut s, PenPhase::Up, [40., 60.]);
        s.frame(1, 1).unwrap();
        assert!(s.renderer_mut().region_requests.is_empty());
        enclose(&mut s);
        s.frame(2, 2).unwrap();
        let request = s.renderer_mut().region_requests.last().unwrap().clone();
        s.renderer_mut().region_reply = Some(RegionResult { colour_on_solid: false, gaps: None, underfill: None,
            request_id: request.request_id,
            pixels: Arc::new(SelectionPixels::bytes([8, 1], [0; 4], vec![0; 2]).unwrap()),
            placement: layer_core::Affine::IDENTITY, tonal_sample: None,
        });
        s.frame(3, 3).unwrap();
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert!(s.engine.can_redo());
        assert!(!s.engine.can_undo());
    }

    #[test]
    fn enclose_fill_leaves_selection_masks_and_refuses_layer_masks() {
        for platform in Platform::ALL {
            for command in [CommandId::QuickMask, CommandId::NewSelectionLayer, CommandId::EditLayerMask] {
                let mut s = session(platform);
                if command == CommandId::EditLayerMask {
                    let id = occurrence_token(s.engine.document().working.occurrence.unwrap());
                    layer(&mut s, LayerAction::AddMask { id, replace: false });
                    invoke(&mut s, CommandId::EditLayerContent);
                }
                invoke(&mut s, command);
                if command == CommandId::EditLayerMask {
                    assert!(!s.command(CommandId::EncloseFill).enabled);
                    assert!(s.dispatch(UiAction::Invoke { command: CommandId::EncloseFill }).is_err());
                } else {
                    assert!(s.command(CommandId::EncloseFill).enabled);
                    invoke(&mut s, CommandId::EncloseFill);
                    assert!(s.selection_masks.target().is_none());
                }
            }
        }
    }
    #[test]
    fn reference_checklist_marks_multiple_layers_and_restores_source_and_stop_choices() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            let paint = s.engine.document().working.occurrence.unwrap();
            layer(&mut s, LayerAction::NewVector);
            let vector = s.engine.document().working.occurrence.unwrap();
            layer(&mut s, LayerAction::New { group: true, clipped: false });
            let group = s.engine.document().working.occurrence.unwrap();
            invoke(&mut s, CommandId::Fill);
            assert!(s.command(CommandId::SelectionVisible).selected);
            assert!(!s.state.tool_actions.iter().any(|action| action.command == CommandId::StopAtColors));
            invoke(&mut s, CommandId::SelectionReference);
            let ToolOption::Checklist { items, .. } = s.state.tool_extra.iter().find(|option| matches!(option, ToolOption::Checklist { .. })).unwrap() else { unreachable!() };
            assert_eq!(items.len(), 3);
            let actions: Vec<_> = items.iter().map(|item| item.action.clone()).collect();
            for id in [paint, vector, group] {
                assert!(actions.contains(&UiAction::Layer { action: LayerAction::Reference { id: occurrence_token(id) } }));
            }
            for action in actions {
                assert!(s.state.toolbar_edit_allowed(&action));
                s.dispatch(UiAction::ToolbarEdit { context: s.state.toolbar_context(), action: Box::new(action) }).unwrap();
            }
            assert_eq!(s.engine.document().scene().references().len(), 3);
            invoke(&mut s, CommandId::Undo);
            assert_eq!(s.engine.document().scene().references().len(), 2);
            invoke(&mut s, CommandId::Redo);
            assert_eq!(s.engine.document().scene().references().len(), 3);
            assert!(s.command(CommandId::StopAtColors).selected);
            invoke(&mut s, CommandId::StopAtColors);
            invoke(&mut s, CommandId::EncloseFill);
            assert!(s.command(CommandId::SelectionReference).selected);
            invoke(&mut s, CommandId::SelectionEditing);
            invoke(&mut s, CommandId::Hand);
            let mut restored = session(platform);
            restored.restore_editing(serde_json::from_value(serde_json::to_value(s.editing_state()).unwrap()).unwrap()).unwrap();
            invoke(&mut restored, CommandId::EncloseFill);
            assert!(restored.command(CommandId::SelectionEditing).selected);
            assert!(!restored.command(CommandId::StopAtColors).selected);
            invoke(&mut restored, CommandId::Fill);
            assert!(restored.command(CommandId::SelectionReference).selected);
        }
    }

    #[test]
    fn fill_texture_controls_share_range_reset_and_saved_strength_on_every_platform() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            for command in [CommandId::Fill, CommandId::LassoFill, CommandId::EncloseFill, CommandId::Gradient] {
                invoke(&mut s, command);
                let field = s.state.tool_settings.iter().find(|field| field.id == "grain_depth").unwrap();
                assert_eq!(field.label_id, MessageId::TOOL_SETTING_GRAIN_DEPTH);
                assert_eq!((field.numeric.min, field.numeric.max, field.value), (0., 2., 1.));
                for value in [0., 1., 2.] {
                    s.dispatch(UiAction::SetToolSetting { id: "grain_depth".into(), value }).unwrap();
                    assert_eq!(s.capture_fill_texture().strength, value);
                }
                for value in [-0.01, 2.01, f32::NAN] {
                    assert!(s.dispatch(UiAction::SetToolSetting { id: "grain_depth".into(), value }).is_err());
                }
                let saved = s.editing_state();
                let mut reopened = session(platform);
                reopened.restore_editing(saved).unwrap();
                assert_eq!(reopened.capture_fill_texture().strength, 2.);
                s.dispatch(UiAction::ResetToolSetting { id: "grain_depth".into() }).unwrap();
                assert_eq!(s.capture_fill_texture().strength, 1.);
            }
            invoke(&mut s, CommandId::Fill);
            s.dispatch(UiAction::SetToolSetting { id: "grain_depth".into(), value: 2. }).unwrap();
            invoke(&mut s, CommandId::Brush);
            s.fill_selection(Selection::polygon(vec![Point { x: 10., y: 10. }, Point { x: 80., y: 10. }, Point { x: 80., y: 80. }]).unwrap()).unwrap();
            let target = s.engine.document().drawing_content().unwrap();
            assert_eq!(s.engine.document().target_operations(target).unwrap().last().unwrap().kind.fill_texture().unwrap().strength, 2.);
            invoke(&mut s, CommandId::AutoSelect);
            assert!(!s.state.tool_settings.iter().any(|field| field.id == "grain_depth"));
        }
    }

    #[test]
    fn fill_contacts_retain_the_paper_resolved_at_pen_down() {
        for command in [CommandId::Fill, CommandId::LassoFill, CommandId::EncloseFill, CommandId::Gradient] {
            let mut s = session(Platform::Gtk);
            invoke(&mut s, command);
            if matches!(command, CommandId::Fill | CommandId::EncloseFill) { invoke(&mut s, CommandId::SelectionEditing); }
            let target = s.engine.document().drawing_content().unwrap();
            let expected = s.engine.document().nearest_paper_binding(Some(target)).unwrap();
            contact(&mut s, PenPhase::Down, [20., 30.]);
            assert_eq!(s.layer_interaction.fill_texture.as_ref().unwrap().paper.as_ref(),Some(&expected));
            let doc = s.engine.document();
            let paper = doc.bottom_paper().unwrap();
            let handle = doc.scene().effect_handle(paper).unwrap();
            let mut effect = doc.artwork.effects.get(handle).unwrap().clone();
            for value in &mut effect.values {
                if let layer_core::EffectValue::Texture(texture) = value {
                    *texture = layer_core::paper::built_in_paper("rough").unwrap().texture.clone();
                }
            }
            let edit = layer_core::Edit::Effect(layer_core::authored::RecordChange::replace(&doc.artwork.effects, handle, Some(effect)).unwrap());
            s.engine.apply_edit(edit).unwrap();
            assert_ne!(s.capture_fill_texture().paper.as_ref(), Some(&expected));
            contact(&mut s, PenPhase::Move, [180., 30.]);
            contact(&mut s, PenPhase::Move, [180., 190.]);
            contact(&mut s, PenPhase::Up, [20., 190.]);
            if matches!(command, CommandId::Fill | CommandId::EncloseFill) {
                s.frame(1, 1).unwrap();
                let request = s.renderer_mut().region_requests.last().unwrap().clone();
                reply(&mut s, &request);
                s.frame(2, 2).unwrap();
            }
            let operations = s.engine.document().target_operations(target).unwrap();
            if command==CommandId::Fill {assert!(operations.is_empty(),"a document change retires the press result");}
            else {assert_eq!(operations.last().unwrap().kind.fill_texture().unwrap().paper.as_ref(), Some(&expected), "{command:?}");}
            assert!(s.layer_interaction.fill_texture.is_none());
            if command == CommandId::Fill {
                contact(&mut s, PenPhase::Down, [20., 30.]);
                s.dispatch(UiAction::SetToolSetting { id: "grain_depth".into(), value: 0. }).unwrap();
                contact(&mut s, PenPhase::Up, [20., 30.]);
                assert!(s.layer_interaction.fill_texture.is_none());
            }

        }
    }

    mod unfilled_area_checks {
        use super::*;
        use layer_core::{FillAreas, Point, SelectionPixels};
        use std::sync::Arc;

        fn footprint(s:&mut UiSession<Recorder>)->Arc<SelectionPixels> {
            let update=s.renderer_mut().selection_updates.last().unwrap().clone();
            assert!(s.renderer_mut().selection_updates.iter().any(|update|!update.dabs.is_empty()));
            let pixels=Arc::new(SelectionPixels::bytes([4,1],[0,0,3,1],vec![0x00ffffff]).unwrap());
            s.renderer_mut().selection_reply=Some(layer_render::SelectionPaintResult {request_id:update.id,pixels:pixels.clone(),changed:true});
            pixels
        }
        fn unfilled_stroke(s:&mut UiSession<Recorder>) {
            for (sequence,phase,position) in [(1,PenPhase::Down,[100.,100.]),(2,PenPhase::Move,[180.,100.]),(3,PenPhase::Up,[220.,120.])] {
                pen_at(s,sequence,phase,position);
            }
            s.frame(10,10).unwrap();
            assert!(s.renderer_mut().selection_updates.last().unwrap().finish);
        }
        fn region_reply(s:&mut UiSession<Recorder>,request:&layer_render::RegionRequest) {
            s.renderer_mut().region_reply=Some(layer_render::RegionResult {request_id:request.request_id,
                pixels:Arc::new(SelectionPixels::bytes([4,1],[0,0,4,1],vec![0xffffffff]).unwrap()),
                colour_on_solid:false,gaps:None,underfill:None,placement:layer_core::Affine::IDENTITY,tonal_sample:None});
        }

        #[test]
        fn fill_areas_options_are_shared_by_enclosure_tools_and_persist() {
            for platform in Platform::ALL {
                let mut s=session(platform);
                assert_eq!(s.editing_state().fill_areas,FillAreas::Transparent);
                for tool in [CommandId::EncloseFill,CommandId::PaintUnfilled] {
                    invoke(&mut s,tool);
                    assert!(!s.state.tool_actions.iter().any(|action|action.command==CommandId::StopAtColors));
                    for (command,areas) in [(CommandId::FillAreasTransparent,FillAreas::Transparent),
                        (CommandId::FillAreasTransparentOrWhite,FillAreas::TransparentOrWhite),
                        (CommandId::FillAreasAnyColour,FillAreas::AnyColour)] {
                        invoke(&mut s,command);
                        assert!(s.command(command).selected);
                        assert_eq!(s.editing_state().fill_areas,areas);
                        assert!(s.state.tool_extra.iter().any(|option|matches!(option,ToolOption::Choice {segmented:true,items,..}
                            if items.iter().any(|item|item.action==(UiAction::Invoke {command})))));
                        let saved=serde_json::to_string(&s.editing_state()).unwrap();
                        let mut restored=session(platform);
                        restored.restore_editing(serde_json::from_str(&saved).unwrap()).unwrap();
                        assert_eq!(restored.editing_state().fill_areas,areas);
                    }
                }
                invoke(&mut s,CommandId::PaintUnfilled);
                assert!(s.state.tool_settings.iter().any(|setting|setting.id=="unfilled_size" && setting.value==32.));
                s.dispatch(UiAction::SetToolSetting {id:"unfilled_size".into(),value:48.}).unwrap();
                invoke(&mut s,CommandId::Brush);invoke(&mut s,CommandId::PaintUnfilled);
                assert!(s.state.tool_settings.iter().any(|setting|setting.id=="unfilled_size" && setting.value==48.));
                let mut reopened=session(platform);reopened.restore_editing(s.editing_state()).unwrap();
                invoke(&mut reopened,CommandId::PaintUnfilled);
                assert!(reopened.state.tool_settings.iter().any(|setting|setting.id=="unfilled_size" && setting.value==48.));
                reopened.dispatch(UiAction::SetBrushSize {value:64.}).unwrap();
                assert!(reopened.state.tool_settings.iter().any(|setting|setting.id=="unfilled_size" && setting.value==64.));
                invoke(&mut s,CommandId::Fill);invoke(&mut s,CommandId::SelectionEditing);
                pen_at(&mut s,1,PenPhase::Down,[80.,80.]);s.frame(1,1).unwrap();
                assert_eq!(s.renderer_mut().region_requests.last().unwrap().fill_areas,None);
                pen_at(&mut s,2,PenPhase::Cancel,[80.,80.]);
            }
        }

        #[test]
        fn released_editing_without_fill_areas_keeps_authored_settings() {
            let released=include_str!("../tests/compat/1.0.15/editing-complete.json");
            assert!(!serde_json::from_str::<serde_json::Value>(released).unwrap().as_object().unwrap().contains_key("fill_areas"));
            let english=Localizer::shared(crate::UiLanguage::English);
            let editing=EditingState::restore(released,&english);
            assert_eq!(editing.fill_areas,FillAreas::Transparent);
            assert_eq!(editing.preset,42);
            let mut with_new=serde_json::to_value(&editing).unwrap();
            with_new["fill_areas"]=serde_json::to_value(FillAreas::AnyColour).unwrap();
            let changed:EditingState=serde_json::from_value(with_new).unwrap();
            let mut original=serde_json::to_value(&editing).unwrap();let mut actual=serde_json::to_value(&changed).unwrap();
            original.as_object_mut().unwrap().remove("fill_areas");actual.as_object_mut().unwrap().remove("fill_areas");
            assert_eq!(actual,original);
            let mut s=session(Platform::Gtk);s.restore_editing(changed).unwrap();
            assert_eq!(s.editing_state().fill_areas,FillAreas::AnyColour);
        }

        #[test]
        fn paint_unfilled_waits_for_gpu_footprint_then_commits_one_fill() {
            for platform in Platform::ALL {
                for (command,areas) in [(CommandId::FillAreasTransparent,FillAreas::Transparent),
                    (CommandId::FillAreasTransparentOrWhite,FillAreas::TransparentOrWhite),(CommandId::FillAreasAnyColour,FillAreas::AnyColour)] {
                    let mut s=session(platform);invoke(&mut s,CommandId::PaintUnfilled);invoke(&mut s,CommandId::SelectionEditing);invoke(&mut s,command);
                    let checkpoint=s.engine.checkpoint();let selected=s.engine.document().working.selection.clone();
                    unfilled_stroke(&mut s);
                    assert!(s.renderer_mut().region_requests.is_empty());assert_eq!(s.engine.checkpoint(),checkpoint);
                    let pixels=footprint(&mut s);s.frame(20,20).unwrap();
                    let request=s.renderer_mut().region_requests.last().unwrap().clone();
                    assert_eq!(request.fill_areas,(areas!=FillAreas::AnyColour).then_some(areas));assert!(request.fill.is_none());
                    let enclosure=request.enclosure.as_ref().unwrap();
                    assert!(matches!(&enclosure.shape,layer_core::SelectionShape::Pixels(actual) if Arc::ptr_eq(actual,&pixels)));
                    assert_eq!(s.engine.document().working.selection,selected);assert!(!s.engine.can_undo());
                    region_reply(&mut s,&request);s.frame(30,30).unwrap();s.frame(40,40).unwrap();
                    assert!(s.engine.can_undo());assert_eq!(s.engine.document().working.selection,selected);
                    invoke(&mut s,CommandId::Undo);s.frame(50,50).unwrap();
                    assert!(!s.engine.can_undo());assert!(s.engine.can_redo());
                }
            }
        }

        #[test]
        fn paint_unfilled_explicit_cancel_discards_pending_footprint_and_region_replies() {
            for after_footprint in [false,true] {
                let mut s=session(Platform::Gtk);invoke(&mut s,CommandId::PaintUnfilled);invoke(&mut s,CommandId::SelectionEditing);
                let checkpoint=s.engine.checkpoint();unfilled_stroke(&mut s);
                footprint(&mut s);
                let pending=if after_footprint {
                    s.frame(20,20).unwrap();Some(s.renderer_mut().region_requests.last().unwrap().clone())
                }else{None};
                invoke(&mut s,CommandId::Brush);
                if let Some(request)=pending {region_reply(&mut s,&request);}
                s.frame(30,30).unwrap();s.frame(40,40).unwrap();
                assert_eq!(s.engine.checkpoint(),checkpoint);assert!(!s.engine.can_undo());
                assert!(s.engine.document().working.selection.is_none());
            }
            let mut s=session(Platform::Gtk);invoke(&mut s,CommandId::PaintUnfilled);invoke(&mut s,CommandId::SelectionEditing);
            pen_at(&mut s,1,PenPhase::Down,[100.,100.]);pen_at(&mut s,2,PenPhase::Move,[180.,100.]);
            s.frame(20_000_000,20_000_000).unwrap();assert!(s.painted_selections.busy());
            assert!(!s.renderer_mut().selection_updates.last().unwrap().finish);
            assert!(key(&mut s,"escape",true,false,false).handled);
            assert!(!s.painted_selections.busy());footprint(&mut s);
            pen_at(&mut s,4,PenPhase::Up,[180.,100.]);s.frame(40_000_000,40_000_000).unwrap();
            assert!(!s.painted_selections.busy());
            assert!(s.renderer_mut().region_requests.is_empty());assert!(!s.engine.can_undo());

        }

        #[test]
        fn paint_unfilled_withdraws_a_finger_stroke_but_rejects_palms_during_a_pen_stroke() {
            let mut s=finger_session();invoke(&mut s,CommandId::PaintUnfilled);invoke(&mut s,CommandId::SelectionEditing);
            let checkpoint=s.engine.checkpoint();
            assert!(finger_sample(&mut s,41,ContactPhase::Down,[100.,100.],10).paint);
            finger_sample(&mut s,41,ContactPhase::Move,[180.,100.],30);finger_frames(&mut s,32);
            assert!(s.painted_selections.has_contact());assert!(!s.renderer_mut().selection_updates.is_empty());
            let withdrawn=finger_sample(&mut s,42,ContactPhase::Down,[220.,220.],50);
            assert!(withdrawn.cancel_paint && !withdrawn.paint);assert!(!s.painted_selections.busy());
            footprint(&mut s);
            finger_sample(&mut s,41,ContactPhase::Up,[180.,100.],60);
            finger_sample(&mut s,42,ContactPhase::Up,[220.,220.],70);finger_frames(&mut s,72);
            assert!(s.renderer_mut().region_requests.is_empty());assert_eq!(s.engine.checkpoint(),checkpoint);
            assert!(!s.engine.can_undo());assert!(s.engine.document().working.selection.is_none());
            for platform in Platform::ALL {
                let mut s=session(platform);invoke(&mut s,CommandId::PaintUnfilled);invoke(&mut s,CommandId::SelectionEditing);
                assert!(s.input(pointer_input(7,ContactPhase::Down,PointerKind::Pen,PointerButton::Primary,[100.,100.],10_000_000)).unwrap().paint);
                pen_at(&mut s,1,PenPhase::Down,[100.,100.]);pen_at(&mut s,2,PenPhase::Move,[180.,100.]);
                s.frame(20_000_000,20_000_000).unwrap();assert!(s.painted_selections.has_contact());
                for id in [41,42] {
                    let palm=s.input(pointer_input(id,ContactPhase::Down,PointerKind::Touch,PointerButton::Primary,[220.,220.],25_000_000)).unwrap();
                    assert!(palm.handled && !palm.paint && !palm.cancel_paint);assert!(s.painted_selections.has_contact());
                }
                s.input(pointer_input(7,ContactPhase::Up,PointerKind::Pen,PointerButton::Primary,[180.,100.],30_000_000)).unwrap();
                pen_at(&mut s,3,PenPhase::Up,[180.,100.]);s.frame(40_000_000,40_000_000).unwrap();
                assert!(s.renderer_mut().selection_updates.last().unwrap().finish);
                footprint(&mut s);s.frame(50_000_000,50_000_000).unwrap();
                let request=s.renderer_mut().region_requests.last().unwrap().clone();
                region_reply(&mut s,&request);s.frame(60_000_000,60_000_000).unwrap();s.frame(70_000_000,70_000_000).unwrap();
                assert!(s.engine.can_undo());invoke(&mut s,CommandId::Undo);s.frame(80_000_000,80_000_000).unwrap();assert!(!s.engine.can_undo());
                for id in [41,42] {s.input(pointer_input(id,ContactPhase::Up,PointerKind::Touch,PointerButton::Primary,[220.,220.],90_000_000)).unwrap();}
            }
        }

        #[test]
        fn paint_unfilled_forced_end_finishes_the_footprint_and_commits_once() {
            let mut s=session(Platform::Gtk);invoke(&mut s,CommandId::PaintUnfilled);invoke(&mut s,CommandId::SelectionEditing);
            pen_at(&mut s,1,PenPhase::Down,[100.,100.]);pen_at(&mut s,2,PenPhase::Move,[180.,100.]);
            s.frame(20_000_000,20_000_000).unwrap();assert!(s.painted_selections.has_contact());
            pen_at(&mut s,3,PenPhase::Cancel,[180.,100.]);s.frame(40_000_000,40_000_000).unwrap();
            assert!(!s.painted_selections.has_contact());assert!(s.painted_selections.busy());
            assert!(s.renderer_mut().selection_updates.last().unwrap().finish);
            footprint(&mut s);s.frame(50_000_000,50_000_000).unwrap();
            let request=s.renderer_mut().region_requests.last().unwrap().clone();region_reply(&mut s,&request);
            s.frame(60_000_000,60_000_000).unwrap();s.frame(70_000_000,70_000_000).unwrap();
            assert!(s.engine.can_undo());invoke(&mut s,CommandId::Undo);s.frame(80_000_000,80_000_000).unwrap();assert!(!s.engine.can_undo());
        }

        #[test]
        fn paint_unfilled_shows_accent_footprint_without_pressure_or_authored_changes() {
            for platform in Platform::ALL {
                let mut s=session(platform);invoke(&mut s,CommandId::PaintUnfilled);invoke(&mut s,CommandId::SelectionEditing);
                s.selection_tools.options.display.overlay=false;
                let checkpoint=s.engine.checkpoint();let selected=s.engine.document().working.selection.clone();
                for (sequence,phase,pressure,position) in [(1,PenPhase::Down,0.15,[100.,100.]),(2,PenPhase::Move,0.9,[180.,100.])] {
                    let mut input=event(&s,sequence,phase,pressure);input.surface_position=Point {x:position[0],y:position[1]};s.pen(input);
                }
                s.frame(20_000_000,20_000_000).unwrap();
                let overlay=s.renderer_mut().overlay.clone().unwrap();assert!(overlay.active);
                let accent=s.state.palette.accent.0.map(|v|f32::from(v)/255.);
                assert_eq!(overlay.color,[accent[0],accent[1],accent[2],0.35]);
                assert!(!s.selection_tools.options.display.overlay);
                let updates=&s.engine.backend().selection_updates;assert!(!updates.is_empty());
                let dabs=updates.iter().flat_map(|update|&update.dabs).collect::<Vec<_>>();assert!(dabs.len()>1);
                for dab in dabs {assert_eq!(dab.radii,[16.,16.]);}
                assert_eq!(s.engine.checkpoint(),checkpoint);assert_eq!(s.engine.document().working.selection,selected);
                pen_at(&mut s,3,PenPhase::Cancel,[180.,100.]);s.frame(30_000_000,30_000_000).unwrap();
                assert!(!s.engine.can_undo());
            }
        }

        fn captured_unfilled_settings(after_request:bool) {
            for platform in Platform::ALL {
                let mut s=session(platform);invoke(&mut s,CommandId::PaintUnfilled);invoke(&mut s,CommandId::SelectionEditing);
                s.dispatch(UiAction::SetColor {rgba:[1.,0.,0.,1.]}).unwrap();
                for (id,value) in [("opacity",0.75),("grain_depth",0.5),("tolerance",0.1)] {
                    s.dispatch(UiAction::SetToolSetting {id:id.into(),value}).unwrap();
                }
                invoke(&mut s,CommandId::EdgesGrowShrink);
                s.dispatch(UiAction::SetToolSetting {id:"edge_offset".into(),value:2.}).unwrap();
                invoke(&mut s,CommandId::FillAreasTransparentOrWhite);
                let target=s.engine.document().active_target().unwrap();
                let layer_core::RasterOperationKind::Fill {color:expected_color,texture:expected_texture,..}=s.fill_operation() else {panic!("fill paint")};
                assert_eq!(expected_texture.strength,0.5);
                let expected_refinement=s.region_tools.refinement;
                unfilled_stroke(&mut s);assert!(s.renderer_mut().region_requests.is_empty());
                let queued=if after_request {
                    footprint(&mut s);s.frame(20,20).unwrap();
                    Some(s.renderer_mut().region_requests.last().unwrap().clone())
                }else{None};
                s.dispatch(UiAction::SetColor {rgba:[0.,0.,1.,1.]}).unwrap();
                for (id,value) in [("opacity",0.25),("grain_depth",1.5),("tolerance",0.35),("edge_offset",-3.)] {
                    s.dispatch(UiAction::SetToolSetting {id:id.into(),value}).unwrap();
                }
                assert_eq!(s.state.brush.opacity,0.25);assert_eq!(s.region_tools.tolerance,0.35);
                assert_eq!(s.region_tools.texture_strength,1.5);
                assert_eq!(s.engine.brush().color_rgba_linear[..3],[0.,0.,1.]);
                assert_eq!(s.region_tools.refinement.edges,layer_core::RegionEdges::GrowShrink(-3));
                let request=queued.unwrap_or_else(|| {
                    footprint(&mut s);s.frame(20,20).unwrap();
                    s.renderer_mut().region_requests.last().unwrap().clone()
                });
                assert_eq!(request.tolerance,0.1);assert_eq!(request.refinement,expected_refinement);
                assert_eq!(request.fill_areas,Some(FillAreas::TransparentOrWhite));
                region_reply(&mut s,&request);s.frame(30,30).unwrap();
                let operations=s.engine.document().target_operations(target).unwrap();assert_eq!(operations.len(),1);
                let layer_core::RasterOperationKind::Fill {color,texture,..}=&operations[0].kind else {panic!("committed fill")};
                assert_eq!(*color,expected_color);assert_eq!(expected_color,[1.,0.,0.,0.75]);
                assert_eq!(*texture,expected_texture);
                assert_eq!(s.engine.brush().color_rgba_linear[..3],[0.,0.,1.]);
                assert_eq!(s.state.brush.opacity,0.25);assert_eq!(s.region_tools.tolerance,0.35);
                assert_eq!(s.region_tools.texture_strength,1.5);
                assert_eq!(s.region_tools.refinement.edges,layer_core::RegionEdges::GrowShrink(-3));
                s.frame(40,40).unwrap();assert!(s.engine.can_undo());
                invoke(&mut s,CommandId::Undo);s.frame(50,50).unwrap();assert!(!s.engine.can_undo());
            }
        }

        #[test]
        fn paint_unfilled_keeps_stroke_start_paint_and_detection_settings() {
            captured_unfilled_settings(false);
        }

        #[test]
        fn paint_unfilled_keeps_queued_request_when_numeric_settings_change() {
            captured_unfilled_settings(true);
        }

        #[test]
        fn paint_unfilled_draws_the_next_footprint_while_the_previous_fill_is_detecting() {
            let mut s=session(Platform::Gtk);invoke(&mut s,CommandId::PaintUnfilled);invoke(&mut s,CommandId::SelectionEditing);
            let target=s.engine.document().active_target().unwrap();let original=s.engine.document().target_raster(target).unwrap().identity();
            s.dispatch(UiAction::SetColor {rgba:[1.,0.,0.,1.]}).unwrap();unfilled_stroke(&mut s);
            let first_id=s.renderer_mut().selection_updates.last().unwrap().id;
            footprint(&mut s);s.frame(20,20).unwrap();let first=s.renderer_mut().region_requests.last().unwrap().clone();
            s.dispatch(UiAction::SetColor {rgba:[0.,0.,1.,1.]}).unwrap();
            pen_at(&mut s,4,PenPhase::Down,[300.,300.]);pen_at(&mut s,5,PenPhase::Move,[400.,300.]);s.frame(30,30).unwrap();
            assert_eq!(s.renderer_mut().region_requests.len(),1);
            let second=s.renderer_mut().selection_updates.last().unwrap().clone();
            assert_ne!(second.id,first_id);assert!(!second.finish && !second.dabs.is_empty());
            assert!(s.renderer_mut().overlay.as_ref().is_some_and(|overlay|overlay.active));
            pen_at(&mut s,6,PenPhase::Up,[420.,320.]);s.frame(40,40).unwrap();
            assert!(s.renderer_mut().selection_updates.last().unwrap().finish);
            footprint(&mut s);s.frame(50,50).unwrap();assert_eq!(s.renderer_mut().region_requests.len(),1);
            region_reply(&mut s,&first);s.frame(60,60).unwrap();
            let operations=s.engine.document().target_operations(target).unwrap();
            assert!(matches!(&operations.last().unwrap().kind,layer_core::RasterOperationKind::Fill {color,..} if *color==[1.,0.,0.,1.]));
            assert!(s.engine.raster_backing_pending());assert_eq!(s.renderer_mut().region_requests.len(),1);
            s.frame(70,70).unwrap();assert!(!s.engine.raster_backing_pending());
            assert_eq!(s.renderer_mut().region_requests.len(),1);
            let first_raster=s.engine.document().target_raster(target).unwrap().identity();assert_ne!(first_raster,original);
            s.frame(80,80).unwrap();assert_eq!(s.renderer_mut().region_requests.len(),2);
            let second=s.renderer_mut().region_requests.last().unwrap().clone();assert_ne!(second.request_id,first.request_id);
            region_reply(&mut s,&second);s.frame(90,90).unwrap();
            let operations=s.engine.document().target_operations(target).unwrap();
            assert!(matches!(&operations.last().unwrap().kind,layer_core::RasterOperationKind::Fill {color,..} if *color==[0.,0.,1.,1.]));
            s.frame(100,100).unwrap();assert!(!s.engine.raster_backing_pending());
            invoke(&mut s,CommandId::Undo);s.frame(110,110).unwrap();
            assert_eq!(s.engine.document().target_raster(target).unwrap().identity(),first_raster);assert!(s.engine.can_undo());
            invoke(&mut s,CommandId::Undo);s.frame(120,120).unwrap();
            assert_eq!(s.engine.document().target_raster(target).unwrap().identity(),original);assert!(!s.engine.can_undo());
        }
    }

}

mod fill_option_checks {
    use super::*;

    #[test]
    fn fill_scope_and_smooth_edges_restore_and_do_not_change_auto_select_or_enclosure_scope() {
        for platform in Platform::ALL {
            let mut s = session(platform);
            invoke(&mut s, CommandId::Fill);
            invoke(&mut s, CommandId::CloseGapsOff);
            invoke(&mut s, CommandId::EdgesGrowShrink);
            s.dispatch(UiAction::SetToolSetting { id: "edge_offset".into(), value: -2. }).unwrap();
            invoke(&mut s, CommandId::FillEverywhere);
            invoke(&mut s, CommandId::SmoothRegionEdges);
            let saved = s.editing_state();
            invoke(&mut s, CommandId::AutoSelect);
            invoke(&mut s, CommandId::CloseGapsAuto);
            invoke(&mut s, CommandId::EdgesLineMiddle);
            s.restore_editing(serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap()).unwrap();
            assert_eq!(s.region_tools.scope, FillScope::Everywhere);
            assert!(!s.region_tools.refinement.smooth_edges);
            assert!(!s.region_tools.refinement.close_gaps);
            assert_eq!(s.region_tools.refinement.edges, layer_render::RegionEdges::GrowShrink(-2));
            for (tool, contiguous, smooth) in [(CommandId::Fill, false, false), (CommandId::AutoSelect, true, true)] {
                invoke(&mut s, tool);
                assert_eq!(s.state.tool_extra.iter().any(|option| matches!(option, ToolOption::Choice { id: "close-gaps", .. })), contiguous);
                assert_eq!(s.command(CommandId::CloseGapsAuto).enabled, contiguous);
                assert!(s.state.tool_extra.iter().any(|option| matches!(option, ToolOption::Choice { id: "region-edges", .. })));
                assert!(!s.state.tool_settings.iter().any(|field| field.id == "smoothing"));
                pen_at(&mut s, 1, PenPhase::Down, [48., 72.]);
                pen_at(&mut s, 2, PenPhase::Up, [48., 72.]);
                s.frame(1, 1).unwrap();
                let request = s.renderer_mut().region_requests.last().unwrap().clone();
                assert_eq!(request.contiguous, contiguous);
                assert_eq!(request.refinement.smooth_edges, smooth);
                s.region_tools.renderer_replaced();
            }
            invoke(&mut s, CommandId::AutoSelect);
            invoke(&mut s, CommandId::SelectionAntialias);
            assert!(!s.command(CommandId::SelectionAntialias).selected);
            assert!(!s.state.tool_actions.iter().any(|action| action.command == CommandId::SmoothRegionEdges));
            invoke(&mut s, CommandId::EncloseFill);
            invoke(&mut s, CommandId::SelectionEditing);
            for (phase, point) in [(PenPhase::Down, [20., 20.]), (PenPhase::Move, [180., 20.]), (PenPhase::Move, [180., 180.]), (PenPhase::Up, [20., 180.])] {
                pen_at(&mut s, 3, phase, point);
            }
            s.frame(2, 2).unwrap();
            let request = s.renderer_mut().region_requests.last().unwrap();
            assert!(request.contiguous && request.enclosure.is_some());
        }
    }

    fn flats_raster(s:&UiSession<Recorder>)->layer_core::raster::RasterRevision {
        use layer_core::raster::{RasterData,RasterPlane,RasterRevision,RasterTile,TileBlob,TileKey};
        let descriptor=RasterPlane::Color.descriptor(s.engine.document().composition().color);
        let tile=RasterTile::backed(TileBlob::encode(descriptor,&[64,128,192,255].repeat(256*256)).unwrap());
        RasterRevision::backed(RasterData {tiles:[(TileKey {plane:RasterPlane::Color,coordinate:[0;2]},tile)].into(),watercolor:None})
    }

    #[test]
    fn flat_all_areas_reads_fill_settings_without_stop_or_current_selection() {
        for source in [RegionSource::Editing,RegionSource::Visible,RegionSource::Reference] {
            for (fixed,close,expected) in [(None,false,0),(None,true,16),(Some(32),true,32),(Some(48),false,48)] {
                let mut s=session(Platform::Gtk);let reference=s.engine.document().working.occurrence.unwrap();
                layer(&mut s,LayerAction::Reference {id:occurrence_token(reference)});
                layer(&mut s,LayerAction::New {group:false,clipped:false});
                let active=s.engine.document().working.occurrence.unwrap();let layers=s.engine.document().scene().children(None).to_vec();
                select(&mut s,rectangle([20.,20.,40.,40.]));
                s.region_tools.source=[RegionSource::Reference,source,RegionSource::Visible];
                s.region_tools.stop_at_colors=true;s.region_tools.tolerance=0.37;s.region_tools.gap_reach=fixed;
                s.region_tools.refinement=layer_render::RegionRefinement {close_gaps:close,edges:layer_render::RegionEdges::GrowShrink(8),smooth_edges:true};
                let tool=s.layer_interaction.tool;
                invoke(&mut s,CommandId::FlatAllAreas);
                assert_eq!(s.engine.document().scene().children(None),layers);
                assert_eq!(s.layer_interaction.tool,tool);
                assert!(!s.command(CommandId::FlatAllAreas).enabled);
                s.region_tools.tolerance=0.9;s.frame(10,10).unwrap();
                assert!(s.renderer_mut().region_requests.is_empty());
                let request=s.renderer_mut().flat_requests.last().unwrap().clone();
                assert_eq!(request.tolerance,0.37);assert_eq!(request.fill.as_ref().unwrap().reach,Some(expected));
                assert!(request.fill.as_ref().unwrap().trail.is_none());
                assert_eq!(request.fill_areas,Some(FillAreas::TransparentOrWhite));
                assert_eq!(request.refinement.edges,layer_render::RegionEdges::LineMiddle);
                assert_eq!(request.refinement.close_gaps,expected>0);assert!(!request.refinement.smooth_edges);
                assert!(!request.contiguous);assert!(request.enclosure.is_none() && request.limit.is_none() && request.selection.is_none());
                assert_eq!(request.source.sources().len(),1);
                let layer_render::RegionSource::Scene {scope,..}=&request.source.sources()[0] else{panic!("scene source");};
                match source {
                    RegionSource::Editing=>assert_eq!(scope,&SceneScope::Raw(s.engine.document().scene().source_target(active).unwrap())),
                    RegionSource::Visible=>assert_eq!(scope,&SceneScope::All),
                    RegionSource::Reference=>assert_eq!(scope,&SceneScope::Members(vec![reference].into())),
                    RegionSource::Auto=>unreachable!(),
                }
            }
        }
    }

    #[test]
    fn flat_all_areas_adds_document_aligned_pixels_below_the_active_layer_in_one_undo() {
        use layer_core::{Edit,authored::RecordChange};
        for (platform,clipped) in Platform::ALL.into_iter().flat_map(|platform|[false,true].map(move |clipped|(platform,clipped))) {
            let mut s=session(platform);
            layer(&mut s,LayerAction::New {group:true,clipped:false});let group=s.engine.document().working.occurrence.unwrap();
            layer(&mut s,LayerAction::New {group:false,clipped:false});
            layer(&mut s,LayerAction::New {group:false,clipped});let active=s.engine.document().working.occurrence.unwrap();
            let mut moved=s.engine.document().scene().occurrence(group).unwrap().clone();moved.offset=[30,-20];
            let edit=Edit::Occurrence(RecordChange::replace(&s.engine.document().artwork.occurrences,group,Some(moved)).unwrap());
            s.engine.apply_edit(edit).unwrap();
            let original=s.engine.document().clone();let before=s.engine.checkpoint();let layers=original.scene().children(Some(group)).to_vec();
            let clipping_base=original.scene().clipping_base(active);assert_eq!(clipping_base.is_some(),clipped);
            let index=layers.iter().position(|id|*id==clipping_base.unwrap_or(active)).unwrap();let raster=flats_raster(&s);
            invoke(&mut s,CommandId::FlatAllAreas);assert_eq!(s.engine.checkpoint(),before);
            s.frame(10,10).unwrap();let request_id=s.renderer_mut().flat_requests.last().unwrap().request_id;
            s.renderer_mut().region_output=Some(layer_render::RegionOutput::FlatAreas {request_id,raster:raster.clone()});
            s.frame(20,20).unwrap();let filled=s.engine.document().clone();let flats=filled.working.occurrence.unwrap();
            let mut expected=layers.clone();expected.insert(index+1,flats);
            assert_eq!(filled.scene().children(Some(group)),expected);
            assert_eq!(filled.scene().clipping_base(active),clipping_base);
            assert_eq!(filled.scene().occurrence(flats).unwrap().name.as_ref(),s.localization().text(MessageId::DOCUMENTS_FLATS_NAME).as_ref());
            assert_eq!(filled.scene().occurrence(flats).unwrap().attachment,layer_core::Attachment::None);
            assert_eq!(filled.scene().occurrence_offset(flats),[0;2]);
            let paint=filled.scene().paint_source(flats).unwrap();
            assert_eq!(paint.raster,raster);assert_eq!(paint.domain,filled.composition().size);
            assert!(paint.operations.is_empty() && paint.base.is_none());
            invoke(&mut s,CommandId::Undo);s.frame(30,30).unwrap();
            assert_eq!(s.engine.checkpoint(),before);assert_eq!(s.engine.document().scene().children(Some(group)),layers);
            assert_eq!(s.engine.document().working.occurrence,Some(active));
            invoke(&mut s,CommandId::Redo);s.frame(40,40).unwrap();
            assert_eq!(s.engine.document().scene().paint_source(flats).unwrap().raster,raster);
            assert_eq!(s.engine.document().scene().children(Some(group)),expected);
            assert_eq!(s.engine.document().scene().clipping_base(active),clipping_base);
        }
    }

    #[test]
    fn flat_all_areas_discards_cancelled_stale_and_failed_results_without_a_layer() {
        for cause in 0..4 {
            let mut s=session(Platform::Gtk);invoke(&mut s,CommandId::FlatAllAreas);s.frame(10,10).unwrap();
            let request_id=s.renderer_mut().flat_requests.last().unwrap().request_id;let raster=flats_raster(&s);
            match cause {
                0=>{key(&mut s,"Escape",true,false,false);},
                1=>{invoke(&mut s,CommandId::Move);},
                2=>{let id=s.engine.document().working.occurrence.unwrap();let mut layer=s.engine.document().scene().occurrence(id).unwrap().clone();layer.opacity=0.5;
                    let edit=layer_core::Edit::Occurrence(layer_core::RecordChange::replace(&s.engine.document().artwork.occurrences,id,Some(layer)).unwrap());s.engine.apply_edit(edit).unwrap();},
                _=>s.renderer_mut().region_fails=true,
            }
            let before=s.engine.document().clone();let checkpoint=s.engine.checkpoint();
            s.renderer_mut().region_output=Some(layer_render::RegionOutput::FlatAreas {request_id,raster});
            let result=s.frame(20,20);if cause==3 {assert!(result.is_err());}else{result.unwrap();}
            assert_eq!(s.engine.document().scene().order(),before.scene().order());
            assert_eq!(s.engine.checkpoint(),checkpoint);assert!(!s.region_tools.publishing_edit());
        }
    }

    #[test]
    fn flat_all_areas_requires_a_readable_layer_and_an_unlocked_destination() {
        let mut s=session(Platform::Gtk);assert!(s.command(CommandId::FlatAllAreas).enabled);
        let paint=s.engine.document().working.occurrence.unwrap();layer(&mut s,LayerAction::Delete {id:occurrence_token(paint)});
        assert!(!s.command(CommandId::FlatAllAreas).enabled);
        layer(&mut s,LayerAction::NewVector);assert!(s.command(CommandId::FlatAllAreas).enabled);
        layer(&mut s,LayerAction::New {group:true,clipped:false});let group=s.engine.document().working.occurrence.unwrap();
        layer(&mut s,LayerAction::New {group:false,clipped:false});layer(&mut s,LayerAction::Lock {id:occurrence_token(group),value:true});
        assert!(!s.command(CommandId::FlatAllAreas).enabled);
        let mut s=session(Platform::Gtk);s.renderer_mut().region_pixels=Some(1);let before=s.engine.checkpoint();
        assert_eq!(s.flat_all_areas().unwrap_err(),s.localization().text(MessageId::DOCUMENTS_TOO_LARGE_MEMORY).as_ref());
        assert_eq!(s.engine.checkpoint(),before);assert!(s.renderer_mut().flat_requests.is_empty());
        let mut document=Document::new(layer_core::PortableId::random(),8193,8193,layer_core::DocumentNames {paint:"Ink".into(),paper:"Paper".into()});
        document.artwork.compositions.get_mut(document.artwork.root).unwrap().color.depth=layer_core::color::SampleDepth::F32;
        let mut s=UiSession::new(Recorder {color:document.composition().color,..Default::default()},document,[1000;2],Platform::Gtk).unwrap();
        let before=s.engine.checkpoint();assert!(s.engine.backend().region_fits([8193;2]));
        assert_eq!(s.flat_all_areas().unwrap_err(),s.localization().text(MessageId::DOCUMENTS_TOO_LARGE_MEMORY).as_ref());
        assert_eq!(s.engine.checkpoint(),before);assert!(s.renderer_mut().flat_requests.is_empty());
    }


}

#[test]
fn legacy_region_values_survive_session_adoption_and_save() {
    let mut s = session(Platform::Gtk);
    let mut saved = s.editing_state();
    saved.region_values.insert("gap_closing".into(),2.);
    saved.region_values.insert("expansion".into(),-2.);
    saved.region_values.insert("smoothing".into(),0.);
    saved.region_edges = layer_core::RegionEdges::GrowShrink(-2);
    saved.region_values.insert("edge_offset".into(),-2.);
    s.restore_editing(saved.clone()).unwrap();
    assert_eq!(s.editing_state(),saved);
}

#[test]
fn raster_line_enclosure_traces_the_vector_target_without_a_paint_layer() {
    for platform in Platform::ALL {
        let (mut s, target) = raster_reference_vector_target(platform);
        let layers = s.engine.document().scene().children(None).to_vec();
        let checkpoint = s.engine.checkpoint();
        invoke(&mut s, CommandId::EncloseFill);
        invoke(&mut s, CommandId::SelectionReference);
        let enclosure = [[0., 0.], [80., 0.], [80., 80.], [0., 80.]].map(|point| surface_at(&s, point));
        press(&mut s, 100, &enclosure);
        assert!(s.renderer_mut().region_requests.last().unwrap().enclosure.is_some());
        assert_eq!(s.engine.document().working.occurrence, Some(target));
        assert_eq!(s.engine.document().scene().children(None), layers);
        vector_trace_reply(&mut s, 32);
        finish_vector_trace(&mut s);
        let object = s.engine.document().scene().vector_layer(target).unwrap();
        assert_eq!(object.path_count(), 1);
        let path = object.paths().next().unwrap().1;
        assert!(path.style.strokes().next().is_none());
        assert!(path.encloses([16., 16.], 0.01));
        assert_eq!(s.engine.document().working.occurrence, Some(target));
        assert_eq!(s.engine.document().scene().children(None), layers);
        invoke(&mut s, CommandId::Undo);
        assert_eq!(s.engine.checkpoint(), checkpoint);
        assert_eq!(s.engine.document().scene().vector_layer(target).unwrap().path_count(), 0);
    }

}
