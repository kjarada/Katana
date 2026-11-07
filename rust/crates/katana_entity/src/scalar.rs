//! std::min, std::max and std::clamp as the C++ writes them: which argument a
//! NaN or a signed zero comes back as is part of the result, and f64::min,
//! f64::max and f64::clamp answer differently for a NaN.

pub(crate) fn min(first: f64, second: f64) -> f64 {
    if second < first { second } else { first }
}

pub(crate) fn max(first: f64, second: f64) -> f64 {
    if first < second { second } else { first }
}

pub(crate) fn clamp(value: f64, low: f64, high: f64) -> f64 {
    if value < low {
        low
    } else if high < value {
        high
    } else {
        value
    }
}
