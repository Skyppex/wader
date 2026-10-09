//! Hover: what the thing under the cursor is.

use rill::lang::Span;
use rill::lang::ast::{BinOp, Expr, ExprKind, UnOp};
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
                let lit = self.expr_at(offset);
                let negated = lit.and_then(|lit| {
                    let outer = self.parent_of(lit.id, offset)?;
                    matches!(outer.kind, ExprKind::Unary(UnOp::Neg, _)).then_some(outer)
                });
                let (operand, value) = match negated {
                    Some(outer) => (Some(outer), -value),
                    None => (lit, value),
                };
                let contents = self.hover_number(offset, value, unit, operand)?;
                return Some(HoverInfo {
                    contents,
                    span: operand.map_or(span, |e| e.span),
                });
            }
            TokenKind::Ident if self.is_event_keyword(span) => {
                keyword_doc(self.slice(span))?.to_owned()
            }
            TokenKind::Ident => match self.hover_seq_field(span) {
                Some(h) => h,
                None => self.hover_builtin_arg(offset, span)?,
            },
            TokenKind::Eof => return None,
            TokenKind::Str => self.hover_import(span)?,
            TokenKind::Fn
            | TokenKind::Const
            | TokenKind::Import
            | TokenKind::Export
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

    /// `event` starting a declaration, or `on` starting a handler. Both are
    /// ordinary names elsewhere.
    /// A word that is a keyword here and an ordinary name elsewhere:
    /// `event`, `on`, `seq`, `invoke`, `trigger`, `halt`, `claim`,
    /// `release`, `each` before an argument, or `start` in `on start`.
    fn is_event_keyword(&self, span: Span) -> bool {
        let next = self.tokens.iter().find(|t| t.span.start >= span.end);
        let prev = self.tokens.iter().rev().find(|t| t.span.end <= span.start);
        let next_kind = next.map(|t| t.kind);
        // `claim` and `release` follow a handler's head: `on keys(n) claim {`.
        let line_start = self.text[..span.start as usize]
            .rfind('\n')
            .map_or(0, |i| i + 1);
        let on_line = self.text[line_start..span.start as usize]
            .trim_start()
            .starts_with("on ");
        match self.slice(span) {
            "event" => self
                .program
                .events
                .iter()
                .any(|e| e.span.start == span.start),
            "seq" => self.program.seqs.iter().any(|s| s.span.start == span.start),
            "on" => next.is_some_and(|t| {
                self.slice(t.span) == "start"
                    || matches!(self.index.at(t.span.start), Some(o) if matches!(o.target, Target::Event(_)))
            }),
            "start" => prev.is_some_and(|t| self.slice(t.span) == "on"),
            "invoke" | "trigger" | "halt" => self.expr_at(span.start).is_some_and(|e| {
                e.span.start == span.start
                    && matches!(e.kind, ExprKind::Invoke { .. } | ExprKind::Halt { .. })
            }),
            "claim" => {
                on_line && matches!(next_kind, Some(TokenKind::LBrace | TokenKind::LParen))
            }
            "release" => on_line && next_kind == Some(TokenKind::LBrace),
            "each" => self
                .call_at(span.start)
                .is_some_and(|c| c.args.iter().any(|a| a.each == Some(span))),
            _ => false,
        }
    }

    fn hover_name(&self, occ: &Occurrence) -> Option<HoverInfo> {
        let contents = match &occ.target {
            Target::Def(i) => markdown(
                &self.render_def(*i).label,
                [self.def_doc(*i), self.origin_note(self.item_module(*i))],
            ),
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
                    BindingKind::EventParam => self
                        .handler_of(b)
                        .map(|name| format!("the payload of event `{}`", name.name)),
                    BindingKind::Let | BindingKind::State | BindingKind::Const => None,
                };
                let payload = match b.kind {
                    BindingKind::EventParam => type_doc(&b.ty.to_string()).map(str::to_owned),
                    _ => None,
                };
                markdown(
                    &self.render_binding(b),
                    [owner, payload, self.binding_doc(b)],
                )
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
            Target::Event(i) if self.checked.seq_event(*i).is_some() => {
                markdown(&self.render_event(*i), [self.event_doc(*i)])
            }
            Target::Event(i) => {
                let kind = self.checked.events.get(*i)?.as_ref()?.kind.name();
                markdown(
                    &self.render_event(*i),
                    [
                        self.event_doc(*i),
                        render::event_kind_doc(kind).map(str::to_owned),
                    ],
                )
            }
            Target::EventKind(name) => {
                markdown(name, [render::event_kind_doc(name).map(str::to_owned)])
            }
            Target::Const(i) => markdown(
                &self.render_const(*i),
                [
                    self.const_doc(*i),
                    self.origin_note(self.program.modules.konst(*i)),
                ],
            ),
            Target::Seq(i) => {
                let seq = &self.program.seqs[*i];
                let steps = seq.steps.len();
                let summary = format!(
                    "A sequence of {steps} step{}. Start it with `invoke {}` in an `on` handler.",
                    if steps == 1 { "" } else { "s" },
                    seq.name.name
                );
                markdown(
                    &self.render_seq(*i),
                    [
                        self.seq_doc(*i),
                        Some(summary),
                        self.origin_note(self.program.modules.seq(*i)),
                    ],
                )
            }
        };
        Some(HoverInfo {
            contents,
            span: occ.span,
        })
    }

    /// Where something declared in another file comes from.
    fn origin_note(&self, module: usize) -> Option<String> {
        (module != 0).then(|| format!("From \"{}\".", self.program.modules.name(module)))
    }

    /// An `import`'s path: the file, and what it gives.
    fn hover_import(&self, span: Span) -> Option<String> {
        let (k, import) = self
            .program
            .imports
            .iter()
            .enumerate()
            .find(|(_, i)| i.path_span == span)?;
        let target = import.module? as usize;
        let file = &self.sources.files.get(target)?.display;
        let modules = &self.program.modules;
        let mut names: Vec<&str> = Vec::new();
        for (i, item) in self.program.items.iter().enumerate() {
            if modules.item(i) == target && item.def().export.is_some() {
                names.push(&item.def().name.name);
            }
        }
        for (j, seq) in self.program.seqs.iter().enumerate() {
            if modules.seq(j) == target && seq.export.is_some() {
                names.push(&seq.name.name);
            }
        }
        for (i, c) in self.program.consts.iter().enumerate() {
            if modules.konst(i) == target && c.export.is_some() {
                names.push(&c.name.name);
            }
        }
        for (i, e) in self.program.events.iter().enumerate() {
            if modules.event(i) == target && e.export.is_some() {
                names.push(&e.name.name);
            }
        }
        let passes_on: Vec<String> = self
            .program
            .imports
            .iter()
            .enumerate()
            .filter(|&(j, i)| modules.import(j) == target && i.export.is_some())
            .map(|(_, i)| format!("\"{}\"", i.path))
            .collect();
        let list = |xs: Vec<String>| xs.join(", ");
        let exports = match names.is_empty() {
            true => "Exports nothing.".to_owned(),
            false => format!(
                "Exports {}.",
                list(names.iter().map(|n| format!("`{n}`")).collect())
            ),
        };
        let passes = (!passes_on.is_empty()).then(|| format!("Passes on {}.", list(passes_on)));
        let how = if import.export.is_some() {
            "Everything it gives is passed on to files importing this one."
        } else {
            "Only this file sees what it gives."
        };
        let _ = k;
        Some(markdown(
            &format!("import \"{}\"", import.path),
            [
                Some(format!("`{file}`")),
                Some(exports),
                passes,
                Some(how.to_owned()),
            ],
        ))
    }

    /// A number literal, as `operand` (the literal, or its negation).
    fn hover_number(
        &self,
        offset: u32,
        value: f64,
        unit: Option<Unit>,
        operand: Option<&Expr>,
    ) -> Option<String> {
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
            Unit::KHz | Unit::Bpm => Some(format!("= {} Hz", num(base))),
            Unit::Ms => Some(format!("= {} s", num(base))),
            Unit::Cents => Some(format!("= {} semitones", num(base))),
            Unit::Db => {
                // Subtracted from a signal, a level scales it the other way.
                if let Some(e) = operand
                    && self.subtracted_from_signal(e, offset)
                {
                    let applied = Unit::Db.to_base(-value);
                    let alone = format!("{}dB", num(value));
                    let line = format!(
                        "Subtracted from a signal here: ×{} in amplitude\n\nOn its own, {alone} = ×{} in amplitude",
                        num(applied),
                        num(base)
                    );
                    Some(line)
                } else {
                    Some(format!("= ×{} in amplitude", num(base)))
                }
            }
        };
        let name = unit.name();
        let what = unit_doc(name).map(|d| format!("`{name}`: {d}"));
        Some(markdown(&ty.to_string(), [meaning, what]))
    }

    /// `e` is the right side of `x - e` where `x` is a signal, not a level.
    fn subtracted_from_signal(&self, e: &Expr, offset: u32) -> bool {
        let Some(parent) = self.parent_of(e.id, offset) else {
            return false;
        };
        let ExprKind::Binary(BinOp::Sub, left, right) = &parent.kind else {
            return false;
        };
        let left_ty = self.checked.types.get(left.id as usize);
        right.id == e.id
            && left_ty.is_some_and(|t| {
                let elem = match t {
                    Type::Frame(elem, _) => elem,
                    t => t,
                };
                elem.is_plain()
            })
    }

    /// A field of a sequence, as in `riff.step_count`: its type, value and
    /// meaning.
    fn hover_seq_field(&self, span: Span) -> Option<String> {
        let (seq, field) = self.seq_field_at(span)?;
        let facts = self.checked.seq_facts.get(seq)?;
        let (ty, value) = rill::lang::check::seq_field(facts, field)?;
        let doc = rill::lang::check::SEQ_FIELDS
            .iter()
            .find(|(n, _)| *n == field)
            .map(|(_, d)| (*d).to_owned());
        let name = &self.program.seqs[seq].name.name;
        Some(markdown(&format!("{name}.{field}: {ty} = {value}"), [doc]))
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
rill osc(freq: Freq, gain: Gain = -6dB) Sample {
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
    fn sequences() {
        let src = |at: &str| {
            "// The main riff.\nseq riff(step: 1/8, tempo: 120bpm) { C4, _, E4 }\nevent lead note_on(sender: riff)\nrill v() Sample {\n    on lead(n) claim(tail: 2s) { }\n    on start { let id = invoke riff\n    trigger 2 id riff\n    halt riff }\n    return 0\n}"
                .replacen(at, &format!("${at}"), 1)
        };
        let decl = "```rill\nseq riff(step: 1/8, tempo: 120bpm)\n```\n\nThe main riff.\n\nA sequence of 3 steps.";
        assert!(hover(&src("riff(step")).unwrap().starts_with(decl));
        assert!(
            hover(&src("riff)")).unwrap().starts_with(decl),
            "from a sender filter"
        );
        assert!(
            hover(&src("riff\n")).unwrap().starts_with(decl),
            "from `invoke`"
        );
        for (word, doc) in [
            ("seq riff", "Declares a sequence"),
            ("invoke", "Starts a sequence"),
            ("trigger", "Starts a sequence at a step"),
            ("halt", "Stops an instance"),
            ("claim", "Makes a `note_on` handler run in one voice"),
            ("start", "A built-in event"),
        ] {
            let h = hover(&src(word)).unwrap_or_default();
            assert!(h.starts_with(doc), "{word}: {h}");
        }
    }

    #[test]
    fn events() {
        let src = |cursor_in: &str| {
            "// The keyboard on the left.\nevent keys note_on(sender: 5, channel: 1)\nevent knob control_change\nrill main() Sample {\n    state x: Float = 0\n    on keys(note) { x = note.velocity }\n    on knob(v) { x = v }\n    return 0\n}"
                .replacen(cursor_in, &format!("${cursor_in}"), 1)
        };
        // The declaration, from its name or from a handler.
        let decl = "```rill\nevent keys note_on(sender: 5, channel: 1)\n```\n\nThe keyboard on the left.\n\nA note starts.";
        assert!(hover(&src("keys note_on")).unwrap().starts_with(decl));
        assert!(hover(&src("keys(note)")).unwrap().starts_with(decl));
        // The kind.
        assert!(
            hover(&src("note_on"))
                .unwrap()
                .starts_with("```rill\nnote_on\n```\n\nA note starts.")
        );
        // The payload.
        assert_eq!(
            hover(&src("note)")).unwrap(),
            "```rill\nnote: NoteOn\n```\n\nthe payload of event `keys`\n\nWhat a `note_on` event's handler receives: `pitch: Pitch` and `velocity: Float` (0–1)."
        );
        assert!(
            hover(&src("v)"))
                .unwrap()
                .starts_with("```rill\nv: Float\n```\n\nthe payload of event `knob`")
        );
        // The keywords.
        assert!(
            hover(&src("event keys"))
                .unwrap()
                .starts_with("Declares an event")
        );
        assert!(
            hover(&src("on keys"))
                .unwrap()
                .starts_with("Handles a declared event")
        );
    }

    #[test]
    fn definitions_show_signature_and_doc() {
        assert_eq!(
            at("osc").unwrap(),
            "```rill\nrill osc(freq: Freq, gain: Gain = -6dB) Sample\n```\n\nTurns a frequency into a sine.\nSmooth."
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
            sin.starts_with("```rill\nfn sin(x: T) T\n```\n\nSine of `x`"),
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
    fn a_sequences_events_and_fields() {
        let src = "seq riff(meter: 3/4, step: 1/8) { C4, _, E4 }\n\
                   rill main() Sample {\n    on start { invoke riff }\n    on riff_step(s) { }\n    let n = riff.step_count\n    return 0\n}";
        let at = |needle: &str| {
            let i = src.find(needle).unwrap();
            hover(&format!("{}${}", &src[..i], &src[i..]))
        };
        let step = at("riff_step").unwrap();
        assert!(
            step.starts_with("```rill\nriff_step: SeqStep\n```"),
            "{step}"
        );
        assert!(
            step.contains("Every step starts, rests included. Made by sequence `riff`."),
            "{step}"
        );
        assert!(
            step.contains("`instance: Int`, `step: Int`, `pass: Int`"),
            "{step}"
        );
        let field = at("step_count").unwrap();
        assert!(
            field.starts_with("```rill\nriff.step_count: Int = 3\n```"),
            "{field}"
        );
        assert!(field.contains("rests included"), "{field}");
    }

    #[test]
    fn each_and_random() {
        let src = "rill osc(freq: Freq, offset: Float = 0) Sample { return sin(offset) }\n\
                   rill main() Sample {\n    let each = 0.5\n    let v = [110Hz, 220Hz] |> osc(offset: each random())\n    return osc(1Hz, each)\n}";
        let at = |needle: &str, nth: usize| {
            let i = src.match_indices(needle).nth(nth).unwrap().0;
            hover(&format!("{}${}", &src[..i], &src[i..]))
        };
        // The keyword before an argument, and a name elsewhere.
        assert!(at("each", 1).unwrap().contains("once per copy"));
        let name = at("each", 2).unwrap();
        assert!(name.starts_with("```rill\nlet each:"), "{name}");
        let random = at("random", 0).unwrap();
        assert!(random.contains("fn random() Float"), "{random}");
        assert!(random.contains("fn random(lo: S, hi: S) S"), "{random}");
        assert!(
            random.contains("once, when the program is built"),
            "{random}"
        );
    }

    #[test]
    fn levels_subtracted_from_a_signal() {
        let h = |line: &str| {
            hover(&format!(
                "rill main(voice: Sample, g: Gain) Sample {{\n    {line}\n}}"
            ))
            .unwrap()
        };
        let sub = h("return voice - $6dB");
        assert!(
            sub.contains("Subtracted from a signal here: ×0.5012 in amplitude"),
            "{sub}"
        );
        assert!(
            sub.contains("On its own, 6dB = ×1.9953 in amplitude"),
            "{sub}"
        );
        // Subtracting a negative level raises the signal.
        let neg = h("return voice - -$6dB");
        assert!(neg.contains("here: ×1.9953"), "{neg}");
        assert!(neg.contains("On its own, -6dB = ×0.5012"), "{neg}");
        // Adding, or level arithmetic: the level as it is.
        assert!(h("return voice + $6dB").contains("= ×1.9953 in amplitude"));
        let levels = h("let x = g - $6dB\n    return voice + x");
        assert!(levels.contains("= ×1.9953 in amplitude"), "{levels}");
        assert!(!levels.contains("Subtracted"), "{levels}");
        // Frames are signals too.
        let frame = h("return [voice, voice] - $6dB");
        assert!(frame.contains("here: ×0.5012"), "{frame}");
    }

    #[test]
    fn named_arguments_of_builtins() {
        let h = hover("rill main() Sample { return sin(equal(A4, a$4: 432Hz) / RATE) }").unwrap();
        assert_eq!(
            h,
            "```rill\na4: Freq = 440Hz\n```\n\nparameter of built-in `equal`"
        );
    }

    #[test]
    fn broken_text() {
        let h = hover(
            "rill main(gain: Float) Sample {\n    let x = sin(1) * ga$in\n    let y = (x +\n",
        )
        .unwrap();
        assert!(h.starts_with("```rill\ngain: Float\n```"), "{h}");
    }

    #[test]
    fn consts() {
        let src = "// How many voices.\nconst VOICES = HALF * 2\nconst HALF = 4\nconst ROOT: Pitch = C3\n\
                   rill main() Sample {\n    const K = VOICES - 1\n    let x = [0; VOICES]\n    return K + ROOT / 1st\n}";
        let at = |needle: &str, nth: usize| {
            let i = src.match_indices(needle).nth(nth).unwrap().0;
            hover(&format!("{}${}", &src[..i], &src[i..]))
        };
        let voices = at("VOICES", 1).unwrap();
        assert!(voices.contains("const VOICES = HALF * 2 // 8"), "{voices}");
        assert!(voices.contains("How many voices."), "{voices}");
        assert!(at("HALF", 1).unwrap().contains("const HALF = 4\n"));
        assert!(at("ROOT", 1).unwrap().contains("const ROOT: Pitch = C3"));
        let k = at("K +", 0).unwrap();
        assert!(k.contains("const K = VOICES - 1"), "{k}");
    }
}
