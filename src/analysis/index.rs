//! Every name in a document and what it refers to.

use rill::lang::Span;
use rill::lang::ast::Program;
use rill::lang::check::{BindingId, Checked, Resolution};
use rill::lang::lexer::{Token, TokenKind};

/// What a name refers to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// A fn or rill, by index into the program's items.
    Def(usize),
    Binding(BindingId),
    Builtin(String),
    Constant(String),
    /// A note name like `F#4`.
    Note(String),
    /// A built-in type like `Sample`.
    Type(String),
    /// A declared event, by index into the program's event declarations.
    Event(usize),
    /// An event kind like `note_on`, in a declaration.
    EventKind(String),
    /// A sequence, by index into the program's sequences.
    Seq(usize),
    /// A top-level `const`, by index into the program's `const`s.
    Const(usize),
}

impl Target {
    /// Declared in the document, so it can be renamed and jumped to.
    pub fn is_user(&self) -> bool {
        matches!(
            self,
            Target::Def(_)
                | Target::Binding(_)
                | Target::Event(_)
                | Target::Seq(_)
                | Target::Const(_)
        )
    }
}

/// One appearance of a name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occurrence {
    /// Exactly the name.
    pub span: Span,
    pub target: Target,
    /// Where it is declared, rather than used.
    pub is_decl: bool,
}

#[derive(Debug, Default)]
pub struct Index {
    /// Sorted by position.
    occurrences: Vec<Occurrence>,
}

impl Index {
    pub fn new(text: &str, tokens: &[Token], program: &Program, checked: &Checked) -> Index {
        let mut occurrences = Vec::new();
        for (i, item) in program.items.iter().enumerate() {
            occurrences.push(Occurrence {
                span: item.def().name.span,
                target: Target::Def(i),
                is_decl: true,
            });
        }
        for (i, e) in program.events.iter().enumerate() {
            occurrences.push(Occurrence {
                span: e.name.span,
                target: Target::Event(i),
                is_decl: true,
            });
            if checked.events.get(i).is_some_and(Option::is_some) {
                occurrences.push(Occurrence {
                    span: e.kind.span,
                    target: Target::EventKind(e.kind.name.clone()),
                    is_decl: false,
                });
            }
        }
        for (i, s) in program.seqs.iter().enumerate() {
            occurrences.push(Occurrence {
                span: s.name.span,
                target: Target::Seq(i),
                is_decl: true,
            });
        }
        for (i, c) in program.consts.iter().enumerate() {
            occurrences.push(Occurrence {
                span: c.name.span,
                target: Target::Const(i),
                is_decl: true,
            });
        }
        for (id, b) in checked.bindings.iter().enumerate() {
            occurrences.push(Occurrence {
                span: b.span,
                target: Target::Binding(id),
                is_decl: true,
            });
        }
        for (span, r) in &checked.resolutions {
            // A name in parentheses has the span of the parentheses; the
            // name is the one identifier inside.
            let Some(span) = name_in(tokens, *span) else {
                continue;
            };
            let name = || text[span.start as usize..span.end as usize].to_owned();
            let target = match r {
                Resolution::Binding(id) => Target::Binding(*id),
                Resolution::Def(i) => Target::Def(*i),
                Resolution::Builtin(n) => Target::Builtin(n.clone()),
                Resolution::Constant(n) => Target::Constant(n.clone()),
                Resolution::Note => Target::Note(name()),
                Resolution::Type => Target::Type(name()),
                Resolution::Event(i) => Target::Event(*i),
                Resolution::Seq(i) => Target::Seq(*i),
                Resolution::Const(i) => Target::Const(*i),
            };
            occurrences.push(Occurrence {
                span,
                target,
                is_decl: false,
            });
        }
        occurrences.sort_by_key(|o| (o.span.start, !o.is_decl));
        // The checker can visit a name twice (a default checked as part of
        // its signature and again on its own); keep one of each.
        occurrences.dedup_by(|a, b| a.span == b.span);
        Index { occurrences }
    }

    /// The name at `offset`, including just after its last character.
    pub fn at(&self, offset: u32) -> Option<&Occurrence> {
        let after = self.occurrences.partition_point(|o| o.span.start <= offset);
        self.occurrences[..after]
            .iter()
            .rev()
            .take(2)
            .find(|o| o.span.end >= offset)
    }

    /// Every appearance of `target`, in order.
    pub fn occurrences_of<'a>(
        &'a self,
        target: &'a Target,
    ) -> impl Iterator<Item = &'a Occurrence> {
        self.occurrences.iter().filter(move |o| o.target == *target)
    }

    pub fn declaration_of(&self, target: &Target) -> Option<&Occurrence> {
        self.occurrences
            .iter()
            .find(|o| o.is_decl && o.target == *target)
    }

    pub fn all(&self) -> &[Occurrence] {
        &self.occurrences
    }
}

/// The span of the first identifier token inside `span`.
fn name_in(tokens: &[Token], span: Span) -> Option<Span> {
    let from = tokens.partition_point(|t| t.span.start < span.start);
    tokens[from..]
        .iter()
        .take_while(|t| t.span.end <= span.end)
        .find(|t| t.kind == TokenKind::Ident)
        .map(|t| t.span)
}
