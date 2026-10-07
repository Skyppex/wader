//! Lifecycle, document sync and diagnostics, end to end.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::json;
use wader::client::Client;
use wader::lsp::{self, DiagnosticSeverity, Position, request as r};
use wader::rpc::{self, ErrorCode, Message, Notification, Request};
use wader::server::Server;

fn uri() -> lsp::Uri {
    "file:///test.rill".into()
}

#[test]
fn requests_before_initialize_are_refused() {
    let mut server = Server::new();
    let out = server.handle(
        Request {
            id: 1.into(),
            method: "textDocument/hover".into(),
            params: Some(json!({})),
        }
        .into(),
    );
    let [Message::Response(resp)] = &out[..] else {
        panic!("{out:?}")
    };
    assert_eq!(
        resp.error.as_ref().unwrap().code,
        ErrorCode::SERVER_NOT_INITIALIZED
    );
}

#[test]
fn initialize_announces_capabilities() {
    let (client, result) = Client::with_capabilities(wader::client::editor_capabilities());
    assert_eq!(result.server_info.unwrap().name, "wader");
    let caps = result.capabilities;
    assert_eq!(
        caps.position_encoding,
        Some(lsp::PositionEncodingKind::utf16())
    );
    assert_eq!(
        caps.text_document_sync.unwrap().change,
        lsp::TextDocumentSyncKind::Full
    );
    assert_eq!(client.shutdown(), 0);
}

#[test]
fn utf8_is_preferred_when_offered() {
    let caps = lsp::ClientCapabilities {
        general: Some(lsp::GeneralClientCapabilities {
            position_encodings: Some(vec![
                lsp::PositionEncodingKind::utf16(),
                lsp::PositionEncodingKind::utf8(),
            ]),
        }),
        ..Default::default()
    };
    let (_, result) = Client::with_capabilities(caps);
    assert_eq!(
        result.capabilities.position_encoding,
        Some(lsp::PositionEncodingKind::utf8())
    );
}

#[test]
fn unknown_methods_and_bad_params() {
    let mut client = Client::new();
    let e = client
        .request_raw("textDocument/nope", json!({}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::METHOD_NOT_FOUND);
    let e = client
        .request_raw("textDocument/hover", json!({"position": 3}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::INVALID_PARAMS);
    // Unknown notifications are ignored without a reply.
    client.notify::<lsp::notification::Cancel>(lsp::CancelParams { id: json!(1) });
}

#[test]
fn after_shutdown_only_exit() {
    let mut client = Client::new();
    client.request::<r::Shutdown>(()).unwrap();
    let e = client.request::<r::Shutdown>(()).unwrap_err();
    assert_eq!(e.code, ErrorCode::INVALID_REQUEST);
}

#[test]
fn exit_without_shutdown_is_code_1() {
    let mut server = Server::new();
    server.handle(
        Notification {
            method: "exit".into(),
            params: None,
        }
        .into(),
    );
    assert_eq!(server.exit_code(), Some(1));
}

#[test]
fn diagnostics_follow_edits() {
    let mut client = Client::new();
    let good = "rill main() Sample {\n    return sin(0.5)\n}\n";
    client.open(&uri(), good);
    assert_eq!(client.diagnostics(&uri()), Some(vec![]));

    let bad = "rill main() Sample {\n    return sine(0.5)\n}\n";
    client.change(&uri(), bad);
    let diags = client.diagnostics(&uri()).unwrap();
    assert_eq!(diags.len(), 1, "{diags:?}");
    let d = &diags[0];
    assert_eq!(d.severity, Some(DiagnosticSeverity::Error));
    assert_eq!(d.source.as_deref(), Some("rill"));
    assert_eq!(d.range.start, Position::new(1, 11));
    assert_eq!(d.range.end, Position::new(1, 15));
    assert!(d.message.contains("sine"), "{}", d.message);
    assert!(
        d.message.contains("help: did you mean `sin`?"),
        "{}",
        d.message
    );

    client.notify::<lsp::notification::DidCloseTextDocument>(lsp::DidCloseTextDocumentParams {
        text_document: lsp::TextDocumentIdentifier { uri: uri() },
    });
    assert_eq!(client.diagnostics(&uri()), Some(vec![]));
}

#[test]
fn library_files_without_main_are_fine() {
    let mut client = Client::new();
    client.open(
        &uri(),
        "fn half(x: Sample) Sample {\n    return x / 2\n}\n",
    );
    assert_eq!(client.diagnostics(&uri()), Some(vec![]));
}

#[test]
fn lexer_errors_are_reported() {
    let mut client = Client::new();
    client.open(&uri(), "rill main() Sample {\n    return 440hz\n}\n");
    let diags = client.diagnostics(&uri()).unwrap();
    assert!(
        diags[0].message.starts_with("unknown unit `hz`"),
        "{diags:?}"
    );
}

#[test]
fn positions_count_utf16_units() {
    let mut client = Client::new();
    // The comment holds a note (2 UTF-16 units) before the error.
    client.open(&uri(), "// 🎵\nrill main() Sample { return nope }\n");
    let diags = client.diagnostics(&uri()).unwrap();
    let col = "rill main() Sample { return ".len() as u32;
    assert_eq!(diags[0].range.start, Position::new(1, col));

    let mut client = Client::new();
    client.open(&uri(), "rill main() Sample { /*🎵*/ return nope }\n");
    let diags = client.diagnostics(&uri()).unwrap();
    let col = "rill main() Sample { /*".len() as u32 + 2 + "*/ return ".len() as u32;
    assert_eq!(diags[0].range.start, Position::new(0, col));
}

/// The binary, over real pipes.
#[test]
fn stdio_round_trip() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wader"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = Vec::new();
    let mut send = |m: Message| rpc::write_message(&mut input, &m).unwrap();
    send(
        Request {
            id: 1.into(),
            method: "initialize".into(),
            params: Some(json!({"capabilities": {}})),
        }
        .into(),
    );
    send(
        Notification {
            method: "textDocument/didOpen".into(),
            params: Some(json!({"textDocument": {
                "uri": "file:///a.rill", "languageId": "rill", "version": 1,
                "text": "rill main() Sample { return nope }"
            }})),
        }
        .into(),
    );
    send(
        Request {
            id: 2.into(),
            method: "shutdown".into(),
            params: None,
        }
        .into(),
    );
    send(
        Notification {
            method: "exit".into(),
            params: None,
        }
        .into(),
    );
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());

    let mut reader = &output.stdout[..];
    let mut methods = Vec::new();
    while let Some(m) = rpc::read_message(&mut reader).unwrap() {
        methods.push(match m {
            Message::Response(r) => format!("response {:?}", r.id.unwrap()),
            Message::Notification(n) => n.method,
            Message::Request(r) => r.method,
        });
    }
    assert_eq!(
        methods,
        [
            "response Int(1)",
            "textDocument/publishDiagnostics",
            "response Int(2)"
        ]
    );
}
