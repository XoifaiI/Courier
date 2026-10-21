//! Prints a schema back as text that parses to the same thing.

use std::fmt::{self, Write as _};

use super::types::{Bounds, Declaration, Field, Role, Schema, Side, Type, TypeId};

impl Schema<'_> {
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.write(&mut out).expect("writing to a String does not fail");
        out
    }

    fn write(&self, out: &mut String) -> fmt::Result {
        for (name, value) in [("output", &self.output), ("runtime", &self.runtime)] {
            if let Some(value) = value {
                writeln!(out, "option {name} = \"{value}\"")?;
            }
        }
        for declaration in &self.declarations {
            match *declaration {
                Declaration::Enum(id) => {
                    let declared = self.enumeration(id);
                    writeln!(out, "enum {} {{ {} }}", declared.name, declared.values.join(", "))?;
                }
                Declaration::Struct(id) => {
                    let declared = self.structure(id);
                    write!(out, "struct {} ", declared.name)?;
                    self.write_fields(out, &declared.fields)?;
                    out.push('\n');
                }
                Declaration::Union(id) => {
                    let declared = self.union(id);
                    write!(out, "union {} {{ ", declared.name)?;
                    for (position, variant) in declared.variants.iter().enumerate() {
                        if position > 0 {
                            out.push_str(", ");
                        }
                        out.push_str(&variant.name);
                        if !variant.fields.is_empty() {
                            out.push(' ');
                            self.write_fields(out, &variant.fields)?;
                        }
                    }
                    out.push_str(" }\n");
                }
            }
        }
        for event in self.events.iter().filter(|event| event.role == Role::Message) {
            let unreliable = if event.reliable { "" } else { " unreliable" };
            write!(out, "event {} from {}{unreliable} ", event.name, side(event.from))?;
            self.write_fields(out, &event.fields)?;
            out.push('\n');
        }
        for function in &self.functions {
            write!(out, "function {} from {} ", function.name, side(function.from))?;
            self.write_fields(out, &function.arguments)?;
            out.push_str(" -> ");
            self.write_fields(out, &function.results)?;
            out.push('\n');
        }
        Ok(())
    }

    fn write_fields(&self, out: &mut String, fields: &[Field]) -> fmt::Result {
        out.push_str("{ ");
        for (position, field) in fields.iter().enumerate() {
            if position > 0 {
                out.push_str(", ");
            }
            write!(out, "{}: ", field.name)?;
            self.write_type(out, field.ty)?;
        }
        out.push_str(" }");
        Ok(())
    }

    pub fn write_type(&self, out: &mut String, ty: TypeId) -> fmt::Result {
        match self.ty(ty) {
            Type::Bool => out.push_str("bool"),
            Type::Int { min, max, wraps: true } => {
                let width = 64 - ((max - min) as u64).leading_zeros();
                write!(out, "{}{width}", if *min < 0 { 'i' } else { 'u' })?;
            }
            Type::Int { min, max, wraps: false } => write!(out, "int({min}..{max})")?,
            Type::Float { min, max, step } => write!(out, "float({min}..{max}, {step})")?,
            Type::Vector { min, max, step } => write!(out, "vector({min}..{max}, {step})")?,
            Type::Real { wide } => out.push_str(if *wide { "f64" } else { "f32" }),
            Type::RealVector => out.push_str("vector"),
            Type::Map { key, value, bounds } => {
                out.push_str("map(");
                self.write_type(out, *key)?;
                out.push_str(", ");
                self.write_type(out, *value)?;
                if bounds.max.is_some() {
                    write!(out, ", {}", BoundsText(*bounds))?;
                }
                out.push(')');
            }
            Type::Rotation { degrees } => write!(out, "rotation({degrees})")?,
            Type::CFrame { min, max, step, degrees } => write!(out, "cframe({min}..{max}, {step}, {degrees})")?,
            Type::Enum(id) => out.push_str(&self.enumeration(*id).name),
            Type::Struct(id) => out.push_str(&self.structure(*id).name),
            Type::Union(id) => out.push_str(&self.union(*id).name),
            Type::Optional(inner) => {
                self.write_type(out, *inner)?;
                out.push('?');
            }
            Type::Array { element, bounds } => {
                self.write_type(out, *element)?;
                write!(out, "[{}]", BoundsText(*bounds))?;
            }
            Type::Text(bounds) | Type::Buffer(bounds) => {
                out.push_str(if matches!(self.ty(ty), Type::Text(_)) { "string" } else { "buffer" });
                if bounds.max.is_some() {
                    write!(out, "({})", BoundsText(*bounds))?;
                }
            }
            Type::Instance { class: Some(class) } => write!(out, "Instance({class})")?,
            Type::Instance { class: None } => out.push_str("Instance"),
            Type::Handle => out.push_str("Handle"),
        }
        Ok(())
    }
}

fn side(side: Side) -> &'static str {
    match side {
        Side::Server => "Server",
        Side::Client => "Client",
    }
}

struct BoundsText(Bounds);

impl fmt::Display for BoundsText {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Bounds { max: None, .. } => Ok(()),
            Bounds { min, max: Some(max) } if min == max => write!(formatter, "{max}"),
            Bounds { min: 0, max: Some(max) } => write!(formatter, "..{max}"),
            Bounds { min, max: Some(max) } => write!(formatter, "{min}..{max}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::parse;

    #[test]
    fn printed_schemas_parse_back_the_same() {
        let source = "enum Kind { Common, Rare }
struct Vec { X: float(-100..100, 0.01), Z: i16 }
union Shape { Circle { Radius: float(0..100, 0.5) }, Nothing }
event Inventory from Server { Owner: u32, Pos: Vec, Items: Vec[..4], Label: string(2..24), Data: buffer?, Tags: Kind[3] }
event Draw from Client unreliable { Main: Shape, Mood: Kind?, Spin: rotation(0.5), At: cframe(-10..10, 0.5, 2) }
function Buy from Client { Item: int(-5..900), Part: Instance(BasePart) } -> { Granted: bool }
";
        let schema = parse(source).expect("valid schema");
        let printed = schema.text();
        assert_eq!(printed, source);
        assert_eq!(parse(&printed).expect("printed schema parses"), schema);
    }
}
