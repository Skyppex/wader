//! Methods: each is a unit type tying a method name to its params and
//! result types.

use serde::Serialize;
use serde::de::DeserializeOwned;

/// A method that expects a response.
pub trait Request {
    const METHOD: &'static str;
    type Params: Serialize + DeserializeOwned;
    type Result: Serialize + DeserializeOwned;
}

/// A method without a response.
pub trait Notification {
    const METHOD: &'static str;
    type Params: Serialize + DeserializeOwned;
}

macro_rules! methods {
    ($trait:ident: $($name:ident $method:literal ($params:ty) $(-> $result:ty)?;)*) => {
        $(
            #[derive(Debug)]
            pub enum $name {}

            impl super::super::methods::$trait for $name {
                const METHOD: &'static str = $method;
                type Params = $params;
                $(type Result = $result;)?
            }
        )*
    };
}

pub mod request {
    use super::super::*;

    methods! { Request:
        Initialize "initialize" (InitializeParams) -> InitializeResult;
        Shutdown "shutdown" (()) -> ();
        HoverRequest "textDocument/hover" (HoverParams) -> Option<Hover>;
        SignatureHelpRequest "textDocument/signatureHelp" (SignatureHelpParams) -> Option<SignatureHelp>;
        Completion "textDocument/completion" (CompletionParams) -> Option<CompletionList>;
        GotoDefinition "textDocument/definition" (DefinitionParams) -> Option<Vec<Location>>;
        GotoDeclaration "textDocument/declaration" (DeclarationParams) -> Option<Vec<Location>>;
        References "textDocument/references" (ReferenceParams) -> Option<Vec<Location>>;
        PrepareRename "textDocument/prepareRename" (PrepareRenameParams) -> Option<PrepareRenameResult>;
        Rename "textDocument/rename" (RenameParams) -> Option<WorkspaceEdit>;
        WillRenameFiles "workspace/willRenameFiles" (RenameFilesParams) -> Option<WorkspaceEdit>;
    }
}

pub mod notification {
    use super::super::*;

    methods! { Notification:
        Initialized "initialized" (InitializedParams);
        Exit "exit" (());
        Cancel "$/cancelRequest" (CancelParams);
        DidOpenTextDocument "textDocument/didOpen" (DidOpenTextDocumentParams);
        DidChangeTextDocument "textDocument/didChange" (DidChangeTextDocumentParams);
        DidCloseTextDocument "textDocument/didClose" (DidCloseTextDocumentParams);
        DidSaveTextDocument "textDocument/didSave" (DidSaveTextDocumentParams);
        DidRenameFiles "workspace/didRenameFiles" (RenameFilesParams);
        PublishDiagnostics "textDocument/publishDiagnostics" (PublishDiagnosticsParams);
        LogMessage "window/logMessage" (LogMessageParams);
        ShowMessage "window/showMessage" (ShowMessageParams);
    }
}
