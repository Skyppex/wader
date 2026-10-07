//! `textDocument/signatureHelp`.

use serde::{Deserialize, Serialize};

use super::base::{MarkupContent, Position, TextDocumentIdentifier};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureHelpOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_characters: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrigger_characters: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureHelpParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<SignatureHelpContext>,
}

int_enum! {
    pub enum SignatureHelpTriggerKind {
        Invoked = 1,
        TriggerCharacter = 2,
        ContentChange = 3,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureHelpContext {
    pub trigger_kind: SignatureHelpTriggerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_character: Option<String>,
    #[serde(default)]
    pub is_retrigger: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_signature_help: Option<SignatureHelp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureHelp {
    pub signatures: Vec<SignatureInformation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_signature: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_parameter: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureInformation {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<MarkupContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Vec<ParameterInformation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_parameter: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParameterInformation {
    pub label: ParameterLabel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<MarkupContent>,
}

/// A substring of the signature label, or its `[start, end)` offsets in the
/// label (in the negotiated position encoding).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParameterLabel {
    Simple(String),
    Offsets([u32; 2]),
}
