//! The C library's `double` functions the games' executables call for
//! scripts' maths (`sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `log`,
//! `pow`), built from `+ - * /`, `sqrt`, comparisons and bit reads alone,
//! as `trig` is, so every platform computes the same bits. Each is within a
//! few units in the last place of an `f64` of the true value: far under the
//! `f32` step the executables round the result to, so the `f32` a script
//! reads is the one the executables' library gives (the approved probes'
//! `script maths` check it on the routines' own inputs).

/// π/2 in three parts (each a few bits short of an `f64`, so a whole number
/// of quarter turns times the first two is exact up to about 2^20 turns).
const HALF_PI_1: f64 = 1.570_796_326_734_125_6;
const HALF_PI_2: f64 = 6.077_100_506_303_966e-11;
const HALF_PI_3: f64 = 2.022_266_248_795_950_6e-21;
const HALF_PI: f64 = std::f64::consts::FRAC_PI_2;
const PI: f64 = std::f64::consts::PI;
/// ln 2 in two parts (the first's last bits clear, so a whole multiple of it
/// is exact).
const LN2_HI: f64 = 6.931_471_803_691_238e-1;
const LN2_LO: f64 = 1.908_214_929_270_587_7e-10;
const LN2: f64 = std::f64::consts::LN_2;
const SQRT_2: f64 = std::f64::consts::SQRT_2;

/// 1 / n!, exact up to n = 22 (whose odd part still fits an `f64`).
const fn inverse_factorial(n: u32) -> f64 {
    let mut product = 1.0;
    let mut k = 2;
    while k <= n {
        product *= k as f64;
        k += 1;
    }
    1.0 / product
}

/// Taylor coefficients: sine's odd ones to x^21, cosine's even ones to x^22,
/// exp's to x^16; on the reduced ranges the first dropped terms are below
/// 1e-22.
const SIN: [f64; 11] = taylor(1);
const COS: [f64; 12] = taylor(0);
const EXP: [f64; 17] = taylor_all();

const fn taylor<const N: usize>(first: u32) -> [f64; N] {
    let mut out = [0.0; N];
    let mut i = 0;
    while i < N {
        let n = first + 2 * i as u32;
        let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
        out[i] = sign * inverse_factorial(n);
        i += 1;
    }
    out
}

const fn taylor_all<const N: usize>() -> [f64; N] {
    let mut out = [0.0; N];
    let mut i = 0;
    while i < N {
        out[i] = inverse_factorial(i as u32);
        i += 1;
    }
    out
}

/// `coefficients` summed as a polynomial in `x` (Horner).
fn polynomial(coefficients: &[f64], x: f64) -> f64 {
    coefficients.iter().rev().fold(0.0, |sum, c| sum * x + c)
}

/// Up to how many quarter turns `quarters` takes them off by the parts of
/// π/2 (each part times so many exact).
const NEAR_TURNS: f64 = (1u32 << 19) as f64;

/// The low bits of π/2's last part (`HALF_PI` + this is π/2 to about 2^-108).
const HALF_PI_LOW: f64 = 6.123_233_995_736_766e-17;

/// 2/π to 1216 bits below the point (⌊2/π · 2^1216⌋), its 64-bit words
/// lowest first: a finite `f64` times it leaves its whole quarter turns and
/// over 128 bits of what remains.
const TWO_OVER_PI: [u64; 19] = [
    0x5603_3046_fc7b_6bab,
    0x6bfb_5fb1_1f8d_5d08,
    0x3d07_39f7_8a52_92ea,
    0x7527_bac7_ebe5_f17b,
    0x4f46_3f66_9e5f_ea2d,
    0x6d36_7ecf_27cb_09b7,
    0xef2f_118b_5a0a_6d1f,
    0x1ff8_97ff_de05_980f,
    0x9c84_5f8b_bdf9_283b,
    0x3991_d639_8353_39f4,
    0xe99c_7026_b45f_7e41,
    0xe882_35f5_2ebb_4484,
    0xfe1d_eb1c_b129_a73e,
    0x0649_2eea_09d1_921c,
    0xb724_6e3a_424d_d2e0,
    0xfe51_63ab_debb_c561,
    0xdb62_9599_3c43_9041,
    0xfc27_57d1_f534_ddc0,
    0xa2f9_836e_4e44_1529,
];
const TWO_OVER_PI_BITS: i64 = 1216;

/// `x` less its nearest whole number of quarter turns, and that number
/// modulo 4: near, by the parts of π/2; farther, from `x`'s exact product
/// with `TWO_OVER_PI`, as the C libraries reduce a large angle.
fn quarters(x: f64) -> (f64, i64) {
    let k = (x / HALF_PI).round_ties_even();
    if k.abs() < NEAR_TURNS {
        let r = ((x - k * HALF_PI_1) - k * HALF_PI_2) - k * HALF_PI_3;
        return (r, (k as i64).rem_euclid(4));
    }
    // x = m · 2^e, m a whole number below 2^53 (x is normal this far out).
    let bits = x.abs().to_bits();
    let m = (bits & ((1u64 << 52) - 1)) | (1u64 << 52);
    let e = ((bits >> 52) & 0x7ff) as i64 - 1075;
    let mut product = [0u64; 20];
    let mut carry = 0u128;
    for (word, out) in TWO_OVER_PI.iter().zip(product.iter_mut()) {
        let sum = u128::from(*word) * u128::from(m) + carry;
        *out = sum as u64;
        carry = sum >> 64;
    }
    product[19] = carry as u64;
    // The product's bits from `start`, `count` of them (at most 128).
    let take = |start: i64, count: u32| -> u128 {
        (0..count).fold(0u128, |sum, i| {
            let at = start + i64::from(i);
            let bit = if (0..1280).contains(&at) {
                (product[(at / 64) as usize] >> (at % 64)) & 1
            } else {
                0
            };
            sum | (u128::from(bit) << i)
        })
    };
    // The point: below it the fraction of a quarter turn, above it whole
    // ones (two bits of them are all that count).
    let point = TWO_OVER_PI_BITS - e;
    let mut turns = take(point, 2) as i64;
    let mut fraction = take(point - 128, 128);
    let negative = fraction >> 127 == 1;
    if negative {
        fraction = fraction.wrapping_neg();
        turns += 1;
    }
    let high = (fraction >> 75) as f64 * f64::from_bits((1023 - 53) << 52);
    let low = (fraction & ((1u128 << 75) - 1)) as f64 * f64::from_bits((1023 - 128) << 52);
    let r = high * HALF_PI + (low * HALF_PI + high * HALF_PI_LOW);
    let r = if negative { -r } else { r };
    if x < 0.0 {
        (-r, (-turns).rem_euclid(4))
    } else {
        (r, turns.rem_euclid(4))
    }
}

/// Sine and cosine of `x` radians; not finite: NaN.
fn sin_cos(x: f64) -> (f64, f64) {
    if !x.is_finite() {
        return (f64::NAN, f64::NAN);
    }
    let (r, quarter) = quarters(x);
    let r2 = r * r;
    let sin = r * polynomial(&SIN, r2);
    let cos = polynomial(&COS, r2);
    match quarter {
        0 => (sin, cos),
        1 => (cos, -sin),
        2 => (-sin, -cos),
        _ => (-cos, sin),
    }
}

/// `sin(x)`, x in radians.
pub fn sin(x: f64) -> f64 {
    sin_cos(x).0
}

/// `cos(x)`, x in radians.
pub fn cos(x: f64) -> f64 {
    sin_cos(x).1
}

/// `tan(x)`, x in radians.
pub fn tan(x: f64) -> f64 {
    let (sin, cos) = sin_cos(x);
    sin / cos
}

/// `atan(x)` in radians, in [-π/2, π/2]: past 1 from π/2 less the angle of
/// its inverse; the angle halved twice (`atan t = 2 atan(t / (1 + sqrt(1 +
/// t²)))`, to at most 11.25°), then the series to the 23rd power.
pub fn atan(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    let steep = x.abs() > 1.0;
    let mut t = if steep { 1.0 / x.abs() } else { x.abs() };
    for _ in 0..2 {
        t /= 1.0 + (1.0 + t * t).sqrt();
    }
    let t2 = t * t;
    let series: f64 = (0..12).rev().fold(0.0, |sum, k| {
        let odd = f64::from(2 * k + 1);
        let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
        sum * t2 + sign / odd
    }) * t;
    let mut angle = 4.0 * series;
    if steep {
        angle = HALF_PI - angle;
    }
    angle.copysign(x)
}

/// `asin(x)` in radians; past ±1: NaN.
pub fn asin(x: f64) -> f64 {
    match x.abs() {
        a if a.is_nan() || a > 1.0 => f64::NAN,
        1.0 => HALF_PI.copysign(x),
        // (1 - x)(1 + x): exact for an `f32`'s x.
        _ => atan(x / ((1.0 - x) * (1.0 + x)).sqrt()),
    }
}

/// `acos(x)` in radians, in [0, π]; past ±1: NaN.
pub fn acos(x: f64) -> f64 {
    match x {
        x if x.is_nan() || x.abs() > 1.0 => f64::NAN,
        -1.0 => PI,
        _ => 2.0 * atan(((1.0 - x) / (1.0 + x)).sqrt()),
    }
}

/// `log(x)`, the natural logarithm: x = m·2^e with m in [√½, √2), ln m as
/// 2 atanh((m - 1)/(m + 1)) summed to the 25th power. Zero: -∞; below zero
/// or NaN: NaN; +∞: +∞.
pub fn log(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() {
        return x;
    }
    // Subnormals scaled into the normal range first (by 2^54).
    let (x, shift) = if x < f64::MIN_POSITIVE {
        (x * f64::from_bits((1023 + 54) << 52), -54)
    } else {
        (x, 0)
    };
    let bits = x.to_bits();
    let mut e = ((bits >> 52) & 0x7ff) as i64 - 1023 + shift;
    let mut m = f64::from_bits((bits & ((1u64 << 52) - 1)) | (1023u64 << 52));
    if m > SQRT_2 {
        m /= 2.0;
        e += 1;
    }
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let series = (0..13)
        .rev()
        .fold(0.0, |sum, k| sum * s2 + 1.0 / f64::from(2 * k + 1))
        * s;
    let e = e as f64;
    e * LN2_HI + (e * LN2_LO + 2.0 * series)
}

/// `exp(x)`: x = k ln 2 + r with |r| ≤ ½ ln 2, e^r's series times 2^k.
fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    // Past an `f64`'s range.
    if x > 709.8 {
        return f64::INFINITY;
    }
    if x < -745.2 {
        return 0.0;
    }
    let k = (x / LN2).round_ties_even();
    let r = (x - k * LN2_HI) - k * LN2_LO;
    let k = k as i32;
    // 2^k in two halves, each within an `f64`'s exponents.
    let half = k / 2;
    let scale = |n: i32| f64::from_bits(((i64::from(n) + 1023) as u64) << 52);
    polynomial(&EXP, r) * scale(half) * scale(k - half)
}

/// Whether `y` is a whole number, and if so whether it is odd.
fn whole(y: f64) -> Option<bool> {
    (y.is_finite() && y == y.trunc()).then(|| (y / 2.0).trunc() * 2.0 != y)
}

/// `pow(x, y)` as C99's annex F has it at its edges (zeros, infinities, a
/// negative base), else e^(y ln x); a power of 1, 2 or -1 exactly.
pub fn pow(x: f64, y: f64) -> f64 {
    if y == 0.0 || x == 1.0 {
        return 1.0;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let odd = whole(y) == Some(true);
    if y.is_infinite() {
        return match (x.abs() == 1.0, x.abs() < 1.0, y > 0.0) {
            (true, _, _) => 1.0,
            (_, true, true) | (_, false, false) => 0.0,
            _ => f64::INFINITY,
        };
    }
    if x == 0.0 || x.is_infinite() {
        let big = (x == 0.0) == (y < 0.0);
        let size = if big { f64::INFINITY } else { 0.0 };
        return if odd { size.copysign(x) } else { size };
    }
    if x < 0.0 {
        return match whole(y) {
            Some(odd) => {
                let size = pow(-x, y);
                if odd { -size } else { size }
            }
            None => f64::NAN,
        };
    }
    whole(y)
        .filter(|_| y.abs() <= WHOLE_POWER_MOST)
        .and_then(|_| whole_power(x, y as i64))
        .unwrap_or_else(|| exp(y * log(x)))
}

/// The largest whole power `pow` raises to by multiplying (past it, by
/// `exp` and `log`).
const WHOLE_POWER_MOST: f64 = (1u32 << 30) as f64;

/// How far from 1 a double-double's head may grow while multiplying (past
/// it, its splitting overflows).
const WHOLE_POWER_RANGE: f64 = 1e150;

/// `x` (positive, finite) to the whole power `n`, multiplied out as
/// double-doubles: to about 2^-100, so its `f64` is the correctly rounded
/// one, as the C libraries' `pow` gives a whole power exactly where it can
/// (a whole number cubed lands on the halfway point between two `f32`s
/// often; the exact product rounds the way the games' does). `None` when it
/// grows too far or too small to multiply.
fn whole_power(x: f64, n: i64) -> Option<f64> {
    let mut base = (x, 0.0);
    let mut out = (1.0, 0.0);
    let mut left = n.unsigned_abs();
    while left > 0 {
        if left & 1 == 1 {
            out = times(out, base);
        }
        left >>= 1;
        if left > 0 {
            base = times(base, base);
        }
        let fits = |(head, _): (f64, f64)| {
            (1.0 / WHOLE_POWER_RANGE..WHOLE_POWER_RANGE).contains(&head.abs())
        };
        if !fits(out) || !fits(base) {
            return None;
        }
    }
    if n >= 0 {
        return Some(out.0 + out.1);
    }
    // 1 / out, with one correction step.
    let q = 1.0 / out.0;
    let (p, e) = two_product(q, out.0);
    let rest = ((1.0 - p) - e) - q * out.1;
    Some(q + q * rest)
}

/// Two double-doubles' product (`head + tail`, the tail under half a unit
/// of the head's last place).
fn times(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    let (p, e) = two_product(a.0, b.0);
    let e = e + (a.0 * b.1 + a.1 * b.0);
    let head = p + e;
    (head, e - (head - p))
}

/// `a × b` as its rounded product and the exact error of it (Dekker's,
/// with Veltkamp's split: no fused multiply-add).
fn two_product(a: f64, b: f64) -> (f64, f64) {
    let split = |v: f64| {
        let c = SPLIT * v;
        let high = c - (c - v);
        (high, v - high)
    };
    let p = a * b;
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    (p, ((ah * bh - p) + ah * bl + al * bh) + al * bl)
}

/// Veltkamp's splitting constant, 2^27 + 1.
const SPLIT: f64 = 134_217_729.0;
