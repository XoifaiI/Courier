//! `courier lsp`, the language server the VS Code extension runs. Each file is read with the
//! other schemas of its folder, open ones as the editor holds them, and errors are published to
//! whichever file of the folder they are in. Saving a schema compiles its folder, as `courier`
//! would, unless the editor turned that off.

use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use courier_compiler::codegen::Mode;
use courier_editor::{
    completions_unit, definition_unit, diagnose_unit, hover_unit, inlay_hints, references_unit, rename_unit, symbols,
};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::{
    CompletionOptions, DocumentSymbolResponse, HoverProviderCapability, LogMessageParams, MessageType, OneOf,
    PublishDiagnosticsParams, SaveOptions, ServerCapabilities, ShowMessageParams, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, Uri,
};

use crate::{Command, Error as Build, compile, schemas_in};

type Documents = HashMap<String, String>;

struct Unit {
    files: Vec<(String, String)>,
    uris: Vec<Uri>,
    current: usize,
}

impl Unit {
    fn files(&self) -> Vec<(&str, &str)> {
        self.files.iter().map(|(name, text)| (name.as_str(), text.as_str())).collect()
    }
}

pub fn serve() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (connection, threads) = Connection::stdio();
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(TextDocumentSyncKind::FULL),
            save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions { include_text: Some(false) })),
            ..TextDocumentSyncOptions::default()
        })),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Left(true)),
        inlay_hint_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![":".into(), "(".into(), ",".into()]),
            ..CompletionOptions::default()
        }),
        ..ServerCapabilities::default()
    };
    let started = connection.initialize(serde_json::to_value(capabilities)?)?;
    let compile_on_save =
        started.pointer("/initializationOptions/compileOnSave").and_then(serde_json::Value::as_bool).unwrap_or(true);
    let mut documents = Documents::new();
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }
                connection.sender.send(Message::Response(answer(&documents, request)))?;
            }
            Message::Notification(notification) => {
                if notification.method == "textDocument/didSave" && compile_on_save {
                    let saved: Option<lsp_types::DidSaveTextDocumentParams> =
                        serde_json::from_value(notification.params.clone()).ok();
                    if let Some(said) = saved.and_then(|saved| build(saved.text_document.uri.as_str())) {
                        connection.sender.send(Message::Notification(said))?;
                    }
                }
                let Some(uri) = update(&mut documents, notification) else { continue };
                let unit = unit(&documents, &uri);
                for (diagnostics, uri) in diagnose_unit(&unit.files()).into_iter().zip(unit.uris) {
                    let params = PublishDiagnosticsParams { uri, diagnostics, version: None };
                    let published = Notification::new("textDocument/publishDiagnostics".into(), params);
                    connection.sender.send(Message::Notification(published))?;
                }
            }
            Message::Response(_) => {}
        }
    }
    drop(threads);
    Ok(())
}

fn build(uri: &str) -> Option<Notification> {
    let folder = path_of(uri)?.parent()?.to_path_buf();
    let schemas = schemas_in(&folder);
    if schemas.is_empty() {
        return None;
    }
    let command = Command { mode: Mode::Api, runtime: None, input: None, output: None, watch: false };
    let (method, params) = match compile(&command, &schemas, &folder) {
        Ok(()) => {
            let message = format!("compiled {} into {}", crate::names(&schemas), folder.display());
            ("window/logMessage", serde_json::to_value(LogMessageParams { typ: MessageType::INFO, message }).ok()?)
        }
        Err(Build::Schema { .. } | Build::Compile { problem: courier_compiler::Error::Layout { .. }, .. }) => {
            return None;
        }
        Err(problem) => {
            let message = format!("Courier: {problem}");
            ("window/showMessage", serde_json::to_value(ShowMessageParams { typ: MessageType::WARNING, message }).ok()?)
        }
    };
    Some(Notification::new(method.into(), params))
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes.get(index + 1..index + 3).and_then(|pair| std::str::from_utf8(pair).ok());
        match (bytes[index], hex.and_then(|pair| u8::from_str_radix(pair, 16).ok())) {
            (b'%', Some(byte)) => {
                out.push(byte);
                index += 3;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn path_of(uri: &str) -> Option<PathBuf> {
    let path = decode(uri.strip_prefix("file://")?);
    let windows = path.as_bytes().get(2) == Some(&b':') && path.starts_with('/');
    Some(PathBuf::from(if windows { &path[1..] } else { &path }))
}

fn uri_of(path: &Path) -> Option<Uri> {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut encoded = String::from(if text.starts_with('/') { "file://" } else { "file:///" });
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => encoded.push(byte as char),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded.parse().ok()
}

fn unit(documents: &Documents, uri: &Uri) -> Unit {
    let alone = || Unit {
        files: vec![(String::new(), documents.get(uri.as_str()).cloned().unwrap_or_default())],
        uris: vec![uri.clone()],
        current: 0,
    };
    let Some(path) = path_of(uri.as_str()) else { return alone() };
    let Some(folder) = path.parent() else { return alone() };
    let mut open: HashMap<PathBuf, (&String, &String)> = HashMap::new();
    for (key, text) in documents {
        if let Some(open_path) = path_of(key) {
            open.insert(open_path, (key, text));
        }
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    paths.extend(open.keys().cloned());
    paths.retain(|candidate| {
        candidate.parent() == Some(folder) && candidate.extension().is_some_and(|extension| extension == "courier")
    });
    if !paths.contains(&path) {
        paths.push(path.clone());
    }
    paths.sort_by_key(|candidate| candidate.file_name().map(|name| name.to_os_string()));
    paths.dedup();
    let mut files = Vec::new();
    let mut uris = Vec::new();
    for candidate in &paths {
        let (found, text) = match open.get(candidate) {
            Some((key, text)) => (key.parse().ok(), (*text).clone()),
            None => (uri_of(candidate), std::fs::read_to_string(candidate).unwrap_or_default()),
        };
        let Some(found) = found else { continue };
        let name = candidate.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        files.push((name, text));
        uris.push(found);
    }
    match paths.iter().position(|candidate| *candidate == path) {
        Some(current) if files.len() == paths.len() => Unit { files, uris, current },
        _ => alone(),
    }
}

fn update(documents: &mut Documents, notification: Notification) -> Option<Uri> {
    let params = notification.params;
    match notification.method.as_str() {
        "textDocument/didOpen" => {
            let opened: lsp_types::DidOpenTextDocumentParams = serde_json::from_value(params).ok()?;
            documents.insert(opened.text_document.uri.as_str().to_string(), opened.text_document.text);
            Some(opened.text_document.uri)
        }
        "textDocument/didChange" => {
            let changed: lsp_types::DidChangeTextDocumentParams = serde_json::from_value(params).ok()?;
            let text = changed.content_changes.into_iter().next_back()?.text;
            documents.insert(changed.text_document.uri.as_str().to_string(), text);
            Some(changed.text_document.uri)
        }
        "textDocument/didClose" => {
            let closed: lsp_types::DidCloseTextDocumentParams = serde_json::from_value(params).ok()?;
            documents.remove(closed.text_document.uri.as_str());
            Some(closed.text_document.uri)
        }
        _ => None,
    }
}

fn answer(documents: &Documents, request: Request) -> Response {
    let id = request.id.clone();
    let params = request.params;
    let result = match request.method.as_str() {
        "textDocument/hover" => serde_json::from_value::<lsp_types::HoverParams>(params).ok().and_then(|params| {
            let place = params.text_document_position_params;
            let unit = unit(documents, &place.text_document.uri);
            serde_json::to_value(hover_unit(&unit.files(), unit.current, place.position)).ok()
        }),
        "textDocument/definition" => {
            serde_json::from_value::<lsp_types::GotoDefinitionParams>(params).ok().and_then(|params| {
                let place = params.text_document_position_params;
                let unit = unit(documents, &place.text_document.uri);
                serde_json::to_value(definition_unit(&unit.files(), &unit.uris, unit.current, place.position)).ok()
            })
        }
        "textDocument/completion" => {
            serde_json::from_value::<lsp_types::CompletionParams>(params).ok().and_then(|params| {
                let place = params.text_document_position;
                let unit = unit(documents, &place.text_document.uri);
                serde_json::to_value(completions_unit(&unit.files(), unit.current, place.position)).ok()
            })
        }
        "textDocument/inlayHint" => {
            serde_json::from_value::<lsp_types::InlayHintParams>(params).ok().and_then(|params| {
                let unit = unit(documents, &params.text_document.uri);
                serde_json::to_value(inlay_hints(&unit.files(), unit.current)).ok()
            })
        }
        "textDocument/references" => {
            serde_json::from_value::<lsp_types::ReferenceParams>(params).ok().and_then(|params| {
                let place = params.text_document_position;
                let unit = unit(documents, &place.text_document.uri);
                serde_json::to_value(references_unit(&unit.files(), &unit.uris, unit.current, place.position)).ok()
            })
        }
        "textDocument/rename" => {
            let Ok(params) = serde_json::from_value::<lsp_types::RenameParams>(params) else {
                return Response::new_err(id, ErrorCode::InvalidParams as i32, "bad rename request".into());
            };
            let place = params.text_document_position;
            let unit = unit(documents, &place.text_document.uri);
            match rename_unit(&unit.files(), &unit.uris, unit.current, place.position, &params.new_name) {
                Ok(edit) => serde_json::to_value(edit).ok(),
                Err(problem) => return Response::new_err(id, ErrorCode::InvalidRequest as i32, problem),
            }
        }
        "textDocument/documentSymbol" => {
            serde_json::from_value::<lsp_types::DocumentSymbolParams>(params).ok().and_then(|params| {
                let text = documents.get(params.text_document.uri.as_str())?;
                serde_json::to_value(DocumentSymbolResponse::Nested(symbols(text))).ok()
            })
        }
        _ => return Response::new_err(id, -32601, format!("{} is not supported", request.method)),
    };
    Response::new_ok(id, result.unwrap_or(serde_json::Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_and_paths_turn_into_each_other() {
        let path = path_of("file:///e%3A/src/Net/My%20Game.courier").expect("a file uri");
        assert_eq!(path, PathBuf::from("e:/src/Net/My Game.courier"));
        let uri = uri_of(&path).expect("a path");
        assert_eq!(uri.as_str(), "file:///e%3A/src/Net/My%20Game.courier");
        assert_eq!(path_of("file:///home/me/Net.courier"), Some(PathBuf::from("/home/me/Net.courier")));
        assert_eq!(path_of("untitled:Untitled-1"), None);
    }
}
