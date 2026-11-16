//! Running the daemon in the background as a login item (a launchd agent).

use std::path::Path;
use std::process::Command;

const LABEL: &str = "dev.southstar21.minimacpad.daemon";

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn plist_path() -> String {
    format!("{}/Library/LaunchAgents/{LABEL}.plist", home())
}

pub fn log_path() -> String {
    format!("{}/Library/Logs/minipad.log", home())
}

fn uid() -> String {
    let out = Command::new("id").arg("-u").output().map(|o| o.stdout).unwrap_or_default();
    String::from_utf8_lossy(&out).trim().to_string()
}

/// True if the login item is set up.
pub fn installed() -> bool {
    Path::new(&plist_path()).exists()
}

/// True if a daemon process is running, however it was started.
pub fn running() -> bool {
    Command::new("pgrep").args(["-x", "minipad"]).output().map(|o| o.status.success()).unwrap_or(false)
}

/// Copy the daemon binary at `exe` into place and start it as a login item,
/// replacing any daemon that is already running.
pub fn install(exe: &Path) -> Result<(), String> {
    let home = home();
    let bin = format!("{home}/.local/bin/minipad");
    std::fs::create_dir_all(format!("{home}/.local/bin")).map_err(|e| e.to_string())?;
    if exe.to_string_lossy() != bin {
        let _ = std::fs::remove_file(&bin);
        std::fs::copy(exe, &bin).map_err(|e| format!("copy {} to {bin}: {e}", exe.display()))?;
    }
    let log = log_path();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{LABEL}</string>
<key>ProgramArguments</key><array><string>{bin}</string><string>run</string></array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><true/>
<key>ProcessType</key><string>Interactive</string>
<key>StandardOutPath</key><string>{log}</string>
<key>StandardErrorPath</key><string>{log}</string>
</dict></plist>
"#
    );
    std::fs::create_dir_all(format!("{home}/Library/LaunchAgents")).map_err(|e| e.to_string())?;
    std::fs::write(plist_path(), plist).map_err(|e| e.to_string())?;
    let domain = format!("gui/{}", uid());
    let _ = Command::new("launchctl").args(["bootout", &format!("{domain}/{LABEL}")]).output();
    let _ = Command::new("pkill").args(["-x", "minipad"]).output();
    let out = Command::new("launchctl")
        .args(["bootstrap", &domain, &plist_path()])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("launchctl: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    println!("installed {bin}\nlog: {log}");
    Ok(())
}

/// If the login item is set up but runs an older daemon than the one at
/// `exe` (the app was updated), replace it.
pub fn sync(exe: &Path) {
    let installed_bin = format!("{}/.local/bin/minipad", home());
    if installed() && exe.exists() && std::fs::read(exe).ok() != std::fs::read(&installed_bin).ok() {
        if let Err(e) = install(exe) {
            eprintln!("could not update the background service: {e}");
        }
    }
}

/// Stop the daemon and remove the login item.
pub fn uninstall() -> Result<(), String> {
    let _ = Command::new("launchctl").args(["bootout", &format!("gui/{}/{LABEL}", uid())]).output();
    let _ = std::fs::remove_file(plist_path());
    let _ = Command::new("pkill").args(["-x", "minipad"]).output();
    println!("removed login item");
    Ok(())
}
