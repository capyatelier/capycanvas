//! Admission for a complete Float32 display pyramid. Query the GPU driver's
//! remaining allowance and applicable process/system headroom. Installed RAM
//! or VRAM is not free GPU memory.
//! This is an admission snapshot, not a reservation against other applications.

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

fn allowance(headroom: Option<u64>, divisor: u64, retained: u64) -> u64 {
    // Leave room for editable layers, scratch, GTK and other documents. This
    // is an admission ceiling: the cache allocates only the actual document's
    // completed pixels and mip levels, never the whole allowance.
    headroom.map_or(0, |bytes| bytes.saturating_add(retained) / divisor)
}

fn resource_allowance(gpu: Option<u64>, system: Option<u64>, retained: u64) -> u64 {
    let headroom = match (gpu, system) {
        (Some(gpu), Some(system)) => Some(gpu.min(system)),
        (gpu, system) => gpu.or(system),
    };
    allowance(headroom, 4, retained).min(512 * 1024 * 1024)
}

/// Exact filter dependencies can use a quarter of available unified memory, or
/// of a discrete GPU's local heap, when the driver budget is unavailable. This
/// must not enlarge optional display/source caches: capacity alone does not
/// establish their fast-residency budget.
pub(super) fn composition_budget(device: &wgpu::Device, display: u64) -> u64 {
    let budget = display.saturating_add(crate::scene::windows::DEFAULT_IMAGE_PIXEL_BYTES);
    // wgpu disables RADV's unreliable memory-budget extension. Respect that
    // workaround; size only dependencies from memory the device reports.
    #[cfg(target_os = "linux")]
    if display == 0 && unified_memory(device) {
        return budget.max(allowance(layer_color::photo::PhotoMemoryBudget::available_memory(), 4, 0));
    }
    #[cfg(target_os = "linux")]
    if display == 0 && device_headroom(device).is_none() {
        return budget.max(allowance(local_heap(device), 4, 0));
    }
    #[cfg(not(target_os = "linux"))]
    let _ = device;
    budget
}

/// Device memory the driver still offers this process, when it reports one.
#[cfg(target_os = "linux")]
pub(crate) fn device_headroom(device: &wgpu::Device) -> Option<u64> {
    vulkan_headroom(device)
}

#[cfg(target_os = "android")]
pub(crate) fn device_headroom(device: &wgpu::Device) -> Option<u64> {
    // Unified memory also serves the process, compositor and other apps.
    // Use the driver budget when available. An integrated device with a
    // host-visible local heap can otherwise use measured system headroom.
    let system = layer_color::photo::PhotoMemoryBudget::available_memory();
    match (vulkan_headroom(device), system) {
        (Some(gpu), Some(system)) => Some(gpu.min(system)),
        (None, Some(system)) if unified_memory(device) => Some(system),
        _ => None,
    }
}

#[cfg(target_vendor = "apple")]
pub(crate) fn device_headroom(device: &wgpu::Device) -> Option<u64> {
    let memory = metal_memory(device)?;
    metal_headroom(memory.budget.unwrap_or(0), memory.bytes,
        layer_color::photo::PhotoMemoryBudget::available_memory())
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

#[cfg(target_os = "windows")]
pub(crate) fn device_headroom(device: &wgpu::Device) -> Option<u64> {
    dx12_headroom(device).or_else(|| {
        remaining_headroom(vulkan_headroom(device)?, 0,
            layer_color::photo::PhotoMemoryBudget::available_memory())
    })
}

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple", target_os = "windows"))]
pub(super) fn complete_budget(device: &wgpu::Device, retained: u64) -> u64 {
    allowance(device_headroom(device), if cfg!(target_os = "android") { 2 } else { 4 }, retained)
}

#[cfg(target_vendor = "apple")]
fn metal_headroom(recommended: u64, allocated: u64, available: Option<u64>) -> Option<u64> {
    if recommended == 0 { return None; }
    // Metal's recommendation is a performance threshold, not free memory.
    // iPadOS's process allowance also accounts for the app's termination limit.
    available.map(|bytes| bytes.min(recommended.saturating_sub(allocated)))
}

#[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
fn vulkan_headroom(device: &wgpu::Device) -> Option<u64> {
    vulkan_memory(device).map(|heap| heap.available())
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



/// The largest device-local heap, which Vulkan reports without a budget.
#[cfg(target_os = "linux")]
fn local_heap(device: &wgpu::Device) -> Option<u64> {
    use ash::vk;
    // SAFETY: the guard owns the live instance; this query only reads properties.
    let hal = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }?;
    let memory = unsafe { hal.shared_instance().raw_instance().get_physical_device_memory_properties(hal.raw_physical_device()) };
    memory.memory_heaps[..memory.memory_heap_count as usize].iter()
        .filter(|heap| heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL)).map(|heap| heap.size).max()
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn unified_memory(device: &wgpu::Device) -> bool {
    use ash::vk;
    // SAFETY: the guard owns the live instance; these queries only read properties.
    let Some(hal) = (unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }) else {
        return false;
    };
    let instance = hal.shared_instance().raw_instance();
    let (properties, memory) = unsafe {
        (
            instance.get_physical_device_properties(hal.raw_physical_device()),
            instance.get_physical_device_memory_properties(hal.raw_physical_device()),
        )
    };
    properties.device_type == vk::PhysicalDeviceType::INTEGRATED_GPU
        && memory.memory_types[..memory.memory_type_count as usize]
            .iter()
            .any(|ty| {
                ty.property_flags.contains(
                    vk::MemoryPropertyFlags::DEVICE_LOCAL | vk::MemoryPropertyFlags::HOST_VISIBLE,
                )
            })
}

#[cfg(target_os = "windows")]
fn dx12_headroom(device: &wgpu::Device) -> Option<u64> {
    let memory = dx12_memory(device)?;
    remaining_headroom(memory.budget?, memory.bytes,
        layer_color::photo::PhotoMemoryBudget::available_memory())
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

#[cfg(target_os = "windows")]
fn remaining_headroom(budget: u64, usage: u64, system: Option<u64>) -> Option<u64> {
    system.map(|available| budget.saturating_sub(usage).min(available))
}

#[cfg(all(test, target_os = "windows"))]
#[test]
fn windows_admission_respects_usage_and_available_memory() {
    assert_eq!(remaining_headroom(4096, 1024, Some(8192)), Some(3072));
    assert_eq!(remaining_headroom(4096, 1024, Some(512)), Some(512));
    assert_eq!(remaining_headroom(4096, 8192, Some(8192)), Some(0));
    assert_eq!(remaining_headroom(4096, 0, Some(0)), Some(0));
    assert_eq!(remaining_headroom(4096, 0, None), None);
    assert_eq!(remaining_headroom(0, 0, Some(8192)), Some(0));
}

pub(super) fn resource_budget(device: &wgpu::Device, retained: u64) -> u64 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let gpu = vulkan_headroom(device);
    #[cfg(any(target_vendor = "apple", target_os = "windows"))]
    let gpu = device_headroom(device);
    resource_allowance(gpu, layer_color::photo::PhotoMemoryBudget::available_memory(), retained)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
    fn vulkan_usage_preserves_raw_values_while_admission_clamps_the_heap_size() {
        let heap = VulkanHeap { bytes: 1024, budget: 8192, size: 4096 };
        assert_eq!(heap.available(), 3072);
        assert_eq!(heap.usage(), GpuMemory { kind: GpuMemoryKind::VulkanHeap, bytes: 1024, budget: Some(8192) });
        let over = VulkanHeap { bytes: 8192, ..heap };
        assert_eq!(over.available(), 0);
        assert_eq!(over.usage().bytes, 8192);
    }
    #[test]
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "windows"))]
    fn vulkan_usage_selects_the_same_local_heap_as_admission() {
        let heaps = [VulkanHeap { bytes: 0, budget: 8192, size: 4 }, VulkanHeap { bytes: 1, budget: 8, size: 8 }];
        let selected = VulkanHeap::select(heaps.into_iter()).unwrap();
        assert_eq!(selected.available(), 7);
        assert_eq!(selected.usage(), GpuMemory { kind: GpuMemoryKind::VulkanHeap, bytes: 1, budget: Some(8) });
        assert!(VulkanHeap::select(std::iter::empty()).is_none());
    }
    #[test]
    #[cfg(target_os = "linux")]
    fn the_local_heap_is_reported_without_a_memory_budget() {
        let r = crate::WgpuRasterizer::new_native_headless(Default::default()).unwrap();
        assert!(local_heap(&r.device).is_some_and(|bytes| bytes >= 256 << 20));
    }
    #[test]
    fn essential_resource_admission_preserves_reported_limits_and_optional_cache_policy() {
        assert_eq!(resource_allowance(None, Some(4096), 0), 1024);
        assert_eq!(resource_allowance(Some(8192), Some(4096), 0), 1024);
        assert_eq!(resource_allowance(Some(1024), Some(4096), 0), 256);
        assert_eq!(resource_allowance(Some(0), Some(4096), 0), 0);
        assert_eq!(resource_allowance(Some(4096), Some(0), 0), 0);
        assert_eq!(resource_allowance(None, Some(0), 0), 0);
        assert_eq!(resource_allowance(None, None, 0), 0);
        assert_eq!(resource_allowance(Some(4096), None, 0), 1024);
        assert_eq!(resource_allowance(None, Some(4096), 128), 1056);
        assert_eq!(resource_allowance(None, Some(u64::MAX), 0), 512 * 1024 * 1024);
        assert_eq!(allowance(None, 4, 0), 0);
    }
    #[test]
    #[cfg(target_vendor = "apple")]
    fn metal_admission_respects_existing_allocations_and_process_headroom() {
        assert_eq!(metal_headroom(4096, 1024, Some(8192)), Some(3072));
        assert_eq!(metal_headroom(4096, 1024, Some(512)), Some(512));
        assert_eq!(metal_headroom(4096, 8192, Some(8192)), Some(0));
        assert_eq!(metal_headroom(4096, 0, Some(0)), Some(0));
        assert_eq!(metal_headroom(4096, 0, None), None);
    }
    #[test]
    #[cfg(target_vendor = "apple")]
    fn unreported_metal_recommendation_is_not_an_exhausted_budget() {
        assert_eq!(metal_headroom(0, 0, Some(8192)), None);
        assert_eq!(allowance(metal_headroom(0, 0, Some(8192)), 4, 0), 0);
    }
    #[test]
    fn retaining_a_pyramid_preserves_its_admission_budget() {
        for divisor in [2, 4] {
            let initial = allowance(Some(4096), divisor, 0);
            let retained = initial - 128;
            assert_eq!(allowance(Some(4096 - retained), divisor, retained), initial);
            assert!(allowance(Some(0), divisor, retained) < retained);
            assert_eq!(allowance(None, divisor, retained), 0);
        }
    }
    #[test]
    fn complete_pyramid_admission_scales_with_remaining_headroom() {
        assert_eq!(allowance(None, 4, 0), 0);
        assert_eq!(allowance(Some(1024), 2, 0), 512);
        assert_eq!(allowance(Some(0), 4, 0), 0);
        assert_eq!(allowance(Some(1024 * 1024 * 1024), 4, 0), 256 * 1024 * 1024);
        assert_eq!(
            allowance(Some(96 * 1024 * 1024 * 1024), 4, 0),
            24 * 1024 * 1024 * 1024
        );
    }
}
