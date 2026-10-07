//! Signature help: the signature of the call around the cursor.

use rill::lang::builtins;
use rill::lang::lexer::TokenKind;
use rill::lang::types::Type;

use super::Analysis;
use super::index::Target;
use super::render::{self, Rendered};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureInfo {
    pub rendered: Rendered,
    /// Markdown.
    pub doc: Option<String>,
    /// Markdown, per parameter.
    pub param_docs: Vec<Option<String>>,
    /// Parameter names, for matching named arguments.
    pub param_names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureHelpInfo {
    pub signatures: Vec<SignatureInfo>,
    pub active_signature: usize,
    /// `None` when the cursor is past the last parameter.
    pub active_parameter: Option<usize>,
}

impl Analysis {
    pub fn signature_help(&self, offset: u32) -> Option<SignatureHelpInfo> {
        let (name, callee_start, index, arg_name) = match self.call_at(offset) {
            Some(call) => (
                call.callee.name.clone(),
                call.callee.span.start,
                call.arg_index,
                call.arg_name.map(str::to_owned),
            ),
            None => {
                let (callee, commas, named) = self.call_at_tokens(offset)?;
                let i = self.tokens.partition_point(|t| t.span.start < callee.start);
                let piped = i > 0 && self.tokens[i - 1].kind == TokenKind::Pipe;
                (
                    self.slice(callee).to_owned(),
                    callee.start,
                    commas + usize::from(piped),
                    named.map(|n| self.slice(n).to_owned()),
                )
            }
        };

        let signatures = self.signatures_of(&name, callee_start)?;
        let active_signature = signatures
            .iter()
            .position(|s| match &arg_name {
                Some(n) => s.param_names.contains(n),
                None => index < s.param_names.len(),
            })
            .unwrap_or(0);
        let sig = &signatures[active_signature];
        let active_parameter = match &arg_name {
            Some(n) => sig.param_names.iter().position(|p| p == n),
            None => (index < sig.param_names.len()).then_some(index),
        };
        Some(SignatureHelpInfo {
            signatures,
            active_signature,
            active_parameter,
        })
    }

    /// The signatures of whatever `name`, called at `at`, refers to.
    fn signatures_of(&self, name: &str, at: u32) -> Option<Vec<SignatureInfo>> {
        let target = match self.index.at(at) {
            Some(occ) => occ.target.clone(),
            // Not in the index: the call is in text too broken to check.
            None => self.target_by_name(name, at)?,
        };
        Some(match target {
            Target::Def(i) => {
                let def = self.def(i);
                let param_docs = def
                    .params
                    .iter()
                    .map(|p| {
                        let b = self
                            .checked
                            .bindings
                            .iter()
                            .find(|b| b.span == p.name.span)?;
                        self.binding_doc(b)
                    })
                    .collect();
                vec![SignatureInfo {
                    rendered: self.render_def(i),
                    doc: self.def_doc(i),
                    param_docs,
                    param_names: def.params.iter().map(|p| p.name.name.clone()).collect(),
                }]
            }
            Target::Builtin(name) => {
                let doc = builtins::doc(&name).map(|d| {
                    let note = render::type_params_note(&builtins::lookup(&name));
                    match note {
                        Some(n) => format!("{d}\n\n{n}"),
                        None => d.to_owned(),
                    }
                });
                builtins::lookup(&name)
                    .into_iter()
                    .zip(render::builtin(&name))
                    .map(|(sig, rendered)| SignatureInfo {
                        rendered,
                        doc: doc.clone(),
                        param_docs: vec![None; sig.params.len()],
                        param_names: sig.params.iter().map(|p| p.name.clone()).collect(),
                    })
                    .collect()
            }
            Target::Binding(id) => {
                // A variable holding a function: its parameters have no names.
                let b = &self.checked.bindings[id];
                let Type::Fn(params, ret) = &b.ty else {
                    return None;
                };
                let mut label = format!("{}(", b.name);
                let mut spans = Vec::new();
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        label += ", ";
                    }
                    let start = label.len();
                    label += &p.to_string();
                    spans.push((start, label.len()));
                }
                label += &format!(") -> {ret}");
                vec![SignatureInfo {
                    rendered: Rendered {
                        label,
                        params: spans,
                    },
                    doc: None,
                    param_docs: vec![None; params.len()],
                    param_names: vec![String::new(); params.len()],
                }]
            }
            _ => return None,
        })
    }

    /// What `name` would refer to at `at`, going by name alone.
    pub fn target_by_name(&self, name: &str, at: u32) -> Option<Target> {
        let local = self
            .checked
            .bindings
            .iter()
            .enumerate()
            .filter(|(_, b)| b.name == name && b.scope.start <= at && at <= b.scope.end)
            .max_by_key(|(_, b)| b.scope.start);
        if let Some((id, _)) = local {
            return Some(Target::Binding(id));
        }
        if let Some(i) = self
            .program
            .items
            .iter()
            .position(|i| i.def().name.name == name)
        {
            return Some(Target::Def(i));
        }
        (!builtins::lookup(name).is_empty()).then(|| Target::Builtin(name.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn help(src: &str) -> Option<(String, Option<String>)> {
        let offset = src.find('$').expect("a `$` cursor") as u32;
        let h = Analysis::new(&src.replacen('$', "", 1)).signature_help(offset)?;
        let sig = &h.signatures[h.active_signature];
        let param = h.active_parameter.map(|p| {
            let (s, e) = sig.rendered.params[p];
            sig.rendered.label[s..e].to_owned()
        });
        Some((sig.rendered.label.clone(), param))
    }

    fn active(src: &str) -> Option<String> {
        help(src).and_then(|h| h.1)
    }

    const DEFS: &str = "\
// Clips softly.
rill clip(x: Sample, drive: Float = 2, // how hard
          ceiling: Float = 1) -> Sample { return tanh(x * drive) * ceiling }
";

    #[test]
    fn positional_arguments() {
        let src = |call: &str| format!("{DEFS}rill main() -> Sample {{ return {call} }}");
        assert_eq!(active(&src("clip($0.5)")).as_deref(), Some("x: Sample"));
        assert_eq!(
            active(&src("clip(0.5, $3)")).as_deref(),
            Some("drive: Float = 2")
        );
        assert_eq!(
            active(&src("clip(0.5,$)")).as_deref(),
            Some("drive: Float = 2")
        );
        assert_eq!(active(&src("clip(0.5 $, 3)")).as_deref(), Some("x: Sample"));
        assert_eq!(active(&src("clip(0.5, 3, 1, $9)")), None);
        // Outside the parentheses: no help.
        assert_eq!(help(&src("clip(0.5)$")), None);
        assert_eq!(help(&src("cl$ip(0.5)")), None);
    }

    #[test]
    fn named_and_piped_arguments() {
        let src = |call: &str| format!("{DEFS}rill main() -> Sample {{ return {call} }}");
        assert_eq!(
            active(&src("clip(0.5, ceiling: $1)")).as_deref(),
            Some("ceiling: Float = 1")
        );
        assert_eq!(
            active(&src("0.5 |> clip($3)")).as_deref(),
            Some("drive: Float = 2")
        );
    }

    #[test]
    fn broken_calls_use_the_tokens() {
        let src = format!("{DEFS}rill main() -> Sample {{\n    let y = clip(0.5, $\n");
        assert_eq!(active(&src).as_deref(), Some("drive: Float = 2"));
        let src = format!("{DEFS}rill main() -> Sample {{\n    let y = 0.5 |> clip($\n");
        assert_eq!(active(&src).as_deref(), Some("drive: Float = 2"));
        let src = format!("{DEFS}rill main() -> Sample {{\n    let y = clip(0.5, ceiling: $\n");
        assert_eq!(active(&src).as_deref(), Some("ceiling: Float = 1"));
    }

    #[test]
    fn builtin_overloads_pick_by_argument_count() {
        let src = |call: &str| format!("rill main(a: Sample) -> Sample {{ return {call} }}");
        let (label, p) = help(&src("max($a)")).unwrap();
        assert_eq!(
            (label.as_str(), p.as_deref()),
            ("fn max<N>(x: [T; N]) -> T", Some("x: [T; N]"))
        );
        let (label, p) = help(&src("max(a, $a)")).unwrap();
        assert_eq!(
            (label.as_str(), p.as_deref()),
            ("fn max(a: S, b: S) -> S", Some("b: S"))
        );
        assert_eq!(
            active(&src("sin(equal(A4, a4: $440Hz))")).as_deref(),
            Some("a4: Freq = 440Hz")
        );
    }

    #[test]
    fn docs_come_along() {
        let offset = DEFS.len() as u32 + "rill main() -> Sample { return clip(".len() as u32;
        let a = Analysis::new(&format!("{DEFS}rill main() -> Sample {{ return clip(1) }}"));
        let h = a.signature_help(offset).unwrap();
        let sig = &h.signatures[0];
        assert_eq!(sig.doc.as_deref(), Some("Clips softly."));
        assert_eq!(sig.param_docs[1].as_deref(), Some("how hard"));
    }

    #[test]
    fn function_values() {
        let src = "rill main(f: fn(Sample, Float) -> Sample) -> Sample { return f(1, $2) }";
        assert_eq!(
            help(src),
            Some(("f(Sample, Float) -> Sample".into(), Some("Float".into())))
        );
    }
}
