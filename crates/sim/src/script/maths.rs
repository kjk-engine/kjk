//! The scripts' maths routines (`cos`, `sin`, `tan`, `acos`, `asin`,
//! `atan`, `log`, `pow`, `sqrt`) as both executables' one handler for them
//! works them: angles in degrees, turned to radians by `f32` π/180 widened
//! to a `double`, and back by `f32` 180/π after the result is an `f32`; the
//! C library's `double` functions between (`collision::libm`, the same bits
//! on every platform), the result rounded to an `f32`. Out of a function's
//! domain the handler gives 0 rather than calling it: `acos` and `asin`
//! past ±1 (NaN too), `log` and `sqrt` of 0 or less (NaN too), `pow` of 0
//! to a negative power (or NaN).

use collision::libm;

/// `f32` π/180, as the executables multiply a script's degrees by it.
const RADIANS_PER_DEGREE: f32 = std::f32::consts::PI / 180.0;
/// `f32` 180/π, as they multiply an inverse function's `f32` result by it.
const DEGREES_PER_RADIAN: f32 = 180.0 / std::f32::consts::PI;

/// One of the maths routines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Maths {
    Cos,
    Sin,
    Tan,
    Acos,
    Asin,
    Atan,
    Log,
    Pow,
    Sqrt,
}

impl Maths {
    /// The routine of `nwscript.nss`'s name.
    pub(super) fn named(name: &str) -> Option<Maths> {
        Some(match name {
            "cos" => Maths::Cos,
            "sin" => Maths::Sin,
            "tan" => Maths::Tan,
            "acos" => Maths::Acos,
            "asin" => Maths::Asin,
            "atan" => Maths::Atan,
            "log" => Maths::Log,
            "pow" => Maths::Pow,
            "sqrt" => Maths::Sqrt,
            _ => return None,
        })
    }

    /// What the routine gives for `x` (and `pow`'s exponent `y`).
    pub(super) fn of(self, x: f32, y: f32) -> f32 {
        let wide = f64::from(x);
        let radians = wide * f64::from(RADIANS_PER_DEGREE);
        let in_unit = x.abs() <= 1.0;
        let degrees = |radians: f64| radians as f32 * DEGREES_PER_RADIAN;
        match self {
            Maths::Cos => libm::cos(radians) as f32,
            Maths::Sin => libm::sin(radians) as f32,
            Maths::Tan => libm::tan(radians) as f32,
            Maths::Acos if in_unit => degrees(libm::acos(wide)),
            Maths::Asin if in_unit => degrees(libm::asin(wide)),
            Maths::Atan => degrees(libm::atan(wide)),
            Maths::Log if x > 0.0 => libm::log(wide) as f32,
            // A zero base's exponent must be 0 or more (not NaN).
            Maths::Pow if x != 0.0 || y >= 0.0 => libm::pow(wide, f64::from(y)) as f32,
            Maths::Sqrt if x > 0.0 => x.sqrt(),
            _ => 0.0,
        }
    }
}
