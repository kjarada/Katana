//! std::min and std::max as the C++ writes them: the first argument unless the
//! comparison says otherwise, so that a NaN goes where the C++ sends it.
//! f64::min and f64::max return the number beside a NaN instead.

/// std::min(a, b): `b` only when `b < a`.
pub(crate) fn std_min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// std::max(a, b): `b` only when `a < b`.
pub(crate) fn std_max(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}
