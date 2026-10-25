//! A style decides how a rendered document looks: the markup added to the HTML from the Markdown, the page around it with its CSS, and which Mermaid engine draws diagrams. GitHub's look is the default and, for now, the only style.

use kuchikiki::NodeRef;

use crate::config;
use crate::page::Opts;

mod github;

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
