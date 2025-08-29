#![feature(int_from_ascii)]
#![warn(clippy::pedantic, clippy::perf, missing_docs)]
#![allow(unstable_name_collisions)]
//! Macroscript reimplementation in Rust.

use std::borrow::Cow;

mod number;
mod macro_trait;
pub mod stdlib;
mod var_reg;
mod exec;
mod text_macro;

pub use macro_trait::{MacroError, Macro};
pub use number::Number;
pub use exec::Executor;
pub use var_reg::VariableRegistry;
pub use text_macro::TextMacro;
