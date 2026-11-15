//! The C++ standard library's `std::min`, `std::max` and `std::clamp`, for
//! floats and integers alike.
//!
//! `f64::min` and `f32::max` return the operand that is not NaN and leave the
//! sign of a zero tie unspecified; the C++ returns the first operand unless the
//! second compares strictly before (or after) it, so a NaN in the first place
//! comes through and of two equal zeros the first is kept. The rasteriser's
//! coverage, depth and colour, and the SIMD kernels that must match them bit
//! for bit, depend on which operand comes back.

/// `std::min(first, second)`: `(second < first) ? second : first`.
pub(crate) fn min<T: PartialOrd + Copy>(first: T, second: T) -> T {
    if second < first { second } else { first }
}

/// `std::max(first, second)`: `(first < second) ? second : first`.
pub(crate) fn max<T: PartialOrd + Copy>(first: T, second: T) -> T {
    if first < second { second } else { first }
}

/// `std::clamp(value, low, high)` as libstdc++ writes it:
/// `std::min(std::max(value, low), high)`.
pub(crate) fn clamp<T: PartialOrd + Copy>(value: T, low: T, high: T) -> T {
    min(max(value, low), high)
}

/// `std::min({a, b, c})`: the first of the smallest, as `std::min_element`.
pub(crate) fn min3<T: PartialOrd + Copy>(values: [T; 3]) -> T {
    min(min(values[0], values[1]), values[2])
}

/// `std::max({a, b, c})`: the first of the largest, as `std::max_element`.
pub(crate) fn max3<T: PartialOrd + Copy>(values: [T; 3]) -> T {
    max(max(values[0], values[1]), values[2])
}
