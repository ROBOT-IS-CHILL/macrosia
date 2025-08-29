//! Defines some basic macros for regular use.


use itertools::Itertools as _;
use std::{borrow::Cow, ops::{Add as _, Mul as _}};
use crate::{var_reg::VariableRegistry, Macro, MacroError, Number};
use const_format::concatcp;


macro_rules! count {
    ($tt: tt $($tts: tt)*) => {
        1 + count!($($tts)*)
    };
    () => { 0 }
}

macro_rules! args {
    (($($arg: ident),*) <- $inp: expr) => {
        const I: usize = const { count!($($arg)*) };
        let Ok([$($arg),*]): Result<[_; _], _> = $inp.take(I).collect::<Vec<_>>().try_into() else {
            return Err(concatcp!("incorrect number of arguments (expected ", I, ")").into())
        };
    };
    (($($arg: ident,)* ... $vararg: ident) <- $inp: expr) => {
        const I: usize = const { count!($($arg)*) };
        let mut iter = $inp;
        let Ok([$($arg),*]): Result<[_; _], _> = (&mut iter).take(I).collect::<Vec<_>>().try_into() else {
            return Err(concatcp!("incorrect number of arguments (expected ", I, ")").into())
        };
        let $vararg = iter;
    };
}


macro_rules! def_macro {
    ($($(#[$meta: meta])* $vis: vis macro $sname: ident [ $name: literal ] $args: tt + $x: ident, $v: ident $body: tt)*) => {$(
        $(#[$meta])*
        $vis struct $sname;
        impl Macro for $sname {
            fn name(&self) -> Cow<'static, [u8]> { Cow::Borrowed($name) }
            fn eval<'arg, 'reg: 'arg, 'exec: 'reg>(&self, $x: &'exec crate::exec::Executor, $v: &'reg mut VariableRegistry, args: &mut dyn Iterator<Item = &'arg [u8]>) -> Result<Cow<'static, [u8]>, MacroError> {
                args!($args <- args);
                $body
            }
        }
    )*

    impl crate::exec::Executor {

        /// Adds all standard library macros to the given execution context.
        pub fn with_stdlib(mut self) -> Self {
            $(
                self.add_macro($sname);
            )*
            self
        }
    }

    };
}

/// Unescapes `[`, `/`, and `]` in an 8-bit clean string.
///
/// # Note
/// This function assumes the string does not end with a lone backslash.
fn unescape(str: &[u8]) -> Cow<'_, [u8]> {
    // Optionally contains an owned buffer for the unescaped string if mutation is required
    // If/when this is set to Some(), it will never be set back to None - it's a one-way latch
    let mut construct = None;
    let mut last_escape = false; // Whether the last character was a \
    for (i, c) in str.iter().copied().enumerate() {
        if last_escape {
            last_escape = false;
            match c {
                b'[' | b']' | b'\\' | b'/' => {
                    // Only if we already don't have an unescaped string buffer,
                    // the buffer is backfilled with the string up to before the backslash preceding this character
                    if construct.is_none() {
                        let mut cons = Vec::with_capacity(str.len());
                        cons.extend(&str[..i - 1]);
                        construct = Some(cons);
                    }
                },
                // If the character isn't a valid escape, we need to push a backslash,
                // since we skipped it last loop (see below)
                _ => if let Some(ref mut c) = construct {
                    c.push(b'\\')
                }
            }
            // Note: Code flows into the `if let Some(...)`
        } else if c == b'\\' {
            last_escape = true;
            // Will never push to the buffer!
            continue;
        }
        // If we have a string buffer, we need to push this character to it
        if let Some(ref mut v) = construct {
            v.push(c);
        }
    }
    // At the end of the loop, we can map_or the option into a Cow
    construct.map_or(Cow::Borrowed(str), Cow::Owned)
}

#[cfg(test)]
mod test {
    use super::{unescape, Cow};
    #[test]
    fn test_unescape() {
        macro_rules! check {
            ($a: literal -> borrowed $b: literal) => { { let v = unescape($a); assert!(matches!(v, Cow::Borrowed(_))); assert_eq!(&*v, &*$b); } };
            ($a: literal -> owned $b: literal) => { { let v = unescape($a); assert!(matches!(v, Cow::Owned(_))); assert_eq!(&*v, &*$b); } }
        }
        check!(b"abcde" -> borrowed b"abcde");
        check!(br"a\bcde" -> borrowed br"a\bcde");
        check!(b"abc\\" -> borrowed b"abc\\");
        check!(br"a\[cd\e" -> owned br"a[cd\e");
        check!(b"a\\[bc\\" -> owned br"a[bc");
    }
}

def_macro! {
    /// Adds all arguments, returning their sum.
    /// # Arguments
    /// - \[Variadic\] Any amount of strings coercible to numbers.
    pub macro Add [b"add"] (... args) + _x, _v {
        args
            .map(|v| Number::try_from(&*v))
            .process_results(|it| {
                Cow::Owned(format!("{}", it.fold(Number::ZERO, Number::add)).into_bytes())
            })
    }

    /// Multiplies all arguments, returning their product.
    /// # Arguments
    /// - \[Variadic\] Any amount of strings coercible to numbers.
    pub macro Multiply [b"multiply"] (... args) + _x, _v {
        args
            .map(|v| Number::try_from(&*v))
            .process_results(|it| {
                Cow::Owned(format!("{}", it.fold(Number::ZERO, Number::mul)).into_bytes())
            })
    }

    /// Subtracts the second argument from the first.
    /// # Arguments
    /// 1. The number to subtract from.
    /// 2. The number to subtract.
    pub macro Subtract [b"subtract"] (a, b) + _x, _v {
        let [a, b]: [Number; 2] = [a.try_into()?, b.try_into()?];
        Ok(Cow::Owned(format!("{}", a - b).into_bytes()))
    }

    /// Divides the first argument by the second.
    /// # Arguments
    /// 1. The numerator of the division.
    /// 2. The denominator of the division.
    pub macro Divide [b"divide"] (a, b) + _x, _v {
        let [a, b]: [Number; 2] = [a.try_into()?, b.try_into()?];
        Ok(Cow::Owned(format!("{}", a / b).into_bytes()))
    }

    /// Raises the first argument to the second.
    /// # Arguments
    /// 1. The base of the exponent.
    /// 2. The power of the exponent.
    pub macro Pow [b"pow"] (a, b) + _x, _v {
        let [a, b]: [Number; 2] = [a.try_into()?, b.try_into()?];
        Ok(Cow::Owned(format!("{}", a.pow(b)).into_bytes()))
    }

    /// Unescapes the argument.
    /// # Arguments
    /// 1. The string to unescape. Must be valid UTF-8.
    pub macro Unescape [b"unescape"] (string) + _x, _v {
        let esc = unescape(&*string);
        if let Cow::Owned(s) = esc {
            return Ok(Cow::<'static, [u8]>::Owned(s))
        }
        Ok(Vec::from(string).into())
    }

    /// Stores a variable in the variable registry.
    /// # Arguments
    /// 1. The name to store the variable under.
    /// 2. The value to store in the variable.
    pub macro Store [b"store"] (name, value) + _x, v {
        v.store(&*name, Vec::from(value).into()); // The value could easily outlive the argument, so we clone
        Ok(Cow::Borrowed(b""))
    }

    /// Loads a variable from the variable registry.
    /// # Arguments
    /// 1. The name of the variable to load.
    pub macro Load [b"load"] (name) + _x, v {
        v.load(&*name)
            .map(Vec::from)
            .map(Cow::Owned)
            .ok_or_else(move || format!("variable {} does not exist", String::from_utf8_lossy(&*name)).into())
    }

    /// Loads a variable from the variable registry, or returns the second argument (or an empty string) if it doesn't exist.
    /// # Arguments
    /// 1. The name of the variable to load.
    /// 2. The value to output if the variable does not exist.
    pub macro Get [b"get"] (name, default) + _x, v {
        Ok(
            Vec::from(v.load(&*name).unwrap_or(default)).into()
        )
    }

    /// Oh no. (`[badquine]` - temporary until I implement text macros)
    pub macro BadQuine [b"badquine"] () + _x, _v {
        Ok(
            Cow::Borrowed(b"[badquine]")
        )
    }
    /// Fuck. (`[worsequine]cba` - temporary until I implement text macros)
    pub macro WorseQuine [b"worsequine"] () + _x, _v {
        Ok(
            Cow::Borrowed(b"[worsequine]cba")
        )
    }

    /// Returns a single byte from a hexadecimal value.
    /// # Arguments
    /// 1. The hexadecimal value of the byte to return.
    pub macro Byte [b"byte"] (hex) + _x, _v {
        str::from_utf8(hex).ok()
            .and_then(|s| u8::from_str_radix(s, 16).ok())
            .ok_or("invalid byte".into())
            .map(|v| Cow::Borrowed(std::slice::from_ref(&BYTES[v as usize])))
    }

    /// Returns a single UTF-8 character from a given integer value.
    /// # Arguments
    /// 1. The codepoint of the character to return.
    pub macro Char [b"chr"] (hex) + _x, _v {
        str::from_utf8(hex).ok()
            .and_then(|s| s.parse::<u32>().ok().and_then(char::from_u32))
            .ok_or("invalid character or not an integer".into())
            .map(|chr| {
                let mut v = vec![0; chr.len_utf8()];
                chr.encode_utf8(&mut v);
                Cow::Owned(v)
            })
    }

    /// Replaces a string within another string, using plain string matching.
    /// # Arguments
    /// 1. The string to replace substrings of
    /// 2. The substring to replace
    /// 3. The string to replace the substring with
    /// 4. [Optional] The amount of times to replace
    pub macro SReplace [b"sreplace"] (haystack, needle, value, ...iter) + _x, _v {
        if needle.is_empty() {
            Err("search pattern value cannot be empty")?
        }
        let max_count = iter.next().map(Number::try_from).transpose()?.map(|v| i64::from(v));
        if max_count.is_some_and(|m| m <= 0) || needle.len() > haystack.len() {
            return Ok(Cow::Owned(haystack.to_vec()))
        }
        let mut strings = vec![];
        let mut i = 0;
        let mut last = 0;
        let mut count = 0;
        while i <= haystack.len() - needle.len() && max_count.is_none_or(|m| m > count) {
            if haystack[i..].starts_with(needle) {
                strings.try_reserve(i - last + value.len()).map_err(|_| "cannot allocate enough memory for replaced string")?;
                strings.extend(&haystack[last .. i]);
                strings.extend(value);
                i += needle.len();
                last = i;
                count += 1;
            } else { i += 1; }
        }
        strings.try_reserve(haystack[last..].len()).map_err(|_| "cannot allocate enough memory for replaced string")?;
        strings.extend(&haystack[last ..]);
        Ok(Cow::Owned(strings))
    }

    /// Repeats a string a given amount of times.
    /// # Arguments
    /// 1. The string to repeat.
    /// 2. The amount of times to repeat the string.
    pub macro Repeat [b"repeat"] (value, times) + _x, _v {
        let count = Number::try_from(times).map(|v| i64::from(v))?;
        if count <= 0 { return Ok(Cow::Borrowed(b"")) };
        let mut vec = Vec::new();
        vec.try_reserve(count as usize * value.len()).map_err(|_| "cannot allocate enough memory for repeated string")?;
        for _ in 0..count { vec.extend(value) }
        Ok(Cow::Owned(vec))
    }
}

/// Static block of bytes that can be used to turn a `u8` into a `&'static u8`.
/// Because fuck it, why not.
static BYTES: [u8; 256] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
    0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F,
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F,
    0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F,
    0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x5B, 0x5C, 0x5D, 0x5E, 0x5F,
    0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F,
    0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x7B, 0x7C, 0x7D, 0x7E, 0x7F,
    0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x8B, 0x8C, 0x8D, 0x8E, 0x8F,
    0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0x9B, 0x9C, 0x9D, 0x9E, 0x9F,
    0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD, 0xAE, 0xAF,
    0xB0, 0xB1, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB8, 0xB9, 0xBA, 0xBB, 0xBC, 0xBD, 0xBE, 0xBF,
    0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6, 0xC7, 0xC8, 0xC9, 0xCA, 0xCB, 0xCC, 0xCD, 0xCE, 0xCF,
    0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6, 0xD7, 0xD8, 0xD9, 0xDA, 0xDB, 0xDC, 0xDD, 0xDE, 0xDF,
    0xE0, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6, 0xE7, 0xE8, 0xE9, 0xEA, 0xEB, 0xEC, 0xED, 0xEE, 0xEF,
    0xF0, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF,
];
