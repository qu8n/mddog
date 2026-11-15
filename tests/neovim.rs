//! The Neovim plugin in `lua/mddog/`, run by `tests/nvim/preview.lua` in a headless Neovim against this build's
//! `mdd --serve`. Skipped where no Neovim 0.12 or later is on PATH, such as a CI runner without one.
use std::process::Command;

#[test]
fn neovim_plugin() {
    let has = Command::new("nvim").args(["--clean", "--headless", "-c", "if has('nvim-0.12') | qa | else | cq | endif"]).status();
    if !has.is_ok_and(|s| s.success()) {
        eprintln!("skipped: no Neovim 0.12 or later on PATH");
        return;
    }
    let out = Command::new("nvim")
        .args(["--clean", "--headless", "-n", "-l", "tests/nvim/preview.lua"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("MDDOG_BIN", env!("CARGO_BIN_EXE_mdd"))
        .env("MDDOG_CACHE_DIR", std::env::temp_dir().join("mddog-test-cache"))
        .output()
        .expect("can't run nvim");
    let (stdout, stderr) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{stdout}{stderr}");
}
