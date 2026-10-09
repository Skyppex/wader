//! Hover, signature help, completion and rename, end to end.

mod common;

use common::{at, open, range, uri};
use wader::analysis::Analysis;
use wader::analysis::index::Target;
use wader::client::Client;
use wader::lsp::{self, request as r};

#[test]
fn capabilities_cover_every_feature() {
    let (_, result) = Client::with_capabilities(wader::client::editor_capabilities());
    let caps = result.capabilities;
    assert_eq!(caps.hover_provider, Some(true));
    assert!(caps.completion_provider.is_some());
    assert!(caps.signature_help_provider.is_some());
    assert_eq!(caps.definition_provider, Some(true));
    assert_eq!(caps.declaration_provider, Some(true));
    assert_eq!(caps.references_provider, Some(true));
    assert_eq!(
        caps.rename_provider,
        Some(lsp::RenameProvider::Options(lsp::RenameOptions {
            prepare_provider: Some(true)
        }))
    );
    // Without prepare support, just `true`.
    let (_, result) = Client::with_capabilities(lsp::ClientCapabilities::default());
    assert_eq!(
        result.capabilities.rename_provider,
        Some(lsp::RenameProvider::Bool(true))
    );
}

#[test]
fn hover() {
    let (mut client, pos) = open(
        "// Doubles.\nfn twice(x: Sample) Sample { x * 2 }\nrill main() Sample { return tw$ice(1) }",
    );
    let h = client.request::<r::HoverRequest>(at(pos)).unwrap().unwrap();
    assert_eq!(h.contents.kind, lsp::MarkupKind::Markdown);
    assert_eq!(
        h.contents.value,
        "```rill\nfn twice(x: Sample) Sample\n```\n\nDoubles."
    );
    assert_eq!(h.range, Some(range(2, 28, 2, 33)));
}

#[test]
fn signature_help_offsets_count_utf16() {
    // A default holding a note shifts the second parameter by two units.
    let src = "rill f(a: Float = 1 /*🎵*/, b: Float = 2) Sample { return a * b }\nrill main() Sample { return f(1, $) }";
    let (mut client, pos) = open(src);
    let help = client
        .request::<r::SignatureHelpRequest>(lsp::SignatureHelpParams {
            text_document: at(pos).text_document,
            position: pos,
            context: None,
        })
        .unwrap()
        .unwrap();
    assert_eq!(help.active_parameter, Some(1));
    let sig = &help.signatures[0];
    let lsp::ParameterLabel::Offsets([s, e]) = sig.parameters.as_ref().unwrap()[1].label else {
        panic!("offsets expected")
    };
    let utf16: Vec<u16> = sig.label.encode_utf16().collect();
    assert_eq!(
        String::from_utf16(&utf16[s as usize..e as usize]).unwrap(),
        "b: Float = 2"
    );
}

#[test]
fn signature_help_without_offset_support_sends_strings() {
    let mut client = Client::with_capabilities(lsp::ClientCapabilities::default()).0;
    let (text, pos) = common::cursor("rill main() Sample { return sin($) }");
    client.open(&uri(), &text);
    let help = client
        .request::<r::SignatureHelpRequest>(lsp::SignatureHelpParams {
            text_document: at(pos).text_document,
            position: pos,
            context: None,
        })
        .unwrap()
        .unwrap();
    let params = help.signatures[0].parameters.as_ref().unwrap();
    assert_eq!(params[0].label, lsp::ParameterLabel::Simple("x: T".into()));
}

#[test]
fn completion_replaces_the_word_being_typed() {
    let (mut client, pos) = open("rill main(gain: Float) Sample {\n    let a = ga$\n");
    let list = client
        .request::<r::Completion>(lsp::CompletionParams {
            text_document: at(pos).text_document,
            position: pos,
            context: None,
        })
        .unwrap()
        .unwrap();
    let gain = list.items.iter().find(|i| i.label == "gain").unwrap();
    let edit = gain.text_edit.as_ref().unwrap();
    assert_eq!(edit.range, range(1, 12, 1, 14));
    assert_eq!(edit.new_text, "gain");
    assert_eq!(gain.kind, Some(lsp::CompletionItemKind::Variable));
}

#[test]
fn prepare_rename_and_rename() {
    let (mut client, pos) =
        open("rill main() Sample {\n    let v$ol = 0.5\n    return sin(vol) * vol\n}");
    let prepared = client
        .request::<r::PrepareRename>(at(pos))
        .unwrap()
        .unwrap();
    assert_eq!(
        prepared,
        lsp::PrepareRenameResult::RangeWithPlaceholder {
            range: range(1, 8, 1, 11),
            placeholder: "vol".into()
        }
    );
    let edit = client
        .request::<r::Rename>(lsp::RenameParams {
            text_document: at(pos).text_document,
            position: pos,
            new_name: "volume".into(),
        })
        .unwrap()
        .unwrap();
    let edits = &edit.changes.unwrap()[&uri()];
    let ranges: Vec<_> = edits.iter().map(|e| e.range).collect();
    assert_eq!(
        ranges,
        [range(1, 8, 1, 11), range(2, 15, 2, 18), range(2, 22, 2, 25)]
    );

    // Refusals come back as errors with a reason.
    let e = client
        .request::<r::Rename>(lsp::RenameParams {
            text_document: at(pos).text_document,
            position: pos,
            new_name: "sin".into(),
        })
        .unwrap_err();
    assert!(e.message.contains("would"), "{}", e.message);
}

/// Rename every name declared in every example, and check the result
/// still compiles cleanly and every use followed.
#[test]
fn renaming_anything_in_the_examples_keeps_them_valid() {
    let mut count = 0;
    for entry in std::fs::read_dir(common::examples_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rill") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let a = Analysis::new(&text);
        assert!(a.diagnostics.is_empty(), "{}", path.display());
        // Event handler parameters get their type from their name, so they
        // cannot be renamed. Events are on their way out of the language.
        let mut event_params = Vec::new();
        for item in &a.program.items {
            for stmt in &item.def().body.stmts {
                if let rill::lang::ast::Stmt::EventHandler { params, .. } = stmt {
                    event_params.extend(params.iter().map(|p| p.span));
                }
            }
        }
        for occ in a
            .index
            .all()
            .iter()
            .filter(|o| o.is_decl && !event_params.contains(&o.span))
        {
            let new_name = format!("renamed_{count}");
            count += 1;
            let edits = a
                .rename(occ.span.start, &new_name)
                .unwrap_or_else(|e| panic!("{}: {:?}: {}", path.display(), occ, e.0));
            // A sequence's events are renamed with it.
            let made_by = |t: &Target| match (t, &occ.target) {
                (Target::Event(i), Target::Seq(j)) => {
                    a.checked.seq_event(*i).is_some_and(|(seq, _)| seq == *j)
                }
                _ => false,
            };
            let uses = a
                .index
                .all()
                .iter()
                .filter(|o| o.target == occ.target || made_by(&o.target))
                .count();
            assert_eq!(edits.len(), uses);
            let mut out = text.clone();
            for (span, name) in edits.iter().rev() {
                out.replace_range(span.start as usize..span.end as usize, name);
            }
            let after = Analysis::new(&out);
            assert!(
                after.diagnostics.is_empty(),
                "{}: renaming {:?} broke it: {:?}",
                path.display(),
                occ.target,
                after.diagnostics
            );
            assert!(matches!(
                occ.target,
                Target::Def(_)
                    | Target::Binding(_)
                    | Target::Event(_)
                    | Target::Seq(_)
                    | Target::Const(_)
            ));
        }
    }
    assert!(count > 50, "only {count} names renamed");
}

/// The same for the example made of several files: every name in each
/// file can be renamed, everywhere it is used in that file and the files it
/// imports, and the program stays valid (the rename itself refuses anything
/// else).
#[test]
fn renaming_across_the_files_of_an_example() {
    let dir = common::examples_dir().join("modules");
    let mut count = 0;
    let mut across = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let a = Analysis::for_file(&path, &text, &rill::lang::Disk);
        assert!(
            a.diagnostics.iter().all(|d| !d.is_error()),
            "{}: {:?}",
            path.display(),
            a.diagnostics
        );
        // Names declared here, and uses here of names from other files.
        let wanted = |o: &&wader::analysis::index::Occurrence| {
            a.in_document(o.span) && o.target.is_user() && a.prepare_rename(o.span.start).is_ok()
        };
        let mut seen = Vec::new();
        for occ in a.index.all().iter().filter(wanted) {
            if seen.contains(&occ.target) {
                continue;
            }
            seen.push(occ.target.clone());
            let edits = a
                .rename(occ.span.start, &format!("renamed_{count}"))
                .unwrap_or_else(|e| panic!("{}: {:?}: {}", path.display(), occ, e.0));
            count += 1;
            if edits.iter().any(|(s, _)| !a.in_document(*s)) {
                across += 1;
            }
        }
    }
    assert!(count > 20, "only {count} names renamed");
    assert!(across > 5, "only {across} renames reached other files");
}
