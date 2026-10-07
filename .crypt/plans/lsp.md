# Plan: wader, the Rill language server

## Goal

A language server for Rill (`../`), written in Rust, speaking LSP over stdio.
Priority features, in order: **diagnostics** (nearly free, and the base for
everything else), **hover**, **signature help**, **completion**, **rename**.
Go-to-definition and find-references fall out of rename's machinery, so they
come along.

No LSP framework crates (`tower-lsp`, `lsp-types`, `lsp-server`): the wire
format lives in `src/rpc/`, the protocol models in `src/lsp/`, both our own.
Allowed dependencies: `serde`, `serde_json`, and `rill` by path.

## Decisions (2026-10-07)

- Doc comments are plain `//` lines directly above a def or param.
- Name resolution lives in rill's checker (one source of truth).
- Events are on their way out: ignore them.
- Also wanted, as early as possible: goto definition, goto declaration, references.
  They are done right after the resolution table (Phase 5.4), before hover.
- A CLI mode (`wader check FILE`, `wader hover FILE LINE:COL`, ...) runs the real
  server in-process over in-memory pipes, for testing without an editor.

## Layout

```
wader/
  Cargo.toml          rill = { path = "..", default-features = false }  (no cpal/ALSA)
  src/
    main.rs           stdio setup, logging, run the server loop
    rpc/              JSON-RPC 2.0 over a byte stream. Knows nothing about LSP.
      mod.rs
      codec.rs        Content-Length framing: read_message / write_message
      message.rs      Message, Request, Response, Notification, RequestId, ResponseError, ErrorCode
    lsp/              Protocol models ONLY: structs/enums + trait impls
      mod.rs          (serde, Default, From, Display, the Request/Notification marker traits).
      base.rs         Position, Range, Location, TextEdit, WorkspaceEdit, MarkupContent, Uri, ...
      lifecycle.rs    InitializeParams, InitializeResult, ServerCapabilities, ClientCapabilities (subset)
      sync.rs         DidOpen/DidChange/DidClose/DidSave params, TextDocumentSyncKind
      diagnostic.rs   Diagnostic, DiagnosticSeverity, PublishDiagnosticsParams
      hover.rs        HoverParams, Hover
      signature.rs    SignatureHelpParams, SignatureHelp, SignatureInformation, ParameterInformation
      completion.rs   CompletionParams, CompletionItem, CompletionItemKind, CompletionList
      rename.rs       RenameParams, PrepareRenameParams, PrepareRenameResult
      navigation.rs   DefinitionParams, ReferenceParams (later: DocumentSymbol, ...)
      methods.rs      One zero-sized type per method implementing Request/Notification
    server/           Application logic: state, dispatch, lifecycle
      mod.rs          Server { state, out } and the main loop
      dispatch.rs     method name -> typed handler; error replies
      state.rs        initialized/shutdown flags, negotiated position encoding, open documents
    document/         Text handling, independent of Rill
      mod.rs          Document { uri, version, text, line_index }
      line_index.rs   byte offset <-> LSP Position (UTF-16 or UTF-8), applying edits
    analysis/         Everything Rill-specific
      mod.rs          Analysis: one snapshot per document version (tokens, AST, check results, index)
      convert.rs      rill Span/Diagnostic -> lsp Range/Diagnostic
      index.rs        symbol table: definitions, references, scopes (built on rill's resolutions)
      locate.rs       "what is at this offset?" (token, enclosing call, enclosing def)
      docs.rs         doc comments from source, built-in docs, rendering signatures as markdown
      hover.rs
      signature.rs
      completion.rs
      rename.rs
  tests/              end-to-end: drive Server in-process over in-memory pipes
```

Rule for `src/lsp/`: if it would need to know what Rill is, it doesn't go there.

---

## Phase 0 — Scaffold

1. `cargo init --bin` in `wader/` (edition 2024, matching rill). Add `languages.rust.enable = true` to `devenv.nix`.
2. Dependencies: `serde` (derive), `serde_json`, `rill = { path = "..", default-features = false }`.
   Check `cargo build` works without ALSA: rill's `device` feature must stay off.
3. Logging: a tiny `log!` macro to stderr (clients show it in their LSP log).
   Never print to stdout outside the codec; stdout is the protocol channel.
4. Add `/.crypt` to `wader/.gitignore` like the parent repo has.

**Done when:** `cargo run` starts, reads stdin, exits on EOF.

## Phase 1 — `src/rpc/`: JSON-RPC transport

1. `message.rs`
   - `RequestId` = `Int(i64) | Str(String)` (untagged).
   - `Request { id, method, params: Option<Value> }`, `Notification { method, params }`,
     `Response { id: Option<RequestId>, result: Option<Value>, error: Option<ResponseError> }`.
   - `Message` = untagged enum of the three; always serialize `"jsonrpc": "2.0"`.
     Distinguish on presence of `id`/`method` (a hand-written `Deserialize` via an
     intermediate struct is clearer than relying on untagged ordering).
   - `ResponseError { code, message, data }` and `ErrorCode` consts:
     ParseError -32700, InvalidRequest -32600, MethodNotFound -32601,
     InvalidParams -32602, InternalError -32603, ServerNotInitialized -32002,
     RequestCancelled -32800.
   - Helpers: `Response::ok(id, impl Serialize)`, `Response::err(id, code, msg)`.
2. `codec.rs`
   - `read_message(&mut impl BufRead) -> io::Result<Option<Message>>`: parse headers
     until blank line, require `Content-Length`, ignore `Content-Type`, read exactly
     N bytes, deserialize. `Ok(None)` on clean EOF.
   - `write_message(&mut impl Write, &Message)`: serialize, write header + body, flush.
3. Tests: round-trip each message kind; two messages back-to-back in one buffer;
   missing header -> error; non-ASCII body (length is bytes, not chars).

**Done when:** unit tests pass. No LSP knowledge anywhere in `rpc/`.

## Phase 2 — `src/lsp/` base models + lifecycle

1. `methods.rs`: protocol-level traits, so dispatch can be typed:
   ```rust
   pub trait Request { const METHOD: &'static str; type Params: DeserializeOwned; type Result: Serialize; }
   pub trait Notification { const METHOD: &'static str; type Params: DeserializeOwned + Serialize; }
   ```
   One unit struct per method (`Initialize`, `Shutdown`, `Hover`, `PublishDiagnostics`, ...).
2. `base.rs`: `Position { line: u32, character: u32 }`, `Range`, `Location`,
   `TextDocumentIdentifier`, `VersionedTextDocumentIdentifier`,
   `TextDocumentPositionParams`, `TextEdit`, `WorkspaceEdit { changes: HashMap<Uri, Vec<TextEdit>> }`,
   `MarkupContent { kind: MarkupKind, value }`. `Uri` is a `String` newtype (no url crate;
   we only compare and echo URIs).
3. `lifecycle.rs`: `InitializeParams` (only fields we read: `capabilities`,
   `client_info`, `general.position_encodings`), `InitializeResult { capabilities, server_info }`,
   `ServerCapabilities` with every field `Option` + `skip_serializing_if`.
4. Conventions for all of `lsp/`: `#[serde(rename_all = "camelCase")]`; unknown
   fields ignored; integer enums (`DiagnosticSeverity`, `CompletionItemKind`,
   `TextDocumentSyncKind`) get hand-written `Serialize`/`Deserialize` as numbers.
5. `server/`: the main loop.
   - `Server::run(reader, writer)` — generic over `BufRead`/`Write` so tests can drive it.
   - State machine: before `initialize` every request gets `ServerNotInitialized`
     (notifications dropped, except `exit`); after `shutdown` requests get `InvalidRequest`;
     `exit` ends the loop (process exit code 0 if shutdown came first, else 1).
   - Unknown request -> `MethodNotFound`; unknown notification -> ignore
     (including `$/cancelRequest` and `$/setTrace` for now).
   - Bad params -> `InvalidParams` with the serde message. A handler panic should
     not kill the server: wrap handlers in `catch_unwind` -> `InternalError`.
6. Editor hookup notes (put in a README): Helix `languages.toml`, Neovim
   `vim.lsp.start{ cmd = {"wader"}, filetypes = {"rill"} }`.

**Done when:** an editor connects, `initialize` / `shutdown` / `exit` work, and the
editor's LSP log shows wader's server info.

## Phase 3 — Documents and positions (`src/document/`)

1. `LineIndex::new(text)`: byte offsets of every line start.
   - `offset_to_position(u32) -> Position` and `position_to_offset(Position) -> u32`,
     in the negotiated encoding. Default is UTF-16; if the client lists `"utf-8"` in
     `general.positionEncodings`, pick it and report it in `ServerCapabilities.position_encoding`.
   - Clamp out-of-range positions instead of panicking (clients do send them).
2. Text sync: start with `TextDocumentSyncKind::Full` (simplest correct thing).
   `didOpen` stores, `didChange` replaces, `didClose` drops. Keep `version`.
   Incremental sync is an optional later step (Phase 11).
3. Tests: ASCII, `F#4`-style text, a line with emoji/`ø` (UTF-16 surrogate pairs vs
   UTF-8 bytes), CRLF line endings, position past end of line/file.

**Done when:** documents are tracked and offset/position conversion is tested.

## Phase 4 — Diagnostics (works with rill as it is today)

1. `analysis/convert.rs`: `rill::lang::Diagnostic` -> `lsp::Diagnostic`.
   Severity maps 1:1; `help` appended to the message on a new line
   (`relatedInformation` needs a location, help has none). `Span::default()`
   (file-level, e.g. "no rill named `main`") -> range `0:0..0:0`.
2. On `didOpen`/`didChange`: run `rill::lang::compile(text)`; publish errors+warnings
   with `textDocument/publishDiagnostics` (include `version`). On success publish
   `checked.warnings`. On `didClose` publish an empty list.
3. Do **not** use `compile_entry`: a library file without `main` is fine in an
   editor. (Optional later: a hint-level "no `main`" only if the file looks like an entry.)
4. Source string `"rill"` on every diagnostic.

**Done when:** typos in an example file show squiggles with the checker's
messages and "did you mean" help.

## Phase 5 — Rill front-end prerequisites (changes in `../src/lang/`)

The front end is built for a compiler: it stops at the first lexer error, the
parser returns no tree when there's any error, the checker returns no types when
there's any error, and name resolution is thrown away. An editor is always
looking at broken code, so each feature below needs some of this. Do these as
separate small changes in the rill repo, each with tests there.

1. **Partial parse.** `parser::parse_partial(src, tokens) -> (Program, Vec<Diagnostic>)`.
   Recovery already skips to the next definition, so it's mostly returning the items
   collected so far. Keep `parse` as a wrapper. Bonus: recover inside a block
   (skip to next statement) so one bad line doesn't drop a whole def.
2. **Lexer recovery.** `lex` returns on the first error. Add a mode that emits an
   `Error` token, records the diagnostic, and continues.
3. **Partial check.** `check::analyze(program) -> Analysis { types, signatures, diags, resolutions, bindings }`
   that always returns. `check` becomes a wrapper. Types of unchecked expressions are
   already `Type::Error`, so this is mainly not discarding the state.
4. **Resolution table** (the big one; rename, goto, references, hover-on-locals need it).
   In the checker, every time a name is looked up, record what it hit:
   ```rust
   pub enum Resolution {
       Binding(BindingId),   // param, let, state, size param, lambda param
       Def(usize),           // index into signatures (fn or rill)
       Builtin(&'static str),
       Constant(&'static str), // PI, TAU, RATE
       Unresolved,
   }
   pub struct Binding { name: String, span: Span /* the declaring ident */, kind: BindingKind, ty: Type, def: usize }
   pub resolutions: Vec<(Span, Resolution)>   // every use site, incl. callee idents, assign targets, named args?
   ```
   Places that look names up: `Name` exprs, `Call.callee`, `Assign.target`, size vars in
   types, pipe stages. Named arguments (`a4: 440Hz`) resolve to the callee's parameter;
   record those too so renaming a parameter updates call sites.
   `VarKind` already exists; `Var` needs the declaring span and an id.
5. **Docs.** Two sources:
   - User code: comments directly above a `fn`/`rill`/`param`. No rill change needed —
     wader can read the `//` lines above `def.span.start` from the text. (If you'd rather
     have `///` doc comments in the language, that's a lexer+AST change; decide first.)
   - Built-ins: add `builtins::doc(name) -> Option<&'static str>` next to `lookup`, so docs
     live beside signatures. Same for `CONSTANTS` and units (`Unit::ALL` already exists).
6. **Spans for names.** `ExprKind::Name` uses the expr span, which is the name — fine.
   Check that `Field`'s ident, generics and lambda params all carry spans (they do via `Ident`).

Order: 1 and 3 unlock hover/signature help on broken files; 4 unlocks rename.
Hover/signature/completion (Phases 7–9) can start right after 1+3 and use 4 when it lands.

**Done when:** rill tests still pass and `analyze` returns useful partial data for
a file with an error in one def.

## Phase 6 — Analysis snapshot (`src/analysis/`)

1. `Analysis::new(text) -> Analysis`: lex, partial parse, partial check, build index.
   Cache per document version (recompute on change; files are small, so no
   incremental computation needed). Diagnostics from Phase 4 move to read from this.
2. `locate.rs`:
   - `token_at(offset)` — the token under/just before the cursor (use rill's lexer tokens).
   - `node_at(offset)` — innermost expr/ident/type/def containing the offset (AST walk
     using spans).
   - `enclosing_call(offset)` — innermost `Call` whose `(`..`)` contains the offset, plus the
     active argument index (count top-level commas before the cursor, honoring named args
     and the `|>` shift: for `x |> f(a, |)` the piped value is argument 0).
   - `scope_at(offset)` — bindings visible at the offset (from the index: bindings in the
     enclosing def whose declaring statement ends before the offset, plus params,
     generics, lambda params).
3. `index.rs`: from `resolutions`, build `defs -> [use spans]` and `binding -> [use spans]`.
4. `docs.rs`: render a `Signature` as a ```` ```rill ```` code block using rill's
   `Display` for `Signature`/`Type`; attach doc comments; built-in docs.

**Done when:** unit tests can ask "what is at offset N" on example files.

## Phase 7 — Hover

`textDocument/hover` -> `Hover { contents: MarkupContent(markdown), range }`.

| Under cursor | Shows |
| --- | --- |
| fn/rill name (def or call) | full signature (`rill peak(x: Sample, release: Time = 300ms) -> Sample @ rate / 2`) + doc comment |
| built-in fn | every overload from `builtins::lookup` + doc; explain `T`/`S` type params |
| constant (`PI`, `RATE`) | `RATE: Freq` + doc |
| local / param / state | `let a: Sample`, `state level: Sample`, `param release: Time = 300ms` |
| any other expr | its type from `types[expr.id]` (useful on `|>` chains and unit literals) |
| number with unit | value converted to base unit (`300ms` → `0.3 s`), via `Unit::to_base` |
| note literal (`F#4`) | its frequency via `check::pitch_literal` |
| keyword | one-line explanation (`state`, `rill`, `@ rate`) |

Skip `Type::Error` results rather than showing `{error}`.

**Done when:** hovering everything in `examples/sketch.rill` shows something sensible.

## Phase 8 — Signature help

`textDocument/signatureHelp`, trigger characters `(` and `,`, retrigger `)`.

1. Find `enclosing_call(offset)`; look up the callee: user def -> one signature,
   built-in -> all overloads (one `SignatureInformation` each).
2. Label = rendered signature; `ParameterInformation.label` as `[start, end]` offsets
   into the label (avoids ambiguity when names repeat).
3. Active parameter: by position, or — if the cursor is in a named argument
   `a4: |` — the parameter with that name. Account for `|>`: piped value fills param 0.
4. Active signature for overloads: the first whose arity fits the args written so far.
5. Show defaults in the label and the param doc.

**Done when:** typing `equal(E4, ` highlights `steps`, and `x |> clamp(` highlights `lo`.

## Phase 9 — Completion

`textDocument/completion`, trigger characters: none initially (clients complete on
identifier chars anyway); maybe `.` later for layouts.

Context decides what to offer (from `token_at` / previous token):

1. **Expression position** (default): visible bindings (`scope_at`), user fns and
   rills (rills only when the enclosing def is a rill — fns can't call rills; check
   `Place`), built-in fns, constants, `true`/`false`, keywords valid in a body
   (`let`, `state` only at the top of a rill body, `return`, `if`, `else`, `fn`).
   Detail = type/signature; documentation = doc comment.
2. **After `|>`**: only callables. Prefer ones whose first parameter accepts the piped
   expression's type (`coerces` from `rill::lang::types`) — sort them first.
3. **Type position** (after `:` in a param/let/state, after `->`, inside `[ ; ]`):
   `Sample Float Int Bool Freq Time Pitch Interval Gain`, `fn(...) -> ...` snippet,
   generics of the enclosing def.
4. **After a number** (`440|`): units from `Unit::ALL` (`Hz`, `ms`, `s`, `st`, `cents`, `dB`).
5. **Named argument** inside a call: `name: ` items for parameters not yet given.
6. **Top level**: `fn` / `rill` snippets.
7. Snippets only if `textDocument.completion.completionItem.snippetSupport`; otherwise
   plain insert text.

Partial parse matters most here: the file is broken at the cursor almost always.
If the AST around the cursor is missing, fall back to token-based context.

**Done when:** completing in the middle of writing a new rill offers the right things
in each position above.

## Phase 10 — Rename (+ definition, references)

1. `textDocument/prepareRename`: allowed on user fn/rill names, params, lets, states,
   generics, lambda params. Refuse (return error with a message) on built-ins,
   constants, keywords, literals. Return the identifier's range + placeholder.
2. `textDocument/rename`:
   - Validate the new name: lexes as a single `Ident` token, isn't a keyword.
   - Collect declaration + all uses from the index (including named-argument labels
     at call sites when renaming a parameter).
   - Conflict check: re-run analysis on the edited text; if the set of resolutions
     changed shape (something now resolves differently / a new error appeared), refuse
     with an explanation. Cheap and catches shadowing problems without special cases.
   - Return a `WorkspaceEdit` with `changes` for the one document.
3. Single-file only for now — rill has no imports. Revisit when it does.
4. `textDocument/definition` and `textDocument/references`: the same index lookup,
   returning `Location`s. Nearly free once rename works.

**Done when:** renaming `phase` in `sine`, a param used as a named arg elsewhere, and a
rill used in a pipe all rewrite correctly; renaming to an existing local name is refused.

## Phase 11 — Later / nice to have

Pick freely; none block the above.

- **Document symbols** (`textDocument/documentSymbol`): defs with params/states as children.
- **Semantic tokens**: distinguish fn vs rill vs state vs param vs built-in; units on numbers.
  (Highlighting otherwise comes from `../tree-sitter-rill`.)
- **Inlay hints**: inferred types on `let`/`state` without annotations; parameter names at
  call sites.
- **Code actions**: apply checker "did you mean `x`?" suggestions (needs the suggestion as
  structured data on `Diagnostic`, not only in `help` text — a small rill change).
- **Formatting**: needs a real formatter in rill (`pretty.rs` is a debug tree printer).
- **Incremental sync** (`TextDocumentSyncKind::Incremental`) — apply ranged edits via `LineIndex`.
- **Cancellation / debouncing**: if analysis ever gets slow, analyze on a worker thread and
  honor `$/cancelRequest`. Not needed while files are small.
- **Entry checks**: run `check_entry` when the file has a `main`, show its errors.
- **Workspace-wide** features once rill has imports/modules.
- **Nix**: package wader in the parent flake next to rill.

## Testing approach

- `rpc/` and `document/` — plain unit tests.
- `analysis/` — unit tests on source strings with a cursor marker, e.g.
  `"rill f(x: Sample) -> Sample { return x|$ }"` → strip `$`, use its offset.
- End-to-end in `tests/` — run `Server::run` over in-memory pipes, send
  `initialize`, `didOpen`, a request, assert on the JSON response.
- Use `../examples/*.rill` as fixtures (they must produce zero diagnostics).

## Suggested order at a glance

| # | Step | Needs rill changes? |
| --- | --- | --- |
| 0 | scaffold | no |
| 1 | rpc codec + messages | no |
| 2 | lsp models + lifecycle + server loop | no |
| 3 | documents + line index | no |
| 4 | diagnostics | no |
| 5.1–5.3 | partial lex/parse/check | **yes** |
| 6 | analysis snapshot + locate | no |
| 7 | hover | 5.5 for built-in docs |
| 8 | signature help | no |
| 9 | completion | no |
| 5.4 | resolution table | **yes** |
| 10 | rename, definition, references | uses 5.4 |
| 11 | extras | some |

## Open questions

- Doc comments: plain `//` above a def (works today, no language change) or a dedicated
  `///` syntax in the lexer/AST?
- Should resolution live in rill's checker (one source of truth, recommended) or be a
  separate scope walk in wader (no rill changes, but duplicates scoping rules and can drift)?
- `event` declarations and `on` handlers are skipped by the parser today; hover/completion
  for them waits until the language settles them.

---

## Status (2026-10-07)

Phases 0–10 are implemented and verified: 80 tests in wader (unit, end to end
through the codec, real stdio, a rename property test over all examples, and a
panic sweep over every offset of every example and every typing prefix), plus
7 new rill tests in `tests/partial.rs`. Phase 11 is not started.

Changes in rill (`../src/lang/`), all additive; `lex`, `parse`, `check`
behave exactly as before:
- `lexer::lex_partial`: reports every error and keeps going.
- `parser::parse_partial`: also recovers per statement, and closes a block left
  open at the end of the file or the next definition.
- `check::check_partial`: always returns `Checked`. `Checked` gained
  `bindings` (every named value with its scope) and `resolutions` (what every
  name refers to).
- `builtins::doc`: docs for every built-in function and constant.

## Assumptions made while implementing

1. Go to declaration = go to definition: Rill has no separate declarations.
2. Diagnostics come from the strict compiler (exactly what `rill check` says);
   a file without `main` is fine. Features use the error-tolerant front end.
3. Completion hides rills where they can't be called (in fns and anonymous fns),
   and hides the enclosing definition (no recursion).
4. Named arguments of built-ins (`a4:` in `equal`) have no declaration to jump
   to; hover and signature help still explain them.
5. Hover on a note name shows no frequency: it has none until a tuning gives it
   one (your correction).
6. Rename refuses when it would change what any name refers to or add an
   error, checked by re-analysing the renamed text. Type names like `Sample` are
   allowed as new names when nothing changes meaning.
7. Doc comments: `//` lines directly above; for parameters, also a trailing
   `//` on the same line.
8. Position encoding: UTF-8 when the client offers it, else UTF-16.
9. Text sync is Full. Every request is answered before the next is read, so
   `$/cancelRequest` is ignored.
10. The only completion trigger character is `>` (for `|>` and `->`).
11. A name in parentheses resolves with the span of the parentheses in rill;
    wader narrows it to the identifier.
12. Event handlers are not specially supported (their parameters are skipped
    by the rename property test).
