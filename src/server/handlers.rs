//! Request and notification handlers: unpack params, find the document,
//! hand off to `analysis`.

use super::dispatch::{HandlerResult, error};
use super::{ClientSupport, Open, Phase, Server};
use crate::analysis::convert;
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
        self.root = params.root_uri.as_ref().and_then(crate::uri::to_path);
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
            // After `|>` there is an obvious thing to follow: a function.
            completion_provider: Some(CompletionOptions {
                // And inside an import's quotes, a file or folder.
                trigger_characters: Some(vec![">".into(), "\"".into(), "/".into()]),
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
            // Moving or renaming a file keeps the imports of and to it working.
            workspace: Some(WorkspaceServerCapabilities {
                file_operations: Some(FileOperationsServerCapabilities {
                    will_rename: Some(rill_files_and_folders()),
                    did_rename: Some(rill_files_and_folders()),
                }),
            }),
        }
    }

    /// Before files or folders move: change every import that would stop
    /// pointing at its file, in the files that move and the files that
    /// import them. The edits are for the files as they are named now.
    pub(super) fn will_rename_files(
        &mut self,
        p: RenameFilesParams,
    ) -> HandlerResult<Option<WorkspaceEdit>> {
        use crate::analysis::moves::{Move, import_edits};
        let moves: Vec<Move> = p
            .files
            .iter()
            .filter_map(|f| {
                Some(Move {
                    from: crate::uri::to_path(&Uri::from(f.old_uri.as_str()))?,
                    to: crate::uri::to_path(&Uri::from(f.new_uri.as_str()))?,
                })
            })
            .collect();
        if moves.is_empty() {
            return Ok(None);
        }
        // Files anywhere in the workspace can import what moves.
        let mut roots: Vec<std::path::PathBuf> = self.root.iter().cloned().collect();
        if roots.is_empty() {
            roots.extend(
                moves
                    .iter()
                    .filter_map(|m| m.from.parent().map(|p| p.to_owned())),
            );
        }
        let mut files: Vec<std::path::PathBuf> = roots.iter().flat_map(|r| rill_files(r)).collect();
        files.extend(self.docs.keys().filter_map(crate::uri::to_path));
        files.sort();
        files.dedup();
        let mut changes = std::collections::BTreeMap::new();
        for path in files {
            let uri = crate::uri::from_path(&path);
            let text = match self.docs.get(&uri) {
                Some(open) => open.doc.text.clone(),
                None => match std::fs::read_to_string(&path) {
                    Ok(t) => t,
                    Err(_) => continue,
                },
            };
            let edits = import_edits(&path, &text, &moves);
            if edits.is_empty() {
                continue;
            }
            let doc = Document::new(text, 0, self.encoding);
            let edits = edits
                .into_iter()
                .map(|(span, new_text)| TextEdit {
                    range: convert::range(&doc, span),
                    new_text,
                })
                .collect();
            changes.insert(uri, edits);
        }
        Ok((!changes.is_empty()).then_some(WorkspaceEdit {
            changes: Some(changes),
        }))
    }

    /// After files moved: what every open document imports may be
    /// somewhere else now.
    pub(super) fn did_rename_files(&mut self, _: RenameFilesParams) {
        let uris: Vec<Uri> = self.docs.keys().cloned().collect();
        for uri in uris {
            let text = self.docs[&uri].doc.text.clone();
            let analysis = self.analyze(&uri, &text);
            if let Some(open) = self.docs.get_mut(&uri) {
                open.analysis = analysis;
            }
            self.publish_diagnostics(&uri);
        }
    }

    pub(super) fn shutdown(&mut self, (): ()) -> HandlerResult<()> {
        self.phase = Phase::ShuttingDown;
        Ok(())
    }

    pub(super) fn did_open(&mut self, params: DidOpenTextDocumentParams) {
        let item = params.text_document;
        let doc = Document::new(item.text, item.version, self.encoding);
        let analysis = self.analyze(&item.uri, &doc.text);
        self.docs.insert(item.uri.clone(), Open { doc, analysis });
        self.publish_diagnostics(&item.uri);
        self.refresh_importers(&item.uri);
    }

    pub(super) fn did_change(&mut self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let Some(open) = self.docs.get_mut(&uri) else {
            log!("change to unopened document {uri}");
            return;
        };
        open.doc
            .apply(params.content_changes, params.text_document.version);
        let text = open.doc.text.clone();
        let analysis = self.analyze(&uri, &text);
        if let Some(open) = self.docs.get_mut(&uri) {
            open.analysis = analysis;
        }
        self.publish_diagnostics(&uri);
        self.refresh_importers(&uri);
    }

    pub(super) fn did_close(&mut self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.docs.remove(&uri);
        self.notify::<notification::PublishDiagnostics>(PublishDiagnosticsParams {
            uri: uri.clone(),
            version: None,
            diagnostics: Vec::new(),
        });
        // Files importing it now read it from the disk.
        self.refresh_importers(&uri);
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

    /// Where `span` is: in the document at `uri`, or in a file it imports.
    fn location(&self, open: &Open, uri: &Uri, span: Span) -> Option<Location> {
        match open.analysis.file_range(span, self.encoding) {
            None => Some(Location {
                uri: uri.clone(),
                range: convert::range(&open.doc, span),
            }),
            Some((path, range)) => Some(Location {
                uri: crate::uri::from_path(path?),
                range,
            }),
        }
    }

    fn locations(
        &self,
        open: &Open,
        uri: &Uri,
        spans: impl IntoIterator<Item = Span>,
    ) -> Vec<Location> {
        spans
            .into_iter()
            .filter_map(|s| self.location(open, uri, s))
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
        Ok(spans.map(|s| self.locations(open, &uri, [s])))
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
        Ok(Some(self.locations(open, &uri, spans)))
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
        let mut changes: std::collections::HashMap<Uri, Vec<TextEdit>> =
            std::collections::HashMap::new();
        let add = |at: Location,
                   new_text: String,
                   changes: &mut std::collections::HashMap<Uri, Vec<TextEdit>>| {
            let edits = changes.entry(at.uri).or_default();
            if !edits.iter().any(|e| e.range == at.range) {
                edits.push(TextEdit {
                    range: at.range,
                    new_text,
                });
            }
        };
        for (span, new_text) in open
            .analysis
            .rename(offset, &p.new_name)
            .map_err(rename_error)?
        {
            if let Some(at) = self.location(open, &uri, span) {
                add(at, new_text, &mut changes);
            }
        }
        // Something a file exports can be used by files this one does not
        // reach: every file under the workspace that imports it.
        if let Some((decl_path, local)) = open.analysis.exported_declaration(offset) {
            let decl_path = rill::lang::Sources::canonical(&rill::lang::Disk, &decl_path);
            let here = crate::uri::to_path(&uri);
            let root = self
                .root
                .clone()
                .or_else(|| decl_path.parent().map(|p| p.to_owned()));
            let old_name = open
                .analysis
                .slice(local_name_span(open, offset))
                .to_owned();
            for path in root.map(|r| rill_files(&r)).unwrap_or_default() {
                if here.as_deref() == Some(path.as_path()) {
                    continue;
                }
                let other_uri = crate::uri::from_path(&path);
                let text = match self.docs.get(&other_uri) {
                    Some(o) => o.doc.text.clone(),
                    None => match std::fs::read_to_string(&path) {
                        Ok(t) => t,
                        Err(_) => continue,
                    },
                };
                if !text.contains(&old_name) {
                    continue;
                }
                let a = self.analyze(&other_uri, &text);
                let Some(at) = a.global_offset(&decl_path, local.start) else {
                    continue;
                };
                let Ok(edits) = a.rename(at, &p.new_name) else {
                    continue;
                };
                let doc = Document::new(text.clone(), 0, self.encoding);
                for (span, new_text) in edits {
                    let location = match a.file_range(span, self.encoding) {
                        None => Location {
                            uri: other_uri.clone(),
                            range: convert::range(&doc, span),
                        },
                        Some((Some(p), range)) => Location {
                            uri: crate::uri::from_path(p),
                            range,
                        },
                        Some((None, _)) => continue,
                    };
                    add(location, new_text, &mut changes);
                }
            }
        }
        Ok(Some(WorkspaceEdit {
            changes: Some(changes.into_iter().collect()),
        }))
    }
}

/// Rill files and any folder, which can hold them.
fn rill_files_and_folders() -> FileOperationRegistrationOptions {
    let filter = |glob: &str, matches: &str| FileOperationFilter {
        scheme: Some("file".into()),
        pattern: FileOperationPattern {
            glob: glob.into(),
            matches: Some(matches.into()),
        },
    };
    FileOperationRegistrationOptions {
        filters: vec![filter("**/*.rill", "file"), filter("**", "folder")],
    }
}

/// The span of the name at `offset`.
fn local_name_span(open: &Open, offset: u32) -> Span {
    open.analysis
        .index
        .at(offset)
        .map_or(Span::default(), |o| o.span)
}

/// Every `.rill` file under `dir`, skipping hidden folders and build output.
fn rill_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if !name.starts_with('.') && name != "target" && name != "node_modules" {
                    stack.push(path);
                }
            } else if name.ends_with(".rill") {
                out.push(path);
            }
            if out.len() > 5000 {
                return out;
            }
        }
    }
    out
}

fn rename_error(e: crate::analysis::rename::RenameError) -> crate::rpc::ResponseError {
    error(ErrorCode::REQUEST_FAILED, e.0)
}
