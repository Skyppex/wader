//! JSON-RPC 2.0 over a byte stream, as LSP uses it. Knows nothing about LSP.

pub mod codec;
pub mod message;

pub use codec::{read_message, write_message};
pub use message::{ErrorCode, Message, Notification, Request, RequestId, Response, ResponseError};
