//! Hover: what the thing under the cursor is.

use rill::lang::Span;
use rill::lang::builtins;
use rill::lang::check::{BindingKind, pitch_literal};
use rill::lang::lexer::{TokenKind, Unit};
use rill::lang::types::Type;

use super::Analysis;
use super::index::{Occurrence, Target};
use super::render::{self, keyword_doc, type_doc, unit_doc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoverInfo {
    /// Markdown.
    pub contents: String,
    pub span: Span,
}

/// Markdown: a code block, then paragraphs.
fn markdown(code: &str, paragraphs: impl IntoIterator<Item = Option<String>>) -> String {
    let mut out = format!("```rill\n{code}\n```");
    for p in paragraphs.into_iter().flatten() {
        out += "\n\n";
        out += &p;
    }
    out
}

/// A number without needless digits.
fn num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.into() }
}

impl Analysis {
    pub fn hover(&self, offset: u32) -> Option<HoverInfo> {
        if let Some(occ) = self.index.at(offset) {
            return self.hover_name(occ);
        }
        let token = *self.token_at(offset)?;
        let span = token.span;
        let contents = match token.kind {
            TokenKind::Number { value, unit, .. } => {
                // `-6dB` lexes as `-` and `6dB`; describe it as written.
                let negated = self
                    .expr_at(offset)
                    .zip(span.start.checked_sub(1).and_then(|o| self.expr_at(o)))
                    .is_some_and(|(lit, outer)| {
                        matches!(&outer.kind, rill::lang::ast::ExprKind::Unary(rill::lang::ast::UnOp::Neg, x) if x.id == lit.id)
                    });
                if negated {
                    let outer = self.expr_at(span.start - 1)?;
                    let contents = self.hover_number(offset, -value, unit)?;
                    return Some(HoverInfo {
                        contents,
                        span: outer.span,
                    });
                }
                self.hover_number(offset, value, unit)?
            }
            TokenKind::Ident => self.hover_builtin_arg(offset, span)?,
            TokenKind::Eof => return None,
            TokenKind::Fn
            | TokenKind::Rill
            | TokenKind::State
            | TokenKind::Let
            | TokenKind::Return
            | TokenKind::If
            | TokenKind::Else
            | TokenKind::True
            | TokenKind::False
            | TokenKind::As => keyword_doc(self.slice(span))?.to_owned(),
            _ => {
                // An operator: the type of what it makes.
                let e = self.expr_at(offset)?;
                let ty = self.checked.types.get(e.id as usize)?;
                if ty.is_wild() || *ty == Type::Unit {
                    return None;
                }
                return Some(HoverInfo {
                    contents: markdown(&ty.to_string(), []),
                    span: e.span,
                });
            }
        };
        Some(HoverInfo { contents, span })
    }

    fn hover_name(&self, occ: &Occurrence) -> Option<HoverInfo> {
        let contents = match &occ.target {
            Target::Def(i) => markdown(&self.render_def(*i).label, [self.def_doc(*i)]),
            Target::Binding(id) => {
                let b = &self.checked.bindings[*id];
                let owner = match b.kind {
                    BindingKind::Param | BindingKind::Size => {
                        let def = self.def(b.def);
                        let what = if b.kind == BindingKind::Size {
                            "size parameter"
                        } else {
                            "parameter"
                        };
                        let kind = self.checked.signatures[b.def].kind.word();
                        Some(format!("{what} of {kind} `{}`", def.name.name))
                    }
                    BindingKind::FnParam => Some("parameter of an anonymous fn".into()),
                    BindingKind::Let | BindingKind::State => None,
                };
                markdown(&self.render_binding(b), [owner, self.binding_doc(b)])
            }
            Target::Builtin(name) => {
                let code: Vec<String> =
                    render::builtin(name).into_iter().map(|r| r.label).collect();
                let doc = builtins::doc(name).map(str::to_owned);
                let note = render::type_params_note(&builtins::lookup(name));
                markdown(&code.join("\n"), [doc, note])
            }
            Target::Constant(name) => {
                let ty = builtins::constant(name)?;
                markdown(
                    &format!("{name}: {ty}"),
                    [builtins::doc(name).map(str::to_owned)],
                )
            }
            Target::Note(name) => {
                pitch_literal(name)?;
                // A pitch has no frequency until a tuning gives it one, so
                // none is shown.
                markdown(
                    &format!("{name}: Pitch"),
                    [Some(
                        "A note name: a position in pitch, not a frequency. A tuning turns it into a `Freq`, as in `A4 |> equal` or `A4 |> just(C)`."
                            .to_owned(),
                    )],
                )
            }
            Target::Type(name) => markdown(name, [type_doc(name).map(str::to_owned)]),
        };
        Some(HoverInfo {
            contents,
            span: occ.span,
        })
    }

    fn hover_number(&self, offset: u32, value: f64, unit: Option<Unit>) -> Option<String> {
        let ty = self
            .expr_at(offset)
            .and_then(|e| self.checked.types.get(e.id as usize))
            .filter(|t| !t.is_wild())
            .cloned();
        let ty = match (ty, unit) {
            (Some(t), _) => t,
            (None, Some(u)) => Type::from_dimension(u.dimension()),
            (None, None) => Type::Num,
        };
        let Some(unit) = unit else {
            return Some(markdown(&ty.to_string(), []));
        };
        let base = unit.to_base(value);
        let meaning = match unit {
            Unit::Hz | Unit::S | Unit::St => None,
            Unit::KHz => Some(format!("= {} Hz", num(base))),
            Unit::Ms => Some(format!("= {} s", num(base))),
            Unit::Cents => Some(format!("= {} semitones", num(base))),
            Unit::Db => Some(format!("= ×{} in amplitude", num(base))),
        };
        let name = unit.name();
        let what = unit_doc(name).map(|d| format!("`{name}`: {d}"));
        Some(markdown(&ty.to_string(), [meaning, what]))
    }

    /// The name of an argument to a built-in, as in `equal(E4, a4: 432Hz)`.
    fn hover_builtin_arg(&self, offset: u32, span: Span) -> Option<String> {
        let call = self.call_at(offset)?;
        let name = self.slice(span);
        if !call
            .args
            .iter()
            .any(|a| a.name.as_ref().is_some_and(|n| n.span == span))
        {
            return None;
        }
        let sigs = builtins::lookup(&call.callee.name);
        let sig = sigs
            .iter()
            .find(|s| s.params.iter().any(|p| p.name == name))?;
        let rendered = render::builtin(&call.callee.name);
        let i = sigs.iter().position(|s| std::ptr::eq(s, sig))?;
        let pi = sig.params.iter().position(|p| p.name == name)?;
        let (s, e) = rendered[i].params[pi];
        Some(markdown(
            &rendered[i].label[s..e],
            [Some(format!(
                "parameter of built-in `{}`",
                call.callee.name
            ))],
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hover(src: &str) -> Option<String> {
        let offset = src.find('$').expect("a `$` cursor") as u32;
        Analysis::new(&src.replacen('$', "", 1))
            .hover(offset)
            .map(|h| h.contents)
    }

    const SRC: &str = "\
// Turns a frequency into a sine.
// Smooth.
rill osc(freq: Freq, gain: Gain = -6dB) -> Sample {
    // Where in the cycle we are.
    state phase: Float = 0
    phase = wrap(phase + freq / RATE)
    let n = A4
    return sin(phase * TAU) + gain
}
";

    fn at(needle: &str) -> Option<String> {
        let i = SRC.find(needle).unwrap_or_else(|| panic!("{needle}"));
        hover(&format!("{}${}", &SRC[..i], &SRC[i..]))
    }

    #[test]
    fn definitions_show_signature_and_doc() {
        assert_eq!(
            at("osc").unwrap(),
            "```rill\nrill osc(freq: Freq, gain: Gain = -6dB) -> Sample\n```\n\nTurns a frequency into a sine.\nSmooth."
        );
    }

    #[test]
    fn bindings() {
        assert_eq!(
            at("phase = wrap").unwrap(),
            "```rill\nstate phase: Float\n```\n\nWhere in the cycle we are."
        );
        assert_eq!(
            at("gain\n").unwrap(),
            "```rill\ngain: Gain = -6dB\n```\n\nparameter of rill `osc`"
        );
        assert!(
            at("n = A4")
                .unwrap()
                .starts_with("```rill\nlet n: Pitch\n```")
        );
    }

    #[test]
    fn builtins_constants_notes_types() {
        let sin = at("sin(").unwrap();
        assert!(
            sin.starts_with("```rill\nfn sin(x: T) -> T\n```\n\nSine of `x`"),
            "{sin}"
        );
        assert!(sin.contains("`T` is a plain number"), "{sin}");
        assert!(
            at("RATE")
                .unwrap()
                .starts_with("```rill\nRATE: Freq\n```\n\nThe host sample rate")
        );
        let a4 = at("A4").unwrap();
        assert!(a4.starts_with("```rill\nA4: Pitch\n```"), "{a4}");
        assert!(!a4.contains("Hz "), "a pitch has no frequency yet: {a4}");
        assert!(
            at("Float")
                .unwrap()
                .starts_with("```rill\nFloat\n```\n\nA plain number")
        );
    }

    #[test]
    fn literals_keywords_operators() {
        let db = at("6dB").unwrap();
        assert!(
            db.starts_with("```rill\nGain\n```\n\n= ×0.5012 in amplitude"),
            "{db}"
        );
        assert!(
            at("state")
                .unwrap()
                .contains("keeps its value between ticks")
        );
        assert_eq!(at("+ freq").unwrap(), "```rill\nFloat\n```");
        assert_eq!(at("// Smooth"), None);
    }

    #[test]
    fn named_arguments_of_builtins() {
        let h =
            hover("rill main() -> Sample { return sin(equal(A4, a$4: 432Hz) / RATE) }").unwrap();
        assert_eq!(
            h,
            "```rill\na4: Freq = 440Hz\n```\n\nparameter of built-in `equal`"
        );
    }

    #[test]
    fn broken_text() {
        let h = hover(
            "rill main(gain: Float) -> Sample {\n    let x = sin(1) * ga$in\n    let y = (x +\n",
        )
        .unwrap();
        assert!(h.starts_with("```rill\ngain: Float\n```"), "{h}");
    }
}
