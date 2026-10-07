//! Messages for the user and the client's log.

use serde::{Deserialize, Serialize};

int_enum! {
    pub enum MessageType {
        Error = 1,
        Warning = 2,
        Info = 3,
        Log = 4,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogMessageParams {
    #[serde(rename = "type")]
    pub typ: MessageType,
    pub message: String,
}

pub type ShowMessageParams = LogMessageParams;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelParams {
    pub id: serde_json::Value,
}
