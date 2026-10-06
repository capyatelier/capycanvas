//! Windows application adapter. After UI-thread surface initialization, the
//! canvas thread exclusively owns this object; UI callbacks only enqueue work.
#![deny(unsafe_op_in_unsafe_fn)]
#![recursion_limit = "256"]
#[cfg(any(target_os = "windows", test))]
mod actions;
#[cfg(any(target_os = "windows", test))]
mod document_io;
#[cfg(any(target_os = "windows", test))]
mod documents;
#[cfg(any(target_os = "windows", test))]
mod document_workflows;
#[cfg(any(target_os = "windows", test))]
mod proof;
#[cfg(target_os = "windows")]
mod display;
#[cfg(any(target_os = "windows", test))]
mod color_storage;
#[cfg(any(target_os = "windows", test))]
mod recovery;
mod events;
#[cfg(any(target_os = "windows", test))]
mod device_loss_test;
#[cfg(any(target_os = "windows", test))]
mod filter_packages;
#[cfg(any(target_os = "windows", test))]
mod navigator;
#[cfg(any(target_os = "windows", test))]
mod previews;
#[cfg(any(target_os = "windows", test))]
mod scopes;
#[cfg(any(target_os = "windows", test))]
mod workspace;
#[cfg(any(target_os = "windows", test))]
mod workspace_async;
#[cfg(any(target_os = "windows", test))]
mod workspace_service;
#[cfg(any(target_os = "windows", test))]
pub use navigator::{capy_navigator_aspect, capy_navigator_image};
#[cfg(any(target_os = "windows", test))]
mod settings;
#[cfg(any(target_os = "windows", test))]
mod storage;
pub use events::CapyPointer;
#[cfg(target_os = "windows")]
mod device;
#[cfg(any(target_os = "windows", test))]
mod frame_pacing;
#[cfg(target_os = "windows")]
mod host;
#[cfg(any(target_os = "windows", test))]
mod palette_files;
#[cfg(target_os = "windows")]
pub use host::*;

mod shared_controls;
pub use shared_controls::*;
mod color;
pub use color::*;

#[cfg(all(test, target_os = "windows"))]
mod gpu_recovery_tests;

#[cfg(test)]
mod test_support {
    use std::path::PathBuf;

    use layer_core::{Document, ProjectLimits, authored::{ArtworkCapture,CaptureCheckpoint,SourceTarget,PaintSource,OccurrenceHandle}};
    use std::{io::{Read,Write},sync::{Arc,atomic::AtomicBool}};
    pub(crate) fn capture(document:&Document)->ArtworkCapture {
        document.artwork.capture(CaptureCheckpoint{owner:document.owner,document:document.artwork.id,session_generation:0,artwork_generation:document.revision,working_generation:document.working.generation,edit_checkpoint:0}).unwrap()
    }
    pub(crate) fn write_capture(capture:&ArtworkCapture,mut output:impl Write)->Result<(),String> {
        let cancelled=AtomicBool::new(false);layer_core::package::codec::PreparedPackage::prepare(capture,None,&cancelled)?.write(&mut output,&cancelled)
    }
    pub(crate) fn write_document(document:&Document,output:impl Write)->Result<(),String>{write_capture(&capture(document),output)}
    pub(crate) fn read_document(mut input:impl Read,limits:ProjectLimits)->Result<Document,String> {
        let mut bytes=Vec::new();input.read_to_end(&mut bytes).map_err(|e|e.to_string())?;
        let source=layer_core::package::ImmutableBacking::new(Arc::new(Arc::<[u8]>::from(bytes)))?;
        let outcome=layer_core::package::codec::open(source,Default::default(),&AtomicBool::new(false))?;
        let layer_core::package::codec::OpenOutcome::Candidate{artwork,..}=outcome else{return Err("Expected editable authored package".into())};
        let document=Document::from_artwork(artwork).map_err(|e|e.to_string())?;document.validate(limits)?;Ok(document)
    }
    pub(crate) fn assert_authored_eq(actual:&Document,expected:&Document) {
        let mut a=Vec::new();let mut b=Vec::new();write_document(actual,&mut a).unwrap();write_document(expected,&mut b).unwrap();assert_eq!(a,b);
    }
    #[cfg(target_os="windows")]
    pub(crate) fn assert_capture_eq(actual:&Document,expected:&ArtworkCapture) {
        let mut a=Vec::new();let mut b=Vec::new();write_document(actual,&mut a).unwrap();write_capture(expected,&mut b).unwrap();assert_eq!(a,b);
    }
    #[cfg(target_os="windows")]
    pub(crate) fn occurrence_at(document:&Document,index:usize)->&layer_core::authored::Occurrence {document.scene().occurrence(document.scene().order()[index]).unwrap()}
    pub(crate) fn paint_at(document:&Document,index:usize)->&PaintSource {document.scene().paint_source(document.scene().order()[index]).unwrap()}
    pub(crate) fn paint_mut(document:&mut Document,index:usize)->&mut PaintSource {let SourceTarget::Paint(h)=document.scene().source_target(document.scene().order()[index]).unwrap() else{panic!("paint")};document.artwork.paint.get_mut(h).unwrap()}
    pub(crate) fn insert_effect(document:&mut Document,name:&str,effect:layer_core::EffectInstance)->OccurrenceHandle {
        use layer_core::authored::*;let application=document.artwork.effects.insert(PortableId::random(),EffectApplication::new(effect.program,effect.values,document.composition().size)).unwrap();let h=document.artwork.occurrences.insert(PortableId::random(),Occurrence::new(OccurrenceContent::Effect(application),name)).unwrap();let stack=document.composition().result;document.artwork.stacks.get_mut(stack).unwrap().entries.insert(0,h);document.apply(layer_core::Edit::Stack(RecordChange::replace(&document.artwork.stacks,stack,document.artwork.stacks.get(stack).cloned()).unwrap())).unwrap();document.working.occurrence=Some(h);document.working.target=None;h
    }
    pub(crate) fn editable(outcome:layer_ui::ImportOutcome)->layer_ui::ImportedDocument {let layer_ui::ImportOutcome::Editable(document)=outcome else{panic!("Expected editable photo")};document}

    pub(crate) fn isolated_storage() -> PathBuf {
        crate::storage::roots().unwrap();
        PathBuf::from(std::env::var_os(layer_host::storage::STORAGE_OVERRIDE).unwrap())
    }
    #[cfg(target_os = "windows")]
    pub(crate) fn temporary_files() {
        if crate::storage::roots().is_err() {
            layer_core::temp_files::set_directory(std::env::temp_dir()).unwrap();
        }
    }

    pub(crate) struct TempDir {
        pub(crate) path: PathBuf,
    }
    impl TempDir {
        pub(crate) fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("capy-windows-test-{}", layer_workspace::new_id()));
            std::fs::create_dir(&path).unwrap();
            Self { path }
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
