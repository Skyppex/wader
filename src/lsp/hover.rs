//! `textDocument/hover`.

use serde::{Deserialize, Serialize};

use super::base::{MarkupContent, Range, TextDocumentPositionParams};

pub type HoverParams = TextDocumentPositionParams;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hover {
    pub contents: MarkupContent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<Range>,
}
