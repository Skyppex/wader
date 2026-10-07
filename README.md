# wader

A language server for [Rill](..). Diagnostics, hover, signature help,
completion, go to definition / declaration, references and rename.

```sh
cargo build --release   # target/release/wader
```

## Editors

Helix (`languages.toml`):

```toml
[language-server.wader]
command = "wader"

[[language]]
name = "rill"
scope = "source.rill"
file-types = ["rill"]
comment-token = "//"
language-servers = ["wader"]
```

Neovim:

```lua
vim.filetype.add({ extension = { rill = "rill" } })
vim.api.nvim_create_autocmd("FileType", {
  pattern = "rill",
  callback = function()
    vim.lsp.start({ name = "wader", cmd = { "wader" } })
  end,
})
```

## Without an editor

The same server runs in-process on one file and prints what an editor would
show. `LINE:COL` start at 1; `COL` counts characters.

```sh
wader check FILE...                  # diagnostics; exit code 1 on errors
wader hover FILE LINE:COL
wader sig FILE LINE:COL              # «» marks the active parameter
wader complete FILE LINE:COL
wader def FILE LINE:COL
wader decl FILE LINE:COL
wader refs FILE LINE:COL
wader prepare-rename FILE LINE:COL
wader rename FILE LINE:COL NEW_NAME  # prints the renamed file
```

## Docs

A `//` comment directly above a `fn`, `rill`, `let` or `state` documents it.
For a parameter, so does a `//` comment at the end of its line.
