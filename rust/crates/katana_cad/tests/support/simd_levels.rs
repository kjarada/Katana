//! For tests that hold a SIMD kernel to its scalar reference: run the same
//! call at each level, through the same public function, and compare
//! (tests/simd_levels.hpp).
//!
//! The level is switched with katana_core's set_simd_level, the in-process
//! form of the KATANA_SIMD override. That is legitimate only because every
//! level is bit-identical - the property these tests exist to prove. Rust runs
//! a binary's tests on parallel threads in one process, so a switch holds one
//! lock for its scope: two tests never switch the level under each other.
//!
//! Where the C++ skips a comparison on a processor with no kernels
//! (KATANA_REQUIRE_SIMD_KERNELS), require_kernels fails instead: a comparison
//! of the scalar path with itself does not count as having compared the
//! kernels (docs/rust.md, "Step 1").
//!
//! Owned by the scene tests (wave G3); a test target includes it with
//! `#[path = "../support/simd_levels.rs"] pub mod simd_levels;`.

#![allow(
    dead_code,
    reason = "Each test binary includes this module and uses its own part of it."
)]

use std::sync::{Mutex, MutexGuard, PoisonError};

use katana_core::cpu_features::{SimdLevel, detected_simd_level, set_simd_level};

static LEVEL_LOCK: Mutex<()> = Mutex::new(());

/// Puts the process at a level for a scope and back afterwards.
pub struct ScopedSimdLevel {
    previous: Option<SimdLevel>,
    // Dropped after drop() has put the level back.
    _lock: MutexGuard<'static, ()>,
}

impl ScopedSimdLevel {
    pub fn new(level: SimdLevel) -> Self {
        let lock = LEVEL_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        Self {
            previous: set_simd_level(level).ok(),
            _lock: lock,
        }
    }

    pub fn ok(&self) -> bool {
        self.previous.is_some()
    }
}

impl Drop for ScopedSimdLevel {
    fn drop(&mut self) {
        if let Some(previous) = self.previous {
            let _ = set_simd_level(previous);
        }
    }
}

/// body() at `level`, with the level restored afterwards. Fails the test if
/// the processor cannot run the level; callers ask kernels_available() first.
pub fn at_simd_level<Value>(level: SimdLevel, body: impl FnOnce() -> Value) -> Value {
    let scope = ScopedSimdLevel::new(level);
    assert!(scope.ok(), "cannot run at {level}");
    body()
}

/// The level whose kernels this machine runs - Avx2 on an x86-64 processor
/// with AVX2, Neon on 64-bit ARM - or Scalar where there are none. It is what
/// every comparison holds against Scalar, so one test covers whichever kernel
/// set the build has.
pub fn kernel_level() -> SimdLevel {
    detected_simd_level()
}

pub fn kernels_available() -> bool {
    kernel_level() != SimdLevel::Scalar
}

/// Scalar, then the kernel level where there is one: the levels a test runs
/// the same call at.
pub fn simd_levels() -> Vec<SimdLevel> {
    let mut levels = vec![SimdLevel::Scalar];
    if kernels_available() {
        levels.push(kernel_level());
    }
    levels
}

/// The C++'s KATANA_REQUIRE_SIMD_KERNELS(): the kernel level, failing the test
/// on a processor that has no kernels to run.
pub fn require_kernels() -> SimdLevel {
    let level = kernel_level();
    assert_ne!(
        level,
        SimdLevel::Scalar,
        "no SIMD kernels run on this processor: only the scalar path can run here"
    );
    level
}
