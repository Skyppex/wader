//! JSON-RPC 2.0 messages.
//!
//! Params and results stay as raw [`Value`]s here; turning them into typed
//! protocol models is the caller's business.

use std::fmt;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// The id of a request. The spec allows a number or a string.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Int(i64),
    Str(String),
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RequestId::Int(n) => write!(f, "{n}"),
            RequestId::Str(s) => write!(f, "{s:?}"),
        }
    }
}

impl From<i64> for RequestId {
    fn from(n: i64) -> RequestId {
        RequestId::Int(n)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub id: RequestId,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub method: String,
    pub params: Option<Value>,
}

/// A reply to a request. Exactly one of `result` and `error` is set.
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    /// `None` only when the request's id could not be read.
    pub id: Option<RequestId>,
    pub result: Option<Value>,
    pub error: Option<ResponseError>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Error codes from JSON-RPC and the LSP spec.
pub struct ErrorCode;

impl ErrorCode {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
    pub const SERVER_NOT_INITIALIZED: i64 = -32002;
    pub const REQUEST_CANCELLED: i64 = -32800;
    pub const REQUEST_FAILED: i64 = -32803;
}

impl Response {
    pub fn ok(id: RequestId, result: impl Serialize) -> Response {
        // Serializing our own protocol models cannot fail.
        let result = serde_json::to_value(result).expect("result serializes");
        Response {
            id: Some(id),
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Option<RequestId>, code: i64, message: impl Into<String>) -> Response {
        Response {
            id,
            result: None,
            error: Some(ResponseError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Request(Request),
    Response(Response),
    Notification(Notification),
}

impl From<Request> for Message {
    fn from(r: Request) -> Message {
        Message::Request(r)
    }
}

impl From<Response> for Message {
    fn from(r: Response) -> Message {
        Message::Response(r)
    }
}

impl From<Notification> for Message {
    fn from(n: Notification) -> Message {
        Message::Notification(n)
    }
}

/// Every field any message can have. Which ones are present decides the kind.
#[derive(Serialize, Deserialize)]
struct Raw {
    jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
    /// Present-but-null must stay `Some(Null)`: it is a successful response.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<ResponseError>,
}

fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

impl Serialize for Message {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let id = |id: &RequestId| serde_json::to_value(id).expect("id serializes");
        let raw = match self.clone() {
            Message::Request(r) => Raw {
                jsonrpc: "2.0".into(),
                id: Some(id(&r.id)),
                method: Some(r.method),
                params: r.params,
                result: None,
                error: None,
            },
            Message::Notification(n) => Raw {
                jsonrpc: "2.0".into(),
                id: None,
                method: Some(n.method),
                params: n.params,
                result: None,
                error: None,
            },
            Message::Response(r) => Raw {
                jsonrpc: "2.0".into(),
                // A response always has an id, `null` when it is unknown.
                id: Some(r.id.as_ref().map_or(Value::Null, id)),
                method: None,
                params: None,
                // A successful response must have `result`, even if null.
                result: if r.error.is_none() {
                    Some(r.result.unwrap_or(Value::Null))
                } else {
                    None
                },
                error: r.error,
            },
        };
        raw.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Message {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Message, D::Error> {
        let raw = Raw::deserialize(d)?;
        if raw.jsonrpc != "2.0" {
            return Err(D::Error::custom(format!(
                "unsupported jsonrpc version {:?}",
                raw.jsonrpc
            )));
        }
        let id = match raw.id {
            None | Some(Value::Null) => None,
            Some(v) => Some(RequestId::deserialize(v).map_err(D::Error::custom)?),
        };
        Ok(match (raw.method, id) {
            (Some(method), Some(id)) => Message::Request(Request {
                id,
                method,
                params: raw.params,
            }),
            (Some(method), None) => Message::Notification(Notification {
                method,
                params: raw.params,
            }),
            (None, id) => {
                if raw.result.is_none() && raw.error.is_none() {
                    return Err(D::Error::custom("message has no method, result or error"));
                }
                Message::Response(Response {
                    id,
                    result: raw.result,
                    error: raw.error,
                })
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn round_trip(m: Message) {
        let text = serde_json::to_string(&m).unwrap();
        let back: Message = serde_json::from_str(&text).unwrap();
        assert_eq!(back, m, "{text}");
    }

    #[test]
    fn kinds_round_trip() {
        round_trip(
            Request {
                id: 1.into(),
                method: "initialize".into(),
                params: Some(json!({"a": 1})),
            }
            .into(),
        );
        round_trip(
            Request {
                id: RequestId::Str("x".into()),
                method: "shutdown".into(),
                params: None,
            }
            .into(),
        );
        round_trip(
            Notification {
                method: "exit".into(),
                params: None,
            }
            .into(),
        );
        round_trip(Response::ok(3.into(), json!({"ok": true})).into());
        round_trip(Response::ok(5.into(), Value::Null).into());
        round_trip(Response::err(Some(4.into()), ErrorCode::METHOD_NOT_FOUND, "nope").into());
    }

    #[test]
    fn null_result_is_still_written() {
        let m: Message = Response::ok(1.into(), Value::Null).into();
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            json!({"jsonrpc": "2.0", "id": 1, "result": null})
        );
    }

    #[test]
    fn unknown_id_is_null() {
        let m: Message = Response::err(None, ErrorCode::PARSE_ERROR, "bad").into();
        let v = serde_json::to_value(&m).unwrap();
        assert_eq!(v["id"], Value::Null);
    }

    #[test]
    fn rejects_garbage() {
        assert!(serde_json::from_str::<Message>(r#"{"jsonrpc":"2.0"}"#).is_err());
        assert!(serde_json::from_str::<Message>(r#"{"jsonrpc":"1.0","method":"x"}"#).is_err());
    }
}
