use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};

type Panic = Box<dyn Any + Send>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

struct Job {
    body: borrowed_body::Body,
    end: usize,
    grain: usize,
    cursor: AtomicUsize,
    remaining: AtomicUsize,
    failed: AtomicBool,
    errors: Mutex<Vec<Panic>>,
}

struct PoolState {
    job: Option<Arc<Job>>,
    generation: usize,
    running: bool,
}

struct Shared {
    state: Mutex<PoolState>,
    wake: Condvar,
    done: Condvar,
}

pub struct TaskPool {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

impl Default for TaskPool {
    fn default() -> Self {
        Self::new(Self::default_worker_threads())
    }
}

impl TaskPool {
    pub fn default_worker_threads() -> usize {
        thread::available_parallelism().map_or(0, |count| count.get().saturating_sub(1))
    }

    pub fn shared() -> &'static Self {
        static POOL: OnceLock<TaskPool> = OnceLock::new();
        POOL.get_or_init(Self::default)
    }

    pub fn new(worker_threads: usize) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(PoolState {
                job: None,
                generation: 0,
                running: true,
            }),
            wake: Condvar::new(),
            done: Condvar::new(),
        });
        let mut threads = Vec::with_capacity(worker_threads);
        for _ in 0..worker_threads {
            let worker_shared = Arc::clone(&shared);
            match thread::Builder::new().spawn(move || worker_loop(&worker_shared)) {
                Ok(worker) => threads.push(worker),
                Err(error) => {
                    lock(&shared.state).running = false;
                    shared.wake.notify_all();
                    for worker in threads {
                        let _ = worker.join();
                    }
                    panic!("failed to create a task-pool worker: {error}");
                }
            }
        }
        Self { shared, threads }
    }

    pub fn concurrency(&self) -> usize {
        self.threads.len() + 1
    }

    #[allow(
        unsafe_code,
        reason = "Completion retires every worker's borrowed callback before body leaves scope, including unwinding."
    )]
    pub fn parallel_ranges<F>(&self, begin: usize, end: usize, grain: usize, body: F)
    where
        F: Fn(usize, usize) + Sync,
    {
        if begin >= end {
            return;
        }
        let job = Arc::new(Job {
            body: unsafe { borrowed_body::Body::new(&body) },
            end,
            grain: grain.max(1),
            cursor: AtomicUsize::new(begin),
            remaining: AtomicUsize::new(0),
            failed: AtomicBool::new(false),
            errors: Mutex::new(Vec::new()),
        });
        let mut completion = Completion {
            shared: &self.shared,
            job: &job,
            published: false,
        };
        {
            let mut state = lock(&self.shared.state);
            if state.job.is_none() && !self.threads.is_empty() && state.running {
                job.remaining.store(self.threads.len(), Ordering::Relaxed);
                state.job = Some(Arc::clone(&job));
                state.generation = state.generation.wrapping_add(1);
                completion.published = true;
            }
        }
        if completion.published {
            self.shared.wake.notify_all();
        }
        run_chunks(&job);
        drop(completion);
        let mut errors = std::mem::take(&mut *lock(&job.errors)).into_iter();
        if let Some(error) = errors.next() {
            for surplus in errors {
                if let Err(secondary) = catch_unwind(AssertUnwindSafe(|| drop(surplus))) {
                    std::mem::forget(secondary);
                }
            }
            resume_unwind(error);
        }
    }

    pub fn parallel_for<F>(&self, begin: usize, end: usize, body: F)
    where
        F: Fn(usize) + Sync,
    {
        self.parallel_ranges(begin, end, 1, |low, high| {
            for index in low..high {
                body(index);
            }
        });
    }
}

impl Drop for TaskPool {
    fn drop(&mut self) {
        lock(&self.shared.state).running = false;
        self.shared.wake.notify_all();
        for worker in self.threads.drain(..) {
            let _ = worker.join();
        }
    }
}

struct Completion<'a> {
    shared: &'a Shared,
    job: &'a Job,
    published: bool,
}

impl Drop for Completion<'_> {
    fn drop(&mut self) {
        if !self.published {
            return;
        }
        self.shared.wake.notify_all();
        let mut state = lock(&self.shared.state);
        while self.job.remaining.load(Ordering::Acquire) != 0 {
            state = self
                .shared
                .done
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
        state.job = None;
    }
}

fn worker_loop(shared: &Shared) {
    let mut last_seen = 0;
    loop {
        let job = {
            let mut state = lock(&shared.state);
            while state.running && (state.job.is_none() || state.generation == last_seen) {
                state = shared
                    .wake
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
            }
            if !state.running {
                return;
            }
            last_seen = state.generation;
            Arc::clone(state.job.as_ref().expect("a generation has a job"))
        };
        run_chunks(&job);
        if job.remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _state = lock(&shared.state);
            shared.done.notify_all();
        }
    }
}

fn run_chunks(job: &Job) {
    while !job.failed.load(Ordering::Relaxed) {
        let low = job.cursor.fetch_add(job.grain, Ordering::Relaxed);
        if low >= job.end {
            return;
        }
        let high = low.wrapping_add(job.grain).min(job.end);
        if let Err(error) = catch_unwind(AssertUnwindSafe(|| job.body.call(low, high))) {
            lock(&job.errors).push(error);
            job.failed.store(true, Ordering::Relaxed);
            return;
        }
    }
}

#[allow(
    unsafe_code,
    reason = "Private scoped callback erasure: F is Sync and Completion waits before its stack borrow can expire."
)]
mod borrowed_body {
    pub(super) struct Body {
        data: *const (),
        invoke: unsafe fn(*const (), usize, usize),
    }

    unsafe impl Send for Body {}
    unsafe impl Sync for Body {}

    impl Body {
        pub(super) unsafe fn new<F: Fn(usize, usize) + Sync>(body: &F) -> Self {
            unsafe fn invoke<F: Fn(usize, usize) + Sync>(data: *const (), low: usize, high: usize) {
                let body = unsafe { &*data.cast::<F>() };
                body(low, high);
            }
            Self {
                data: (body as *const F).cast(),
                invoke: invoke::<F>,
            }
        }

        pub(super) fn call(&self, low: usize, high: usize) {
            unsafe { (self.invoke)(self.data, low, high) };
        }
    }
}
