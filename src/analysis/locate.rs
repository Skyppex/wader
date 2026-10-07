//! What is at a position: tokens, expressions, calls.

use rill::lang::Span;
use rill::lang::ast::{Arg, Block, Expr, ExprKind, Ident, Stmt};
use rill::lang::lexer::{Token, TokenKind};

use super::Analysis;

fn contains(span: Span, offset: u32) -> bool {
    span.start <= offset && offset <= span.end
}

/// Call `f` on every expression in `block`, parents before children.
pub fn walk_block<'a>(block: &'a Block, f: &mut impl FnMut(&'a Expr)) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let { value, .. } => walk_expr(value, f),
            Stmt::State { init, .. } => walk_expr(init, f),
            Stmt::Assign { value, .. } => walk_expr(value, f),
            Stmt::Return { value, .. } => walk_expr(value, f),
            Stmt::EventHandler { body, .. } => walk_block(body, f),
            Stmt::Expr(e) => walk_expr(e, f),
        }
    }
}

pub fn walk_expr<'a>(e: &'a Expr, f: &mut impl FnMut(&'a Expr)) {
    f(e);
    match &e.kind {
        ExprKind::Number { .. } | ExprKind::Bool(_) | ExprKind::Name(_) => {}
        ExprKind::Unary(_, x) | ExprKind::Cast(x, _) | ExprKind::Field(x, _) => walk_expr(x, f),
        ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) => {
            walk_expr(a, f);
            walk_expr(b, f);
        }
        ExprKind::Call { args, .. } => {
            for a in args {
                walk_expr(&a.value, f);
            }
        }
        ExprKind::If { cond, then, els } => {
            walk_expr(cond, f);
            walk_block(then, f);
            if let Some(els) = els {
                walk_expr(els, f);
            }
        }
        ExprKind::Block(b) | ExprKind::Fn { body: b, .. } => walk_block(b, f),
        ExprKind::Frame(xs) => {
            for x in xs {
                walk_expr(x, f);
            }
        }
    }
}

/// A call around a position, and which argument the position is in.
#[derive(Debug)]
pub struct CallSite<'a> {
    pub callee: &'a Ident,
    pub args: &'a [Arg],
    pub piped: bool,
    /// Index into the parameters the position is at, counting the piped
    /// value as the first.
    pub arg_index: usize,
    /// The name of the argument the position is in, if it is named.
    pub arg_name: Option<&'a str>,
}

impl Analysis {
    /// The token containing `offset`, or else the one ending there.
    pub fn token_at(&self, offset: u32) -> Option<&Token> {
        let i = self.tokens.partition_point(|t| t.span.end < offset);
        let mut candidates = self.tokens[i..]
            .iter()
            .take(2)
            .filter(|t| contains(t.span, offset));
        let first = candidates.next()?;
        // Between two tokens, prefer the one starting here.
        Some(
            candidates
                .next()
                .filter(|t| t.span.start == offset)
                .unwrap_or(first),
        )
    }

    /// Index of the token containing `offset` or ending right at it, for
    /// looking at its neighbours. At a gap, the token before the gap.
    pub fn token_index_before(&self, offset: u32) -> Option<usize> {
        let i = self.tokens.partition_point(|t| t.span.start < offset);
        i.checked_sub(1)
    }

    /// The innermost expression containing `offset`.
    pub fn expr_at(&self, offset: u32) -> Option<&Expr> {
        let def = self.def(self.def_at(offset)?);
        let mut found = None;
        for p in &def.params {
            if let Some(d) = &p.default {
                walk_expr(d, &mut |e| {
                    if contains(e.span, offset) {
                        found = Some(e)
                    }
                });
            }
        }
        walk_block(&def.body, &mut |e| {
            if contains(e.span, offset) {
                found = Some(e)
            }
        });
        found
    }

    /// The expression directly containing the one with `id`, which is at
    /// `offset`.
    pub fn parent_of<'a>(&'a self, id: u32, offset: u32) -> Option<&'a Expr> {
        let def = self.def(self.def_at(offset)?);
        let mut parent: Option<&Expr> = None;
        let mut visit = |e: &'a Expr| {
            let mut children = Vec::new();
            match &e.kind {
                ExprKind::Unary(_, x) | ExprKind::Cast(x, _) | ExprKind::Field(x, _) => {
                    children.push(x.id)
                }
                ExprKind::Binary(_, a, b) | ExprKind::Index(a, b) => children.extend([a.id, b.id]),
                ExprKind::Call { args, .. } => children.extend(args.iter().map(|a| a.value.id)),
                ExprKind::Frame(xs) => children.extend(xs.iter().map(|x| x.id)),
                ExprKind::If { cond, els, .. } => {
                    children.push(cond.id);
                    children.extend(els.iter().map(|x| x.id));
                }
                _ => {}
            }
            if children.contains(&id) {
                parent = Some(e);
            }
        };
        for d in def.params.iter().filter_map(|p| p.default.as_ref()) {
            walk_expr(d, &mut visit);
        }
        walk_block(&def.body, &mut visit);
        parent
    }

    /// The innermost call whose argument list contains `offset`, found in
    /// the syntax tree.
    pub fn call_at(&self, offset: u32) -> Option<CallSite<'_>> {
        let def = self.def(self.def_at(offset)?);
        let mut found = None;
        walk_block(&def.body, &mut |e| {
            if let ExprKind::Call {
                callee,
                args,
                piped,
            } = &e.kind
            {
                // Inside the parentheses: after the callee, before the end
                // (or at the end, if the closing parenthesis is missing).
                let open = callee.span.end;
                let in_parens = open < offset
                    && offset <= e.span.end
                    && self.text.as_bytes().get(open as usize) == Some(&b'(')
                    && !(offset == e.span.end
                        && self.text.as_bytes()[e.span.end as usize - 1] == b')');
                if in_parens {
                    found = Some((callee, args.as_slice(), *piped));
                }
            }
        });
        let (callee, args, piped) = found?;
        let skip = usize::from(piped);
        let written = &args[skip..];
        // One argument per comma passed: each sits between the end of an
        // argument and the start of the next.
        let start_of = |a: &Arg| a.name.as_ref().map_or(a.value.span.start, |n| n.span.start);
        let index = written
            .iter()
            .enumerate()
            .filter(|(i, a)| {
                let end = a.value.span.end;
                let next = written.get(i + 1).map_or(offset, start_of).min(offset);
                end <= next && self.text[end as usize..next as usize].contains(',')
            })
            .count();
        let arg_name = written
            .get(index)
            .and_then(|a| a.name.as_ref())
            .map(|n| n.name.as_str());
        Some(CallSite {
            callee,
            args,
            piped,
            arg_index: index + skip,
            arg_name,
        })
    }

    /// The call whose argument list contains `offset`, found from the
    /// tokens: for text too broken to parse. Returns the callee's name
    /// token, the number of commas before `offset` at the call's level, and
    /// the name of the argument `offset` is in, if it is named.
    pub fn call_at_tokens(&self, offset: u32) -> Option<(Span, usize, Option<Span>)> {
        let end = self.tokens.partition_point(|t| t.span.end <= offset);
        let mut depth = 0i32;
        let mut commas = 0;
        let mut name: Option<Span> = None;
        for i in (0..end).rev() {
            let t = &self.tokens[i];
            match t.kind {
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => depth += 1,
                TokenKind::LBracket => depth -= 1,
                TokenKind::LBrace => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                }
                TokenKind::LParen => {
                    if depth == 0 {
                        let callee = self.tokens[..i].last()?;
                        return (callee.kind == TokenKind::Ident
                            && callee.span.end == t.span.start)
                            .then_some((callee.span, commas, name));
                    }
                    depth -= 1;
                }
                TokenKind::Comma if depth == 0 => commas += 1,
                TokenKind::Colon if depth == 0 && commas == 0 => {
                    if let Some(id) = self.tokens[..i].last()
                        && id.kind == TokenKind::Ident
                    {
                        name = name.or(Some(id.span));
                    }
                }
                _ => {}
            }
        }
        None
    }
}
