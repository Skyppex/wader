//! An in-process client: drives a [`Server`] without an editor, for the
//! command line and for tests.
//!
//! Every message goes through the real wire codec in both directions, so
//! this exercises the same path an editor does, minus the pipe.

use std::collections::HashMap;

use serde_json::Value;

use crate::lsp::{self, notification as n, request as r};
use crate::rpc::{self, Message, Notification, Request, RequestId, ResponseError};
use crate::server::Server;

pub struct Client {
    server: Server,
    next_id: i64,
    versions: HashMap<lsp::Uri, i32>,
    /// Every notification the server sent, oldest first.
    pub notifications: Vec<Notification>,
}

/// Capabilities of a well-equipped editor.
pub fn editor_capabilities() -> lsp::ClientCapabilities {
    serde_json::from_value(serde_json::json!({
        "textDocument": {
            "hover": {"contentFormat": ["markdown", "plaintext"]},
            "completion": {"completionItem": {"snippetSupport": true}},
            "signatureHelp": {"signatureInformation": {
                "parameterInformation": {"labelOffsetSupport": true}
            }},
            "rename": {"prepareSupport": true}
        }
    }))
    .expect("valid capabilities")
}

/// Send `m` through the codec and read it back.
fn wire(m: Message) -> Message {
    let mut buf = Vec::new();
    rpc::write_message(&mut buf, &m).expect("write to memory");
    rpc::read_message(&mut &buf[..])
        .expect("read back what was written")
        .expect("one message")
}

impl Client {
    /// Initialize with an editor's capabilities and UTF-16 positions.
    pub fn new() -> Client {
        Client::with_capabilities(editor_capabilities()).0
    }

    pub fn with_capabilities(capabilities: lsp::ClientCapabilities) -> (Client, lsp::InitializeResult) {
        let mut client = Client {
            server: Server::new(),
            next_id: 0,
            versions: HashMap::new(),
            notifications: Vec::new(),
        };
        let result = client
            .request::<r::Initialize>(lsp::InitializeParams {
                capabilities,
                client_info: Some(lsp::ClientInfo {
                    name: "wader-client".into(),
                    version: None,
                }),
                ..Default::default()
            })
            .expect("initialize succeeds");
        client.notify::<n::Initialized>(lsp::InitializedParams {});
        (client, result)
    }

    fn send(&mut self, m: Message) -> Vec<Message> {
        let out = self.server.handle(wire(m));
        let mut rest = Vec::new();
        for m in out.into_iter().map(wire) {
            match m {
                Message::Notification(note) => self.notifications.push(note),
                other => rest.push(other),
            }
        }
        rest
    }

    /// Send a raw request and return the raw result.
    pub fn request_raw(&mut self, method: &str, params: Value) -> Result<Value, ResponseError> {
        self.next_id += 1;
        let id = RequestId::Int(self.next_id);
        let replies = self.send(
            Request {
                id: id.clone(),
                method: method.to_owned(),
                params: Some(params),
            }
            .into(),
        );
        let response = replies
            .into_iter()
            .find_map(|m| match m {
                Message::Response(r) if r.id.as_ref() == Some(&id) => Some(r),
                _ => None,
            })
            .expect("the server answers every request");
        match response.error {
            Some(e) => Err(e),
            None => Ok(response.result.unwrap_or(Value::Null)),
        }
    }

    pub fn request<R: lsp::Request>(&mut self, params: R::Params) -> Result<R::Result, ResponseError> {
        let params = serde_json::to_value(params).expect("params serialize");
        let value = self.request_raw(R::METHOD, params)?;
        Ok(serde_json::from_value(value).expect("server sent a well-formed result"))
    }

    pub fn notify<N: lsp::Notification>(&mut self, params: N::Params) {
        let params = serde_json::to_value(params).expect("params serialize");
        self.send(
            Notification {
                method: N::METHOD.to_owned(),
                params: Some(params),
            }
            .into(),
        );
    }

    pub fn open(&mut self, uri: &lsp::Uri, text: &str) {
        self.versions.insert(uri.clone(), 1);
        self.notify::<n::DidOpenTextDocument>(lsp::DidOpenTextDocumentParams {
            text_document: lsp::TextDocumentItem {
                uri: uri.clone(),
                language_id: "rill".into(),
                version: 1,
                text: text.to_owned(),
            },
        });
    }

    /// Replace the whole text of an open document.
    pub fn change(&mut self, uri: &lsp::Uri, text: &str) {
        let version = self.versions.entry(uri.clone()).or_insert(0);
        *version += 1;
        let version = *version;
        self.notify::<n::DidChangeTextDocument>(lsp::DidChangeTextDocumentParams {
            text_document: lsp::VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version,
            },
            content_changes: vec![lsp::TextDocumentContentChangeEvent {
                range: None,
                text: text.to_owned(),
            }],
        });
    }

    /// The diagnostics most recently published for `uri`.
    pub fn diagnostics(&self, uri: &lsp::Uri) -> Option<Vec<lsp::Diagnostic>> {
        self.notifications
            .iter()
            .rev()
            .filter(|note| note.method == <n::PublishDiagnostics as lsp::Notification>::METHOD)
            .filter_map(|note| {
                serde_json::from_value::<lsp::PublishDiagnosticsParams>(note.params.clone()?).ok()
            })
            .find(|p| &p.uri == uri)
            .map(|p| p.diagnostics)
    }

    /// `shutdown` then `exit`; returns the server's exit code.
    pub fn shutdown(mut self) -> i32 {
        self.request::<r::Shutdown>(()).expect("shutdown succeeds");
        self.notify::<n::Exit>(());
        self.server.exit_code().expect("exit sets a code")
    }
}

impl Default for Client {
    fn default() -> Client {
        Client::new()
    }
}
