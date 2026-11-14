use crate::submission::Submission;
use layer_core::raster::ContentVersion;
use crate::raster::contents::Intent;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Version {
    name: ContentVersion,
    submission: Submission,
}
impl Version {
    fn same(&self,other:&Self)->bool {self.name.identity()==other.name.identity() && self.submission.same(&other.submission)}
}

#[derive(Default)]
struct Names {
    version: Option<Version>,
    before: Option<Version>,
}

impl Names {
    fn held(&self) -> Option<&Version> {
        self.version.as_ref().filter(|version| !version.submission.discarded())
            .or_else(|| self.before.as_ref().filter(|version| !version.submission.discarded()))
    }
}

#[derive(Clone)]
pub(crate) struct Resident<T> {
    value: T,
    names: Arc<Mutex<Names>>,
}

pub(crate) trait Storage { fn storage_bytes(&self)->u64; }
impl<T:Storage> Resident<T> { pub fn storage_bytes(&self)->u64 {self.value.storage_bytes()} }
pub(crate) trait TextureStorage:Storage {
    fn texture(&self)->&wgpu::Texture;
    fn view(&self)->&wgpu::TextureView;
}
impl<T:TextureStorage> Resident<T> {
    #[cfg(test)]
    pub(crate) fn value_view(&self)->&wgpu::TextureView {self.value.view()}
    pub(crate) fn texture_reference(&self)->Resident<crate::source_access::RawTile> {
        Resident {value:crate::source_access::RawTile {texture:self.value.texture().clone().into(),view:self.value.view().clone()},names:self.names.clone()}
    }
    pub(crate) fn account_texture(&self,textures:&mut crate::RetainedTextures)->bool {textures.insert(self.value.texture().clone())}
    pub(crate) fn holds_texture(&self,texture:&wgpu::Texture)->bool {self.value.texture()==texture}
    pub fn format(&self)->wgpu::TextureFormat {self.value.texture().format()}
    pub fn size(&self)->wgpu::Extent3d {self.value.texture().size()}
}

pub(crate) struct Read<'a, T> {
    value: &'a T,
    version: Version,
    names:Arc<Mutex<Names>>,
}

#[derive(Clone)]
pub(crate) struct Input {
    version:Version,
    names:Arc<Mutex<Names>>,
}

impl Input {
    pub fn version(&self)->&ContentVersion {&self.version.name}
    pub(crate) fn same(&self,other:&Self)->bool {Arc::ptr_eq(&self.names,&other.names) && self.version.same(&other.version)}
}

pub(crate) struct RasterWrite<'a, T> {
    resident: &'a mut Resident<T>,
    submission: &'a Submission,
    intent: Option<Intent>,
}

pub(crate) struct Encoding {
    names:Arc<Mutex<Names>>,
    version:Version,
    intent:Option<Intent>,
}

pub(crate) struct Written {
    version: Version,
    intent: Option<Intent>,
    submission:Submission,
}

pub(crate) fn write_refused() {
    #[cfg(test)]
    crate::test_support::PassCounts {raster_write_defers:1,..Default::default()}.record();
}

impl Written {
    pub fn version(&self) -> &ContentVersion { &self.version.name }
    pub fn submission(&self) -> &Submission { &self.submission }
    pub fn intent(&self) -> Option<&Intent> { self.intent.as_ref() }
}

impl<T> Resident<T> {
    pub fn unwritten(value: T) -> Self { Self { value, names: Arc::default() } }

    #[cfg(test)]
    pub fn name(&self) -> Option<layer_core::raster::ContentStamp> { self.names.lock().unwrap().held().map(|version| version.name.identity()) }
    pub fn allocation_identity(&self)->usize {Arc::as_ptr(&self.names) as usize}
    pub fn has_name(&self,name:&ContentVersion)->bool {
        self.names.lock().unwrap().held().is_some_and(|version|version.name.matches(name)==Some(true))
    }

    pub fn read(&self, expected: impl Into<ContentVersion>, consumer: Option<&Submission>) -> Option<Read<'_, T>> {
        let expected = expected.into();
        let version = self.names.lock().unwrap().held().filter(|version| version.name.matches(&expected) == Some(true)
            && (version.submission.submitted() || consumer.is_some_and(|consumer| version.submission.same(consumer)))).cloned()?;
        Some(Read { value: &self.value, version, names:self.names.clone() })
    }

    pub fn write<'a>(&'a mut self, intent: &Intent, submission: &'a Submission) -> Option<RasterWrite<'a, T>> {
        if self.read(intent.version(), Some(submission)).is_none() {
            write_refused();
            return None;
        }
        Some(RasterWrite { resident: self, submission, intent: Some(intent.clone()) })
    }

    pub fn revise<'a>(&'a mut self,expected:&ContentVersion,submission:&'a Submission)->Option<RasterWrite<'a,T>> {
        self.read(expected,Some(submission))?;
        Some(RasterWrite {resident:self,submission,intent:None})
    }

    pub fn continue_written<'a>(&'a mut self,written:&Written,submission:&'a Submission)->Option<RasterWrite<'a,T>> {
        let intent=written.intent.as_ref()?;
        self.read(&written.version.name,Some(submission))?;
        Some(RasterWrite {resident:self,submission,intent:Some(intent.clone())})
    }

    pub fn initialize<'a>(&'a mut self, submission: &'a Submission) -> Option<RasterWrite<'a, T>> {
        let unwritten = self.names.lock().unwrap().held().is_none();
        unwritten.then_some(RasterWrite { resident: self, submission, intent: None })
    }

    pub fn overwrite<'a>(&'a mut self,submission:&'a Submission)->Option<RasterWrite<'a,T>> {
        let ordered=self.names.lock().unwrap().held().is_none_or(|version|version.submission.submitted() || version.submission.same(submission));
        ordered.then_some(RasterWrite {resident:self,submission,intent:None})
    }

    pub fn overwrite_from<'a>(&'a mut self,intent:&Intent,input:&Input,submission:&'a Submission)->Option<RasterWrite<'a,T>> {
        if input.version.submission.discarded() || input.version.name.matches(intent.version())!=Some(true)
            || !(input.version.submission.submitted() || input.version.submission.same(submission))
            || !input.names.lock().unwrap().held().is_some_and(|held|held.name.matches(&input.version.name)==Some(true)
                && (held.submission.submitted() || held.submission.same(submission))) {write_refused();return None;}
        let mut write=self.overwrite(submission)?;write.intent=Some(intent.clone());Some(write)
    }

    pub fn retire(self) -> T { self.value }

    fn publish(&mut self, name: ContentVersion, submission: Submission) {
        let mut names = self.names.lock().unwrap();
        if !names.version.as_ref().is_some_and(|previous| previous.submission.same(&submission)) {
            names.before = names.held().cloned();
        }
        names.version = Some(Version { name, submission });
    }
}

impl<'a, T> Read<'a, T> {
    pub fn value(&self) -> &'a T { self.value }
    pub fn input(&self)->Input {Input {version:self.version.clone(),names:self.names.clone()}}
    pub(crate) fn witnessed_by(&self,input:&Input)->bool {Arc::ptr_eq(&self.names,&input.names) && self.version.same(&input.version)}
    #[cfg(test)]
    pub fn name(&self) -> layer_core::raster::ContentStamp { self.version.name.identity() }
    pub fn version(&self) -> &ContentVersion { &self.version.name }
    #[cfg(test)]
    pub fn submission(&self) -> &Submission { &self.version.submission }
    pub fn replace(self,intent:&Intent,submission:&Submission)->Written {
        Written {version:self.version,intent:Some(intent.clone()),submission:submission.clone()}
    }
}

impl<T:TextureStorage> Read<'_,T> {
    pub fn alias<U:TextureStorage>(self,value:U)->Option<Resident<U>> {
        (self.value.texture()==value.texture() && self.value.view()==value.view()).then_some(Resident {value,names:self.names})
    }
}

impl<T> RasterWrite<'_, T> {
    pub fn value(&self) -> &T { &self.resident.value }
    pub fn encoded(self,name:impl Into<ContentVersion>)->Encoding {
        Encoding {names:self.resident.names.clone(),version:Version {name:name.into(),submission:self.submission.clone()},intent:self.intent}
    }
    pub fn publish(self, name: impl Into<ContentVersion>) -> Written {
        let name = name.into();
        self.resident.publish(name.clone(), self.submission.clone());
        Written { version: Version { name, submission: self.submission.clone() }, intent: self.intent, submission:self.submission.clone() }
    }
}

impl Encoding {
    pub fn intent(&self)->Option<&Intent> {self.intent.as_ref()}
    pub fn publish(mut self,actual:&Submission)->Written {
        self.version.submission=actual.clone();
        let mut names=self.names.lock().unwrap();
        if !names.version.as_ref().is_some_and(|previous|previous.submission.same(&self.version.submission)) {names.before=names.held().cloned();}
        names.version=Some(self.version.clone());
        Written {submission:self.version.submission.clone(),version:self.version,intent:self.intent}
    }
}

impl Resident<crate::PageSurface> {
    pub(crate) fn shared_storage(&self)->Self {
        Self {value:self.value.sharing(),names:self.names.clone()}
    }
    pub(crate) fn clear_bindings(&self) {
        self.value.material_output.clear();
    }
    pub(crate) fn discharge(&mut self) {self.value.discharge();}
    pub(crate) fn set_preview(&self,preview:Option<(crate::SourceTarget,[u32;2])>) {self.value.preview.set(preview);}
    pub(crate) fn preview(&self)->Option<(crate::SourceTarget,[u32;2])> {self.value.preview.get()}
}
