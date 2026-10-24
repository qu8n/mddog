# mddog

Preview Markdown and MDX in the terminal the way GitHub or Obsidian shows them: real fonts, tables, syntax highlighting, math, Mermaid diagrams, and images, drawn by WebKit.

Needs macOS and a terminal with the [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/): Ghostty, kitty, WezTerm, or iTerm2.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/qu8n/mddog/main/install.sh | sh
```

This puts `mdd` in `~/.local/bin` and adds a login agent that starts a warm spare, so even the first preview after login opens in about 40 ms instead of 250 ms. Run it again to update. `MDDOG_INSTALL_DIR` picks another folder, and `MDDOG_NO_AGENT=1` skips the agent.

From source: `cargo install --path . && mdd --install-agent`.

## Usage

```sh
mdd              # pick a file in this git repo (or folder)
mdd docs/        # pick a file in a folder
mdd README.md    # preview a file
command | mdd    # preview standard input
```

The preview reloads when the file changes. Press `?` for every key.

| Key | Action |
| --- | --- |
| `j` `k`, `Space` `b`, wheel | scroll |
| `/`, `n` `N` | search |
| `t`, `1`-`9` | table of contents, jump to a section |
| click | open a link, press a copy button |
| drag, `Cmd-c` | select and copy text |
| `Backspace` | go back |
| `Ctrl-p` | open another file |
| `e` | edit in `$EDITOR` |
| `q` | quit |

`Cmd-c` needs the [kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/) and a terminal that doesn't keep `Cmd-c` for itself. Ghostty and kitty work as they are. In WezTerm, set `enable_kitty_keyboard = true` and unbind `Cmd-c`. Elsewhere, use `copy_on_select = true`. Copies also go out as OSC 52, so they work over SSH.

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
engine = "pele"  # or "obsidian". Unset: the theme picks
```

Built-in themes: `github-light`, `github-dark`, `obsidian-light`, `obsidian-dark`. For small changes, put CSS in `style.css` next to the config. To write your own theme, see [docs/themes.md](docs/themes.md).

## Limitations

- mddog refuses to run in Warp and zellij, whose bugs garble scrolling ([Warp#16424](https://github.com/warpdotdev/Warp/issues/16424), [zellij#5573](https://github.com/zellij-org/zellij/issues/5573)). It can't detect a multiplexer running inside Warp.
- tmux can't show images yet ([tmux#5445](https://github.com/tmux/tmux/pull/5445)).
- In herdr, scrolling lags outside Ghostty ([herdr#5512](https://github.com/herdrdev/herdr/issues/5512)). `TERM_PROGRAM=ghostty herdr` fixes it, but herdr then treats the terminal as Ghostty.

## How it works

mddog turns the file into the HTML GitHub's Markdown API returns, lays it out in an off-screen WebKit view, and sends the page to the terminal as kitty graphics tiles. Scrolling moves the tiles without rendering again. The Claude Code plugin runs `mdd --headless`, which writes the tiles as PNG files instead. [ARCHITECTURE.md](ARCHITECTURE.md) has the details.

## License

MIT. Bundles [github-markdown-css](https://github.com/sindresorhus/github-markdown-css), [Primer](https://primer.style), [Mermaid](https://mermaid.js.org), [MathJax](https://www.mathjax.org), and [Pele](https://pele.run), each under its own license.
