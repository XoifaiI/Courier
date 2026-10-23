//! Turns a laid out schema into Luau.

mod api;
mod declarations;
mod emitter;
mod module;
mod reader;
mod types;
mod writer;

pub use module::{File, Mode, Options, Runtime, generate};
