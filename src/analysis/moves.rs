//! Keeping imports working when files move or are renamed.

use std::path::{Component, Path, PathBuf};

use rill::lang::lexer::{self, TokenKind};
use rill::lang::module::normalize;
use rill::lang::{Span, parser};

/// Where a file or folder goes: `from` becomes `to`.
#[derive(Clone, Debug)]
pub struct Move {
    pub from: PathBuf,
    pub to: PathBuf,
}

/// Where `path` ends up after `moves`: itself, or inside a moved folder.
pub fn moved(path: &Path, moves: &[Move]) -> PathBuf {
    for m in moves {
        if let Ok(rest) = path.strip_prefix(&m.from) {
            return if rest.as_os_str().is_empty() {
                m.to.clone()
            } else {
                m.to.join(rest)
            };
        }
    }
    path.to_owned()
}

/// The edits that keep the imports of the file at `path` (its old place)
/// pointing at the same files once `moves` are made: for each import whose
/// path changes, its span (quotes included) and the new text.
pub fn import_edits(path: &Path, text: &str, moves: &[Move]) -> Vec<(Span, String)> {
    let (tokens, _) = lexer::lex_partial(text);
    // Only the imports are needed, and they are found even in a file that
    // does not parse.
    if !tokens.iter().any(|t| t.kind == TokenKind::Import) {
        return Vec::new();
    }
    let (program, _) = parser::parse_partial(text, tokens);
    let old_dir = path.parent().unwrap_or(Path::new(""));
    let new_path = moved(path, moves);
    let new_dir = new_path.parent().unwrap_or(Path::new(""));
    let mut edits = Vec::new();
    for import in &program.imports {
        // A path that is not a module name, or has an extension, is left
        // for the diagnostics to explain.
        if rill::lang::module::check_path(&import.path, import.path_span).is_err() {
            continue;
        }
        let target = normalize(&old_dir.join(format!("{}.rill", import.path)));
        let new_target = moved(&target, moves);
        let Some(rel) = relative(new_dir, &new_target) else {
            continue;
        };
        let rel = rel.strip_suffix(".rill").unwrap_or(&rel).to_owned();
        if rel != import.path {
            edits.push((import.path_span, format!("\"{rel}\"")));
        }
    }
    edits
}

/// `to` written relative to the folder `from`, with `/` between parts.
fn relative(from: &Path, to: &Path) -> Option<String> {
    let (from, to) = (normalize(from), normalize(to));
    let from: Vec<Component> = from.components().collect();
    let to: Vec<Component> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == 0
        && from
            .first()
            .is_some_and(|c| matches!(c, Component::RootDir | Component::Prefix(_)))
    {
        return None;
    }
    let mut parts: Vec<String> = vec!["..".into(); from.len() - common];
    for c in &to[common..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mv(from: &str, to: &str) -> Move {
        Move {
            from: from.into(),
            to: to.into(),
        }
    }

    fn paths(path: &str, src: &str, moves: &[Move]) -> Vec<String> {
        import_edits(Path::new(path), src, moves)
            .into_iter()
            .map(|(_, t)| t)
            .collect()
    }

    #[test]
    fn renaming_an_imported_file() {
        let src = "import \"osc\"\nimport \"fx\"\nrill main() Sample { return 0 }";
        assert_eq!(
            paths(
                "/s/song.rill",
                src,
                &[mv("/s/osc.rill", "/s/oscillators.rill")]
            ),
            ["\"oscillators\""]
        );
    }

    #[test]
    fn moving_an_imported_file() {
        let src = "import \"osc\"";
        assert_eq!(
            paths("/s/song.rill", src, &[mv("/s/osc.rill", "/s/lib/osc.rill")]),
            ["\"lib/osc\""]
        );
    }

    #[test]
    fn moving_the_importing_file() {
        let src = "import \"osc\"\nimport \"lib/fx\"";
        assert_eq!(
            paths(
                "/s/song.rill",
                src,
                &[mv("/s/song.rill", "/s/songs/song.rill")]
            ),
            ["\"../osc\"", "\"../lib/fx\""]
        );
    }

    #[test]
    fn moving_a_folder() {
        // Inside the folder nothing changes; from outside, the folder's name.
        let src = "import \"fx\"";
        assert_eq!(
            paths("/s/lib/osc.rill", src, &[mv("/s/lib", "/s/sound")]),
            Vec::<String>::new()
        );
        let src = "import \"lib/osc\"";
        assert_eq!(
            paths("/s/song.rill", src, &[mv("/s/lib", "/s/sound")]),
            ["\"sound/osc\""]
        );
    }

    #[test]
    fn imports_that_do_not_move_stay() {
        let src = "import \"osc\"\nimport \"osc.rill\"";
        assert_eq!(
            paths("/s/song.rill", src, &[mv("/s/other.rill", "/s/x.rill")]),
            Vec::<String>::new()
        );
    }
}
