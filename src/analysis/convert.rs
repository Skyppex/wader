//! Rill front-end values -> LSP values.

use rill::lang::{Diagnostic, Severity, Span};

use crate::document::Document;
use crate::lsp;

pub fn range(doc: &Document, span: Span) -> lsp::Range {
    doc.range(span.start, span.end)
}

pub fn diagnostic(doc: &Document, d: &Diagnostic) -> lsp::Diagnostic {
    let mut message = d.message.clone();
    // `help` has no location of its own, so it can't be related information.
    if let Some(help) = &d.help {
        message.push_str("\nhelp: ");
        message.push_str(help);
    }
    lsp::Diagnostic {
        // A file-level diagnostic has the default span, which is 0:0.
        range: range(doc, d.span),
        severity: Some(match d.severity {
            Severity::Error => lsp::DiagnosticSeverity::Error,
            Severity::Warning => lsp::DiagnosticSeverity::Warning,
        }),
        source: Some("rill".into()),
        message,
        related_information: None,
    }
}
