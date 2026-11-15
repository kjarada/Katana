mod task_pool {
    use katana_core::task_pool::TaskPool;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    #[test]
    fn visits_every_index_exactly_once() {
        for workers in [0, 1, 5] {
            let pool = TaskPool::new(workers);
            let count = 10_000;
            let visits: Vec<_> = (0..count).map(|_| AtomicUsize::new(0)).collect();
            pool.parallel_for(0, count, |index| {
                visits[index].fetch_add(1, Ordering::SeqCst);
            });
            for (index, visited) in visits.iter().enumerate() {
                assert_eq!(
                    visited.load(Ordering::SeqCst),
                    1,
                    "index {index} with {workers} workers"
                );
            }
        }
    }

    #[test]
    fn ranges_cover_the_whole_interval_without_overlap() {
        let pool = TaskPool::new(4);
        let count = 5000;
        let visits: Vec<_> = (0..count).map(|_| AtomicUsize::new(0)).collect();
        let calls = AtomicUsize::new(0);
        pool.parallel_ranges(0, count, 64, |low, high| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert!(low < high);
            assert!(high <= count);
            for visited in &visits[low..high] {
                visited.fetch_add(1, Ordering::SeqCst);
            }
        });
        assert_eq!(
            visits
                .iter()
                .map(|visited| visited.load(Ordering::SeqCst))
                .sum::<usize>(),
            count
        );
        for (index, visited) in visits.iter().enumerate() {
            assert_eq!(visited.load(Ordering::SeqCst), 1, "index {index}");
        }
        assert!(calls.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn an_empty_or_inverted_range_does_nothing() {
        let pool = TaskPool::new(2);
        let calls = AtomicUsize::new(0);
        pool.parallel_for(5, 5, |_| {
            calls.fetch_add(1, Ordering::SeqCst);
        });
        pool.parallel_for(9, 3, |_| {
            calls.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_grain_of_zero_is_treated_as_one_rather_than_looping_forever() {
        let pool = TaskPool::new(2);
        let total = AtomicUsize::new(0);
        pool.parallel_ranges(0, 100, 0, |low, high| {
            total.fetch_add(high - low, Ordering::SeqCst);
        });
        assert_eq!(total.load(Ordering::SeqCst), 100);
    }

    #[test]
    fn results_are_identical_whatever_the_worker_count() {
        let count = 4096;
        let mut reference = Vec::new();
        for workers in [0, 1, 3, 8] {
            let pool = TaskPool::new(workers);
            let out: Vec<_> = (0..count)
                .map(|_| AtomicU64::new(0.0f64.to_bits()))
                .collect();
            pool.parallel_ranges(0, count, 33, |low, high| {
                for (index, value) in out.iter().enumerate().take(high).skip(low) {
                    value.store(
                        ((index as f64 * 0.001).sin() * 1.0e6).to_bits(),
                        Ordering::SeqCst,
                    );
                }
            });
            let out: Vec<f64> = out
                .iter()
                .map(|value| f64::from_bits(value.load(Ordering::SeqCst)))
                .collect();
            if reference.is_empty() {
                reference = out;
            } else {
                assert_eq!(out, reference, "{workers} workers");
            }
        }
    }

    #[test]
    fn an_exception_stops_the_thread_that_threw_at_once() {
        let pool = TaskPool::new(0);
        let ran = AtomicUsize::new(0);
        let failure = catch_unwind(AssertUnwindSafe(|| {
            pool.parallel_for(0, 100_000, |index| {
                ran.fetch_add(1, Ordering::SeqCst);
                if index == 10 {
                    panic!("boom");
                }
            });
        }));
        assert!(failure.is_err());
        assert_eq!(ran.load(Ordering::SeqCst), 11);
    }

    #[test]
    fn an_exception_in_a_body_reaches_the_caller_and_stops_the_rest() {
        let pool = TaskPool::new(4);
        let ran = AtomicUsize::new(0);
        let thrown = AtomicBool::new(false);
        let failure = catch_unwind(AssertUnwindSafe(|| {
            pool.parallel_for(0, 100_000, |index| {
                ran.fetch_add(1, Ordering::SeqCst);
                if index == 0 {
                    thrown.store(true, Ordering::SeqCst);
                    panic!("boom");
                }
                while !thrown.load(Ordering::SeqCst) {
                    std::thread::yield_now();
                }
                let until = Instant::now() + Duration::from_micros(20);
                while Instant::now() < until {
                    std::hint::spin_loop();
                }
            });
        }));
        assert!(failure.is_err());
        assert!(ran.load(Ordering::SeqCst) < 100_000);
        let after = AtomicUsize::new(0);
        pool.parallel_for(0, 1000, |_| {
            after.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(after.load(Ordering::SeqCst), 1000);
    }

    #[test]
    fn an_exception_on_a_worker_thread_reaches_the_caller_and_the_pool_stays_usable() {
        let pool = TaskPool::new(4);
        let caller = std::thread::current().id();
        let thrown = AtomicBool::new(false);
        let failure = catch_unwind(AssertUnwindSafe(|| {
            pool.parallel_for(0, 100_000, |_| {
                if std::thread::current().id() != caller {
                    thrown.store(true, Ordering::SeqCst);
                    panic!("boom on a worker");
                }
                while !thrown.load(Ordering::SeqCst) {
                    std::thread::yield_now();
                }
            });
        }))
        .expect_err("the worker's exception did not reach the caller");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"boom on a worker"));
        let after = AtomicUsize::new(0);
        pool.parallel_for(0, 1000, |_| {
            after.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(after.load(Ordering::SeqCst), 1000);
    }

    #[test]
    fn nested_calls_run_inline_instead_of_deadlocking() {
        let pool = TaskPool::new(3);
        let inner = AtomicUsize::new(0);
        pool.parallel_for(0, 8, |_| {
            pool.parallel_for(0, 10, |_| {
                inner.fetch_add(1, Ordering::SeqCst);
            });
        });
        assert_eq!(inner.load(Ordering::SeqCst), 80);
    }

    #[test]
    fn concurrency_counts_the_calling_thread() {
        assert_eq!(
            TaskPool::new(0).concurrency(),
            1,
            "with no workers the caller still runs the work"
        );
        assert_eq!(TaskPool::new(3).concurrency(), 4);
    }

    #[test]
    fn many_consecutive_jobs_all_complete() {
        let pool = TaskPool::new(4);
        for round in 0..200 {
            let visits: Vec<_> = (0..256).map(|_| AtomicUsize::new(0)).collect();
            pool.parallel_for(0, visits.len(), |index| {
                visits[index].fetch_add(1, Ordering::SeqCst);
            });
            for (index, visited) in visits.iter().enumerate() {
                assert_eq!(
                    visited.load(Ordering::SeqCst),
                    1,
                    "round {round} index {index}"
                );
            }
        }
    }
}

mod task_pool_contract {
    use katana_core::task_pool::TaskPool;
    use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier, Mutex, mpsc};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn nested_calls_keep_the_calling_thread_with_zero_or_one_worker() {
        for workers in [0, 1] {
            let pool = TaskPool::new(workers);
            let visits = AtomicUsize::new(0);
            pool.parallel_for(0, 8, |_| {
                let caller = std::thread::current().id();
                pool.parallel_for(0, 10, |_| {
                    assert_eq!(std::thread::current().id(), caller);
                    visits.fetch_add(1, Ordering::SeqCst);
                });
            });
            assert_eq!(visits.load(Ordering::SeqCst), 80);
            let failure = catch_unwind(AssertUnwindSafe(|| {
                pool.parallel_for(0, 1, |_| {
                    pool.parallel_for(0, 1, |_| panic!("nested failure"))
                });
            }))
            .unwrap_err();
            assert_eq!(failure.downcast_ref::<&str>(), Some(&"nested failure"));
            pool.parallel_for(0, 1, |_| {
                visits.fetch_add(1, Ordering::SeqCst);
            });
            assert_eq!(visits.load(Ordering::SeqCst), 81);
        }
    }

    #[test]
    fn surplus_panic_destructors_run_on_the_caller_and_leave_workers_usable() {
        struct PanickingPayload {
            armed: bool,
            drops: Arc<Mutex<Vec<thread::ThreadId>>>,
            secondary_dropped: Arc<AtomicBool>,
        }

        struct SecondaryPayload(Arc<AtomicBool>);

        impl Drop for PanickingPayload {
            fn drop(&mut self) {
                if self.armed {
                    self.drops.lock().unwrap().push(thread::current().id());
                    panic_any(SecondaryPayload(Arc::clone(&self.secondary_dropped)));
                }
            }
        }

        impl Drop for SecondaryPayload {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
                panic!("the secondary panic payload was dropped");
            }
        }

        let (completed, completion) = mpsc::channel();
        let runner = thread::spawn(move || {
            let pool = TaskPool::new(2);
            let caller = thread::current().id();
            let ready = Barrier::new(pool.concurrency());
            let drops = Arc::new(Mutex::new(Vec::new()));
            let secondary_dropped = Arc::new(AtomicBool::new(false));
            let failure = catch_unwind(AssertUnwindSafe(|| {
                pool.parallel_for(0, pool.concurrency(), |_| {
                    ready.wait();
                    panic_any(PanickingPayload {
                        armed: true,
                        drops: Arc::clone(&drops),
                        secondary_dropped: Arc::clone(&secondary_dropped),
                    });
                });
            }))
            .expect_err("a callback panic must reach the caller");
            let original = match failure.downcast::<PanickingPayload>() {
                Ok(mut payload) => {
                    payload.armed = false;
                    true
                }
                Err(payload) => {
                    std::mem::forget(payload);
                    false
                }
            };
            assert!(
                original,
                "the caller must receive a callback's panic payload"
            );
            assert_eq!(*drops.lock().unwrap(), vec![caller; 2]);
            assert!(!secondary_dropped.load(Ordering::SeqCst));

            let visits = AtomicUsize::new(0);
            pool.parallel_for(0, 1000, |_| {
                visits.fetch_add(1, Ordering::SeqCst);
            });
            assert_eq!(visits.load(Ordering::SeqCst), 1000);
            drop(pool);
            completed.send(()).unwrap();
        });
        completion
            .recv_timeout(Duration::from_secs(10))
            .expect("panic disposal, worker reuse and shutdown must finish without deadlocking");
        runner.join().unwrap();
    }
}
