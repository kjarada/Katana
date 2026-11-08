//! The C library's `asinh`, `erf`, `erfc` and `lgamma`, which the C++ reaches
//! as `std::asinh`, `std::erf`, `std::erfc` and `std::lgamma`. Rust computes
//! `f64::asinh` itself (from `ln_1p` and `hypot`), which can differ from the C
//! library's in the last place, and has no stable `erf`, `erfc` or `lgamma`;
//! a port whose digits must be the C++'s calls these. The others the port
//! uses (sin, cos, atan2, exp, ln, pow, hypot, ln_1p and the like) std already
//! takes from the C library.
//!
//! libm on Linux, the C runtime on Windows: std links both already, for the
//! functions it takes from them.

#![allow(
    unsafe_code,
    reason = "The declarations of the C library's functions are an extern block, which is unsafe to write."
)]

use std::sync::{Mutex, PoisonError};

#[cfg_attr(unix, link(name = "m"))]
unsafe extern "C" {
    // SAFETY: each takes a double by value and returns one; none reads or
    // writes memory of the caller's (errno, which a range error may set, is
    // the calling thread's own), and each is defined for every input, NaN and
    // the infinities included (C11 7.12.5.2, 7.12.8.1 to 7.12.8.3), so a call
    // is sound whatever the argument. lgamma also writes the C library's
    // global `signgam`, which is why it is reached only through `lgamma`
    // below.

    /// `std::asinh`: the inverse hyperbolic sine.
    pub safe fn asinh(x: f64) -> f64;

    /// `std::erf`: the error function.
    pub safe fn erf(x: f64) -> f64;

    /// `std::erfc`: the complementary error function, `1 - erf(x)` without
    /// the cancellation.
    pub safe fn erfc(x: f64) -> f64;

    #[link_name = "lgamma"]
    safe fn c_lgamma(x: f64) -> f64;
}

/// Held for every call of the C library's `lgamma`, which stores the sign of
/// Gamma(x) in the global `signgam` (POSIX; glibc lists it "MT-Unsafe
/// race:signgam", and MinGW's has the same global): two threads in it at once
/// would race on that int.
static LGAMMA: Mutex<()> = Mutex::new(());

/// `std::lgamma`: the natural logarithm of |Gamma(x)|, the C library's digits.
/// Calls are taken one at a time, because the C function also writes the
/// global `signgam`. `lgamma_r` would not, but the C runtime on Windows has
/// none, and `std::lgamma` is `lgamma`.
pub fn lgamma(x: f64) -> f64 {
    let _one_at_a_time = LGAMMA.lock().unwrap_or_else(PoisonError::into_inner);
    c_lgamma(x)
}
