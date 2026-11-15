//! katana_math::libm, the C library's asinh, erf, erfc and lgamma. New tests,
//! not twins of a C++ case: that the declarations link and reach the C
//! functions. The special values are the ones C11 Annex F fixes exactly
//! (F.10.2.2, F.10.5.3, F.10.8.1, F.10.8.2); the ordinary values are published
//! constants, compared within 1e-15, about five units in the last place at
//! these magnitudes - room for a C library's own error of one or two.

#![allow(
    clippy::excessive_precision,
    reason = "Published constants are written with all the digits their tables give."
)]

use katana_math::libm::{asinh, erf, erfc, lgamma};

const NAN: f64 = f64::NAN;
const INFINITY: f64 = f64::INFINITY;

#[track_caller]
fn same(actual: f64, expected: f64) {
    assert_eq!(
        actual.to_bits(),
        expected.to_bits(),
        "{actual:?} is not {expected:?}, bit for bit"
    );
}

#[track_caller]
fn near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-15,
        "{actual:?} is not within 1e-15 of {expected:?}"
    );
}

mod math_libm {
    use super::*;

    #[test]
    fn asinh_keeps_the_sign_of_zero_and_of_infinity() {
        same(asinh(0.0), 0.0);
        same(asinh(-0.0), -0.0);
        same(asinh(INFINITY), INFINITY);
        same(asinh(-INFINITY), -INFINITY);
        assert!(asinh(NAN).is_nan());
        // asinh 1 = ln(1 + sqrt 2).
        near(asinh(1.0), 0.881_373_587_019_543_025);
    }

    #[test]
    fn erf_and_erfc_reach_their_limits_exactly() {
        same(erf(0.0), 0.0);
        same(erf(-0.0), -0.0);
        same(erf(INFINITY), 1.0);
        same(erf(-INFINITY), -1.0);
        same(erfc(INFINITY), 0.0);
        same(erfc(-INFINITY), 2.0);
        assert!(erf(NAN).is_nan());
        assert!(erfc(NAN).is_nan());
        // erf 1 to eighteen places (Abramowitz and Stegun, table 7.1, gives
        // fifteen: 0.84270 07929 49715), and erfc 1 = 1 - erf 1.
        near(erf(1.0), 0.842_700_792_949_714_869);
        near(erfc(1.0), 0.157_299_207_050_285_131);
    }

    #[test]
    fn lgamma_is_zero_at_one_and_two_and_infinite_at_its_poles() {
        same(lgamma(1.0), 0.0);
        same(lgamma(2.0), 0.0);
        same(lgamma(0.0), INFINITY);
        same(lgamma(-0.0), INFINITY);
        same(lgamma(-1.0), INFINITY);
        same(lgamma(INFINITY), INFINITY);
        same(lgamma(-INFINITY), INFINITY);
        assert!(lgamma(NAN).is_nan());
        // Gamma(3) = 2; Gamma(1/2) = sqrt pi; Gamma(-1/2) = -2 sqrt pi, whose
        // logarithm of the magnitude is ln 2 + ln sqrt pi.
        near(lgamma(3.0), std::f64::consts::LN_2);
        near(lgamma(0.5), 0.572_364_942_924_700_087);
        near(lgamma(-0.5), 1.265_512_123_484_645_396);
    }

    #[test]
    fn lgamma_gives_the_same_bits_from_threads_at_once() {
        // The lock around the C function's global sign: every thread's answer
        // is the serial one.
        let arguments: Vec<f64> = (1..=400).map(|i| f64::from(i) * 0.0625 - 12.0).collect();
        let serial: Vec<u64> = arguments.iter().map(|&x| lgamma(x).to_bits()).collect();
        let arguments = &arguments;
        std::thread::scope(|scope| {
            let workers: [_; 4] = std::array::from_fn(|_| {
                scope.spawn(move || {
                    arguments
                        .iter()
                        .map(|&x| lgamma(x).to_bits())
                        .collect::<Vec<u64>>()
                })
            });
            for worker in workers {
                assert_eq!(worker.join().unwrap(), serial);
            }
        });
    }
}
