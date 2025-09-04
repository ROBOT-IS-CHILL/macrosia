#![feature(int_from_ascii)]
#![warn(clippy::pedantic, clippy::perf, missing_docs)]
#![allow(unstable_name_collisions)]
//! Macrosia, in Rust.

use std::borrow::Cow;

mod exec;
mod macro_trait;
mod number;
pub mod stdlib;
mod text_macro;
mod var_reg;

pub use rand;
pub use regex;

pub use exec::Executor;
pub use macro_trait::{Macro, MacroError};
pub use number::Number;
pub use text_macro::TextMacro;
pub use var_reg::VariableRegistry;
