
use crate::var_reg::VariableRegistry;
use crate::Cow;

#[derive(Debug, Clone, PartialEq, Eq)]
/// An error struct representing what went wrong during a macro call.
pub struct MacroError {
    pub(crate) message: Cow<'static, str>,
    pub(crate) context: String
}

impl From<&'static str> for MacroError {
    fn from(value: &'static str) -> Self {
        Self { message: Cow::Borrowed(value), context: String::new() }
    }
}

impl From<String> for MacroError {
    fn from(value: String) -> Self {
        Self { message: Cow::Owned(value), context: String::new() }
    }
}

impl std::fmt::Display for MacroError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Error while expanding this block: {}\n{}", self.context, self.message)
    }
}

impl std::error::Error for MacroError {}

/// Defines a struct as a macro.
pub trait Macro {
    /// The macro's defined name.
    fn name(&self) -> Cow<'static, [u8]>;
    /// Evaluates the macro.
    fn eval<'arg, 'reg: 'arg, 'exec: 'reg>(
        &self,
        exec: &'exec crate::exec::Executor,
        vars: &'reg mut VariableRegistry,
        rng: &mut rand::rngs::SmallRng,
        args: &mut dyn Iterator<Item = &'arg [u8]>,
    ) -> Result<Cow<'static, [u8]>, MacroError>;
}
