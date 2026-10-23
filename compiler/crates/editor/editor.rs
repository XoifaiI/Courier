//! What the editor shows for a .courier file: errors, sizes on hover, definitions, completion and
//! the outline. It all comes from the compiler, so it always matches what courier builds. A file is
//! read with the other schemas of its folder, in file name order, as courier compiles them.

use courier_compiler::Span;
use courier_compiler::layout::{EventLayout, Layout, Measure, layout, measure, varies};
use courier_compiler::schema::{Field, Role, Schema, Token, Type, TypeId, parse_unit, tokenize, tokenize_file};
use lsp_types::{
    CompletionItem, CompletionItemKind, Diagnostic, DiagnosticSeverity, DocumentSymbol, GotoDefinitionResponse, Hover,
    HoverContents, InsertTextFormat, Location, MarkupContent, MarkupKind, Position, Range, SymbolKind, Uri,
};

const BASE_TYPES: [(&str, &str); 20] = [
    ("bool", "bool"),
    ("u8", "u8"),
    ("u16", "u16"),
    ("u32", "u32"),
    ("i8", "i8"),
    ("i16", "i16"),
    ("i32", "i32"),
    ("int", "int(${1:0}..${2:100})"),
    ("float", "float(${1:0}..${2:100}, ${3:0.01})"),
    ("f32", "f32"),
    ("f64", "f64"),
    ("vector", "vector"),
    ("vector(min..max, step)", "vector(${1:-1024}..${2:1024}, ${3:0.01})"),
    ("rotation", "rotation(${1:0.5})"),
    ("cframe", "cframe(${1:-1024}..${2:1024}, ${3:0.01}, ${4:0.5})"),
    ("string", "string(..${1:100})"),
    ("buffer", "buffer(..${1:100})"),
    ("Instance", "Instance"),
    ("Handle", "Handle"),
    ("map", "map(${1:string}, ${2:u8}, ..${3:100})"),
];

const DECLARING: [&str; 5] = ["enum", "struct", "union", "event", "function"];

fn position(text: &str, span: Span) -> Position {
    let line = text.lines().nth(span.line as usize - 1).unwrap_or("");
    let bytes = (span.column as usize - 1).min(line.len());
    let character = line.get(..bytes).map_or(bytes, |before| before.encode_utf16().count());
    Position { line: span.line - 1, character: character as u32 }
}

fn span_at(text: &str, position: Position) -> Span {
    let line = text.lines().nth(position.line as usize).unwrap_or("");
    let mut units = 0;
    let mut bytes = line.len();
    for (index, character) in line.char_indices() {
        if units >= position.character as usize {
            bytes = index;
            break;
        }
        units += character.len_utf16();
    }
    Span { file: 0, line: position.line + 1, column: bytes as u32 + 1 }
}

fn width(token: &Token) -> u32 {
    match token {
        Token::Name(name) => name.len() as u32,
        Token::Text(text) => text.len() as u32 + 2,
        Token::Symbol(symbol) => symbol.len() as u32,
        Token::Number(number) => number.to_string().len() as u32,
        Token::End => 1,
    }
}

fn range(text: &str, span: Span, length: u32) -> Range {
    let start = position(text, span);
    let end = position(text, Span { column: span.column + length, ..span });
    Range { start, end }
}

fn token_range(text: &str, tokens: &[(Token, Span)], span: Span) -> Range {
    let length = tokens.iter().find(|(_, at)| *at == span).map_or(1, |(token, _)| width(token));
    range(text, span, length)
}

type Tokens<'s> = Vec<(Token<'s>, Span)>;

fn unit_tokens<'s>(files: &[(&str, &'s str)]) -> Vec<Tokens<'s>> {
    files.iter().enumerate().map(|(index, (_, text))| tokenize_file(text, index as u32).unwrap_or_default()).collect()
}

pub fn diagnose(text: &str) -> Vec<Diagnostic> {
    diagnose_unit(&[("", text)]).swap_remove(0)
}

pub fn diagnose_unit(files: &[(&str, &str)]) -> Vec<Vec<Diagnostic>> {
    let mut found = vec![Vec::new(); files.len().max(1)];
    let tokens = unit_tokens(files);
    let problem = match parse_unit(files) {
        Err(courier_compiler::Error::Schema(span, problem)) => {
            let file = (span.file as usize).min(files.len().saturating_sub(1));
            let range = files.get(file).map_or_else(Range::default, |(_, text)| token_range(text, &tokens[file], span));
            Some((file, range, problem.to_string()))
        }
        Err(other) => Some((0, Range::default(), other.to_string())),
        Ok(schema) => match layout(&schema) {
            Ok(_) => None,
            Err(courier_compiler::Error::Layout { path, problem }) => {
                let (file, range) = path_range(files, &tokens, &path);
                Some((file, range, format!("{path}: {problem}")))
            }
            Err(other) => Some((0, Range::default(), other.to_string())),
        },
    };
    if let Some((file, range, message)) = problem {
        found[file].push(Diagnostic {
            range,
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("courier".into()),
            message,
            ..Diagnostic::default()
        });
    }
    found
}

fn path_range(files: &[(&str, &str)], tokens: &[Tokens], path: &str) -> (usize, Range) {
    let mut steps = path.split('.');
    let Some(first) = steps.next() else { return (0, Range::default()) };
    let declared = tokens.iter().enumerate().find_map(|(file, tokens)| {
        let at = tokens.windows(2).position(|pair| {
            matches!(pair[0].0, Token::Name(word) if DECLARING.contains(&word)) && pair[1].0 == Token::Name(first)
        })?;
        Some((file, at + 1))
    });
    let Some((file, mut found)) = declared else { return (0, Range::default()) };
    let tokens = &tokens[file];
    for step in steps.filter(|step| *step != "[]") {
        if let Some(next) = (found + 1..tokens.len().saturating_sub(1))
            .find(|&index| tokens[index].0 == Token::Name(step) && tokens[index + 1].0 == Token::Symbol(":"))
        {
            found = next;
        }
    }
    let (token, span) = &tokens[found];
    (file, range(files[file].1, *span, width(token)))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Context<'s> {
    Declaration { keyword: &'s str, name: &'s str },
    Field { owner: Owner<'s>, name: &'s str },
    Type(&'s str),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Owner<'s> {
    Event(&'s str),
    Struct(&'s str),
    Variant { union: &'s str, variant: &'s str },
    Arguments(&'s str),
    Results(&'s str),
}

fn context<'s>(tokens: &[(Token<'s>, Span)], at: usize) -> Option<Context<'s>> {
    let Token::Name(name) = tokens[at].0 else { return None };
    if at > 0
        && let Token::Name(keyword) = tokens[at - 1].0
        && DECLARING.contains(&keyword)
    {
        return Some(Context::Declaration { keyword, name });
    }
    if tokens.get(at + 1).is_some_and(|(token, _)| *token == Token::Symbol(":")) {
        return owner(tokens, at).map(|owner| Context::Field { owner, name });
    }
    Some(Context::Type(name))
}

fn owner<'s>(tokens: &[(Token<'s>, Span)], at: usize) -> Option<Owner<'s>> {
    let mut current: Option<(&str, &str)> = None;
    let mut variant = None;
    let mut results = false;
    let mut depth = 0;
    for index in 0..at {
        match tokens[index].0 {
            Token::Name(keyword) if DECLARING.contains(&keyword) && depth == 0 => {
                if let Some((Token::Name(name), _)) = tokens.get(index + 1) {
                    current = Some((keyword, name));
                    variant = None;
                    results = false;
                }
            }
            Token::Symbol("{") => {
                if depth == 1
                    && let Some((Token::Name(name), _)) = index.checked_sub(1).map(|before| &tokens[before])
                {
                    variant = Some(*name);
                }
                depth += 1;
            }
            Token::Symbol("}") => depth -= 1,
            Token::Symbol("->") if depth == 0 => results = true,
            _ => {}
        }
    }
    let (keyword, name) = current?;
    Some(match keyword {
        "event" => Owner::Event(name),
        "struct" => Owner::Struct(name),
        "union" => Owner::Variant { union: name, variant: variant? },
        "function" if results => Owner::Results(name),
        "function" => Owner::Arguments(name),
        _ => return None,
    })
}

fn fields<'a>(schema: &'a Schema, owner: Owner) -> Option<&'a [Field<'a>]> {
    match owner {
        Owner::Event(name) => schema
            .events
            .iter()
            .find(|event| event.name == name && event.role == Role::Message)
            .map(|event| event.fields.as_slice()),
        Owner::Struct(name) => {
            schema.structs.iter().find(|declared| declared.name == name).map(|declared| declared.fields.as_slice())
        }
        Owner::Variant { union, variant } => schema
            .unions
            .iter()
            .find(|declared| declared.name == union)?
            .variants
            .iter()
            .find(|declared| declared.name == variant)
            .map(|declared| declared.fields.as_slice()),
        Owner::Arguments(name) => {
            schema.functions.iter().find(|function| function.name == name).map(|function| function.arguments.as_slice())
        }
        Owner::Results(name) => {
            schema.functions.iter().find(|function| function.name == name).map(|function| function.results.as_slice())
        }
    }
}

pub fn article(number: u64) -> &'static str {
    let text = number.to_string();
    let eleven = text.len() % 3 == 2 && (text.starts_with("11") || text.starts_with("18"));
    if text.starts_with('8') || eleven { "an" } else { "a" }
}

fn plural(bits: u32) -> String {
    if bits == 1 { "1 bit".to_string() } else { format!("{bits} bits") }
}

fn least_bits(bits: u32, varies: bool) -> String {
    match (bits, varies) {
        (0, true) => "0 or more bits".to_string(),
        (_, true) => format!("at least {}", plural(bits)),
        _ => plural(bits),
    }
}

fn sized(bits: u32, noun: &str) -> String {
    format!("{} {bits} bit {noun}", article(bits.into()))
}

fn bytes(bits: u32) -> String {
    let bytes = f64::from(bits) / 8.0;
    if bytes == 1.0 {
        "1 byte".to_string()
    } else if bytes.fract() == 0.0 {
        format!("{bytes:.0} bytes")
    } else {
        format!("{bytes:.1} bytes")
    }
}

fn names(list: &[&str]) -> String {
    let quoted: Vec<String> = list.iter().map(|name| format!("`{name}`")).collect();
    match quoted.as_slice() {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn varying<'f>(schema: &Schema, fields: &'f [Field]) -> Vec<&'f str> {
    fields
        .iter()
        .filter(|field| measure(schema, field.ty).is_ok_and(|measured| measured.variable))
        .map(|field| field.name.as_ref())
        .collect()
}

fn fields_text(schema: &Schema, fields: &[Field]) -> Vec<String> {
    fields.iter().map(|field| format!("- `{}`: {}", field.name, describe(schema, field.ty))).collect()
}

fn collection(kind: &str, bits: u32, bounds: courier_compiler::schema::Bounds, items: &str) -> String {
    match bounds.max {
        Some(max) if max == bounds.min => format!("a {kind} of exactly {max}: {items}"),
        Some(max) => format!("a {kind} of up to {max}: {}, then {items}", sized(bits, "count")),
        None => format!("a {kind} with no bound: a count of at least 3 bits, then {items}"),
    }
}

fn describe(schema: &Schema, ty: TypeId) -> String {
    let Ok(measured) = measure(schema, ty) else { return "does not lay out".to_string() };
    let Measure { bits, min_bits, variable, per_item, item_varies } = measured;
    match schema.ty(ty) {
        &Type::Array { bounds, .. } => {
            let items = match per_item {
                Some(item) => format!("{} per item", least_bits(item, item_varies)),
                None => "its items packed together in chunks".to_string(),
            };
            collection("list", bits, bounds, &items)
        }
        &Type::Map { bounds, .. } => {
            let items = format!("{} per entry", least_bits(per_item.unwrap_or(0), item_varies));
            collection("map", bits, bounds, &items)
        }
        Type::Text(bounds) | Type::Buffer(bounds) => match bounds.max {
            Some(1) if bounds.min == 1 => "exactly 1 byte, in the byte section".to_string(),
            Some(max) if max == bounds.min => format!("exactly {max} bytes, in the byte section"),
            Some(_) => format!("{}, then its bytes in the byte section", sized(bits, "length")),
            None => "a length of at least 4 bits, then its bytes in the byte section".to_string(),
        },
        Type::Instance { .. } | Type::Handle => "no bits; it is sent beside the frame as an InstanceHandle".to_string(),
        Type::Union(_) => format!("{}, then the chosen variant's fields", sized(bits, "variant tag")),
        Type::Optional(inner) if matches!(schema.ty(*inner), Type::Union(_)) => {
            format!("{}, 0 for nil, then the chosen variant's fields", sized(bits, "variant tag"))
        }
        Type::Optional(inner) => format!("a presence bit; when present, {}", describe(schema, *inner)),
        Type::Enum(id) => format!("{}, one of {} names", plural(bits), schema.enumeration(*id).values.len()),
        Type::Struct(id) if variable => {
            let varied = names(&varying(schema, &schema.structure(*id).fields));
            if min_bits == 0 {
                format!("no bits of its own; its size is what {varied} take")
            } else {
                format!("at least {}, more with {varied}", plural(min_bits))
            }
        }
        _ if variable => format!("at least {}", plural(min_bits)),
        _ if min_bits == 0 => "no bits".to_string(),
        _ => plural(min_bits),
    }
}

fn event_size(schema: &Schema, laid: &Layout, event: &EventLayout) -> String {
    let node = laid.node(event.node);
    let declared = &schema.events[event.event];
    let total = node.min_bits + if event.aligned { event.code_bits } else { 0 };
    let id = if event.aligned {
        format!("the {} bit id and up to 7 bits of padding to a byte included", event.code_bits)
    } else {
        format!("the {} bit id included", event.code_bits)
    };
    let mut lines = if varies(laid, event.node) {
        let varied = varying(schema, &declared.fields);
        let varied = if varied.is_empty() { "its fields".to_string() } else { names(&varied) };
        vec![format!("**{}**: at least {} per message, {id}; more with {varied}", declared.name, plural(total))]
    } else {
        vec![format!("**{}**: {} per message ({}), {id}", declared.name, plural(total), bytes(total))]
    };
    lines.extend(fields_text(schema, &declared.fields));
    lines.join("\n")
}

fn declared_text(schema: &Schema, name: &str) -> Option<String> {
    let mut copy = schema.clone();
    if let Some(id) = copy.structs.iter().position(|declared| declared.name == name) {
        let ty = copy.intern(Type::Struct(courier_compiler::schema::StructId(id as u32)));
        let mut lines = vec![format!("**{name}**: {}", describe(&copy, ty))];
        lines.extend(fields_text(&copy, &copy.structs[id].fields));
        return Some(lines.join("\n"));
    }
    if let Some(id) = copy.unions.iter().position(|declared| declared.name == name) {
        let ty = copy.intern(Type::Union(courier_compiler::schema::UnionId(id as u32)));
        let mut lines = vec![format!("**{name}**: {}", describe(&copy, ty))];
        for variant in copy.unions[id].variants.clone() {
            if variant.fields.is_empty() {
                lines.push(format!("- `{}`: no fields", variant.name));
                continue;
            }
            let shape = courier_compiler::schema::StructId(copy.structs.len() as u32);
            copy.structs.push(courier_compiler::schema::Struct { name: variant.name.clone(), fields: variant.fields });
            let fields = copy.intern(Type::Struct(shape));
            lines.push(format!("- `{}`: {}", variant.name, describe(&copy, fields)));
        }
        return Some(lines.join("\n"));
    }
    let id = copy.enums.iter().position(|declared| declared.name == name)?;
    let ty = copy.intern(Type::Enum(courier_compiler::schema::EnumId(id as u32)));
    let values: Vec<&str> = copy.enums[id].values.iter().map(|value| value.as_ref()).collect();
    let listed = if values.len() <= 8 { format!(": {}", names(&values)) } else { String::new() };
    Some(format!("**{name}**: {}{listed}", describe(&copy, ty)))
}

pub fn hover(text: &str, at: Position) -> Option<Hover> {
    hover_unit(&[("", text)], 0, at)
}

pub fn hover_unit(files: &[(&str, &str)], current: usize, at: Position) -> Option<Hover> {
    let text = files.get(current)?.1;
    let tokens = tokenize(text).ok()?;
    let span = span_at(text, at);
    let index = tokens.iter().position(|(token, start)| {
        start.line == span.line && start.column <= span.column && span.column < start.column + width(token)
    })?;
    let schema = parse_unit(files).ok()?;
    let laid = layout(&schema).ok()?;
    let markdown = match context(&tokens, index)? {
        Context::Declaration { keyword: "event" | "function", name } => {
            let mut matched: Vec<&EventLayout> = laid
                .events()
                .map(|(_, event)| event)
                .filter(|event| {
                    let declared = &schema.events[event.event];
                    match declared.role {
                        Role::Message => declared.name == name,
                        Role::Request(id) | Role::Reply(id) => schema.function(id).name == name,
                    }
                })
                .collect();
            matched.sort_by_key(|event| matches!(schema.events[event.event].role, Role::Reply(_)));
            let sizes: Vec<String> = matched.iter().map(|event| event_size(&schema, &laid, event)).collect();
            sizes.join("\n\n")
        }
        Context::Declaration { name, .. } | Context::Type(name) => declared_text(&schema, name)?,
        Context::Field { owner, name } => {
            let field = fields(&schema, owner)?.iter().find(|field| field.name == name)?;
            format!("`{name}`: {}", describe(&schema, field.ty))
        }
    };
    let (_, start) = tokens[index];
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value: markdown }),
        range: Some(range(text, start, width(&tokens[index].0))),
    })
}

fn declarations<'s>(tokens: &[(Token<'s>, Span)]) -> Vec<(&'s str, &'s str, Span)> {
    tokens
        .windows(2)
        .filter_map(|pair| match (pair[0].0, pair[1].0) {
            (Token::Name(keyword), Token::Name(name)) if DECLARING.contains(&keyword) => {
                Some((keyword, name, pair[1].1))
            }
            _ => None,
        })
        .collect()
}

pub fn definition(text: &str, uri: &Uri, at: Position) -> Option<GotoDefinitionResponse> {
    definition_unit(&[("", text)], std::slice::from_ref(uri), 0, at)
}

pub fn definition_unit(
    files: &[(&str, &str)],
    uris: &[Uri],
    current: usize,
    at: Position,
) -> Option<GotoDefinitionResponse> {
    let text = files.get(current)?.1;
    let tokens = tokenize(text).ok()?;
    let span = span_at(text, at);
    let (token, _) = tokens.iter().find(|(token, start)| {
        start.line == span.line && start.column <= span.column && span.column < start.column + width(token)
    })?;
    let Token::Name(name) = token else { return None };
    let (file, declared) = unit_tokens(files).iter().enumerate().find_map(|(file, tokens)| {
        let (_, _, declared) = declarations(tokens).into_iter().find(|(_, declared, _)| declared == name)?;
        Some((file, declared))
    })?;
    let location = Location { uri: uris.get(file)?.clone(), range: range(files[file].1, declared, name.len() as u32) };
    Some(GotoDefinitionResponse::Scalar(location))
}

pub fn completions(text: &str) -> Vec<CompletionItem> {
    completions_unit(&[("", text)])
}

pub fn completions_unit(files: &[(&str, &str)]) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = BASE_TYPES
        .iter()
        .map(|(label, snippet)| CompletionItem {
            label: (*label).to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            insert_text: Some((*snippet).to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..CompletionItem::default()
        })
        .collect();
    for tokens in unit_tokens(files) {
        for (keyword, name, _) in declarations(&tokens) {
            let kind = match keyword {
                "enum" => CompletionItemKind::ENUM,
                "struct" => CompletionItemKind::STRUCT,
                "union" => CompletionItemKind::ENUM,
                _ => continue,
            };
            items.push(CompletionItem { label: name.to_string(), kind: Some(kind), ..CompletionItem::default() });
        }
    }
    items
}

#[allow(deprecated)]
pub fn symbols(text: &str) -> Vec<DocumentSymbol> {
    let Ok(tokens) = tokenize(text) else { return Vec::new() };
    declarations(&tokens)
        .into_iter()
        .map(|(keyword, name, span)| {
            let kind = match keyword {
                "enum" => SymbolKind::ENUM,
                "struct" => SymbolKind::STRUCT,
                "union" => SymbolKind::ENUM,
                "event" => SymbolKind::EVENT,
                _ => SymbolKind::FUNCTION,
            };
            let range = range(text, span, name.len() as u32);
            DocumentSymbol {
                name: name.to_string(),
                detail: Some(keyword.to_string()),
                kind,
                tags: None,
                deprecated: None,
                range,
                selection_range: range,
                children: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_read_as_one() {
        let files = [("A.courier", "event Hit from Server { W: Weapon }"), ("B.courier", "enum Weapon { Sword, Bow }")];
        assert!(diagnose_unit(&files).iter().all(Vec::is_empty));
        let hovered = hover_unit(&files, 0, Position { line: 0, character: 28 }).expect("a hover on Weapon");
        let HoverContents::Markup(shown) = hovered.contents else { panic!("markdown") };
        assert!(shown.value.starts_with("**Weapon**"));
        let files = [("A.courier", "event Hit from Server { W: Weapon }"), ("B.courier", "enum Weapon { Sword }")];
        let found = diagnose_unit(&files);
        assert!(found[0].is_empty());
        assert_eq!(found[1][0].range.start, Position { line: 0, character: 5 });
    }
}
