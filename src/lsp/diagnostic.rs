//! `textDocument/publishDiagnostics`.

use serde::{Deserialize, Serialize};

use super::base::{Location, Range, Uri};

int_enum! {
    pub enum DiagnosticSeverity {
        Error = 1,
        Warning = 2,
        Information = 3,
        Hint = 4,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub range: Range,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<DiagnosticSeverity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_information: Option<Vec<DiagnosticRelatedInformation>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticRelatedInformation {
    pub location: Location,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishDiagnosticsParams {
    pub uri: Uri,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<i32>,
    pub diagnostics: Vec<Diagnostic>,
}
