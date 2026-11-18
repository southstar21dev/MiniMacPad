//! Shared pieces of the MiniMacPad daemon and its settings app.

pub mod actions;
pub mod config;
pub mod device;
pub mod flash;
pub mod macos;
pub mod pad;
pub mod service;
pub mod update;

/// Apps started by launchd or Finder get a bare PATH; add the usual places tools live.
pub fn extend_path() {
    let home = std::env::var("HOME").unwrap_or_default();
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{home}/.local/bin:/opt/homebrew/bin:/usr/local/bin:{path}"));
}
