#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("mddog runs on macOS and Linux");

#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod editor;
#[doc(hidden)]
pub mod headless;
#[doc(hidden)]
pub mod render;
#[doc(hidden)]
pub mod resume;
#[doc(hidden)]
pub mod sweep;
#[doc(hidden)]
pub mod term;
#[doc(hidden)]
pub mod theme;
#[doc(hidden)]
pub mod tmp;
#[doc(hidden)]
pub mod trace;
#[doc(hidden)]
pub mod viewer;
#[doc(hidden)]
pub mod watch;
