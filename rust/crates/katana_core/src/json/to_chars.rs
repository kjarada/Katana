// This file is a translation into Rust of nlohmann/json 3.12.0 (detail/conversions/to_chars.hpp),
// Copyright (c) 2013-2025 Niels Lohmann <https://nlohmann.me>, used under the MIT
// licence, whose notice is kept in THIRD_PARTY_NOTICES.md ("Code translated into the
// Rust port").
// That file is Florian Loitsch's reference implementation of Grisu2, Copyright (c) 2009
// Florian Loitsch <https://florian.loitsch.com/>, also under the MIT licence.
//! nlohmann's dtoa (detail/conversions/to_chars.hpp): Grisu2 (Loitsch, PLDI 2010), printed like
//! printf's %g with at least one fractional digit ("1.0", "1e+20", "1e-05"). Grisu2 is not always
//! the shortest round-trip text, so neither ryu nor katana_core's format_exact_real can stand in
//! for it.
//!
//! Ported line by line from nlohmann 3.12.0, whose code is Florian Loitsch's reference
//! implementation, slightly modified (MIT). The unsigned arithmetic wraps as the C++'s does; the
//! algorithm keeps every product in range.

#[derive(Clone, Copy)]
struct DiyFp {
    f: u64,
    e: i32,
}

impl DiyFp {
    const fn new(f: u64, e: i32) -> Self {
        Self { f, e }
    }

    /// x - y, for x.e == y.e and x.f >= y.f.
    fn sub(x: DiyFp, y: DiyFp) -> DiyFp {
        DiyFp::new(x.f.wrapping_sub(y.f), x.e)
    }

    /// x * y, rounded: only the upper 64 bits are returned.
    fn mul(x: DiyFp, y: DiyFp) -> DiyFp {
        let u_lo = x.f & 0xFFFF_FFFF;
        let u_hi = x.f >> 32;
        let v_lo = y.f & 0xFFFF_FFFF;
        let v_hi = y.f >> 32;

        let p0 = u_lo.wrapping_mul(v_lo);
        let p1 = u_lo.wrapping_mul(v_hi);
        let p2 = u_hi.wrapping_mul(v_lo);
        let p3 = u_hi.wrapping_mul(v_hi);

        let p0_hi = p0 >> 32;
        let p1_lo = p1 & 0xFFFF_FFFF;
        let p1_hi = p1 >> 32;
        let p2_lo = p2 & 0xFFFF_FFFF;
        let p2_hi = p2 >> 32;

        let mut q = p0_hi.wrapping_add(p1_lo).wrapping_add(p2_lo);
        // Round, ties up: only the highest bit of the low half is needed.
        q = q.wrapping_add(1_u64 << (64 - 32 - 1));
        let h = p3
            .wrapping_add(p2_hi)
            .wrapping_add(p1_hi)
            .wrapping_add(q >> 32);
        DiyFp::new(h, x.e + y.e + 64)
    }

    /// Normalised so that the significand is >= 2^63; x.f != 0.
    fn normalize(mut x: DiyFp) -> DiyFp {
        while (x.f >> 63) == 0 {
            x.f <<= 1;
            x.e -= 1;
        }
        x
    }

    /// Normalised to the exponent `target_exponent`.
    fn normalize_to(x: DiyFp, target_exponent: i32) -> DiyFp {
        let delta = x.e - target_exponent;
        DiyFp::new(x.f << delta, target_exponent)
    }
}

struct Boundaries {
    w: DiyFp,
    minus: DiyFp,
    plus: DiyFp,
}

/// The normalised diyfp of `value` (finite and positive) and its boundaries
/// m- and m+.
fn compute_boundaries(value: f64) -> Boundaries {
    const PRECISION: i32 = 53; // p, the hidden bit included
    const BIAS: i32 = 1024 - 1 + (PRECISION - 1);
    const MIN_EXP: i32 = 1 - BIAS;
    const HIDDEN_BIT: u64 = 1 << (PRECISION - 1);

    let bits = value.to_bits();
    let exponent = bits >> (PRECISION - 1);
    let fraction = bits & (HIDDEN_BIT - 1);

    let is_denormal = exponent == 0;
    let v = if is_denormal {
        DiyFp::new(fraction, MIN_EXP)
    } else {
        DiyFp::new(fraction + HIDDEN_BIT, exponent as i32 - BIAS)
    };

    // m- = (v- + v) / 2 and m+ = (v + v+) / 2: all real numbers strictly
    // between them round to v.
    let lower_boundary_is_closer = fraction == 0 && exponent > 1;
    let m_plus = DiyFp::new(2 * v.f + 1, v.e - 1);
    let m_minus = if lower_boundary_is_closer {
        DiyFp::new(4 * v.f - 1, v.e - 2)
    } else {
        DiyFp::new(2 * v.f - 1, v.e - 1)
    };

    let w_plus = DiyFp::normalize(m_plus);
    let w_minus = DiyFp::normalize_to(m_minus, w_plus.e);
    Boundaries {
        w: DiyFp::normalize(v),
        minus: w_minus,
        plus: w_plus,
    }
}

const ALPHA: i32 = -60;
const GAMMA: i32 = -32;
// nlohmann's static_asserts on the two: the digit generation needs the
// exponent of the scaled value in [-60, -32].
const _: () = assert!(ALPHA >= -60 && GAMMA <= -32);

#[derive(Clone, Copy)]
struct CachedPower {
    f: u64,
    e: i32,
    k: i32,
}

const fn cached(f: u64, e: i32, k: i32) -> CachedPower {
    CachedPower { f, e, k }
}

/// The normalised powers of ten 10^-300 ... 10^324 in steps of 8, from the
/// reference implementation.
const CACHED_POWERS: [CachedPower; 79] = [
    cached(0xAB70FE17C79AC6CA, -1060, -300),
    cached(0xFF77B1FCBEBCDC4F, -1034, -292),
    cached(0xBE5691EF416BD60C, -1007, -284),
    cached(0x8DD01FAD907FFC3C, -980, -276),
    cached(0xD3515C2831559A83, -954, -268),
    cached(0x9D71AC8FADA6C9B5, -927, -260),
    cached(0xEA9C227723EE8BCB, -901, -252),
    cached(0xAECC49914078536D, -874, -244),
    cached(0x823C12795DB6CE57, -847, -236),
    cached(0xC21094364DFB5637, -821, -228),
    cached(0x9096EA6F3848984F, -794, -220),
    cached(0xD77485CB25823AC7, -768, -212),
    cached(0xA086CFCD97BF97F4, -741, -204),
    cached(0xEF340A98172AACE5, -715, -196),
    cached(0xB23867FB2A35B28E, -688, -188),
    cached(0x84C8D4DFD2C63F3B, -661, -180),
    cached(0xC5DD44271AD3CDBA, -635, -172),
    cached(0x936B9FCEBB25C996, -608, -164),
    cached(0xDBAC6C247D62A584, -582, -156),
    cached(0xA3AB66580D5FDAF6, -555, -148),
    cached(0xF3E2F893DEC3F126, -529, -140),
    cached(0xB5B5ADA8AAFF80B8, -502, -132),
    cached(0x87625F056C7C4A8B, -475, -124),
    cached(0xC9BCFF6034C13053, -449, -116),
    cached(0x964E858C91BA2655, -422, -108),
    cached(0xDFF9772470297EBD, -396, -100),
    cached(0xA6DFBD9FB8E5B88F, -369, -92),
    cached(0xF8A95FCF88747D94, -343, -84),
    cached(0xB94470938FA89BCF, -316, -76),
    cached(0x8A08F0F8BF0F156B, -289, -68),
    cached(0xCDB02555653131B6, -263, -60),
    cached(0x993FE2C6D07B7FAC, -236, -52),
    cached(0xE45C10C42A2B3B06, -210, -44),
    cached(0xAA242499697392D3, -183, -36),
    cached(0xFD87B5F28300CA0E, -157, -28),
    cached(0xBCE5086492111AEB, -130, -20),
    cached(0x8CBCCC096F5088CC, -103, -12),
    cached(0xD1B71758E219652C, -77, -4),
    cached(0x9C40000000000000, -50, 4),
    cached(0xE8D4A51000000000, -24, 12),
    cached(0xAD78EBC5AC620000, 3, 20),
    cached(0x813F3978F8940984, 30, 28),
    cached(0xC097CE7BC90715B3, 56, 36),
    cached(0x8F7E32CE7BEA5C70, 83, 44),
    cached(0xD5D238A4ABE98068, 109, 52),
    cached(0x9F4F2726179A2245, 136, 60),
    cached(0xED63A231D4C4FB27, 162, 68),
    cached(0xB0DE65388CC8ADA8, 189, 76),
    cached(0x83C7088E1AAB65DB, 216, 84),
    cached(0xC45D1DF942711D9A, 242, 92),
    cached(0x924D692CA61BE758, 269, 100),
    cached(0xDA01EE641A708DEA, 295, 108),
    cached(0xA26DA3999AEF774A, 322, 116),
    cached(0xF209787BB47D6B85, 348, 124),
    cached(0xB454E4A179DD1877, 375, 132),
    cached(0x865B86925B9BC5C2, 402, 140),
    cached(0xC83553C5C8965D3D, 428, 148),
    cached(0x952AB45CFA97A0B3, 455, 156),
    cached(0xDE469FBD99A05FE3, 481, 164),
    cached(0xA59BC234DB398C25, 508, 172),
    cached(0xF6C69A72A3989F5C, 534, 180),
    cached(0xB7DCBF5354E9BECE, 561, 188),
    cached(0x88FCF317F22241E2, 588, 196),
    cached(0xCC20CE9BD35C78A5, 614, 204),
    cached(0x98165AF37B2153DF, 641, 212),
    cached(0xE2A0B5DC971F303A, 667, 220),
    cached(0xA8D9D1535CE3B396, 694, 228),
    cached(0xFB9B7CD9A4A7443C, 720, 236),
    cached(0xBB764C4CA7A44410, 747, 244),
    cached(0x8BAB8EEFB6409C1A, 774, 252),
    cached(0xD01FEF10A657842C, 800, 260),
    cached(0x9B10A4E5E9913129, 827, 268),
    cached(0xE7109BFBA19C0C9D, 853, 276),
    cached(0xAC2820D9623BF429, 880, 284),
    cached(0x80444B5E7AA7CF85, 907, 292),
    cached(0xBF21E44003ACDD2D, 933, 300),
    cached(0x8E679C2F5E44FF8F, 960, 308),
    cached(0xD433179D9C8CB841, 986, 316),
    cached(0x9E19DB92B4E31BA9, 1013, 324),
];

/// The cached power c such that the exponent of w * c lies in [ALPHA, GAMMA]
/// for a normalised w of exponent `e`.
fn cached_power_for_binary_exponent(e: i32) -> CachedPower {
    const CACHED_POWERS_MIN_DEC_EXP: i32 = -300;
    const CACHED_POWERS_DEC_STEP: i32 = 8;
    // The same k as ceil((ALPHA - e - 1) * log10(2)) for |e| <= 1500, without
    // floating point: log10(2) ~= 78913 / 2^18. Integer division truncates
    // towards zero in both languages.
    let f = ALPHA - e - 1;
    let k = (f * 78913) / (1 << 18) + i32::from(f > 0);
    let index =
        (-CACHED_POWERS_MIN_DEC_EXP + k + (CACHED_POWERS_DEC_STEP - 1)) / CACHED_POWERS_DEC_STEP;
    CACHED_POWERS[index as usize]
}

/// For n != 0, k such that 10^(k-1) <= n < 10^k, and 10^(k-1).
fn find_largest_pow10(n: u32) -> (i32, u32) {
    let mut k = 10;
    let mut pow10 = 1_000_000_000;
    while k > 1 && n < pow10 {
        k -= 1;
        pow10 /= 10;
    }
    (k, pow10)
}

/// Decrements the last digit while that takes the decimal closer to w. The
/// tests are written in this order to avoid overflow in unsigned arithmetic.
fn grisu2_round(buffer: &mut [u8], dist: u64, delta: u64, mut rest: u64, ten_k: u64) {
    let last = buffer.len() - 1;
    while rest < dist
        && delta.wrapping_sub(rest) >= ten_k
        && (rest.wrapping_add(ten_k) < dist
            || dist.wrapping_sub(rest) > rest.wrapping_add(ten_k).wrapping_sub(dist))
    {
        buffer[last] -= 1;
        rest = rest.wrapping_add(ten_k);
    }
}

/// Generates V = buffer * 10^decimal_exponent with M- <= V <= M+; M- and M+
/// share an exponent in [-60, -32].
fn grisu2_digit_gen(
    buffer: &mut Vec<u8>,
    decimal_exponent: &mut i32,
    m_minus: DiyFp,
    w: DiyFp,
    m_plus: DiyFp,
) {
    let mut delta = DiyFp::sub(m_plus, m_minus).f;
    let mut dist = DiyFp::sub(m_plus, w).f;

    // M+ = p1 + p2 * 2^e, p1 the integral part and p2 the fraction.
    let shift = (-m_plus.e) as u32;
    let one = DiyFp::new(1_u64 << shift, m_plus.e);
    let mut p1 = (m_plus.f >> shift) as u32;
    let mut p2 = m_plus.f & (one.f - 1);

    // 1) The digits of the integral part, stopping as soon as the rest is
    // within delta.
    let (k, mut pow10) = find_largest_pow10(p1);
    let mut n = k;
    while n > 0 {
        let d = p1 / pow10;
        let r = p1 % pow10;
        buffer.push(b'0' + d as u8);
        p1 = r;
        n -= 1;
        let rest = (u64::from(p1) << shift).wrapping_add(p2);
        if rest <= delta {
            *decimal_exponent += n;
            // pow10 = 10^n is now one unit in the last place of V.
            let ten_n = u64::from(pow10) << shift;
            grisu2_round(buffer, dist, delta, rest, ten_n);
            return;
        }
        pow10 /= 10;
    }

    // 2) The digits of the fractional part, until the remainder is within
    // delta (scaled by 10 with each digit).
    let mut m = 0;
    loop {
        p2 = p2.wrapping_mul(10);
        let d = p2 >> shift;
        let r = p2 & (one.f - 1);
        buffer.push(b'0' + d as u8);
        p2 = r;
        m += 1;
        delta = delta.wrapping_mul(10);
        dist = dist.wrapping_mul(10);
        if p2 <= delta {
            break;
        }
    }
    *decimal_exponent -= m;
    // One unit in the last place is now 10^-m, which is one.f with delta and
    // dist scaled by 10^m.
    let ten_m = one.f;
    grisu2_round(buffer, dist, delta, p2, ten_m);
}

/// The digits and decimal exponent of `value`, finite and positive.
fn grisu2(value: f64) -> (Vec<u8>, i32) {
    let w = compute_boundaries(value);
    let cached = cached_power_for_binary_exponent(w.plus.e);
    let c_minus_k = DiyFp::new(cached.f, cached.e);

    let w_scaled = DiyFp::mul(w.w, c_minus_k);
    let w_minus = DiyFp::mul(w.minus, c_minus_k);
    let w_plus = DiyFp::mul(w.plus, c_minus_k);

    // The products are off by less than an ulp each way; any number in
    // [M-, M+] rounds to w when read back.
    let m_minus = DiyFp::new(w_minus.f.wrapping_add(1), w_minus.e);
    let m_plus = DiyFp::new(w_plus.f.wrapping_sub(1), w_plus.e);

    let mut decimal_exponent = -cached.k;
    let mut buffer = Vec::with_capacity(17);
    grisu2_digit_gen(
        &mut buffer,
        &mut decimal_exponent,
        m_minus,
        w_scaled,
        m_plus,
    );
    (buffer, decimal_exponent)
}

/// At least two exponent digits, as printf("%g") writes them.
fn append_exponent(out: &mut String, exponent: i32) {
    out.push(if exponent < 0 { '-' } else { '+' });
    let k = exponent.unsigned_abs();
    if k < 10 {
        out.push('0');
        out.push(char::from(b'0' + k as u8));
    } else if k < 100 {
        out.push(char::from(b'0' + (k / 10) as u8));
        out.push(char::from(b'0' + (k % 10) as u8));
    } else {
        out.push(char::from(b'0' + (k / 100) as u8));
        out.push(char::from(b'0' + (k % 100 / 10) as u8));
        out.push(char::from(b'0' + (k % 10) as u8));
    }
}

/// v = digits * 10^decimal_exponent, in fixed notation in [10^min_exp,
/// 10^max_exp) and in exponential notation otherwise.
fn format_buffer(
    out: &mut String,
    digits: &[u8],
    decimal_exponent: i32,
    min_exp: i32,
    max_exp: i32,
) {
    let k = digits.len() as i32;
    let n = k + decimal_exponent;
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();

    if k <= n && n <= max_exp {
        // digits[000].0 - made to look like a floating-point number.
        out.push_str(&text(digits));
        out.push_str(&"0".repeat((n - k) as usize));
        out.push_str(".0");
        return;
    }
    if 0 < n && n <= max_exp {
        // dig.its
        out.push_str(&text(&digits[..n as usize]));
        out.push('.');
        out.push_str(&text(&digits[n as usize..]));
        return;
    }
    if min_exp < n && n <= 0 {
        // 0.[000]digits
        out.push_str("0.");
        out.push_str(&"0".repeat((-n) as usize));
        out.push_str(&text(digits));
        return;
    }
    if k == 1 {
        // dE+123
        out.push_str(&text(digits));
    } else {
        // d.igitsE+123
        out.push_str(&text(&digits[..1]));
        out.push('.');
        out.push_str(&text(&digits[1..]));
    }
    out.push('e');
    append_exponent(out, n - 1);
}

/// nlohmann::detail::to_chars for a finite double.
pub(super) fn to_chars(mut value: f64) -> String {
    let mut out = String::new();
    // signbit rather than < 0, so that -0 keeps its sign.
    if value.is_sign_negative() {
        value = -value;
        out.push('-');
    }
    if value == 0.0 {
        // Made to look like a floating-point number (nlohmann #362, #378).
        out.push_str("0.0");
        return out;
    }
    let (digits, decimal_exponent) = grisu2(value);
    // Like printf("%.*g"), with digits10 (15) as the largest fixed exponent
    // for compatibility with nlohmann's version 2.
    const MIN_EXP: i32 = -4;
    const MAX_EXP: i32 = 15;
    format_buffer(&mut out, &digits, decimal_exponent, MIN_EXP, MAX_EXP);
    out
}
