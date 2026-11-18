//! Finding the pad, opening its HID interfaces and turning raw reports into events.

use hidapi::{HidApi, HidDevice};
use std::collections::HashSet;
use std::ffi::CString;
use std::sync::mpsc::{channel, Sender};

pub const VID: u16 = 0x1189;
pub const PID: u16 = 0x8890;

/// Key codes the pad is flashed with, in control order. These sit in a range
/// the USB keyboard standard leaves unassigned, so macOS and apps ignore them
/// and only this program (which reads the raw reports) reacts.
pub const CONTROLS: [(&str, u8); 9] = [
    ("key1", 0xf0),
    ("key2", 0xf1),
    ("key3", 0xf2),
    ("key4", 0xf3),
    ("key5", 0xf4),
    ("key6", 0xf5),
    ("knob_left", 0xf6),
    ("knob_press", 0xf7),
    ("knob_right", 0xf8),
];

/// A control went down or came back up. `control` is its config name.
/// One knob step arrives as a press followed at once by a release.
pub struct Event {
    pub control: String,
    pub pressed: bool,
}

/// Name used in the config file for a keyboard usage code.
pub fn key_name(code: u8) -> String {
    CONTROLS
        .iter()
        .find(|(_, c)| *c == code)
        .map(|(n, _)| n.to_string())
        .unwrap_or_else(|| format!("k{code:02x}"))
}

pub struct PadInterface {
    pub path: CString,
    pub interface: i32,
}

/// The pad's keyboard interfaces (the mouse interface is not needed).
pub fn keyboard_interfaces(api: &HidApi) -> Vec<PadInterface> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for d in api.device_list() {
        if d.vendor_id() != VID || d.product_id() != PID {
            continue;
        }
        if d.usage_page() != 1 || d.usage() != 6 {
            continue;
        }
        if seen.insert(d.path().to_owned()) {
            out.push(PadInterface { path: d.path().to_owned(), interface: d.interface_number() });
        }
    }
    out
}

/// Open every keyboard interface. Returns the opened devices and any errors.
pub fn open_all(api: &HidApi) -> (Vec<HidDevice>, Vec<String>) {
    // Shared access: taking a keyboard exclusively needs root on macOS.
    api.set_open_exclusive(false);
    let mut devs = Vec::new();
    let mut errs = Vec::new();
    for i in keyboard_interfaces(api) {
        match api.open_path(&i.path) {
            Ok(d) => devs.push(d),
            Err(e) => errs.push(format!("interface {}: {e}", i.interface)),
        }
    }
    (devs, errs)
}

/// Blocking read loop for one interface. Ends when the pad is unplugged.
pub fn read_loop(dev: HidDevice, tx: Sender<Event>, raw: bool) {
    let mut buf = [0u8; 64];
    let mut held: Vec<u8> = Vec::new();
    let mut media: u16 = 0;
    loop {
        let n = match dev.read(&mut buf) {
            Ok(n) => n,
            Err(_) => return,
        };
        if raw {
            println!("  raw: {:02x?}", &buf[..n]);
        }
        let mut events = Vec::new();
        match buf[0] {
            // Keyboard report: id, modifiers, reserved, key codes...
            1 if n >= 4 => {
                let now: Vec<u8> = buf[3..n].iter().copied().filter(|&c| c > 3).collect();
                for c in &now {
                    if !held.contains(c) {
                        events.push(Event { control: key_name(*c), pressed: true });
                    }
                }
                for c in &held {
                    if !now.contains(c) {
                        events.push(Event { control: key_name(*c), pressed: false });
                    }
                }
                held = now;
            }
            // Consumer (media) report: id, 16-bit usage.
            2 if n >= 3 => {
                let usage = u16::from_le_bytes([buf[1], buf[2]]);
                if usage != media {
                    if media != 0 {
                        events.push(Event { control: format!("m{media:04x}"), pressed: false });
                    }
                    if usage != 0 {
                        events.push(Event { control: format!("m{usage:04x}"), pressed: true });
                    }
                    media = usage;
                }
            }
            _ => {}
        }
        for e in events {
            if tx.send(e).is_err() {
                return;
            }
        }
    }
}

/// Open the pad and feed its events to `handle` until it is unplugged.
/// `on_connect` runs once the pad is open. If it could not be opened, returns
/// why: no errors means the pad is not plugged in.
pub fn pump(
    api: &mut HidApi,
    raw: bool,
    on_connect: impl FnOnce(),
    mut handle: impl FnMut(Event),
) -> Result<(), Vec<String>> {
    let _ = api.refresh_devices();
    let (devs, errs) = open_all(api);
    if devs.is_empty() {
        return Err(errs);
    }
    let (tx, rx) = channel();
    for dev in devs {
        let tx = tx.clone();
        std::thread::spawn(move || read_loop(dev, tx, raw));
    }
    drop(tx);
    eprintln!("pad connected");
    on_connect();
    for event in rx {
        handle(event);
    }
    eprintln!("pad disconnected");
    Ok(())
}
