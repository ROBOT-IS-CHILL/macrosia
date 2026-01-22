//! Defines some basic macros for regular use.

use crate::{Macro, MacroError, Number, var_reg::VariableRegistry, expr::ExpressionFunction};
use aho_corasick::AhoCorasick;
use base64::Engine;
use const_format::concatcp;
use flate2::{Compression, write::ZlibEncoder, read::ZlibDecoder};
use itertools::Itertools as _;
use rand::seq::SliceRandom as _;
use rand::{Rng, SeedableRng};
use regex::Regex;
use std::i64;
use std::io::prelude::*;
use std::{
    borrow::Cow,
    cmp::Ordering,
    ops::{Add as _, Mul as _},
    sync::OnceLock,
};
use rand_xoshiro::Xoshiro128PlusPlus;

macro_rules! regex {
    ($re:literal $(,)?) => {{
        static RE: OnceLock<Regex> = OnceLock::new();
        RE.get_or_init(|| Regex::new($re).unwrap())
    }};
}

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
    ($($(#[doc = $doc: literal])* $(@ $deprecated: ident;)? $vis: vis macro $sname: ident [ $name: literal ] $args: tt + $x: ident, $v: ident, $r: ident $body: tt)*) => {$(
        $(#[doc = $doc])*
        $(#[$deprecated])?
        #[derive(Copy, Clone)]
        $vis struct $sname;
        #[allow(deprecated)]
        impl Macro for $sname {
            fn name(&self) -> &[u8] { $name }
            fn description(&self) -> &str {
                concat!($($doc, "\n"),*)
            }
            fn eval<'arg, 'reg: 'arg, 'exec: 'reg>(&self, $x: &'exec crate::exec::Executor, $v: &'reg mut VariableRegistry, $r: &mut Xoshiro128PlusPlus, args: &mut dyn Iterator<Item = &'arg [u8]>) -> Result<Cow<'static, [u8]>, MacroError> {
                args!($args <- args);
                $body
            }
            fn clone(&self) -> Box<dyn Macro> { Box::new(*self) }
            fn source(&self) -> &[u8] { stringify!($body).trim().as_bytes() }
        }
    )*

    impl crate::exec::Executor {

        /// Adds all standard library macros to the given execution context.
        #[allow(deprecated)]
        pub fn add_stdlib(&mut self) {
            $(
                self.add_macro($sname);
            )*
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
                }
                // If the character isn't a valid escape, we need to push a backslash,
                // since we skipped it last loop (see below)
                _ => {
                    if let Some(ref mut c) = construct {
                        c.push(b'\\')
                    }
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
    use super::{Cow, unescape};
    #[test]
    fn test_unescape() {
        macro_rules! check {
            ($a: literal -> borrowed $b: literal) => {{
                let v = unescape($a);
                assert!(matches!(v, Cow::Borrowed(_)));
                assert_eq!(&*v, &*$b);
            }};
            ($a: literal -> owned $b: literal) => {{
                let v = unescape($a);
                assert!(matches!(v, Cow::Owned(_)));
                assert_eq!(&*v, &*$b);
            }};
        }
        check!(b"abcde" -> borrowed b"abcde");
        check!(br"a\bcde" -> borrowed br"a\bcde");
        check!(b"abc\\" -> borrowed b"abc\\");
        check!(br"a\[cd\e" -> owned br"a[cd\e");
        check!(b"a\\[bc\\" -> owned br"a[bc");
    }
}

fn is_truthy(value: &[u8]) -> bool {
    !matches!(
        value,
        b"false" | b"0" | b"False" | b"0.0" | b"0.0+0.0j" | b"0+0j" | b"0j" | b""
    )
}

def_macro! {
    /// Discards all arguments, returning nothing.
    /// Will still run any macros within its arguments - this one isn't "special".
    pub macro Discard [b""] (... _args) + _x, _v, _r {
        return Ok(Cow::Borrowed(b""))
    }

    /// Adds all arguments, returning their sum.
    /// # Arguments
    /// 1... Any amount of strings coercible to numbers.
    pub macro Add [b"add"] (... args) + _x, _v, _r {
        args
            .map(|v| Number::try_from(&*v))
            .process_results(|it| {
                Cow::Owned(format!("{}", it.fold(Number::ZERO, Number::add)).into_bytes())
            })
    }

    /// Multiplies all arguments, returning their product.
    /// # Arguments
    /// 1... Any amount of strings coercible to numbers.
    pub macro Multiply [b"multiply"] (... args) + _x, _v, _r {
        args
            .map(|v| Number::try_from(&*v))
            .process_results(|it| {
                Cow::Owned(format!("{}", it.fold(Number::ONE, Number::mul)).into_bytes())
            })
    }

    /// Subtracts the second argument from the first.
    /// # Arguments
    /// 1. The number to subtract from.
    /// 2. The number to subtract.
    pub macro Subtract [b"subtract"] (a, b) + _x, _v, _r {
        let [a, b]: [Number; 2] = [a.try_into()?, b.try_into()?];
        Ok(Cow::Owned(format!("{}", a - b).into_bytes()))
    }

    /// Divides the first argument by the second.
    /// # Arguments
    /// 1. The numerator of the division.
    /// 2. The denominator of the division.
    pub macro Divide [b"divide"] (a, b) + _x, _v, _r {
        let [a, b]: [Number; 2] = [a.try_into()?, b.try_into()?];
        Ok(Cow::Owned(format!("{}", a / b).into_bytes()))
    }

    /// Takes the modulus of the first argument with the second.
    /// # Arguments
    /// 1. The numerator of the modulus.
    /// 2. The denominator of the modulus.
    pub macro Modulus [b"mod"] (a, b) + _x, _v, _r {
        let [a, b]: [Number; 2] = [a.try_into()?, b.try_into()?];
        Ok(Cow::Owned(format!("{}", a % b).into_bytes()))
    }

    /// Checks if one number is greater than another.
    /// # Arguments
    /// 1. The number to compare.
    /// 2. The number to compare against.
    pub macro Greater [b"greater"] (a, b) + _x, _v, _r {
        let [a, b]: [Number; 2] = [a.try_into()?, b.try_into()?];
        Ok(Cow::Borrowed(if a > b { b"true" } else { b"false" }))
    }

    /// Checks if one number is less than another.
    /// # Arguments
    /// 1. The number to compare.
    /// 2. The number to compare against.
    pub macro Less [b"less"] (a, b) + _x, _v, _r {
        let [a, b]: [Number; 2] = [a.try_into()?, b.try_into()?];
        Ok(Cow::Borrowed(if a < b { b"true" } else { b"false" }))
    }

    /// Checks if one number is equal than another.
    /// # Arguments
    /// 1. The number to compare.
    /// 2. The number to compare against.
    pub macro NumEqual [b"num_equal"] (a, b) + _x, _v, _r {
        let [a, b]: [Number; 2] = [a.try_into()?, b.try_into()?];
        Ok(Cow::Borrowed(if a == b { b"true" } else { b"false" }))
    }

    /// Checks if two strings are equal.
    /// # Arguments
    /// 1. The string to compare.
    /// 2. The string to compare against.
    pub macro Equal [b"equal"] (a, b) + _x, _v, _r {
        Ok(Cow::Borrowed(if a == b { b"true" } else { b"false" }))
    }

    /// Compares a number to another. Returns `1` on [`Ordering::Greater`], `0` on [`Ordering::Equal`], `-1` on [`Ordering::Less`], and `nan` if an order cannot be determined.
    /// # Arguments
    /// 1. The number to compare.
    /// 2. The number to compare against.
    pub macro Compare [b"cmp"] (a, b) + _x, _v, _r {
        let [a, b]: [Number; 2] = [a.try_into()?, b.try_into()?];
        Ok(Cow::Borrowed(match a.partial_cmp(&b) {
            None => b"nan",
            Some(Ordering::Less) => b"-1",
            Some(Ordering::Equal) => b"0",
            Some(Ordering::Greater) => b"1",
        }))
    }

    /// Raises the first argument to the second.
    /// # Arguments
    /// 1. The base of the exponent.
    /// 2. The power of the exponent.
    pub macro Pow [b"pow"] (a, b) + _x, _v, _r {
        let [a, b]: [Number; 2] = [a.try_into()?, b.try_into()?];
        Ok(Cow::Owned(format!("{}", a.pow(b)).into_bytes()))
    }

    /// Takes the logarithm of the first argument with the second as a base.
    /// # Arguments
    /// 1. The value to take the logarithm of.
    /// 2. The base of the logarithm.
    pub macro Log [b"log"] (a, b) + _x, _v, _r {
        let [a, b]: [Number; 2] = [a.try_into()?, b.try_into()?];
        Ok(Cow::Owned(format!("{}", a.log(b)).into_bytes()))
    }

    /// Gets the real component of a number.
    /// # Arguments
    /// 1. The number.
    pub macro Real [b"real"] (a) + _x, _v, _r {
        Number::try_from(a).map(|v| match v {
            Number::Integer(_) | Number::Float(_) => format!("{v}").into_bytes(),
            Number::Complex(_) => format!("{}", Number::Float(v.into())).into_bytes()
        }).map(Cow::Owned)
    }

    /// Gets the imaginary component of a number.
    /// # Arguments
    /// 1. The number.
    pub macro Imaginary [b"imag"] (a) + _x, _v, _r {
        Number::try_from(a).map(|v| match v {
            Number::Integer(_) | Number::Float(_) => Cow::Borrowed(b"0" as &[u8]),
            Number::Complex(c) => Cow::Owned(format!("{}", Number::Float(c.im)).into_bytes())
        })
    }

    /// Gets the sine of a number.
    /// # Arguments
    /// 1. The number.
    pub macro Sine [b"sin"] (a) + _x, _v, _r {
        Number::try_from(a).map(|v| match v {
            Number::Integer(i) => Cow::Owned(format!("{}", Number::Float((i as f64).sin())).into_bytes()),
            Number::Float(f) => Cow::Owned(format!("{}", Number::Float(f.sin())).into_bytes()),
            Number::Complex(c) => Cow::Owned(format!("{}", Number::Complex(c.sin())).into_bytes()),
        })
    }

    /// Gets the cosine of a number.
    /// # Arguments
    /// 1. The number.
    pub macro Cosine [b"cos"] (a) + _x, _v, _r {
        Number::try_from(a).map(|v| match v {
            Number::Integer(i) => Cow::Owned(format!("{}", Number::Float((i as f64).cos())).into_bytes()),
            Number::Float(f) => Cow::Owned(format!("{}", Number::Float(f.cos())).into_bytes()),
            Number::Complex(c) => Cow::Owned(format!("{}", Number::Complex(c.cos())).into_bytes()),
        })
    }

    /// Gets the tangent of a number.
    /// # Arguments
    /// 1. The number.
    pub macro Tangent [b"tan"] (a) + _x, _v, _r {
        Number::try_from(a).map(|v| match v {
            Number::Integer(i) => Cow::Owned(format!("{}", Number::Float((i as f64).tan())).into_bytes()),
            Number::Float(f) => Cow::Owned(format!("{}", Number::Float(f.tan())).into_bytes()),
            Number::Complex(c) => Cow::Owned(format!("{}", Number::Complex(c.tan())).into_bytes()),
        })
    }

    /// Unescapes the argument.
    /// # Arguments
    /// 1. The string to unescape. Must be valid UTF-8.
    pub macro Unescape [b"unescape"] (string) + _x, _v, _r {
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
    pub macro Store [b"store"] (name, value) + _x, v, _r {
        v.store(&*name, Vec::from(value).into()); // The value could easily outlive the argument, so we clone
        Ok(Cow::Borrowed(b""))
    }

    /// Loads a variable from the variable registry.
    /// # Arguments
    /// 1. The name of the variable to load.
    pub macro Load [b"load"] (name) + _x, v, _r {
        v.load(&*name)
            .map(Vec::from)
            .map(Cow::Owned)
            .ok_or_else(move || format!("variable {} does not exist", String::from_utf8_lossy(&*name)).into())
    }

    /// Drops a variable from the variable registry.
    /// # Arguments
    /// 1. The name of the variable to drop.
    pub macro Drop [b"drop"] (name) + _x, v, _r {
        v.drop(name);
        Ok(Cow::Borrowed(b""))
    }

    /// Loads a variable from the variable registry, or sets it to and returns the second argument if it doesn't exist.
    /// # Arguments
    /// 1. The name of the variable to load.
    /// 2. The value to output if the variable does not exist.
    pub macro Get [b"get"] (name, default) + _x, v, _r {
        let value = match v.load(&*name) {
            Some(v) => v,
            None => {
                v.store(&*name, Cow::Borrowed(default));
                &*default
            }
        };
        Ok(
            Vec::from(value).into()
        )
    }

    /// Returns a single byte from a hexadecimal value.
    /// # Arguments
    /// 1. The hexadecimal value of the byte to return.
    pub macro Byte [b"byte"] (hex) + _x, _v, _r {
        str::from_utf8(hex).ok()
            .and_then(|s| u8::from_str_radix(s, 16).ok())
            .ok_or("invalid byte".into())
            .map(|v| Cow::Borrowed(std::slice::from_ref(&BYTES[v as usize])))
    }

    /// Returns a single UTF-8 character from a given integer value.
    /// # Arguments
    /// 1. The codepoint of the character to return.
    pub macro Char [b"chr"] (val) + _x, _v, _r {
        let v = Number::try_from(val)?;
        u32::try_from(i64::from(v)).ok()
            .and_then(char::from_u32)
            .ok_or("invalid character codepoint".into())
            .map(|chr| {
                let mut v = vec![0; chr.len_utf8()];
                chr.encode_utf8(&mut v);
                Cow::Owned(v)
            })
    }

    /// Gets the Unicode codepoint of a given one-character string.
    /// # Arguments
    /// 1. The character to get the codepoint of. The string must be valid UTF-8, and have at least one character. All other characters will be ignored.
    pub macro Ord [b"ord"] (string) + _x, _v, _r {
        str::from_utf8(string).ok()
            .and_then(|s| s.chars().next())
            .map(|c| Cow::Owned(format!("{}", c as u32).into_bytes()))
            .ok_or_else(|| "value was not valid UTF-8".into())
    }

    /// Converts a value into a boolean.
    /// # Arguments
    /// 1. The value to convert.
    pub macro ToBoolean [b"to_boolean"] (value) + _x, _v, _r {
        Ok(Cow::Borrowed(if is_truthy(value) {b"true"} else {b"false"}))
    }

    /// Returns its first argument. Legacy alias for compatiblity reasons.
    @deprecated;
    pub macro ToFloat [b"to_float"] (value) + _x, _v, _r {
        Ok(Cow::Owned(value.to_vec()))
    }

    /// Checks if a value is a number.
    /// # Arguments
    /// 1. The value to check.
    pub macro IsNumber [b"is_number"] (value) + _x, _v, _r {
        Ok(Cow::Borrowed(
            if Number::try_from(value).is_ok() {b"true"} else {b"false"}
        ))
    }

    /// Replaces a string within another string, using the Aho-Corasick algorithm.
    ///
    /// All replacements happen _at once_, meaning, for example,
    /// `[replace/baba/a/i/bibi/koko]` will be `bibi`, not `koko`.
    ///
    /// # Arguments
    /// 1. The string to replace substrings of
    /// 2... The substring to replace
    /// 3... The string to replace the substring with
    pub macro SReplace [b"sreplace"] (haystack, ...iter) + _x, _v, _r {
        let mut builder = AhoCorasick::builder();
        builder.kind(Some(aho_corasick::AhoCorasickKind::NoncontiguousNFA));
        let mut patterns = vec![];
        let mut replacements = vec![];
        for mut chunk in &iter.chunks(2) {
            let needle = chunk.next().expect("first of chunk cannot be empty");
            let Some(value) = chunk.next() else { return Err("replace requires an odd number of arguments")?; };
            if needle.is_empty() {
                Err("search pattern value cannot be empty")?
            }
            patterns.push(needle);
            replacements.push(value);
        }
        let repl = builder.build(patterns).map_err(|err| format!("failed to build replacement algorithm: {err}"))?;
        let haystack = repl.try_replace_all_bytes(&haystack, &replacements).map_err(|err| format!("failed to replace: {err}"))?;
        Ok(Cow::Owned(haystack))
    }

    /// Repeats a string a given amount of times.
    /// # Arguments
    /// 1. The string to repeat.
    /// 2. The amount of times to repeat the string.
    /// 3? The separator between each string.
    pub macro Repeat [b"repeat"] (times, value, ...iter) + _x, _v, _r {
        let joiner = iter.next().unwrap_or(b"");
        let count = Number::try_from(times).map(|v| i64::from(v))?;
        if count <= 0 { return Ok(Cow::Borrowed(b"")) };
        let mut vec = Vec::<u8>::new();
        vec.try_reserve(count as usize * value.len() + (count - 1) as usize * joiner.len()).map_err(|_| "cannot allocate enough memory for repeated string")?;
        vec.extend(value);
        for _ in 1..count { vec.extend(joiner); vec.extend(value) }
        Ok(Cow::Owned(vec))
    }

    /// Creates a random value on the range [0, 1).
    /// # Arguments
    /// 1? A string to seed the RNG with.
    pub macro Random [b"rand"] (...iter) + _x, _v, r {
        if let Some(seed) = iter.next() {
            *r = Xoshiro128PlusPlus::seed_from_u64(seahash::hash(seed));
        }
        Ok(Cow::Owned(format!("{}", r.random::<f64>()).into_bytes()))
    }

    /// Shuffles the given arguments.
    /// # Arguments
    /// 1... The arguments to shuffle.
    pub macro Shuffle [b"random.shuffle"] (...iter) + _x, _v, r {
        let mut args = iter.collect::<Vec<_>>();
        args.shuffle(r);
        Ok(Cow::Owned(args.join(b"/" as &[u8])))
    }

    /// Converts the first argument to an integer.
    /// # Arguments
    /// 1. The number to convert to an integer.
    /// 2? The base to convert from. Defaults to 10. Must be between 2 and 36.
    pub macro Int [b"int"] (num, ...iter) + _x, _v, _r {
        let base = Number::try_from(iter.next().unwrap_or(b"10")).map(i64::from)?;
        if !(2..=36).contains(&base) {
            return Err("base must be between 2 and 36 inclusive")?
        }
        let n = if base == 10 {
            Number::try_from(num).map(i64::from)?
        } else {
            let str = str::from_utf8(num)?;
            i64::from_str_radix(str, base as u32).map_err(|_| "invalid integer for given base")?
        };
        Ok(Cow::Owned(format!("{n}").into_bytes()))
    }

    /// Joins each argument.
    /// # Arguments
    /// 1. The string to join each value with.
    /// 2... Strings to join.
    pub macro Join [b"join"] (sep, ...iter) + _x, _v, _r {
        Ok(Cow::Owned(
            iter.intersperse(sep)
                .flatten()
                .copied()
                .collect()
        ))
    }

    /// Joins each argument with an empty string.
    /// # Arguments
    /// 1... Strings to join.
    pub macro Concat [b"concat"] (...iter) + _x, _v, _r {
        Ok(Cow::Owned(
            iter.flatten()
                .copied()
                .collect()
        ))
    }

    /// Hashes the given value.
    /// # Arguments
    /// 1. The value to hash.
    pub macro Hash [b"hash"] (value) + _x, _v, _r {
        Ok(Cow::Owned(format!("{}", seahash::hash(value)).into_bytes()))
    }

    /// Gets the byte length of a string.
    /// # Arguments
    /// 1. The string to get the length of.
    pub macro ByteLength [b"byte.len"] (value) + _x, _v, _r {
        Ok(Cow::Owned(format!("{}", value.len()).into_bytes()))
    }

    /// Gets the character length of a UTF-8 string.
    /// # Arguments
    /// 1. The string to get the length of. Must be valid UTF-8.
    pub macro Length [b"len"] (value) + _x, _v, _r {
        str::from_utf8(value).ok()
            .map(|value| Cow::Owned(format!("{}", value.chars().count()).into_bytes()))
            .ok_or_else(|| "value was not valid UTF-8".into())
    }

    /// Raises an error with a specified message.
    /// # Arguments
    /// 1? The error message. Defaults to `<unspecified>`.
    pub macro Error [b"error"] (...msg) + _x, _v, _r {
        Err(String::from_utf8_lossy(msg.next().unwrap_or(b"<unspecified>")).into_owned().into())
    }

    /// Converts the given bytestring to UTF-8 lossily, replacing errors with `U+FFFD`.
    /// # Arguments
    /// 1. The string to convert to UTF-8.
    pub macro Utf8 [b"utf8"] (string) + _x, _v, _r {
        Ok(Cow::Owned(
            String::from_utf8_lossy(string).into_owned().into_bytes()
        ))
    }

    /// Replaces a string within another string, using regex matching.
    /// All arguments must be valid UTF-8.
    ///
    /// Note that for legacy reasons, this uses `\1` instead of `$1`,
    /// to emulate Python's regex.
    ///
    /// # Arguments
    /// 1. The string to replace substrings of
    /// 2... The substring to replace
    /// 3... The string to replace the substring with
    pub macro Replace [b"replace"] (haystack, ...iter) + _x, _v, _r {
        let mut haystack = String::from_utf8(haystack.to_vec())?;
        for mut chunk in &iter.chunks(2) {
            let needle = str::from_utf8(
                chunk.next().expect("first of chunk cannot be empty")
            )?;
            if needle.is_empty() {
                Err("search pattern value cannot be empty")?
            }
            let Some(value) = chunk.next() else { return Err("replace requires an odd number of arguments")?; };
            let mut replacement = str::from_utf8(value)?.to_string();
            replacement = regex!(r"\\(\d+)|\$").replace_all(&replacement, |caps: &regex::Captures| {
                if let Some(digits) = caps.get(1) {
                    format!("${{{}}}", digits.as_str())
                } else {
                    "$$".to_string()
                }
            }).to_string();

            let pat = Regex::new(needle).map_err(|_| format!("invalid regex pattern: {needle}"))?;
            haystack = pat.replace_all(&haystack, replacement).into_owned();
        }
        Ok(Cow::Owned(haystack.into_bytes()))
    }

    /// Replaces a string within another string, using regex matching.
    /// Unescapes needles and replacement patterns first.
    /// All arguments must be valid UTF-8.
    ///
    /// # Arguments
    /// 1. The string to replace substrings of
    /// 2... The substring to replace
    /// 3... The string to replace the substring with
    pub macro UReplace [b"ureplace"] (haystack, ...iter) + _x, _v, _r {
        let mut haystack = String::from_utf8(haystack.to_vec())?;
        for mut chunk in &iter.chunks(2) {
            let needle = String::from_utf8(
                unescape(chunk.next().expect("first of chunk cannot be empty")).into_owned()
            )?;
            if needle.is_empty() {
                Err("search pattern value cannot be empty")?
            }
            let Some(value) = chunk.next() else { return Err("replace requires an odd number of arguments")?; };
            let mut replacement = String::from_utf8(
                unescape(value).into_owned()
            )?;
            replacement = regex!(r"\\(\d+)|\$").replace_all(&replacement, |caps: &regex::Captures| {
                if let Some(digits) = caps.get(1) {
                    format!("${{{}}}", digits.as_str())
                } else {
                    "$$".to_string()
                }
            }).to_string();
            let pat = Regex::new(&needle).map_err(|_| format!("invalid regex pattern: {needle}"))?;
            haystack = pat.replace_all(&haystack, &replacement).into_owned();
        }
        Ok(Cow::Owned(haystack.into_bytes()))
    }

    /// Repeats a string a set amount times, replacing a pattern in each
    /// with a number on a range, optionally separated by a separator.
    /// The pattern, repeated string, and separator all must be valid UTF-8.
    ///
    /// # Arguments
    /// 1. The pattern to replace in the repeated string
    /// 2. The start of the range to repeat on
    /// 3. The end of the range to repeat on
    /// 4. The string to repeat
    /// 5? The separator between repetitions
    ///
    /// # Examples
    /// > `[sequence/@/1/5/(@)/,]` -> `(1),(2),(3),(4),(5)`
    /// > `[sequence/@/1/3/@]` -> `123`
    pub macro Sequence [b"sequence"] (needle, start, end, haystack, ...iter) + _x, _v, _r {
        let joiner = str::from_utf8(iter.next().unwrap_or(b""))?;
        let mut start = Number::try_from(start).map(|v| i64::from(v))?;
        let mut end = Number::try_from(end).map(|v| i64::from(v))?;
        let mut flip = false;
        if end <= start { flip = true; (end, start) = (start, end); };
        let haystack = str::from_utf8(haystack)?;
        let needle = str::from_utf8(needle)?;
        let mut strings = Vec::new();
        for i in start ..= end {
            strings.push(haystack.replace(needle, &format!("{i}")));
            if i.checked_add(1).is_some_and(|i| i <= end) {
                strings.push(String::from(joiner));
            }
        }
        if flip {
            strings.reverse();
        }
        Ok(Cow::Owned(strings.concat().into_bytes()))
    }

    /// Repeats a string for each element in a list, replacing one pattern in each
    /// with the list index and another with the value, optionally separated by a separator.
    /// The string, patterns, and list all must be valid UTF-8.
    ///
    /// # Arguments
    /// 1. The list to loop over
    /// 2. The list delimiter
    /// 3. The pattern to replace with the index
    /// 4. The pattern to repace with the value
    /// 5? The separator between repetitions
    ///
    /// # Example
    /// > `[for/a,b,c/,/#/@/#:@/,]` -> `0:a,1:b,2:c`
    pub macro For [b"for"] (list, delim, idx_pat, item_pat, string, ...iter) + _x, _v, _r {
        let list = str::from_utf8(list)?;
        let idx_pat = str::from_utf8(idx_pat)?;
        let item_pat = str::from_utf8(item_pat)?;
        let string = str::from_utf8(string)?;
        let joiner = str::from_utf8(iter.next().unwrap_or(b""))?;
        let mut buf = String::new();
        let mut array = list;
        let mut list_idx = 0;
        while let Some(idx) = array.as_bytes().windows(delim.len()).position(|w| w == delim) {
            let repl_string = string
                .replace(idx_pat, &format!("{list_idx}"))
                .replace(item_pat, &array[..idx]);
            buf.try_reserve(repl_string.len())?;
            buf.push_str(&repl_string);
            buf.try_reserve(joiner.len())?;
            buf.push_str(&joiner);
            array = &array[idx + delim.len()..];
            list_idx = list_idx + 1;
        }
        let repl_string = string
            .replace(idx_pat, &format!("{list_idx}"))
            .replace(item_pat, &array);
        buf.try_reserve(repl_string.len())?;
        buf.push_str(&repl_string);
        Ok(Cow::Owned(buf.into_bytes()))
    }

    /// Chooses between a set of return values from a chain of booleans.
    /// # Arguments
    /// 1... A condition to check.
    /// 2... The value to return if the condition is true.
    /// ...
    /// n? The value to return if no conditions are true. If this is not supplied, will return the empty string if reached.
    pub macro If [b"if"] (...iter) + _x, _v, _r {
        for mut chunk in &iter.chunks(2) {
            let cond = chunk.next().expect("first of chunk always exists");
            let Some(value) = chunk.next() else {
                // This is an else branch
                return Ok(Cow::Owned(cond.into()))
            };
            if is_truthy(cond) { return Ok(Cow::Owned(value.into())) }
        };
        Ok(Cow::Borrowed(b""))
    }

    /// Takes the boolean and of all inputs.
    /// # Arguments
    /// 1... Any value. Will be converted to a boolean.
    pub macro And [b"and"] (...iter) + _x, _v, _r {
        Ok(Cow::Borrowed('b: {
            for val in iter {
                if !is_truthy(val) { break 'b b"false"; }
            }
            b"true"
        }))
    }

    /// Takes the boolean or of all inputs.
    /// # Arguments
    /// 1... Any value. Will be converted to a boolean.
    pub macro Or [b"or"] (...iter) + _x, _v, _r {
        Ok(Cow::Borrowed('b: {
            for val in iter {
                if is_truthy(val) { break 'b b"true"; }
            }
            b"false"
        }))
    }

    /// Logically negates a boolean.
    /// # Arguments
    /// 1. The boolean to negate. Will be converted if it's not already one.
    pub macro Not [b"not"] (val) + _x, _v, _r {
        Ok(Cow::Borrowed(if is_truthy(val) {b"false"} else {b"true"}))
    }

    /// Raises an error with a specified message if the first argument is not truthy.
    /// # Arguments
    /// 1. The condition to check.
    /// 2? The error message. Defaults to `<unspecified>`.
    pub macro Assert [b"assert"] (val, ...msg) + _x, _v, _r {
        if !is_truthy(val) {
            return Err(String::from_utf8_lossy(msg.next().unwrap_or(b"<unspecified>")).into_owned())?
        }
        Ok(b"".into())
    }

    /// Decodes some given Base64.
    /// # Arguments
    /// 1... The Base64 string to decode.
    pub macro Base64Decode [b"base64.decode"] (...val) + _x, _v, _r {
        let engine = base64::engine::general_purpose::URL_SAFE;
        let joined = val.intersperse(b"/").flatten().copied().collect::<Vec<_>>();
        let mut buf = Vec::new();
        buf.try_reserve(base64::decoded_len_estimate(joined.len()))?;
        unsafe {
            let sbuf = buf.spare_capacity_mut();
            std::ptr::write_bytes(sbuf.as_mut_ptr(), 0, sbuf.len());
            let len = sbuf.len();
            buf.set_len(len);
        }
        let written_len = engine.decode_slice(joined, &mut buf).map_err(|_| "failed to decode base64")?;
        buf.truncate(written_len);
        Ok(Cow::Owned(buf))
    }

    /// Encodes some given data to Base64.
    /// # Arguments
    /// 1... The string to encode.
    pub macro Base64Encode [b"base64.encode"] (...val) + _x, _v, _r {
        let engine = base64::engine::general_purpose::URL_SAFE;
        let joined = val.intersperse(b"/").flatten().copied().collect::<Vec<_>>();
        let mut buf = Vec::new();
        buf.try_reserve(base64::encoded_len(joined.len(), true).ok_or("base64 value is way too large")?)?;
        unsafe {
            let sbuf = buf.spare_capacity_mut();
            std::ptr::write_bytes(sbuf.as_mut_ptr(), 0, sbuf.len());
            let len = sbuf.len();
            buf.set_len(len);
        }
        let written = engine.encode_slice(joined, &mut buf).map_err(|_| "failed to encode base64")?;
        buf.truncate(written);
        Ok(Cow::Owned(buf))
    }

    /// Gets the amount of seconds since January 1, 1970, 00:00 GMT.
    pub macro UnixTime [b"unixtime"] () + _x, _v, _r {
        let time = web_time::SystemTime::now();
        let since = time.duration_since(web_time::SystemTime::UNIX_EPOCH).map_err(|_| "getting time since unix epoch failed")?;
        Ok(Cow::Owned(format!("{}", since.as_secs_f64()).into_bytes()))
    }

    /// Slices the given string by a start, stop, and optional step, based on UTF-8 characters.
    /// The string must be valid UTF-8.
    /// # Arguments
    /// 1. The string to slice.
    /// 2? The slice start.
    /// 3? The slice end.
    /// 4? The slice step.
    pub macro Slice [b"slice"] (haystack, ...args) + _x, _v, _r {
        let start = args.next().and_then(|v| (!v.is_empty()).then_some(v)).map(|v| Number::try_from(v).map(i64::from)).transpose()?;
        let end = args.next().and_then(|v| (!v.is_empty()).then_some(v)).map(|v| Number::try_from(v).map(i64::from)).transpose()?;
        let step = args.next().and_then(|v| (!v.is_empty()).then_some(v)).map(|v| Number::try_from(v).map(i64::from)).transpose()?;
        let mut start = start.unwrap_or(0);
        let haystack = str::from_utf8(haystack)?;
        let clen = haystack.chars().count();
        if start < 0 { start = clen as i64 + start; }
        if start >= clen as i64  { return Ok(Cow::Borrowed(b"")); }
        let mut end = end.unwrap_or(clen as i64);
        if end < 0 { end = clen as i64 + end; }
        if end < 0 { return Ok(Cow::Borrowed(b"")); }
        let step = step.unwrap_or(1);
        if end < start { Err("slice end cannot be less than start")? }
        if step == 0 { Err("cannot have a step size of 0")? }
        if step < 0 {
            return Ok(Cow::Owned(haystack.chars().rev().skip(start as usize).take((end - start) as usize).step_by((-step) as usize).collect::<String>().into_bytes()));
        }
        return Ok(Cow::Owned(haystack.chars().skip(start as usize).take((end - start) as usize).step_by(step as usize).collect::<String>().into_bytes()));
    }

    /// Slices the given string by a start, stop, and optional step, based on bytes.
    /// # Arguments
    /// 1. The string to slice.
    /// 2? The slice start.
    /// 3? The slice end.
    /// 4? The slice step.
    pub macro BSlice [b"byte.slice"] (haystack, ...args) + _x, _v, _r {
        let start = args.next().and_then(|v| (!v.is_empty()).then_some(v)).map(|v| Number::try_from(v).map(i64::from)).transpose()?;
        let end = args.next().and_then(|v| (!v.is_empty()).then_some(v)).map(|v| Number::try_from(v).map(i64::from)).transpose()?;
        let step = args.next().and_then(|v| (!v.is_empty()).then_some(v)).map(|v| Number::try_from(v).map(i64::from)).transpose()?;
        let mut start = start.unwrap_or(0);
        if start < 0 { start = haystack.len() as i64 + start; }
        let mut end = end.unwrap_or(haystack.len() as i64);
        if end < 0 { end = haystack.len() as i64 + end; }
        let step = step.unwrap_or(1);
        if end < start { Err("slice end cannot be less than start")? }
        if step == 0 { Err("cannot have a step size of 0")? }
        if step < 0 {
            return Ok(Cow::Owned(haystack.iter().copied().rev().skip(start as usize).take((end - start) as usize).step_by((-step) as usize).collect::<Vec<u8>>()));
        }
        return Ok(Cow::Owned(haystack.iter().copied().skip(start as usize).take((end - start) as usize).step_by(step as usize).collect::<Vec<u8>>()));
    }

    /// Splits a string into a list by a delimiter, and then indexes into that list.
    /// # Arguments
    /// 1. The value to split.
    /// 2. The list delimiter.
    /// 3. The index to grab.
    pub macro Split [b"split"] (array, delim, index) + _x, _v, _r {
        let mut index = Number::try_from(index).map(i64::from)?;
        let mut vec = Vec::new();
        let mut array = array;
        while let Some(idx) = array.windows(delim.len()).position(|w| w == delim) {
            vec.try_reserve(1)?;
            vec.push(&array[..idx]);
            array = &array[idx + delim.len()..];
        }
        vec.push(array);
        if index < 0 {
            index = array.len() as i64 + index;
        }
        Ok(Cow::Owned(Vec::from(*vec.get(index as usize).ok_or("index out of bounds")?)))
    }

    /// Checks if macros exist within the execution context.
    /// # Arguments
    /// 1... The macro names to check.
    pub macro IsMacro [b"macro"] (...args) + x, _v, _r {
        Ok(Cow::Owned(
            args.map(|arg| -> &[u8] { if x.macros().get(arg).is_some() { b"true" } else { b"false" } } ).intersperse(b"/").flatten().copied().collect::<Vec<_>>()
        ))
    }

    /// Gets a slice of the given arguments.
    /// # Arguments
    /// 1. The slice, of the form `<start>[:<stop>[:<step>]]`.
    /// 2... The arguments to slice.
    pub macro Argslice [b"argslice"] (slice, ...args) + _x, _v, _r  {
        let mut slice = slice.split(|b| *b == b':');
        let start = slice.next().and_then(|v| (!v.is_empty()).then_some(v)).map(|v| Number::try_from(v).map(i64::from)).transpose()?;
        let end = slice.next().and_then(|v| (!v.is_empty()).then_some(v)).map(|v| Number::try_from(v).map(i64::from)).transpose()?;
        let step = slice.next().and_then(|v| (!v.is_empty()).then_some(v)).map(|v| Number::try_from(v).map(i64::from)).transpose()?;
        let args = args.collect::<Vec<_>>();
        let mut start = start.unwrap_or(1);
        if start == 0 { Err("slice cannot start at 0 - argslice slice is 1-indexed")? }
        if start < 0 { start = args.len() as i64 + start; } else { start = start - 1; }
        let mut end = end.unwrap_or(args.len() as i64 + 1);
        if end == 0 { Err("slice cannot start at 0 - argslice slice is 1-indexed")? }
        if end < 0 { end = args.len() as i64 + end; } else { end = end - 1; }
        let step = step.unwrap_or(1);
        if end < start { Err("slice end cannot be less than start")? }
        if step == 0 { Err("cannot have a step size of 0")? }
        if step < 0 {
            return Ok(Cow::Owned(
                args.iter().rev()
                .skip(start as usize)
                .take((end - start) as usize)
                .step_by(step as usize)
                .map(|v| *v).intersperse(b"/" as &[u8])
                .flatten().copied().collect()
            ));
        }
        return Ok(Cow::Owned(
                args.iter()
                .skip(start as usize)
                .take((end - start) as usize)
                .step_by(step as usize)
                .map(|v| *v).intersperse(b"/" as &[u8])
                .flatten().copied().collect()
            ));
    }

    /// Converts its first argument to hexadecimal.
    /// # Arguments
    /// 1. The number to convert.
    pub macro Hex [b"hex"] (val) + _x, _v, _r {
        let val = Number::try_from(val).map(i64::from)?;
        Ok(Cow::Owned(format!("{val:#x}").into_bytes()))
    }

    /// Converts its first argument to octal.
    /// # Arguments
    /// 1. The number to convert.
    pub macro Oct [b"oct"] (val) + _x, _v, _r {
        let val = Number::try_from(val).map(i64::from)?;
        Ok(Cow::Owned(format!("{val:#o}").into_bytes()))
    }

    /// Converts its first argument to binary.
    /// # Arguments
    /// 1. The number to convert.
    pub macro Bin [b"bin"] (val) + _x, _v, _r {
        let val = Number::try_from(val).map(i64::from)?;
        Ok(Cow::Owned(format!("{val:#b}").into_bytes()))
    }

    /// Converts its first argument to ASCII lowercase.
    /// # Arguments
    /// 1. The string to convert.
    pub macro Lower [b"lower"] (val) + _x, _v, _r {
        Ok(Cow::Owned(val.iter().map(|c| c.to_ascii_lowercase()).collect()))
    }

    /// Converts its first argument to ASCII uppercase.
    /// # Arguments
    /// 1. The string to convert.
    pub macro Upper [b"upper"] (val) + _x, _v, _r {
        Ok(Cow::Owned(val.iter().map(|c| c.to_ascii_uppercase()).collect()))
    }

    /// Converts its first argument to ASCII title case.
    /// # Arguments
    /// 1. The string to convert.
    pub macro Title [b"title"] (val) + _x, _v, _r {
        let mut buf = Vec::new();
        for substr in val.split(|c| c.is_ascii_whitespace()) {
            if substr.len() == 0 { continue; }
            buf.try_reserve(1)?;
            buf.push(substr[0].to_ascii_uppercase());
            buf.try_reserve(substr[1..].len())?;
            buf.extend(substr[1..].iter().map(|c| c.to_ascii_lowercase()));
        }
        Ok(Cow::Owned(buf))
    }

    /// Checks if a variable exists.
    /// # Arguments
    /// 1. The variable name to check.
    pub macro IsStored [b"is_stored"] (val) + _x, v, _r {
        Ok(Cow::Borrowed(v.load(val).map_or(b"false", |_| b"true")))
    }

    /// Gets the current execution step number.
    pub macro Step [b"step"] () + x, _v, _r {
        Ok(Cow::Owned(format!("{}", x.step()).into_bytes()))
    }

    /// Finds the amount of occurrences of a string within another, optionally between a given range.
    /// # Arguments
    /// 1. The value to search.
    /// 2. The value to search for.
    /// 3? The start index. Defaults to 0.
    /// 4? The end index. Defaults to the length of the string.
    pub macro Count [b"count"] (haystack, needle, ...iter) + _x, _v, _r {
        let mut start = iter.next().map(Number::try_from).transpose()?.map(i64::from).unwrap_or(0);
        let end = iter.next().map(Number::try_from).transpose()?.map(i64::from).unwrap_or(haystack.len() as i64);
        if start < 0 { start += haystack.len() as i64 }
        if start < 0 { return Err("search start cannot be before string start")? }
        if end > haystack.len() as i64 { return Err("search end cannot be larger than string")? };
        let mut haystack = haystack.get(start as usize .. end as usize).ok_or("haystack slice failed")?;
        let mut count = 0;
        if needle.len() > haystack.len() { return Ok(Cow::Borrowed(b"0")) }
        while let Some(idx) = haystack.windows(needle.len()).position(|w| w == needle) {
            count += 1;
            haystack = &haystack[idx + needle.len()..];
        }
        Ok(Cow::Owned(format!("{count}").into_bytes()))
    }

    /// Finds the first occurrence of a string within another, optionally between a given range.
    /// Returns -1 if not found.
    /// # Arguments
    /// 1. The value to search.
    /// 2. The value to search for.
    /// 3? The start index. Defaults to 0.
    /// 4? The end index. Defaults to the length of the string.
    pub macro Find [b"find"] (haystack, needle, ...iter) + _x, _v, _r {
        let mut start = iter.next().map(Number::try_from).transpose()?.map(i64::from).unwrap_or(0);
        let end = iter.next().map(Number::try_from).transpose()?.map(i64::from).unwrap_or(haystack.len() as i64);
        if start < 0 { start += haystack.len() as i64 }
        if start < 0 { return Err("search start cannot be before string start")? }
        if end > haystack.len() as i64 { return Err("search end cannot be larger than string")? };
        let haystack = haystack.get(start as usize .. end as usize).ok_or("haystack slice failed")?;
        if needle.len() > haystack.len() { return Ok(Cow::Borrowed(b"0")) }
        if let Some(idx) = haystack.windows(needle.len()).position(|w| w == needle) {
            Ok(Cow::Owned(format!("{idx}").into_bytes()))
        } else { Ok(Cow::Borrowed(b"-1")) }
    }

    /// Compresses a given string using zlib, returning the compressed data Base64-encoded.
    /// # Arguments
    /// 1... The string to compress. Slashes do not need to be escaped.
    pub macro ZlibCompress [b"zlib.compress"] (...iter) + _x, _v, _r {
        let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
        for arg in iter.intersperse(b"/") {
            e.write_all(arg).map_err(|e| format!("writing to zlib stream failed: {e}"))?
        }
        let bytes = e.finish().map_err(|e| format!("failed to compress: {e}"))?;
        let engine = base64::engine::general_purpose::URL_SAFE;
        let mut buf = Vec::new();
        buf.try_reserve(base64::encoded_len(bytes.len(), true).ok_or("base64 value is way too large")?)?;
        unsafe {
            let sbuf = buf.spare_capacity_mut();
            std::ptr::write_bytes(sbuf.as_mut_ptr(), 0, sbuf.len());
            let len = sbuf.len();
            buf.set_len(len);
        }
        let written = engine.encode_slice(bytes, &mut buf).map_err(|e| format!("failed to encode base64: {e}"))?;
        buf.truncate(written);
        Ok(Cow::Owned(buf))
    }

    /// Decompresses a given string using zlib, first decoding it from Base64.
    /// # Arguments
    /// 1. The string to decompress. Must be valid Base64.
    pub macro ZlibDecompress [b"zlib.decompress"] (string) + _x, _v, _r {
        let engine = base64::engine::general_purpose::URL_SAFE;
        let mut buf = Vec::new();
        buf.try_reserve(base64::decoded_len_estimate(string.len()))?;
        unsafe {
            let sbuf = buf.spare_capacity_mut();
            std::ptr::write_bytes(sbuf.as_mut_ptr(), 0, sbuf.len());
            let len = sbuf.len();
            buf.set_len(len);
        }
        let written_len = engine.decode_slice(string, &mut buf).map_err(|_| "failed to decode base64")?;
        buf.truncate(written_len);
        let mut e = ZlibDecoder::new(&*buf);
        let mut vec = Vec::new();
        e.read_to_end(&mut vec).map_err(|e| format!("failed to decompress data: {e}"))?;
        Ok(Cow::Owned(vec))
    }

    /// Sets a single byte of a variable to a hexadecimal value.
    /// # Arguments
    /// 1. The variable to index into.
    /// 2. The byte index in the variable. Must be greater than or equal to 0.
    /// 3. The value to set the byte to.
    pub macro ByteSet [b"byte.set"] (name, index, value) + _x, v, _r {
        let buf = v.load_mut(&*name)
            .ok_or_else(move || -> MacroError { format!("variable {} does not exist", String::from_utf8_lossy(&*name)).into() })?;
        let index = Number::try_from(index).map(i64::from)?;
        let index = usize::try_from(index).map_err(|_| "invalid index")?;
        let byte = buf.get_mut(index).ok_or("index out of bounds")?;
        let value = Number::try_from(value).map(i64::from)?;
        let value = u8::try_from(value).map_err(|_| "invalid byte")?;
        *byte = value;
        Ok(Cow::Borrowed(b""))
    }

    /// Gets a single byte of a variable as a hexadecimal value.
    /// # Arguments
    /// 1. The variable to index into.
    /// 2. The byte index in the variable. Must be greater than or equal to 0.
    pub macro ByteGet [b"byte.get"] (name, index) + _x, v, _r {
        let buf = v.load(&*name)
            .ok_or_else(move || -> MacroError { format!("variable {} does not exist", String::from_utf8_lossy(&*name)).into() })?;
        let index = Number::try_from(index).map(i64::from)?;
        let index = usize::try_from(index).map_err(|_| "invalid index")?;
        let byte = buf.get(index).ok_or("index out of bounds")?;
        Ok(Cow::Owned(format!("{byte:02x}").into_bytes()))
    }

    /// Splices a string of hexadecimal bytes into a variable.
    /// # Arguments
    /// 1. The variable to splice.
    /// 2. The hexadecimal string splice into the byte.
    /// 3. The byte index to start in the variable. Must be greater than or equal to 0.
    /// 4? The byte index to end in the variable. Must be greater than or equal to 0. Defaults to the end of the string.
    pub macro ByteSplice [b"byte.splice"] (name, value, start, ...iter) + _x, v, _r {
        let buf = v.load_mut(&*name)
            .ok_or_else(move || -> MacroError { format!("variable {} does not exist", String::from_utf8_lossy(&*name)).into() })?;

        if value.len() % 2 != 0 { return Err("hexstring length must be even")? }

        let start = usize::try_from(Number::try_from(start).map(i64::from)?).map_err(|_| "invalid index")?;
        let end = match iter.next() {
            Some(end) => usize::try_from(Number::try_from(end).map(i64::from)?).map_err(|_| "invalid index")?,
            None => buf.len()
        };

        let prefix = buf.get(..start).ok_or("start index out of bounds")?;
        let suffix = buf.get(end..).ok_or("end index out of bounds")?;
        let mut buf = Vec::from(prefix);
        buf.try_reserve(value.len() / 2 + suffix.len())?;
        for hex in value.chunks(2) {
            let byte_str = str::from_utf8(hex)?;
            buf.push(u8::from_str_radix(byte_str, 16).map_err(|_| format!("invalid byte: {byte_str}"))?);
        }
        buf.extend(suffix);
        Ok(Cow::Borrowed(b""))
    }

    /// Calculates the binary AND of the given values. All values will be coerced to integers.
    /// # Arguments
    /// 1... The numbers to operate on.
    pub macro BitAnd [b"bit.and"] (...args) + _x, _v, _r {
        args
            .map(|v| Number::try_from(&*v).map(i64::from))
            .process_results(|it| {
                Cow::Owned(format!("{}", it.fold(!0i64, |a, b| a & b)).into_bytes())
            })
    }

    /// Calculates the binary OR of the given values. All values will be coerced to integers.
    /// # Arguments
    /// 1... The numbers to operate on.
    pub macro BitOr [b"bit.or"] (...args) + _x, _v, _r {
        args
            .map(|v| Number::try_from(&*v).map(i64::from))
            .process_results(|it| {
                Cow::Owned(format!("{}", it.fold(0i64, |a, b| a | b)).into_bytes())
            })
    }

    /// Calculates the binary XOR of the given values. All values will be coerced to integers.
    /// # Arguments
    /// 1... The numbers to operate on.
    pub macro BitXor [b"bit.xor"] (...args) + _x, _v, _r {
        args
            .map(|v| Number::try_from(&*v).map(i64::from))
            .process_results(|it| {
                Cow::Owned(format!("{}", it.fold(0i64, |a, b| a ^ b)).into_bytes())
            })
    }

    /// Calculates the binary NOT of the given values individually. All values will be coerced to integers.
    /// # Arguments
    /// 1... The numbers to operate on.
    pub macro BitNot [b"bit.not"] (...args) + _x, _v, _r {
        args
            .map(|v| Number::try_from(&*v).map(i64::from))
            .process_results(|it| {
                Cow::Owned(it.map(|v| format!("{}", (!v))).join("/").into_bytes())
            })
    }

    /// Calculates the left bit shift of the given value. All values will be coerced to integers.
    /// # Arguments
    /// 1. The number to shift.
    /// 2. The amount to shift by. Must be in the range of 0 to 63, inclusive.
    pub macro BitLShift [b"bit.shl"] (value, amount) + _x, _v, _r {
        let value = Number::try_from(value).map(i64::from)?;
        let amount = Number::try_from(amount).map(i64::from)?;
        if !(0..=63).contains(&amount) { return Err("shift amount must be in range 0 ..= 63")? }
        Ok(Cow::Owned(format!("{}", value.unbounded_shl(amount as u32)).into_bytes()))
    }

    /// Calculates the arithmetic right bit shift of the given value. All values will be coerced to integers.
    /// # Arguments
    /// 1. The number to shift.
    /// 2. The amount to shift by. Must be in the range of 0 to 63, inclusive.
    pub macro BitARShift [b"bit.ashr"] (value, amount) + _x, _v, _r {
        let value = Number::try_from(value).map(i64::from)?;
        let amount = Number::try_from(amount).map(i64::from)?;
        if !(0..=63).contains(&amount) { return Err("shift amount must be in range 0 ..= 63")? }
        Ok(Cow::Owned(format!("{}", value.unbounded_shr(amount as u32)).into_bytes()))
    }

    /// Calculates the logical right bit shift of the given value. All values will be coerced to integers.
    /// # Arguments
    /// 1. The number to shift.
    /// 2. The amount to shift by. Must be in the range of 0 to 63, inclusive.
    pub macro BitLRShift [b"bit.lshr"] (value, amount) + _x, _v, _r {
        let value = Number::try_from(value).map(i64::from)?;
        let amount = Number::try_from(amount).map(i64::from)?;
        if !(0..=63).contains(&amount) { return Err("shift amount must be in range 0 ..= 63")? }
        Ok(Cow::Owned(format!("{}", (value as u64).unbounded_shr(amount as u32) as i64).into_bytes()))
    }

    /// Checks if the given input value to a text macro was used.
    /// # Arguments
    /// 1. The value to check.
    pub macro Input [b"input"] (value) + _x, _v, _r {
        if value.len() >= 2 && value[0] == b'$' && (matches!(value, b"$!" | b"$#") ||
            str::from_utf8(&value[1..]).map_err(|_| ()).and_then(|v| str::parse::<u64>(v).map_err(|_| ())).is_ok())
        {
            Ok(Cow::Borrowed(b"false"))
        } else {
            Ok(Cow::Borrowed(b"true"))
        }
    }

    /// Interpolates a value using a given time and easing method.
    /// 
    /// Supported easings:
    /// `back`, `bounce`, `circ`, `elastic`, `expo`, `sine`, `quad`, `cubic`, `quart`, `quint`, `linear`
    /// 
    /// All easings except for `linear` must be followed by `_in`, `_out`, or `_in_out`.
    /// 
    /// For more information, see https://easings.net/.
    /// 
    /// # Arguments
    /// 1. The number at the start of the easing animation.
    /// 2. The number at the end of the easing animation.
    /// 3. The time the easing animation should calculate.
    /// 4. The easing animation kind.
    pub macro Ease [b"ease"] (start, end, t, kind) + _x, _v, _r {
        let start = Number::try_from(start)?;
        let end = Number::try_from(end)?;
        let t = Number::try_from(t)?;
        let easing_function: fn(f32) -> f32 = match kind {
            b"back_in" => simple_easing::back_in,
            b"back_in_out" => simple_easing::back_in_out,
            b"back_out" => simple_easing::back_out,
            b"bounce_in" => simple_easing::bounce_in,
            b"bounce_in_out" => simple_easing::bounce_in_out,
            b"bounce_out" => simple_easing::bounce_out,
            b"circ_in" => simple_easing::circ_in,
            b"circ_in_out" => simple_easing::circ_in_out,
            b"circ_out" => simple_easing::circ_out,
            b"cubic_in" => simple_easing::cubic_in,
            b"cubic_in_out" => simple_easing::cubic_in_out,
            b"cubic_out" => simple_easing::cubic_out,
            b"elastic_in" => simple_easing::elastic_in,
            b"elastic_in_out" => simple_easing::elastic_in_out,
            b"elastic_out" => simple_easing::elastic_out,
            b"expo_in" => simple_easing::expo_in,
            b"expo_in_out" => simple_easing::expo_in_out,
            b"expo_out" => simple_easing::expo_out,
            b"linear" => simple_easing::linear,
            b"quad_in" => simple_easing::quad_in,
            b"quad_in_out" => simple_easing::quad_in_out,
            b"quad_out" => simple_easing::quad_out,
            b"quart_in" => simple_easing::quart_in,
            b"quart_in_out" => simple_easing::quart_in_out,
            b"quart_out" => simple_easing::quart_out,
            b"quint_in" => simple_easing::quint_in,
            b"quint_in_out" => simple_easing::quint_in_out,
            b"quint_out" => simple_easing::quint_out,
            b"sine_in" => simple_easing::sine_in,
            b"sine_in_out" => simple_easing::sine_in_out,
            b"sine_out" => simple_easing::sine_out,
            other => return Err(format!("unsupported easing mode: {}", String::from_utf8_lossy(other)))?
        };
        let mul = easing_function(f64::from(t) as f32);
        let interpolated = start * Number::Float((1.0 - mul) as f64) + end * Number::Float(mul as f64);
        Ok(Cow::Owned(format!("{}", interpolated).into_bytes()))
    }

    /// Parses a RPN expression and saves it to a function variable.
    ///
    /// # Syntax
    /// Expressions are defined using Reverse Polish Notation.
    /// For example, `1 2 +` -> `3`.
    /// 
    /// Each operator or number (generally called a _node_) must be
    /// separated by at least one whitespace character.
    /// Also supported is the node `$N`, for input values, and
    /// `#<ident>`, which allows calling other expressions inside of an expression.
    /// 
    /// Calling an expression will pop its required arguments from the stack.
    /// For example, `[expr.def/inc/1 +][expr.call/inc/5]` -> `6`.
    /// ## Supported Operators
    /// - `**`: Exponent
    /// - `log`: Log of arg 1 with base of arg 2
    /// - `abs`: Absolute value
    /// - `<=>`: Three-way comparison
    /// - `!=`: Not equal
    /// - `==`: Equal
    /// - `<=`: Less or equal
    /// - `>=`: Greater or equal
    /// - `<<`: Left shift
    /// - `>>`: Logical right shift
    /// - `>>>`: Arithmetic right shift
    /// - `<`: Less
    /// - `>`: Greater
    /// - `+`: Add
    /// - `-`: Subtract
    /// - `*`: Multiply
    /// - `/`: Divide
    /// - `%`: Modulus
    /// - `~`: Negate
    /// - `?`: Ternary (if first argument is nonzero, choose first argument, otherwise choose second argument)
    /// - `&`: Bitwise AND
    /// - `|`: Bitwise OR
    /// - `^`: Bitwise XOR
    /// - `!`: Bitwise NOT
    /// - `sin`: Sine
    /// - `cos`: Cosine
    /// - `tan`: Tangent
    /// - `asin`: Arcsine
    /// - `acos`: Arccosine
    /// - `atan`: Arctangent
    /// - `real`: Real component of complex number
    /// - `imag`: Imaginary component of complex number
    /// - `arg`: Argument of complex number
    ///
    /// # Arguments
    /// 1. The name to save the expression under.
    /// 2... The expression
    pub macro ExprDef [b"expr.def"] (name, ...value) + _x, v, _r {
        let expr_str = value.intersperse(b"/").flatten().copied().collect::<Vec<u8>>();
        let expr = ExpressionFunction::parse(&expr_str)?;
        v.store_fn(name, expr);
        Ok(Cow::Borrowed(b""))
    }

    /// Evaluates a stored RPN expression.
    /// # Arguments
    /// 1. The name of the expression to call.
    /// 2... The expression arguments. Must all be numbers.
    pub macro ExprCall [b"expr.call"] (name, ...args) + _x, v, _r {
        let expr = v.load_fn(name).ok_or("expression is undefined")?;
        let args = args.map(|v| Number::try_from(&*v)).collect::<Result<Vec<_>, _>>()?;
        let res = expr.exec(&args, &*v, &name)?;
        Ok(Cow::Owned(format!("{res}").into_bytes()))
    }

    /// Evaluates an RPN expression. See [expr.def].
    /// # Arguments
    /// 1... The expression.
    pub macro Expr [b"expr"] (...value) + _x, v, _r {
        let expr_str = value.intersperse(b"/").flatten().copied().collect::<Vec<u8>>();
        let expr = ExpressionFunction::parse(&expr_str)?;
        let res = expr.exec(&[], &*v, b"<inline>")?;
        Ok(Cow::Owned(format!("{res}").into_bytes()))
    }
}

/// Static block of bytes that can be used to turn a `u8` into a `&'static u8`.
pub(crate) static BYTES: [u8; 256] = [
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
