//! Go to definition / declaration and find references, end to end.

mod common;

use common::{at, open, range, uri};
use wader::lsp::{self, request as r};

const SRC: &str = "\
// 🎵 a note before anything, to shift UTF-16 columns on this line only
rill osc(freq: Freq) -> Sample {
    state phase: Float = 0
    phase = wrap(phase + freq / RATE)
    return sin(phase * TAU)
}
rill main() -> Sample {
    let /*🎵*/ f = 440Hz
    return osc(freq: f) + $osc(f)
}
";

#[test]
fn definition_and_declaration() {
    let (mut client, pos) = open(SRC);
    let expected = vec![lsp::Location {
        uri: uri(),
        range: range(1, 5, 1, 8),
    }];
    assert_eq!(
        client.request::<r::GotoDefinition>(at(pos)).unwrap(),
        Some(expected.clone())
    );
    assert_eq!(
        client.request::<r::GotoDeclaration>(at(pos)).unwrap(),
        Some(expected)
    );
}

#[test]
fn utf16_columns_after_a_wide_character() {
    // `f` in `osc(f)`, declared after a comment holding a note: 2 UTF-16 units.
    let (mut client, pos) = open(&SRC.replace("$osc(f)", "osc($f)"));
    let locs = client
        .request::<r::GotoDefinition>(at(pos))
        .unwrap()
        .unwrap();
    let col = "    let /*".len() as u32 + 2 + "*/ ".len() as u32;
    assert_eq!(locs[0].range, range(7, col, 7, col + 1));
}

#[test]
fn references_with_and_without_the_declaration() {
    let (mut client, pos) = open(SRC);
    let refs = |client: &mut wader::client::Client, include_declaration| {
        client
            .request::<r::References>(lsp::ReferenceParams {
                text_document: at(pos).text_document,
                position: pos,
                context: lsp::ReferenceContext {
                    include_declaration,
                },
            })
            .unwrap()
            .unwrap()
    };
    let all = refs(&mut client, true);
    assert_eq!(
        all.iter().map(|l| l.range.start.line).collect::<Vec<_>>(),
        [1, 8, 8]
    );
    assert_eq!(refs(&mut client, false).len(), 2);
}

#[test]
fn nothing_at_a_keyword() {
    let (mut client, pos) = open(
        &SRC.replace("$osc(f)", "osc(f)")
            .replace("return sin", "$return sin"),
    );
    assert_eq!(client.request::<r::GotoDefinition>(at(pos)).unwrap(), None);
}

#[test]
fn unopened_documents_are_an_error() {
    let mut client = wader::client::Client::new();
    let e = client
        .request::<r::GotoDefinition>(at(lsp::Position::new(0, 0)))
        .unwrap_err();
    assert!(e.message.contains("not open"), "{}", e.message);
}
