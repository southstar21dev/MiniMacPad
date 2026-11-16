//! Self-update from GitHub releases (docs/RELEASING.md).
//!
//! A release counts once its manifest, `minimacpad-update.json`, is attached:
//! the release workflow uploads it last, so a half-built release is ignored.
//! A download is installed only if its SHA-256 matches the manifest and it is
//! signed with the same certificate as the running app.

use crate::service;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const REPO: &str = "southstar21dev/MiniMacPad";
const MANIFEST: &str = "minimacpad-update.json";

/// The version this build was made as: a release's tag, or the last tag plus
/// build metadata for a local build (scripts/version.sh).
pub const VERSION: &str = match option_env!("MINIMACPAD_VERSION") {
    Some(v) => v,
    None => "0.0.0",
};

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: String,
    url: String,
    sha256: String,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct GithubRelease {
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct MacFile {
    file: String,
    sha256: String,
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    mac: MacFile,
}

/// MAJOR.MINOR.PATCH of a version, ignoring a leading "v", a pre-release
/// suffix and build metadata.
fn triple(version: &str) -> Option<(u64, u64, u64)> {
    let core = version.trim().trim_start_matches('v');
    let core = core.split(['+', '-']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    let triple = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(triple)
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((triple(candidate), triple(current)), (Some(a), Some(b)) if a > b)
}

fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn fetch(url: &str) -> Result<String, String> {
    run(Command::new("curl").args(["-fsSL", "--max-time", "20", "-H", "User-Agent: MiniMacPad", url]))
        .map_err(|e| if e.is_empty() { "could not reach GitHub".into() } else { e })
}

/// The latest complete release, if it is newer than this build.
pub fn check() -> Result<Option<Release>, String> {
    let latest = fetch(&format!("https://api.github.com/repos/{REPO}/releases/latest"))?;
    let latest: GithubRelease = serde_json::from_str(&latest).map_err(|e| e.to_string())?;
    let Some(manifest) = latest.assets.iter().find(|a| a.name == MANIFEST) else {
        return Ok(None); // still building
    };
    let manifest: Manifest = serde_json::from_str(&fetch(&manifest.browser_download_url)?).map_err(|e| e.to_string())?;
    if !is_newer(&manifest.version, VERSION) {
        return Ok(None);
    }
    let zip = latest
        .assets
        .iter()
        .find(|a| a.name == manifest.mac.file)
        .ok_or("the release is missing its Mac download")?;
    Ok(Some(Release { version: manifest.version, url: zip.browser_download_url.clone(), sha256: manifest.mac.sha256 }))
}

/// The .app this program is running from.
fn app_bundle() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    exe.ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .map(Path::to_path_buf)
        .ok_or_else(|| "not running from an app bundle".to_string())
}

/// The code-signing requirement that identifies builds of this app, taken
/// from the running app's own signature.
fn own_requirement(app: &Path) -> Result<String, String> {
    let out = Command::new("codesign").args(["-d", "-r-"]).arg(app).output().map_err(|e| e.to_string())?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let requirement = text
        .lines()
        .find_map(|l| l.split_once("designated => ").map(|(_, r)| r.trim().to_string()))
        .ok_or("this app is not signed")?;
    if requirement.contains("cdhash") {
        return Err("this is a local build without a release signature; update it by building again".into());
    }
    Ok(requirement)
}

/// Download, verify and install a release over the running app, then start
/// the new version. Returns only on failure; on success the process exits.
pub fn install(release: &Release) -> Result<(), String> {
    let app = app_bundle()?;
    let requirement = own_requirement(&app)?;
    let tmp = std::env::temp_dir().join(format!("minimacpad-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = install_from(release, &app, &requirement, &tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    let _ = std::fs::remove_file(crate::config::pause_path());
    let _ = Command::new("open").arg("-n").arg(&app).spawn();
    std::process::exit(0);
}

fn install_from(release: &Release, app: &Path, requirement: &str, tmp: &Path) -> Result<(), String> {
    let zip = tmp.join("update.zip");
    run(Command::new("curl").args(["-fsSL", "--max-time", "300", "-H", "User-Agent: MiniMacPad", "-o"]).arg(&zip).arg(&release.url))
        .map_err(|e| format!("download failed: {e}"))?;
    let sum = run(Command::new("shasum").args(["-a", "256"]).arg(&zip))?;
    if sum.split_whitespace().next() != Some(release.sha256.as_str()) {
        return Err("the download does not match the release's checksum".into());
    }
    let unpacked = tmp.join("unpacked");
    run(Command::new("ditto").args(["-x", "-k"]).arg(&zip).arg(&unpacked))?;
    let name = app.file_name().ok_or("bad app path")?;
    let new_app = unpacked.join(name);
    if !new_app.is_dir() {
        return Err("the download does not contain the app".into());
    }
    run(Command::new("codesign").args(["--verify", "--deep", "--strict"]).arg(format!("-R={requirement}")).arg(&new_app))
        .map_err(|_| "the download is not signed by the same developer as this app".to_string())?;

    // Swap the bundles, putting the old one back if the copy fails.
    let old = tmp.join("old.app");
    if std::fs::rename(app, &old).is_err() {
        run(Command::new("ditto").arg(app).arg(&old))?;
        std::fs::remove_dir_all(app).map_err(|e| format!("could not replace {}: {e}", app.display()))?;
    }
    if let Err(e) = run(Command::new("ditto").arg(&new_app).arg(app)) {
        let _ = std::fs::remove_dir_all(app);
        let _ = run(Command::new("ditto").arg(&old).arg(app));
        return Err(format!("could not install the update: {e}"));
    }
    if service::installed() {
        service::install(&app.join("Contents/MacOS/minipad"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn compares_semantic_versions() {
        assert!(is_newer("1.0.0", "0.0.0+12.abc1234"));
        assert!(is_newer("v1.10.0", "1.9.3"));
        assert!(is_newer("2.0.0", "1.99.99"));
        assert!(!is_newer("1.2.0", "1.2.0+40.abc1234"));
        assert!(!is_newer("1.1.9", "1.2.0"));
        assert!(!is_newer("nonsense", "1.0.0"));
    }
}
