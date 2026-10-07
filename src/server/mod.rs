//! The server: lifecycle, open documents, and the main loop.

mod dispatch;
mod handlers;

use std::collections::HashMap;
use std::io::{self, BufRead, Write};

use crate::analysis::Analysis;
use crate::document::{Document, Encoding};
use crate::lsp::{self, InitializeParams, MarkupKind, Uri};
use crate::rpc::{self, ErrorCode, Message, Notification, Response};

/// Print to stderr, which clients show in their LSP log.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        eprintln!("[wader] {}", format_args!($($arg)*))
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Waiting for `initialize`.
    Uninitialized,
    Running,
    /// `shutdown` received; only `exit` is left.
    ShuttingDown,
}

/// What the client said it can handle, boiled down.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClientSupport {
    pub snippets: bool,
    pub markdown: bool,
    pub label_offsets: bool,
    pub prepare_rename: bool,
}

impl ClientSupport {
    fn from_params(params: &InitializeParams) -> ClientSupport {
        let td = params
            .capabilities
            .text_document
            .clone()
            .unwrap_or_default();
        let markdown = |formats: Option<Vec<MarkupKind>>| {
            formats.is_none_or(|f| f.contains(&MarkupKind::Markdown))
        };
        let sig_info = td.signature_help.and_then(|s| s.signature_information);
        ClientSupport {
            snippets: td
                .completion
                .and_then(|c| c.completion_item)
                .and_then(|i| i.snippet_support)
                .unwrap_or(false),
            markdown: markdown(td.hover.and_then(|h| h.content_format)),
            label_offsets: sig_info
                .and_then(|s| s.parameter_information)
                .and_then(|p| p.label_offset_support)
                .unwrap_or(false),
            prepare_rename: td.rename.and_then(|r| r.prepare_support).unwrap_or(false),
        }
    }
}

/// An open document and what is known about it.
pub struct Open {
    pub doc: Document,
    pub analysis: Analysis,
}

pub struct Server {
    phase: Phase,
    encoding: Encoding,
    client: ClientSupport,
    docs: HashMap<Uri, Open>,
    /// Messages to send once the current one is handled.
    outbox: Vec<Message>,
    /// Set by `exit`: the process exit code.
    exit: Option<i32>,
}

impl Default for Server {
    fn default() -> Server {
        Server::new()
    }
}

impl Server {
    pub fn new() -> Server {
        Server {
            phase: Phase::Uninitialized,
            encoding: Encoding::default(),
            client: ClientSupport::default(),
            docs: HashMap::new(),
            outbox: Vec::new(),
            exit: None,
        }
    }

    /// Handle one incoming message, returning what to send back.
    pub fn handle(&mut self, message: Message) -> Vec<Message> {
        match message {
            Message::Request(req) => {
                let response = self.on_request(req);
                self.outbox.push(response.into());
            }
            Message::Notification(n) => self.on_notification(n),
            // wader sends no requests, so there is nothing to match these to.
            Message::Response(r) => log!("ignoring unexpected response {:?}", r.id),
        }
        std::mem::take(&mut self.outbox)
    }

    /// `Some(code)` once `exit` arrived.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit
    }

    fn notify<N: lsp::Notification>(&mut self, params: N::Params) {
        let params = serde_json::to_value(params).expect("params serialize");
        self.outbox.push(
            Notification {
                method: N::METHOD.to_owned(),
                params: Some(params),
            }
            .into(),
        );
    }

    fn publish_diagnostics(&mut self, uri: &Uri) {
        let Some(open) = self.docs.get(uri) else {
            return;
        };
        let params = lsp::PublishDiagnosticsParams {
            uri: uri.clone(),
            version: Some(open.doc.version),
            diagnostics: open.analysis.lsp_diagnostics(&open.doc),
        };
        self.notify::<lsp::notification::PublishDiagnostics>(params);
    }
}

/// Serve over a byte stream until `exit` or end of input. Returns the exit
/// code: 0 after a proper `shutdown` + `exit`, 1 otherwise.
pub fn run(mut reader: impl BufRead, mut writer: impl Write) -> io::Result<i32> {
    let mut server = Server::new();
    loop {
        let message = match rpc::read_message(&mut reader) {
            Ok(Some(m)) => m,
            Ok(None) => {
                log!("input closed without `exit`");
                return Ok(1);
            }
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                log!("{e}");
                let reply = Response::err(None, ErrorCode::PARSE_ERROR, e.to_string());
                rpc::write_message(&mut writer, &reply.into())?;
                continue;
            }
            Err(e) => return Err(e),
        };
        for out in server.handle(message) {
            rpc::write_message(&mut writer, &out)?;
        }
        if let Some(code) = server.exit_code() {
            return Ok(code);
        }
    }
}
