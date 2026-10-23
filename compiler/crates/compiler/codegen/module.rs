//! Puts a whole module together, runtime pieces included.
//!
//! A stream is one buffer: a 20 byte header, message bits from the front and string bytes from
//! the back.

use super::api::{api, bindings};
use super::declarations::{alias, declarations, schema_types};
use super::emitter::Emitter;
use super::reader::{Reader, map_power};
use super::writer::{writer, writer_name};
use crate::Error;
use crate::layout::{Chunk, Codec, Count, Layout, LeafKind, Tail};
use crate::luau::builtins::REQUIRE;
use crate::luau::{Ast, Entry, ExprId, Name, Stmt, TypeBody, local, print};
use crate::schema::{Role, Schema, Side};

const RUNTIME: &str = include_str!("templates/runtime.luau");
const FULL: &str = include_str!("templates/full.luau");
const OVERFLOW: &str = include_str!("templates/overflow.luau");
const BROADCAST: &str = include_str!("templates/broadcast.luau");
const APPEND: &str = include_str!("templates/append.luau");
const CODED: &str = include_str!("templates/coded.luau");
const INSTANCES: &str = include_str!("templates/instances.luau");
const ROTATION: &str = include_str!("templates/rotation.luau");
const FLOATS: &str = include_str!("templates/floats.luau");
const MAPS: &str = include_str!("templates/maps.luau");

const MAX_LOCALS: usize = 200;

const MAX_CHECKED_LINES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Codec,
    Api,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runtime {
    Bundled,
    Expression(String),
}

#[derive(Debug, Clone)]
pub struct Options {
    pub mode: Mode,
    pub runtime: Runtime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub path: &'static str,
    pub text: String,
}

pub fn generate(schema: &Schema, layout: &Layout, options: &Options) -> Result<Vec<File>, Error> {
    let implementation = implementation(schema, layout, options)?;
    if options.mode == Mode::Codec {
        return Ok(vec![File { path: "", text: implementation }]);
    }
    Ok(vec![
        File { path: "init.luau", text: front(schema, layout, options) },
        File { path: "Implementation.luau", text: implementation },
    ])
}

fn uses(layout: &Layout, found: impl Fn(&LeafKind) -> bool + Copy) -> bool {
    layout.events().any(|(_, event)| layout.any_leaf(event.node, &|leaf| found(&leaf.kind)))
}

fn uses_tail(layout: &Layout, found: impl Fn(&Tail) -> bool + Copy) -> bool {
    layout.events().any(|(_, event)| layout.any_tail(event.node, &found))
}

pub fn broadcasts(layout: &Layout, schema: &Schema) -> bool {
    layout.events().any(|(direction, event)| {
        direction.from == Side::Server
            && schema.events[event.event].role == Role::Message
            && !layout.tails(event.node).is_empty()
    })
}

fn runtime<'a>(layout: &Layout, schema: &Schema, mode: Mode) -> Vec<Stmt<'a>> {
    let max_words = layout.events().map(|(_, event)| layout.node(event.node).words()).max().unwrap_or(0);
    let max_id_bits = layout.directions.iter().map(|direction| direction.ids.bits).max().unwrap_or(1);
    let broadcast = mode == Mode::Api && broadcasts(layout, schema);
    let append = mode == Mode::Codec || broadcast;
    let mut writers = String::new();
    if append || layout.events().any(|(_, event)| layout.tails(event.node).is_empty()) {
        writers.push_str(FULL);
        writers.push('\n');
    }
    if broadcast {
        writers.push_str(BROADCAST);
        writers.push('\n');
    }
    if layout.events().any(|(_, event)| !layout.tails(event.node).is_empty()) {
        let check = "\tif Stream == BROADCAST then\n\t\terror(`Courier: a message cannot fit a stream of {buffer.len(Stream) - HEADER - 4} bytes even when empty, so it is dropped`, 4)\n\tend\n";
        writers.push_str(&OVERFLOW.replace("@BROADCAST@\n", if broadcast { check } else { "" }));
        writers.push('\n');
    }
    if append {
        writers.push_str(APPEND);
        writers.push('\n');
    }
    if mode == Mode::Codec {
        writers.push_str(
            "local function Append(Stream: Stream, From: Stream)\n\tAppendTo(Stream, From, buffer.len(From))\nend\n\n",
        );
    }
    let text = RUNTIME
        .replace("@WRITERS@", &writers)
        .replace("@GAP@", &(32 + 7 + max_id_bits).to_string())
        .replace("@READ_PAD@", &(max_words.max(2) * 4 + 8).to_string());
    let mut out = vec![Stmt::Verbatim(text.into())];
    if uses_tail(layout, |tail| matches!(tail, Tail::Instance { .. })) {
        out.push(Stmt::Verbatim(INSTANCES.into()));
    }
    let coded = |tail: &Tail| {
        matches!(
            tail,
            Tail::Array { count: Count::Coded { .. }, .. }
                | Tail::Map { count: Count::Coded { .. }, .. }
                | Tail::Bytes { count: Count::Coded { .. }, .. }
        )
    };
    if uses_tail(layout, coded) {
        out.push(Stmt::Blank);
        out.push(Stmt::Verbatim(CODED.into()));
    }
    if uses(layout, |kind| matches!(kind, LeafKind::Rotation { .. })) {
        out.push(Stmt::Blank);
        out.push(Stmt::Verbatim(ROTATION.into()));
    }
    if uses(layout, |kind| matches!(kind, LeafKind::Value(Codec::F32 | Codec::F64 { .. }))) {
        out.push(Stmt::Blank);
        out.push(Stmt::Verbatim(FLOATS.into()));
    }
    let power = layout.tails.iter().filter_map(|tail| match tail {
        Tail::Map { count, .. } => Some(map_power(count)),
        _ => None,
    });
    if let Some(power) = power.max().filter(|power| *power > 0) {
        out.push(Stmt::Blank);
        out.push(Stmt::Verbatim(MAPS.replace("@MAP_POWER@", &power.to_string()).into()));
    }
    out.push(Stmt::Blank);
    out
}

fn entry<'a>(b: &Ast<'a>, key: &'static str) -> Entry<'a> {
    Entry { key: Some(key.into()), value: b.fixed(key) }
}

fn codec_exports<'a>(e: &Emitter<'_, 'a>, spill: bool) -> Stmt<'a> {
    let b = e.b;
    let writers = if spill {
        b.fixed("WRITERS")
    } else {
        let entries: Vec<Entry> = e
            .layout
            .events()
            .map(|(_, event)| Entry {
                key: Some(e.schema.events[event.event].name.as_ref().into()),
                value: b.name(writer_name(e, event)),
            })
            .collect();
        b.table(entries, true)
    };
    let entries = vec![
        entry(b, "NewStream"),
        entry(b, "SetOnFull"),
        entry(b, "IsEmpty"),
        entry(b, "Finish"),
        entry(b, "Append"),
        Entry { key: Some("Write".into()), value: writers },
        entry(b, "ReadOnServer"),
        entry(b, "ReadOnClient"),
        Entry { key: Some("ServerHandlers".into()), value: b.fixed("SERVER_HANDLERS") },
        Entry { key: Some("ClientHandlers".into()), value: b.fixed("CLIENT_HANDLERS") },
    ];
    Stmt::Return(vec![b.table(entries, true)])
}

fn bind<'a>(b: &Ast<'a>) -> Stmt<'a> {
    let codec =
        ["NewStream", "SetOnFull", "IsEmpty", "Finish", "ReadOnServer", "ReadOnClient"].map(|key| entry(b, key));
    Stmt::Call(b.invoke("Courier.Bind", [b.table(codec, true)]))
}

fn into_field<'a>(b: &Ast<'a>, function: Stmt<'a>, target: ExprId) -> Stmt<'a> {
    let Stmt::Function { parameters, returns, body, .. } = function else {
        panic!("only functions become fields");
    };
    Stmt::Assign(vec![target], vec![b.function(parameters, returns, body)])
}

fn top_level_locals(statements: &[Stmt]) -> usize {
    statements
        .iter()
        .map(|statement| match statement {
            Stmt::Local(..) | Stmt::Function { .. } => 1,
            Stmt::Locals(names, ..) => names.len(),
            Stmt::Declare(names) => names.len(),
            Stmt::Verbatim(text) => text.lines().filter(|line| line.starts_with("local ")).count(),
            _ => 0,
        })
        .sum()
}

fn require_runtime<'a>(b: &Ast<'a>, options: &'a Options, front: bool) -> Stmt<'a> {
    let path = match &options.runtime {
        Runtime::Bundled if front => b.string("@self/Courier"),
        Runtime::Bundled => b.string("./Courier"),
        Runtime::Expression(expression) => b.fixed(expression),
    };
    local(Name::Fixed("Courier"), b.invoke(REQUIRE, [path]))
}

fn chunk_tables<'a>(e: &Emitter<'_, 'a>) -> Vec<Stmt<'a>> {
    let b = e.b;
    let mut chunks: Vec<Chunk> = e
        .layout
        .tails
        .iter()
        .filter_map(|tail| match tail {
            Tail::Array { chunk, .. } => *chunk,
            _ => None,
        })
        .collect();
    chunks.sort_by_key(|chunk| chunk.base);
    chunks.dedup();
    let mut out: Vec<Stmt<'a>> = chunks
        .into_iter()
        .map(|chunk| {
            let entries: Vec<Entry> =
                (0..chunk.size).map(|count| Entry { key: None, value: b.number(chunk.partial_bits(count)) }).collect();
            let table = b.table(entries, false);
            Stmt::Local(b.symbol(super::writer::chunk_table(chunk)), Some("{ number }".into()), Some(table))
        })
        .collect();
    if !out.is_empty() {
        out.push(Stmt::Blank);
    }
    out
}

fn runtime_types<'a>() -> [Stmt<'a>; 2] {
    [
        alias("Future", "<T...>", TypeBody::Inline("Courier.Future<T...>".into())),
        alias("Middleware", "<T, A = { any }>", TypeBody::Inline("Courier.Middleware<T, A>".into())),
    ]
}

fn front(schema: &Schema, layout: &Layout, options: &Options) -> String {
    let b = Ast::default();
    let e = Emitter::new(schema, layout, &b);
    let mut out = vec![Stmt::Verbatim("--!strict\n".into()), Stmt::Blank, require_runtime(&b, options, true)];
    let implementation = b.invoke(REQUIRE, [b.string("@self/Implementation")]);
    out.push(local(Name::Fixed("Implementation"), implementation));
    out.push(Stmt::Blank);
    out.extend(runtime_types());
    out.extend(schema_types(schema));
    out.push(api(&e).0);
    out.push(Stmt::Blank);
    out.push(Stmt::Return(vec![b.cast(b.cast(b.fixed("Implementation"), "any"), "Api")]));
    print(&b.finish(), &out)
}

fn implementation(schema: &Schema, layout: &Layout, options: &Options) -> Result<String, Error> {
    let b = Ast::default();
    let e = Emitter::new(schema, layout, &b);
    let mut out = assemble(&e, options, false);
    if top_level_locals(&out) >= MAX_LOCALS {
        out = assemble(&e, options, true);
    }
    let needed = top_level_locals(&out);
    if needed >= MAX_LOCALS {
        return Err(Error::TooManyLocals { needed });
    }
    let text = print(&b.finish(), &out);
    if text.lines().count() > MAX_CHECKED_LINES {
        return Ok(text.replacen("--!strict", "--!nocheck", 1));
    }
    Ok(text)
}

fn assemble<'a>(e: &Emitter<'_, 'a>, options: &'a Options, spill: bool) -> Vec<Stmt<'a>> {
    let (mode, b) = (options.mode, e.b);
    let mut out = vec![Stmt::Verbatim("--!strict\n--!native\n--!optimize 2\n".into()), Stmt::Blank];
    if mode == Mode::Api {
        out.push(require_runtime(b, options, false));
        out.push(Stmt::Blank);
        out.extend(runtime_types());
    }
    out.extend(declarations(e, mode));
    out.extend(runtime(e.layout, e.schema, mode));
    out.extend(chunk_tables(e));
    if spill {
        out.push(Stmt::Local(Name::Fixed("READERS"), Some("{ [string]: any }".into()), Some(b.table([], false))));
        if mode == Mode::Codec {
            out.push(Stmt::Local(Name::Fixed("WRITERS"), Some("{ [string]: any }".into()), Some(b.table([], false))));
        }
        out.push(Stmt::Blank);
    }
    let field = |function, target| into_field(b, function, target);
    if mode == Mode::Api {
        out.extend(bindings(e));
    } else {
        for (_, event) in e.layout.events() {
            let written = writer(e, event, spill);
            let name: &'a str = &e.schema.events[event.event].name;
            out.push(if spill { field(written, b.field(b.fixed("WRITERS"), name)) } else { written });
            out.push(Stmt::Blank);
        }
    }
    for direction in &e.layout.directions {
        out.extend(Reader::new(e, direction.from, mode, spill).readers(direction, field));
    }
    match mode {
        Mode::Codec => out.push(codec_exports(e, spill)),
        Mode::Api => {
            let (alias, table) = api(e);
            out.extend([alias, Stmt::Blank]);
            out.extend(table);
            out.extend([Stmt::Blank, bind(b), Stmt::Blank]);
            out.push(Stmt::Return(vec![b.fixed("API")]));
        }
    }
    out
}
