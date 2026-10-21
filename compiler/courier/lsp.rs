//! `courier lsp`, the language server the VS Code extension runs.

use std::collections::HashMap;
use std::error::Error;

use courier_editor::{completions, definition, diagnose, hover, symbols};
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::{
    CompletionOptions, DocumentSymbolResponse, HoverProviderCapability, OneOf, PublishDiagnosticsParams,
    ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
};

pub fn serve() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (connection, threads) = Connection::stdio();
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![":".into(), "(".into(), ",".into()]),
            ..CompletionOptions::default()
        }),
        ..ServerCapabilities::default()
    };
    connection.initialize(serde_json::to_value(capabilities)?)?;
    let mut documents: HashMap<String, String> = HashMap::new();
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }
                connection.sender.send(Message::Response(answer(&documents, request)))?;
            }
            Message::Notification(notification) => {
                if let Some(uri) = update(&mut documents, notification) {
                    let diagnostics = documents.get(uri.as_str()).map(|text| diagnose(text)).unwrap_or_default();
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

fn update(documents: &mut HashMap<String, String>, notification: Notification) -> Option<Uri> {
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

fn answer(documents: &HashMap<String, String>, request: Request) -> Response {
    let id = request.id.clone();
    let params = request.params;
    let result = match request.method.as_str() {
        "textDocument/hover" => serde_json::from_value::<lsp_types::HoverParams>(params).ok().and_then(|params| {
            let place = params.text_document_position_params;
            let text = documents.get(place.text_document.uri.as_str())?;
            serde_json::to_value(hover(text, place.position)).ok()
        }),
        "textDocument/definition" => {
            serde_json::from_value::<lsp_types::GotoDefinitionParams>(params).ok().and_then(|params| {
                let place = params.text_document_position_params;
                let text = documents.get(place.text_document.uri.as_str())?;
                serde_json::to_value(definition(text, &place.text_document.uri, place.position)).ok()
            })
        }
        "textDocument/completion" => {
            serde_json::from_value::<lsp_types::CompletionParams>(params).ok().and_then(|params| {
                let text = documents.get(params.text_document_position.text_document.uri.as_str())?;
                serde_json::to_value(completions(text)).ok()
            })
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
