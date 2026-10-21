//! Exported types, enum and union tables, and handler tables.

use super::emitter::Emitter;
use super::module::Mode;
use super::reader::handlers_table;
use super::types::{enum_table, handler_type, luau_type, screaming};
use super::writer::MAX_CHAIN;
use crate::luau::{Entry, Stmt, TypeBody};
use crate::schema::{Enum, Schema, Side, Struct, UNION_TAG, Union};

pub fn alias<'a>(name: &'a str, generics: &'static str, body: TypeBody<'a>) -> Stmt<'a> {
    Stmt::TypeAlias { name: name.into(), generics, body }
}

fn enum_alias<'a>(declared: &'a Enum) -> Stmt<'a> {
    let union: Vec<String> = declared.values.iter().map(|value| format!("\"{value}\"")).collect();
    alias(&declared.name, "", TypeBody::Inline(union.join(" | ").into()))
}

fn struct_alias<'a>(schema: &Schema, declared: &'a Struct) -> Stmt<'a> {
    let fields =
        declared.fields.iter().map(|field| (field.name.as_ref().into(), luau_type(schema, field.ty, false).into()));
    alias(&declared.name, "", TypeBody::Fields(fields.collect()))
}

fn union_alias<'a>(schema: &Schema, declared: &'a Union) -> Stmt<'a> {
    if declared.variants.iter().all(|variant| variant.fields.is_empty()) {
        let names: Vec<String> = declared.variants.iter().map(|variant| format!("\"{}\"", variant.name)).collect();
        return alias(&declared.name, "", TypeBody::Inline(format!("{{ {UNION_TAG}: {} }}", names.join(" | ")).into()));
    }
    let variants: Vec<String> = declared
        .variants
        .iter()
        .map(|variant| {
            let mut fields = vec![format!("{UNION_TAG}: \"{}\"", variant.name)];
            let typed =
                variant.fields.iter().map(|field| format!("{}: {}", field.name, luau_type(schema, field.ty, false)));
            fields.extend(typed);
            format!("{{ {} }}", fields.join(", "))
        })
        .collect();
    alias(&declared.name, "", TypeBody::Inline(variants.join(" | ").into()))
}

pub fn schema_types<'a>(schema: &'a Schema<'a>) -> Vec<Stmt<'a>> {
    let mut out: Vec<Stmt> = schema.enums.iter().map(enum_alias).collect();
    out.extend(schema.structs.iter().map(|declared| struct_alias(schema, declared)));
    out.extend(schema.unions.iter().map(|declared| union_alias(schema, declared)));
    out.push(Stmt::Blank);
    out
}

pub fn declarations<'a>(e: &Emitter<'_, 'a>, mode: Mode) -> Vec<Stmt<'a>> {
    let (schema, b) = (e.schema, e.b);
    let mut out = vec![alias("Stream", "", TypeBody::Inline("buffer".into())), Stmt::Blank];

    for declared in &schema.enums {
        out.push(enum_alias(declared));
        let mut entries: Vec<Entry> =
            declared.values.iter().map(|value| Entry { key: None, value: b.string(value.as_ref()) }).collect();
        entries.extend(
            declared
                .values
                .iter()
                .enumerate()
                .map(|(position, value)| Entry { key: Some(value.as_ref().into()), value: b.number(position as f64) }),
        );
        let name = b.symbol(enum_table(declared));
        out.push(Stmt::Local(name, Some("{ [any]: any }".into()), Some(b.table(entries, true))));
        out.push(Stmt::Blank);
    }

    for declared in &schema.structs {
        out.push(struct_alias(schema, declared));
        out.push(Stmt::Blank);
    }

    for declared in &schema.unions {
        out.push(union_alias(schema, declared));
        if declared.variants.len() <= MAX_CHAIN {
            out.push(Stmt::Blank);
            continue;
        }
        let indices = declared.variants.iter().enumerate().map(|(position, variant)| Entry {
            key: Some(variant.name.as_ref().into()),
            value: b.number(position as f64),
        });
        let name = b.symbol(format!("{}_INDEX", screaming(&declared.name)));
        out.push(Stmt::Local(name, Some("{ [string]: number }".into()), Some(b.table(indices, true))));
        out.push(Stmt::Blank);
    }

    for direction in &e.layout.directions {
        let type_name = match direction.from {
            Side::Client => "ServerHandlers",
            Side::Server => "ClientHandlers",
        };
        let handlers = direction
            .events
            .iter()
            .filter_map(|event| {
                let declared = &schema.events[event.event];
                let handler = handler_type(schema, declared, mode)?;
                Some((declared.name.as_ref().into(), format!("({handler})?").into()))
            })
            .collect();
        out.push(alias(type_name, "", TypeBody::Fields(handlers)));
        if mode == Mode::Api && direction.events.is_empty() {
            out.push(Stmt::Blank);
            continue;
        }
        let table = b.table([], false);
        out.push(Stmt::Local(
            crate::luau::Name::Fixed(handlers_table(direction.from)),
            Some(type_name.into()),
            Some(table),
        ));
        out.push(Stmt::Blank);
    }
    out
}
