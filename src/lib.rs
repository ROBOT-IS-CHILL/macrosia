#![feature(int_from_ascii, vec_deque_retain_range, deque_extend_front)]
#![warn(clippy::pedantic, clippy::perf, missing_docs)]
#![allow(unstable_name_collisions)]
//! Macrosia, in Rust.

use std::borrow::Cow;

mod exec;
mod expr;
pub mod intern;
mod macro_trait;
mod number;
pub mod stdlib;
mod text_macro;
mod var_reg;

pub use rand_xoshiro;
pub use regex;

pub use exec::Executor;
pub use macro_trait::{Macro, MacroError};
pub use number::Number;
pub use text_macro::TextMacro;
pub use var_reg::VariableRegistry;

#[test]
fn full_test() -> Result<(), Box<dyn std::error::Error>> {
    let string = b"
        [subtract/0/[subtract/[unixtime]/[/[expr/1 1 +]][unixtime]]]
    ";

    static KILL_MACROS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    let mut exec = crate::Executor::new(b't');
    exec.add_stdlib();
    let mut reg = VariableRegistry::new();
    let mut func = exec.evaluate(string, &mut reg, None, None, &KILL_MACROS);
    let out = loop {
        if let Some(v) = func() {
            break v;
        }
    }.map_err(|v| panic!("{v}"))?;
    println!("{}", String::from_utf8_lossy(&out));
    Ok(())
}