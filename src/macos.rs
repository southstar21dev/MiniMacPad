//! Direct macOS calls: output volume (CoreAudio) and synthetic key presses
//! (CoreGraphics). Hand-written bindings keep the binary small.

use std::ffi::c_void;
use std::ptr;

#[repr(C)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectGetPropertyData(
        id: u32,
        addr: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
    fn AudioObjectSetPropertyData(
        id: u32,
        addr: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> i32;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreateKeyboardEvent(source: *const c_void, key: u16, down: bool) -> *mut c_void;
    fn CGEventSetFlags(event: *mut c_void, flags: u64);
    fn CGEventPost(tap: u32, event: *mut c_void);
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDRequestAccess(request: u32) -> bool;
}

/// Ask macOS for permission to read key presses (Input Monitoring). Shows the
/// system prompt the first time; returns whether access is granted.
pub fn request_input_monitoring() -> bool {
    unsafe { IOHIDRequestAccess(1) }
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(obj: *const c_void);
}

const fn fourcc(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

const SYSTEM_OBJECT: u32 = 1;
const DEFAULT_OUTPUT: Address = Address { selector: fourcc(b"dOut"), scope: fourcc(b"glob"), element: 0 };
const MAIN_VOLUME: Address = Address { selector: fourcc(b"vmvc"), scope: fourcc(b"outp"), element: 0 };
const MUTE: Address = Address { selector: fourcc(b"mute"), scope: fourcc(b"outp"), element: 0 };

fn get<T: Default>(id: u32, addr: &Address) -> Option<T> {
    let mut value = T::default();
    let mut size = std::mem::size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(id, addr, 0, ptr::null(), &mut size, &mut value as *mut T as *mut c_void)
    };
    (status == 0).then_some(value)
}

fn set<T>(id: u32, addr: &Address, value: T) -> bool {
    let size = std::mem::size_of::<T>() as u32;
    unsafe { AudioObjectSetPropertyData(id, addr, 0, ptr::null(), size, &value as *const T as *const c_void) == 0 }
}

/// Change the default output device's volume by `percent` points.
pub fn change_volume(percent: f32) -> Result<(), &'static str> {
    let dev: u32 = get(SYSTEM_OBJECT, &DEFAULT_OUTPUT).ok_or("no output device")?;
    let current: f32 = get(dev, &MAIN_VOLUME).ok_or("this output has no volume control")?;
    let next = (current + percent / 100.0).clamp(0.0, 1.0);
    if percent > 0.0 {
        set(dev, &MUTE, 0u32); // turning up un-mutes, like the built-in keys
    }
    set(dev, &MAIN_VOLUME, next).then_some(()).ok_or("could not set volume")
}

pub fn toggle_mute() -> Result<(), &'static str> {
    let dev: u32 = get(SYSTEM_OBJECT, &DEFAULT_OUTPUT).ok_or("no output device")?;
    let muted: u32 = get(dev, &MUTE).ok_or("this output cannot be muted")?;
    set(dev, &MUTE, u32::from(muted == 0)).then_some(()).ok_or("could not set mute")
}

const KEYCODES: &[(&str, u16)] = &[
    ("a", 0), ("s", 1), ("d", 2), ("f", 3), ("h", 4), ("g", 5), ("z", 6), ("x", 7), ("c", 8),
    ("v", 9), ("b", 11), ("q", 12), ("w", 13), ("e", 14), ("r", 15), ("y", 16), ("t", 17),
    ("1", 18), ("2", 19), ("3", 20), ("4", 21), ("6", 22), ("5", 23), ("=", 24), ("9", 25),
    ("7", 26), ("-", 27), ("8", 28), ("0", 29), ("]", 30), ("o", 31), ("u", 32), ("[", 33),
    ("i", 34), ("p", 35), ("return", 36), ("enter", 36), ("l", 37), ("j", 38), ("'", 39),
    ("k", 40), (";", 41), ("\\", 42), (",", 43), ("/", 44), ("n", 45), ("m", 46), (".", 47),
    ("tab", 48), ("space", 49), ("`", 50), ("delete", 51), ("backspace", 51), ("escape", 53),
    ("esc", 53), ("f1", 122), ("f2", 120), ("f3", 99), ("f4", 118), ("f5", 96), ("f6", 97),
    ("f7", 98), ("f8", 100), ("f9", 101), ("f10", 109), ("f11", 103), ("f12", 111),
    ("home", 115), ("pageup", 116), ("forwarddelete", 117), ("end", 119), ("pagedown", 121),
    ("left", 123), ("right", 124), ("down", 125), ("up", 126),
];

/// Press a shortcut written like "cmd+shift+4". Needs Accessibility permission.
pub fn press_keys(combo: &str) -> Result<(), String> {
    let mut flags = 0u64;
    let mut key = None;
    for part in combo.split('+').map(|p| p.trim().to_lowercase()) {
        match part.as_str() {
            "cmd" | "command" => flags |= 0x10_0000,
            "shift" => flags |= 0x2_0000,
            "ctrl" | "control" => flags |= 0x4_0000,
            "alt" | "opt" | "option" => flags |= 0x8_0000,
            name => {
                let code = KEYCODES.iter().find(|(n, _)| *n == name).map(|(_, c)| *c);
                key = Some(code.ok_or_else(|| format!("unknown key \"{name}\" in \"{combo}\""))?);
            }
        }
    }
    let key = key.ok_or_else(|| format!("no key in \"{combo}\""))?;
    for down in [true, false] {
        unsafe {
            let event = CGEventCreateKeyboardEvent(ptr::null(), key, down);
            if event.is_null() {
                return Err("could not create key event".into());
            }
            CGEventSetFlags(event, flags);
            CGEventPost(0, event);
            CFRelease(event);
        }
    }
    Ok(())
}
