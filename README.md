# mddog

Preview Markdown and MDX in the terminal the way GitHub or Obsidian shows them: real fonts, tables, syntax highlighting, math, Mermaid diagrams, and images, drawn by WebKit.

Needs macOS and a terminal with the [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/): Ghostty, kitty, WezTerm, or iTerm2.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/qu8n/mddog/main/install.sh | sh
```

This puts `mdd` in `~/.local/bin`. It also has macOS start a hidden `mdd` at login with WebKit already loaded, so your first preview opens in about 40 ms instead of 250 ms. Run the script again to update.

- `MDDOG_INSTALL_DIR=<folder>` installs somewhere else.
- `MDDOG_NO_START_AT_LOGIN=1` skips the start at login.
- To uninstall, run `mdd --no-start-at-login`, then delete `~/.local/bin/mdd`.
- To build from source instead, install [Rust](https://rust-lang.org/tools/install/) and run `cargo install --path . && mdd --start-at-login` in a clone of this repo.

## Usage

```sh
mdd              # list the Markdown files in this git repo (or folder) and pick one
mdd docs/        # same, for another folder
mdd README.md    # preview a file
command | mdd    # preview standard input
```

The preview reloads when the file changes. Press `?` for every key.

| Key | Action |
| --- | --- |
| `j` `k`, mouse wheel | scroll a line |
| `Space` `b` | scroll a page |
| `/`, then `n` `N` | search, then go to the next or previous match |
| `t`, `1`-`9` | table of contents, jump to a section |
| click | open a link, or copy a code block with its copy button |
| drag, `Cmd-c` | select and copy text |
| `Backspace` | go back to where you were before a jump or file change |
| `Ctrl-p` | pick another file |
| `e` | edit in `$EDITOR` |
| `q` | quit |

`Cmd-c` works only when the terminal passes it to mddog instead of running its own copy. Ghostty and kitty do. In WezTerm, set `enable_kitty_keyboard = true` and remove its `Cmd-c` binding. In iTerm2, set `copy_on_select = true` instead. mddog copies through the terminal (OSC 52), so copying also works over SSH.

## Claude Code plugin

Shows a Markdown file in a pane beside the chat, redrawn as Claude edits it.

```sh
claude plugin marketplace add qu8n/mddog && claude plugin install mddog@mddog
```

Open a file with `/mddog <file.md>`, or ask Claude to show you one. Update with `claude plugin update mddog@mddog`. Claude Code shows images only in terminals it knows (Ghostty, kitty, iTerm2, WezTerm). Elsewhere, start it with `CLAUDE_CODE_FORCE_TERMINAL_IMAGES=1 claude`.

## Config

Optional, in `~/.config/mddog/config.toml`. Every key, with its default, is in [`config.example.toml`](config.example.toml).

```toml
theme = "light:github-light,dark:github-dark"  # or one theme, or a path to a .css file
live_reload = true
smooth_scroll = true
copy_on_select = false

[mermaid]
engine = "pele"  # GitHub-style box with zoom, copy, and expand buttons
# engine = "obsidian"  # plain Mermaid, drawn as Obsidian draws it
# Unset, each theme picks its own
```

Built-in themes: `github-light`, `github-dark`, `obsidian-light`, `obsidian-dark`. To change a few colors or fonts, put CSS in `~/.config/mddog/style.css`. It loads after the theme. To write your own theme, see [docs/themes.md](docs/themes.md).

## Limitations

- mddog refuses to run in Warp and zellij, whose bugs garble scrolling ([Warp#16424](https://github.com/warpdotdev/Warp/issues/16424), [zellij#5573](https://github.com/zellij-org/zellij/issues/5573)). In a multiplexer running inside Warp, mddog can't tell and runs anyway, with the same garbling.
- tmux can't show images yet ([tmux#5445](https://github.com/tmux/tmux/pull/5445)).
- In herdr, scrolling lags outside Ghostty ([herdr#5512](https://github.com/herdrdev/herdr/issues/5512)). Starting herdr with `TERM_PROGRAM=ghostty herdr` fixes it, but herdr then also treats the terminal as Ghostty for notifications and focus.

## How it works

mddog turns the file into the HTML GitHub's Markdown API returns, lays it out in a hidden WebKit view, and sends the page to the terminal as image strips over the kitty graphics protocol. Scrolling moves the strips without rendering again. The Claude Code plugin runs `mdd --headless`, which writes the strips as PNG files instead. [ARCHITECTURE.md](ARCHITECTURE.md) has the details.

## License

MIT. Bundles [github-markdown-css](https://github.com/sindresorhus/github-markdown-css), [Primer](https://primer.style), [Mermaid](https://mermaid.js.org), [MathJax](https://www.mathjax.org), and [Pele](https://pele.run), each under its own license.
