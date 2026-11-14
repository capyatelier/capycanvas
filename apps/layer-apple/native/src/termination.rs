use std::{io::Write, sync::mpsc, time::{Duration, Instant}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Finish { Complete, Incomplete }

fn finish_bounded(timeout: Duration, work: impl FnOnce() -> Result<(), String> + Send + 'static) -> Finish {
    let start = Instant::now();
    let (send, receive) = mpsc::channel();
    if std::thread::Builder::new().name("process-gpu-finish".into()).spawn(move || {
        let _ = send.send(work());
    }).is_err() { return Finish::Incomplete; }
    match receive.recv_timeout(timeout.saturating_sub(start.elapsed())) {
        Ok(Ok(())) if start.elapsed() <= timeout => Finish::Complete,
        _ => Finish::Incomplete,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn capy_apple_finish_process_gpu(timeout_ms: u64) -> bool {
    finish_bounded(Duration::from_millis(timeout_ms.min(2000)), layer_host::gpu::finish_process_gpu) == Finish::Complete
}

#[unsafe(no_mangle)]
pub extern "C" fn capy_apple_termination_requires_exit(storage_ready: bool, gpu_complete: bool) -> bool {
    storage_ready && !gpu_complete
}

#[unsafe(no_mangle)]
pub extern "C" fn capy_apple_termination_incomplete() {
    eprintln!("GPU termination incomplete");
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, thread, time::Instant};

    #[test]
    fn blocked_recipe_join_returns_incomplete_within_two_seconds() {
        let (release, resume) = mpsc::channel();
        let recipe = thread::spawn(move || resume.recv().unwrap());
        let cleanup = thread::spawn(move || {
            thread::sleep(Duration::from_millis(2100));
            release.send(()).unwrap();
        });
        let (joined, done) = mpsc::channel();
        let start = Instant::now();
        let result = finish_bounded(Duration::from_secs(2), move || {
            let result = recipe.join().map_err(|_| "recipe failed".to_owned());
            let _ = joined.send(());
            result
        });
        let elapsed = start.elapsed();
        cleanup.join().unwrap();
        done.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(result, Finish::Incomplete);
        assert!(elapsed <= Duration::from_millis(2050), "{elapsed:?}");
        assert!(capy_apple_termination_requires_exit(true, result == Finish::Complete));
    }

    #[test]
    fn joined_services_use_normal_termination() {
        let result = finish_bounded(Duration::from_secs(2), || Ok(()));
        assert_eq!(result, Finish::Complete);
        assert!(!capy_apple_termination_requires_exit(true, result == Finish::Complete));
    }

    #[test]
    fn failed_finish_never_claims_complete_or_exits_before_storage() {
        let result = finish_bounded(Duration::from_secs(2), || Err("driver failed".into()));
        assert_eq!(result, Finish::Incomplete);
        assert!(!capy_apple_termination_requires_exit(false, result == Finish::Complete));
        assert!(!capy_apple_termination_requires_exit(false, true));
    }
}
