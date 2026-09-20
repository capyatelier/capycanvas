//! Bindings live with the resources that bound their lifetime. Replacing a
//! dependent buffer/view replaces the one retained binding, rather than growing
//! a global cache that can keep retired tiles or stroke state alive.
use std::cell::RefCell;

pub(super) struct CachedBinding<K>(RefCell<Option<Box<(K, wgpu::BindGroup)>>>);
impl<K> Default for CachedBinding<K> {
    fn default() -> Self {
        Self(RefCell::new(None))
    }
}
impl<K> CachedBinding<K> {
    pub fn clear(&self) {
        *self.0.borrow_mut() = None;
    }
}
impl<K: PartialEq> CachedBinding<K> {
    pub fn get(&self, key: K, create: impl FnOnce() -> wgpu::BindGroup) -> wgpu::BindGroup {
        let mut entry = self.0.borrow_mut();
        if let Some((previous, binding)) = entry.as_deref()
            && *previous == key
        {
            return binding.clone();
        }
        let binding = create();
        *entry = Some(Box::new((key, binding.clone())));
        binding
    }
}

pub(super) type MaterialInput = CachedBinding<([wgpu::Buffer; 2], [wgpu::TextureView; 2])>;
pub(super) type MaterialOutput =
    CachedBinding<(wgpu::Buffer, wgpu::TextureView, Option<wgpu::TextureView>)>;
