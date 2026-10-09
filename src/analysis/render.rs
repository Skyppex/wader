//! Signatures and other text shown to the user.

use rill::event::EventKind;
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
            BindingKind::Let | BindingKind::State | BindingKind::Const => {
                let line_start = self.text[..b.span.start as usize]
                    .rfind('\n')
                    .map_or(0, |i| i + 1);
                let keyword_at = line_start
                    + (self.text[line_start..].len() - self.text[line_start..].trim_start().len());
                (keyword_at as u32, b.scope.start)
            }
            BindingKind::Size | BindingKind::FnParam | BindingKind::EventParam => return None,
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
            BindingKind::Const => {
                // A `const` is visible from the end of its declaration.
                let decl = &self.text[b.span.end as usize..b.scope.start as usize];
                let value = decl.split_once('=').map_or("", |(_, v)| v.trim());
                const_label(&b.name, &b.ty, value, None)
            }
            BindingKind::FnParam | BindingKind::EventParam => format!("{}: {}", b.name, b.ty),
        }
    }

    /// An event declaration as code: `event keys note_on(channel: 1)`. For
    /// one a sequence makes, its name and payload: `riff_step: SeqStep`.
    pub fn render_event(&self, index: usize) -> String {
        if let Some((_, kind)) = self.checked.seq_event(index) {
            let name = self.checked.events[index]
                .as_ref()
                .map_or("", |d| d.name.as_str());
            return format!("{name}: {}", kind.type_name());
        }
        let e = &self.program.events[index];
        let mut out = format!("event {} {}", e.name.name, e.kind.name);
        if !e.filters.is_empty() {
            let filters: Vec<String> = e
                .filters
                .iter()
                .map(|f| format!("{}: {}", f.name.name, self.slice(f.value.span)))
                .collect();
            out += &format!("({})", filters.join(", "));
        }
        out
    }

    /// A sequence's header as code: `seq riff(step: 1/8, tempo: 120bpm)`.
    pub fn render_seq(&self, index: usize) -> String {
        let s = &self.program.seqs[index];
        let mut out = format!("seq {}", s.name.name);
        if !s.settings.is_empty() {
            let settings: Vec<String> = s
                .settings
                .iter()
                .map(|f| format!("{}: {}", f.name.name, self.slice(f.value.span)))
                .collect();
            out += &format!("({})", settings.join(", "));
        }
        out
    }

    /// A top-level `const` as code: `const VOICES = 8`, with its value when
    /// it is worked out from others: `const VOICES = HALF * 2 // 8`.
    pub fn render_const(&self, index: usize) -> String {
        let c = &self.program.consts[index];
        let info = self.checked.consts.get(index);
        let ty = info.map_or(rill::lang::types::Type::Error, |i| i.ty.clone());
        let value = info.and_then(|i| i.value);
        const_label(&c.name.name, &ty, self.slice(c.value.span), value)
    }

    /// The doc comment of the top-level `const` at `index`.
    pub fn const_doc(&self, index: usize) -> Option<String> {
        let c = &self.program.consts[index];
        docs::comment(&self.text, c.span.start, c.name.span.end)
    }

    /// The doc comment of the sequence at `index`.
    pub fn seq_doc(&self, index: usize) -> Option<String> {
        let s = &self.program.seqs[index];
        docs::comment(&self.text, s.span.start, s.name.span.end)
    }

    /// The doc comment of the event declaration at `index`; for one a
    /// sequence makes, what it is.
    pub fn event_doc(&self, index: usize) -> Option<String> {
        if let Some((seq, kind)) = self.checked.seq_event(index) {
            let seq = &self.program.seqs[seq].name.name;
            return Some(format!(
                "{} Made by sequence `{seq}`. {}",
                seq_event_doc(kind),
                payload_doc(kind)
            ));
        }
        let e = &self.program.events[index];
        docs::comment(&self.text, e.span.start, e.name.span.end)
    }

    /// The `on` handler whose payload parameter is declared at `span`.
    pub fn handler_of(&self, b: &Binding) -> Option<&rill::lang::ast::Ident> {
        let mut found = None;
        find_handlers(&self.def(b.def).body, &mut |name, params| {
            if params.first().is_some_and(|p| p.span == b.span) {
                found = Some(name);
            }
        });
        found
    }
}

/// Call `f` with the event name and parameters of every `on` handler in
/// `block`, nested ones included.
pub fn find_handlers<'a>(
    block: &'a rill::lang::ast::Block,
    f: &mut impl FnMut(&'a rill::lang::ast::Ident, &'a [rill::lang::ast::Ident]),
) {
    use rill::lang::ast::{ExprKind, Stmt};
    for stmt in &block.stmts {
        if let Stmt::EventHandler {
            name, params, body, ..
        } = stmt
        {
            f(name, params);
            find_handlers(body, f);
        }
    }
    // Handlers can also sit in blocks inside expressions.
    super::locate::walk_block(block, &mut |e| {
        if let ExprKind::Block(b) | ExprKind::If { then: b, .. } = &e.kind {
            for stmt in &b.stmts {
                if let Stmt::EventHandler { name, params, .. } = stmt {
                    f(name, params);
                }
            }
        }
    });
}

/// What an event kind is and what its handlers receive.
pub fn event_kind_doc(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "note_on" => {
            "A note starts. Handlers receive a `NoteOn`: `pitch: Pitch` and `velocity: Float` (0–1)."
        }
        "note_off" => {
            "A note ends. Handlers receive a `NoteOff`: `pitch: Pitch` and `release: Float` (0–1), the release velocity."
        }
        "control_change" => {
            "A control moves: a knob, fader, pedal, pitch bend or aftertouch, one per channel. Handlers receive its value as a `Float`, as the sender sent it."
        }
        _ => return None,
    })
}

/// When a sequence makes an event of `kind`.
pub fn seq_event_doc(kind: EventKind) -> &'static str {
    match kind {
        EventKind::NoteOn => "A note of the sequence starts.",
        EventKind::NoteOff => "A note of the sequence ends.",
        EventKind::ControlChange => "",
        EventKind::Start => {
            "An instance starts playing: `invoke`, or `trigger` on a stopped one. Never when it goes round again."
        }
        EventKind::Finished => "An instance's last pass is over: it ran out of `repeat`s.",
        EventKind::Halted => {
            "An instance is stopped early: by `halt`, or by `trigger` restarting it."
        }
        EventKind::Replaced => {
            "An instance is stopped to make room: every one of `instances` was playing when another started."
        }
        EventKind::End => "An instance stops, right after `finished`, `halted` or `replaced`.",
        EventKind::Repeated => {
            "An instance goes back to step 1 for another pass, from `repeat` or `loop`."
        }
        EventKind::Step => "Every step starts, rests included.",
        EventKind::Rest => "A step without notes starts, just after its `step`.",
        EventKind::Beat => "Every beat of the meter, counted from the start of each pass.",
        EventKind::Bar => "Every bar's first beat, just before that beat.",
    }
}

/// What a handler of `kind` receives.
pub fn payload_doc(kind: EventKind) -> String {
    if kind == EventKind::ControlChange {
        return "Handlers receive its value as a `Float`.".to_owned();
    }
    let fields: Vec<String> = kind
        .fields()
        .iter()
        .map(|f| {
            let ty = match (kind, *f) {
                (_, "pitch") => "Pitch",
                (_, "velocity" | "release") => "Float",
                _ => "Int",
            };
            format!("`{f}: {ty}`")
        })
        .collect();
    format!(
        "Handlers receive a `{}`: {}.",
        kind.type_name(),
        fields.join(", ")
    )
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
        "NoteOn" => {
            "What a `note_on` event's handler receives: `pitch: Pitch` and `velocity: Float` (0–1)."
        }
        "NoteOff" => {
            "What a `note_off` event's handler receives: `pitch: Pitch` and `release: Float` (0–1), the release velocity."
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
        "const" => {
            "Names a value worked out once, when the program is built: `const VOICES = 8`. At the top level it belongs to the file; in a block, to the block. A whole number can be a size, as in `[Float; VOICES]`."
        }
        "return" => {
            "Produces the output for this tick. Every path through a rill returns exactly once."
        }
        "if" | "else" => {
            "Chooses between two values; both branches have the same type. Without `else`, it produces nothing."
        }
        "as" => "Converts between `Sample`, `Float` and `Int`. Units never disappear by a cast.",
        "seq" => {
            "Declares a sequence: a pattern of notes that plays when invoked, as in `seq riff(step: 1/8) { C4, _, E4@0.5 }`. It is a sender: `event lead note_on(sender: riff)` hears its notes."
        }
        "invoke" => {
            "Starts a sequence (`invoke riff`, `invoke id riff`) and gives its instance id, or sends a declared event (`invoke keys(pitch: C4)`). Only in `on` handlers."
        }
        "trigger" => {
            "Starts a sequence at a step, counting from 1 (`trigger 5 id riff`), restarting it if it is playing. Gives the instance id. Only in `on` handlers."
        }
        "halt" => {
            "Stops an instance of a sequence (`halt id riff`) or all of them (`halt riff`), ending the notes they hold. Only in `on` handlers."
        }
        "claim" => {
            "Makes a `note_on` handler run in one voice of a pool, which then holds the note. `claim(tail: 2s)` sets how long the voice must be silent after its release before it is free (100ms by default)."
        }
        "release" => {
            "Makes a `note_off` handler run only in the voice holding that note. The voice stays busy until its sound has died away."
        }
        "each" => {
            "Makes an argument once per copy of a rill that runs per element, instead of once for all of them: `saw(offset: each random())` gives every copy its own offset, and `each lfo(0.3Hz)` its own LFO."
        }
        "start" => "A built-in event: `on start { ... }` runs once, before the first sample.",
        "event" => {
            "Declares an event: a name, a kind (`note_on`, `note_off` or `control_change`) and optional filters, as in `event keys note_on(sender: 5, channel: 1)`. Handlers use the name."
        }
        "on" => {
            "Handles a declared event, as in `on keys(note) { ... }`. The parameter is the event's payload, typed by its kind."
        }
        "true" | "false" => "A `Bool`.",
        _ => return None,
    })
}

/// A short description of a unit.
pub fn unit_doc(unit: &str) -> Option<&'static str> {
    Some(match unit {
        "Hz" => "hertz",
        "kHz" => "kilohertz",
        "bpm" => "beats per minute: a tempo, which is a frequency (`120bpm` is `2Hz`)",
        "s" => "seconds",
        "ms" => "milliseconds",
        "st" => "semitones",
        "cents" => "hundredths of a semitone",
        "dB" => "decibels",
        _ => return None,
    })
}

/// `const NAME: Type = value`. A plain number's type is left out, as it
/// takes the type of wherever it is used. A long value is shortened, and a
/// value worked out from other names is shown after it.
fn const_label(
    name: &str,
    ty: &rill::lang::types::Type,
    value: &str,
    worked_out: Option<f64>,
) -> String {
    let ty = match ty {
        rill::lang::types::Type::Num | rill::lang::types::Type::Error => String::new(),
        t => format!(": {t}"),
    };
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = match value.chars().count() {
        0 => format!("const {name}{ty}"),
        n if n > 60 => format!(
            "const {name}{ty} = {}…",
            value.chars().take(59).collect::<String>()
        ),
        _ => format!("const {name}{ty} = {value}"),
    };
    if let Some(v) = worked_out
        && value.parse::<f64>().is_err()
    {
        out += &format!(" // {v}");
    }
    out
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
        assert_eq!(min, ["fn min<N>(x: [F; N]) F", "fn min(a: S, b: S) S"]);
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
