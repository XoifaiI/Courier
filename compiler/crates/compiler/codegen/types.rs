//! Luau types for schema types and handlers.

use super::module::Mode;
use crate::luau::{Name, Parameter};
use crate::schema::{Enum, Event, Field, Role, Schema, Side, Type, TypeId};

pub fn luau_type(schema: &Schema, ty: TypeId, sending: bool) -> String {
    match schema.ty(ty) {
        Type::Bool => "boolean".to_string(),
        Type::Int { .. } | Type::Float { .. } | Type::Real { .. } => "number".to_string(),
        Type::Vector { .. } | Type::RealVector => "Vector3".to_string(),
        Type::Rotation { .. } | Type::CFrame { .. } => "CFrame".to_string(),
        Type::Enum(id) => schema.enumeration(*id).name.to_string(),
        Type::Struct(id) => schema.structure(*id).name.to_string(),
        Type::Union(id) => schema.union(*id).name.to_string(),
        Type::Optional(inner) => {
            let inner = luau_type(schema, *inner, sending);
            if inner.ends_with('?') { inner } else { format!("{inner}?") }
        }
        Type::Array { element, .. } => match schema.ty(*element) {
            Type::Instance { class } => format!("{{ {} }}", class.as_deref().unwrap_or("Instance")),
            _ => format!("{{ {} }}", luau_type(schema, *element, sending)),
        },
        Type::Map { key, value, .. } => {
            let value = match schema.ty(*value) {
                Type::Instance { class } => class.as_deref().unwrap_or("Instance").to_string(),
                _ => luau_type(schema, *value, sending),
            };
            format!("{{ [{}]: {value} }}", luau_type(schema, *key, sending))
        }
        Type::Text(_) => "string".to_string(),
        Type::Buffer(_) => "buffer".to_string(),
        Type::Instance { class } => {
            let class = class.as_deref().unwrap_or("Instance");
            if sending { class.to_string() } else { format!("{class}?") }
        }
        Type::Handle => "InstanceHandle".to_string(),
    }
}

#[derive(Debug, Clone)]
pub struct Typed<'a> {
    pub name: &'a str,
    pub ty: String,
}

impl<'a> Typed<'a> {
    pub fn new(name: &'a str, ty: impl Into<String>) -> Typed<'a> {
        Typed { name, ty: ty.into() }
    }

    pub fn parameter(&self) -> Parameter<'a> {
        Parameter::new(Name::Fixed(self.name), self.ty.clone())
    }
}

pub fn fields_typed<'a>(schema: &Schema, fields: &'a [Field], sending: bool) -> Vec<Typed<'a>> {
    fields.iter().map(|field| Typed::new(&field.name, luau_type(schema, field.ty, sending))).collect()
}

pub fn parameters<'a>(typed: &[Typed<'a>]) -> Vec<Parameter<'a>> {
    typed.iter().map(Typed::parameter).collect()
}

pub fn function_type(arguments: &[Typed], returns: &str) -> String {
    let arguments: Vec<String> = arguments.iter().map(|typed| format!("{}: {}", typed.name, typed.ty)).collect();
    format!("({}) -> {returns}", arguments.join(", "))
}

pub fn returns(schema: &Schema, fields: &[Field], sending: bool) -> String {
    let types: Vec<String> = fields.iter().map(|field| luau_type(schema, field.ty, sending)).collect();
    format!("({})", types.join(", "))
}

pub fn handler_type(schema: &Schema, event: &Event, mode: Mode) -> Option<String> {
    let mut arguments = Vec::new();
    if event.from == Side::Client {
        arguments.push(Typed::new("Sender", if mode == Mode::Api { "Player" } else { "any" }));
    }
    match (mode, event.role) {
        (Mode::Codec, _) | (Mode::Api, Role::Message) => {
            arguments.extend(fields_typed(schema, &event.fields, false));
            Some(function_type(&arguments, "()"))
        }
        (Mode::Api, Role::Request(function)) => {
            arguments.extend(fields_typed(schema, &event.fields[1..], false));
            Some(function_type(&arguments, &returns(schema, &schema.function(function).results, true)))
        }
        (Mode::Api, Role::Reply(_)) => None,
    }
}

pub fn arguments<'e, 's>(event: &'e Event<'s>) -> &'e [Field<'s>] {
    match event.role {
        Role::Request(_) => &event.fields[1..],
        Role::Message | Role::Reply(_) => &event.fields[..],
    }
}

pub fn call_type(schema: &Schema, event: &Event) -> String {
    let fields: Vec<String> = arguments(event)
        .iter()
        .map(|field| format!("{}: {}", field.name, luau_type(schema, field.ty, false)))
        .collect();
    let args = if fields.is_empty() { "{}".to_string() } else { format!("{{ {} }}", fields.join(", ")) };
    let player = if event.from == Side::Client { "Player" } else { "nil" };
    format!(
        "{{ read Name: \"{}\", read Player: {player}, Args: {args}, read Shared: {{ [unknown]: unknown }}, read Reject: (self: unknown, Reason: string) -> () }}",
        event.name
    )
}

pub fn screaming(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    for (position, character) in text.chars().enumerate() {
        if character.is_ascii_uppercase() && position > 0 {
            out.push('_');
        }
        out.push(character.to_ascii_uppercase());
    }
    out
}

pub fn enum_table(declared: &Enum) -> String {
    format!("{}_ENUM", screaming(&declared.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screaming_splits_words() {
        assert_eq!(screaming("WeaponKind"), "WEAPON_KIND");
        assert_eq!(screaming("Hit"), "HIT");
    }

    #[test]
    fn calls_name_their_event_sender_and_arguments() {
        let schema = crate::schema::parse(
            "event Move from Client { X: u8, Part: Instance(BasePart) }
event Ping from Server {}
function Buy from Client { Item: u16 } -> { Ok: bool }",
        )
        .expect("the schema parses");
        let call = call_type(&schema, &schema.events[0]);
        assert!(call.starts_with("{ read Name: \"Move\", read Player: Player, Args: { X: number, Part: BasePart? }"));
        assert!(call_type(&schema, &schema.events[1]).contains("read Player: nil, Args: {},"));
        assert!(call_type(&schema, &schema.events[2]).contains("Args: { Item: number }"));
    }

    #[test]
    fn function_types_name_their_parameters() {
        let arguments = [Typed::new("Sender", "Player"), Typed::new("Hit", "number")];
        assert_eq!(function_type(&arguments, "()"), "(Sender: Player, Hit: number) -> ()");
    }
}
