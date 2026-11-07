//! The one function of the C library's maths the writer needs that Rust's std
//! does not take from it: `asinh`. Rust computes `f64::asinh` itself (from
//! `ln_1p` and `hypot`), which can differ from the C library's in the last
//! place; the length of a vertical curve's IfcCurveSegment is written from it
//! (alignment.rs), so the file would differ from the C++'s by a digit. Every
//! other function the writer and reader use (sin, cos, atan, atan2, acos, pow,
//! hypot, ln) std already takes from the C library.

#![allow(
    unsafe_code,
    reason = "The declaration of the C library's asinh is an extern block, which is unsafe to write."
)]

// libm on Linux, the C runtime on Windows: std links both already, for the
// functions it takes from them.
unsafe extern "C" {
    // SAFETY: asinh takes a double by value and returns one; it reads and
    // writes no memory, and is defined for every input (NaN and infinities
    // included), so a call is sound whatever the argument.
    #[link_name = "asinh"]
    safe fn c_asinh(value: f64) -> f64;
}

/// `std::asinh`: the C library's inverse hyperbolic sine.
pub(crate) fn asinh(value: f64) -> f64 {
    c_asinh(value)
}
