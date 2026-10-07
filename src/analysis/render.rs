//! Signatures and other text shown to the user.

use rill::lang::builtins;
use rill::lang::check::{Binding, BindingKind};
use rill::lang::types::{DefKind, Signature, Type};

use super::{Analysis, docs};

/// A signature as text, with each parameter's span (byte offsets into the
/// text), for highlighting the active one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    pub label: String,
    pub params: Vec<(usize, usize)>,
}

fn render(sig: &Signature, keyword: &str, default: impl Fn(usize) -> Option<String>) -> Rendered {
    let mut label = format!("{keyword} {}", sig.name);
    if !sig.generics.is_empty() {
        label += &format!("<{}>", sig.generics.join(", "));
    }
    label.push('(');
    let mut params = Vec::new();
    for (i, p) in sig.params.iter().enumerate() {
        if i > 0 {
            label += ", ";
        }
        let start = label.len();
        label += &format!("{}: {}", p.name, p.ty);
        if let Some(d) = default(i) {
            label += &format!(" = {d}");
        } else if p.has_default {
            label += " = ..";
        }
        params.push((start, label.len()));
    }
    label += &format!(") {}", sig.ret);
    match sig.rate {
        (1, 1) => {}
        (1, d) => label += &format!(" @ rate / {d}"),
        (n, _) => label += &format!(" @ rate * {n}"),
    }
    Rendered { label, params }
}

/// The default of a built-in's parameter, written as a literal.
fn builtin_default(name: &str, param: &str, ty: &Type) -> Option<String> {
    let v = builtins::default_value(name, param)?;
    Some(match ty {
        Type::Freq => format!("{v}Hz"),
        Type::Time => format!("{v}s"),
        _ => format!("{v}"),
    })
}

/// What `T` and `S` mean in built-in signatures, if `sig` uses them.
pub fn type_params_note(sigs: &[Signature]) -> Option<String> {
    fn mentions(t: &Type, param: &str) -> bool {
        match t {
            Type::Param(p) => *p == param,
            Type::Frame(elem, _) => mentions(elem, param),
            Type::Fn(ps, r) => ps.iter().any(|p| mentions(p, param)) || mentions(r, param),
            _ => false,
        }
    }
    let uses = |param: &str| {
        sigs.iter()
            .any(|s| mentions(&s.ret, param) || s.params.iter().any(|p| mentions(&p.ty, param)))
    };
    let mut notes = Vec::new();
    if uses("T") {
        notes.push("`T` is a plain number: `Sample`, `Float` or `Int`.");
    }
    if uses("S") {
        notes.push("`S` is any number, with or without a unit, or a `Pitch`.");
    }
    if uses("F") {
        notes.push("`F` is a plain number, or a frame of them.");
    }
    (!notes.is_empty()).then(|| notes.join(" "))
}

impl Analysis {
    /// The signature of the fn or rill at `index`, defaults as written.
    pub fn render_def(&self, index: usize) -> Rendered {
        let def = self.def(index);
        let sig = &self.checked.signatures[index];
        let keyword = if sig.kind == DefKind::Rill {
            "rill"
        } else {
            "fn"
        };
        render(sig, keyword, |i| {
            let d = def.params.get(i)?.default.as_ref()?;
            Some(self.slice(d.span).to_owned())
        })
    }

    /// The doc comment of the fn or rill at `index`.
    pub fn def_doc(&self, index: usize) -> Option<String> {
        let def = self.def(index);
        docs::comment(&self.text, def.span.start, def.name.span.end)
    }

    /// The doc comment of a binding: above its line, or trailing it.
    pub fn binding_doc(&self, b: &Binding) -> Option<String> {
        let def = self.def(b.def);
        // A parameter ends with its type and default; a `let` or `state`
        // statement starts with its keyword.
        let (start, end) = match b.kind {
            BindingKind::Param => {
                let p = def.params.iter().find(|p| p.name.span == b.span)?;
                let end = p.default.as_ref().map_or(p.ty.span().end, |d| d.span.end);
                (p.name.span.start, end)
            }
            BindingKind::Let | BindingKind::State => {
                let line_start = self.text[..b.span.start as usize]
                    .rfind('\n')
                    .map_or(0, |i| i + 1);
                let keyword_at = line_start
                    + (self.text[line_start..].len() - self.text[line_start..].trim_start().len());
                (keyword_at as u32, b.scope.start)
            }
            BindingKind::Size | BindingKind::FnParam => return None,
        };
        docs::comment(&self.text, start, end)
    }

    /// How a binding is declared, as code: `let a: Sample`.
    pub fn render_binding(&self, b: &Binding) -> String {
        match b.kind {
            BindingKind::Param => {
                let def = self.def(b.def);
                let default = def
                    .params
                    .iter()
                    .find(|p| p.name.span == b.span)
                    .and_then(|p| p.default.as_ref())
                    .map(|d| format!(" = {}", self.slice(d.span)))
                    .unwrap_or_default();
                format!("{}: {}{default}", b.name, b.ty)
            }
            BindingKind::Size => format!("<{}>", b.name),
            BindingKind::Let => format!("let {}: {}", b.name, b.ty),
            BindingKind::State => format!("state {}: {}", b.name, b.ty),
            BindingKind::FnParam => format!("{}: {}", b.name, b.ty),
        }
    }
}

/// Every signature of the built-in `name`, defaults filled in.
pub fn builtin(name: &str) -> Vec<Rendered> {
    builtins::lookup(name)
        .iter()
        .map(|sig| {
            render(sig, "fn", |i| {
                let p = &sig.params[i];
                builtin_default(name, &p.name, &p.ty)
            })
        })
        .collect()
}

/// What a built-in type is.
pub fn type_doc(name: &str) -> Option<&'static str> {
    Some(match name {
        "Sample" => "One audio value, nominally in [-1.0, 1.0]. Its storage is chosen per target.",
        "Float" => "A plain number for control logic. Not converted at I/O.",
        "Int" => "A whole number for control logic.",
        "Bool" => "`true` or `false`.",
        "Freq" => "A frequency, written in `Hz` or `kHz`. Converts to samples via the host rate.",
        "Time" => "A duration, written in `s` or `ms`. Converts to samples via the host rate.",
        "Pitch" => {
            "A position in pitch, written as a note name like `A4` or `F#3`. Becomes a `Freq` through a tuning such as `equal`."
        }
        "Interval" => {
            "A distance in pitch, written in `st` (semitones) or `cents`. `Pitch ± Interval` gives a `Pitch`."
        }
        "Gain" => {
            "A level change, written in `dB`. `x + 6dB` makes `x` louder; any plain number is an amplitude factor."
        }
        _ => return None,
    })
}

/// What a keyword does.
pub fn keyword_doc(keyword: &str) -> Option<&'static str> {
    Some(match keyword {
        "fn" => {
            "Defines a pure function: it takes exactly what it declares and keeps no state. Without a name, `fn(x) { ... }` is an anonymous fn."
        }
        "rill" => {
            "Defines a stream processor. Its body runs once per tick and may keep `state`. A rill taking `Sample` can be applied to a frame; it then runs once per channel."
        }
        "state" => {
            "A variable that keeps its value between ticks. Only at the top of a rill body, with a constant initial value."
        }
        "let" => "Binds a value. A stream bound with `let` and used twice is one instance.",
        "return" => {
            "Produces the output for this tick. Every path through a rill returns exactly once."
        }
        "if" | "else" => {
            "Chooses between two values; both branches have the same type. Without `else`, it produces nothing."
        }
        "as" => "Converts between `Sample`, `Float` and `Int`. Units never disappear by a cast.",
        "true" | "false" => "A `Bool`.",
        _ => return None,
    })
}

/// A short description of a unit.
pub fn unit_doc(unit: &str) -> Option<&'static str> {
    Some(match unit {
        "Hz" => "hertz",
        "kHz" => "kilohertz",
        "s" => "seconds",
        "ms" => "milliseconds",
        "st" => "semitones",
        "cents" => "hundredths of a semitone",
        "dB" => "decibels",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_signatures_keep_their_defaults() {
        let a = Analysis::new(
            "rill crush<N>(x: [Sample; N], bits: Float = 8, hold: Time = 0.5ms) Sample @ rate / 2 { return x[0] }",
        );
        let r = a.render_def(0);
        assert_eq!(
            r.label,
            "rill crush<N>(x: [Sample; N], bits: Float = 8, hold: Time = 0.5ms) Sample @ rate / 2"
        );
        let params: Vec<&str> = r.params.iter().map(|&(s, e)| &r.label[s..e]).collect();
        assert_eq!(
            params,
            ["x: [Sample; N]", "bits: Float = 8", "hold: Time = 0.5ms"]
        );
    }

    #[test]
    fn builtins() {
        let equal = builtin("equal");
        assert_eq!(
            equal[0].label,
            "fn equal(pitch: Pitch, steps: Int = 12, a4: Freq = 440Hz) Freq"
        );
        let min: Vec<String> = builtin("min").into_iter().map(|r| r.label).collect();
        assert_eq!(
            min,
            ["fn min<N>(x: [F; N]) F", "fn min(a: S, b: S) S"]
        );
        assert!(
            type_params_note(&builtins::lookup("min"))
                .unwrap()
                .contains("`S`")
        );
        assert!(
            type_params_note(&builtins::lookup("sum"))
                .unwrap()
                .contains("`F` is a plain number, or a frame of them")
        );
        assert_eq!(type_params_note(&builtins::lookup("decay")), None);
    }
}
