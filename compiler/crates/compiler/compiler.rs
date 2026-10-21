//! Courier's schema compiler: schema text in, Luau out.

pub mod codegen;
mod compile;
mod error;
pub mod layout;
pub mod luau;
pub mod schema;

pub use compile::{compile, compile_schema};
pub use error::{Error, LayoutError, SchemaError, Span};
