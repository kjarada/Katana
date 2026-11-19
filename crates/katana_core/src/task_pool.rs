use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;

type Panic = Box<dyn Any + Send>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

/// One call's work: the body, borrowed from the caller's frame, and the cursor its chunks are
/// taken from. The helpers borrow it through `thread::scope`, which joins them before the call
/// returns, so the borrow the compiler checks is the one the contract needs: no task outlives
/// the body it runs.
struct Job<'a, F> {
    body: &'a F,
    end: usize,
    grain: usize,
    cursor: AtomicUsize,
    failed: AtomicBool,
    errors: Mutex<Vec<Panic>>,
}

/// The threads of a call are its caller and, for as long as the call lasts, up to
/// `worker_threads` helpers, started for it in a `thread::scope` and joined before it returns.
/// Workers kept warm between calls would run a body borrowed from a frame that is not theirs,
/// which only `unsafe` can say (docs/architecture.md, "Threading"); a scoped helper borrows it
/// in a way the compiler checks. The price is a thread started per helper per call.
pub struct TaskPool {
    worker_threads: usize,
    // Set while a call has the helpers: a call made meanwhile - a nested one, or another
    // thread's - runs inline, as it did when the call had the pool's workers.
    busy: AtomicBool,
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
        Self {
            worker_threads,
            busy: AtomicBool::new(false),
        }
    }

    pub fn concurrency(&self) -> usize {
        self.worker_threads + 1
    }

    pub fn parallel_ranges<F>(&self, begin: usize, end: usize, grain: usize, body: F)
    where
        F: Fn(usize, usize) + Sync,
    {
        if begin >= end {
            return;
        }
        let job = Job {
            body: &body,
            end,
            grain: grain.max(1),
            cursor: AtomicUsize::new(begin),
            failed: AtomicBool::new(false),
            errors: Mutex::new(Vec::new()),
        };
        // No more helpers than there are chunks beside the caller's first: a helper started for
        // a chunk that is not there would only be started and joined.
        let helpers = self
            .worker_threads
            .min((end - begin).div_ceil(job.grain) - 1);
        let claim = if self.worker_threads > 0 {
            Claim::take(&self.busy)
        } else {
            None
        };
        if claim.is_some() && helpers > 0 {
            thread::scope(|scope| {
                for _ in 0..helpers {
                    // A helper the system will not start leaves its chunks to the others and the
                    // caller: the result is the same, only slower.
                    if thread::Builder::new()
                        .spawn_scoped(scope, || run_chunks(&job))
                        .is_err()
                    {
                        break;
                    }
                }
                run_chunks(&job);
            });
        } else {
            run_chunks(&job);
        }
        drop(claim);
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

/// The helpers, held by one call; let go when it is dropped.
struct Claim<'a>(&'a AtomicBool);

impl<'a> Claim<'a> {
    fn take(busy: &'a AtomicBool) -> Option<Self> {
        busy.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Self(busy))
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

// Every thread of a call runs this: chunks of `grain` indices taken from the cursor until there
// are none, or until a body has panicked, whose payload is kept for the caller.
fn run_chunks<F>(job: &Job<'_, F>)
where
    F: Fn(usize, usize) + Sync,
{
    while !job.failed.load(Ordering::Relaxed) {
        let low = job.cursor.fetch_add(job.grain, Ordering::Relaxed);
        if low >= job.end {
            return;
        }
        let high = low.wrapping_add(job.grain).min(job.end);
        if let Err(error) = catch_unwind(AssertUnwindSafe(|| (job.body)(low, high))) {
            lock(&job.errors).push(error);
            job.failed.store(true, Ordering::Relaxed);
            return;
        }
    }
}
