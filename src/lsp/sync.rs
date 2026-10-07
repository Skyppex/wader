//! Keeping the server's copy of a document in step with the editor.

use serde::{Deserialize, Serialize};

use super::base::{Range, TextDocumentIdentifier, TextDocumentItem, VersionedTextDocumentIdentifier};

int_enum! {
    pub enum TextDocumentSyncKind {
        None = 0,
        Full = 1,
        Incremental = 2,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentSyncOptions {
    pub open_close: bool,
    pub change: TextDocumentSyncKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidOpenTextDocumentParams {
    pub text_document: TextDocumentItem,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidChangeTextDocumentParams {
    pub text_document: VersionedTextDocumentIdentifier,
    pub content_changes: Vec<TextDocumentContentChangeEvent>,
}

/// With `range`, replace that part; without, the whole text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextDocumentContentChangeEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidCloseTextDocumentParams {
    pub text_document: TextDocumentIdentifier,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidSaveTextDocumentParams {
    pub text_document: TextDocumentIdentifier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}
