<h1 align="center">
  <img src="assets/logo.png" width="150" alt="Logo"/><br/>
  mddog
</h1>

<p align="center">
  <a href="https://github.com/qu8n/mddog/releases/latest"><img src="https://img.shields.io/github/v/release/qu8n/mddog" alt="Latest release"/></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/qu8n/mddog" alt="License"/></a>
  <img src="https://img.shields.io/badge/platform-macOS-lightgrey?logo=apple" alt="Platform: macOS"/>
  <img src="https://img.shields.io/badge/built%20with-Rust-orange?logo=rust" alt="Built with Rust"/>
</p>

<p align="center">
  Real Markdown rendering, right in your terminal
</p>

https://github.com/user-attachments/assets/982f59b8-5723-4da3-9665-11384b4fda58

## Install

```sh
brew install qu8n/tap/mddog
```

Or, without Homebrew:

```sh
curl -fsSL https://raw.githubusercontent.com/qu8n/mddog/main/install.sh | sh
```

### Requirements

- macOS
- A terminal that supports the [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/): Ghostty, iTerm2, kitty, WezTerm, or Rex


## Usage

```sh
mdd             # list Markdown files in the current directory to pick from
mdd README.md   # preview a file
```

## Docs

Read the full docs at **[mddog.pages.dev/docs](https://mddog.pages.dev/docs)** for key bindings, Claude Code plugin, configuration, themes, and more.
