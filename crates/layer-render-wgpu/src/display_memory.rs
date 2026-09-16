//! Linux admission for a complete Float32 display pyramid. Query the Vulkan
//! driver's current process budget; installed VRAM is not free GPU memory.
//! This is an admission snapshot, not a reservation against other applications.

fn allowance(headroom: Option<u64>) -> u64 {
    // Leave room for editable layers, scratch, GTK and other documents. This
    // is an admission ceiling: the cache allocates only the actual document's
    // completed pixels and mip levels, never the whole allowance.
    headroom.map_or(0, |bytes| bytes / 4)
}

pub(super) fn complete_budget(device: &wgpu::Device) -> u64 {
    allowance(vulkan_headroom(device))
}

fn vulkan_headroom(device: &wgpu::Device) -> Option<u64> {
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
    memory.memory_heaps[..memory.memory_heap_count as usize]
        .iter()
        .enumerate()
        .filter(|(_, heap)| heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
        .map(|(i, heap)| {
            budget.heap_budget[i]
                .min(heap.size)
                .saturating_sub(budget.heap_usage[i])
        })
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_pyramid_admission_scales_with_remaining_headroom() {
        assert_eq!(allowance(None), 0);
        assert_eq!(allowance(Some(0)), 0);
        assert_eq!(allowance(Some(1024 * 1024 * 1024)), 256 * 1024 * 1024);
        assert_eq!(
            allowance(Some(96 * 1024 * 1024 * 1024)),
            24 * 1024 * 1024 * 1024
        );
    }
}
