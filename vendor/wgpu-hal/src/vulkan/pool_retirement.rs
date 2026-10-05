use alloc::sync::Arc;
use std::sync::{mpsc, OnceLock};

use ash::vk;
use parking_lot::{Condvar, Mutex};

#[derive(Default)]
pub(super) struct PendingPools {
    pending: Mutex<usize>,
    drained: Condvar,
}

impl PendingPools {
    fn register(&self) {
        *self.pending.lock() += 1;
    }

    fn complete(&self) {
        let mut pending = self.pending.lock();
        *pending -= 1;
        if *pending == 0 {
            self.drained.notify_all();
        }
    }

    pub(super) fn drain(&self) {
        let mut pending = self.pending.lock();
        while *pending != 0 {
            self.drained.wait(&mut pending);
        }
    }
}

struct RetiredPool {
    raw: vk::CommandPool,
    device: ash::Device,
    pending: Arc<PendingPools>,
}

impl RetiredPool {
    fn new(device: &super::DeviceShared, raw: vk::CommandPool) -> Self {
        device.retired_pools.register();
        Self {
            raw,
            device: device.raw.clone(),
            pending: device.retired_pools.clone(),
        }
    }

    fn enqueue(self, sender: Option<&mpsc::SyncSender<Self>>) {
        if let Some(sender) = sender {
            if let Err(mpsc::TrySendError::Full(pool) | mpsc::TrySendError::Disconnected(pool)) =
                sender.try_send(self)
            {
                drop(pool);
            }
        }
    }
}

impl Drop for RetiredPool {
    fn drop(&mut self) {
        unsafe { self.device.destroy_command_pool(self.raw, None) };
        self.pending.complete();
    }
}

fn worker() -> Option<&'static mpsc::SyncSender<RetiredPool>> {
    static WORKER: OnceLock<Option<mpsc::SyncSender<RetiredPool>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (sender, receiver) = mpsc::sync_channel::<RetiredPool>(1);
            std::thread::Builder::new()
                .name("wgpu-pool-retire".into())
                .spawn(move || {
                    for pool in receiver {
                        drop(pool);
                    }
                })
                .ok()
                .map(|_| sender)
        })
        .as_ref()
}

pub(super) fn initialize() {
    let _ = worker();
}

pub(super) fn retire(device: &super::DeviceShared, raw: vk::CommandPool) {
    RetiredPool::new(device, raw).enqueue(worker());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Adapter as _, Instance as _};

    fn device() -> Arc<super::super::DeviceShared> {
        let instance = unsafe {
            super::super::Instance::init(&crate::InstanceDescriptor {
                name: "pool retirement test",
                flags: wgt::InstanceFlags::VALIDATION,
                memory_budget_thresholds: Default::default(),
                backend_options: Default::default(),
                telemetry: None,
                display: None,
            })
        }
        .unwrap();
        let adapter = unsafe { instance.enumerate_adapters(None) }
            .into_iter()
            .find(|adapter| adapter.info.device_type != wgt::DeviceType::Cpu)
            .unwrap();
        let opened = unsafe {
            adapter.adapter.open(
                wgt::Features::empty(),
                &wgt::Limits::downlevel_defaults(),
                &wgt::MemoryHints::MemoryUsage,
            )
        }
        .unwrap();
        opened.device.shared.clone()
    }

    fn pool(device: &Arc<super::super::DeviceShared>) -> RetiredPool {
        let raw = unsafe {
            device.raw.create_command_pool(
                &vk::CommandPoolCreateInfo::default().queue_family_index(device.family_index),
                None,
            )
        }
        .unwrap();
        RetiredPool::new(device, raw)
    }

    #[test]
    fn bounded_jobs_balance_fallbacks_and_drain_before_final_device_destruction() {
        let device = device();
        let lease = Arc::downgrade(&device);
        let pending = device.retired_pools.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let active = pool(&device);
        pool(&device).enqueue(Some(&sender));
        assert_eq!(*pending.pending.lock(), 2);
        pool(&device).enqueue(Some(&sender));
        assert_eq!(*pending.pending.lock(), 2);
        drop(receiver);
        assert_eq!(*pending.pending.lock(), 1);
        pool(&device).enqueue(Some(&sender));
        pool(&device).enqueue(None);
        assert_eq!(*pending.pending.lock(), 1);
        let (done, completion) = mpsc::channel();
        let owner = std::thread::spawn(move || {
            drop(device);
            done.send(()).unwrap();
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while lease.strong_count() != 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(lease.strong_count(), 0);
        assert!(matches!(
            completion.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert_eq!(*pending.pending.lock(), 1);
        drop(active);
        completion
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        owner.join().unwrap();
        assert_eq!(*pending.pending.lock(), 0);
        assert_eq!(lease.strong_count(), 0);
    }

    #[test]
    fn worker_reclaims_pools_after_device_owner_drops() {
        initialize();
        assert!(worker().is_some());
        let device = device();
        let lease = Arc::downgrade(&device);
        let pending = device.retired_pools.clone();
        for _ in 0..128 {
            let raw = unsafe {
                device.raw.create_command_pool(
                    &vk::CommandPoolCreateInfo::default().queue_family_index(device.family_index),
                    None,
                )
            }
            .unwrap();
            retire(&device, raw);
        }
        assert_eq!(lease.strong_count(), 1);
        drop(device);
        assert_eq!(*pending.pending.lock(), 0);
        assert_eq!(lease.strong_count(), 0);
    }
}
