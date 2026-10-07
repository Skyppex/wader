//! Helpers shared by the end-to-end tests.
#![allow(dead_code)]

use wader::client::Client;
use wader::lsp::{self, Position};

pub fn uri() -> lsp::Uri {
    "file:///test.rill".into()
}

/// Strip the `$` cursor from `src` and return the text and the cursor's
/// position, counting UTF-16 units like an editor.
pub fn cursor(src: &str) -> (String, Position) {
    let at = src.find('$').expect("a `$` cursor");
    let before = &src[..at];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let character = before[line_start..].encode_utf16().count() as u32;
    (src.replacen('$', "", 1), Position { line, character })
}

/// A client with `src` open; returns the cursor position.
pub fn open(src: &str) -> (Client, Position) {
    let (text, pos) = cursor(src);
    let mut client = Client::new();
    client.open(&uri(), &text);
    (client, pos)
}

pub fn at(position: Position) -> lsp::TextDocumentPositionParams {
    lsp::TextDocumentPositionParams {
        text_document: lsp::TextDocumentIdentifier { uri: uri() },
        position,
    }
}

pub fn range(sl: u32, sc: u32, el: u32, ec: u32) -> lsp::Range {
    lsp::Range::new(Position::new(sl, sc), Position::new(el, ec))
}

/// Rill's examples: next to wader in a checkout, or inside it in the Nix
/// build (see `source.nix`).
pub fn examples_dir() -> std::path::PathBuf {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let inside = here.join("rill/examples");
    if inside.is_dir() {
        inside
    } else {
        here.join("../examples")
    }
}
