//! Carrying out the actions named in the config file.

use crate::config::Action;
use crate::macos;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Quote a string for use inside an AppleScript string literal.
fn applescript(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Quote a string as a single shell word.
fn shell_word(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn notify(text: &str) {
    let short: String = text.chars().take(300).collect();
    let script = format!("display notification {} with title \"MiniMacPad\"", applescript(&short));
    let _ = Command::new("osascript").args(["-e", &script]).status();
}

fn run_ok(cmd: &mut Command) -> Result<(), String> {
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn clipboard() -> String {
    Command::new("pbpaste")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn set_clipboard(text: &str) -> Result<(), String> {
    let mut child = Command::new("pbcopy").stdin(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    }
    child.wait().map_err(|e| e.to_string())?;
    Ok(())
}

/// Desktop assistants: (config name, app name, shortcut that starts a new chat).
pub const ASSISTANT_APPS: [(&str, &str, &str); 4] = [
    ("claude-app", "Claude", "cmd+n"),
    ("chatgpt", "ChatGPT", "cmd+n"),
    ("gemini", "Gemini", "cmd+n"),
    ("antigravity-app", "Antigravity", "cmd+l"),
];

fn app_running(app: &str) -> bool {
    let script = format!("application {} is running", applescript(app));
    Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false)
}

/// Type a prompt into a desktop app: bring it forward, start a new chat,
/// paste and press return. Needs Accessibility permission.
fn ask_app(app: &str, shortcut: &str, text: &str) -> Result<(), String> {
    let cold = !app_running(app);
    let before = clipboard();
    set_clipboard(text)?;
    run_ok(Command::new("open").args(["-a", app]))?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while frontmost_app().as_deref() != Some(app) {
        if Instant::now() > deadline {
            return Err(format!("{app} did not come to the front"));
        }
        sleep(Duration::from_millis(200));
    }
    sleep(Duration::from_millis(if cold { 3500 } else { 500 }));
    if !shortcut.trim().is_empty() {
        macos::press_keys(shortcut)?;
        sleep(Duration::from_millis(600));
    }
    macos::press_keys("cmd+v")?;
    sleep(Duration::from_millis(400));
    macos::press_keys("return")?;
    sleep(Duration::from_millis(600));
    set_clipboard(&before)
}

fn ask(action: &Action, prompt: &str, claude_bin: &str) -> Result<(), String> {
    let input = match action.input.as_deref().unwrap_or("none") {
        "none" => String::new(),
        "clipboard" => clipboard(),
        "selection" => {
            macos::press_keys("cmd+c")?;
            sleep(Duration::from_millis(200));
            clipboard()
        }
        other => return Err(format!("unknown input \"{other}\"")),
    };
    let full = if input.trim().is_empty() { prompt.to_string() } else { format!("{prompt}\n\n{input}") };

    let with = action.with.as_deref().unwrap_or("claude");
    if let Some((_, app, usual)) = ASSISTANT_APPS.iter().find(|(name, _, _)| *name == with) {
        return ask_app(app, action.shortcut.as_deref().unwrap_or(usual), &full);
    }
    // Command-line assistants: (program, flag that starts an interactive session).
    let (bin, interactive): (&str, &[&str]) = match with {
        "claude" => (claude_bin, &[]),
        "antigravity" => ("agy", &["-i"]),
        other => return Err(format!("unknown assistant \"{other}\"")),
    };
    let output = action.output.as_deref().unwrap_or("clipboard");
    if !["clipboard", "paste", "terminal"].contains(&output) {
        return Err(format!("unknown output \"{output}\""));
    }

    if output == "terminal" {
        let mut words = vec![shell_word(bin)];
        words.extend(interactive.iter().map(|w| w.to_string()));
        words.push(shell_word(&full));
        let script = format!("tell application \"Terminal\"\nactivate\ndo script {}\nend tell", applescript(&words.join(" ")));
        return run_ok(Command::new("osascript").args(["-e", &script]));
    }

    notify("Asking…");
    let out = Command::new(bin)
        .args(["-p", &full])
        .current_dir(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not start {bin}: {e}"))?;
    let answer = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || answer.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(format!("{bin} failed: {}", if err.is_empty() { answer } else { err }));
    }
    set_clipboard(&answer)?;
    if output == "paste" {
        macos::press_keys("cmd+v")?;
    }
    notify(&answer);
    Ok(())
}

fn run_one(action: &Action, claude_bin: &str) -> Result<(), String> {
    if let Some(app) = &action.open {
        run_ok(Command::new("open").args(["-a", app]))?;
    }
    if let Some(url) = &action.url {
        run_ok(Command::new("open").arg(url))?;
    }
    if let Some(cmd) = &action.shell {
        run_ok(Command::new("/bin/zsh").args(["-c", cmd]))?;
    }
    if let Some(combo) = &action.keys {
        macos::press_keys(combo)?;
    }
    if let Some(step) = action.volume {
        macos::change_volume(step)?;
    }
    if action.mute == Some(true) {
        macos::toggle_mute()?;
    }
    if let Some(prompt) = &action.ask {
        ask(action, prompt, claude_bin)?;
    }
    if let Some(text) = &action.notify {
        notify(text);
    }
    Ok(())
}

/// True for actions quick enough to run on the event loop itself.
fn instant(a: &Action) -> bool {
    a.open.is_none() && a.url.is_none() && a.shell.is_none() && a.ask.is_none() && a.notify.is_none()
}

fn run_all(control: &str, actions: &[Action], claude_bin: &str) {
    for a in actions {
        if let Err(e) = run_one(a, claude_bin) {
            eprintln!("{control}: {e}");
            if !instant(a) {
                notify(&format!("{control}: {e}"));
            }
            return;
        }
    }
}

/// Run a binding's actions in order. Slow ones go to a background thread so
/// the knob stays responsive.
pub fn dispatch(control: String, actions: Vec<Action>, claude_bin: String) {
    if actions.iter().all(instant) {
        run_all(&control, &actions, &claude_bin);
    } else {
        std::thread::spawn(move || run_all(&control, &actions, &claude_bin));
    }
}

/// Pull the app name out of `lsappinfo info` output. Depending on the macOS
/// version that is either `"LSDisplayName"="Safari"` or a block whose first
/// line starts with the quoted name: `"Safari" ASN:0x0-0x1234: (in front)`.
fn parse_app_name(text: &str) -> Option<String> {
    let text = text.trim();
    let rest = text.strip_prefix("\"LSDisplayName\"=").unwrap_or(text);
    let name = rest.strip_prefix('"')?.split('"').next()?;
    (!name.is_empty()).then(|| name.to_string())
}

/// Name of the app in front, as shown in the menu bar.
pub fn frontmost_app() -> Option<String> {
    let out = Command::new("/bin/sh")
        .args(["-c", "lsappinfo info -only name \"$(lsappinfo front)\""])
        .output()
        .ok()?;
    parse_app_name(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::parse_app_name;

    #[test]
    fn reads_both_lsappinfo_formats() {
        assert_eq!(parse_app_name("\"LSDisplayName\"=\"Safari\"\n").as_deref(), Some("Safari"));
        let block = "\"iTerm2\" ASN:0x0-0xc5cc5c: (in front) \n    bundleID=[ NULL ] \n Arch=!!none \n";
        assert_eq!(parse_app_name(block).as_deref(), Some("iTerm2"));
        assert_eq!(parse_app_name(""), None);
    }
}
