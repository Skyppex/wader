//! `initialize` and the capabilities both sides announce in it.

use serde::{Deserialize, Serialize};

use super::base::{MarkupKind, PositionEncodingKind, Uri};
use super::completion::CompletionOptions;
use super::rename::RenameOptions;
use super::signature::SignatureHelpOptions;
use super::sync::TextDocumentSyncOptions;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InitializeParams {
    pub process_id: Option<i64>,
    pub client_info: Option<ClientInfo>,
    pub root_uri: Option<Uri>,
    pub capabilities: ClientCapabilities,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// The client capabilities wader looks at.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub general: Option<GeneralClientCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_document: Option<TextDocumentClientCapabilities>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GeneralClientCapabilities {
    /// In order of preference. UTF-16 is implied if missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_encodings: Option<Vec<PositionEncodingKind>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextDocumentClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion: Option<CompletionClientCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover: Option<HoverClientCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_help: Option<SignatureHelpClientCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rename: Option<RenameClientCapabilities>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CompletionClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_item: Option<CompletionItemClientCapabilities>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CompletionItemClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet_support: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_format: Option<Vec<MarkupKind>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HoverClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_format: Option<Vec<MarkupKind>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SignatureHelpClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_information: Option<SignatureInformationClientCapabilities>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SignatureInformationClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_format: Option<Vec<MarkupKind>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameter_information: Option<ParameterInformationClientCapabilities>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ParameterInformationClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_offset_support: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenameClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prepare_support: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub capabilities: ServerCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_info: Option<ServerInfo>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_encoding: Option<PositionEncodingKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_document_sync: Option<TextDocumentSyncOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_provider: Option<CompletionOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_help_provider: Option<SignatureHelpOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declaration_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definition_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references_provider: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rename_provider: Option<RenameProvider>,
}

/// `true`, or options. Options are only allowed if the client said it
/// supports `prepareRename`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RenameProvider {
    Bool(bool),
    Options(RenameOptions),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializedParams {}
