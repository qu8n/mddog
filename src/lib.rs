#[cfg(not(target_os = "macos"))]
compile_error!("mddog runs on macOS only");

pub use render::{Doc, render};

mod render;
mod style;

#[doc(hidden)]
pub mod assets;
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod editor;
#[doc(hidden)]
pub mod engine;
#[doc(hidden)]
pub mod headless;
#[doc(hidden)]
pub mod page;
#[doc(hidden)]
pub mod sweep;
#[doc(hidden)]
pub mod term;
#[doc(hidden)]
pub mod theme;
#[doc(hidden)]
pub mod trace;
#[doc(hidden)]
pub mod viewer;
#[doc(hidden)]
pub mod watch;
