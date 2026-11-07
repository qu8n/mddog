<h1 align="center">
  <img src="assets/spike.png" width="70" alt="Logo"/><br/>
  mddog
</h1>

<p align="center">
  <a href="https://github.com/qu8n/mddog/releases/latest"><img src="https://img.shields.io/github/v/release/qu8n/mddog" alt="Latest release"/></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/qu8n/mddog" alt="License"/></a>
  <img src="https://img.shields.io/badge/platform-macOS-lightgrey?logo=apple" alt="Platform: macOS"/>
  <img src="https://img.shields.io/badge/built%20with-Rust-orange?logo=rust" alt="Built with Rust"/>
</p>

<p align="center">
  Preview Markdown in your terminal the way a browser renders it.
</p>

https://github.com/user-attachments/assets/3a6fda37-b0d3-4678-bff6-7a5085d48719

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/qu8n/mddog/main/install.sh | sh
```

You need macOS and a terminal that supports the kitty graphics protocol: Ghostty, kitty, WezTerm, iTerm2, or Rex. Rex was tested alone, not inside a multiplexer.

## Usage

```sh
mdd README.md       # preview a file
mdd                 # pick from the Markdown files in this repo or folder
cat notes.md | mdd  # preview standard input
```

Press `?` in the preview to see every key.

## Docs

Read the full docs at **[mddog.pages.dev/docs](https://mddog.pages.dev/docs)**: key bindings, features, the Claude Code plugin, configuration, themes, and limitations.
