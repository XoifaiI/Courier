//! A small Luau AST and its printer.

mod ast;
pub mod builtins;
mod printer;

pub use ast::*;
pub use printer::{number, print};
