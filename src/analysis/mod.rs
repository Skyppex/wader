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
pub mod nav;
pub mod render;
pub mod rename;
pub mod signature;

use rill::lang::ast::{Def, Program};
use rill::lang::check::{Checked, check_partial};
use rill::lang::lexer::{self, Token};
use rill::lang::{Diagnostic, Span, parser};

use crate::document::Document;
use crate::lsp;
use index::Index;

/// What the front end found in one version of a document.
#[derive(Debug)]
pub struct Analysis {
    pub text: String,
    /// What the compiler reports, exactly as `rill check` would.
    pub diagnostics: Vec<Diagnostic>,
    /// The rest comes from the error-tolerant front end, so it is there
    /// even while the text is broken.
    pub tokens: Vec<Token>,
    pub program: Program,
    pub checked: Checked,
    pub index: Index,
}

impl Analysis {
    pub fn new(text: &str) -> Analysis {
        let diagnostics = match rill::lang::compile(text) {
            Ok((_, checked)) => checked.warnings,
            Err(diags) => diags,
        };
        let (tokens, _) = lexer::lex_partial(text);
        let (program, _) = parser::parse_partial(text, tokens.clone());
        let (checked, _) = check_partial(&program);
        let index = Index::new(text, &tokens, &program, &checked);
        Analysis {
            text: text.to_owned(),
            diagnostics,
            tokens,
            program,
            checked,
            index,
        }
    }

    pub fn lsp_diagnostics(&self, doc: &Document) -> Vec<lsp::Diagnostic> {
        self.diagnostics
            .iter()
            .map(|d| convert::diagnostic(doc, d))
            .collect()
    }

    pub fn slice(&self, span: Span) -> &str {
        &self.text[span.start as usize..span.end as usize]
    }

    pub fn def(&self, index: usize) -> &Def {
        self.program.items[index].def()
    }

    /// The definition whose text contains `offset`.
    pub fn def_at(&self, offset: u32) -> Option<usize> {
        (0..self.program.items.len())
            .find(|&i| self.def(i).span.start <= offset && offset <= self.def_end(i))
    }

    /// Where the definition at `index` ends. A body left open (while it is
    /// being typed) runs on to the next definition or the end of the text.
    pub fn def_end(&self, index: usize) -> u32 {
        let span = self.def(index).span;
        if self.text[..span.end as usize].ends_with('}') {
            return span.end;
        }
        self.program
            .items
            .get(index + 1)
            .map_or(self.text.len() as u32, |next| next.def().span.start)
    }
}
