//! Files that import each other, end to end: real files in a temporary
//! folder, some of them open in the editor.

mod common;

use std::path::{Path, PathBuf};

use common::cursor;
use wader::client::Client;
use wader::lsp::{self, Position, request as r};
use wader::uri;

/// A folder of files that is removed again when the test ends.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str, files: &[(&str, &str)]) -> Folder {
        let dir = std::env::temp_dir().join(format!("wader-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (path, text) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        Folder(dir)
    }

    fn uri(&self, file: &str) -> lsp::Uri {
        uri::from_path(&self.0.join(file))
    }

    fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const OSC: &str = "\
// A ramp from -1 to 1.
export rill saw(freq: Freq) Sample {
    state phase: Float = 0
    phase = wrap(phase + freq / RATE)
    return phase * 2 - 1 + blep(phase)
}

fn blep(t: Float) Float { t * 0 }
";

/// Open `file` from `folder`, its text with a `$` cursor; returns the
/// client and the cursor.
fn open(folder: &Folder, file: &str, src: &str) -> (Client, Position) {
    let (text, pos) = cursor(src);
    std::fs::write(folder.path(file), &text).unwrap();
    let mut client = Client::new();
    client.open(&folder.uri(file), &text);
    (client, pos)
}

fn at(folder: &Folder, file: &str, position: Position) -> lsp::TextDocumentPositionParams {
    lsp::TextDocumentPositionParams {
        text_document: lsp::TextDocumentIdentifier {
            uri: folder.uri(file),
        },
        position,
    }
}

fn complete_at(folder: &Folder, position: Position) -> lsp::CompletionParams {
    lsp::CompletionParams {
        text_document: lsp::TextDocumentIdentifier {
            uri: folder.uri("song.rill"),
        },
        position,
        context: None,
    }
}

#[test]
fn errors_in_an_imported_file_show_on_its_import() {
    let folder = Folder::new("errors", &[("osc.rill", "export fn oops( Float { 1 }\n")]);
    let (client, _) = open(
        &folder,
        "song.rill",
        "import ./osc\nrill main() Sample { return 0 }$",
    );
    let diags = client.diagnostics(&folder.uri("song.rill")).unwrap();
    assert_eq!(diags.len(), 1, "{diags:#?}");
    let d = &diags[0];
    assert_eq!(d.range, common::range(0, 7, 0, 12));
    assert!(d.message.contains("osc.rill has 1 error"), "{}", d.message);
    let related = d.related_information.as_ref().unwrap();
    assert_eq!(related[0].location.uri, folder.uri("osc.rill"));
    assert_eq!(related[0].location.range.start.line, 0);
}

#[test]
fn editing_an_open_import_updates_its_importers() {
    let folder = Folder::new("refresh", &[("osc.rill", OSC)]);
    let (mut client, _) = open(
        &folder,
        "song.rill",
        "import ./osc\nrill main() Sample { return saw(110Hz) }$",
    );
    assert_eq!(client.diagnostics(&folder.uri("song.rill")).unwrap(), []);
    // Unsaved: `saw` is no longer exported.
    client.open(&folder.uri("osc.rill"), OSC);
    client.change(
        &folder.uri("osc.rill"),
        &OSC.replace("export rill saw", "rill saw"),
    );
    let diags = client.diagnostics(&folder.uri("song.rill")).unwrap();
    let errors: Vec<&str> = diags
        .iter()
        .filter(|d| d.severity == Some(lsp::DiagnosticSeverity::Error))
        .map(|d| d.message.lines().next().unwrap())
        .collect();
    assert_eq!(errors, ["`saw` is private to `osc`"]);
    // Closed without saving: back to what is on disk.
    client.notify::<lsp::notification::DidCloseTextDocument>(lsp::DidCloseTextDocumentParams {
        text_document: lsp::TextDocumentIdentifier {
            uri: folder.uri("osc.rill"),
        },
    });
    assert_eq!(client.diagnostics(&folder.uri("song.rill")).unwrap(), []);
}

#[test]
fn imports_that_are_not_allowed() {
    let folder = Folder::new("paths", &[("osc.rill", OSC), ("my-osc.rill", OSC)]);
    let (client, _) = open(
        &folder,
        "song.rill",
        "import ./osc.rill\nimport ./kick.wav\nimport ./my-osc\nimport ./nothere\nrill main() Sample { return 0 }$",
    );
    let messages: Vec<String> = client
        .diagnostics(&folder.uri("song.rill"))
        .unwrap()
        .into_iter()
        .map(|d| d.message.lines().next().unwrap().to_owned())
        .collect();
    assert_eq!(
        messages,
        [
            "leave out `.rill`: a module is imported by its name",
            "only Rill files can be imported, and `kick.wav` is not one",
            "cannot find module `./nothere`",
        ]
    );
}

#[test]
fn any_file_name_is_fine() {
    let folder = Folder::new("names", &[]);
    let (client, _) = open(&folder, "my-song.rill", "rill main() Sample { return 0 }$");
    assert_eq!(client.diagnostics(&folder.uri("my-song.rill")).unwrap(), []);
}

#[test]
fn definitions_in_other_files() {
    let folder = Folder::new("definition", &[("osc.rill", OSC)]);
    let (mut client, pos) = open(
        &folder,
        "song.rill",
        "import ./osc\nrill main() Sample { return s$aw(110Hz) }",
    );
    let found = client
        .request::<r::GotoDefinition>(at(&folder, "song.rill", pos))
        .unwrap()
        .unwrap();
    assert_eq!(found[0].uri, folder.uri("osc.rill"));
    assert_eq!(found[0].range, common::range(1, 12, 1, 15));

    // The path of an import leads to the file.
    let found = client
        .request::<r::GotoDefinition>(at(&folder, "song.rill", Position::new(0, 9)))
        .unwrap()
        .unwrap();
    assert_eq!(found[0].uri, folder.uri("osc.rill"));
    assert_eq!(found[0].range, common::range(0, 0, 0, 0));
}

#[test]
fn hover_says_where_things_come_from() {
    let folder = Folder::new("hover", &[("osc.rill", OSC)]);
    let (mut client, pos) = open(
        &folder,
        "song.rill",
        "import ./osc\nrill main() Sample { return s$aw(110Hz) }",
    );
    let hover = |client: &mut Client, position| {
        client
            .request::<r::HoverRequest>(at(&folder, "song.rill", position))
            .unwrap()
            .unwrap()
            .contents
            .value
    };
    let text = hover(&mut client, pos);
    assert!(text.contains("rill saw(freq: Freq) Sample"), "{text}");
    assert!(text.contains("A ramp from -1 to 1."), "{text}");
    assert!(text.contains("From `osc`."), "{text}");
    let text = hover(&mut client, Position::new(0, 9));
    assert!(text.contains("Exports `saw`."), "{text}");
    assert!(
        text.contains("Only this file sees what it gives."),
        "{text}"
    );
}

#[test]
fn completion_offers_what_is_imported_and_nothing_private() {
    let folder = Folder::new("completion", &[("osc.rill", OSC)]);
    let (mut client, pos) = open(
        &folder,
        "song.rill",
        "import ./osc\nrill main() Sample { return $ }",
    );
    let labels: Vec<String> = client
        .request::<r::Completion>(complete_at(&folder, pos))
        .unwrap()
        .unwrap()
        .items
        .into_iter()
        .map(|i| i.label)
        .collect();
    assert!(labels.contains(&"saw".to_owned()));
    assert!(!labels.contains(&"blep".to_owned()));
}

#[test]
fn completion_of_import_paths() {
    let folder = Folder::new(
        "path-completion",
        &[
            ("osc.rill", OSC),
            ("lib/fx.rill", "export fn f() Float { 1 }"),
            ("my-thing.rill", OSC),
            ("notes.txt", "x"),
        ],
    );
    let labels = |src: &str| {
        let (mut client, pos) = open(&folder, "song.rill", src);
        let mut labels: Vec<String> = client
            .request::<r::Completion>(complete_at(&folder, pos))
            .unwrap()
            .unwrap()
            .items
            .into_iter()
            .map(|i| i.label)
            .collect();
        labels.sort();
        labels
    };
    // Never the extension, never the file itself, never a name that cannot
    // be imported.
    assert_eq!(
        labels("import $"),
        [
            "../",
            "./lib/",
            "./my-thing",
            "./osc",
            "@effects/",
            "@gen/",
            "@mix/",
            "@util/"
        ]
    );
    assert_eq!(labels("import ./lib/$"), ["./lib/fx"]);
    assert_eq!(
        labels("import ./l$"),
        ["../", "./lib/", "./my-thing", "./osc"]
    );
    // The core library, from anywhere.
    assert_eq!(
        labels("import @$"),
        ["@effects/", "@gen/", "@mix/", "@util/"]
    );
    assert_eq!(
        labels("import @gen/$"),
        ["@gen/noise", "@gen/osc", "@gen/sampler"]
    );
}

#[test]
fn renaming_reaches_other_files() {
    let folder = Folder::new("rename", &[("osc.rill", OSC)]);
    let (mut client, pos) = open(
        &folder,
        "song.rill",
        "import ./osc\nrill main() Sample { return s$aw(110Hz) + saw(1Hz) }",
    );
    let edit = client
        .request::<r::Rename>(lsp::RenameParams {
            text_document: lsp::TextDocumentIdentifier {
                uri: folder.uri("song.rill"),
            },
            position: pos,
            new_name: "ramp".into(),
        })
        .unwrap()
        .unwrap();
    let changes = edit.changes.unwrap();
    assert_eq!(changes[&folder.uri("song.rill")].len(), 2);
    let in_osc = &changes[&folder.uri("osc.rill")];
    assert_eq!(in_osc.len(), 1);
    assert_eq!(in_osc[0].range, common::range(1, 12, 1, 15));
}

#[test]
fn uris_and_paths() {
    let path = Path::new("/tmp/a b/song.rill");
    assert_eq!(uri::to_path(&uri::from_path(path)).unwrap(), path);
}

#[test]
fn renaming_an_export_reaches_the_files_importing_it() {
    let folder = Folder::new(
        "rename-export",
        &[
            (
                "song.rill",
                "import ./synth\nrill main() Sample { return saw(1Hz) }\n",
            ),
            ("synth.rill", "export import ./osc\n"),
            (
                "other/demo.rill",
                "import ../osc\nrill main() Sample { return saw(2Hz) }\n",
            ),
            (
                "unrelated.rill",
                "rill saw() Sample { return 0 }\nrill main() Sample { return saw() }\n",
            ),
        ],
    );
    let (mut client, pos) = open(&folder, "osc.rill", &OSC.replace("rill saw", "rill s$aw"));
    let edit = client
        .request::<r::Rename>(lsp::RenameParams {
            text_document: lsp::TextDocumentIdentifier {
                uri: folder.uri("osc.rill"),
            },
            position: pos,
            new_name: "ramp".into(),
        })
        .unwrap()
        .unwrap();
    let changes = edit.changes.unwrap();
    let count = |file: &str| changes.get(&folder.uri(file)).map_or(0, Vec::len);
    assert_eq!(count("osc.rill"), 1);
    assert_eq!(count("song.rill"), 1);
    assert_eq!(count("other/demo.rill"), 1);
    // Its own `saw` is something else.
    assert_eq!(count("unrelated.rill"), 0);
}

#[test]
fn moving_files_keeps_imports_working() {
    let folder = Folder::new(
        "move",
        &[
            ("osc.rill", OSC),
            ("lib/fx.rill", "export fn f() Float { 1 }\n"),
        ],
    );
    let (mut client, _) = open(
        &folder,
        "song.rill",
        "import ./osc\nimport ./lib/fx\nrill main() Sample { return saw(1Hz) * f() }$",
    );
    let rename = |client: &mut Client, from: &str, to: &str| {
        client
            .request::<r::WillRenameFiles>(lsp::RenameFilesParams {
                files: vec![lsp::FileRename {
                    old_uri: folder.uri(from).0,
                    new_uri: folder.uri(to).0,
                }],
            })
            .unwrap()
            .map(|e| e.changes.unwrap())
            .unwrap_or_default()
    };
    // Renaming an imported file.
    let changes = rename(&mut client, "osc.rill", "oscillators.rill");
    let edits = &changes[&folder.uri("song.rill")];
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "./oscillators");
    assert_eq!(edits[0].range, common::range(0, 7, 0, 12));
    // Moving the importing file into a folder: both its imports change.
    let changes = rename(&mut client, "song.rill", "songs/song.rill");
    let texts: Vec<&str> = changes[&folder.uri("song.rill")]
        .iter()
        .map(|e| e.new_text.as_str())
        .collect();
    assert_eq!(texts, ["../osc", "../lib/fx"]);
    // Renaming a folder.
    let changes = rename(&mut client, "lib", "sound");
    assert_eq!(changes[&folder.uri("song.rill")][0].new_text, "./sound/fx");
    // Renaming a file in a folder.
    let changes = rename(&mut client, "lib/fx.rill", "lib/effects.rill");
    assert_eq!(
        changes[&folder.uri("song.rill")][0].new_text,
        "./lib/effects"
    );
    // A file nothing imports: no edits.
    assert!(rename(&mut client, "other.rill", "renamed.rill").is_empty());
    let caps = Client::with_capabilities(wader::client::editor_capabilities())
        .1
        .capabilities;
    assert!(
        caps.workspace
            .unwrap()
            .file_operations
            .unwrap()
            .will_rename
            .is_some()
    );
}

#[test]
fn the_core_library_in_the_editor() {
    let folder = Folder::new("core", &[]);
    let (mut client, pos) = open(
        &folder,
        "song.rill",
        "import @gen/osc\nrill main() Sample { return s$aw(110Hz) }",
    );
    assert_eq!(client.diagnostics(&folder.uri("song.rill")).unwrap(), []);
    // Its source opens from a read-only copy.
    let found = client
        .request::<r::GotoDefinition>(at(&folder, "song.rill", pos))
        .unwrap()
        .unwrap();
    let path = uri::to_path(&found[0].uri).unwrap();
    assert!(path.ends_with("gen/osc.rill"), "{}", path.display());
    let copy = std::fs::read_to_string(&path).unwrap();
    assert_eq!(copy, rill::lang::core::module("gen/osc").unwrap());
    assert!(std::fs::metadata(&path).unwrap().permissions().readonly());
    // Hover says where it is from.
    let text = client
        .request::<r::HoverRequest>(at(&folder, "song.rill", pos))
        .unwrap()
        .unwrap()
        .contents
        .value;
    assert!(text.contains("From `@gen/osc`."), "{text}");
    // It cannot be renamed.
    let e = client
        .request::<r::Rename>(lsp::RenameParams {
            text_document: lsp::TextDocumentIdentifier {
                uri: folder.uri("song.rill"),
            },
            position: pos,
            new_name: "ramp".into(),
        })
        .unwrap_err();
    assert!(e.message.contains("from the core library"), "{}", e.message);
}

/// A mono 16-bit WAV of `frames` silent frames at 44.1kHz, saying it plays
/// D4 unchanged.
fn wav(path: &Path, frames: u32) {
    let mut fmt = Vec::new();
    for v in [1u16, 1] {
        fmt.extend(v.to_le_bytes());
    }
    fmt.extend(44_100u32.to_le_bytes());
    fmt.extend(88_200u32.to_le_bytes());
    fmt.extend(2u16.to_le_bytes());
    fmt.extend(16u16.to_le_bytes());
    let mut smpl = Vec::new();
    for v in [0u32, 0, 0, 62, 0, 0, 0, 0, 0] {
        smpl.extend(v.to_le_bytes());
    }
    let chunk = |id: &[u8], body: &[u8]| {
        let mut c = id.to_vec();
        c.extend((body.len() as u32).to_le_bytes());
        c.extend(body);
        c
    };
    let mut body = b"WAVE".to_vec();
    body.extend(chunk(b"fmt ", &fmt));
    body.extend(chunk(b"data", &vec![0; frames as usize * 2]));
    body.extend(chunk(b"smpl", &smpl));
    let mut file = b"RIFF".to_vec();
    file.extend((body.len() as u32).to_le_bytes());
    file.extend(body);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, file).unwrap();
}

#[test]
fn audio_files_in_the_editor() {
    let folder = Folder::new("audio", &[("notes.txt", "x")]);
    wav(&folder.path("drums/kick.wav"), 22_050);
    wav(&folder.path("pad.flac.wav"), 441);
    let labels = |src: &str| {
        let (mut client, pos) = open(&folder, "song.rill", src);
        let mut labels: Vec<String> = client
            .request::<r::Completion>(complete_at(&folder, pos))
            .unwrap()
            .map(|c| c.items.into_iter().map(|i| i.label).collect())
            .unwrap_or_default();
        labels.sort();
        labels
    };
    // Audio files and folders inside a path; not other files.
    assert_eq!(labels("const KICK = ./$"), ["./drums/", "./pad.flac.wav"]);
    assert_eq!(labels("const KICK = ./drums/k$"), ["./drums/kick.wav"]);
    // Fields after an audio file.
    let fields =
        labels("const KICK = ./drums/kick.wav\nrill main() Sample { return KICK.$length / 1s }");
    assert!(fields.contains(&"length".to_owned()), "{fields:?}");
    assert!(fields.contains(&"root".to_owned()), "{fields:?}");
    // `Audio` is a type.
    assert!(labels("fn f(a: Au$) Float { 1 }").contains(&"Audio".to_owned()));

    // Hover on the path says what the file is.
    let src = "const KICK = ./drums/kick.wav\nrill main() Sample { return KICK.length / 1s }$";
    let (mut client, _) = open(&folder, "song.rill", src);
    let hover = |client: &mut Client, position| {
        client
            .request::<r::HoverRequest>(at(&folder, "song.rill", position))
            .unwrap()
            .map(|h| h.contents.value)
            .unwrap_or_default()
    };
    let text = hover(&mut client, Position::new(0, 16));
    assert!(text.contains("./drums/kick.wav: Audio<1>"), "{text}");
    assert!(
        text.contains("0.500s, mono, 44100Hz (22050 frames)."),
        "{text}"
    );
    assert!(text.contains("Plays D4 unchanged."), "{text}");
    let text = hover(&mut client, Position::new(1, 34));
    assert!(text.contains("length: Time"), "{text}");
    // A file that is not there is an error on its path.
    let (client, _) = open(&folder, "song.rill", "const GONE = ./gone.wav$");
    let messages: Vec<String> = client
        .diagnostics(&folder.uri("song.rill"))
        .unwrap()
        .into_iter()
        .map(|d| d.message)
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m.starts_with("cannot read `./gone.wav`")),
        "{messages:?}"
    );
}
