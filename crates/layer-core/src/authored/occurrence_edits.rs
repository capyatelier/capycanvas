use super::*;
use crate::{Document, DocumentError, Edit, Point};
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

fn insert<T: Clone>(store: &mut Store<T>, value: T) -> Result<RecordChange<T>, DocumentError> {
    let change = RecordChange::insert(store, value);
    store.change(change.handle, change.id, change.value.clone())?;
    Ok(change)
}
fn content_position(scene:SceneView<'_>,entries:&[OccurrenceHandle],index:usize)->usize {
    let Some(next)=entries.get(index..).unwrap_or(&[]).iter().copied().find(|h|scene.occurrence(*h).is_some_and(|o|o.is_artwork())) else{return index;};
    let owner=scene.effect_owner(next).unwrap_or(next);
    entries.iter().position(|h|scene.effect_owner(*h)==Some(owner)).map_or(index,|top|top.min(index))
}
fn invalid(message: &'static str) -> DocumentError { DocumentError::InvalidLayerOperation(message) }

impl Document {
    pub fn group_blend_edit(&self,id:OccurrenceHandle,blend:crate::LayerBlend)->Result<Edit,DocumentError> {
        let scene=self.scene();let original=scene.occurrence(id).ok_or(DocumentError::MissingOccurrence(id))?;
        if !matches!(original.content,OccurrenceContent::Stack(_)){return Err(invalid("Choose a group"));}
        if self.is_locked(id){return Err(DocumentError::ProtectedOccurrence(id));}
        if blend==crate::LayerBlend::PassThrough && (original.attachment!=Attachment::None || !scene.attached_effects(id).is_empty() || scene.order().iter().any(|h|scene.clipping_base(*h)==Some(id))) {return Err(invalid("Release the group's clipping and effects before using Pass Through"));}
        let mut occurrence=original.clone();
        if blend==crate::LayerBlend::PassThrough {if occurrence.blend!=blend {occurrence.isolated_blend=occurrence.blend;}} else {occurrence.isolated_blend=blend;}
        occurrence.blend=blend;
        Ok(Edit::Occurrence(RecordChange::replace(&self.artwork.occurrences,id,Some(occurrence))?))
    }
    pub fn attachment_edit(&self,id:OccurrenceHandle,enabled:bool,isolate:bool)->Result<Edit,DocumentError> {
        let scene=self.scene();let original=scene.occurrence(id).ok_or(DocumentError::MissingOccurrence(id))?;
        if self.is_locked(id){return Err(DocumentError::ProtectedOccurrence(id));}
        if !original.is_artwork(){return Err(invalid("Selection Layers cannot attach"));}
        let adjustment=scene.effect(id).is_some_and(|e|e.program.kind==crate::EffectKind::Adjustment);
        if enabled&&adjustment {
            let owner=self.attachment_target_below(id,false).ok_or(invalid("Choose paint or an isolated group below"))?;
            return self.attach_effect_edit(id,owner,scene.attached_effects(owner).iter().filter(|h|**h!=id).count(),isolate);
        }
        let mut occurrence=original.clone();let mut edits=Vec::new();
        if enabled {
            let target=self.attachment_target_below(id,true).ok_or(invalid("Choose paint or an isolated group below"))?;
            if original.passes_through(){if !isolate{return Err(invalid("Isolate the group before clipping"));}occurrence.blend=occurrence.isolated_blend;}
            if scene.occurrence(target).is_some_and(Occurrence::passes_through){if !isolate{return Err(invalid("Isolate the target group before attaching"));}edits.push(self.group_blend_edit(target,scene.occurrence(target).unwrap().isolated_blend)?);}
            occurrence.attachment=Attachment::Clip;
        } else {occurrence.attachment=Attachment::None;}
        edits.push(Edit::Occurrence(RecordChange::replace(&self.artwork.occurrences,id,Some(occurrence))?));
        if !enabled && original.attachment!=Attachment::None {
            let stack=scene.stack(id).ok_or(DocumentError::MissingOccurrence(id))?;let mut entries=self.artwork.stacks.get(stack).unwrap().clone();
            let owner=scene.effect_owner(id).unwrap_or(id);let base=scene.clipping_base(owner).unwrap_or(owner);
            let moving=if adjustment {vec![id]}else{let mut members=scene.attached_effects(id).to_vec();members.push(id);members.sort_by_key(|h|scene.position(*h));members};
            let top=self.clipping_stack_top(base).unwrap_or(base);let old_at=entries.entries.iter().position(|h|*h==top).unwrap();
            let at=entries.entries[..old_at].iter().filter(|h|!moving.contains(h)).count();entries.entries.retain(|h|!moving.contains(h));entries.entries.splice(at..at,moving.iter().copied());
            if self.artwork.stacks.get(stack)!=Some(&entries){edits.push(Edit::Stack(RecordChange::replace(&self.artwork.stacks,stack,Some(entries))?));}
        }
        self.checked_relationship_edit(Edit::Batch(edits),&self.relationship_roots(&[id]))
    }
    pub(crate) fn attachment_target_below(&self,id:OccurrenceHandle,clip:bool)->Option<OccurrenceHandle> {
        let scene=self.scene();let siblings=scene.children(scene.parent(id));let at=siblings.iter().position(|h|*h==id)?;
        siblings[at+1..].iter().copied().find(|h|scene.occurrence(*h).is_some_and(|o|o.is_artwork()&&if clip{o.attachment==Attachment::None}else{o.attachment!=Attachment::Effect})).filter(|h|scene.occurrence(*h).is_some_and(|o|matches!(o.content,OccurrenceContent::Paint(_)|OccurrenceContent::Stack(_))))
    }
    pub fn content_insertion(&self,parent:Option<OccurrenceHandle>,index:usize)->(usize,Attachment) {
        let index=content_position(self.scene(),self.scene().children(parent),index);
        (index,self.insertion_attachment(parent,index))
    }
    pub fn insertion_attachment(&self,parent:Option<OccurrenceHandle>,index:usize)->Attachment {
        let scene=self.scene();let siblings=scene.children(parent);
        let below=siblings.get(index..).unwrap_or(&[]).iter().copied().find(|h|scene.occurrence(*h).is_some_and(|o|o.is_artwork()&&o.attachment!=Attachment::Effect));
        let above=siblings.get(..index).unwrap_or(&[]).iter().rev().copied().find(|h|scene.occurrence(*h).is_some_and(|o|o.is_artwork()&&o.attachment!=Attachment::Effect));
        if below.is_some_and(|h|scene.occurrence(h).unwrap().attachment==Attachment::Clip)||above.is_some_and(|h|scene.occurrence(h).unwrap().attachment==Attachment::Clip){Attachment::Clip}else{Attachment::None}
    }
    pub(crate) fn checked_relationship_edit(&self,edit:Edit,changed:&[OccurrenceHandle])->Result<Edit,DocumentError> {
        let mut candidate=self.clone();candidate.apply(edit.clone())?;let old=self.scene();let new=candidate.scene();
        for &h in old.order().iter().filter(|h|!changed.contains(h)) {
            if old.attachment_target(h)!=new.attachment_target(h){return Err(invalid("Keep unrelated layers with their current clipping base and effect owner"));}
        }
        Ok(edit)
    }
    pub fn attach_effect_edit(&self,id:OccurrenceHandle,owner:OccurrenceHandle,index:usize,isolate:bool)->Result<Edit,DocumentError> {
        let scene=self.scene();let original=scene.occurrence(id).ok_or(DocumentError::MissingOccurrence(id))?;
        if !scene.effect(id).is_some_and(|e|e.program.kind==crate::EffectKind::Adjustment){return Err(invalid("Choose an adjustment effect"));}
        if self.is_locked(id){return Err(DocumentError::ProtectedOccurrence(id));}
        if self.is_locked(owner){return Err(DocumentError::ProtectedOccurrence(owner));}
        let target=scene.occurrence(owner).ok_or(DocumentError::MissingOccurrence(owner))?;
        if !matches!(target.content,OccurrenceContent::Paint(_)|OccurrenceContent::Stack(_)){return Err(invalid("Choose paint or an isolated group"));}
        let chain:Vec<_>=scene.attached_effects(owner).iter().copied().filter(|h|*h!=id).collect();
        if index>chain.len(){return Err(invalid("Invalid effect position"));}
        let old_stack=scene.stack(id).ok_or(DocumentError::MissingOccurrence(id))?;let stack=scene.stack(owner).ok_or(DocumentError::MissingOccurrence(owner))?;
        let mut destination=self.artwork.stacks.get(stack).unwrap().clone();destination.entries.retain(|h|*h!=id);
        let anchor=if index==0 {owner}else{chain[index-1]};let at=destination.entries.iter().position(|h|*h==anchor).unwrap();destination.entries.insert(at,id);
        let mut occurrence=original.clone();occurrence.attachment=Attachment::Effect;
        let old_origin=scene.parent(id).map_or(Point::default(),|h|self.layer_offset(h));let new_origin=scene.parent(owner).map_or(Point::default(),|h|self.layer_offset(h));
        let delta=Point{x:old_origin.x-new_origin.x,y:old_origin.y-new_origin.y};occurrence.translation.x+=delta.x;occurrence.translation.y+=delta.y;
        if let Some(mask)=&mut occurrence.mask{mask.translation.x+=delta.x;mask.translation.y+=delta.y;}
        let mut edits=vec![Edit::Occurrence(RecordChange::replace(&self.artwork.occurrences,id,Some(occurrence))?),Edit::Stack(RecordChange::replace(&self.artwork.stacks,stack,Some(destination))?)];
        if old_stack!=stack {let mut old=self.artwork.stacks.get(old_stack).unwrap().clone();old.entries.retain(|h|*h!=id);edits.push(Edit::Stack(RecordChange::replace(&self.artwork.stacks,old_stack,Some(old))?));}
        if target.passes_through(){if !isolate{return Err(invalid("Isolate the target group before attaching"));}edits.push(self.group_blend_edit(owner,target.isolated_blend)?);}
        self.checked_relationship_edit(Edit::Batch(edits),&[id])
    }
    pub fn reparent_occurrence_edit(&self,id:OccurrenceHandle,parent:Option<OccurrenceHandle>,index:usize)->Result<Edit,DocumentError> {
        let scene=self.scene();scene.occurrence(id).ok_or(DocumentError::MissingOccurrence(id))?;
        let old_stack=scene.stack(id).ok_or(DocumentError::MissingOccurrence(id))?;
        let moving=self.relationship_roots(&[id]);
        if parent.is_some_and(|h|self.layer_subtrees(&moving).contains(&h)){return Err(invalid("A group cannot contain itself"));}
        if let Some(parent)=parent && self.is_locked(parent){return Err(DocumentError::ProtectedOccurrence(parent));}
        for &h in &moving {if self.is_locked(h){return Err(DocumentError::ProtectedOccurrence(h));}}
        let new_stack=match parent {Some(h)=>match scene.occurrence(h).map(|o|&o.content){Some(OccurrenceContent::Stack(s))=>*s,_=>return Err(invalid("Choose a group"))},None=>self.composition().result};
        let mut destination=self.artwork.stacks.get(new_stack).unwrap().clone();destination.entries.retain(|h|!moving.contains(h));
        if index>destination.entries.len(){return Err(invalid("Invalid layer position"));}
        let original=scene.occurrence(id).unwrap();let adjustment=scene.effect(id).is_some_and(|e|e.program.kind==crate::EffectKind::Adjustment);
        let at=if adjustment{index}else{content_position(scene,&destination.entries,index)};
        let anchor=destination.entries.get(at).copied();
        let next=destination.entries[at..].iter().copied().find(|h|scene.occurrence(*h).is_some_and(|o|o.is_artwork()&&o.attachment!=Attachment::Effect));
        let previous=destination.entries[..at].iter().rev().copied().find(|h|scene.occurrence(*h).is_some_and(|o|o.is_artwork()&&o.attachment!=Attachment::Effect));
        let joins=next.is_some_and(|h|scene.occurrence(h).unwrap().attachment==Attachment::Clip)||previous.is_some_and(|h|scene.occurrence(h).unwrap().attachment==Attachment::Clip);
        let attaches=anchor.is_some_and(|h|scene.effect_owner(h).is_some() || scene.eligible_target(h)&&(scene.effect_owner(id)==Some(h)||destination.entries[..at].iter().rev().find(|h|scene.occurrence(**h).is_some_and(|o|o.is_artwork())).is_some_and(|previous|scene.effect_owner(*previous)==Some(h))));
        let attachment=if !original.is_artwork(){Attachment::None}else if adjustment {if attaches{Attachment::Effect}else{Attachment::None}}else if joins&&!original.passes_through(){Attachment::Clip}else{Attachment::None};
        destination.entries.splice(at..at,moving.iter().copied());
        let delta=Point{x:scene.parent(id).map_or(0.,|h|self.layer_offset(h).x)-parent.map_or(0.,|h|self.layer_offset(h).x),y:scene.parent(id).map_or(0.,|h|self.layer_offset(h).y)-parent.map_or(0.,|h|self.layer_offset(h).y)};
        let mut edits=Vec::new();
        for &h in &moving {let mut occurrence=scene.occurrence(h).unwrap().clone();if h==id {occurrence.attachment=attachment;}occurrence.translation.x+=delta.x;occurrence.translation.y+=delta.y;if let Some(mask)=&mut occurrence.mask{mask.translation.x+=delta.x;mask.translation.y+=delta.y;}if scene.occurrence(h)!=Some(&occurrence){edits.push(Edit::Occurrence(RecordChange::replace(&self.artwork.occurrences,h,Some(occurrence))?));}}
        if old_stack!=new_stack {let mut old=self.artwork.stacks.get(old_stack).unwrap().clone();old.entries.retain(|h|!moving.contains(h));edits.push(Edit::Stack(RecordChange::replace(&self.artwork.stacks,old_stack,Some(old))?));}
        if self.artwork.stacks.get(new_stack)!=Some(&destination){edits.push(Edit::Stack(RecordChange::replace(&self.artwork.stacks,new_stack,Some(destination))?));}
        self.checked_relationship_edit(Edit::Batch(edits),&moving)
    }

    pub fn duplicate_layers_edit(&self, roots: &[OccurrenceHandle]) -> Result<(Edit, Vec<OccurrenceHandle>), DocumentError> {
        if roots.is_empty() { return Err(invalid("Select layers first")); }
        let scene = self.scene();
        for h in roots {
            scene.occurrence(*h).ok_or(DocumentError::MissingOccurrence(*h))?;
            if scene.stack(*h).is_none() { return Err(DocumentError::MissingOccurrence(*h)); }
            if scene.parent(*h).is_some_and(|parent| self.is_locked(parent)) { return Err(invalid("The destination group is locked")); }
        }
        let selected_roots=self.layer_roots(&roots.iter().copied().collect());
        let roots = self.layer_roots(&self.relationship_roots(&selected_roots).into_iter().collect());
        let mut artwork = self.artwork.clone();
        let mut edits = Vec::new();
        let mut copies = BTreeMap::new();
        let mut effects = BTreeMap::new();
        fn copy(source: &Artwork, target: &mut Artwork, id: OccurrenceHandle, edits: &mut Vec<Edit>, copies: &mut BTreeMap<OccurrenceHandle, OccurrenceHandle>, effects: &mut BTreeMap<EffectHandle, EffectHandle>) -> Result<OccurrenceHandle, DocumentError> {
            let mut occurrence = source.occurrences.get(id).ok_or(DocumentError::MissingOccurrence(id))?.clone();
            occurrence.content = match occurrence.content {
                OccurrenceContent::Paint(h) => {
                    let change = insert(&mut target.paint, source.paint.get(h).ok_or(invalid("Unknown paint source"))?.clone())?;
                    let content = OccurrenceContent::Paint(change.handle); edits.push(Edit::Paint(change)); content
                }
                OccurrenceContent::Stack(h) => {
                    let original = source.stacks.get(h).ok_or(invalid("Unknown stack"))?;
                    let entries = original.entries.iter().map(|h| copy(source, target, *h, edits, copies, effects)).collect::<Result<_, _>>()?;
                    let change = insert(&mut target.stacks, Stack { entries })?;
                    let content = OccurrenceContent::Stack(change.handle); edits.push(Edit::Stack(change)); content
                }
                OccurrenceContent::Effect(h) => {
                    let change = insert(&mut target.effects, source.effects.get(h).ok_or(invalid("Unknown effect"))?.clone())?;
                    effects.insert(h, change.handle);
                    let content = OccurrenceContent::Effect(change.handle); edits.push(Edit::Effect(change)); content
                }
                OccurrenceContent::Selection(h) => {
                    let change = insert(&mut target.selections, source.selections.get(h).ok_or(invalid("Unknown selection"))?.clone())?;
                    let content = OccurrenceContent::Selection(change.handle); edits.push(Edit::SavedSelection(change)); content
                }
            };
            if let Some(mask) = &mut occurrence.mask {
                let change = insert(&mut target.coverage, source.coverage.get(mask.source).ok_or(invalid("Unknown coverage source"))?.clone())?;
                mask.source = change.handle; edits.push(Edit::Coverage(change));
            }
            let change = insert(&mut target.occurrences, occurrence)?;
            let handle = change.handle; copies.insert(id, handle); edits.push(Edit::Occurrence(change)); Ok(handle)
        }
        for id in &roots {copy(&self.artwork, &mut artwork, *id, &mut edits, &mut copies, &mut effects)?;}
        let duplicated=selected_roots.iter().map(|id|copies[id]).collect();
        let stacks: BTreeSet<_> = roots.iter().map(|h| scene.stack(*h).unwrap()).collect();
        for h in stacks {
            let mut stack = self.artwork.stacks.get(h).unwrap().clone();
            let members: Vec<_> = roots.iter().copied().filter(|id| scene.stack(*id) == Some(h)).collect();
            let first = members[0];
            let anchor = if scene.occurrence(first).unwrap().attachment.is_clip() { first } else { self.clipping_stack_top(first).unwrap_or(first) };
            let at = stack.entries.iter().position(|id| *id == anchor).unwrap();
            stack.entries.splice(at..at, members.iter().map(|id| copies[id]));
            edits.push(Edit::Stack(RecordChange::replace(&self.artwork.stacks, h, Some(stack))?));
        }
        for (h, _, output) in self.artwork.outputs.iter() {
            let additions: Vec<_> = output.context.phases.iter().filter_map(|(h, phase)| effects.get(h).map(|copy| (*copy, *phase))).collect();
            if !additions.is_empty() {
                let mut output = output.clone(); Arc::make_mut(&mut output.context.phases).extend(additions);
                edits.push(Edit::Output(RecordChange::replace(&self.artwork.outputs, h, Some(output))?));
            }
        }
        let edit = Edit::Batch(edits);
        let mut candidate = self.clone(); candidate.apply(edit.clone())?;
        for &h in scene.order() {
            let Some(target)=scene.attachment_target(h) else{continue;};
            if candidate.scene().attachment_target(h)!=Some(target){return Err(invalid("Include the complete clipping and effect chains"));}
            if let Some(copy)=copies.get(&h) {
                let expected=copies.get(&target).copied().unwrap_or(target);
                if candidate.scene().attachment_target(*copy)!=Some(expected){return Err(invalid("Include the complete clipping and effect chains"));}
            }
        }
        Ok((edit, duplicated))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoverageSnapshot, DocumentNames};
    use std::sync::Arc;

    fn document() -> Document {
        Document::new(PortableId::random(), 16, 16, DocumentNames { paint: "Ink".into(), paper: "Paper".into() })
    }
    fn group(doc: &mut Document, name: &str, translation: Point) -> OccurrenceHandle {
        let stack = RecordChange::insert(&doc.artwork.stacks, Stack::default());
        let mut occurrence = Occurrence::new(OccurrenceContent::Stack(stack.handle), name);
        occurrence.translation = translation;
        let occurrence = RecordChange::insert(&doc.artwork.occurrences, occurrence);
        let h = occurrence.handle;
        let root = doc.composition().result;
        let mut entries = doc.artwork.stacks.get(root).unwrap().clone(); entries.entries.insert(0, h);
        doc.apply(Edit::Batch(vec![Edit::Stack(stack), Edit::Occurrence(occurrence), Edit::Stack(RecordChange::replace(&doc.artwork.stacks, root, Some(entries)).unwrap())])).unwrap();
        h
    }
    fn mask(doc: &mut Document, id: OccurrenceHandle, translation: Point) -> CoverageHandle {
        let snapshot = CoverageSnapshot::reveal_all(doc.artwork.coverage.next_handle(), [16, 16], translation);
        let source = RecordChange::insert(&doc.artwork.coverage, snapshot.source);
        let mut occurrence = doc.scene().occurrence(id).unwrap().clone(); occurrence.mask = Some(snapshot.use_);
        let h = source.handle;
        doc.apply(Edit::Batch(vec![Edit::Coverage(source), Edit::Occurrence(RecordChange::replace(&doc.artwork.occurrences, id, Some(occurrence)).unwrap())])).unwrap();
        h
    }

    #[test]
    fn relationship_edits_move_join_release_duplicate_and_delete_atomically() {
        use crate::{operation_test_support as f,LayerBlend};
        let mut doc=f::document([32,32],&["Clip","FX","Base","Lower","Group","Child"]);
        f::effect(&mut doc,"FX","gaussian_blur");f::nest(&mut doc,"Group",&["Child"]);
        let clip=f::id(&doc,"Clip");let fx=f::id(&doc,"FX");let base=f::id(&doc,"Base");let lower=f::id(&doc,"Lower");let group=f::id(&doc,"Group");
        doc.apply(doc.attachment_edit(fx,true,false).unwrap()).unwrap();doc.apply(doc.attachment_edit(clip,true,false).unwrap()).unwrap();
        assert!(doc.group_layers_edit(&[fx],LayerBlend::Normal,"Dangling effect").is_err());
        let before=doc.clone();let undo=doc.apply(doc.reparent_occurrence_edit(base,Some(group),0).unwrap()).unwrap();
        assert_eq!(doc.scene().children(Some(group)),[clip,fx,base,f::id(&doc,"Child")]);assert_eq!(doc.scene().clipping_base(clip),Some(base));doc.apply(undo).unwrap();f::restored(&before,&doc);
        let undo=doc.apply(doc.attachment_edit(base,true,false).unwrap()).unwrap();assert_eq!(doc.scene().clipping_base(clip),Some(lower));assert_eq!(doc.scene().clipping_base(base),Some(lower));doc.apply(undo).unwrap();
        let undo=doc.apply(doc.attachment_edit(fx,false,false).unwrap()).unwrap();assert_eq!(doc.scene().children(None)[0],fx);assert_eq!(doc.scene().clipping_base(clip),Some(base));doc.apply(undo).unwrap();
        let (edit,copies)=doc.duplicate_layers_edit(&[base]).unwrap();let undo=doc.apply(edit).unwrap();let copied=copies[0];assert_eq!(doc.scene().attached_effects(copied).len(),1);assert_eq!(doc.scene().clipping_base(clip),Some(base));doc.apply(undo).unwrap();
        let undo=doc.apply(doc.delete_layers_edit(&[base]).unwrap()).unwrap();assert!(doc.scene().occurrence(fx).is_none());assert!(doc.scene().occurrence(clip).is_none());doc.apply(undo).unwrap();f::restored(&before,&doc);
        doc.apply(doc.group_blend_edit(group,LayerBlend::Multiply).unwrap()).unwrap();doc.apply(doc.group_blend_edit(group,LayerBlend::PassThrough).unwrap()).unwrap();assert_eq!(doc.scene().occurrence(group).unwrap().isolated_blend,LayerBlend::Multiply);
        assert!(doc.attach_effect_edit(fx,group,0,false).is_err());let undo=doc.apply(doc.attach_effect_edit(fx,group,0,true).unwrap()).unwrap();assert_eq!(doc.scene().occurrence(group).unwrap().blend,LayerBlend::Multiply);assert!(doc.group_blend_edit(group,LayerBlend::PassThrough).is_err());
        let reopened=f::roundtrip(&doc);assert_eq!(reopened.scene().occurrence(f::id(&reopened,"Group")).unwrap().isolated_blend,LayerBlend::Multiply);doc.apply(undo).unwrap();
    }
    #[test]
    fn saved_selections_stay_above_contiguous_effect_chains_and_undo_atomically() {
        use crate::operation_test_support as f;
        let mut doc=f::document([32,32],&["Curves","Saved","Blur","Shade","Base"]);
        f::effect(&mut doc,"Curves","exposure");f::effect(&mut doc,"Blur","gaussian_blur");f::saved(&mut doc,"Saved",crate::Selection::empty());
        let curves=f::id(&doc,"Curves");let blur=f::id(&doc,"Blur");let shade=f::id(&doc,"Shade");let base=f::id(&doc,"Base");let saved=f::id(&doc,"Saved");
        for id in [shade,blur]{doc.apply(doc.attachment_edit(id,true,false).unwrap()).unwrap();}
        let original=doc.clone();let undo=doc.apply(doc.attachment_edit(curves,true,false).unwrap()).unwrap();
        assert_eq!(&doc.scene().children(None)[..5],&[saved,curves,blur,shade,base]);assert_eq!(doc.scene().occurrence(saved),original.scene().occurrence(saved));
        assert_eq!(doc.artwork.selections,original.artwork.selections);assert_eq!(doc.scene().attached_effects(shade),[blur,curves]);assert_eq!(doc.scene().clipping_base(shade),Some(base));
        let normalized=doc.clone();doc.apply(undo).unwrap();f::restored(&original,&doc);doc=normalized;
        let undo=doc.apply(doc.reparent_occurrence_edit(saved,None,3).unwrap()).unwrap();assert_eq!(&doc.scene().children(None)[..5],&[curves,blur,shade,saved,base]);doc.apply(undo).unwrap();
        doc.apply(doc.reparent_occurrence_edit(saved,None,3).unwrap()).unwrap();let below=doc.clone();let undo=doc.apply(doc.reparent_occurrence_edit(saved,None,2).unwrap()).unwrap();
        assert_eq!(&doc.scene().children(None)[..5],&[saved,curves,blur,shade,base]);assert_eq!(doc.scene().occurrence(saved).unwrap().attachment,Attachment::None);assert_eq!(doc.scene().attached_effects(shade),[blur,curves]);
        let reopened=f::roundtrip(&doc);assert_eq!(f::names(&reopened),f::names(&doc));doc.apply(undo).unwrap();f::restored(&below,&doc);
        let mut invalid=doc.artwork.clone();let entries=&mut invalid.stacks.get_mut(doc.composition().result).unwrap().entries;entries.retain(|h|*h!=saved);entries.insert(1,saved);assert!(SceneIndex::build(&invalid).is_err());
    }
    #[test]
    fn duplicates_share_immutable_resources_but_own_edits_and_restore_exact_handles() {
        let mut doc = document();
        let id = doc.scene().order()[0];
        let original = crate::color::source::rgba8_source([16, 16], |_, _| [40, 80, 120, 255]);
        let OccurrenceContent::Paint(paint) = doc.scene().occurrence(id).unwrap().content else { unreachable!() };
        doc.artwork.paint.get_mut(paint).unwrap().original = Some(original.clone());
        doc.artwork.occurrences.get_mut(id).unwrap().reference = true;
        let coverage = mask(&mut doc, id, Point { x: 3., y: 4. });
        let unplaced = doc.artwork.paint.insert(PortableId::random(), doc.artwork.paint.get(paint).unwrap().clone()).unwrap();
        let note_id = PortableId::random();
        Arc::make_mut(&mut doc.artwork.extensions).records.insert(note_id, serde_json::json!({"id":note_id,"type":"test.note/1","ancillary":true,"copy_safe":true,"data":{"owner":{"ref":doc.artwork.occurrences.id(id).unwrap()}}}));
        let retained = doc.artwork.extensions.clone();
        let (edit, copies) = doc.duplicate_layers_edit(&[id]).unwrap();
        let copied = copies[0];
        let inverse = doc.apply(edit).unwrap();
        let copied_id = doc.artwork.occurrences.id(copied).unwrap();
        let OccurrenceContent::Paint(copied_paint) = doc.scene().occurrence(copied).unwrap().content else { unreachable!() };
        let copied_mask = doc.scene().mask(copied).unwrap().0.source;
        assert_ne!(paint, copied_paint);
        assert_ne!(doc.artwork.paint.id(paint), doc.artwork.paint.id(copied_paint));
        assert_ne!(coverage, copied_mask);
        assert_ne!(doc.artwork.coverage.id(coverage), doc.artwork.coverage.id(copied_mask));
        assert!(doc.scene().occurrence(copied).unwrap().reference);
        assert!(Arc::ptr_eq(doc.artwork.paint.get(copied_paint).unwrap().original.as_ref().unwrap(), &original));
        let tile = original.tiles.values().next().unwrap();
        assert_eq!(doc.artwork.paint.get(copied_paint).unwrap().original.as_ref().unwrap().tiles.values().next().unwrap().resource_id(), tile.resource_id());
        assert!(doc.artwork.paint.get(unplaced).is_some());
        assert_eq!(doc.artwork.extensions, retained);
        let mut copy = doc.artwork.paint.get(copied_paint).unwrap().clone(); copy.original = None;
        let edit = Edit::Paint(RecordChange::replace(&doc.artwork.paint, copied_paint, Some(copy)).unwrap());
        let restore_copy = doc.apply(edit).unwrap();
        assert!(Arc::ptr_eq(doc.artwork.paint.get(paint).unwrap().original.as_ref().unwrap(), &original));
        doc.apply(restore_copy).unwrap();
        let redo = doc.apply(inverse).unwrap();
        assert!(doc.artwork.occurrences.get(copied).is_none());
        assert!(doc.artwork.paint.get(copied_paint).is_none());
        let (_, future) = doc.duplicate_layers_edit(&[id]).unwrap();
        assert!(future[0].index() > copied.index());
        doc.apply(redo).unwrap();
        assert_eq!(doc.artwork.occurrences.id(copied), Some(copied_id));
        assert_eq!(doc.scene().mask(copied).unwrap().0.source, copied_mask);
    }

    #[test]
    fn duplicated_groups_copy_applications_selections_and_membership_with_shared_definitions() {
        let mut doc = document();
        let paint = doc.scene().order()[0];
        let group_handle = group(&mut doc, "Group", Point { x: 7., y: 11. });
        doc.apply(doc.reparent_occurrence_edit(paint, Some(group_handle), 0).unwrap()).unwrap();
        let program = crate::bundled_effect_catalog().get("unsharp_mask").unwrap().program();
        let draft = crate::EffectInstance::new(program.clone());
        let definition = RecordChange::insert(&doc.artwork.definitions, Definition { program });
        let application = RecordChange::insert(&doc.artwork.effects, EffectApplication { definition: definition.handle, values: draft.values, domain: [16, 16] });
        let effect = RecordChange::insert(&doc.artwork.occurrences, Occurrence::new(OccurrenceContent::Effect(application.handle), "Effect"));
        let effect_handle = effect.handle; let app_handle = application.handle;
        let selection = RecordChange::insert(&doc.artwork.selections, SavedSelection { selection: crate::Selection::empty(), display: Default::default() });
        let selected = RecordChange { handle: OccurrenceHandle::from_index(effect.handle.index() + 1), id: PortableId::random(), value: Some(Occurrence::new(OccurrenceContent::Selection(selection.handle), "Selection")) };
        let selected_handle = selected.handle;
        let OccurrenceContent::Stack(stack) = doc.scene().occurrence(group_handle).unwrap().content else { unreachable!() };
        let output_handle = doc.artwork.default_output;
        let mut output = doc.output().clone(); Arc::make_mut(&mut output.context.phases).push((application.handle, 1.25));
        let entries = Stack { entries: vec![effect.handle, paint, selected.handle] };
        doc.apply(Edit::Batch(vec![Edit::Definition(definition), Edit::Effect(application), Edit::Occurrence(effect), Edit::SavedSelection(selection), Edit::Occurrence(selected), Edit::Stack(RecordChange::replace(&doc.artwork.stacks, stack, Some(entries)).unwrap()), Edit::Output(RecordChange::replace(&doc.artwork.outputs, output_handle, Some(output)).unwrap())])).unwrap();
        let definitions = doc.artwork.definitions.len();
        let (edit, copies) = doc.duplicate_layers_edit(&[group_handle, paint]).unwrap();
        assert_eq!(copies.len(), 1);
        doc.apply(edit).unwrap();
        let children = doc.scene().children(Some(copies[0]));
        assert_eq!(children.iter().map(|h| doc.scene().occurrence(*h).unwrap().name.as_ref()).collect::<Vec<_>>(), ["Effect", "Ink", "Selection"]);
        assert!(children.iter().all(|h| ![effect_handle, paint, selected_handle].contains(h)));
        assert_eq!(doc.layer_offset(children[1]), doc.layer_offset(paint));
        let copied_effect = doc.scene().effect_handle(children[0]).unwrap();
        assert_ne!(copied_effect, app_handle);
        assert_eq!(doc.scene().effect_application(children[0]).unwrap().definition, doc.scene().effect_application(effect_handle).unwrap().definition);
        assert!(doc.output().context.phases.contains(&(copied_effect, 1.25)));
        assert_ne!(doc.scene().source_target(children[2]), doc.scene().source_target(selected_handle));
        assert_eq!(doc.artwork.definitions.len(), definitions);
    }

    #[test]
    fn reparent_preserves_content_and_unlinked_mask_world_placements_and_undo() {
        let mut doc = document();
        let paint = doc.scene().order()[0];
        let coverage = mask(&mut doc, paint, Point { x: 19., y: 23. });
        let occurrence = doc.artwork.occurrences.get_mut(paint).unwrap();
        occurrence.translation = Point { x: 5., y: 9. }; occurrence.mask.as_mut().unwrap().linked = false;
        let group_handle = group(&mut doc, "Group", Point { x: 40., y: 60. });
        let paint_target = doc.scene().source_target(paint).unwrap();
        let mask_target = SourceTarget::Coverage(coverage);
        let geometry = [doc.target_geometry(paint_target), doc.target_geometry(mask_target)];
        let order = doc.scene().order().to_vec();
        let edit = doc.reparent_occurrence_edit(paint, Some(group_handle), 0).unwrap();
        let inverse = doc.apply(edit).unwrap();
        assert_eq!([doc.target_geometry(paint_target), doc.target_geometry(mask_target)], geometry);
        assert_eq!(doc.scene().children(Some(group_handle)), [paint]);
        doc.apply(inverse).unwrap();
        assert_eq!(doc.scene().order(), order);
        assert_eq!([doc.target_geometry(paint_target), doc.target_geometry(mask_target)], geometry);
        let child = group(&mut doc, "Child", Point::default());
        doc.apply(doc.reparent_occurrence_edit(child, Some(group_handle), 0).unwrap()).unwrap();
        assert!(doc.reparent_occurrence_edit(group_handle, Some(child), 0).is_err());
        doc.artwork.occurrences.get_mut(group_handle).unwrap().locked = true;
        assert!(doc.reparent_occurrence_edit(paint, Some(child), 0).is_err());
        assert!(doc.reparent_occurrence_edit(child, None, 0).is_err());
    }

    #[test]
    fn duplicate_and_move_preserve_existing_clipping_bases_with_ordinary_fills() {
        let mut doc = document();
        let base = doc.scene().order()[0]; let paper = doc.scene().order()[1];
        let (edit, copies) = doc.duplicate_layers_edit(&[base]).unwrap(); doc.apply(edit).unwrap(); let clipped = copies[0];
        doc.artwork.occurrences.get_mut(clipped).unwrap().attachment = crate::Attachment::Clip;
        let destination = group(&mut doc, "Group", Point::default());
        let inverse=doc.apply(doc.reparent_occurrence_edit(base,Some(destination),0).unwrap()).unwrap();
        assert_eq!(doc.scene().children(Some(destination)),[clipped,base]);
        assert_eq!(doc.scene().clipping_base(clipped),Some(base));doc.apply(inverse).unwrap();
        let inverse=doc.apply(doc.reparent_occurrence_edit(clipped,Some(destination),0).unwrap()).unwrap();
        assert_eq!(doc.scene().occurrence(clipped).unwrap().attachment,Attachment::None);doc.apply(inverse).unwrap();
        let before = doc.artwork.clone();
        let undo = doc.apply(doc.reparent_occurrence_edit(paper, None, 0).unwrap()).unwrap();
        assert_eq!(doc.scene().children(None)[0], paper);
        assert_eq!(doc.clipping_base(clipped), Some(base));
        doc.apply(undo).unwrap();
        assert_eq!(doc.artwork, before);
        let (edit, copies) = doc.duplicate_layers_edit(&[clipped, base]).unwrap(); doc.apply(edit).unwrap();
        assert_eq!(doc.clipping_base(clipped), Some(base));
        assert_eq!(doc.clipping_base(copies[0]), Some(copies[1]));
        let (edit, copied_base) = doc.duplicate_layers_edit(&[base]).unwrap(); doc.apply(edit).unwrap();
        assert_eq!(doc.clipping_base(clipped), Some(base));
        let before = doc.artwork.clone();
        let undo = doc.apply(doc.reparent_occurrence_edit(copied_base[0], None, doc.scene().children(None).len() - doc.relationship_roots(&[copied_base[0]]).len()).unwrap()).unwrap();
        assert_eq!(doc.scene().children(None).last(), Some(&copied_base[0]));
        assert_eq!(doc.clipping_base(clipped), Some(base));
        doc.apply(undo).unwrap();
        assert_eq!(doc.artwork, before);
    }
}
