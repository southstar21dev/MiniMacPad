//! The pad's own firmware: telling MiniMacPad's firmware from the maker's, setting the
//! backlight, and installing firmware updates with the chip's USB bootloader.

use crate::device::{PID, VID};
use hidapi::HidApi;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

/// The firmware built into this app, from `firmware/` (see firmware/makefile).
pub const FIRMWARE_HEX: &str = include_str!("../firmware/minimacpad.hex");
/// Its version, as the pad reports it in the USB device version (BCD, 0x0201 = 2.1).
pub const FIRMWARE_VERSION: u16 = 0x0203;
/// MiniMacPad firmware reports 2.0 or higher; the maker's firmware reports less.
const FIRST_OWN_VERSION: u16 = 0x0200;
/// First version that can be told to enter flashing mode over USB.
const FIRST_REMOTE_BOOT: u16 = 0x0201;

/// The CH552's USB bootloader.
const BOOT_VID: u16 = 0x4348;
const BOOT_PID: u16 = 0x55e0;

/// The firmware caps each LED's r+g+b at this, to stay within USB power
/// (LED_SUM_MAX in firmware/minimacpad.c).
const LED_SUM_MAX: u32 = 300;

/// Vendor output report for MiniMacPad firmware commands.
const CMD_REPORT: u8 = 5;
const CMD_BACKLIGHT: u8 = 1;
const CMD_BOOT: u8 = 2;

/// "2.1" for 0x0201.
pub fn version_text(bcd: u16) -> String {
    format!("{}.{}", bcd >> 8, bcd & 0xff)
}

/// Which firmware the plugged-in pad runs.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Firmware {
    /// The pad's original firmware from its maker.
    Original,
    /// MiniMacPad's firmware, with its version.
    Own(u16),
}

impl Firmware {
    pub fn is_own(self) -> bool {
        matches!(self, Firmware::Own(_))
    }

    /// True if this app carries newer firmware than the pad runs.
    pub fn update_available(self) -> bool {
        matches!(self, Firmware::Own(v) if v < FIRMWARE_VERSION)
    }
}

/// The plugged-in pad's firmware, if a pad is plugged in.
pub fn firmware() -> Option<Firmware> {
    let devices = rusb::devices().ok()?;
    let desc = devices.iter().find_map(|d| {
        d.device_descriptor().ok().filter(|x| x.vendor_id() == VID && x.product_id() == PID)
    })?;
    let v = desc.device_version();
    let bcd = (u16::from(v.major()) << 8) | (u16::from(v.minor()) << 4) | u16::from(v.sub_minor());
    Some(if bcd >= FIRST_OWN_VERSION { Firmware::Own(bcd) } else { Firmware::Original })
}

/// True while a pad waits in the chip's USB flashing mode.
pub fn in_flashing_mode() -> bool {
    rusb::devices()
        .map(|list| {
            list.iter().any(|d| {
                d.device_descriptor().is_ok_and(|x| x.vendor_id() == BOOT_VID && x.product_id() == BOOT_PID)
            })
        })
        .unwrap_or(false)
}

/// Send one vendor command to a pad running MiniMacPad firmware.
fn command(body: [u8; 7]) -> Result<(), String> {
    let api = HidApi::new().map_err(|e| e.to_string())?;
    // Shared access: taking a keyboard exclusively needs root on macOS.
    api.set_open_exclusive(false);
    let info = api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
        .max_by_key(|d| d.usage_page() == 0xff00)
        .ok_or("pad not found (is it plugged in?)")?;
    let dev = info.open_device(&api).map_err(|e| format!("cannot open the pad: {e}"))?;
    let mut report = [0u8; 8];
    report[0] = CMD_REPORT;
    report[1..].copy_from_slice(&body);
    dev.write(&report).map_err(|e| format!("cannot write to the pad: {e}"))?;
    Ok(())
}

/// What the key backlight does.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    Off,
    #[default]
    Solid,
    /// Each key lights while it is held down.
    Press,
    /// Colours cycle across the keys.
    Rainbow,
}

impl Effect {
    pub const ALL: [Effect; 4] = [Effect::Solid, Effect::Press, Effect::Rainbow, Effect::Off];

    pub fn label(self) -> &'static str {
        match self {
            Effect::Off => "Off",
            Effect::Solid => "Solid colour",
            Effect::Press => "Light while pressed",
            Effect::Rainbow => "Rainbow",
        }
    }

    fn code(self) -> u8 {
        match self {
            Effect::Off => 0,
            Effect::Solid => 1,
            Effect::Press => 2,
            Effect::Rainbow => 3,
        }
    }

    pub fn parse(name: &str) -> Option<Effect> {
        match name {
            "off" => Some(Effect::Off),
            "solid" => Some(Effect::Solid),
            "press" => Some(Effect::Press),
            "rainbow" => Some(Effect::Rainbow),
            _ => None,
        }
    }
}

/// The backlight as chosen in the app. The pad keeps its own copy; this one lets the
/// app show what was last set.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Backlight {
    pub effect: Effect,
    /// Colour at full brightness.
    pub color: [u8; 3],
    /// 0..=100 percent.
    pub brightness: u8,
}

impl Default for Backlight {
    fn default() -> Self {
        Backlight { effect: Effect::Solid, color: [255, 255, 255], brightness: 40 }
    }
}

impl Backlight {
    fn path() -> PathBuf {
        crate::config::path().with_file_name("backlight.json")
    }

    pub fn load() -> Backlight {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn store(&self) {
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(), text);
        }
    }

    /// What the LEDs get. 100% brightness is the brightest the firmware's power cap
    /// allows for this colour, so every step of the slider shows, and 0% is off. The
    /// rainbow only takes its peak level from this.
    fn level(&self) -> [u8; 3] {
        let percent = u32::from(self.brightness.min(100));
        if self.effect == Effect::Rainbow {
            let peak = (255 * percent / 100) as u8;
            return [peak; 3];
        }
        let sum: u32 = self.color.iter().map(|&c| u32::from(c)).sum();
        let room = if sum > LED_SUM_MAX { LED_SUM_MAX * 1000 / sum } else { 1000 };
        self.color.map(|c| (u32::from(c) * percent * room / 100_000) as u8)
    }

    /// Show this on the pad. With `save`, the pad also keeps it after unplugging;
    /// leave it off while a setting is still being dragged, to spare the chip's memory.
    pub fn apply(&self, save: bool) -> Result<(), String> {
        let [r, g, b] = self.level();
        command([CMD_BACKLIGHT, self.effect.code(), r, g, b, u8::from(save), 0])
    }
}

/// Ask a pad on MiniMacPad firmware 2.1+ to restart into its USB flashing mode.
pub fn enter_flashing_mode() -> Result<(), String> {
    command([CMD_BOOT, b'B', b'O', b'O', b'T', 0, 0])
}

/// Whether `enter_flashing_mode` works on this firmware, or the knob must be held while
/// plugging in.
pub fn can_enter_flashing_mode(fw: Firmware) -> bool {
    matches!(fw, Firmware::Own(v) if v >= FIRST_REMOTE_BOOT)
}

/// The wchisp flashing tool: bundled next to this program, or installed with cargo.
fn wchisp() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("wchisp")));
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [beside, Some(PathBuf::from(format!("{home}/.cargo/bin/wchisp")))];
    candidates.into_iter().flatten().find(|p| p.exists()).or_else(|| {
        let out = Command::new("which").arg("wchisp").output().ok()?;
        out.status.success().then(|| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
    })
}

/// Write `hex` (Intel HEX) to a pad in flashing mode, waiting up to `wait` for one to
/// appear. `status` hears what is happening. Returns once the pad runs the new firmware,
/// or with a hint if it needs replugging.
pub fn flash(hex: &str, wait: Duration, status: impl Fn(&str)) -> Result<String, String> {
    let tool = wchisp().ok_or("the wchisp flashing tool is missing")?;
    let file = std::env::temp_dir().join(format!("minimacpad-firmware-{}.hex", std::process::id()));
    std::fs::write(&file, hex).map_err(|e| e.to_string())?;

    status("Waiting for the pad in flashing mode…");
    let start = Instant::now();
    while !in_flashing_mode() {
        if start.elapsed() > wait {
            let _ = std::fs::remove_file(&file);
            return Err("the pad did not enter flashing mode".into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    status("Writing the firmware…");
    let out = Command::new(&tool).arg("flash").arg(&file).output().map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&file);
    let out = out?;
    let log = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    if !out.status.success() || !log.contains("Verify OK") {
        let last = log.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("unknown error");
        return Err(format!("flashing failed: {last}"));
    }

    status("Restarting the pad…");
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        if firmware().is_some() {
            return Ok("Pad firmware installed".into());
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    Ok("Firmware installed: unplug the pad and plug it in again".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_version_matches_the_firmware_source() {
        let config = include_str!("../firmware/src/config.h");
        let line = config.lines().find(|l| l.contains("USB_DEVICE_VERSION")).unwrap();
        let hex = line.split_whitespace().nth(2).unwrap().trim_start_matches("0x");
        assert_eq!(u16::from_str_radix(hex, 16).unwrap(), FIRMWARE_VERSION);
    }

    #[test]
    fn brightness_spans_what_the_power_cap_allows() {
        let white = |brightness| Backlight { effect: Effect::Solid, color: [255; 3], brightness };
        assert_eq!(white(100).level(), [99, 99, 99]); // sum 297, just under the cap
        assert_eq!(white(50).level(), [49, 49, 49]);
        assert_eq!(white(0).level(), [0, 0, 0]);
        let orange = Backlight { effect: Effect::Solid, color: [255, 0, 0], brightness: 50 };
        assert_eq!(orange.level(), [127, 0, 0]); // under the cap: plain scaling
        let rainbow = Backlight { effect: Effect::Rainbow, color: [0; 3], brightness: 100 };
        assert_eq!(rainbow.level(), [255; 3]);
    }
}
