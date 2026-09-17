//! Admission for a complete Float32 display pyramid. Query the GPU driver's
//! remaining allowance and applicable process/system headroom. Installed RAM
//! or VRAM is not free GPU memory.
//! This is an admission snapshot, not a reservation against other applications.

fn allowance(headroom: Option<u64>, divisor: u64) -> u64 {
    // Leave room for editable layers, scratch, GTK and other documents. This
    // is an admission ceiling: the cache allocates only the actual document's
    // completed pixels and mip levels, never the whole allowance.
    headroom.map_or(0, |bytes| bytes / divisor)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(super) fn complete_budget(device: &wgpu::Device) -> u64 {
    let headroom = vulkan_headroom(device);
    #[cfg(target_os = "android")]
    {
        // Unified memory also serves the process, compositor and other apps.
        // Use the driver budget when available. An integrated device with a
        // host-visible local heap can otherwise use measured system headroom.
        let system = layer_color::photo::PhotoMemoryBudget::available_memory();
        let headroom = match (headroom, system) {
            (Some(gpu), Some(system)) => Some(gpu.min(system)),
            (None, Some(system)) if unified_memory(device) => Some(system),
            _ => None,
        };
        let bytes = allowance(headroom, 2);
        android_admission_log(headroom, system, bytes);
        bytes
    }
    #[cfg(not(target_os = "android"))]
    allowance(headroom, 4)
}

#[cfg(target_vendor = "apple")]
pub(super) fn complete_budget(device: &wgpu::Device) -> u64 {
    // SAFETY: the guard retains wgpu's live device. These read-only Metal
    // properties neither allocate resources nor submit work.
    let Some(hal) = (unsafe { device.as_hal::<wgpu::hal::api::Metal>() }) else {
        return 0;
    };
    use objc2_metal::MTLDevice;
    let metal = hal.raw_device();
    let headroom = metal_headroom(
        metal.recommendedMaxWorkingSetSize(),
        metal.currentAllocatedSize() as u64,
        layer_color::photo::PhotoMemoryBudget::available_memory(),
    );
    allowance(headroom, 4)
}

#[cfg(target_vendor = "apple")]
fn metal_headroom(recommended: u64, allocated: u64, available: Option<u64>) -> Option<u64> {
    // Metal's recommendation is a performance threshold, not free memory.
    // iPadOS's process allowance also accounts for the app's termination limit.
    available.map(|bytes| bytes.min(recommended.saturating_sub(allocated)))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
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
    #[cfg(target_vendor = "apple")]
    fn metal_admission_respects_existing_allocations_and_process_headroom() {
        assert_eq!(metal_headroom(4096, 1024, Some(8192)), Some(3072));
        assert_eq!(metal_headroom(4096, 1024, Some(512)), Some(512));
        assert_eq!(metal_headroom(4096, 8192, Some(8192)), Some(0));
        assert_eq!(metal_headroom(0, 0, Some(8192)), Some(0));
        assert_eq!(metal_headroom(4096, 0, Some(0)), Some(0));
        assert_eq!(metal_headroom(4096, 0, None), None);
    }
    #[test]
    fn complete_pyramid_admission_scales_with_remaining_headroom() {
        assert_eq!(allowance(None, 4), 0);
        assert_eq!(allowance(Some(1024), 2), 512);
        assert_eq!(allowance(Some(0), 4), 0);
        assert_eq!(allowance(Some(1024 * 1024 * 1024), 4), 256 * 1024 * 1024);
        assert_eq!(
            allowance(Some(96 * 1024 * 1024 * 1024), 4),
            24 * 1024 * 1024 * 1024
        );
    }
}

#[cfg(target_os = "android")]
fn android_admission_log(headroom: Option<u64>, system: Option<u64>, allowance: u64) {
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(
            priority: i32,
            tag: *const std::ffi::c_char,
            text: *const std::ffi::c_char,
        ) -> i32;
    }
    let message = std::ffi::CString::new(format!(
        "display headroom={headroom:?} system={system:?} allowance={allowance}"
    ))
    .unwrap();
    // SAFETY: both strings are NUL-terminated and live through this call.
    unsafe {
        __android_log_write(4, c"CapyDisplay".as_ptr(), message.as_ptr());
    }
}

#[cfg(target_os = "android")]
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
