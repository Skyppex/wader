//! `wader` serves LSP over stdio. The other commands run the same server
//! in-process on one file and print what an editor would show.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use wader::client::{Client, editor_capabilities};
use wader::lsp::{self, request as r};

#[derive(Parser)]
#[command(
    version,
    about = "A language server for Rill",
    args_conflicts_with_subcommands = true,
    after_help = "With no command, serves LSP over stdin/stdout.\n\
                  LINE and COL start at 1; COL counts characters."
)]
struct Cli {
    /// Serve LSP over stdin/stdout (the default; accepted since editors pass it).
    #[arg(long)]
    stdio: bool,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Serve LSP over stdin/stdout.
    Serve,
    /// Print diagnostics; exits with 1 if there are errors.
    Check {
        /// Rill source files.
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Hover text at a position.
    Hover(At),
    /// Signature help; «» marks the active parameter.
    Sig(At),
    /// Completion items.
    Complete(At),
    /// Go to definition.
    Def(At),
    /// Go to declaration.
    Decl(At),
    /// Find references, including the declaration.
    Refs(At),
    /// What a rename would change.
    PrepareRename(At),
    /// Print the file with the rename applied.
    Rename {
        #[command(flatten)]
        at: At,
        /// What to call it instead.
        new_name: String,
    },
}

/// A position in a file.
#[derive(clap::Args)]
struct At {
    /// Rill source file.
    file: PathBuf,
    /// LINE:COL, both starting at 1.
    #[arg(value_name = "LINE:COL", value_parser = parse_position)]
    position: lsp::Position,
}

fn parse_position(arg: &str) -> Result<lsp::Position, String> {
    let parse = |s: &str| s.parse::<u32>().ok().filter(|&n| n > 0);
    let (line, col) = arg
        .split_once(':')
        .and_then(|(l, c)| Some((parse(l)?, parse(c)?)))
        .ok_or_else(|| format!("expected LINE:COL, both from 1, got `{arg}`"))?;
    Ok(lsp::Position::new(line - 1, col - 1))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command.unwrap_or(Command::Serve)) {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::from(2)
        }
    }
}

fn run(command: Command) -> Result<ExitCode, String> {
    let at = match &command {
        Command::Serve => {
            let stdin = std::io::stdin().lock();
            let stdout = std::io::stdout().lock();
            let code = wader::server::run(stdin, stdout).map_err(|e| e.to_string())?;
            return Ok(ExitCode::from(code as u8));
        }
        Command::Check { files } => return check(files),
        Command::Hover(at)
        | Command::Sig(at)
        | Command::Complete(at)
        | Command::Def(at)
        | Command::Decl(at)
        | Command::Refs(at)
        | Command::PrepareRename(at)
        | Command::Rename { at, .. } => at,
    };
    let mut s = Session::open(&at.file)?;
    let pos = at.position;
    match &command {
        Command::Hover(_) => s.hover(pos),
        Command::Sig(_) => s.signature(pos),
        Command::Complete(_) => s.complete(pos),
        Command::Def(_) => s.locations::<r::GotoDefinition>(pos),
        Command::Decl(_) => s.locations::<r::GotoDeclaration>(pos),
        Command::Refs(_) => s.references(pos),
        Command::PrepareRename(_) => s.prepare_rename(pos),
        Command::Rename { new_name, .. } => s.rename(pos, new_name),
        Command::Serve | Command::Check { .. } => unreachable!("handled above"),
    }
    Ok(ExitCode::SUCCESS)
}

/// A client with one file open, positions counted in characters.
struct Session {
    client: Client,
    path: String,
    uri: lsp::Uri,
    text: String,
}

fn client() -> Client {
    let mut caps = editor_capabilities();
    caps.general = Some(lsp::GeneralClientCapabilities {
        position_encodings: Some(vec![lsp::PositionEncodingKind::utf32()]),
    });
    Client::with_capabilities(caps).0
}

fn uri_for(path: &Path) -> lsp::Uri {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    lsp::Uri(format!("file://{}", abs.display()))
}

impl Session {
    fn open(path: &Path) -> Result<Session, String> {
        let shown = path.display().to_string();
        let text = std::fs::read_to_string(path).map_err(|e| format!("{shown}: {e}"))?;
        let mut client = client();
        let uri = uri_for(path);
        client.open(&uri, &text);
        Ok(Session {
            client,
            path: shown,
            uri,
            text,
        })
    }

    fn at(&self, position: lsp::Position) -> lsp::TextDocumentPositionParams {
        lsp::TextDocumentPositionParams {
            text_document: lsp::TextDocumentIdentifier {
                uri: self.uri.clone(),
            },
            position,
        }
    }

    fn line(&self, line: u32) -> &str {
        self.text.lines().nth(line as usize).unwrap_or("")
    }

    fn print_location(&self, range: lsp::Range) {
        println!(
            "{}:{}: {}",
            self.path,
            range.start,
            self.line(range.start.line).trim()
        );
    }

    fn print_error(&self, e: wader::rpc::ResponseError) {
        println!("error: {}", e.message);
    }

    fn hover(&mut self, at: lsp::Position) {
        match self.client.request::<r::HoverRequest>(self.at(at)) {
            Ok(Some(h)) => println!("{}", h.contents.value),
            Ok(None) => println!("(nothing)"),
            Err(e) => self.print_error(e),
        }
    }

    fn signature(&mut self, at: lsp::Position) {
        let params = lsp::SignatureHelpParams {
            text_document: self.at(at).text_document,
            position: at,
            context: None,
        };
        let help = match self.client.request::<r::SignatureHelpRequest>(params) {
            Ok(Some(h)) => h,
            Ok(None) => return println!("(nothing)"),
            Err(e) => return self.print_error(e),
        };
        for (i, sig) in help.signatures.iter().enumerate() {
            let active_sig = help.active_signature.unwrap_or(0) as usize == i;
            let active = sig.active_parameter.or(help.active_parameter);
            let mut label = sig.label.clone();
            // Mark the active parameter with «».
            if active_sig
                && let Some(lsp::ParameterInformation {
                    label: lsp::ParameterLabel::Offsets([a, b]),
                    ..
                }) = active.and_then(|p| sig.parameters.as_ref()?.get(p as usize))
            {
                let chars: Vec<char> = label.chars().collect();
                let (a, b) = (*a as usize, *b as usize);
                label = format!(
                    "{}«{}»{}",
                    chars[..a].iter().collect::<String>(),
                    chars[a..b].iter().collect::<String>(),
                    chars[b..].iter().collect::<String>()
                );
            }
            println!("{} {label}", if active_sig { ">" } else { " " });
        }
        if let Some(doc) = help
            .signatures
            .get(help.active_signature.unwrap_or(0) as usize)
            .and_then(|s| s.documentation.as_ref())
        {
            println!("\n{}", doc.value);
        }
    }

    fn complete(&mut self, at: lsp::Position) {
        let params = lsp::CompletionParams {
            text_document: self.at(at).text_document,
            position: at,
            context: None,
        };
        match self.client.request::<r::Completion>(params) {
            Ok(Some(mut list)) => {
                list.items.sort_by(|a, b| a.sort_text.cmp(&b.sort_text));
                for item in list.items {
                    let kind = item.kind.map_or(String::new(), |k| format!("{k:?}"));
                    let detail = item.detail.unwrap_or_default();
                    println!("{:<14} {:<10} {detail}", item.label, kind);
                }
            }
            Ok(None) => println!("(nothing)"),
            Err(e) => self.print_error(e),
        }
    }

    fn locations<R>(&mut self, at: lsp::Position)
    where
        R: lsp::Request<
                Params = lsp::TextDocumentPositionParams,
                Result = Option<Vec<lsp::Location>>,
            >,
    {
        match self.client.request::<R>(self.at(at)) {
            Ok(Some(locs)) if !locs.is_empty() => {
                for loc in locs {
                    self.print_location(loc.range);
                }
            }
            Ok(_) => println!("(nothing)"),
            Err(e) => self.print_error(e),
        }
    }

    fn references(&mut self, at: lsp::Position) {
        let params = lsp::ReferenceParams {
            text_document: self.at(at).text_document,
            position: at,
            context: lsp::ReferenceContext {
                include_declaration: true,
            },
        };
        match self.client.request::<r::References>(params) {
            Ok(Some(locs)) if !locs.is_empty() => {
                for loc in locs {
                    self.print_location(loc.range);
                }
            }
            Ok(_) => println!("(nothing)"),
            Err(e) => self.print_error(e),
        }
    }

    fn prepare_rename(&mut self, at: lsp::Position) {
        match self.client.request::<r::PrepareRename>(self.at(at)) {
            Ok(Some(lsp::PrepareRenameResult::RangeWithPlaceholder { range, placeholder })) => {
                println!("rename `{placeholder}` at {}", range.start)
            }
            Ok(Some(other)) => println!("{other:?}"),
            Ok(None) => println!("(cannot rename here)"),
            Err(e) => self.print_error(e),
        }
    }

    fn rename(&mut self, at: lsp::Position, name: &str) {
        let params = lsp::RenameParams {
            text_document: self.at(at).text_document,
            position: at,
            new_name: name.to_owned(),
        };
        let edit = match self.client.request::<r::Rename>(params) {
            Ok(Some(edit)) => edit,
            Ok(None) => return println!("(cannot rename here)"),
            Err(e) => return self.print_error(e),
        };
        let mut edits = edit
            .changes
            .and_then(|mut c| c.remove(&self.uri))
            .unwrap_or_default();
        eprintln!("{} edits", edits.len());
        // Apply back to front so earlier offsets stay valid. Positions are
        // in characters (UTF-32).
        edits.sort_by_key(|e| std::cmp::Reverse(e.range.start));
        let mut lines: Vec<String> = self.text.split('\n').map(str::to_owned).collect();
        for e in edits {
            assert_eq!(
                e.range.start.line, e.range.end.line,
                "renames stay on one line"
            );
            let line = &mut lines[e.range.start.line as usize];
            let chars: Vec<char> = line.chars().collect();
            let (a, b) = (
                e.range.start.character as usize,
                e.range.end.character as usize,
            );
            *line = chars[..a].iter().collect::<String>()
                + &e.new_text
                + &chars[b..].iter().collect::<String>();
        }
        print!("{}", lines.join("\n"));
    }
}

fn check(files: &[PathBuf]) -> Result<ExitCode, String> {
    let mut errors = 0;
    for file in files {
        let s = Session::open(file)?;
        let path = &s.path;
        let diags = s.client.diagnostics(&s.uri).unwrap_or_default();
        for d in &diags {
            let kind = match d.severity {
                Some(lsp::DiagnosticSeverity::Error) => {
                    errors += 1;
                    "error"
                }
                Some(lsp::DiagnosticSeverity::Warning) => "warning",
                _ => "info",
            };
            let (message, help) = d
                .message
                .split_once("\nhelp: ")
                .map_or((d.message.as_str(), None), |(m, h)| (m, Some(h)));
            println!("{path}:{}: {kind}: {message}", d.range.start);
            let start = d.range.start;
            let line = s.line(start.line);
            if !line.is_empty() {
                let width = if d.range.end.line == start.line {
                    (d.range.end.character - start.character).max(1)
                } else {
                    1
                };
                println!("    {line}");
                println!(
                    "    {}{}",
                    " ".repeat(start.character as usize),
                    "^".repeat(width as usize)
                );
            }
            if let Some(help) = help {
                println!("    help: {help}");
            }
        }
        if diags.is_empty() {
            println!("{path}: ok");
        }
    }
    Ok(if errors > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
