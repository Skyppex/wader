//! Go to definition / declaration, and find references.

use rill::lang::Span;

use super::Analysis;

impl Analysis {
    /// Where the name at `offset` is defined. Built-ins have no source, so
    /// they have no definition.
    pub fn definition(&self, offset: u32) -> Option<Span> {
        let occ = self.index.at(offset)?;
        let decl = self.index.declaration_of(&occ.target)?;
        Some(decl.span)
    }

    /// Every appearance of the name at `offset`, in order: user names and
    /// built-ins alike.
    pub fn references(&self, offset: u32, include_declaration: bool) -> Vec<Span> {
        let Some(occ) = self.index.at(offset) else {
            return Vec::new();
        };
        self.index
            .occurrences_of(&occ.target)
            .filter(|o| include_declaration || !o.is_decl)
            .map(|o| o.span)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `src` with `$` marking the cursor.
    fn at(src: &str) -> (Analysis, u32) {
        let offset = src.find('$').expect("a `$` cursor") as u32;
        (Analysis::new(&src.replacen('$', "", 1)), offset)
    }

    fn texts(a: &Analysis, spans: &[Span]) -> Vec<(usize, String)> {
        spans
            .iter()
            .map(|s| (s.start as usize, a.slice(*s).to_owned()))
            .collect()
    }

    const SRC: &str = "\
fn half(x: Sample) -> Sample { x / 2 }
rill main(freq: Freq = 440Hz) -> Sample {
    state phase: Float = 0
    phase = wrap(phase + freq / RATE)
    let s = sin(phase * TAU) |> half
    return (s) + half(s)
}
";

    fn cursor(needle: &str, nth: usize) -> String {
        let at = SRC.match_indices(needle).nth(nth).unwrap().0;
        format!("{}${}", &SRC[..at], &SRC[at..])
    }

    #[test]
    fn definitions() {
        let (a, o) = at(&cursor("half(s)", 0));
        let def = a.definition(o).unwrap();
        assert_eq!((def.start, a.slice(def)), (3, "half"));

        // From anywhere in the name, and from just after it.
        let (a, o) = at(&cursor("phase + freq", 0));
        let def = a.definition(o + 3).unwrap();
        assert_eq!(a.slice(def), "phase");
        assert_eq!(def.start as usize, SRC.find("phase:").unwrap());
        let (a, o) = at(&cursor("freq / RATE", 0));
        assert_eq!(a.definition(o + 4).map(|d| d.start as usize), SRC.find("freq:"));

        // A name in parentheses.
        let (a, o) = at(&cursor("s) + half", 0));
        assert_eq!(a.definition(o).map(|d| d.start as usize), SRC.find("s = sin"));

        // A declaration is its own definition.
        let (a, o) = at(&cursor("main", 0));
        assert_eq!(a.definition(o).map(|d| d.start as usize), SRC.find("main"));

        // Built-ins, constants, keywords and literals have none.
        for needle in ["wrap", "RATE", "state", "440Hz", "Sample"] {
            let (a, o) = at(&cursor(needle, 0));
            assert_eq!(a.definition(o), None, "{needle}");
        }
    }

    #[test]
    fn references() {
        let (a, o) = at(&cursor("half", 0));
        let refs = a.references(o, true);
        let starts: Vec<_> = SRC.match_indices("half").map(|(i, _)| i).collect();
        assert_eq!(texts(&a, &refs).iter().map(|r| r.0).collect::<Vec<_>>(), starts);
        assert_eq!(a.references(o, false).len(), starts.len() - 1);

        let (a, o) = at(&cursor("phase", 2));
        assert_eq!(a.references(o, true).len(), 4);

        // Built-ins are found too, though they have no declaration.
        let src = "rill main() -> Sample { return sin(sin(1)) }";
        let (a, o) = at(&src.replacen("sin", "$sin", 1));
        assert_eq!(a.references(o, true).len(), 2);
    }

    #[test]
    fn named_arguments_refer_to_the_parameter() {
        let src = "rill osc(freq: Freq) -> Sample { return sin(freq / RATE) }\nrill main() -> Sample { return osc(fr$eq: 440Hz) }";
        let (a, o) = at(src);
        let def = a.definition(o).unwrap();
        assert_eq!(def.start as usize, src.find("freq").unwrap());
        assert_eq!(a.references(o, true).len(), 3);
    }

    #[test]
    fn works_while_the_text_is_broken() {
        let src = "rill main(gain: Float) -> Sample {\n    let x = sin(1) * ga$in\n    let y = (x +\n    return x\n";
        let (a, o) = at(src);
        assert_eq!(a.definition(o).map(|d| d.start as usize), src.find("gain"));
    }
}
