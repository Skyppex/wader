//! Documentation: `//` comments in the source, and the built-ins' docs.

/// The doc comment for a declaration starting at `at`: the `//` lines
/// directly above its line, or failing that a `//` comment at the end of
/// the line it ends on (`end`), as is usual for parameters.
pub fn comment(text: &str, at: u32, end: u32) -> Option<String> {
    let line_start = text[..at as usize].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[..line_start];

    // Only comments directly above, and only if nothing else on the
    // declaration's line comes before it (`a: Float, b: Float` shares a line).
    let mut above = Vec::new();
    if text[line_start..at as usize].trim().is_empty() {
        for line in before.lines().rev() {
            match line.trim_start().strip_prefix("//") {
                Some(c) => above.push(c.strip_prefix(' ').unwrap_or(c).trim_end()),
                None => break,
            }
        }
    }
    if !above.is_empty() {
        above.reverse();
        return Some(above.join("\n"));
    }

    let end = end as usize;
    let line_end = text[end..].find('\n').map_or(text.len(), |i| end + i);
    let rest = &text[end..line_end];
    // A trailing comment, after at most a separator.
    let rest = rest.trim_start().trim_start_matches([',', '{', ')']).trim_start();
    let c = rest.strip_prefix("//")?;
    Some(c.trim().to_owned()).filter(|c| !c.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_of(text: &str, needle: &str) -> Option<String> {
        let at = text.find(needle).unwrap() as u32;
        comment(text, at, at + needle.len() as u32)
    }

    #[test]
    fn lines_above() {
        let text = "// header\n\n// Makes a sine.\n// Very smooth.\nrill sine() {}\n";
        assert_eq!(doc_of(text, "rill sine").as_deref(), Some("Makes a sine.\nVery smooth."));
        assert_eq!(doc_of("rill sine() {}", "rill sine"), None);
        // A blank line ends the comment.
        assert_eq!(doc_of("// far away\n\nrill sine() {}", "rill sine"), None);
    }

    #[test]
    fn trailing_comments_for_params() {
        let text = "rill crush(\n    x: Sample,\n    bits: Float = 8, // bit depth\n    // how long to hold\n    hold: Time\n)";
        assert_eq!(doc_of(text, "bits: Float = 8").as_deref(), Some("bit depth"));
        assert_eq!(doc_of(text, "hold: Time").as_deref(), Some("how long to hold"));
        assert_eq!(doc_of(text, "x: Sample"), None);
        // Something else earlier on the line: no comment from above.
        let text = "// about a\nrill f(a: Float, b: Float)";
        assert_eq!(doc_of(text, "b: Float"), None);
    }
}
