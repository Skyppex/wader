//! Structures shared by many requests.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// A document URI. Only compared and echoed back, so it stays a string.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Uri(pub String);

impl fmt::Display for Uri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Uri {
    fn from(s: &str) -> Uri {
        Uri(s.to_owned())
    }
}

impl From<String> for Uri {
    fn from(s: String) -> Uri {
        Uri(s)
    }
}

/// Zero-based line and character offset. What a "character" is depends on
/// the negotiated [`PositionEncodingKind`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub fn new(line: u32, character: u32) -> Position {
        Position { line, character }
    }
}

impl fmt::Display for Position {
    /// One-based `line:col`, the way editors show it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line + 1, self.character + 1)
    }
}

/// `start` inclusive, `end` exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start: Position, end: Position) -> Range {
        Range { start, end }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    pub uri: Uri,
    pub range: Range,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextDocumentIdentifier {
    pub uri: Uri,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionedTextDocumentIdentifier {
    pub uri: Uri,
    pub version: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentItem {
    pub uri: Uri,
    pub language_id: String,
    pub version: i32,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentPositionParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceEdit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changes: Option<BTreeMap<Uri, Vec<TextEdit>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarkupKind {
    #[serde(rename = "plaintext")]
    PlainText,
    Markdown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkupContent {
    pub kind: MarkupKind,
    pub value: String,
}

impl MarkupContent {
    pub fn markdown(value: impl Into<String>) -> MarkupContent {
        MarkupContent {
            kind: MarkupKind::Markdown,
            value: value.into(),
        }
    }

    pub fn plain(value: impl Into<String>) -> MarkupContent {
        MarkupContent {
            kind: MarkupKind::PlainText,
            value: value.into(),
        }
    }
}

/// How `Position::character` counts. Clients may offer more than the three
/// the spec names, so this stays an open set of strings.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PositionEncodingKind(pub String);

impl PositionEncodingKind {
    pub fn utf8() -> PositionEncodingKind {
        PositionEncodingKind("utf-8".into())
    }

    pub fn utf16() -> PositionEncodingKind {
        PositionEncodingKind("utf-16".into())
    }

    pub fn utf32() -> PositionEncodingKind {
        PositionEncodingKind("utf-32".into())
    }
}
