//! Completion: what could go at the cursor.
//!
//! The context comes from the tokens before the cursor rather than the
//! syntax tree, since the statement being typed rarely parses.

use std::collections::HashSet;

use rill::lang::Span;
use rill::lang::ast::ExprKind;
use rill::lang::builtins;
use rill::lang::check::{Binding, BindingKind};
use rill::lang::lexer::{Token, TokenKind, Unit};
use rill::lang::types::{DefKind, Type, coerces};

use super::Analysis;
use super::locate::walk_block;
use super::render::{self, keyword_doc, type_doc, unit_doc};
use crate::lsp::{CompletionItem, CompletionItemKind as Kind, InsertTextFormat, MarkupContent};

/// What completing at a position would offer, and the text it replaces.
#[derive(Debug)]
pub struct Completions {
    /// The partial word before the cursor, replaced by the chosen item.
    pub replace: Span,
    pub items: Vec<CompletionItem>,
}

/// Where the cursor is, as far as completion cares.
#[derive(Debug, PartialEq)]
enum Context {
    /// Nothing makes sense here: a comment, or a name being declared.
    Nothing,
    /// Between definitions.
    TopLevel,
    Type,
    /// The unit of a number literal.
    Unit,
    /// After `event NAME`: the kind.
    EventKind,
    /// Inside an event declaration's parentheses: a filter name.
    EventFilter,
    /// After `on`: a declared event.
    EventName,
    /// After `|>`: something to call, with the type of what is piped in.
    Pipe(Option<Type>),
    Expr {
        /// At the start of a statement, where `let` and friends go.
        stmt_start: bool,
    },
}

pub const TYPES: [&str; 9] = [
    "Sample", "Float", "Int", "Bool", "Freq", "Time", "Pitch", "Interval", "Gain",
];

fn item(
    label: &str,
    kind: Kind,
    detail: Option<String>,
    doc: Option<String>,
    sort: u8,
) -> CompletionItem {
    CompletionItem {
        label: label.to_owned(),
        kind: Some(kind),
        detail,
        documentation: doc.map(MarkupContent::markdown),
        sort_text: Some(format!("{sort}{label}")),
        ..Default::default()
    }
}

fn is_word(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Ident
            | TokenKind::Fn
            | TokenKind::Rill
            | TokenKind::State
            | TokenKind::Let
            | TokenKind::Return
            | TokenKind::If
            | TokenKind::Else
            | TokenKind::True
            | TokenKind::False
            | TokenKind::As
    )
}

impl Analysis {
    pub fn completions(&self, offset: u32, snippets: bool) -> Completions {
        let (replace, context) = self.completion_context(offset);
        let mut items = match context {
            Context::Nothing => Vec::new(),
            Context::TopLevel => self.keyword_items(&["fn", "rill", "event"], snippets),
            Context::EventKind => event_kind_items(),
            Context::EventFilter => ["sender", "channel"]
                .iter()
                .map(|f| item(f, Kind::Property, Some("filter".into()), None, 0))
                .collect(),
            Context::EventName => self.event_name_items(),
            Context::Type => self.type_items(offset),
            Context::Unit => unit_items(),
            Context::Pipe(piped) => self.callable_items(offset, piped.as_ref()),
            Context::Expr { stmt_start } => {
                let mut items = self.named_arg_items(offset);
                items.extend(self.value_items(offset));
                items.extend(self.callable_items(offset, None));
                let mut keywords = vec!["if", "fn", "true", "false"];
                if stmt_start {
                    keywords.extend(["let", "return"]);
                    if self.in_rill_body(offset) {
                        keywords.extend(["state", "on"]);
                    }
                }
                items.extend(self.keyword_items(&keywords, snippets));
                items
            }
        };
        // One entry per label: a local hides a definition of the same name.
        let mut seen = HashSet::new();
        items.retain(|i| seen.insert(i.label.clone()));
        Completions { replace, items }
    }

    fn completion_context(&self, offset: u32) -> (Span, Context) {
        let here = Span {
            start: offset,
            end: offset,
        };
        if self.in_comment(offset) {
            return (here, Context::Nothing);
        }
        // The token being typed, if the cursor is in or right after one.
        let i = self.tokens.partition_point(|t| t.span.start < offset);
        let current = i
            .checked_sub(1)
            .map(|j| &self.tokens[j])
            .filter(|t| t.span.end >= offset && t.kind != TokenKind::Eof);
        let (replace, before) = match current {
            Some(t) if is_word(t.kind) => (
                Span {
                    start: t.span.start,
                    end: offset,
                },
                i - 1,
            ),
            Some(t) if matches!(t.kind, TokenKind::Number { .. }) => {
                // The letters at the end are the unit typed so far.
                let text = &self.text[t.span.start as usize..offset as usize];
                let digits = text.trim_end_matches(|c: char| c.is_ascii_alphabetic());
                let start = t.span.start + digits.len() as u32;
                return (Span { start, end: offset }, Context::Unit);
            }
            // Inside an operator such as `|>`: nothing goes there.
            Some(t) if t.span.end > offset => return (here, Context::Nothing),
            _ => (here, i),
        };
        let prev = before.checked_sub(1).map(|j| &self.tokens[j]);
        let prev2 = before.checked_sub(2).map(|j| &self.tokens[j]);
        let prev3 = before.checked_sub(3).map(|j| &self.tokens[j]);
        let kind = |t: Option<&Token>| t.map(|t| t.kind);

        let word = |t: Option<&Token>, w: &str| {
            t.is_some_and(|t| t.kind == TokenKind::Ident && self.slice(t.span) == w)
        };
        // `event` is only a keyword at the start of a line.
        let starts_line = |t: Option<&Token>| t.is_some_and(|t| t.newline_before);
        if word(prev, "event") && (starts_line(prev) || before == 1) {
            return (replace, Context::Nothing);
        }
        if word(prev2, "event")
            && (starts_line(prev2) || before == 2)
            && kind(prev) == Some(TokenKind::Ident)
        {
            return (replace, Context::EventKind);
        }
        if word(prev, "on") {
            return (replace, Context::EventName);
        }
        if matches!(kind(prev), Some(TokenKind::LParen | TokenKind::Comma))
            && self.in_event_filters(before - 1)
        {
            return (replace, Context::EventFilter);
        }

        let context = match kind(prev) {
            // Naming something new.
            Some(TokenKind::Let | TokenKind::State | TokenKind::Rill | TokenKind::Fn) => {
                Context::Nothing
            }
            Some(TokenKind::Lt) if self.in_def_signature(offset) => Context::Nothing,
            Some(TokenKind::LParen | TokenKind::Comma) if self.in_param_list(before) => {
                Context::Nothing
            }
            Some(TokenKind::As) => Context::Type,
            // The return type, right after the parameter list.
            Some(TokenKind::RParen) if self.in_param_list(before - 1) => Context::Type,
            Some(TokenKind::LBracket) if self.opens_frame_type(before - 1) => Context::Type,
            Some(TokenKind::Colon) => {
                let let_like = matches!(kind(prev3), Some(TokenKind::Let | TokenKind::State));
                if let_like || self.in_param_list(before - 2) {
                    Context::Type
                } else {
                    Context::Expr { stmt_start: false }
                }
            }
            Some(TokenKind::Pipe) => {
                let p = prev.expect("matched");
                Context::Pipe(self.type_ending_at(p.span.start))
            }
            _ if self
                .def_at(offset)
                .is_none_or(|d| offset < self.def(d).body.span.start) =>
            {
                // Outside any body: between definitions, or in a signature
                // that did not parse.
                if self.def_at(offset).is_some() {
                    Context::Nothing
                } else {
                    Context::TopLevel
                }
            }
            Some(TokenKind::LBrace | TokenKind::RBrace | TokenKind::Semi) | None => {
                Context::Expr { stmt_start: true }
            }
            Some(_) => {
                let gap =
                    &self.text[prev.expect("matched").span.end as usize..replace.start as usize];
                Context::Expr {
                    stmt_start: gap.contains('\n'),
                }
            }
        };
        (replace, context)
    }

    /// Inside a `//` or `/* */` comment.
    fn in_comment(&self, offset: u32) -> bool {
        let i = self.tokens.partition_point(|t| t.span.end <= offset);
        let from = i.checked_sub(1).map_or(0, |j| self.tokens[j].span.end) as usize;
        if self.tokens.get(i).is_some_and(|t| t.span.start < offset) {
            return false; // inside a token
        }
        let gap = &self.text[from..offset as usize];
        let line = gap.rsplit('\n').next().unwrap_or("");
        line.contains("//")
            || gap
                .rfind("/*")
                .is_some_and(|open| !gap[open..].contains("*/"))
    }

    /// Whether the `(` or `,` at token `before - 1` is in the parameter list
    /// of a definition or an anonymous fn, where names are declared.
    fn in_param_list(&self, before: usize) -> bool {
        let mut depth = 0;
        for j in (0..before).rev() {
            match self.tokens[j].kind {
                TokenKind::RParen | TokenKind::RBracket => depth += 1,
                TokenKind::LBracket => depth -= 1,
                TokenKind::LBrace | TokenKind::RBrace => return false,
                TokenKind::LParen if depth == 0 => {
                    let k = |n: usize| j.checked_sub(n).map(|i| self.tokens[i].kind);
                    return k(1) == Some(TokenKind::Fn)
                        || (k(1) == Some(TokenKind::Ident)
                            && matches!(k(2), Some(TokenKind::Fn | TokenKind::Rill)))
                        || (k(1) == Some(TokenKind::Gt)
                            && self.in_def_signature(self.tokens[j].span.start));
                }
                TokenKind::LParen => depth -= 1,
                _ => {}
            }
        }
        false
    }

    /// Is token `i` inside the filters of an event declaration, as in
    /// `event keys note_on(sender: 5, |`?
    fn in_event_filters(&self, i: usize) -> bool {
        let mut depth = 0;
        for j in (0..=i).rev() {
            match self.tokens[j].kind {
                TokenKind::RParen => depth += 1,
                TokenKind::LParen if depth == 0 => {
                    let k = |n: usize| j.checked_sub(n).map(|i| &self.tokens[i]);
                    return k(3).is_some_and(|t| {
                        t.kind == TokenKind::Ident && self.slice(t.span) == "event"
                    }) && matches!(k(2).map(|t| t.kind), Some(TokenKind::Ident))
                        && matches!(k(1).map(|t| t.kind), Some(TokenKind::Ident));
                }
                TokenKind::LParen => depth -= 1,
                TokenKind::LBrace | TokenKind::RBrace => return false,
                _ => {}
            }
        }
        false
    }

    /// The program's declared events.
    fn event_name_items(&self) -> Vec<CompletionItem> {
        self.checked
            .events
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let d = d.as_ref()?;
                Some(item(
                    &d.name,
                    Kind::Event,
                    Some(self.render_event(i)),
                    self.event_doc(i),
                    0,
                ))
            })
            .collect()
    }

    /// Does the `[` at token `i` start a frame type? It does after `:` or
    /// a parameter list, and inside another frame type (`[[Sample; 2]; 4]`).
    fn opens_frame_type(&self, i: usize) -> bool {
        let mut j = i;
        while j > 0 && self.tokens[j - 1].kind == TokenKind::LBracket {
            j -= 1;
        }
        match j.checked_sub(1).map(|k| self.tokens[k].kind) {
            Some(TokenKind::Colon) => true,
            Some(TokenKind::RParen) => self.in_param_list(j - 1),
            _ => false,
        }
    }

    fn in_def_signature(&self, offset: u32) -> bool {
        // Between `fn`/`rill` and the body's `{`, with no `{` in between.
        let i = self.tokens.partition_point(|t| t.span.start < offset);
        for t in self.tokens[..i].iter().rev() {
            match t.kind {
                TokenKind::Fn | TokenKind::Rill => return true,
                TokenKind::LBrace | TokenKind::RBrace => return false,
                _ => {}
            }
        }
        false
    }

    /// Inside a rill's body and not in an anonymous fn: where rills can be
    /// called and `state` declared.
    fn in_rill_body(&self, offset: u32) -> bool {
        let Some(d) = self.def_at(offset) else {
            return false;
        };
        if self.checked.signatures[d].kind != DefKind::Rill {
            return false;
        }
        let mut in_lambda = false;
        walk_block(&self.def(d).body, &mut |e| {
            if let ExprKind::Fn { body, .. } = &e.kind
                && body.span.start < offset
                && offset < body.span.end
            {
                in_lambda = true;
            }
        });
        !in_lambda
    }

    /// The type of the expression ending right before `end` (`x * 2` in
    /// `x * 2 |> f`), if it was checked.
    fn type_ending_at(&self, end: u32) -> Option<Type> {
        let last = *self.tokens[..self.tokens.partition_point(|t| t.span.end <= end)].last()?;
        let def = self.def(self.def_at(last.span.end)?);
        let mut best: Option<&rill::lang::ast::Expr> = None;
        walk_block(&def.body, &mut |e| {
            if e.span.end == last.span.end && best.is_none_or(|b| e.span.start < b.span.start) {
                best = Some(e);
            }
        });
        if let Some(e) = best {
            let ty = self.checked.types.get(e.id as usize)?;
            return (!ty.is_wild()).then(|| ty.clone());
        }
        // The statement did not parse: go by the last token alone.
        match last.kind {
            TokenKind::Number { unit: Some(u), .. } => Some(Type::from_dimension(u.dimension())),
            TokenKind::Number { unit: None, .. } => Some(Type::Num),
            TokenKind::Ident => {
                let name = self.slice(last.span);
                match self.target_by_name(name, last.span.start) {
                    Some(super::index::Target::Binding(b)) => {
                        Some(self.checked.bindings[b].ty.clone())
                    }
                    _ if rill::lang::check::pitch_literal(name).is_some() => Some(Type::Pitch),
                    _ => builtins::constant(name).cloned(),
                }
            }
            _ => None,
        }
    }

    /// Bindings usable at `offset`, innermost first per name.
    fn visible_bindings(&self, offset: u32) -> Vec<&Binding> {
        let Some(d) = self.def_at(offset) else {
            return Vec::new();
        };
        let def_end = self.def(d).span.end;
        let mut found: Vec<&Binding> = self
            .checked
            .bindings
            .iter()
            .filter(|b| {
                b.def == d
                    && b.scope.start <= offset
                    // A block still open while typing reaches the cursor.
                    && (offset <= b.scope.end || b.scope.end >= def_end)
            })
            .collect();
        found.sort_by_key(|b| std::cmp::Reverse(b.scope.start));
        let mut seen = HashSet::new();
        found.retain(|b| seen.insert(b.name.clone()));
        found
    }

    fn value_items(&self, offset: u32) -> Vec<CompletionItem> {
        let mut items: Vec<CompletionItem> = self
            .visible_bindings(offset)
            .into_iter()
            .map(|b| {
                let kind = match b.kind {
                    BindingKind::Size => Kind::TypeParameter,
                    BindingKind::State => Kind::Field,
                    _ => Kind::Variable,
                };
                item(
                    &b.name,
                    kind,
                    Some(self.render_binding(b)),
                    self.binding_doc(b),
                    0,
                )
            })
            .collect();
        for (name, ty) in builtins::CONSTANTS {
            let doc = builtins::doc(name).map(str::to_owned);
            items.push(item(name, Kind::Constant, Some(ty.to_string()), doc, 3));
        }
        items
    }

    /// Fns, rills and built-ins, and variables holding functions. With a
    /// piped type, the ones taking it first sort first.
    fn callable_items(&self, offset: u32, piped: Option<&Type>) -> Vec<CompletionItem> {
        let rills_allowed = self.in_rill_body(offset);
        // Recursion is not allowed, so the definition itself is no use.
        let current = self.def_at(offset);
        let fits = |p: Option<&Type>, lifts: bool| match (piped, p) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(t), Some(Type::Param(c))) => {
                builtins::satisfies(c, t)
                    || matches!(t, Type::Frame(e, _) if lifts && builtins::satisfies(c, e))
            }
            (Some(t), Some(p)) => {
                coerces(t, p) || matches!(t, Type::Frame(e, _) if lifts && coerces(e, p))
            }
        };
        let rank = |fit: bool, base: u8| {
            if piped.is_some() && !fit {
                base + 5
            } else {
                base
            }
        };

        let mut items = Vec::new();
        for (d, sig) in self.checked.signatures.iter().enumerate() {
            let is_rill = sig.kind == DefKind::Rill;
            if (is_rill && !rills_allowed) || current == Some(d) {
                continue;
            }
            let fit = fits(sig.params.first().map(|p| &p.ty), is_rill);
            let kind = if is_rill { Kind::Class } else { Kind::Function };
            items.push(item(
                &sig.name,
                kind,
                Some(self.render_def(d).label),
                self.def_doc(d),
                rank(fit, 1),
            ));
        }
        for name in builtins::FUNCTIONS {
            let sigs = builtins::lookup(name);
            let fit = sigs.iter().any(|s| {
                fits(
                    s.params.first().map(|p| &p.ty),
                    builtins::takes_frames(name),
                )
            });
            let detail = render::builtin(name).into_iter().next().map(|r| r.label);
            let doc = builtins::doc(name).map(str::to_owned);
            items.push(item(name, Kind::Function, detail, doc, rank(fit, 2)));
        }
        for b in self.visible_bindings(offset) {
            if let Type::Fn(params, _) = &b.ty {
                let fit = fits(params.first(), false);
                items.push(item(
                    &b.name,
                    Kind::Variable,
                    Some(b.ty.to_string()),
                    None,
                    rank(fit, 0),
                ));
            }
        }
        items
    }

    /// `name:` for the parameters of the call around `offset` that are not
    /// given yet, when at the start of an argument.
    fn named_arg_items(&self, offset: u32) -> Vec<CompletionItem> {
        let Some((callee, _, None)) = self.call_at_tokens(offset) else {
            return Vec::new();
        };
        // Only at the start of an argument.
        let i = self.tokens.partition_point(|t| t.span.end <= offset);
        let prev = self.tokens[..i]
            .iter()
            .rev()
            .find(|t| !(t.span.end >= offset && is_word(t.kind)));
        if !prev.is_some_and(|t| matches!(t.kind, TokenKind::LParen | TokenKind::Comma)) {
            return Vec::new();
        }
        let name = self.slice(callee);
        let params: Vec<(String, String)> = match self.target_by_name(name, callee.start) {
            Some(super::index::Target::Def(d)) => self.checked.signatures[d]
                .params
                .iter()
                .map(|p| (p.name.clone(), p.ty.to_string()))
                .collect(),
            Some(super::index::Target::Builtin(n)) => builtins::lookup(&n)
                .into_iter()
                .flat_map(|s| s.params)
                .map(|p| (p.name, p.ty.to_string()))
                .collect(),
            _ => return Vec::new(),
        };
        // Names already written in this call, and parameters already given
        // by position.
        let call_text = &self.text[callee.end as usize..offset as usize];
        let positional = self.call_at(offset).map_or(0, |c| {
            c.args.iter().take_while(|a| a.name.is_none()).count()
        });
        params
            .into_iter()
            .skip(positional)
            .filter(|(p, _)| !call_text.contains(&format!("{p}:")))
            .map(|(p, ty)| CompletionItem {
                insert_text: Some(format!("{p}: ")),
                filter_text: Some(p.clone()),
                ..item(&format!("{p}:"), Kind::Property, Some(ty), None, 0)
            })
            .collect()
    }

    fn type_items(&self, offset: u32) -> Vec<CompletionItem> {
        let mut items: Vec<CompletionItem> = TYPES
            .iter()
            .map(|t| item(t, Kind::Struct, None, type_doc(t).map(str::to_owned), 0))
            .collect();
        // Size parameters, for frame types. From the tokens, since the
        // signature being typed has not parsed.
        let i = self.tokens.partition_point(|t| t.span.start < offset);
        let keyword = self.tokens[..i]
            .iter()
            .rposition(|t| matches!(t.kind, TokenKind::Fn | TokenKind::Rill));
        if let Some(k) = keyword
            && self
                .tokens
                .get(k + 2)
                .is_some_and(|t| t.kind == TokenKind::Lt)
        {
            for t in self.tokens[k + 3..i]
                .iter()
                .take_while(|t| t.kind != TokenKind::Gt)
            {
                if t.kind == TokenKind::Ident {
                    let name = self.slice(t.span);
                    items.push(item(
                        name,
                        Kind::TypeParameter,
                        Some("size".into()),
                        None,
                        1,
                    ));
                }
            }
        }
        items
    }

    fn keyword_items(&self, keywords: &[&str], snippets: bool) -> Vec<CompletionItem> {
        keywords
            .iter()
            .map(|k| {
                let snippet = match *k {
                    "rill" => Some("rill ${1:name}($2) ${3:Sample} {\n\t$0\n}"),
                    "fn" => Some("fn ${1:name}($2) ${3:Sample} {\n\t$0\n}"),
                    "if" => Some("if $1 {\n\t$0\n}"),
                    "event" => Some("event ${1:name} ${2|note_on,note_off,control_change|}"),
                    "on" => Some("on ${1:event}(${2:note}) {\n\t$0\n}"),
                    _ => None,
                };
                let mut it = item(k, Kind::Keyword, None, keyword_doc(k).map(str::to_owned), 4);
                if let (true, Some(s)) = (snippets, snippet) {
                    it.insert_text = Some(s.to_owned());
                    it.insert_text_format = Some(InsertTextFormat::Snippet);
                    it.kind = Some(Kind::Snippet);
                }
                it
            })
            .collect()
    }
}

fn event_kind_items() -> Vec<CompletionItem> {
    rill::event::EventKind::ALL
        .iter()
        .map(|k| {
            item(
                k.name(),
                Kind::Keyword,
                None,
                render::event_kind_doc(k.name()).map(str::to_owned),
                0,
            )
        })
        .collect()
}

fn unit_items() -> Vec<CompletionItem> {
    Unit::ALL
        .iter()
        .map(|(name, unit)| {
            let ty = Type::from_dimension(unit.dimension());
            item(
                name,
                Kind::Unit,
                Some(ty.to_string()),
                unit_doc(name).map(str::to_owned),
                0,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn complete(src: &str) -> Completions {
        let offset = src.find('$').expect("a `$` cursor") as u32;
        Analysis::new(&src.replacen('$', "", 1)).completions(offset, true)
    }

    fn labels(src: &str) -> Vec<String> {
        let mut items = complete(src).items;
        items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
        items.into_iter().map(|i| i.label).collect()
    }

    fn has(src: &str, wanted: &[&str]) {
        let got = labels(src);
        for w in wanted {
            assert!(got.iter().any(|g| g == w), "{w} missing from {got:?}");
        }
    }

    fn lacks(src: &str, unwanted: &[&str]) {
        let got = labels(src);
        for u in unwanted {
            assert!(!got.iter().any(|g| g == u), "{u} should not be in {got:?}");
        }
    }

    const HEAD: &str = "fn half(x: Sample) Sample { x / 2 }\nrill osc(freq: Freq) Sample { return sin(freq / RATE) }\n";

    #[test]
    fn expressions_offer_locals_defs_builtins() {
        let src = format!(
            "{HEAD}rill main(gain: Float = 1) Sample {{\n    let a = 1\n    return a * g$\n}}"
        );
        has(&src, &["a", "gain", "half", "osc", "sin", "RATE", "true"]);
        lacks(&src, &["let", "state", "freq", "x"]);
        // Locals first.
        assert_eq!(&labels(&src)[..2], ["a", "gain"]);
        // The word being typed is replaced.
        let c = complete(&src);
        assert_eq!(c.replace.end - c.replace.start, 1);
    }

    #[test]
    fn statement_start_offers_statement_keywords() {
        let src = format!("{HEAD}rill main() Sample {{\n    let a = 1\n    $\n}}");
        has(&src, &["let", "state", "return", "a"]);
        let src = format!("{HEAD}fn f() Sample {{\n    $\n}}");
        has(&src, &["let", "return"]);
        // Fns cannot call rills or keep state.
        lacks(&src, &["state", "osc"]);
    }

    #[test]
    fn while_typing_at_the_end_of_an_unclosed_body() {
        let src =
            format!("{HEAD}rill main(gain: Float) Sample {{\n    let a = 1\n    let b = a + $");
        has(&src, &["a", "gain", "osc"]);
        lacks(&src, &["b"]);
    }

    #[test]
    fn types() {
        has(&format!("{HEAD}rill f(x: $"), &["Sample", "Freq", "Gain"]);
        has(&format!("{HEAD}rill f(x: Sample) $"), &["Sample"]);
        has(&format!("{HEAD}rill f(x: Sample) [$"), &["Sample"]);
        has(&format!("{HEAD}rill f(x: [[S$"), &["Sample"]);
        has(&format!("{HEAD}rill f() [[$"), &["Sample"]);
        lacks(
            &format!("{HEAD}rill f() Sample {{\n    let a = [[$"),
            &["Sample"],
        );
        has(&format!("{HEAD}rill f(x: fn(Pitch) $"), &["Freq"]);
        lacks(
            &format!("{HEAD}rill f() Sample {{\n    return sin(1) $"),
            &["Sample"],
        );
        has(
            &format!("{HEAD}rill f() Sample {{\n    let a: F$\n"),
            &["Float", "Freq"],
        );
        has("rill f<N>(x: [S$", &["Sample", "N"]);
        lacks(&format!("{HEAD}rill f(x: $"), &["half", "osc", "let"]);
    }

    #[test]
    fn units() {
        let c = complete("rill f() Sample { let t = 300m$ }");
        let labels: Vec<&str> = c.items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"ms") && labels.contains(&"Hz"),
            "{labels:?}"
        );
        // Only the unit is replaced, not the digits.
        assert_eq!(c.replace.end - c.replace.start, 1);
    }

    #[test]
    fn pipes_prefer_what_fits() {
        let src = format!("{HEAD}rill main() Sample {{ return 440Hz |> $ }}");
        let got = labels(&src);
        let pos = |n: &str| got.iter().position(|g| g == n).unwrap();
        // `osc` takes a `Freq`; `half` takes a `Sample`.
        assert!(pos("osc") < pos("half"), "{got:?}");
        lacks(&src, &["let", "true", "RATE"]);
    }

    #[test]
    fn named_arguments() {
        let src = format!("{HEAD}rill main() Sample {{ return osc($) }}");
        has(&src, &["freq:"]);
        let src = "rill main() Sample { return sin(equal(A4, steps: 24, $)) }";
        has(src, &["a4:"]);
        lacks(src, &["steps:", "pitch:"]);
    }

    #[test]
    fn nothing_when_naming_or_commenting() {
        assert!(labels(&format!("{HEAD}rill main() Sample {{\n    let $")).is_empty());
        assert!(labels(&format!("{HEAD}rill m$")).is_empty());
        assert!(labels(&format!("{HEAD}rill main(fr$")).is_empty());
        assert!(labels(&format!("{HEAD}rill main() Sample {{ // so$\n }}")).is_empty());
        assert!(labels("rill main() Sample { return fn(p$) { p } }").is_empty());
    }

    #[test]
    fn events() {
        let decls = "event keys note_on(sender: 1)\nevent knob control_change\n";
        // The kind after the name.
        has("event keys $", &["note_on", "note_off", "control_change"]);
        has("event keys note_o$", &["note_on", "note_off"]);
        assert!(labels("event $").is_empty(), "naming the event");
        // Filters inside the parentheses.
        assert_eq!(labels("event keys note_on($"), ["channel", "sender"]);
        assert_eq!(
            labels("event keys note_on(sender: 1, $"),
            ["channel", "sender"]
        );
        lacks("rill main() Sample { return sin($", &["sender"]);
        // Declared events after `on`.
        assert_eq!(
            labels(&format!("{decls}rill main() Sample {{\n    on $")),
            ["keys", "knob"]
        );
        // `on` starts statements in rills only.
        has(&format!("{HEAD}rill main() Sample {{\n    $\n}}"), &["on"]);
        lacks(&format!("{HEAD}fn f() Sample {{\n    $\n}}"), &["on"]);
    }

    #[test]
    fn top_level() {
        let got = labels(&format!("{HEAD}\n$"));
        assert_eq!(got, ["event", "fn", "rill"]);
        let c = complete(&format!("{HEAD}\nri$"));
        let rill = c.items.iter().find(|i| i.label == "rill").unwrap();
        assert_eq!(rill.insert_text_format, Some(InsertTextFormat::Snippet));
    }
}
