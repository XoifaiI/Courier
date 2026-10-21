//! Layout: where every value goes in bits, decided before any Luau is generated.

mod builder;
mod numbers;
mod packing;
mod types;

pub use builder::{Measure, layout, measure, varies};
pub use numbers::{IdCodes, bits_for, float_steps, mask, rotation_steps};
pub use packing::{Chunk, MAX_PRODUCT, group_bits};
pub use types::*;
