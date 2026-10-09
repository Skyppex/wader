//! Method name -> typed handler.

use std::panic::{AssertUnwindSafe, catch_unwind};

use serde_json::Value;

use super::{Phase, Server};
use crate::log;
use crate::lsp::{self, notification as n, request as r};
use crate::rpc::{ErrorCode, Notification, Request, Response, ResponseError};

pub type HandlerResult<T> = Result<T, ResponseError>;

pub fn error(code: i64, message: impl Into<String>) -> ResponseError {
    ResponseError {
        code,
        message: message.into(),
        data: None,
    }
}

fn params<P: serde::de::DeserializeOwned>(params: Option<Value>) -> HandlerResult<P> {
    serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|e| error(ErrorCode::INVALID_PARAMS, format!("invalid params: {e}")))
}

/// The message of a caught panic.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

impl Server {
    fn call<R: lsp::Request>(
        &mut self,
        raw: Option<Value>,
        handler: fn(&mut Server, R::Params) -> HandlerResult<R::Result>,
    ) -> HandlerResult<Value> {
        let p = params::<R::Params>(raw)?;
        let result = handler(self, p)?;
        Ok(serde_json::to_value(result).expect("result serializes"))
    }

    fn call_notification<N: lsp::Notification>(
        &mut self,
        raw: Option<Value>,
        handler: fn(&mut Server, N::Params),
    ) {
        match params::<N::Params>(raw) {
            Ok(p) => handler(self, p),
            Err(e) => log!("{}: {}", N::METHOD, e.message),
        }
    }

    pub(super) fn on_request(&mut self, req: Request) -> Response {
        let Request { id, method, params } = req;
        match self.phase {
            Phase::Uninitialized if method != <r::Initialize as lsp::Request>::METHOD => {
                return Response::err(
                    Some(id),
                    ErrorCode::SERVER_NOT_INITIALIZED,
                    "server not initialized",
                );
            }
            Phase::ShuttingDown => {
                return Response::err(
                    Some(id),
                    ErrorCode::INVALID_REQUEST,
                    "server is shutting down",
                );
            }
            _ => {}
        }

        let result = catch_unwind(AssertUnwindSafe(|| self.route_request(&method, params)));
        match result {
            Ok(Ok(value)) => Response {
                id: Some(id),
                result: Some(value),
                error: None,
            },
            Ok(Err(e)) => Response {
                id: Some(id),
                result: None,
                error: Some(e),
            },
            Err(payload) => {
                let msg = panic_message(&*payload);
                log!("{method} panicked: {msg}");
                Response::err(
                    Some(id),
                    ErrorCode::INTERNAL_ERROR,
                    format!("internal error: {msg}"),
                )
            }
        }
    }

    fn route_request(&mut self, method: &str, p: Option<Value>) -> HandlerResult<Value> {
        use lsp::Request as R;
        match method {
            r::Initialize::METHOD => self.call::<r::Initialize>(p, Server::initialize),
            r::Shutdown::METHOD => self.call::<r::Shutdown>(p, Server::shutdown),
            r::HoverRequest::METHOD => self.call::<r::HoverRequest>(p, Server::hover),
            r::SignatureHelpRequest::METHOD => {
                self.call::<r::SignatureHelpRequest>(p, Server::signature_help)
            }
            r::Completion::METHOD => self.call::<r::Completion>(p, Server::completion),
            r::GotoDefinition::METHOD => self.call::<r::GotoDefinition>(p, Server::definition),
            r::GotoDeclaration::METHOD => self.call::<r::GotoDeclaration>(p, Server::declaration),
            r::References::METHOD => self.call::<r::References>(p, Server::references),
            r::PrepareRename::METHOD => self.call::<r::PrepareRename>(p, Server::prepare_rename),
            r::Rename::METHOD => self.call::<r::Rename>(p, Server::rename),
            r::WillRenameFiles::METHOD => {
                self.call::<r::WillRenameFiles>(p, Server::will_rename_files)
            }
            _ => Err(error(
                ErrorCode::METHOD_NOT_FOUND,
                format!("unknown method `{method}`"),
            )),
        }
    }

    pub(super) fn on_notification(&mut self, note: Notification) {
        use lsp::Notification as N;
        let Notification { method, params: p } = note;
        if method == n::Exit::METHOD {
            self.exit = Some(if self.phase == Phase::ShuttingDown {
                0
            } else {
                1
            });
            return;
        }
        if self.phase != Phase::Running {
            // Before `initialize`, notifications are dropped; after
            // `shutdown`, only `exit` matters.
            return;
        }
        let result = catch_unwind(AssertUnwindSafe(|| match method.as_str() {
            n::Initialized::METHOD => {}
            n::DidOpenTextDocument::METHOD => {
                self.call_notification::<n::DidOpenTextDocument>(p, Server::did_open)
            }
            n::DidChangeTextDocument::METHOD => {
                self.call_notification::<n::DidChangeTextDocument>(p, Server::did_change)
            }
            n::DidCloseTextDocument::METHOD => {
                self.call_notification::<n::DidCloseTextDocument>(p, Server::did_close)
            }
            n::DidSaveTextDocument::METHOD => {}
            n::DidRenameFiles::METHOD => {
                self.call_notification::<n::DidRenameFiles>(p, Server::did_rename_files)
            }
            // Every request is answered before the next is read, so there
            // is never anything to cancel.
            n::Cancel::METHOD => {}
            m if m.starts_with("$/") => {}
            m => log!("ignoring notification `{m}`"),
        }));
        if let Err(payload) = result {
            log!("{method} panicked: {}", panic_message(&*payload));
        }
    }
}
