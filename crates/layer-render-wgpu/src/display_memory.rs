#[cfg(not(target_arch = "wasm32"))]
use layer_render::{GpuMemory, GpuMemoryKind};

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn usage(device: &wgpu::Device) -> Option<GpuMemory> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    { vulkan_memory(device).map(VulkanHeap::usage) }
    #[cfg(target_os = "windows")]
    { dx12_memory(device).or_else(|| vulkan_memory(device).map(VulkanHeap::usage)) }
    #[cfg(target_vendor = "apple")]
    { metal_memory(device) }
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "windows", target_vendor = "apple")))]
    { let _ = device; None }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn facts(device: &wgpu::Device, usage: Option<GpuMemory>) -> layer_core::DeviceMemoryFacts {
    let mut facts = layer_core::DeviceMemoryFacts { budget_bytes: usage.and_then(|sample| sample.budget),
        usage_bytes: usage.map(|sample| sample.bytes), ..Default::default() };
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
    if let Some((heap, unified)) = vulkan_topology(device) {
        facts.heap_bytes = heap;
        facts.unified = unified;
    }
    #[cfg(target_os = "windows")]
    if let Some((heap, unified)) = dx12_topology(device) {
        facts.heap_bytes = heap;
        facts.unified = unified;
    }
    #[cfg(target_vendor = "apple")]
    if let Some(metal) = unsafe { device.as_hal::<wgpu::hal::api::Metal>() } {
        use objc2_metal::MTLDevice;
        facts.unified = metal.raw_device().hasUnifiedMemory();
    }
    let _ = device;
    facts
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
fn vulkan_topology(device: &wgpu::Device) -> Option<(Option<u64>, bool)> {
    use ash::vk;
    let hal = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }?;
    let instance = hal.shared_instance().raw_instance();
    let (properties, memory) = unsafe { (instance.get_physical_device_properties(hal.raw_physical_device()),
        instance.get_physical_device_memory_properties(hal.raw_physical_device())) };
    let heap = memory.memory_heaps[..memory.memory_heap_count as usize].iter()
        .filter(|heap| heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL)).map(|heap| heap.size).max();
    let unified = properties.device_type == vk::PhysicalDeviceType::INTEGRATED_GPU
        && memory.memory_types[..memory.memory_type_count as usize].iter().any(|kind|
            kind.property_flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL | vk::MemoryPropertyFlags::HOST_VISIBLE));
    Some((heap, unified))
}

#[cfg(target_os = "windows")]
fn dx12_topology(device: &wgpu::Device) -> Option<(Option<u64>, bool)> {
    use windows::Win32::Graphics::{Direct3D12::{D3D12_FEATURE_ARCHITECTURE1, D3D12_FEATURE_DATA_ARCHITECTURE1},
        Dxgi::{CreateDXGIFactory1, IDXGIFactory4, IDXGIAdapter3}};
    let hal = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }?;
    let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory1() }.ok()?;
    let adapter: IDXGIAdapter3 = unsafe { factory.EnumAdapterByLuid(hal.raw_device().GetAdapterLuid()) }.ok()?;
    let description = unsafe { adapter.GetDesc1() }.ok()?;
    let mut architecture = D3D12_FEATURE_DATA_ARCHITECTURE1::default();
    let unified = unsafe { hal.raw_device().CheckFeatureSupport(D3D12_FEATURE_ARCHITECTURE1,
        (&raw mut architecture).cast(), std::mem::size_of_val(&architecture) as u32) }.ok().map(|()| architecture.UMA.as_bool())?;
    Some(((description.DedicatedVideoMemory != 0).then_some(description.DedicatedVideoMemory as u64), unified))
}

#[cfg(target_vendor = "apple")]
fn metal_memory(device: &wgpu::Device) -> Option<GpuMemory> {
    // SAFETY: the guard retains wgpu's live device. These read-only Metal
    // properties neither allocate resources nor submit work.
    let hal = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }?;
    use objc2_metal::MTLDevice;
    let metal = hal.raw_device();
    Some(GpuMemory { kind: GpuMemoryKind::MetalAllocated, bytes: metal.currentAllocatedSize() as u64,
        budget: std::num::NonZeroU64::new(metal.recommendedMaxWorkingSetSize()).map(std::num::NonZeroU64::get) })
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
#[derive(Clone, Copy)]
struct VulkanHeap { bytes: u64, budget: u64, size: u64 }

#[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
impl VulkanHeap {
    fn available(&self) -> u64 { self.budget.min(self.size).saturating_sub(self.bytes) }
    fn usage(self) -> GpuMemory { GpuMemory { kind: GpuMemoryKind::VulkanHeap, bytes: self.bytes, budget: Some(self.budget) } }
    fn select(heaps: impl Iterator<Item = Self>) -> Option<Self> { heaps.max_by_key(Self::available) }
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
fn vulkan_memory(device: &wgpu::Device) -> Option<VulkanHeap> {
    use ash::vk;
    // SAFETY: the guard keeps wgpu's device/instance alive. We only query
    // immutable physical-device properties and never mutate/destroy its objects.
    let hal = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }?;
    if !hal
        .enabled_device_extensions()
        .contains(&ash::ext::memory_budget::NAME)
        || hal.shared_instance().instance_api_version() < vk::API_VERSION_1_1
    {
        return None;
    }
    let mut budget = vk::PhysicalDeviceMemoryBudgetPropertiesEXT::default();
    let mut properties = vk::PhysicalDeviceMemoryProperties2::default().push_next(&mut budget);
    // SAFETY: Vulkan 1.1 and VK_EXT_memory_budget are present. Both output
    // structures live through the call and the physical device belongs to this
    // live instance. The query does not submit work or change allocator state.
    unsafe {
        hal.shared_instance()
            .raw_instance()
            .get_physical_device_memory_properties2(hal.raw_physical_device(), &mut properties);
    }
    let memory = properties.memory_properties;
    // Do not sum unrelated heaps: a texture must fit one device-local heap.
    VulkanHeap::select(memory.memory_heaps[..memory.memory_heap_count as usize]
        .iter()
        .enumerate()
        .filter(|(_, heap)| heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
        .map(|(i, heap)| VulkanHeap { bytes: budget.heap_usage[i], budget: budget.heap_budget[i], size: heap.size }))
}



#[cfg(target_os = "windows")]
fn dx12_memory(device: &wgpu::Device) -> Option<GpuMemory> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory4,
        IDXGIAdapter3, DXGI_MEMORY_SEGMENT_GROUP_LOCAL};
    // The guard retains the renderer's live device. Query the matching adapter,
    // not an arbitrary monitor or the amount of installed/shared video memory.
    let hal = unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }?;
    let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory1() }.ok()?;
    let adapter: IDXGIAdapter3 = unsafe {
        factory.EnumAdapterByLuid(hal.raw_device().GetAdapterLuid())
    }.ok()?;
    let mut memory = Default::default();
    unsafe { adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut memory) }.ok()?;
    Some(GpuMemory { kind: GpuMemoryKind::DxgiLocal, bytes: memory.CurrentUsage, budget: Some(memory.Budget) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
    fn vulkan_usage_preserves_raw_values_while_heap_selection_clamps_size() {
        let heap = VulkanHeap { bytes: 1024, budget: 8192, size: 4096 };
        assert_eq!(heap.available(), 3072);
        assert_eq!(heap.usage(), GpuMemory { kind: GpuMemoryKind::VulkanHeap, bytes: 1024, budget: Some(8192) });
        let over = VulkanHeap { bytes: 8192, ..heap };
        assert_eq!(over.available(), 0);
        assert_eq!(over.usage().bytes, 8192);
    }
    #[test]
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
    fn vulkan_usage_selects_the_local_heap_with_most_available_bytes() {
        let heaps = [VulkanHeap { bytes: 0, budget: 8192, size: 4 }, VulkanHeap { bytes: 1, budget: 8, size: 8 }];
        let selected = VulkanHeap::select(heaps.into_iter()).unwrap();
        assert_eq!(selected.available(), 7);
        assert_eq!(selected.usage(), GpuMemory { kind: GpuMemoryKind::VulkanHeap, bytes: 1, budget: Some(8) });
        assert!(VulkanHeap::select(std::iter::empty()).is_none());
    }
}
