//! Shared accounting for admission and eviction. The current document owns its
//! backing independently; history is charged for additional retained ownership.
use super::*;
use std::collections::HashSet;

pub(super) const BYTE_BUDGET: usize = 512 * 1024 * 1024;
pub(super) const ENTRY_BUDGET: usize = 256;

#[derive(Default)]
pub(super) struct Accounting {
    roots: HashSet<u64>,
    tiles: HashSet<u64>,
    sources: color::source::SourceAccounting,
    selections: HashSet<usize>,
    meshes: HashSet<usize>,
    mesh_arrays: HashSet<usize>,
    resources: HashSet<usize>,
    profiles: HashSet<usize>,
    programs: HashSet<usize>,
    codes: HashSet<usize>,
    extensions: HashSet<usize>,
    backings: HashSet<u64>,
    opaque_resources: HashSet<usize>,
}
impl Accounting {
    pub fn new(document: &Document) -> Self {
        Self::seed(document,false)
    }
    pub fn for_admission(document: &Document) -> Self {
        Self::seed(document,true)
    }
    fn seed(document:&Document,authored_only:bool)->Self {
        let mut result = Self::default();
        let mut roots = RootInventory {authored_only,..Default::default()};
        roots.document(document);
        for extensions in roots.extensions { result.charge_extensions(extensions); }
        for selection in roots.selections { result.charge_selection(selection); }
        for resource in roots.resources { result.charge_resource(resource); }
        for mesh in roots.meshes { result.charge_mesh(mesh); }
        for source in roots.sources { result.sources.charge(source); }
        for profile in roots.profiles { result.charge_profile(profile); }
        for program in roots.programs { result.seed_program(program); }
        for revision in roots.rasters {
            result.roots.insert(revision.identity());
            if let Some(Ok(data)) = revision.try_data() {
                result.tiles.extend(data.tiles.values().map(|t| t.identity()));
            }
        }
        result
    }

    pub fn charge(&mut self, entry: &HistoryEntry) -> usize {
        let mut bytes = entry.metadata_bytes;
        let mut inventory = RootInventory::default(); entry.edit.roots(&mut inventory);
        for extensions in inventory.extensions { bytes=bytes.saturating_add(self.charge_extensions(extensions)); }
        for resource in inventory.resources { bytes=bytes.saturating_add(self.charge_resource(resource)); }
        for mesh in inventory.meshes { bytes=bytes.saturating_add(self.charge_mesh(mesh)); }
        for selection in inventory.selections { bytes=bytes.saturating_add(self.charge_selection(selection)); }
        for source in inventory.sources { bytes=bytes.saturating_add(self.sources.charge(source)); }
        for profile in inventory.profiles { bytes=bytes.saturating_add(self.charge_profile(profile)); }
        for program in inventory.programs { bytes=bytes.saturating_add(self.charge_program(program)); }
        let mut roots = Vec::new();
        entry.edit.raster_roots(&mut roots);
        for revision in roots {
            if !self.roots.insert(revision.identity()) {
                continue;
            }
            match revision.try_data() {
                Some(Ok(data)) => {
                    bytes = bytes.saturating_add(data.tiles.len().saturating_mul(96));
                    for tile in data.tiles.values() {
                        if self.tiles.insert(tile.identity()) {
                            bytes = bytes.saturating_add(match tile.try_backing() {
                                Some(Ok(blob)) => blob.resident_bytes(),
                                _ => {
                                    raster::TileBlob::max_compressed_len(tile.descriptor())
                                        .unwrap_or(raster::MAX_COMPRESSED_TILE_BYTES)
                                }
                            });
                        }
                    }
                }
                // Pending producers reserve the permitted capture, never zero.
                None => bytes = bytes.saturating_add(revision.pending_bytes()),
                Some(Err(_)) => (),
            }
        }
        bytes
    }

    pub(super) fn charge_extension_metadata(&mut self, extensions:&Arc<authored::Extensions>)->usize {
        if !self.extensions.insert(Arc::as_ptr(extensions) as usize) {return 0;}
        extensions.resources.values().fold(crate::extension_record_metadata(extensions),|bytes,resource|{
            bytes.saturating_add(if self.opaque_resources.insert(Arc::as_ptr(resource) as usize){crate::opaque_resource_metadata(resource)}else{0})
        })
    }
    pub(super) fn charge_extensions(&mut self, extensions:&Arc<authored::Extensions>)->usize {
        extensions.resources.values().fold(self.charge_extension_metadata(extensions),|bytes,resource|{
            bytes.saturating_add(if self.backings.insert(resource.backing.identity()){resource.backing.resident_bytes()}else{0})
        })
    }
    pub(super) fn charge_profile(&mut self, profile: &color::ColorProfile) -> usize {
        if let color::ColorProfile::Icc(bytes)=profile {
            if self.profiles.insert(bytes.as_ptr() as usize) {bytes.len()} else {0}
        } else {0}
    }
    fn seed_program(&mut self, program:&Arc<EffectProgram>) {
        self.programs.insert(Arc::as_ptr(program) as usize);
        self.charge_shader(&program.wgsl);
        for lookup in program.lookups.iter(){self.charge_shader(&lookup.wgsl);}
    }
    fn charge_shader(&mut self,shader:&EffectShader)->usize {
        match shader {
            EffectShader::Code(code)=>if self.codes.insert(code.as_ptr() as usize){code.len()}else{0},
            EffectShader::Linked{sources}=>sources.iter().map(|code|if self.codes.insert(code.as_ptr() as usize){code.len()}else{0}).fold(0usize,usize::saturating_add),
            EffectShader::Modules(modules)=>modules.iter().map(|code|if self.codes.insert(code.as_ptr() as usize){code.len()}else{0}).fold(0usize,usize::saturating_add),
        }
    }
    pub(super) fn charge_program(&mut self, program:&Arc<EffectProgram>)->usize {
        if !self.programs.insert(Arc::as_ptr(program) as usize){return 0;}
        let metadata=std::mem::size_of::<EffectProgram>()+program.id.len()+program.entry.len()
            +json_len(&program.label).saturating_mul(4)+json_len(&program.parameters).saturating_mul(4)
            +json_len(&program.pages).saturating_mul(4)+json_len(&program.constraints).saturating_mul(4)
            +json_len(&program.passes).saturating_mul(4)+json_len(&program.auxiliary).saturating_mul(4);
        let mut bytes=metadata.saturating_add(self.charge_shader(&program.wgsl));
        for lookup in program.lookups.iter(){bytes=bytes.saturating_add(std::mem::size_of::<EffectLookup>()+lookup.entry.len()+lookup.dependencies.iter().map(|key|key.len()).sum::<usize>()).saturating_add(self.charge_shader(&lookup.wgsl));}
        bytes
    }
    pub(super) fn charge_resource(&mut self, resource: &Lut3d) -> usize {
        resource.storage().filter(|bytes| self.resources.insert(bytes.as_ptr() as usize)).map_or(0, |bytes| bytes.len())
    }
    pub(super) fn charge_mesh(&mut self,mesh:&Arc<MeshMap>)->usize {
        let mut bytes=if self.meshes.insert(Arc::as_ptr(mesh) as usize){std::mem::size_of::<MeshMap>()}else{0};
        if self.mesh_arrays.insert(mesh.net.as_ptr() as usize){bytes=bytes.saturating_add(mesh.net.len()*std::mem::size_of::<Point>());}
        for b in &mesh.breakpoints{if self.mesh_arrays.insert(b.as_ptr() as usize){bytes=bytes.saturating_add(b.len()*std::mem::size_of::<f32>());}}
        bytes
    }
    pub(super) fn charge_selection(&mut self, selection: &Selection) -> usize {
        match &selection.shape {
            SelectionShape::Pixels(pixels) => {
                if self.selections.insert(pixels.words().as_ptr() as usize) {
                    pixels.words().len().saturating_mul(4)
                } else { 0 }
            }
            SelectionShape::Contours(paths) => paths.iter().map(|path| {
                if self.selections.insert(path.as_ptr() as usize) {
                    path.len().saturating_mul(std::mem::size_of::<Point>())
                } else { 0 }
            }).fold(0usize, usize::saturating_add),
        }
    }
}

#[cfg(test)]
mod tests;
