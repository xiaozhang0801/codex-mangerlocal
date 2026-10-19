use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

fn test_runtime() -> std::io::Result<Runtime> {
    Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
}

#[test]
fn concurrent_cold_initialization_from_async_context_builds_one_shared_runtime() {
    const WAITERS: usize = 3;
    let slot = StdMutex::new(None);
    let builds = AtomicUsize::new(0);
    let (building, build_started) = mpsc::channel();
    let (release, build_released) = mpsc::channel();
    let (attempted, attempts) = mpsc::channel();
    let timeout = Duration::from_secs(10);

    // A local cold slot makes this independent of whether another test has
    // already initialized the process-wide SQLite runtime. Hold its first
    // builder until every other async caller has reached the initialization
    // boundary; no sleeps or repeated attempts are needed.
    thread::scope(|scope| {
        let first_slot = &slot;
        let first_builds = &builds;
        let first = scope.spawn(move || {
            let caller = Builder::new_current_thread().build().unwrap();
            caller.block_on(async {
                initialize_sqlite_runtime(first_slot, || {
                    first_builds.fetch_add(1, Ordering::SeqCst);
                    building.send(()).unwrap();
                    build_released.recv_timeout(timeout).unwrap();
                    test_runtime()
                })
                .unwrap()
            })
        });
        build_started.recv_timeout(timeout).unwrap();
        assert!(
            slot.try_lock().is_err(),
            "runtime construction must hold the initialization lock"
        );
        let mut waiters = Vec::new();
        for _ in 0..WAITERS {
            let attempted = attempted.clone();
            let slot = &slot;
            let builds = &builds;
            waiters.push(scope.spawn(move || {
                let caller = Builder::new_current_thread().build().unwrap();
                caller.block_on(async {
                    attempted.send(()).unwrap();
                    initialize_sqlite_runtime(slot, || {
                        builds.fetch_add(1, Ordering::SeqCst);
                        test_runtime()
                    })
                    .unwrap()
                })
            }));
        }
        for _ in 0..WAITERS {
            attempts.recv_timeout(timeout).unwrap();
        }
        release.send(()).unwrap();
        let runtime = first
            .join()
            .expect("first async initializer must not panic");
        for waiter in waiters {
            let shared = waiter.join().expect("async initializer must not panic");
            assert!(Arc::ptr_eq(&runtime, &shared));
        }
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        assert!(Arc::ptr_eq(
            &runtime,
            slot.lock().unwrap().as_ref().unwrap()
        ));
    });
    // The local slot's final runtime is dropped here, outside every caller's
    // async context, just as the static production slot retains it for life.
}

#[test]
fn failed_initialization_returns_error_and_leaves_slot_available_for_later_call() {
    let slot = StdMutex::new(None);
    let error = initialize_sqlite_runtime(&slot, || {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "forced runtime initialization failure",
        ))
    })
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("forced runtime initialization failure"));
    assert!(slot.lock().unwrap().is_none());

    let initialized = initialize_sqlite_runtime(&slot, test_runtime).unwrap();
    let shared = initialize_sqlite_runtime(&slot, || {
        panic!("a successful runtime must be reused without another build")
    })
    .unwrap();
    assert!(Arc::ptr_eq(&initialized, &shared));
}
