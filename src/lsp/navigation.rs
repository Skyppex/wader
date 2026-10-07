//! Go to definition / declaration and find references.

use serde::{Deserialize, Serialize};

use super::base::{Position, TextDocumentIdentifier, TextDocumentPositionParams};

pub type DefinitionParams = TextDocumentPositionParams;
pub type DeclarationParams = TextDocumentPositionParams;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    pub context: ReferenceContext,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceContext {
    pub include_declaration: bool,
}
