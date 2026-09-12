//! Handles text-defined macros.

use rand_xoshiro::Xoshiro128PlusPlus;

use crate::stdlib::BYTES;
use std::borrow::Cow;
use std::sync::Arc;

use crate::Macro;

#[derive(Debug, Clone, PartialEq)]
/// A macro defined using text.
pub struct TextMacro {
    /// The source backing the macro.
    pub source: Arc<Vec<u8>>,
    /// The macro's description.
    pub description: Arc<String>,
    /// The macro's name.
    pub name: Arc<Vec<u8>>,
}

impl Macro for TextMacro {
    fn name(&self) -> &[u8] {
        &*self.name
    }

    fn source(&self) -> &[u8] {
        &*self.source
    }

    fn description(&self) -> &str {
        &*self.description
    }

    fn clone(&self) -> Box<dyn Macro> {
        Box::new(TextMacro {
            source: self.source.clone(),
            description: self.description.clone(),
            name: self.name.clone(),
        })
    }

    fn eval<'arg, 'reg: 'arg, 'exec: 'reg>(
        &self,
        exec: &'exec crate::exec::Executor,
        _vars: &'reg mut crate::VariableRegistry,
        _rng: &mut Xoshiro128PlusPlus,
        args: &mut dyn Iterator<Item = &'arg [u8]>,
    ) -> Result<std::borrow::Cow<'static, [u8]>, crate::MacroError> {
        let args = args.collect::<Vec<_>>();
        // None is a sentinel value for $ contained in expanded values.
        // This is pretty inefficient way of doing it, but eh. Don't really care.
        let mut source = self.source.iter().copied().map(Some).collect::<Vec<_>>();
        while let Some((i, _)) = source
            .iter()
            .enumerate()
            .rfind(|(_, c)| c.is_some_and(|c| c == b'$'))
        {
            match (|| {
                // i has the index of the rightmost $ not already found
                let next_char = source.get(i + 1)?.as_ref()?;
                match next_char {
                    b'#' => Some((2, Cow::Owned(format!("{}", args.len()).into_bytes()))),
                    b'!' => Some((
                        2,
                        Cow::Borrowed(std::slice::from_ref(&BYTES[exec.context() as usize])),
                    )),
                    b'0' => Some((2, args.join(b"/" as &[u8]).into())),
                    c if c.is_ascii_digit() || *c == b'-' => {
                        let mut start = i + 1;
                        let mut from_back = false;
                        if *c == b'-' {
                            start += 1;
                            from_back = true;
                        }
                        let mut end = i + 2;
                        while let Some(byte) = source.get(end).and_then(Option::as_ref)
                            && byte.is_ascii_digit()
                        {
                            end += 1;
                        }
                        let source_slice = source
                            .get(start..end)?
                            .iter()
                            .copied()
                            .filter_map(|v| v)
                            .collect::<Vec<_>>();
                        let mut idx = isize::from_ascii_bytes(&source_slice).ok()?;
                        if from_back {
                            if idx == 0 {
                                return Some((3, args.join(b"/" as &[u8]).into()));
                            }
                            idx = (args.len() as isize) - idx + 1;
                        }
                        args.get(usize::try_from(idx).ok().and_then(|v| v.checked_sub(1))?)
                            .map(|arg| (end - i, Cow::Borrowed(*arg)))
                    }
                    _ => None,
                }
            })() {
                Some((len, s)) => {
                    let tail = source.split_off(i);
                    source.reserve(s.len() + tail.len());
                    source.extend(s.iter().map(|c| (*c != b'$').then_some(*c)));
                    source.extend(tail[len..].iter().copied());
                }
                None => source[i] = None,
            }
        }
        Ok(Cow::Owned(
            source.iter().copied().map(|v| v.unwrap_or(b'$')).collect(),
        ))
    }
}
