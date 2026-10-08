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
        if let Some(reason)=self.pixel_write_refusal() {return Some(reason);}
        doc.drawing_content().and_then(|target|doc.scene().source_owner(target)).and_then(|h|doc.scene().occurrence(h)).is_some_and(|o|o.alpha_locked)
            .then_some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_ALPHA_LOCK_KEEPS_TRANSPARENCY_UNLOCK_THE_LAYER_FIRST))
    }
    /// Why the active content can't take a pixel write, other than holding images.
    fn pixel_write_refusal(&self)->Option<Arc<str>> {
        match self.engine.document().try_drawing_content() {
            Ok(_)|Err(layer_core::DrawingRefusal::Object)=>None,
            Err(reason)=>Some(notices::drawing_refusal_text(reason,self.localization())),
        }
    }
    pub(super) fn clear_selection(&mut self, outside: bool) -> Result<(),String> {
        refused(self.clear_refusal())?;
        if self.refuse_image_content() {return Ok(());}
        let doc=self.engine.document();let target=doc.drawing_content().ok_or("Select a drawing layer")?;
        let alpha_locked=doc.scene().source_owner(target).and_then(|h|doc.scene().occurrence(h)).is_some_and(|o|o.alpha_locked);
        let mut selection=doc.working.selection.clone().ok_or_else(||self.localization().text(NO_SELECTION).to_string())?;
        selection.inverted^=outside;
        let operation=self.erase_operation(target,&selection,alpha_locked)?;
        self.engine.append_raster_operation(target,operation).map_err(error)?;
        self.layer_interaction.changed=true;Ok(())
    }
    pub(super) fn erase_operation(&mut self,target:SourceTarget,selection:&Selection,alpha_locked:bool)->Result<RasterOperation,String> {
        let doc=self.engine.document();let domain=doc.target_extent(target);
        let inverse=Affine::translation(layer_core::offsets::point(doc.target_offset(target))).inverse().ok_or("Invalid layer placement")?;
        let mut coverage=CoverageSnapshot::reveal_all(self.engine.allocate_coverage_handle(),domain,[0;2]);
        coverage.source.default_coverage=f32::from(selection.inverted);
        coverage.selection=Some(selection.transformed(inverse).map_err(error)?);
        Ok(RasterOperation {placement:Affine::IDENTITY,coverage,kind:RasterOperationKind::Erase {alpha_locked}})
    }
    pub(super) fn selection_to_layer_refusal(&self,cut:bool)->Option<Arc<str>> {
        let l=self.localization();let doc=self.engine.document();let scene=doc.scene();
        if self.selection_masks.target().is_some() {return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_ARTWORK_FIRST));}
        if matches!(doc.working.target,Some(SourceTarget::Coverage(_))) {return Some(l.text(MessageId::COMMANDS_RETURN_TO_THE_LAYER_S_ARTWORK_FIRST));}
        if doc.working.selection.is_none() {
            if cut {return Some(l.text(MessageId::COMMANDS_MAKE_A_SELECTION_FIRST));}
            let mut selected=self.selected_layers().iter().filter_map(|h|scene.occurrence(*h));
            return match selected.next() {
                None=>Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_SELECT_LAYERS_FIRST)),
                Some(_)=>None,
            };
        }
        let Some(id)=doc.working.occurrence else {return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_SELECT_A_LAYER_FIRST));};
        let Some(o)=scene.occurrence(id) else {return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_SELECT_A_LAYER_FIRST));};
        match o.kind() {
            LayerKind::Paint|LayerKind::Object=>{},
            LayerKind::Group=>return Some(notices::drawing_refusal_text(layer_core::DrawingRefusal::Group,l)),
            LayerKind::Effect=>return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_AN_EFFECT_LAYER_HAS_NO_PIXELS_OF_ITS_OWN)),
            LayerKind::Selection=>return Some(notices::drawing_refusal_text(layer_core::DrawingRefusal::SelectionLayer,l)),
        }
        if scene.parent(id).is_some_and(|h|doc.is_locked(h)) {return Some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_THE_LAYER_S_GROUP_IS_LOCKED));}
        if cut && let Some(reason)=self.pixel_write_refusal() {return Some(reason);}
        (cut && o.alpha_locked).then_some(l.text(MessageId::COMMANDS_REFUSAL_SELECTION_PIXELS_ALPHA_LOCK_KEEPS_TRANSPARENCY_UNLOCK_THE_LAYER_FIRST))
    }
    pub(super) fn selection_to_layer(&mut self,cut:bool)->Result<(),String> {
        refused(self.selection_to_layer_refusal(cut))?;
        if cut && self.refuse_image_content() {return Ok(());}
        let Some(selection)=self.engine.document().working.selection.clone() else {return self.layer_action(LayerAction::DuplicateSelected);};
        let doc=self.engine.document();let scene=doc.scene();let source_id=doc.working.occurrence.ok_or("Select a layer first")?;
        let source=scene.occurrence(source_id).ok_or("Select a layer first")?.clone();
        let source_target=scene.source_target(source_id);
        let top=doc.clipping_stack_top(source_id).ok_or("Unknown layer")?;
        let stack_handle=scene.stack(top).ok_or("Missing containing stack")?;
        let mut stack=doc.artwork.stacks.get(stack_handle).ok_or("Missing stack")?.clone();
        let index=stack.entries.iter().position(|h|*h==top).ok_or("Unknown layer")?;
        let parent_origin=scene.layer_origin(scene.parent(source_id));
        let (origin,extent)=doc.bake_extent(&BTreeSet::from([source_id])).map_err(|_|self.localization().text(MessageId::COMMANDS_COPY_PIXELS_TOO_LARGE).to_string())?;
        let mut snapshot=doc.snapshot();let mut members=vec![source_id];let mut ancestor=scene.parent(source_id);
        while let Some(h)=ancestor {members.push(h);ancestor=scene.parent(h);}
        for &h in &members {
            let o=Arc::make_mut(&mut snapshot).artwork.occurrences.get_mut(h).ok_or("Missing capture occurrence")?;
            o.visible=true;o.opacity=1.;o.blend=layer_core::LayerBlend::Normal;o.attachment=layer_core::Attachment::None;
            if h!=source_id {o.mask=None;}
        }
        let paint=RecordChange::insert(&doc.artwork.paint,PaintSource { color_mode: Default::default(),domain:extent,raster:Default::default(),base:None,operations:Arc::default()});
        let target=SourceTarget::Paint(paint.handle);
        let mut copy=Occurrence::new(OccurrenceContent::Paint(paint.handle),format!("{} copy",source.name));
        copy.opacity=source.opacity;copy.visible=source.visible;copy.blend=source.blend;
        copy.offset=layer_core::offsets::exact(origin).and_then(|origin|layer_core::offsets::checked_sub(origin,parent_origin)).ok_or("The copied pixels exceed the editor's range")?;
        let occurrence=RecordChange::insert(&doc.artwork.occurrences,copy);let id=occurrence.handle;
        stack.entries.insert(index,id);
        let mut working=doc.working.clone();working.occurrence=Some(id);working.target=Some(target);working.inspect_mask=None;
        working.layer_selection=BTreeSet::from([id]);working.layer_anchor=Some(id);
        let edits=vec![Edit::Paint(paint),Edit::Occurrence(occurrence),Edit::Stack(RecordChange::replace(&doc.artwork.stacks,stack_handle,Some(stack)).map_err(error)?),Edit::Working(working)];
        let mut coverage=CoverageSnapshot::reveal_all(self.engine.allocate_coverage_handle(),extent,[0;2]);
        coverage.source.default_coverage=f32::from(selection.inverted);
        coverage.selection=Some(selection.transformed(Affine::translation(Point{x:-origin.x,y:-origin.y})).map_err(error)?);
        let local=selection.translated(Point{x:-origin.x,y:-origin.y});
        let operation=RasterOperation {placement:Affine::IDENTITY,coverage,
            kind:RasterOperationKind::Bake {scene:snapshot,scope:SceneScope::Members(members.into()),offset:Point{x:-origin.x,y:-origin.y}}};
        if !cut {return self.insert_bake(layer_core::MergePlan {edits,result:id,target,operation},Some(local),Some(selection));}
        let source_target=source_target.ok_or("Missing paint source")?;
        let operations=vec![(target,operation),(source_target,self.erase_operation(source_target,&selection,source.alpha_locked)?)];
        self.engine.insert_with_operations(edits,operations,Some(None)).map_err(error)?;
        self.selection_masks.reselect=Some(selection);self.layer_interaction.changed=true;Ok(())
    }
    /// A new mask for `owner`. With a selection, the returned operation
    /// stores the selection's coverage in the mask.
    pub(super) fn selection_mask(&self,owner:&Occurrence,hide:bool,parent:Option<OccurrenceHandle>,domain:[u32;2])
        ->Result<(RecordChange<CoverageSource>,MaskUse,Option<RasterOperation>),String> {
        let doc=self.engine.document();let handle=doc.artwork.coverage.next_handle();
        let mut coverage=CoverageSnapshot::reveal_all(handle,domain,owner.mask.as_ref().map_or([0;2],|m|m.offset));
        coverage.use_.linked=owner.mask.as_ref().is_none_or(|m|m.linked);
        let mut operation=None;
        if let Some(selection)=&doc.working.selection {
            let mut selection=selection.clone();selection.inverted^=hide;
            let parent_origin=doc.scene().layer_origin(parent);
            let frame=if coverage.use_.linked && owner.positioned() {layer_core::offsets::checked_add(parent_origin,owner.offset)} else {Some(parent_origin)};
            let origin=layer_core::offsets::point(frame.and_then(|frame|layer_core::offsets::checked_add(frame,coverage.use_.offset)).ok_or("A layer offset exceeds the editor's range")?);
            coverage.source.default_coverage=f32::from(selection.inverted);
            let mut stored=CoverageSnapshot::reveal_all(handle,domain,[0;2]);
            stored.source.default_coverage=coverage.source.default_coverage;
            stored.selection=Some(selection.translated(Point{x:-origin.x,y:-origin.y}));
            operation=Some(RasterOperation {placement:Affine::IDENTITY,coverage:stored,kind:RasterOperationKind::Coverage});
        }
        Ok((RecordChange::insert(&doc.artwork.coverage,coverage.source),coverage.use_,operation))
    }
}
