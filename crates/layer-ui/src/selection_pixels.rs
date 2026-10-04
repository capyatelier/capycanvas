//! Selection coverage applied to authored paint sources and occurrence masks.
use super::*;
use layer_core::{Affine, Edit, CoverageSnapshot, RasterOperation, RasterOperationKind, Point, Selection,
    authored::{CoverageSource, MaskUse, Occurrence, OccurrenceContent, OccurrenceHandle, PaintSource, RecordChange, SceneScope, SourceTarget}};
use std::{collections::BTreeSet,sync::Arc};

const NO_SELECTION: MessageId = MessageId::COMMANDS_MAKE_A_SELECTION_FIRST;

impl<R: CanvasRenderer> UiSession<R> {
    pub(super) fn clear_refusal(&self) -> Option<Arc<str>> {
        let l=self.localization();let doc=self.engine.document();
        if self.selection_masks.quick() {return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_QUICK_MASK_EDITS_THE_SELECTION_LEAVE_IT_TO_CLEAR_ARTWORK));}
        if self.selection_masks.target().is_some() {return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST));}
        if matches!(doc.working.target,Some(SourceTarget::Coverage(_))) {return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_MASKS_AREN_T_CLEARED_THIS_WAY_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST));}
        if doc.working.selection.is_none() {return Some(l.text(MessageId::COMMANDS_MAKE_A_SELECTION_FIRST));}
        match doc.try_drawing_content() {
            Err(reason)=>Some(notices::drawing_refusal_text(reason,l)),
            Ok(target)=>doc.scene().source_owner(target).and_then(|h|doc.scene().occurrence(h)).is_some_and(|o|o.alpha_locked)
                .then_some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_ALPHA_LOCK_KEEPS_TRANSPARENCY_UNLOCK_THE_LAYER_FIRST)),
        }
    }
    pub(super) fn clear_selection(&mut self, outside: bool) -> Result<(),String> {
        refused(self.clear_refusal())?;
        let doc=self.engine.document();let target=doc.drawing_content().ok_or("Select a drawing layer")?;
        let alpha_locked=doc.scene().source_owner(target).and_then(|h|doc.scene().occurrence(h)).is_some_and(|o|o.alpha_locked);
        let mut selection=doc.working.selection.clone().ok_or_else(||self.localization().text(NO_SELECTION).to_string())?;
        selection.inverted^=outside;
        let operation=self.erase_operation(target,&selection,alpha_locked)?;
        self.engine.append_raster_operation(target,operation).map_err(error)?;
        self.layer_interaction.changed=true;Ok(())
    }
    fn erase_operation(&mut self,target:SourceTarget,selection:&Selection,alpha_locked:bool)->Result<RasterOperation,String> {
        let doc=self.engine.document();let domain=doc.target_extent(target);
        let inverse=doc.affine_edit_transform(target).and_then(Affine::inverse).ok_or("Invalid layer placement")?;
        let mut coverage=CoverageSnapshot::reveal_all(self.engine.allocate_coverage_handle(),domain,Point::default());
        coverage.source.default_coverage=f32::from(selection.inverted);
        coverage.source.initial=Some(selection.transformed(inverse).map_err(error)?);
        Ok(RasterOperation {placement:Affine::IDENTITY,coverage,kind:RasterOperationKind::Erase {alpha_locked}})
    }
    pub(super) fn selection_to_layer_refusal(&self,cut:bool)->Option<Arc<str>> {
        let l=self.localization();let doc=self.engine.document();let scene=doc.scene();
        if self.selection_masks.target().is_some() {return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST));}
        if matches!(doc.working.target,Some(SourceTarget::Coverage(_))) {return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST));}
        if doc.working.selection.is_none() {
            if cut {return Some(l.text(MessageId::COMMANDS_MAKE_A_SELECTION_FIRST));}
            let mut selected=self.layer_interaction.selected.iter().filter_map(|h|scene.occurrence(*h));
            return match selected.next() {
                None=>Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_SELECT_LAYERS_FIRST)),
                Some(_)=>None,
            };
        }
        let Some(id)=doc.working.occurrence else {return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_SELECT_A_LAYER_FIRST));};
        let Some(o)=scene.occurrence(id) else {return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_SELECT_A_LAYER_FIRST));};
        match o.kind() {
            LayerKind::Paint=>{},
            LayerKind::Group=>return Some(notices::drawing_refusal_text(layer_core::DrawingRefusal::Group,l)),
            LayerKind::Effect=>return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_AN_EFFECT_LAYER_HAS_NO_PIXELS_OF_ITS_OWN)),
            LayerKind::Selection=>return Some(notices::drawing_refusal_text(layer_core::DrawingRefusal::SelectionLayer,l)),
        }
        if scene.parent(id).is_some_and(|h|doc.is_locked(h)) {return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_THE_LAYER_S_GROUP_IS_LOCKED));}
        if cut && let Err(reason)=doc.validate_content_write(scene.source_target(id).unwrap()) {return Some(notices::drawing_refusal_text(reason,l));}
        (cut && o.alpha_locked).then_some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_ALPHA_LOCK_KEEPS_TRANSPARENCY_UNLOCK_THE_LAYER_FIRST))
    }
    pub(super) fn selection_to_layer(&mut self,cut:bool)->Result<(),String> {
        refused(self.selection_to_layer_refusal(cut))?;
        let Some(selection)=self.engine.document().working.selection.clone() else {return self.layer_action(LayerAction::DuplicateSelected);};
        let doc=self.engine.document();let scene=doc.scene();let source_id=doc.working.occurrence.ok_or("Select a layer first")?;
        let source=scene.occurrence(source_id).ok_or("Select a layer first")?.clone();
        let source_target=scene.source_target(source_id).ok_or("Missing paint source")?;
        let top=doc.clipping_stack_top(source_id).ok_or("Unknown layer")?;
        let stack_handle=scene.stack(top).ok_or("Missing containing stack")?;
        let mut stack=doc.artwork.stacks.get(stack_handle).ok_or("Missing stack")?.clone();
        let index=stack.entries.iter().position(|h|*h==top).ok_or("Unknown layer")?;
        let parent=scene.parent(source_id);let parent_offset=parent.map_or(Point::default(),|h|doc.layer_offset(h));
        let (origin,extent)=doc.bake_extent(&BTreeSet::from([source_id])).map_err(|_|self.localization().text(MessageId::COMMANDS_COPY_PIXELS_TOO_LARGE).to_string())?;
        let mut snapshot=doc.snapshot();let mut members=vec![source_id];let mut ancestor=scene.parent(source_id);
        while let Some(h)=ancestor {members.push(h);ancestor=scene.parent(h);}
        for &h in &members {
            let o=Arc::make_mut(&mut snapshot).artwork.occurrences.get_mut(h).ok_or("Missing capture occurrence")?;
            o.visible=true;o.opacity=1.;o.blend=layer_core::LayerBlend::Normal;o.attachment=layer_core::Attachment::None;
            if h!=source_id {o.mask=None;}
        }
        let paint=RecordChange::insert(&doc.artwork.paint,PaintSource {domain:extent,raster:Default::default(),original:None,operations:Arc::default()});
        let target=SourceTarget::Paint(paint.handle);
        let mut copy=Occurrence::new(OccurrenceContent::Paint(paint.handle),format!("{} copy",source.name));
        copy.opacity=source.opacity;copy.visible=source.visible;copy.blend=source.blend;
        copy.translation=Point {x:origin.x-parent_offset.x,y:origin.y-parent_offset.y};
        let occurrence=RecordChange::insert(&doc.artwork.occurrences,copy);let id=occurrence.handle;
        stack.entries.insert(index,id);
        let mut working=doc.working.clone();working.occurrence=Some(id);working.target=Some(target);working.inspect_mask=None;
        let edits=vec![Edit::Paint(paint),Edit::Occurrence(occurrence),Edit::Stack(RecordChange::replace(&doc.artwork.stacks,stack_handle,Some(stack)).map_err(error)?),Edit::Working(working)];
        let mut coverage=CoverageSnapshot::reveal_all(self.engine.allocate_coverage_handle(),extent,Point::default());
        coverage.source.default_coverage=f32::from(selection.inverted);
        coverage.source.initial=Some(selection.transformed(Affine::translation(Point{x:-origin.x,y:-origin.y})).map_err(error)?);
        let mut operations=vec![(target,RasterOperation {placement:Affine::IDENTITY,coverage,
            kind:RasterOperationKind::Bake {scene:snapshot,scope:SceneScope::Members(members.into()),offset:Point{x:-origin.x,y:-origin.y}}})];
        if cut {operations.push((source_target,self.erase_operation(source_target,&selection,source.alpha_locked)?));}
        self.engine.insert_with_operations(edits,operations,Some(None)).map_err(error)?;
        self.selection_masks.reselect=Some(selection);self.layer_interaction.editing=Some(id);
        self.layer_interaction.selected=BTreeSet::from([id]);self.layer_interaction.changed=true;Ok(())
    }
    pub(super) fn selection_mask(&self,owner:&Occurrence,hide:bool,parent:Option<OccurrenceHandle>,domain:[u32;2])
        ->Result<(RecordChange<CoverageSource>,MaskUse),String> {
        let doc=self.engine.document();let handle=doc.artwork.coverage.next_handle();
        let mut coverage=CoverageSnapshot::reveal_all(handle,domain,owner.mask.as_ref().map_or(owner.translation,|m|m.translation));
        coverage.use_.linked=owner.mask.as_ref().is_none_or(|m|m.linked);
        if let Some(selection)=&doc.working.selection {
            let mut selection=selection.clone();selection.inverted^=hide;
            let parent_offset=parent.map_or(Point::default(),|h|doc.layer_offset(h));
            let geometry=coverage.use_.geometry_in_parent(owner).as_affine().map(|map|map.then(Affine::translation(parent_offset)));
            let inverse=if let Some(geometry)=geometry {geometry.inverse().ok_or("Invalid mask placement")?} else {
                coverage.use_.linked=false;coverage.use_.translation=Point{x:-parent_offset.x,y:-parent_offset.y};
                coverage.source.domain=doc.composition().size;Affine::IDENTITY
            };
            coverage.source.initial=Some(selection.transformed(inverse).map_err(error)?);
            coverage.source.default_coverage=f32::from(selection.inverted);
        }
        Ok((RecordChange::insert(&doc.artwork.coverage,coverage.source),coverage.use_))
    }
}
