//! Everything the compiler can refuse, worded the way the CLI prints it.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub file: u32,
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for Span {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "{}:{}", self.line, self.column)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}: {1}")]
    Schema(Span, SchemaError),
    #[error("{path}: {problem}")]
    Layout { path: String, problem: LayoutError },
    #[error(
        "the module needs {needed} top level locals, over Luau's limit of 200, one for each enum, union and function; merge some"
    )]
    TooManyLocals { needed: usize },
}

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("unexpected character '{0}'")]
    UnexpectedCharacter(char),
    #[error("\"{0}\" is not a number")]
    NotANumber(String),
    #[error("expected {expected}, got {found}")]
    Expected { expected: String, found: String },
    #[error("\"{0}\" is reserved by Luau or the generated code")]
    ReservedName(String),
    #[error("\"{name}\" is already declared at {at}")]
    AlreadyDeclared { name: String, at: String },
    #[error("{kind} \"{name}\" is repeated")]
    Repeated { kind: &'static str, name: String },
    #[error("field name \"{0}\" is a Luau keyword")]
    KeywordField(String),
    #[error("field name \"{0}\" is reserved by the generated code")]
    ReservedField(String),
    #[error("variant field \"Kind\" is reserved for the variant's name")]
    VariantTag,
    #[error("argument name \"CallId\" is reserved for the call id")]
    CallIdArgument,
    #[error("unknown type \"{0}\"; declare it as an enum, struct or union in any schema of this folder")]
    UnknownType(String),
    #[error("\"{0}\" cannot hold itself")]
    HoldsItself(String),
    #[error("integer width must be 1 to 32, got {0}")]
    IntWidth(u64),
    #[error("int needs whole numbers with min below max, got {min}..{max}")]
    IntRange { min: f64, max: f64 },
    #[error("{kind} needs min below max and a positive step, got {min}..{max}, {step}")]
    QuantizedRange { kind: &'static str, min: f64, max: f64, step: f64 },
    #[error("a rotation's error must be 0.001 to 45 degrees, got {0}")]
    Degrees(f64),
    #[error("a size needs whole numbers from 0 to 16777215 with min at most max, got {min}..{max}")]
    Size { min: f64, max: f64 },
    #[error("a type is optional once")]
    OptionalTwice,
    #[error(
        "an array cannot hold optional values, since a Luau array cannot hold nil; use an array of structs with an optional field"
    )]
    OptionalElements,
    #[error("a map key must be a bool, a number, an enum or a string")]
    MapKey,
    #[error("a map cannot hold optional values, since a Luau table cannot hold nil")]
    OptionalMapValues,
    #[error("text needs a closing \" on the same line")]
    UnterminatedText,
    #[error("an enum needs at least two values")]
    TooFewValues,
    #[error("a struct needs at least one field")]
    EmptyStruct,
    #[error("a union needs at least two variants")]
    TooFewVariants,
    #[error("unreliable event \"{0}\" cannot carry Instances")]
    UnreliableInstances(String),
}

#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    #[error("needs {0} bits; a field may use at most 32, so narrow its range or coarsen its step")]
    LeafTooWide(u32),
    #[error("an array element must carry at least one bit or an Instance")]
    EmptyElement,
}
