use super::PresentState;
use ash::vk;

#[test]
fn rejected_present_requires_reconfiguration_and_fence_retirement() {
    for (fault, expected) in [
        (
            vk::Result::ERROR_OUT_OF_DATE_KHR,
            crate::SurfaceError::Outdated,
        ),
        (
            vk::Result::ERROR_SURFACE_LOST_KHR,
            crate::SurfaceError::Lost,
        ),
    ] {
        for previously_presented in [false, true] {
            let mut state = PresentState::default();
            if previously_presented {
                state.complete(Ok(false), None).unwrap();
            }
            let mut pending = false;
            assert_eq!(
                state.complete(Err(fault), Some(&mut pending)),
                Err(expected.clone())
            );
            assert!(pending, "rejected presentation still enqueues its fence");
            assert_eq!(state.initialized, previously_presented);
            // Polling again cannot hide the error, even after every slot in the
            // shared presentation pool would have been visited.
            for _ in 0..8 {
                assert_eq!(state.check(), Err(expected.clone()));
            }
            // Reconfiguration creates a new swapchain with new image history.
            let replacement = PresentState::default();
            assert!(replacement.check().is_ok());
            assert!(!replacement.initialized);
        }
    }
}

#[test]
fn unqueued_present_failure_does_not_wait_for_an_unsignaled_fence() {
    for fault in [
        vk::Result::ERROR_OUT_OF_HOST_MEMORY,
        vk::Result::ERROR_OUT_OF_DEVICE_MEMORY,
        vk::Result::ERROR_DEVICE_LOST,
    ] {
        let mut state = PresentState::default();
        let mut pending = true;
        let error = state.complete(Err(fault), Some(&mut pending)).unwrap_err();
        assert!(matches!(error, crate::SurfaceError::Device(_)));
        assert!(
            !pending,
            "no present fence is guaranteed after this failure"
        );
        assert!(!state.initialized);
        assert_eq!(
            state.check(),
            Err(error),
            "failed presentation cannot reuse its semaphores"
        );
    }
}

#[test]
fn successful_and_suboptimal_presents_keep_the_acquisition_path_ready() {
    let mut state = PresentState::default();
    for suboptimal in [false, true, false] {
        let mut pending = false;
        assert_eq!(
            state.complete(Ok(suboptimal), Some(&mut pending)),
            Ok(suboptimal)
        );
        assert!(pending);
        assert!(state.initialized);
        assert!(state.check().is_ok());
    }
}
