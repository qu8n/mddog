//! A style is the markup added to the HTML from the Markdown, the page around it with its base CSS, and the Mermaid engine to use when neither the config nor the theme names one. The theme's CSS loads on top and can restyle the whole page, as the Obsidian themes do. GitHub's markup is the only style so far, so it sits under every theme.

use kuchikiki::NodeRef;

use crate::config;
use crate::page::Opts;

pub(crate) mod github;

pub struct Style {
    /// Adds this style's markup to the sanitized HTML. `front` is the file's front matter, if any.
    pub dress: fn(&NodeRef, Option<&str>),
    /// Wraps the dressed HTML in a whole page with this style's CSS and scripts.
    pub page: fn(&str, &Opts) -> String,
    /// The Mermaid engine when the config names none.
    pub mermaid: config::Mermaid,
}

pub const GITHUB: Style = Style { dress: github::dress, page: github::page, mermaid: config::Mermaid::Pele };

pub const DEFAULT: &Style = &GITHUB;
