use std::collections::TryReserveError;
use std::str::Utf8Error;
use std::string::FromUtf8Error;

use rand_xoshiro::Xoshiro128PlusPlus;

use crate::Cow;
use crate::var_reg::VariableRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
/// An error struct representing what went wrong during a macro call.
pub struct MacroError {
    pub(crate) message: Cow<'static, str>,
    pub(crate) trace: Vec<Vec<u8>>,
}

impl MacroError {
    /// Returns the error message.
    pub fn message(&self) -> &str {
        &*self.message
    }

    /// Returns a traceback for the given error, in reverse order.
    pub fn trace(&self) -> &[Vec<u8>] {
        &self.trace
    }
}

impl From<&'static str> for MacroError {
    fn from(value: &'static str) -> Self {
        Self {
            message: Cow::Borrowed(value),
            trace: Vec::new(),
        }
    }
}
impl From<Utf8Error> for MacroError {
    fn from(_value: Utf8Error) -> Self {
        Self {
            message: Cow::Borrowed("string was not valid UTF-8"),
            trace: Vec::new(),
        }
    }
}
impl From<TryReserveError> for MacroError {
    fn from(_value: TryReserveError) -> Self {
        Self {
            message: Cow::Borrowed("ran out of memory"),
            trace: Vec::new(),
        }
    }
}
impl From<FromUtf8Error> for MacroError {
    fn from(_value: FromUtf8Error) -> Self {
        Self {
            message: Cow::Borrowed("string was not valid UTF-8"),
            trace: Vec::new(),
        }
    }
}

impl From<String> for MacroError {
    fn from(value: String) -> Self {
        Self {
            message: Cow::Owned(value),
            trace: Vec::new(),
        }
    }
}

impl std::fmt::Display for MacroError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}\n\nTraceback:", self.message)?;
        for step in self.trace.iter().rev() {
            writeln!(f, "-----")?;
            writeln!(f, "{}", String::from_utf8_lossy(step))?;
        }
        writeln!(f, "-----")
    }
}

impl std::error::Error for MacroError {}

/// Defines a struct as a macro.
pub trait Macro: Send + Sync {
    /// The macro's defined name.
    fn name(&self) -> &[u8];
    /// The macro's description.
    fn description(&self) -> &str;
    /// The macro's source code.
    fn source(&self) -> &[u8];
    /// Evaluates the macro.
    fn eval<'arg, 'reg: 'arg, 'exec: 'reg>(
        &self,
        exec: &'exec crate::exec::Executor,
        vars: &'reg mut VariableRegistry,
        rng: &mut Xoshiro128PlusPlus,
        args: &mut dyn Iterator<Item = &'arg [u8]>,
    ) -> Result<Cow<'static, [u8]>, MacroError>;
    /// Clones this macro into a box;
    fn clone(&self) -> Box<dyn Macro>;
}
