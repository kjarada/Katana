//! What the C++ takes from its standard library on doubles, where Rust's own
//! functions answer otherwise: `std::min`, `std::max`, `std::clamp` and
//! `std::remainder`. This is the port's one home for them: a crate that needs
//! the C++'s answer calls it here and writes no copy of its own.
//!
//! `std::min` and `std::max` return their first argument unless the second
//! compares strictly less (greater). So a NaN in the first place comes back, a
//! NaN in the second is passed over, and of two zeros of opposite sign the
//! first is kept. `f64::min` and `f64::max` return the operand that is not NaN,
//! wherever it stands, and leave the sign of a zero tie unspecified.
//! `f64::clamp` panics on a NaN bound and on bounds that cross, where the C++
//! returns a value. Rust has no IEEE remainder: `%` is C's `fmod`, which
//! truncates the quotient where the remainder rounds it to the nearest.
//!
//! Nothing here is re-exported at the crate root, where `clamp` is already
//! `math::clamp` (numerics.rs) and a bare `min` or `max` would read as Rust's:
//! a caller writes `cpp::max(a, b)` or imports the names it wants.

/// `std::min(a, b)` on doubles: `(b < a) ? b : a`, as libstdc++ writes it -
/// "the first argument when the arguments are equivalent" ([alg.min.max]).
/// A NaN in either place gives `a`, and so does a tie of `-0.0` and `0.0`.
#[inline]
pub fn min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// `std::max(a, b)` on doubles: `(a < b) ? b : a`, the first argument when
/// neither is less ([alg.min.max]). A NaN in either place gives `a`, and so
/// does a tie of `-0.0` and `0.0`.
#[inline]
pub fn max(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// `std::min({a, b, c})`: the first of the smallest. libstdc++ answers with
/// `std::min_element`, which walks the list keeping the least so far and
/// takes an element only when it is strictly less; that is `min(min(a, b), c)`.
/// A NaN in the first place comes back; one after it is passed over.
#[inline]
pub fn min3(a: f64, b: f64, c: f64) -> f64 {
    min(min(a, b), c)
}

/// `std::max({a, b, c})`: the first of the largest, by `std::max_element`'s
/// walk, which is `max(max(a, b), c)`. A NaN in the first place comes back;
/// one after it is passed over.
#[inline]
pub fn max3(a: f64, b: f64, c: f64) -> f64 {
    max(max(a, b), c)
}

/// `std::clamp(value, low, high)` by the standard's Returns clause
/// ([alg.clamp]): `low` when `value < low`, `high` when `high < value`,
/// otherwise `value` itself, which keeps a NaN value and the sign of a zero. A
/// NaN bound is neither less nor greater than anything, so it is passed over,
/// where `f64::clamp` panics.
///
/// The standard requires `!(high < low)`. libstdc++ 16 writes the body as
/// `std::min(std::max(value, low), high)` (stl_algo.h), and so does this: the
/// C++ the port is checked against is GCC's build of it. On every input that
/// meets the requirement that is the standard's Returns clause; where the
/// bounds cross (`high < low`, which the C++ calls do not rule out when the
/// bounds are computed) it answers `high`, as the C++ program does in a
/// Release build, where `_GLIBCXX_ASSERTIONS` is off.
#[inline]
pub fn clamp(value: f64, low: f64, high: f64) -> f64 {
    min(max(value, low), high)
}

/// `std::remainder(x, y)`, the IEEE 754 remainder (5.3.1): `x - n y`, `n` the
/// integer nearest `x / y` and the even one when `x / y` is halfway between
/// two. The result is exact, at most `|y| / 2` in magnitude, a zero result has
/// the sign of `x`, and the sign of `y` does not matter. `x` itself when `y`
/// is infinite and `x` finite; NaN when `x` is infinite, `y` is zero or either
/// is NaN (7.2).
///
/// fdlibm's `__ieee754_remainder` (e_remainder.c), step for step, on top of
/// the exact `fmod` that Rust's `%` is. Because the result is exact and
/// unique, every correct implementation (glibc's, the C runtime's on Windows)
/// returns the same bits, but for a NaN's payload.
pub fn remainder(x: f64, y: f64) -> f64 {
    // The high word of |y|, as fdlibm tests it.
    let high_y = (y.to_bits() >> 32) as u32 & 0x7fff_ffff;
    if y == 0.0 || !x.is_finite() || y.is_nan() {
        #[allow(
            clippy::eq_op,
            reason = "fdlibm's own way to make the NaN of an invalid operation, kept for its bits."
        )]
        return (x * y) / (x * y);
    }
    let negative = x.is_sign_negative();
    let y = y.abs();
    let mut r = x;
    // Below 2^1023 y + y is finite: now |r| < 2y.
    if high_y <= 0x7fdf_ffff {
        r %= y + y;
    }
    if x.abs() == y {
        return 0.0 * x;
    }
    r = r.abs();
    if high_y < 0x0020_0000 {
        // y below 2^-1021: halving it could lose its last bit.
        if r + r > y {
            r -= y;
            if r + r >= y {
                r -= y;
            }
        }
    } else {
        let half = 0.5 * y;
        if r > half {
            r -= y;
            if r >= half {
                r -= y;
            }
        }
    }
    // fdlibm flips the sign bit for a negative x, whatever r's own sign is.
    if negative { -r } else { r }
}
