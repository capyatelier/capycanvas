use super::*;
use std::collections::BTreeSet;

#[derive(Clone)]
pub(super) struct RegionBatch {
    pub output: PixelRect,
    pub pages: BTreeSet<[u32; 2]>,
}

pub(super) struct Regions {
    pending: Vec<PixelRect>,
    current: Option<RegionBatch>,
    failed: bool,
}
impl Regions {
    pub fn new(output:PixelRect)->Self {Self {pending:vec![output],current:None,failed:false}}
    pub fn prepare(&mut self,capacity:usize,steps:&mut usize,
        mut demand:impl FnMut(PixelRect)->Result<BTreeSet<[u32;2]>,GpuRasterError>,
    )->Result<Option<&RegionBatch>,GpuRasterError> {
        if self.failed {return Err(GpuRasterError::SourceWorkingSetExceeded);}
        while self.current.is_none() && !self.pending.is_empty() {
            if *steps==0 {return Err(GpuRasterError::NotReady);}
            *steps-=1;
            let output=self.pending.pop().unwrap();
            if output.is_empty() {continue;}
            let pages=match demand(output) {
                Ok(pages)=>Some(pages),
                Err(GpuRasterError::SourceWorkingSetExceeded)=>None,
                Err(error)=>return Err(error),
            };
            if let Some(pages)=pages.filter(|pages|pages.len()<=capacity) {
                self.current=Some(RegionBatch {output,pages});
                break;
            }
            let [x,y,right,bottom]=[output.min_x(),output.min_y(),output.max_x(),output.max_y()];
            let (first,second)=if output.width()>=output.height() && output.width()>1 {
                let middle=x+output.width()/2;
                (PixelRect::new(x,y,middle,bottom),PixelRect::new(middle,y,right,bottom))
            } else if output.height()>1 {
                let middle=y+output.height()/2;
                (PixelRect::new(x,y,right,middle),PixelRect::new(x,middle,right,bottom))
            } else {
                self.failed=true;
                return Err(GpuRasterError::SourceWorkingSetExceeded);
            };
            self.pending.push(second);self.pending.push(first);
        }
        Ok(self.current.as_ref())
    }
    pub fn advance(&mut self) {self.current=None;}
}
#[cfg(test)]
fn region_batches(output:PixelRect,capacity:usize,mut demand:impl FnMut(PixelRect)->Result<BTreeSet<[u32;2]>,GpuRasterError>)
    ->Result<Vec<RegionBatch>,GpuRasterError> {
    let mut regions=Regions::new(output);let mut batches=Vec::new();let mut steps=usize::MAX;
    while regions.prepare(capacity,&mut steps,&mut demand)?.is_some() {
        batches.push(regions.current.take().unwrap());
    }
    Ok(batches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_partition_preserves_every_odd_extent_output_pixel_and_requested_page() {
        let bounds = PixelRect::new(3, 7, 40, 26);
        let demand = |r: PixelRect| -> Result<BTreeSet<[u32; 2]>, GpuRasterError> {
            Ok((r.min_y()..r.max_y()).flat_map(|y| (r.min_x()..r.max_x()).map(move |x| [x / 3, y / 2])).collect())
        };
        let expected = demand(bounds).unwrap();
        let batches = region_batches(bounds, 4, demand).unwrap();
        let mut covered = BTreeSet::new();
        let mut pages = BTreeSet::new();
        for batch in batches {
            assert!(batch.pages.len() <= 4);
            assert_eq!(batch.pages, demand(batch.output).unwrap());
            pages.extend(batch.pages);
            for y in batch.output.min_y()..batch.output.max_y() {
                for x in batch.output.min_x()..batch.output.max_x() { assert!(covered.insert([x, y])); }
            }
        }
        assert_eq!(covered.len(), bounds.width() as usize * bounds.height() as usize);
        assert_eq!(pages, expected);
    }

    #[test]
    fn an_unadmitted_pixel_is_an_error_not_a_missing_directory_sample() {
        assert!(matches!(region_batches(PixelRect::new(0, 0, 2, 2), 1,
            |_| Ok(BTreeSet::from([[0, 0], [1, 0]]))), Err(GpuRasterError::SourceWorkingSetExceeded)));
    }

    #[test]
    fn coarse_only_regions_need_no_fine_storage_or_subdivision() {
        let bounds = PixelRect::new(0, 0, 6000, 4000);
        let batches = region_batches(bounds, 0, |_| Ok(BTreeSet::new())).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].output, bounds);
    }
}


pub(super) fn slot_for(slots:impl Iterator<Item=Option<[u32;2]>>,desired:&BTreeSet<[u32;2]>,coordinate:[u32;2])->Option<usize> {
    let mut vacant=None;let mut reusable=None;
    for (index,slot) in slots.enumerate() {
        match slot {
            Some(key) if key==coordinate=>return Some(index),
            None if vacant.is_none()=>vacant=Some(index),
            Some(key) if !desired.contains(&key) && reusable.is_none()=>reusable=Some(index),
            _=>{},
        }
    }
    vacant.or(reusable)
}
pub(super) struct Allocation {
    pub bytes:u64,
    pub retired:Arc<crate::residency::Tier>,
    pub pending:std::sync::OnceLock<crate::residency::Charge>,
}
impl Allocation {
    pub fn retire(&self) {self.pending.get_or_init(||self.retired.charge(self.bytes));}
}
#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn overflowing_or_uncertain_region_subdivides_without_losing_any_output() {
        let bounds=PixelRect::new(0,0,9,3);
        let batches=region_batches(bounds,1,|r|if r.area()>1 {Err(GpuRasterError::SourceWorkingSetExceeded)}
            else {Ok(BTreeSet::from([[r.min_x(),r.min_y()]]))}).unwrap();
        assert_eq!(batches.len(),27);
        assert_eq!(batches.iter().map(|b|b.output.area()).sum::<u64>(),bounds.area());
    }
    #[test]
    fn next_batch_can_reuse_only_slots_that_its_own_draw_does_not_require() {
        let old=[Some([1,1]),Some([2,2])];let desired=BTreeSet::from([[2,2],[3,3]]);
        assert_eq!(slot_for(old.into_iter(),&desired,[3,3]),Some(0));
        assert_eq!(slot_for(old.into_iter(),&desired,[2,2]),Some(1));
        assert_eq!(slot_for(old.into_iter(),&BTreeSet::from([[1,1],[2,2]]),[3,3]),None);
    }
    #[test]
    fn submitted_and_accepted_consumers_hold_one_retired_charge_until_the_last_completion() {
        let retired=Arc::new(crate::residency::Tier::default());let allocation=Arc::new(Allocation {bytes:608<<20,retired:retired.clone(),pending:Default::default()});
        let accepted=allocation.clone();let submitted=allocation.clone();
        allocation.retire();allocation.retire();drop(allocation);
        assert_eq!(retired.used(),608<<20);
        drop(submitted);assert_eq!(retired.used(),608<<20);
        drop(accepted);assert_eq!(retired.used(),0);
    }
    #[test]
    fn discarded_unretired_allocation_does_not_subtract_some_other_consumers_charge() {
        let retired=Arc::new(crate::residency::Tier::default());let other=retired.charge(17);
        drop(Allocation {bytes:91,retired:retired.clone(),pending:Default::default()});
        assert_eq!(retired.used(),17);drop(other);
    }
    #[test]
    fn cursor_yields_during_splitting_and_keeps_each_accepted_draw_until_advanced() {
        let bounds=PixelRect::new(0,0,8,5);let mut cursor=Regions::new(bounds);let mut covered=BTreeSet::new();let mut yields=0;
        loop {
            let mut steps=2;let mut calls=0;
            let result=cursor.prepare(1,&mut steps,|r| {
                calls+=1;
                if r.area()>1 {Err(GpuRasterError::SourceWorkingSetExceeded)}
                else {Ok(BTreeSet::from([[r.min_x(),r.min_y()]]))}
            }).map(|batch|batch.cloned());
            assert!(calls<=2);
            match result {
                Err(GpuRasterError::NotReady)=>{assert_eq!(steps,0);yields+=1;},
                Ok(Some(batch))=>{
                    assert!(covered.insert([batch.output.min_x(),batch.output.min_y()]));
                    let mut exhausted=0;
                    assert_eq!(cursor.prepare(1,&mut exhausted,|_|panic!("accepted work must survive upload deferral")).unwrap().unwrap().output,batch.output);
                    cursor.advance();
                },
                Ok(None)=>break,
                Err(error)=>panic!("unexpected planner error: {error:?}"),
            }
        }
        assert!(yields>0);assert_eq!(covered.len() as u64,bounds.area());
    }
    #[test]
    fn single_pixel_refusal_is_terminal_and_never_skips_the_missing_pixel() {
        let mut cursor=Regions::new(PixelRect::new(0,0,1,1));let mut steps=48;
        assert!(matches!(cursor.prepare(1,&mut steps,|_|Err(GpuRasterError::SourceWorkingSetExceeded)),Err(GpuRasterError::SourceWorkingSetExceeded)));
        assert!(matches!(cursor.prepare(1,&mut steps,|_|panic!("terminal refusal cannot retry as success")),Err(GpuRasterError::SourceWorkingSetExceeded)));
    }
    #[test]
    fn multiple_output_regions_share_one_frame_planning_allowance() {
        let mut steps=1;let mut first=Regions::new(PixelRect::new(0,0,1,1));let mut second=Regions::new(PixelRect::new(1,0,2,1));
        assert!(first.prepare(1,&mut steps,|_|Ok(Default::default())).unwrap().is_some());first.advance();
        assert!(matches!(second.prepare(1,&mut steps,|_|panic!("the frame allowance must not reset per region")),Err(GpuRasterError::NotReady)));
        steps=1;assert!(second.prepare(1,&mut steps,|_|Ok(Default::default())).unwrap().is_some());
    }

}
