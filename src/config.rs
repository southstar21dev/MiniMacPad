//! The TOML config file: which control does what, globally and per app.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::SystemTime;

/// One thing to do. Exactly one of the main fields should be set.
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Action {
    /// Launch or focus an app by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open: Option<String>,
    /// Open a URL in the default browser.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Run a shell command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Press a keyboard shortcut, e.g. "cmd+shift+4".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keys: Option<String>,
    /// Change the output volume by this many percent (negative lowers it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<f32>,
    /// Toggle mute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mute: Option<bool>,
    /// Send this prompt to an AI assistant. (`claude` is the older name.)
    #[serde(alias = "claude", skip_serializing_if = "Option::is_none")]
    pub ask: Option<String>,
    /// Which assistant: "claude" (default) or "antigravity" run on the command
    /// line; "claude-app", "chatgpt", "gemini" and "antigravity-app" type the
    /// prompt into the desktop app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with: Option<String>,
    /// Desktop apps only: shortcut pressed before pasting, to start a new
    /// chat. Leave unset for the app's usual one; "" presses nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
    /// Extra text sent with the prompt: "none" (default), "clipboard" or "selection".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    /// Command-line assistants only. Where the answer goes: "clipboard"
    /// (default), "paste" or "terminal".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Show a notification with this text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum Binding {
    One(Action),
    Many(Vec<Action>),
}

impl Binding {
    pub fn actions(&self) -> Vec<Action> {
        match self {
            Binding::One(a) => vec![a.clone()],
            Binding::Many(v) => v.clone(),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Path to the claude binary, if it is not on PATH.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_bin: Option<String>,
    #[serde(default)]
    pub keys: BTreeMap<String, Binding>,
    /// Per-app overrides, keyed by the app's name as shown in the menu bar.
    #[serde(default)]
    pub apps: BTreeMap<String, BTreeMap<String, Binding>>,
}

pub fn path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".config/minipad/config.toml")
}

/// While the settings app is the active window it keeps this file fresh, and
/// the daemon leaves the pad alone so presses can be used to pick controls.
pub fn pause_path() -> PathBuf {
    path().with_file_name("paused")
}

/// True if the settings app refreshed the pause file within the last few
/// seconds. A stale file (the app crashed) stops counting on its own.
pub fn paused() -> bool {
    std::fs::metadata(pause_path())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age.as_secs() < 3)
}

fn mtime() -> Option<SystemTime> {
    std::fs::metadata(path()).and_then(|m| m.modified()).ok()
}

pub fn parse() -> Result<Config, String> {
    let text = std::fs::read_to_string(path()).map_err(|e| e.to_string())?;
    toml::from_str(&text).map_err(|e| e.to_string())
}

/// Write the config file. The running daemon picks it up on the next press.
pub fn save(config: &Config) -> Result<(), String> {
    let text = toml::to_string(config).map_err(|e| e.to_string())?;
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, text).map_err(|e| e.to_string())
}

/// Holds the current config and reloads it when the file changes.
pub struct Store {
    pub config: Config,
    stamp: Option<SystemTime>,
}

impl Store {
    /// Load the config, writing the example file first if none exists.
    pub fn load() -> Store {
        let p = path();
        if !p.exists() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&p, include_str!("../config.example.toml"));
            eprintln!("wrote starter config to {}", p.display());
        }
        let config = parse().unwrap_or_else(|e| {
            eprintln!("config error: {e}");
            Config::default()
        });
        Store { config, stamp: mtime() }
    }

    /// Returns an error message if the file changed but no longer parses;
    /// the previous config stays active in that case.
    pub fn reload_if_changed(&mut self) -> Option<String> {
        let now = mtime();
        if now == self.stamp {
            return None;
        }
        self.stamp = now;
        match parse() {
            Ok(c) => {
                self.config = c;
                eprintln!("config reloaded");
                None
            }
            Err(e) => Some(e),
        }
    }

    pub fn lookup(&self, control: &str, app: Option<&str>) -> Option<&Binding> {
        app.and_then(|a| self.config.apps.get(a))
            .and_then(|m| m.get(control))
            .or_else(|| self.config.keys.get(control))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the settings app writes must read back the same.
    #[test]
    fn saved_config_reads_back() {
        let mut config: Config = toml::from_str(include_str!("../config.example.toml")).unwrap();
        let two = vec![
            Action { keys: Some("cmd+c".into()), ..Default::default() },
            Action { shell: Some("echo \"hi\"\nsay done".into()), ..Default::default() },
        ];
        config.keys.insert("key6".into(), Binding::Many(two));
        let safari = BTreeMap::from([("knob_left".to_string(), Binding::One(Action { volume: Some(-2.0), ..Default::default() }))]);
        config.apps.insert("Safari".into(), safari);
        config.claude_bin = Some("/usr/local/bin/claude".into());

        let text = toml::to_string(&config).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.claude_bin, config.claude_bin);
        assert_eq!(back.keys.len(), 9);
        assert_eq!(back.keys["key6"].actions(), config.keys["key6"].actions());
        assert_eq!(back.apps["Safari"]["knob_left"].actions()[0].volume, Some(-2.0));
    }
}
