//! `textDocument/rename` and `textDocument/prepareRename`.

use serde::{Deserialize, Serialize};

use super::base::{Position, Range, TextDocumentIdentifier, TextDocumentPositionParams};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prepare_provider: Option<bool>,
}

pub type PrepareRenameParams = TextDocumentPositionParams;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PrepareRenameResult {
    RangeWithPlaceholder {
        range: Range,
        placeholder: String,
    },
    Range(Range),
    DefaultBehavior {
        #[serde(rename = "defaultBehavior")]
        default_behavior: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    pub new_name: String,
}
