//! Everything Rill-specific: running the front end over a document and
//! answering questions about the result.
//!
//! Features work in byte offsets into the text, always on a character
//! boundary; the server converts to and from LSP positions.

pub mod completion;
pub mod convert;
pub mod docs;
pub mod hover;
pub mod index;
pub mod locate;
pub mod moves;
pub mod nav;
pub mod rename;
pub mod render;
pub mod signature;

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use rill::lang::ast::{Def, Program};
use rill::lang::check::{Checked, Global, check, check_partial};
use rill::lang::lexer::{self, Token};
use rill::lang::module::{load_tree, normalize};
use rill::lang::{Diagnostic, SourceMap, Sources, Span, parser};

use crate::document::{Document, Encoding};
use crate::lsp;
use index::Index;

/// What the front end found in one version of a document.
#[derive(Debug)]
pub struct Analysis {
    pub text: String,
    /// Where the document is saved, if it is. Its imports are read from
    /// next to it.
    pub path: Option<PathBuf>,
    /// What the compiler reports, exactly as `rill check` would, for every
    /// file of the program.
    pub diagnostics: Vec<Diagnostic>,
    /// The rest comes from the error-tolerant front end, so it is there
    /// even while the text is broken.
    pub tokens: Vec<Token>,
    pub program: Program,
    pub checked: Checked,
    pub index: Index,
    /// Every file of the program: the document first, at offset 0, then
    /// what it imports, each after the last in the program's spans.
    pub sources: SourceMap,
}

/// Files kept in memory, by path.
#[derive(Debug, Default)]
pub struct InMemory(pub HashMap<PathBuf, String>);

impl Sources for InMemory {
    fn read(&self, path: &Path) -> io::Result<String> {
        self.0
            .get(&normalize(path))
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }
}

impl Analysis {
    /// A document that is not saved anywhere: it cannot import anything.
    pub fn new(text: &str) -> Analysis {
        Analysis::build(None, text, &InMemory::default())
    }

    /// The document saved at `path`, with what it imports read through
    /// `sources`.
    pub fn for_file(path: &Path, text: &str, sources: &dyn Sources) -> Analysis {
        Analysis::build(Some(path), text, sources)
    }

    fn build(path: Option<&Path>, text: &str, sources: &dyn Sources) -> Analysis {
        let diagnostics = match path {
            None => match rill::lang::compile(text) {
                Ok((_, checked)) => checked.warnings,
                Err(diags) => diags,
            },
            Some(path) => {
                let loaded = load_tree(path, Some(text), sources, false);
                let mut found = loaded.diagnostics;
                if !found.iter().any(Diagnostic::is_error) {
                    match check(&loaded.program) {
                        Ok(checked) => found.extend(checked.warnings),
                        Err(diags) => found.extend(diags),
                    }
                }
                found.sort_by_key(|d| d.span.start);
                found
            }
        };
        let (program, sources) = match path {
            None => {
                let (tokens, _) = lexer::lex_partial(text);
                let (program, _) = parser::parse_partial(text, tokens);
                (program, SourceMap::single("", text))
            }
            Some(path) => {
                let loaded = load_tree(path, Some(text), sources, true);
                (loaded.program, loaded.sources)
            }
        };
        let (checked, _) = check_partial(&program);
        let (tokens, _) = lexer::lex_partial(text);
        // Names in every file, so references and renames reach them all.
        let mut all_tokens = tokens.clone();
        for f in sources.files.iter().skip(1) {
            all_tokens.extend(lexer::lex_partial_at(&f.text, f.base).0);
        }
        let index = Index::new(&sources, &all_tokens, &program, &checked);
        Analysis {
            text: text.to_owned(),
            path: path.map(Path::to_owned),
            diagnostics,
            tokens,
            program,
            checked,
            index,
            sources,
        }
    }

    /// Whether `span` is in the document itself, rather than a file it
    /// imports.
    pub fn in_document(&self, span: Span) -> bool {
        span.end as usize <= self.text.len()
    }

    /// The file `span` is in, and `span` within that file's text.
    pub fn locate_span(&self, span: Span) -> Option<(&rill::lang::module::SourceFile, Span)> {
        let f = &self.sources.files[self.sources.file_at(span.start)?];
        Some((
            f,
            Span {
                start: span.start - f.base,
                end: span.end.saturating_sub(f.base),
            },
        ))
    }

    /// The LSP range of `span` in its own file, with the file's path (`None`
    /// for the document itself).
    pub fn file_range(
        &self,
        span: Span,
        encoding: Encoding,
    ) -> Option<(Option<&Path>, lsp::Range)> {
        let (f, local) = self.locate_span(span)?;
        if f.base == 0 {
            return None;
        }
        let doc = Document::new(f.text.clone(), 0, encoding);
        Some((f.path.as_deref(), doc.range(local.start, local.end)))
    }

    /// The doc comment above the declaration starting at `start`, whose
    /// name ends at `end`, in whichever file it is.
    pub fn doc_comment(&self, start: u32, end: u32) -> Option<String> {
        let (f, local) = self.locate_span(Span { start, end })?;
        docs::comment(&f.text, local.start, local.end)
    }

    /// For the name at `offset`, if it stands for something a file exports:
    /// the file that declares it, and where in that file.
    pub fn exported_declaration(&self, offset: u32) -> Option<(PathBuf, Span)> {
        use index::Target;
        let occ = self.index.at(offset)?;
        let exported = match &occ.target {
            Target::Def(i) => self.def(*i).export.is_some(),
            Target::Seq(j) => self.program.seqs[*j].export.is_some(),
            Target::Const(i) => self.program.consts[*i].export.is_some(),
            Target::Event(i) if *i < self.program.events.len() => {
                self.program.events[*i].export.is_some()
            }
            _ => false,
        };
        if !exported {
            return None;
        }
        let decl = self.index.declaration_of(&occ.target)?;
        let (f, local) = self.locate_span(decl.span)?;
        Some((f.path.clone()?, local))
    }

    /// The program-wide offset of `local` in the file at `path`, if this
    /// program includes that file.
    pub fn global_offset(&self, path: &Path, local: u32) -> Option<u32> {
        let disk = rill::lang::Disk;
        let f = self
            .sources
            .files
            .iter()
            .find(|f| f.path.as_deref().is_some_and(|p| disk.canonical(p) == path))?;
        Some(f.base + local)
    }

    /// Whether the document can use `g` by its name: its own, or imported.
    pub fn visible(&self, name: &str, g: Global) -> bool {
        match self.checked.scopes.first() {
            Some(scope) => scope.get(name).contains(&g),
            None => true,
        }
    }

    /// The module that declares item `i`.
    pub fn item_module(&self, i: usize) -> usize {
        self.program.modules.item(i)
    }

    /// The document's diagnostics. Problems in the files it imports are
    /// shown on the `import` that leads to them, with where they are.
    pub fn lsp_diagnostics(&self, doc: &Document) -> Vec<lsp::Diagnostic> {
        let mut out: Vec<lsp::Diagnostic> = self
            .diagnostics
            .iter()
            .filter(|d| self.in_document(d.span))
            .map(|d| convert::diagnostic(doc, d))
            .collect();
        // Per import of the document: the errors in the files reached
        // through it.
        let mut by_import: Vec<(Span, Vec<&Diagnostic>)> = Vec::new();
        for d in self
            .diagnostics
            .iter()
            .filter(|d| d.is_error() && !self.in_document(d.span))
        {
            let Some(file) = self.sources.file_at(d.span.start) else {
                continue;
            };
            let Some(import) = self.import_reaching(file) else {
                continue;
            };
            match by_import.iter_mut().find(|(s, _)| *s == import) {
                Some((_, ds)) => ds.push(d),
                None => by_import.push((import, vec![d])),
            }
        }
        for (import, ds) in by_import {
            let encoding = doc.index.encoding();
            let related = ds
                .iter()
                .filter_map(|d| {
                    let (path, range) = self.file_range(d.span, encoding)?;
                    Some(lsp::DiagnosticRelatedInformation {
                        location: lsp::Location {
                            uri: crate::uri::from_path(path?),
                            range,
                        },
                        message: d.message.clone(),
                    })
                })
                .collect();
            let first = ds[0];
            let (f, _) = self.locate_span(first.span).expect("in a file");
            let line = self
                .file_range(first.span, encoding)
                .map_or(0, |(_, r)| r.start.line + 1);
            let n = ds.len();
            out.push(lsp::Diagnostic {
                range: convert::range(doc, import),
                severity: Some(lsp::DiagnosticSeverity::Error),
                source: Some("rill".into()),
                message: format!(
                    "{} has {n} error{}: {} (line {line})",
                    f.display,
                    if n == 1 { "" } else { "s" },
                    first.message
                ),
                related_information: Some(related),
            });
        }
        out
    }

    /// The path of the document's `import` that `file` is reached through.
    fn import_reaching(&self, file: usize) -> Option<Span> {
        let imports = &self.program.imports;
        let modules = &self.program.modules;
        let targets = |m: usize| {
            (0..imports.len())
                .filter(move |&k| modules.import(k) == m)
                .filter_map(|k| imports[k].module.map(|t| t as usize))
        };
        for k in (0..imports.len()).filter(|&k| modules.import(k) == 0) {
            let Some(start) = imports[k].module.map(|t| t as usize) else {
                continue;
            };
            let mut seen = vec![start];
            let mut i = 0;
            while i < seen.len() {
                if seen[i] == file {
                    return Some(imports[k].path_span);
                }
                for t in targets(seen[i]) {
                    if !seen.contains(&t) {
                        seen.push(t);
                    }
                }
                i += 1;
            }
        }
        None
    }

    /// The text `span` covers, in whichever file it is.
    pub fn slice(&self, span: Span) -> &str {
        self.sources.slice(span)
    }

    pub fn def(&self, index: usize) -> &Def {
        self.program.items[index].def()
    }

    /// The definition whose text contains `offset`.
    pub fn def_at(&self, offset: u32) -> Option<usize> {
        (0..self.program.items.len()).find(|&i| {
            self.item_module(i) == 0
                && self.def(i).span.start <= offset
                && offset <= self.def_end(i)
        })
    }

    /// Where the definition at `index` ends. A body left open (while it is
    /// being typed) runs on to the next definition or the end of the text.
    pub fn def_end(&self, index: usize) -> u32 {
        let span = self.def(index).span;
        if !self.in_document(span) || self.text[..span.end as usize].ends_with('}') {
            return span.end;
        }
        self.program
            .items
            .get(index + 1)
            .map(|next| next.def().span.start)
            .filter(|&start| start as usize <= self.text.len())
            .unwrap_or(self.text.len() as u32)
    }
}
