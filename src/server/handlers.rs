//! Request and notification handlers: unpack params, find the document,
//! hand off to `analysis`.

use super::dispatch::{HandlerResult, error};
use super::{ClientSupport, Open, Phase, Server};
use crate::analysis::{Analysis, convert};
use crate::document::{Document, Encoding};
use crate::log;
use crate::lsp::*;
use crate::rpc::ErrorCode;
use rill::lang::Span;

impl Server {
    pub(super) fn initialize(
        &mut self,
        params: InitializeParams,
    ) -> HandlerResult<InitializeResult> {
        if self.phase != Phase::Uninitialized {
            return Err(error(ErrorCode::INVALID_REQUEST, "already initialized"));
        }
        let offered = params
            .capabilities
            .general
            .as_ref()
            .and_then(|g| g.position_encodings.as_deref());
        self.encoding = Encoding::negotiate(offered);
        self.client = ClientSupport::from_params(&params);
        self.phase = Phase::Running;

        Ok(InitializeResult {
            capabilities: self.capabilities(),
            server_info: Some(ServerInfo {
                name: "wader".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            position_encoding: Some(self.encoding.kind()),
            text_document_sync: Some(TextDocumentSyncOptions {
                open_close: true,
                change: TextDocumentSyncKind::Full,
            }),
            hover_provider: Some(true),
            // `|>` and `->` both end in `>`, and both have obvious
            // things to follow them.
            completion_provider: Some(CompletionOptions {
                trigger_characters: Some(vec![">".into()]),
                resolve_provider: None,
            }),
            signature_help_provider: Some(SignatureHelpOptions {
                trigger_characters: Some(vec!["(".into(), ",".into()]),
                retrigger_characters: Some(vec![")".into()]),
            }),
            rename_provider: Some(if self.client.prepare_rename {
                RenameProvider::Options(RenameOptions {
                    prepare_provider: Some(true),
                })
            } else {
                RenameProvider::Bool(true)
            }),
            definition_provider: Some(true),
            declaration_provider: Some(true),
            references_provider: Some(true),
        }
    }

    pub(super) fn shutdown(&mut self, (): ()) -> HandlerResult<()> {
        self.phase = Phase::ShuttingDown;
        Ok(())
    }

    pub(super) fn did_open(&mut self, params: DidOpenTextDocumentParams) {
        let item = params.text_document;
        let doc = Document::new(item.text, item.version, self.encoding);
        let analysis = Analysis::new(&doc.text);
        self.docs.insert(item.uri.clone(), Open { doc, analysis });
        self.publish_diagnostics(&item.uri);
    }

    pub(super) fn did_change(&mut self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let Some(open) = self.docs.get_mut(&uri) else {
            log!("change to unopened document {uri}");
            return;
        };
        open.doc
            .apply(params.content_changes, params.text_document.version);
        open.analysis = Analysis::new(&open.doc.text);
        self.publish_diagnostics(&uri);
    }

    pub(super) fn did_close(&mut self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.docs.remove(&uri);
        self.notify::<notification::PublishDiagnostics>(PublishDiagnosticsParams {
            uri,
            version: None,
            diagnostics: Vec::new(),
        });
    }

    fn open(&self, uri: &Uri) -> HandlerResult<&Open> {
        self.docs.get(uri).ok_or_else(|| {
            error(
                ErrorCode::REQUEST_FAILED,
                format!("document {uri} is not open"),
            )
        })
    }

    /// The document and byte offset a position request is about.
    fn at(&self, uri: &Uri, position: Position) -> HandlerResult<(&Open, u32)> {
        let open = self.open(uri)?;
        Ok((open, open.doc.offset(position)))
    }

    fn locations(open: &Open, uri: &Uri, spans: impl IntoIterator<Item = Span>) -> Vec<Location> {
        spans
            .into_iter()
            .map(|s| Location {
                uri: uri.clone(),
                range: convert::range(&open.doc, s),
            })
            .collect()
    }

    pub(super) fn hover(&mut self, p: HoverParams) -> HandlerResult<Option<Hover>> {
        let (open, offset) = self.at(&p.text_document.uri, p.position)?;
        let markdown = self.client.markdown;
        Ok(open.analysis.hover(offset).map(|h| Hover {
            contents: if markdown {
                MarkupContent::markdown(h.contents)
            } else {
                MarkupContent::plain(h.contents)
            },
            range: Some(convert::range(&open.doc, h.span)),
        }))
    }

    pub(super) fn signature_help(
        &mut self,
        p: SignatureHelpParams,
    ) -> HandlerResult<Option<SignatureHelp>> {
        let (open, offset) = self.at(&p.text_document.uri, p.position)?;
        let Some(help) = open.analysis.signature_help(offset) else {
            return Ok(None);
        };
        let client = self.client;
        let encoding = self.encoding;
        let markup = |s: String| {
            if client.markdown {
                MarkupContent::markdown(s)
            } else {
                MarkupContent::plain(s)
            }
        };
        let signatures = help
            .signatures
            .into_iter()
            .map(|sig| {
                let label = sig.rendered.label;
                let parameters = sig
                    .rendered
                    .params
                    .iter()
                    .zip(sig.param_docs)
                    .map(|(&(s, e), doc)| ParameterInformation {
                        label: if client.label_offsets {
                            ParameterLabel::Offsets([
                                encoding.len(&label[..s]),
                                encoding.len(&label[..e]),
                            ])
                        } else {
                            ParameterLabel::Simple(label[s..e].to_owned())
                        },
                        documentation: doc.map(markup),
                    })
                    .collect();
                SignatureInformation {
                    label,
                    documentation: sig.doc.map(markup),
                    parameters: Some(parameters),
                    active_parameter: None,
                }
            })
            .collect();
        Ok(Some(SignatureHelp {
            signatures,
            active_signature: Some(help.active_signature as u32),
            active_parameter: help.active_parameter.map(|p| p as u32),
        }))
    }

    pub(super) fn completion(
        &mut self,
        p: CompletionParams,
    ) -> HandlerResult<Option<CompletionList>> {
        let (open, offset) = self.at(&p.text_document.uri, p.position)?;
        let completions = open.analysis.completions(offset, self.client.snippets);
        let range = convert::range(&open.doc, completions.replace);
        let items = completions
            .items
            .into_iter()
            .map(|mut item| {
                let new_text = item
                    .insert_text
                    .take()
                    .unwrap_or_else(|| item.label.clone());
                item.text_edit = Some(TextEdit { range, new_text });
                item
            })
            .collect();
        Ok(Some(CompletionList {
            is_incomplete: false,
            items,
        }))
    }

    pub(super) fn definition(
        &mut self,
        p: DefinitionParams,
    ) -> HandlerResult<Option<Vec<Location>>> {
        let uri = p.text_document.uri;
        let (open, offset) = self.at(&uri, p.position)?;
        let spans = open.analysis.definition(offset);
        Ok(spans.map(|s| Self::locations(open, &uri, [s])))
    }

    /// Rill has no declarations apart from definitions, so this is the
    /// same as going to the definition.
    pub(super) fn declaration(
        &mut self,
        p: DeclarationParams,
    ) -> HandlerResult<Option<Vec<Location>>> {
        self.definition(p)
    }

    pub(super) fn references(
        &mut self,
        p: ReferenceParams,
    ) -> HandlerResult<Option<Vec<Location>>> {
        let uri = p.text_document.uri;
        let (open, offset) = self.at(&uri, p.position)?;
        let spans = open
            .analysis
            .references(offset, p.context.include_declaration);
        Ok(Some(Self::locations(open, &uri, spans)))
    }

    pub(super) fn prepare_rename(
        &mut self,
        p: PrepareRenameParams,
    ) -> HandlerResult<Option<PrepareRenameResult>> {
        let (open, offset) = self.at(&p.text_document.uri, p.position)?;
        let prepared = open.analysis.prepare_rename(offset).map_err(rename_error)?;
        Ok(prepared.map(
            |(span, placeholder)| PrepareRenameResult::RangeWithPlaceholder {
                range: convert::range(&open.doc, span),
                placeholder,
            },
        ))
    }

    pub(super) fn rename(&mut self, p: RenameParams) -> HandlerResult<Option<WorkspaceEdit>> {
        let uri = p.text_document.uri;
        let (open, offset) = self.at(&uri, p.position)?;
        let edits = open
            .analysis
            .rename(offset, &p.new_name)
            .map_err(rename_error)?
            .into_iter()
            .map(|(span, new_text)| TextEdit {
                range: convert::range(&open.doc, span),
                new_text,
            })
            .collect();
        Ok(Some(WorkspaceEdit {
            changes: Some([(uri, edits)].into_iter().collect()),
        }))
    }
}

fn rename_error(e: crate::analysis::rename::RenameError) -> crate::rpc::ResponseError {
    error(ErrorCode::REQUEST_FAILED, e.0)
}
