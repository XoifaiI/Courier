//! Parses and checks a schema, then turns each function into a request and a reply event.
//!
//! ```text
//! Schema   := { Option | Enum | Struct | Union | Event | Function }
//! Option   := "option" ("output" | "runtime") "=" Text
//! Enum     := "enum" Name "{" Name { "," Name } [","] "}"
//! Struct   := "struct" Name "{" Field { "," Field } [","] "}"
//! Union    := "union" Name "{" Variant { "," Variant } [","] "}"
//! Variant  := Name [ "{" Field { "," Field } [","] "}" ]
//! Event    := "event" Name "from" Side ["unreliable"] Fields
//! Function := "function" Name "from" Side Fields "->" Fields
//! Side     := "Server" | "Client"
//! Fields   := "{" [Field { "," Field } [","]] "}"
//! Field    := Name ":" Type
//! Type     := Base { "?" | "[" [Bounds] "]" }
//! Base     := "bool" | "u"N | "i"N | "int" "(" Number ".." Number ")" | "f32" | "f64"
//!           | "float" "(" Number ".." Number "," Number ")"
//!           | "vector" ["(" Number ".." Number "," Number ")"] | "rotation" "(" Number ")"
//!           | "cframe" "(" Number ".." Number "," Number "," Number ")"
//!           | ("string" | "buffer") ["(" Bounds ")"] | "Instance" ["(" ClassName ")"] | "Handle"
//!           | "map" "(" Type "," Type ["," Bounds] ")" | EnumName | StructName | UnionName
//! Bounds   := Number | ".." Number | Number ".." Number
//! ```

use std::borrow::Cow;
use std::collections::HashMap;

use super::lexer::{Token, tokenize};
use super::names::{KEYWORDS, RESERVED_TYPES, is_reserved_field};
use super::types::{
    Bounds, Declaration, Enum, EnumId, Event, Field, Function, Role, Schema, Side, Struct, StructId, Type, TypeId,
    UNION_TAG, Union, UnionId, Variant,
};
use crate::error::{Error, SchemaError, Span};

const LARGEST_COUNT: f64 = 16_777_215.0;

type Parsed<T> = Result<T, (Span, SchemaError)>;

pub fn parse(source: &str) -> Result<Schema<'_>, Error> {
    let tokens = tokenize(source).map_err(|(span, error)| Error::Schema(span, error))?;
    let mut parser = Parser { tokens, index: 0, schema: Schema::default(), declared: HashMap::new() };
    parser.schema().map_err(|(span, error)| Error::Schema(span, error))?;
    let mut schema = parser.schema;
    schema.desugar();
    Ok(schema)
}

#[derive(Clone, Copy)]
enum Declared {
    Enum(EnumId),
    Struct(StructId),
    Union(UnionId),
    Open,
    Other,
}

struct Parser<'s> {
    tokens: Vec<(Token<'s>, Span)>,
    index: usize,
    schema: Schema<'s>,
    declared: HashMap<Cow<'s, str>, (Span, Declared)>,
}

impl<'s> Parser<'s> {
    fn peek(&self) -> Token<'s> {
        self.tokens[self.index].0
    }

    fn next(&mut self) -> (Token<'s>, Span) {
        let token = self.tokens[self.index];
        if self.index + 1 < self.tokens.len() {
            self.index += 1;
        }
        token
    }

    fn expected<T>(&self, (token, span): (Token<'s>, Span), expected: impl Into<String>) -> Parsed<T> {
        Err((span, SchemaError::Expected { expected: expected.into(), found: token.describe() }))
    }

    fn symbol(&mut self, symbol: &'static str) -> Parsed<Span> {
        match self.next() {
            (Token::Symbol(found), span) if found == symbol => Ok(span),
            token => self.expected(token, format!("\"{symbol}\"")),
        }
    }

    fn name(&mut self, what: &'static str) -> Parsed<(&'s str, Span)> {
        match self.next() {
            (Token::Name(name), span) => Ok((name, span)),
            token => self.expected(token, what),
        }
    }

    fn number(&mut self) -> Parsed<f64> {
        match self.next() {
            (Token::Number(number), _) => Ok(number),
            token => self.expected(token, "a number"),
        }
    }

    fn at(&self, symbol: &str) -> bool {
        matches!(self.peek(), Token::Symbol(found) if found == symbol)
    }

    fn eat(&mut self, symbol: &str) -> bool {
        let found = self.at(symbol);
        if found {
            self.next();
        }
        found
    }

    fn list(&mut self, mut item: impl FnMut(&mut Self) -> Parsed<()>) -> Parsed<()> {
        self.symbol("{")?;
        while !self.at("}") {
            item(self)?;
            if !self.eat(",") && !self.at("}") {
                return self.expected(self.tokens[self.index], "\",\" or \"}\"");
            }
        }
        self.symbol("}")?;
        Ok(())
    }

    fn declare(&mut self, name: Cow<'s, str>, span: Span, declared: Declared) -> Parsed<()> {
        if KEYWORDS.contains(&name.as_ref()) || RESERVED_TYPES.contains(&name.as_ref()) {
            return Err((span, SchemaError::ReservedName(name.into_owned())));
        }
        if let Some((first, _)) = self.declared.get(&name) {
            return Err((span, SchemaError::AlreadyDeclared { name: name.into_owned(), line: first.line }));
        }
        self.declared.insert(name, (span, declared));
        Ok(())
    }

    fn close(&mut self, name: &str, declared: Declared) {
        if let Some((_, open)) = self.declared.get_mut(name) {
            *open = declared;
        }
    }

    fn bounds(&mut self, closing: &str) -> Parsed<Bounds> {
        let span = self.tokens[self.index].1;
        if self.at(closing) {
            return Ok(Bounds::UNBOUNDED);
        }
        let (min, max) = if self.eat("..") {
            (0.0, self.number()?)
        } else {
            let first = self.number()?;
            if self.eat("..") { (first, self.number()?) } else { (first, first) }
        };
        if min.fract() != 0.0 || max.fract() != 0.0 || min < 0.0 || min > max || max > LARGEST_COUNT {
            return Err((span, SchemaError::Size { min, max }));
        }
        Ok(Bounds { min: min as u32, max: Some(max as u32) })
    }

    fn degrees(&mut self, span: Span) -> Parsed<f64> {
        let degrees = self.number()?;
        if (0.001..=45.0).contains(&degrees) { Ok(degrees) } else { Err((span, SchemaError::Degrees(degrees))) }
    }

    fn quantized(&mut self, kind: &'static str, span: Span) -> Parsed<(f64, f64, f64)> {
        let min = self.number()?;
        self.symbol("..")?;
        let max = self.number()?;
        self.symbol(",")?;
        let step = self.number()?;
        if min >= max || step <= 0.0 {
            return Err((span, SchemaError::QuantizedRange { kind, min, max, step }));
        }
        Ok((min, max, step))
    }

    fn base(&mut self) -> Parsed<Type<'s>> {
        let (name, span) = self.name("a type")?;
        let ty = match name {
            "bool" => Type::Bool,
            "int" => {
                self.symbol("(")?;
                let min = self.number()?;
                self.symbol("..")?;
                let max = self.number()?;
                self.symbol(")")?;
                if min.fract() != 0.0 || max.fract() != 0.0 || min >= max {
                    return Err((span, SchemaError::IntRange { min, max }));
                }
                Type::Int { min: min as i64, max: max as i64, wraps: false }
            }
            "vector" if !self.at("(") => Type::RealVector,
            "f32" => Type::Real { wide: false },
            "f64" => Type::Real { wide: true },
            "map" => {
                self.symbol("(")?;
                let key = self.ty()?;
                if !matches!(
                    self.schema.ty(key),
                    Type::Bool
                        | Type::Int { .. }
                        | Type::Float { .. }
                        | Type::Real { .. }
                        | Type::Enum(_)
                        | Type::Text(_)
                ) {
                    return Err((span, SchemaError::MapKey));
                }
                self.symbol(",")?;
                let value = self.ty()?;
                if matches!(self.schema.ty(value), Type::Optional(_)) {
                    return Err((span, SchemaError::OptionalMapValues));
                }
                let bounds = if self.eat(",") { self.bounds(")")? } else { Bounds::UNBOUNDED };
                self.symbol(")")?;
                Type::Map { key, value, bounds }
            }
            "float" | "vector" => {
                self.symbol("(")?;
                let kind = if name == "float" { "float" } else { "vector" };
                let (min, max, step) = self.quantized(kind, span)?;
                self.symbol(")")?;
                if name == "float" { Type::Float { min, max, step } } else { Type::Vector { min, max, step } }
            }
            "cframe" => {
                self.symbol("(")?;
                let (min, max, step) = self.quantized("cframe", span)?;
                self.symbol(",")?;
                let degrees = self.degrees(span)?;
                self.symbol(")")?;
                Type::CFrame { min, max, step, degrees }
            }
            "rotation" => {
                self.symbol("(")?;
                let degrees = self.degrees(span)?;
                self.symbol(")")?;
                Type::Rotation { degrees }
            }
            "Instance" => {
                let class = if self.eat("(") {
                    let (class, _) = self.name("a class name")?;
                    self.symbol(")")?;
                    Some(Cow::Borrowed(class))
                } else {
                    None
                };
                Type::Instance { class }
            }
            "Handle" => Type::Handle,
            "string" | "buffer" => {
                let bounds = if self.eat("(") {
                    let bounds = self.bounds(")")?;
                    self.symbol(")")?;
                    bounds
                } else {
                    Bounds::UNBOUNDED
                };
                if name == "string" { Type::Text(bounds) } else { Type::Buffer(bounds) }
            }
            _ => match sized_int(name) {
                Some((_, width)) if !(1..=32).contains(&width) => return Err((span, SchemaError::IntWidth(width))),
                Some((true, width)) => Type::Int { min: -(1 << (width - 1)), max: (1 << (width - 1)) - 1, wraps: true },
                Some((false, width)) => Type::Int { min: 0, max: (1 << width) - 1, wraps: true },
                None => match self.declared.get(name).map(|(_, declared)| *declared) {
                    Some(Declared::Enum(id)) => Type::Enum(id),
                    Some(Declared::Struct(id)) => Type::Struct(id),
                    Some(Declared::Union(id)) => Type::Union(id),
                    Some(Declared::Open) => return Err((span, SchemaError::HoldsItself(name.to_string()))),
                    Some(Declared::Other) | None => return Err((span, SchemaError::UnknownType(name.to_string()))),
                },
            },
        };
        Ok(ty)
    }

    fn ty(&mut self) -> Parsed<TypeId> {
        let base = self.base()?;
        let mut ty = self.schema.intern(base);
        loop {
            if self.at("?") {
                let (_, span) = self.next();
                if matches!(self.schema.ty(ty), Type::Optional(_)) {
                    return Err((span, SchemaError::OptionalTwice));
                }
                ty = self.schema.intern(Type::Optional(ty));
            } else if self.at("[") {
                let (_, span) = self.next();
                if matches!(self.schema.ty(ty), Type::Optional(_)) {
                    return Err((span, SchemaError::OptionalElements));
                }
                let bounds = self.bounds("]")?;
                self.symbol("]")?;
                ty = self.schema.intern(Type::Array { element: ty, bounds });
            } else {
                return Ok(ty);
            }
        }
    }

    fn fields(&mut self, parameters: bool) -> Parsed<Vec<Field<'s>>> {
        let mut fields: Vec<Field<'s>> = Vec::new();
        self.list(|parser| {
            let (name, span) = parser.name("a field name")?;
            if fields.iter().any(|field| field.name == name) {
                return Err((span, SchemaError::Repeated { kind: "field", name: name.to_string() }));
            }
            if KEYWORDS.contains(&name) {
                return Err((span, SchemaError::KeywordField(name.to_string())));
            }
            if parameters && is_reserved_field(name) {
                return Err((span, SchemaError::ReservedField(name.to_string())));
            }
            parser.symbol(":")?;
            let ty = parser.ty()?;
            fields.push(Field { name: Cow::Borrowed(name), ty });
            Ok(())
        })?;
        Ok(fields)
    }

    fn enumeration(&mut self) -> Parsed<()> {
        let (name, span) = self.name("an enum name")?;
        let id = EnumId(self.schema.enums.len() as u32);
        self.declare(Cow::Borrowed(name), span, Declared::Enum(id))?;
        let mut values: Vec<Cow<'s, str>> = Vec::new();
        self.list(|parser| {
            let (value, span) = parser.name("an enum value")?;
            if values.iter().any(|existing| existing == value) {
                return Err((span, SchemaError::Repeated { kind: "enum value", name: value.to_string() }));
            }
            values.push(Cow::Borrowed(value));
            Ok(())
        })?;
        if values.len() < 2 {
            return Err((span, SchemaError::TooFewValues));
        }
        self.schema.enums.push(Enum { name: Cow::Borrowed(name), values });
        self.schema.declarations.push(Declaration::Enum(id));
        Ok(())
    }

    fn structure(&mut self) -> Parsed<()> {
        let (name, span) = self.name("a struct name")?;
        let id = StructId(self.schema.structs.len() as u32);
        self.declare(Cow::Borrowed(name), span, Declared::Open)?;
        let fields = self.fields(false)?;
        self.close(name, Declared::Struct(id));
        if fields.is_empty() {
            return Err((span, SchemaError::EmptyStruct));
        }
        self.schema.structs.push(Struct { name: Cow::Borrowed(name), fields });
        self.schema.declarations.push(Declaration::Struct(id));
        Ok(())
    }

    fn union(&mut self) -> Parsed<()> {
        let (name, span) = self.name("a union name")?;
        let id = UnionId(self.schema.unions.len() as u32);
        self.declare(Cow::Borrowed(name), span, Declared::Open)?;
        let mut variants: Vec<Variant<'s>> = Vec::new();
        self.list(|parser| {
            let (variant, span) = parser.name("a variant name")?;
            if variants.iter().any(|existing| existing.name == variant) {
                return Err((span, SchemaError::Repeated { kind: "variant", name: variant.to_string() }));
            }
            let fields = if parser.at("{") { parser.fields(false)? } else { Vec::new() };
            if fields.iter().any(|field| field.name == UNION_TAG) {
                return Err((span, SchemaError::VariantTag));
            }
            variants.push(Variant { name: Cow::Borrowed(variant), fields });
            Ok(())
        })?;
        if variants.len() < 2 {
            return Err((span, SchemaError::TooFewVariants));
        }
        self.close(name, Declared::Union(id));
        self.schema.unions.push(Union { name: Cow::Borrowed(name), variants });
        self.schema.declarations.push(Declaration::Union(id));
        Ok(())
    }

    fn side(&mut self) -> Parsed<Side> {
        match self.next() {
            (Token::Name("from"), _) => {}
            token => return self.expected(token, "\"from\""),
        }
        match self.next() {
            (Token::Name("Server"), _) => Ok(Side::Server),
            (Token::Name("Client"), _) => Ok(Side::Client),
            token => self.expected(token, "\"Server\" or \"Client\""),
        }
    }

    fn event(&mut self) -> Parsed<()> {
        let (name, span) = self.name("an event name")?;
        self.declare(Cow::Borrowed(name), span, Declared::Other)?;
        let from = self.side()?;
        let reliable = self.peek() != Token::Name("unreliable");
        if !reliable {
            self.next();
        }
        let fields = self.fields(true)?;
        if !reliable && fields.iter().any(|field| self.schema.holds_instance(field.ty)) {
            return Err((span, SchemaError::UnreliableInstances(name.to_string())));
        }
        self.schema.events.push(Event { name: Cow::Borrowed(name), from, reliable, role: Role::Message, fields });
        Ok(())
    }

    fn function(&mut self) -> Parsed<()> {
        let (name, span) = self.name("a function name")?;
        self.declare(Cow::Borrowed(name), span, Declared::Other)?;
        self.declare(Cow::Owned(format!("{name}Reply")), span, Declared::Other)?;
        self.declare(Cow::Owned(format!("{name}Result")), span, Declared::Other)?;
        let from = self.side()?;
        let arguments = self.fields(true)?;
        if arguments.iter().any(|field| field.name == super::types::CALL_ID) {
            return Err((span, SchemaError::CallIdArgument));
        }
        self.symbol("->")?;
        let results = self.fields(true)?;
        self.schema.functions.push(Function { name: Cow::Borrowed(name), from, arguments, results });
        Ok(())
    }

    fn option(&mut self) -> Parsed<()> {
        let (name, span) = self.name("an option name")?;
        self.symbol("=")?;
        let value = match self.next() {
            (Token::Text(text), _) => Cow::Borrowed(text),
            token => return self.expected(token, "text in double quotes"),
        };
        let slot = match name {
            "output" => &mut self.schema.output,
            "runtime" => &mut self.schema.runtime,
            _ => return Err((span, SchemaError::UnknownOption(name.to_string()))),
        };
        if slot.is_some() {
            return Err((span, SchemaError::Repeated { kind: "option", name: name.to_string() }));
        }
        *slot = Some(value);
        Ok(())
    }

    fn schema(&mut self) -> Parsed<()> {
        loop {
            match self.next() {
                (Token::End, _) => return Ok(()),
                (Token::Name("enum"), _) => self.enumeration()?,
                (Token::Name("struct"), _) => self.structure()?,
                (Token::Name("union"), _) => self.union()?,
                (Token::Name("event"), _) => self.event()?,
                (Token::Name("function"), _) => self.function()?,
                (Token::Name("option"), _) => self.option()?,
                token => {
                    return self
                        .expected(token, "\"option\", \"enum\", \"struct\", \"union\", \"event\" or \"function\"");
                }
            }
        }
    }
}

fn sized_int(name: &str) -> Option<(bool, u64)> {
    let signed = match name.as_bytes().first() {
        Some(b'u') => false,
        Some(b'i') => true,
        _ => return None,
    };
    let digits = &name[1..];
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().map(|width| (signed, width))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(source: &str) -> SchemaError {
        match parse(source) {
            Err(Error::Schema(_, error)) => error,
            other => panic!("expected a schema error, got {other:?}"),
        }
    }

    #[test]
    fn functions_become_a_request_and_a_reply() {
        let schema = parse("function Buy from Client { Item: u16 } -> { Granted: bool }").expect("valid schema");
        let names: Vec<&str> = schema.events.iter().map(|event| event.name.as_ref()).collect();
        assert_eq!(names, ["Buy", "BuyReply"]);
        assert_eq!(schema.events[0].fields[0].name, "CallId");
        assert_eq!(schema.events[1].from, Side::Server);
        assert_eq!(schema.unions[0].name, "BuyResult");
    }

    #[test]
    fn options_name_the_output_and_runtime() {
        let schema =
            parse("option output = \"src/shared/Net\"\noption runtime = \"ReplicatedStorage.Packages.Courier\"")
                .expect("valid schema");
        assert_eq!(schema.output.as_deref(), Some("src/shared/Net"));
        assert_eq!(schema.runtime.as_deref(), Some("ReplicatedStorage.Packages.Courier"));
        assert!(matches!(error("option colour = \"red\""), SchemaError::UnknownOption(_)));
        assert!(matches!(error("option output = \"a\" option output = \"b\""), SchemaError::Repeated { .. }));
    }

    #[test]
    fn maps_and_full_precision_floats_parse() {
        let schema = parse("event A from Server { M: map(string(..50), u8, ..500), F: f32, D: f64, V: vector }")
            .expect("valid schema");
        let fields = &schema.events[0].fields;
        let Type::Map { bounds, .. } = schema.ty(fields[0].ty) else { panic!("a map") };
        assert_eq!(bounds.max, Some(500));
        assert_eq!(schema.ty(fields[1].ty), &Type::Real { wide: false });
        assert_eq!(schema.ty(fields[2].ty), &Type::Real { wide: true });
        assert_eq!(schema.ty(fields[3].ty), &Type::RealVector);
    }

    #[test]
    fn types_are_shared_in_the_arena() {
        let schema = parse("event A from Server { X: u8, Y: u8, Z: u8? }").expect("valid schema");
        let fields = &schema.events[0].fields;
        assert_eq!(fields[0].ty, fields[1].ty);
        assert_eq!(schema.ty(fields[2].ty), &Type::Optional(fields[0].ty));
    }

    #[test]
    fn mistakes_name_their_kind() {
        assert!(matches!(error("event A from Server { X: u33 }"), SchemaError::IntWidth(33)));
        assert!(matches!(error("event A from Server { X: Thing }"), SchemaError::UnknownType(_)));
        assert!(matches!(error("event A from Server { X: u8?[] }"), SchemaError::OptionalElements));
        assert!(matches!(error("struct S { A: u8 } event A from Server { X: map(S, u8) }"), SchemaError::MapKey));
        assert!(matches!(error("event A from Server { X: map(u8, u8?) }"), SchemaError::OptionalMapValues));
        assert!(matches!(error("event A from Server { Cursor: u8 }"), SchemaError::ReservedField(_)));
        assert!(matches!(error("enum E { A }"), SchemaError::TooFewValues));
        assert!(matches!(error("struct S { A: S }"), SchemaError::HoldsItself(_)));
        assert!(matches!(error("union U { A { B: U[] }, C }"), SchemaError::HoldsItself(_)));
        assert!(matches!(error("event A from Server {} event A from Client {}"), SchemaError::AlreadyDeclared { .. }));
        assert!(matches!(error("event A from Elsewhere {}"), SchemaError::Expected { .. }));
        assert!(matches!(error("event A from Server unreliable { P: Instance }"), SchemaError::UnreliableInstances(_)));
        assert!(matches!(error("event A from Server unreliable { P: Handle[] }"), SchemaError::UnreliableInstances(_)));
    }
}
