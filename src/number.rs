

use std::ops::Rem;

use num_complex::Complex64;
use crate::MacroError;

/// A convenience type for numbers.
#[derive(Debug, Copy, Clone)]
pub enum Number {
    /// A basic i64 type.
    Integer(i64),
    /// A basic f64 type.
    Float(f64),
    /// A complex number, with real and imaginary parts.
    Complex(Complex64)
}

impl Number {
    /// Zero.
    pub const ZERO: Number = Number::Integer(0);
    /// One.
    pub const ONE: Number = Number::Integer(1);
}

fn parse_number(mut value: &[u8]) -> Result<Number, MacroError> {
    if value == b"inf" {
        return Ok(Number::Float(f64::INFINITY))
    } else if value == b"-inf" {
        return Ok(Number::Float(f64::NEG_INFINITY))
    } else if value == b"nan" || value == b"NaN" {
        return Ok(Number::Float(f64::NAN))
    }

    let mut negative = false;
    if value.starts_with(b"+") {
        value = &value[1..];
    } else if value.starts_with(b"-") {
        value = &value[1..];
        negative = true;
    }

    macro_rules! impl_intpref {
        ($lit: literal, $base: literal) => {
            if value.starts_with($lit) {
                return str::from_utf8(&value)
                    .map_err(|_| MacroError::from("string is not valid UTF-8"))
                    .and_then(|v| i64::from_str_radix(v, $base).map_err(|e| MacroError::from(format!("{e}"))))
                    .map(Number::Integer);
            }
        }
    }
    impl_intpref!(b"0x", 16);
    impl_intpref!(b"0b", 2);
    impl_intpref!(b"0o", 8);

    str::from_utf8(&value)
        .map_err(|_| MacroError::from("string is not valid UTF-8"))
        .and_then(
            |v| v.parse::<i64>().map(Number::Integer).map_err(|_| ()).or_else(
                |_| v.parse::<f64>().map(Number::Float).map_err(|_| MacroError::from(format!("invalid number: {}", String::from_utf8_lossy(value)))
            )))
        .map(|v| if negative {-v} else {v})
}

impl TryFrom<&[u8]> for Number {
    type Error = MacroError;
    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let Some(last) = value.last() else {
            return Err(MacroError::from("cannot convert empty string to number"))
        };
        if *last == b'j' {
            let Some(second_number_start) = value.iter().rposition(|c| *c == b'+' || *c == b'-').filter(|i| *i != 0) else {
                let num = parse_number(&value[..value.len() - 1])?;
                return Ok(Number::Complex(Complex64::new(0.0, num.into())));
            };
            let real = parse_number(&value[..second_number_start])?;
            let imag = parse_number(&value[second_number_start .. value.len() - 1])?;
            return Ok(Number::Complex(Complex64::new(real.into(), imag.into())))
        }
        parse_number(&value)
    }
}

impl std::fmt::Display for Number {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Integer(x) => write!(f, "{x}"),
            Self::Float(x) if x.fract() == 0.0 => write!(f, "{x:.0}"),
            Self::Float(x) => write!(f, "{x}"),
            Self::Complex(c) if c.im.is_sign_positive() => write!(f, "{}+{}j", c.re, c.im),
            Self::Complex(c) => write!(f, "{}-{}j", c.re, c.im),
        }
    }
}

impl From<i64> for Number {
    fn from(val: i64) -> Self { Number::Integer(val) }
}
impl From<f64> for Number {
    fn from(val: f64) -> Self { Number::Float(val) }
}
impl From<Complex64> for Number {
    fn from(val: Complex64) -> Self { Number::Complex(val) }
}

impl From<Number> for i64 {
    fn from(value: Number) -> i64 {
        match value {
            Number::Integer(i) => i,
            Number::Float(f) => f as i64,
            Number::Complex(c) => c.re as i64
        }
    }
}

impl From<Number> for f64 {
    fn from(value: Number) -> f64 {
        match value {
            Number::Integer(i) => i as f64,
            Number::Float(f) => f,
            Number::Complex(c) => c.re
        }
    }
}

impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Number::Integer(a), Number::Integer(b)) => *a == *b,
            (Number::Integer(a), Number::Float(b)) => (*a as f64) == *b,
            (Number::Integer(a), Number::Complex(b)) => Complex64::new(*a as f64, 0.0) == *b,
            (Number::Float(a), Number::Float(b)) => *a == *b,
            (Number::Float(a), Number::Complex(b)) => Complex64::new(*a, 0.0) == *b,
            (Number::Complex(a), Number::Complex(b)) => *a == *b,
            (a, b) => b == a
        }
    }
}

impl PartialOrd for Number {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (_, Number::Complex(_)) | (Number::Complex(_), _) => None,
            (Number::Integer(a), Number::Integer(b)) => Some(a.cmp(b)),
            (Number::Integer(a), Number::Float(b)) => (*a as f64).partial_cmp(b),
            (Number::Float(a), Number::Integer(b)) => a.partial_cmp(&(*b as f64)),
            (Number::Float(a), Number::Float(b)) => a.partial_cmp(b),
        }
    }
}

mod op_impl {
    use std::ops::*;
    use num_complex::Complex64;

    use super::Number;

    impl Add for Number {
        type Output = Self;

        fn add(self, rhs: Self) -> Self::Output {
            match (self, rhs) {
                (Self::Complex(a), Self::Complex(b)) =>
                    Self::Complex(a+b),
                (Self::Complex(a), Self::Float(b)) | (Self::Float(b), Self::Complex(a)) => {
                    Self::Complex(a + Complex64::new(b, 0.0))
                },
                (Self::Complex(a), Self::Integer(b)) | (Self::Integer(b), Self::Complex(a)) => {
                    Self::Complex(a + Complex64::new(b as f64, 0.0))
                },
                (Self::Float(a), Self::Float(b)) =>
                    Self::Float(a + b),
                (Self::Float(a), Self::Integer(b)) | (Self::Integer(b), Self::Float(a)) =>
                    Self::Float(a + b as f64),
                (Self::Integer(a), Self::Integer(b)) => a.checked_add(b).map_or_else(|| Self::Float(a as f64 + b as f64), Self::Integer)
            }
        }
    }
    impl Neg for Number {
        type Output = Self;
        fn neg(self) -> Self::Output {
            match self {
                Self::Integer(a) => Self::Integer(-a),
                Self::Float(a) => Self::Float(-a),
                Self::Complex(a) => Self::Complex(-a),
            }
        }
    }
    impl Sub for Number {
        type Output = Self;

        fn sub(self, rhs: Self) -> Self::Output {
            self + (-rhs)
        }
    }
    impl Mul for Number {
        type Output = Self;
        fn mul(self, rhs: Self) -> Self::Output {
            match (self, rhs) {
                (Self::Integer(a), Self::Integer(b)) =>
                    a.checked_mul(b).map_or_else(|| Self::Float(a as f64 * b as f64), Self::Integer),
                (Self::Float(f), Self::Integer(i)) | (Self::Integer(i), Self::Float(f)) =>
                    Self::Float(f * i as f64),
                (Self::Float(a), Self::Float(b)) => Self::Float(a * b),
                (Self::Complex(c), Self::Integer(i)) | (Self::Integer(i), Self::Complex(c))
                    => Self::Complex(c * i as f64),
                (Self::Complex(c), Self::Float(f)) | (Self::Float(f), Self::Complex(c))
                    => Self::Complex(c * f),
                (Self::Complex(a), Self::Complex(b))
                    => Self::Complex(a * b)
            }
        }
    }
    impl Div for Number {
        type Output = Self;
        fn div(self, rhs: Self) -> Self::Output {
            match (self, rhs) {
                (Self::Integer(a), Self::Integer(b)) if b != 0 && a % b == 0
                    => Self::Integer(a / b),
                (Self::Integer(a), Self::Integer(b)) => Self::Float((a as f64) / (b as f64)),
                (Self::Integer(i), Self::Float(f)) => Self::Float((i as f64) / f),
                (Self::Integer(i), Self::Complex(c)) => Self::Complex((i as f64) / c),
                (Self::Float(f), Self::Integer(i)) => Self::Float(f / (i as f64)),
                (Self::Float(a), Self::Float(b)) =>  Self::Float(a / b),
                (Self::Float(f), Self::Complex(c)) => Self::Complex(f / c),
                (Self::Complex(c), Self::Integer(i)) => Self::Complex(c / (i as f64)),
                (Self::Complex(c), Self::Float(f)) => Self::Complex(c / f),
                (Self::Complex(a), Self::Complex(b)) => Self::Complex(a / b),
            }
        }
    }
    impl Number {
        /// Raises a number to the power of another.
        pub fn pow(self, rhs: Self) -> Self {
            match (self, rhs) {
                (Self::Complex(c), _) |
                (_, Self::Complex(c)) if c.is_nan() => Self::Complex(Complex64::new(f64::NAN, f64::NAN)),
                (Self::Float(f), _) |
                (_, Self::Float(f)) if f.is_nan() => Self::Float(f64::NAN),

                (Self::Integer(a), Self::Integer(b)) if b >= 0 =>
                    u32::try_from(b).ok().and_then(|b| a.checked_pow(b))
                    .map_or_else(|| Self::Float((a as f64).powf(b as f64)), Self::Integer),
                (Self::Integer(a), Self::Integer(b)) => Self::Float((a as f64).powf(b as f64)),
                (Self::Integer(i), Self::Float(f))
                    if i < 0 && f.is_finite() && (f % 1.0 != 0.0) =>
                        // Complex result!
                        Self::Complex(Complex64::new(i as f64, 0.0).powf(f)),
                (Self::Integer(i), Self::Float(f)) =>
                    // Real result
                    Self::Float((i as f64).powf(f)),
                (Self::Float(f), Self::Integer(i)) =>
                    // Cannot be complex, since exp cannot be decimal
                    Self::Float(f.powf(i as f64)),
                (Self::Float(a), Self::Float(b))
                    if a < 0.0 && b.is_finite() && (b % 1.0 != 0.0) =>
                        // Complex result!
                        Self::Complex(Complex64::new(a, 0.0).powf(b)),
                (Self::Float(a), Self::Float(b)) =>
                    // Real result
                    Self::Float(a.powf(b)),
                (Self::Integer(i), Self::Complex(c)) => Self::Complex(Complex64::new(i as f64, 0.0).powc(c)),
                (Self::Float(f), Self::Complex(c)) => Self::Complex(Complex64::new(f, 0.0).powc(c)),
                (Self::Complex(a), Self::Complex(b)) => Self::Complex(a.powc(b)),
                (Self::Complex(c), Self::Integer(i)) => Self::Complex(c.powc(Complex64::new(i as f64, 0.0))),
                (Self::Complex(c), Self::Float(f)) => Self::Complex(c.powc(Complex64::new(f, 0.0))),
            }
        }
        /// Takes the logarithm of a number with the base of another.
        pub fn log(self, rhs: Self) -> Self {
            match (self, rhs) {
                (Self::Complex(a), Self::Complex(b)) => Self::Complex(a.ln() / b.ln()),
                (n, c @ Self::Complex(_)) => (n + Number::Complex(Complex64::ZERO)).log(c),
                (c @ Self::Complex(_), b) => c.log(b + Number::Complex(Complex64::ZERO)),
                (Self::Float(x), Self::Float(b)) if x >= 0.0 => Number::Float(x.log(b)),
                (Self::Float(x), Self::Float(b)) => Number::Complex(Complex64::from(x).log(b)),
                (Self::Integer(a), Self::Integer(b)) if a > 0 && b >= 2 => (a.ilog(b) as i64).into(),
                (Number::Integer(x), Number::Integer(b)) => Number::Float(x as f64).log((b as f64).into()),
                (Number::Integer(x), Number::Float(b)) => Number::Float(x as f64).log(b.into()),
                (Number::Float(x), Number::Integer(b)) => Number::Float(x).log((b as f64).into()),
            }
        }
    }
}

impl Rem for Number {
    type Output = Number;

    fn rem(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::Complex(_), _) | (_, Self::Complex(_)) => Complex64::new(f64::NAN, f64::NAN).into(),
            (_, Number::Integer(i)) if i == 0 => f64::NAN.into(),
            (Self::Integer(a), Self::Integer(b)) => (a % b).into(),
            (Self::Float(a), Self::Integer(b)) => (a % (b as f64)).into(),
            (Self::Integer(a), Self::Float(b)) => ((a as f64) % b).into(),
            (Self::Float(a), Self::Float(b)) => (a % b).into(),
        }
    }
}
