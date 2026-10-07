//! `textDocument/completion`.

use serde::{Deserialize, Serialize};

use super::base::{MarkupContent, Position, TextDocumentIdentifier, TextEdit};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_characters: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolve_provider: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionParams {
    pub text_document: TextDocumentIdentifier,
    pub position: Position,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<CompletionContext>,
}

int_enum! {
    pub enum CompletionTriggerKind {
        Invoked = 1,
        TriggerCharacter = 2,
        TriggerForIncompleteCompletions = 3,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionContext {
    pub trigger_kind: CompletionTriggerKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_character: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionList {
    pub is_incomplete: bool,
    pub items: Vec<CompletionItem>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_details: Option<CompletionItemLabelDetails>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<CompletionItemKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub documentation: Option<MarkupContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insert_text_format: Option<InsertTextFormat>,
    /// Replaces `range` with `new_text`; takes precedence over `insert_text`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_edit: Option<TextEdit>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionItemLabelDetails {
    /// Shown right after the label, e.g. a signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Shown further away, e.g. a type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

int_enum! {
    pub enum CompletionItemKind {
        Text = 1,
        Method = 2,
        Function = 3,
        Constructor = 4,
        Field = 5,
        Variable = 6,
        Class = 7,
        Interface = 8,
        Module = 9,
        Property = 10,
        Unit = 11,
        Value = 12,
        Enum = 13,
        Keyword = 14,
        Snippet = 15,
        Color = 16,
        File = 17,
        Reference = 18,
        Folder = 19,
        EnumMember = 20,
        Constant = 21,
        Struct = 22,
        Event = 23,
        Operator = 24,
        TypeParameter = 25,
    }
}

int_enum! {
    pub enum InsertTextFormat {
        PlainText = 1,
        Snippet = 2,
    }
}
