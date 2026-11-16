//! One-time programming of the pad's own memory so every control sends a
//! unique, otherwise unassigned key code. Protocol as documented by the
//! open-source ch57x-keyboard-tool project for product id 0x8890.
//!
//! The pad takes its configuration on a separate USB interface that macOS
//! attaches no driver to, so this talks to it with raw USB.

use crate::device::{CONTROLS, PID, VID};
use rusb::{Direction, TransferType};
use std::thread::sleep;
use std::time::Duration;

type Handle = rusb::DeviceHandle<rusb::GlobalContext>;

fn send(dev: &Handle, endpoint: u8, msg: &[u8]) -> Result<(), String> {
    let mut buf = [0u8; 64];
    buf[..msg.len()].copy_from_slice(msg);
    let n = dev
        .write_interrupt(endpoint, &buf, Duration::from_millis(500))
        .map_err(|e| e.to_string())?;
    if n != buf.len() {
        return Err(format!("short write ({n} bytes)"));
    }
    sleep(Duration::from_millis(10));
    Ok(())
}

/// The pad's own id for each control, in config order. The six keys are wired
/// 3 2 1 down the left column and 6 5 4 down the right (knob at the top),
/// while config names go left to right, top to bottom; the knob is 13 (left),
/// 14 (press), 15 (right).
fn key_id(index: usize) -> u8 {
    [3, 6, 2, 5, 1, 4, 13, 14, 15][index]
}

fn bind(dev: &Handle, ep: u8, layer: u8, id: u8, code: u8) -> Result<(), String> {
    let kind = ((layer + 1) << 4) | 0x01;
    send(dev, ep, &[0x03, 0xfe, layer + 1, 0x01, 0x01])?;
    send(dev, ep, &[0x03, id, kind, 1, 0, 0, 0])?;
    send(dev, ep, &[0x03, id, kind, 1, 1, 0, code])?;
    send(dev, ep, &[0x03, 0xaa, 0xaa])
}

/// The interface and address of the pad's interrupt OUT endpoint.
fn config_endpoint(dev: &rusb::Device<rusb::GlobalContext>) -> Result<(u8, u8), String> {
    let conf = dev.config_descriptor(0).map_err(|e| e.to_string())?;
    for iface in conf.interfaces() {
        for desc in iface.descriptors() {
            for ep in desc.endpoint_descriptors() {
                if ep.direction() == Direction::Out && ep.transfer_type() == TransferType::Interrupt {
                    return Ok((iface.number(), ep.address()));
                }
            }
        }
    }
    Err("pad has no configuration endpoint".into())
}

/// Open the pad's configuration interface.
fn open() -> Result<(Handle, u8), String> {
    let dev = rusb::devices()
        .map_err(|e| e.to_string())?
        .iter()
        .find(|d| {
            d.device_descriptor()
                .map(|x| x.vendor_id() == VID && x.product_id() == PID)
                .unwrap_or(false)
        })
        .ok_or("pad not found (is it plugged in?)")?;
    let (iface, ep) = config_endpoint(&dev)?;
    let handle = dev.open().map_err(|e| format!("open: {e}"))?;
    handle.claim_interface(iface).map_err(|e| format!("claim interface {iface}: {e}"))?;
    Ok((handle, ep))
}

/// Switch the pad's backlight mode. What each number looks like is up to the
/// pad's firmware.
pub fn set_led(mode: u8) -> Result<(), String> {
    let (handle, ep) = open()?;
    send(&handle, ep, &[])?;
    send(&handle, ep, &[0x03, 0xa1, 0x01])?;
    send(&handle, ep, &[0x03, 0xb0, 0x18, mode])?;
    send(&handle, ep, &[0x03, 0xaa, 0xa1])
}

pub fn run(args: &[String]) -> Result<(), String> {
    let mut layers = 1u8;
    match args {
        [] => {}
        [flag, n] if flag == "--layers" => {
            layers = n.parse().ok().filter(|v| (1..=3).contains(v)).ok_or("--layers takes 1-3")?;
        }
        _ => return Err("usage: minipad flash [--layers 1-3]".into()),
    }

    let (handle, ep) = open()?;

    send(&handle, ep, &[])?; // wake-up packet
    for layer in 0..layers {
        for (index, (name, code)) in CONTROLS.iter().enumerate() {
            bind(&handle, ep, layer, key_id(index), *code).map_err(|e| format!("{name}: {e}"))?;
            println!("layer {} {name} -> code {code:#04x}", layer + 1);
        }
    }
    println!("done");
    Ok(())
}
