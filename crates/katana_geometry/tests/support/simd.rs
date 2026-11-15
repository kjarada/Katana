use katana_core::cpu_features::{SimdLevel, detected_simd_level, set_simd_level};
use std::sync::Mutex;

static LEVEL_LOCK: Mutex<()> = Mutex::new(());
struct RestoreLevel(SimdLevel);
impl Drop for RestoreLevel {
    fn drop(&mut self) {
        set_simd_level(self.0).expect("restore SIMD level");
    }
}
pub fn at_simd_level<Value>(level: SimdLevel, body: impl FnOnce() -> Value) -> Value {
    let _lock = LEVEL_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _restore = RestoreLevel(set_simd_level(level).expect("requested SIMD level must run"));
    body()
}
pub fn kernel_level() -> SimdLevel {
    detected_simd_level()
}
pub fn kernels_available() -> bool {
    kernel_level() != SimdLevel::Scalar
}
pub fn require_kernel() -> SimdLevel {
    let level = kernel_level();
    assert_ne!(
        level,
        SimdLevel::Scalar,
        "a real SIMD backend is required for this parity test"
    );
    level
}
pub fn simd_levels() -> Vec<SimdLevel> {
    let mut levels = vec![SimdLevel::Scalar];
    if kernels_available() {
        levels.push(kernel_level());
    }
    levels
}
