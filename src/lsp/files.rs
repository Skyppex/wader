//! `workspace/willRenameFiles` and `workspace/didRenameFiles`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameFilesParams {
    pub files: Vec<FileRename>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRename {
    pub old_uri: String,
    pub new_uri: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceServerCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_operations: Option<FileOperationsServerCapabilities>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOperationsServerCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub will_rename: Option<FileOperationRegistrationOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_rename: Option<FileOperationRegistrationOptions>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOperationRegistrationOptions {
    pub filters: Vec<FileOperationFilter>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOperationFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    pub pattern: FileOperationPattern,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOperationPattern {
    pub glob: String,
    /// `"file"` or `"folder"`; both when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matches: Option<String>,
}
