//! Renaming fns, rills, parameters and variables.

use std::collections::HashMap;

use rill::lang::Span;
use rill::lang::lexer::{self, TokenKind};

use super::Analysis;
use super::index::Target;

/// Why a rename cannot happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenameError(pub String);

impl Analysis {
    /// The name a rename at `offset` would change. `Ok(None)` when there is
    /// no name there; an error for names that cannot be renamed.
    pub fn prepare_rename(&self, offset: u32) -> Result<Option<(Span, String)>, RenameError> {
        let Some(occ) = self.index.at(offset) else {
            return Ok(None);
        };
        let name = self.slice(occ.span).to_owned();
        let what = match &occ.target {
            Target::Def(_) | Target::Binding(_) => return Ok(Some((occ.span, name))),
            Target::Builtin(_) => "built-in function",
            Target::Constant(_) => "built-in constant",
            Target::Note(_) => "note name",
            Target::Type(_) => "built-in type",
        };
        Err(RenameError(format!("`{name}` is a {what} and cannot be renamed")))
    }

    /// The edits renaming the name at `offset` to `new_name`, sorted by
    /// position.
    pub fn rename(&self, offset: u32, new_name: &str) -> Result<Vec<(Span, String)>, RenameError> {
        let Some((_, old_name)) = self.prepare_rename(offset)? else {
            return Err(RenameError("there is no name here to rename".into()));
        };
        valid_name(new_name)?;
        let target = self.index.at(offset).expect("prepared").target.clone();
        let edits: Vec<(Span, String)> = self
            .index
            .occurrences_of(&target)
            .map(|o| (o.span, new_name.to_owned()))
            .collect();
        if new_name != old_name {
            self.check_rename(&edits, &old_name, new_name)?;
        }
        Ok(edits)
    }

    /// Refuse a rename that changes what any name refers to, or adds errors.
    fn check_rename(&self, edits: &[(Span, String)], old: &str, new: &str) -> Result<(), RenameError> {
        let mut text = self.text.clone();
        for (span, name) in edits.iter().rev() {
            text.replace_range(span.start as usize..span.end as usize, name);
        }
        let after = Analysis::new(&text);
        let refused = || {
            RenameError(format!(
                "renaming `{old}` to `{new}` would change what other names refer to"
            ))
        };

        let errors = |a: &Analysis| a.diagnostics.iter().filter(|d| d.is_error()).count();
        if errors(&after) > errors(self) {
            let first = after
                .diagnostics
                .iter()
                .find(|d| d.is_error() && !self.diagnostics.iter().any(|o| o.message == d.message));
            return Err(match first {
                Some(d) => RenameError(format!("renaming `{old}` to `{new}` would cause an error: {}", d.message)),
                None => refused(),
            });
        }

        // Where an offset moves to once the edits are made.
        let delta = (new.len() as i64) - (old.len() as i64);
        let moved = |at: u32| {
            let before = edits.iter().filter(|(s, _)| s.start < at).count() as i64;
            (i64::from(at) + before * delta) as u32
        };
        let before_occ = self.index.all();
        let after_occ = after.index.all();
        if before_occ.len() != after_occ.len() {
            return Err(refused());
        }
        // Every name must point at the same thing, under its new identity.
        let mut map: HashMap<&Target, &Target> = HashMap::new();
        let mut back: HashMap<&Target, &Target> = HashMap::new();
        for o in before_occ {
            let Some(n) = after.index.at(moved(o.span.start)).filter(|n| n.span.start == moved(o.span.start)) else {
                return Err(refused());
            };
            if *map.entry(&o.target).or_insert(&n.target) != &n.target
                || *back.entry(&n.target).or_insert(&o.target) != &o.target
            {
                return Err(refused());
            }
        }
        Ok(())
    }
}

fn valid_name(name: &str) -> Result<(), RenameError> {
    let invalid = || RenameError(format!("`{name}` is not a valid name"));
    let tokens = lexer::lex(name).map_err(|_| invalid())?;
    match tokens.as_slice() {
        [t, eof] if eof.kind == TokenKind::Eof && t.span.start == 0 && t.span.end as usize == name.len() => {
            if t.kind != TokenKind::Ident {
                return Err(RenameError(format!("`{name}` is a keyword")));
            }
            Ok(())
        }
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rename(src: &str, new: &str) -> Result<String, RenameError> {
        let offset = src.find('$').expect("a `$` cursor") as u32;
        let text = src.replacen('$', "", 1);
        let a = Analysis::new(&text);
        let edits = a.rename(offset, new)?;
        let mut out = text.clone();
        for (span, name) in edits.iter().rev() {
            out.replace_range(span.start as usize..span.end as usize, name);
        }
        Ok(out)
    }

    const SRC: &str = "\
rill osc(freq: Freq) -> Sample {
    state phase: Float = 0
    phase = wrap(phase + freq / RATE)
    return sin(phase * TAU)
}
rill main() -> Sample {
    let f = 440Hz
    return osc(freq: f) + (f |> osc)
}
";

    fn cursor(needle: &str, nth: usize) -> String {
        let at = SRC.match_indices(needle).nth(nth).unwrap().0;
        format!("{}${}", &SRC[..at], &SRC[at..])
    }

    #[test]
    fn renames_every_use() {
        let out = rename(&cursor("osc", 2), "voice").unwrap();
        assert_eq!(out, SRC.replace("osc", "voice"));

        let out = rename(&cursor("phase", 1), "ph").unwrap();
        assert_eq!(out, SRC.replace("phase", "ph"));
    }

    #[test]
    fn parameters_rename_named_arguments_too() {
        let out = rename(&cursor("freq", 0), "hz").unwrap();
        assert_eq!(out, SRC.replace("freq", "hz"));
        // From the named argument, too.
        let out = rename(&cursor("freq: f", 0), "hz").unwrap();
        assert_eq!(out, SRC.replace("freq", "hz"));
    }

    #[test]
    fn names_in_parentheses() {
        let out = rename(&cursor("f |>", 0), "g").unwrap();
        assert!(out.contains("let g = 440Hz") && out.contains("(g |> osc)") && out.contains("freq: g)"), "{out}");
    }

    #[test]
    fn refuses_what_cannot_be_renamed() {
        let e = rename(&cursor("wrap", 0), "x").unwrap_err();
        assert_eq!(e.0, "`wrap` is a built-in function and cannot be renamed");
        assert!(rename(&cursor("RATE", 0), "x").is_err());
        assert!(rename(&cursor("Sample", 0), "x").is_err());
        assert!(rename(&cursor("return", 0), "x").is_err());
    }

    #[test]
    fn refuses_bad_names() {
        assert_eq!(rename(&cursor("phase", 0), "let").unwrap_err().0, "`let` is a keyword");
        for bad in ["", "a b", "1x", "a-b", "x(", "é"] {
            assert!(rename(&cursor("phase", 0), bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn refuses_renames_that_change_meaning() {
        // `freq` would hide the parameter in `phase + freq`.
        let e = rename(&cursor("phase", 0), "freq").unwrap_err();
        assert!(e.0.starts_with("renaming `phase` to `freq` would"), "{}", e.0);
        // A local named like a built-in hides it.
        assert!(rename(&cursor("phase", 0), "sin").is_err());
        // Two definitions with one name.
        assert!(rename(&cursor("osc", 0), "main").is_err());
        // A fn named like a note: calls still find the fn, but as a value
        // the name would be read as the note.
        let src = "fn $tune(p: Pitch) -> Freq { equal(p) }\nrill main() -> Sample { return sin(A4 |> tune) }";
        assert!(rename(src, "C4").unwrap().contains("|> C4"));
        let src = "fn $tune(p: Pitch) -> Freq { equal(p) }\nrill main() -> Sample {\n    let g = tune\n    return sin(A4 |> g / RATE)\n}";
        assert!(rename(src, "x").is_ok());
        assert!(rename(src, "C4").is_err());
    }

    #[test]
    fn renaming_to_the_same_name_is_a_no_op() {
        assert_eq!(rename(&cursor("osc", 0), "osc").unwrap(), SRC);
    }
}
