<h1 align="center">
  <img src="assets/logo.webp" width="160" alt="Logo"/><br/>
  mddog
</h1>

<p align="center">
  <a href="https://github.com/qu8n/mddog/releases/latest"><img src="https://img.shields.io/github/v/release/qu8n/mddog" alt="Latest release"/></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/qu8n/mddog" alt="License"/></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey" alt="Platform: macOS and Linux"/>
  <img src="https://img.shields.io/badge/built%20with-Rust-orange?logo=rust" alt="Built with Rust"/>
</p>

<p align="center">
  Real Markdown, right in your terminal
</p>

<p align="center">
  <a href="https://github.com/user-attachments/assets/bf496fc9-b348-47e7-bbad-5c2874194c61"><img src="assets/demo.webp" width="100%" alt="mddog rendering Markdown in the terminal"/></a>
</p>

## Why mddog?

Other terminal viewers render Markdown with only characters. Code editors render the real page but live outside the terminal. mddog renders the real page in your terminal, using a single binary without browser nor Electron.

|                                       | mddog | Text-based terminal viewers | Code editor preview |
|---------------------------------------|:-----:|:---------------------------:|:-------------------:|
| Stays in the terminal                 | ✅    | ✅                          | 🚫                  |
| Real fonts and heading sizes          | ✅    | 🚫                          | ✅                  |
| Diagrams and math drawn as graphics   | ✅    | 🚫                          | Some                |
| Images                                | ✅    | Some                        | ✅                  |
| Live reload                           | ✅    | Some                        | ✅                  |
| Pane in Claude Code                   | ✅    | 🚫                          | 🚫                  |

<sub><em>Compared in November 2026 to terminal viewers like glow, leaf, mdcat, Frogmouth, and rich-cli, and to the built-in Markdown previews in VS Code, Cursor, Zed, and JetBrains IDEs.</em></sub>

## Requirements

- macOS or Linux
- A terminal that supports the [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/): Ghostty, iTerm2, kitty, Konsole, WezTerm, or Rex

See [Compatibility](https://mddog.pages.dev/docs/compatibility) for what works, what doesn't, and available workarounds.

## Install

```sh
curl -fsSL https://mddog.pages.dev/install.sh | sh
```

Run the same command again to update.

On macOS, you can use Homebrew instead: `brew install qu8n/tap/mddog` (update with `brew upgrade mddog`).

For Nix, building from source, updating, and uninstalling, see [Install](https://mddog.pages.dev/docs#install) in the docs.

## Usage

```sh
mdd             # list Markdown files in the current directory to pick from
mdd README.md   # view a file
```

## Docs

Read the full docs at [mddog.pages.dev/docs](https://mddog.pages.dev/docs) for the full feature list, optional Claude Code plugin, configuration, themes, and more.

## Credits

The logo was made by Helen Nguyen ([@daymelon](https://instagram.com/daymelon)), commissioned for mddog and not covered by the MIT license.
