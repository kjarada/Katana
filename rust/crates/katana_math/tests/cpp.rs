//! katana_math::cpp, the C++ standard library's answers on doubles. New tests,
//! not twins of a C++ case: every expected value comes from the standard's
//! definition, never from a run. std::min and std::max return the first
//! argument when neither is less, and std::min({..}) and std::max({..}) the
//! first of the smallest or the largest ([alg.min.max]); std::clamp is
//! libstdc++'s min(max(value, low), high), which is its Returns clause
//! ([alg.clamp]) wherever the bounds do not cross; std::remainder is IEEE
//! 754's remainder (5.3.1), x - n y with n the integer nearest x / y and the even one on a
//! tie, exact, a zero result carrying the sign of x.

use katana_math::cpp::{clamp, max, max3, min, min3, remainder};

const NAN: f64 = f64::NAN;
const INFINITY: f64 = f64::INFINITY;

/// The bits, not the value: `-0.0 == 0.0` would pass a test of which zero
/// came back.
#[track_caller]
fn same(actual: f64, expected: f64) {
    assert_eq!(
        actual.to_bits(),
        expected.to_bits(),
        "{actual:?} is not {expected:?}, bit for bit"
    );
}

#[track_caller]
fn nan(actual: f64) {
    assert!(actual.is_nan(), "{actual:?} is not NaN");
}

/// 2^exponent: a normal double's biased exponent alone, or below 2^-1022 the
/// one bit of a subnormal.
fn two_to(exponent: i32) -> f64 {
    assert!((-1074..=1023).contains(&exponent));
    if exponent < -1022 {
        f64::from_bits(1_u64 << (exponent + 1074))
    } else {
        f64::from_bits(((exponent + 1023) as u64) << 52)
    }
}

/// `count` times the smallest subnormal, 2^-1074: a subnormal's bits are its
/// count of them.
fn subnormals(count: u64) -> f64 {
    assert!(count < 1 << 52);
    f64::from_bits(count)
}

mod cpp_min_max {
    use super::*;

    #[test]
    fn min_and_max_answer_the_smaller_and_the_larger_of_ordinary_values() {
        same(min(1.0, 2.0), 1.0);
        same(min(2.0, 1.0), 1.0);
        same(max(1.0, 2.0), 2.0);
        same(max(2.0, 1.0), 2.0);
        same(min(-3.5, 2.25), -3.5);
        same(max(-3.5, 2.25), 2.25);
        same(min(-INFINITY, -1e308), -INFINITY);
        same(max(INFINITY, 1e308), INFINITY);
        same(min(INFINITY, 1.0), 1.0);
        same(max(-INFINITY, 1.0), 1.0);
        same(min(subnormals(1), 0.0), 0.0);
        same(max(subnormals(1), 0.0), subnormals(1));
    }

    #[test]
    fn a_nan_first_comes_back_and_a_nan_second_is_passed_over() {
        nan(min(NAN, 1.0));
        same(min(1.0, NAN), 1.0);
        nan(max(NAN, 1.0));
        same(max(1.0, NAN), 1.0);
        nan(min(NAN, NAN));
        nan(max(NAN, NAN));
        // Why the module exists: Rust's own passes over the NaN wherever it
        // stands (its documentation: "If one of the arguments is NaN, then
        // the other argument is returned").
        same(f64::min(NAN, 1.0), 1.0);
        same(f64::max(NAN, 1.0), 1.0);
    }

    #[test]
    fn of_two_zeros_the_first_is_kept_in_either_order() {
        same(min(-0.0, 0.0), -0.0);
        same(min(0.0, -0.0), 0.0);
        same(max(-0.0, 0.0), -0.0);
        same(max(0.0, -0.0), 0.0);
    }

    #[test]
    fn min3_and_max3_answer_every_ordering_of_three_values() {
        let orders: [[f64; 3]; 6] = [
            [1.0, 2.0, 3.0],
            [1.0, 3.0, 2.0],
            [2.0, 1.0, 3.0],
            [2.0, 3.0, 1.0],
            [3.0, 1.0, 2.0],
            [3.0, 2.0, 1.0],
        ];
        for [a, b, c] in orders {
            same(min3(a, b, c), 1.0);
            same(max3(a, b, c), 3.0);
        }
    }

    #[test]
    fn a_nan_comes_back_from_three_only_in_the_first_place() {
        nan(min3(NAN, 1.0, 2.0));
        same(min3(1.0, NAN, 2.0), 1.0);
        same(min3(2.0, NAN, 1.0), 1.0);
        same(min3(1.0, 2.0, NAN), 1.0);
        same(min3(2.0, 1.0, NAN), 1.0);
        nan(max3(NAN, 1.0, 2.0));
        same(max3(1.0, NAN, 2.0), 2.0);
        same(max3(2.0, NAN, 1.0), 2.0);
        same(max3(1.0, 2.0, NAN), 2.0);
        same(max3(2.0, 1.0, NAN), 2.0);
    }

    #[test]
    fn of_three_the_first_of_the_tied_zeros_is_kept() {
        same(min3(0.0, -0.0, -0.0), 0.0);
        same(min3(-0.0, 0.0, 0.0), -0.0);
        same(max3(0.0, -0.0, -0.0), 0.0);
        same(max3(-0.0, 0.0, 0.0), -0.0);
        // A larger value first: the first zero after it is the first of the
        // smallest.
        same(min3(1.0, -0.0, 0.0), -0.0);
        same(min3(1.0, 0.0, -0.0), 0.0);
        same(max3(-1.0, -0.0, 0.0), -0.0);
        same(max3(-1.0, 0.0, -0.0), 0.0);
        // A strictly smaller value later still wins.
        same(min3(0.0, -0.0, -1.0), -1.0);
    }
}

mod cpp_clamp {
    use super::*;

    #[test]
    fn a_value_inside_the_bounds_is_kept_and_one_outside_takes_the_nearer_bound() {
        same(clamp(0.5, 0.0, 1.0), 0.5);
        same(clamp(-1.0, 0.0, 1.0), 0.0);
        same(clamp(2.0, 0.0, 1.0), 1.0);
        same(clamp(0.0, 0.0, 1.0), 0.0);
        same(clamp(1.0, 0.0, 1.0), 1.0);
        same(clamp(INFINITY, 0.0, 1.0), 1.0);
        same(clamp(-INFINITY, 0.0, 1.0), 0.0);
        same(clamp(1.0, 2.0, 2.0), 2.0);
        same(clamp(3.0, 2.0, 2.0), 2.0);
    }

    #[test]
    fn a_nan_value_comes_back() {
        nan(clamp(NAN, 0.0, 1.0));
        nan(clamp(NAN, NAN, NAN));
    }

    #[test]
    fn a_nan_bound_is_passed_over() {
        // f64::clamp panics on each of these.
        same(clamp(0.5, NAN, 1.0), 0.5);
        same(clamp(2.0, NAN, 1.0), 1.0);
        same(clamp(-5.0, NAN, 1.0), -5.0);
        same(clamp(0.5, 0.0, NAN), 0.5);
        same(clamp(-1.0, 0.0, NAN), 0.0);
        same(clamp(5.0, 0.0, NAN), 5.0);
        same(clamp(5.0, NAN, NAN), 5.0);
    }

    #[test]
    fn a_zero_value_keeps_its_sign_against_a_zero_bound() {
        same(clamp(-0.0, 0.0, 1.0), -0.0);
        same(clamp(0.0, -0.0, 1.0), 0.0);
        same(clamp(0.0, -1.0, -0.0), 0.0);
        same(clamp(-0.0, -1.0, 0.0), -0.0);
    }

    #[test]
    fn crossed_bounds_answer_as_libstdcpp_does_high_last() {
        // The standard requires !(high < low). libstdc++ 16's body,
        // std::min(std::max(value, low), high) (stl_algo.h), is what the C++
        // program runs: max(-1, 1) = 1, then min(1, 0) = 0; max(2, 1) = 2,
        // min(2, 0) = 0; max(0.5, 1) = 1, min(1, 0) = 0. The Returns clause
        // alone would say 1.0 for the first and the third.
        same(clamp(-1.0, 1.0, 0.0), 0.0);
        same(clamp(2.0, 1.0, 0.0), 0.0);
        same(clamp(0.5, 1.0, 0.0), 0.0);
    }
}

mod cpp_remainder {
    use super::*;

    #[test]
    fn halfway_quotients_round_to_the_even_multiple() {
        same(remainder(1.0, 2.0), 1.0); //  0.5 -> 0
        same(remainder(3.0, 2.0), -1.0); // 1.5 -> 2
        same(remainder(5.0, 2.0), 1.0); //  2.5 -> 2
        same(remainder(7.0, 2.0), -1.0); // 3.5 -> 4
        same(remainder(-1.0, 2.0), -1.0); // -0.5 -> 0
        same(remainder(-3.0, 2.0), 1.0); //  -1.5 -> -2
        same(remainder(-5.0, 2.0), -1.0); // -2.5 -> -2
        same(remainder(-7.0, 2.0), 1.0); //  -3.5 -> -4
        same(remainder(180.0, 360.0), 180.0); //  0.5 -> 0
        same(remainder(540.0, 360.0), -180.0); // 1.5 -> 2
        same(remainder(900.0, 360.0), 180.0); //  2.5 -> 2
        same(remainder(-180.0, 360.0), -180.0);
        same(remainder(-540.0, 360.0), 180.0);
    }

    #[test]
    fn other_quotients_round_to_the_nearest_multiple() {
        same(remainder(5.25, 2.0), -0.75); // 2.625 -> 3
        same(remainder(4.75, 2.0), 0.75); //  2.375 -> 2
        same(remainder(0.75, 1.0), -0.25);
        same(remainder(0.25, 1.0), 0.25);
        same(remainder(370.0, 360.0), 10.0);
        same(remainder(710.0, 360.0), -10.0);
        same(remainder(-370.0, 360.0), -10.0);
        // Rust's % truncates the quotient instead: 7 % 2 is 1.
        same(7.0_f64 % 2.0, 1.0);
    }

    #[test]
    fn the_sign_of_the_divisor_does_not_matter() {
        same(remainder(5.0, -2.0), 1.0);
        same(remainder(7.0, -2.0), -1.0);
        same(remainder(5.25, -2.0), -0.75);
        same(remainder(-5.25, -2.0), 0.75);
    }

    #[test]
    fn a_zero_remainder_has_the_sign_of_x() {
        same(remainder(4.0, 2.0), 0.0);
        same(remainder(-4.0, 2.0), -0.0);
        same(remainder(2.0, 2.0), 0.0);
        same(remainder(-2.0, 2.0), -0.0);
        same(remainder(2.0, -2.0), 0.0);
        same(remainder(0.0, 1.0), 0.0);
        same(remainder(-0.0, 1.0), -0.0);
        same(remainder(720.0, 360.0), 0.0);
        same(remainder(-720.0, 360.0), -0.0);
    }

    #[test]
    fn the_result_is_exact_where_the_quotient_is_not() {
        // 2^53 = 9007199254740992 = 3 * 3002399751580330 + 2: the nearest
        // multiple of 3 is 9007199254740993, one above. x - y * (x / y).round()
        // answers 0 here, because that multiple is not a double.
        let x = two_to(53);
        same(remainder(x, 3.0), -1.0);
        // 2^53 + 2 = 3 * 3002399751580331 + 1; 2^53 + 4 = 3 * 3002399751580332.
        same(remainder(x + 2.0, 3.0), 1.0);
        same(remainder(x + 4.0, 3.0), 0.0);
    }

    #[test]
    fn a_divisor_too_large_to_double_still_rounds_to_the_nearest() {
        // y = 2^1023, so 2y overflows and fdlibm takes no fmod.
        let y = two_to(1023);
        // f64::MAX = 2^1024 - 2^971, so x / y = 2 - 2^-52 -> 2: x - 2y = -2^971.
        same(remainder(f64::MAX, y), -two_to(971));
        // 1.5 y is a tie between 1 and 2: the even one gives -0.5 y.
        same(remainder(1.5 * y, y), -two_to(1022));
        same(remainder(-1.5 * y, y), two_to(1022));
        same(remainder(y, y), 0.0);
        same(remainder(f64::MAX, f64::MAX), 0.0);
        same(remainder(-f64::MAX, f64::MAX), -0.0);
        same(remainder(1.0, f64::MAX), 1.0);
    }

    #[test]
    fn subnormal_divisors_round_halfway_quotients_to_even() {
        // Below 2^-1021 fdlibm doubles the remainder rather than halve the
        // divisor, whose last bit halving could lose. s is 2^-1074.
        let s = subnormals;
        same(remainder(s(1), s(2)), s(1)); //   0.5 -> 0
        same(remainder(s(3), s(2)), -s(1)); //  1.5 -> 2
        same(remainder(s(5), s(2)), s(1)); //   2.5 -> 2
        same(remainder(s(7), s(2)), -s(1)); //  3.5 -> 4
        same(remainder(-s(3), s(2)), s(1)); // -1.5 -> -2
        same(remainder(s(15), s(6)), s(3)); //  2.5 -> 2
        same(remainder(s(21), s(6)), -s(3)); // 3.5 -> 4
        same(remainder(s(1), s(3)), s(1)); //   1/3 -> 0
        same(remainder(s(2), s(3)), -s(1)); //  2/3 -> 1
        same(remainder(s(1), s(1)), 0.0);
        same(remainder(s(3), s(1)), 0.0);
        same(remainder(-s(3), s(1)), -0.0);
        // The smallest normal divisor, 2^-1022, is still on the doubling path;
        // 2^-1021 is the first that halves. A tie at each.
        let y = two_to(-1022);
        same(remainder(1.5 * y, y), -two_to(-1023));
        same(remainder(2.5 * y, y), two_to(-1023));
        let y = two_to(-1021);
        same(remainder(1.5 * y, y), -two_to(-1022));
        same(remainder(2.5 * y, y), two_to(-1022));
    }

    #[test]
    fn invalid_operations_give_nan_and_an_infinite_divisor_gives_x() {
        nan(remainder(INFINITY, 1.0));
        nan(remainder(-INFINITY, 1.0));
        nan(remainder(1.0, 0.0));
        nan(remainder(1.0, -0.0));
        nan(remainder(0.0, 0.0));
        nan(remainder(INFINITY, 0.0));
        nan(remainder(INFINITY, INFINITY));
        nan(remainder(NAN, 1.0));
        nan(remainder(1.0, NAN));
        same(remainder(3.0, INFINITY), 3.0);
        same(remainder(-3.0, -INFINITY), -3.0);
        same(remainder(-0.0, INFINITY), -0.0);
        same(remainder(f64::MAX, INFINITY), f64::MAX);
    }
}
