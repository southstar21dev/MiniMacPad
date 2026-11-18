use hidapi::HidApi;
use minipad::device::{self, Event};
use minipad::{actions, config, flash, macos, service};
use std::thread::sleep;
use std::time::{Duration, Instant};

const USAGE: &str = "minipad - driver for the 6-key + knob macro pad

  minipad run         run the daemon (default)
  minipad monitor     print what each control sends (add --raw for raw reports)
  minipad flash       program the pad's keys for this driver (one time)
  minipad led N       switch the backlight mode (0, 1, 2…)
  minipad install     install and start as a login item
  minipad uninstall   stop and remove the login item
  minipad config      print the config file path
  minipad version     print the version";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("run") => daemon(),
        Some("monitor") => monitor(args.iter().any(|a| a == "--raw")),
        Some("flash") => flash::run(&args[1..]),
        Some("led") => match args.get(1).and_then(|m| m.parse().ok()) {
            Some(mode) => flash::set_led(mode),
            None => Err("usage: minipad led N".into()),
        },
        Some("install") => std::env::current_exe().map_err(|e| e.to_string()).and_then(|exe| service::install(&exe)),
        Some("uninstall") => service::uninstall(),
        Some("version") => {
            println!("{}", minipad::update::VERSION);
            Ok(())
        }
        Some("config") => {
            println!("{}", config::path().display());
            Ok(())
        }
        _ => {
            println!("{USAGE}");
            Ok(())
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn daemon() -> Result<(), String> {
    minipad::extend_path();
    let mut api = HidApi::new().map_err(|e| e.to_string())?;
    let mut store = config::Store::load();
    let mut front: (Instant, Option<String>) = (Instant::now(), None);
    // Ask for Input Monitoring ourselves: this lists the daemon in Privacy
    // settings and shows the prompt, instead of failing silently.
    if !macos::input_monitoring_granted() && !macos::request_input_monitoring() {
        eprintln!("Input Monitoring is off for {}; allow it in System Settings", service::bin_path());
    }
    let mut last: Option<service::Status> = None;
    loop {
        let result = device::pump(&mut api, false, || {
            service::write_status(service::Status::Connected);
            last = Some(service::Status::Connected);
        }, |event| {
            if !event.pressed || config::paused() {
                return;
            }
            let control = event.control;
            if let Some(e) = store.reload_if_changed() {
                eprintln!("config error: {e}");
                actions::notify(&format!("Config error: {e}"));
            }
            let app = if store.config.apps.is_empty() {
                None
            } else {
                if front.1.is_none() || front.0.elapsed() > Duration::from_millis(500) {
                    front = (Instant::now(), actions::frontmost_app());
                }
                front.1.as_deref()
            };
            if let Some(binding) = store.lookup(&control, app) {
                let bin = store.config.claude_bin.clone().unwrap_or_else(|| "claude".into());
                actions::dispatch(control, binding.actions(), bin);
            }
        });
        // Log and record only changes, so a lasting problem is one log line.
        let (status, errs) = match result {
            Ok(()) => (service::Status::NoPad, Vec::new()),
            Err(errs) if errs.is_empty() => (service::Status::NoPad, errs),
            Err(errs) => (service::Status::Blocked, errs),
        };
        if last != Some(status) {
            match status {
                service::Status::Blocked => {
                    for e in &errs {
                        eprintln!("cannot open pad: {e}");
                    }
                    eprintln!("allow {} under Input Monitoring in System Settings", service::bin_path());
                }
                _ => eprintln!("waiting for the pad…"),
            }
            service::write_status(status);
            last = Some(status);
        }
        sleep(Duration::from_secs(2));
    }
}

fn monitor(raw: bool) -> Result<(), String> {
    let mut api = HidApi::new().map_err(|e| e.to_string())?;
    println!("press keys and turn the knob (ctrl-c to stop)");
    let result = device::pump(&mut api, raw, || {}, |event: Event| {
        if event.pressed {
            println!("{}", event.control);
        }
    });
    match result {
        Ok(()) => Ok(()),
        Err(errs) if errs.is_empty() => Err("pad not found".into()),
        Err(errs) => Err(format!("pad not accessible (allow this program under Input Monitoring): {}", errs.join("; "))),
    }
}
