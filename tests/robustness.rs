//! No feature may panic, wherever the cursor is and however broken the
//! text: every offset of every example, and the end of every prefix of
//! them (what an editor sees while the file is typed).

mod common;

use wader::analysis::Analysis;

fn every_feature(a: &Analysis, offset: u32) {
    let _ = a.hover(offset);
    let _ = a.signature_help(offset);
    let _ = a.completions(offset, true);
    let _ = a.definition(offset);
    let _ = a.references(offset, true);
    if let Ok(Some(_)) = a.prepare_rename(offset) {
        let _ = a.rename(offset, "zz");
    }
}

fn examples() -> Vec<(String, String)> {
    let dir = common::examples_dir();
    let read = |dir: std::path::PathBuf| std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path());
    read(dir.clone())
        .chain(read(dir.join("modules")))
        .filter(|p| p.extension().is_some_and(|e| e == "rill"))
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            (path.display().to_string(), text)
        })
        .collect()
}

#[test]
fn every_offset_of_every_example() {
    for (_, text) in examples() {
        let a = Analysis::new(&text);
        // The server only asks about character boundaries.
        for offset in (0..=text.len()).filter(|&i| text.is_char_boundary(i)) {
            every_feature(&a, offset as u32);
        }
    }
}

#[test]
fn while_typing() {
    for (_, text) in examples() {
        let cuts = (0..=text.len())
            .filter(|&i| text.is_char_boundary(i))
            .step_by(3);
        for cut in cuts {
            let prefix = &text[..cut];
            let a = Analysis::new(prefix);
            every_feature(&a, cut as u32);
            // And with the rest of the line removed, but later lines kept.
            let rest = text[cut..].find('\n').map_or("", |i| &text[cut + i..]);
            let edited = format!("{prefix}{rest}");
            let a = Analysis::new(&edited);
            every_feature(&a, cut as u32);
        }
    }
}
