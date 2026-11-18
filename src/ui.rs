//! MiniMacPad settings app: a picture of the pad you click to choose what each
//! control does. It edits the same config file the daemon reads.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2};
use hidapi::HidApi;
use minipad::config::{self, Action, Binding, Config};
use minipad::device::{self, Event};
use minipad::update::{self, Release};
use minipad::{actions, flash, macos, service};
use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Bindings = BTreeMap<String, Vec<Action>>;

const BG: Color32 = Color32::from_rgb(30, 30, 32);
const SIDEBAR: Color32 = Color32::from_rgb(38, 38, 40);
const CARD: Color32 = Color32::from_rgb(44, 44, 46);
const HAIRLINE: Color32 = Color32::from_rgb(58, 58, 60);
const INK: Color32 = Color32::from_rgb(235, 235, 240);
const DIM: Color32 = Color32::from_rgb(152, 152, 158);
const ACCENT: Color32 = Color32::from_rgb(10, 132, 255);
const GOOD: Color32 = Color32::from_rgb(48, 209, 88);
const WARN: Color32 = Color32::from_rgb(255, 159, 10);
const LINE: f32 = 2.5;

/// A quiet dark look in the manner of macOS: the system font, soft rounded
/// controls, filled cards with hairline edges, one blue accent, no shadows.
fn apply_theme(ctx: &egui::Context) {
    use egui::{CornerRadius, FontFamily, Shadow, TextStyle};
    ctx.set_theme(egui::Theme::Dark);

    // Use the system fonts when they can be read; egui's own stay as fallback.
    let mut fonts = egui::FontDefinitions::default();
    for (name, path, family) in [
        ("system", "/System/Library/Fonts/SFNS.ttf", FontFamily::Proportional),
        ("system-mono", "/System/Library/Fonts/SFNSMono.ttf", FontFamily::Monospace),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
            fonts.families.entry(family).or_default().insert(0, name.into());
        }
    }
    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::new(24.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(13.0, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(11.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace)),
        ]
        .into();
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.menu_margin = egui::Margin::same(6);

        let v = &mut style.visuals;
        v.panel_fill = BG;
        v.window_fill = CARD;
        v.extreme_bg_color = Color32::from_rgb(22, 22, 24);
        v.faint_bg_color = CARD;
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.window_stroke = Stroke::new(1.0_f32, HAIRLINE);
        v.window_shadow = Shadow::NONE;
        v.popup_shadow = Shadow::NONE;
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
        v.hyperlink_color = ACCENT;

        let fills = [
            (&mut v.widgets.noninteractive, CARD, HAIRLINE, 10),
            (&mut v.widgets.inactive, Color32::from_rgb(58, 58, 60), Color32::from_rgb(70, 70, 72), 6),
            (&mut v.widgets.hovered, Color32::from_rgb(72, 72, 74), Color32::from_rgb(88, 88, 90), 6),
            (&mut v.widgets.active, Color32::from_rgb(84, 84, 86), Color32::from_rgb(100, 100, 102), 6),
            (&mut v.widgets.open, Color32::from_rgb(72, 72, 74), Color32::from_rgb(88, 88, 90), 6),
        ];
        for (w, fill, edge, radius) in fills {
            w.corner_radius = CornerRadius::same(radius);
            w.expansion = 0.0;
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.bg_stroke = Stroke::new(1.0_f32, edge);
            w.fg_stroke = Stroke::new(1.5_f32, INK);
        }
        v.widgets.active.fg_stroke.color = Color32::WHITE;
    });
}

/// Open System Settings at Privacy & Security > Input Monitoring.
fn open_input_monitoring() {
    let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
    let _ = std::process::Command::new("open").arg(url).spawn();
}

/// Small grey explanatory text.
fn note(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text).small().color(DIM));
}

/// Label above a field.
fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().color(DIM));
}

/// A button filled with the accent colour, for the main action.
fn primary(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(ACCENT)
}

/// Write a shortcut the way macOS menus do: "cmd+shift+4" becomes "⌘⇧4".
fn pretty_keys(combo: &str) -> String {
    combo
        .split('+')
        .map(|part| match part.trim().to_lowercase().as_str() {
            "cmd" | "command" => "⌘".to_string(),
            "shift" => "⇧".to_string(),
            "alt" | "opt" | "option" => "⌥".to_string(),
            "ctrl" | "control" => "⌃".to_string(),
            "return" | "enter" => "↩".to_string(),
            "space" => "Space".to_string(),
            key if key.chars().count() == 1 => key.to_uppercase(),
            key => {
                let mut c = key.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            }
        })
        .collect()
}

const PAD_ABSENT: u8 = 0;
/// Plugged in, but macOS will not let this app read its key presses.
const PAD_BLOCKED: u8 = 1;
const PAD_CONNECTED: u8 = 2;

const KEYS: [&str; 6] = ["key1", "key2", "key3", "key4", "key5", "key6"];

fn title(control: &str) -> String {
    match control {
        "knob_left" => "Knob: Turn Left".into(),
        "knob_right" => "Knob: Turn Right".into(),
        "knob_press" => "Knob: Press".into(),
        key => format!("Key {}", key.trim_start_matches("key")),
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Kind {
    Open,
    Url,
    Shell,
    Keys,
    Volume,
    Mute,
    Claude,
    Notify,
}

const KINDS: [(Kind, &str); 8] = [
    (Kind::Open, "Open an App"),
    (Kind::Url, "Open a Link"),
    (Kind::Keys, "Press a Shortcut"),
    (Kind::Volume, "Change Volume"),
    (Kind::Mute, "Mute or Unmute"),
    (Kind::Claude, "Ask an AI"),
    (Kind::Shell, "Run a Shell Command"),
    (Kind::Notify, "Show a Notification"),
];

fn kind_of(a: &Action) -> Kind {
    if a.open.is_some() {
        Kind::Open
    } else if a.url.is_some() {
        Kind::Url
    } else if a.shell.is_some() {
        Kind::Shell
    } else if a.keys.is_some() {
        Kind::Keys
    } else if a.volume.is_some() {
        Kind::Volume
    } else if a.ask.is_some() {
        Kind::Claude
    } else if a.notify.is_some() {
        Kind::Notify
    } else {
        Kind::Mute
    }
}

fn new_action(kind: Kind) -> Action {
    let mut a = Action::default();
    match kind {
        Kind::Open => a.open = Some(String::new()),
        Kind::Url => a.url = Some("https://".into()),
        Kind::Shell => a.shell = Some(String::new()),
        Kind::Keys => a.keys = Some(String::new()),
        Kind::Volume => a.volume = Some(4.0),
        Kind::Mute => a.mute = Some(true),
        Kind::Claude => a.ask = Some(String::new()),
        Kind::Notify => a.notify = Some(String::new()),
    }
    a
}

/// Short text shown on a key in the pad picture.
fn summary(actions: &[Action]) -> String {
    let Some(a) = actions.first() else { return "Empty".into() };
    let text = match kind_of(a) {
        Kind::Open => a.open.clone().unwrap_or_default(),
        Kind::Url => a
            .url
            .as_deref()
            .unwrap_or_default()
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or_default()
            .to_string(),
        Kind::Shell => "Shell".into(),
        Kind::Keys => pretty_keys(a.keys.as_deref().unwrap_or_default()),
        Kind::Volume => format!("Vol {:+}", a.volume.unwrap_or(0.0)),
        Kind::Mute => "Mute".into(),
        Kind::Claude => match a.with.as_deref().unwrap_or("claude") {
            "claude" | "claude-app" => "Claude".into(),
            "antigravity" | "antigravity-app" => "Antigravity".into(),
            "chatgpt" => "ChatGPT".into(),
            "gemini" => "Gemini".into(),
            other => other.to_string(),
        },
        Kind::Notify => "Notify".into(),
    };
    // Up to 11 characters fit on a keycap.
    let mut short = text.clone();
    if text.chars().count() > 11 {
        short = text.chars().take(9).collect();
        short.push_str("..");
    }
    if actions.len() > 1 {
        short.push_str(&format!(" +{}", actions.len() - 1));
    }
    short
}

/// The editable form of the config file.
#[derive(Clone, PartialEq, Default)]
struct Doc {
    claude_bin: String,
    global: Bindings,
    apps: BTreeMap<String, Bindings>,
}

fn to_lists(map: &BTreeMap<String, Binding>) -> Bindings {
    map.iter().map(|(k, b)| (k.clone(), b.actions())).collect()
}

fn to_bindings(map: &Bindings) -> BTreeMap<String, Binding> {
    map.iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| {
            let binding = if v.len() == 1 { Binding::One(v[0].clone()) } else { Binding::Many(v.clone()) };
            (k.clone(), binding)
        })
        .collect()
}

impl Doc {
    fn load() -> Result<Doc, String> {
        let c = config::parse()?;
        Ok(Doc {
            claude_bin: c.claude_bin.unwrap_or_default(),
            global: to_lists(&c.keys),
            apps: c.apps.iter().map(|(app, map)| (app.clone(), to_lists(map))).collect(),
        })
    }

    fn save(&self) -> Result<(), String> {
        let bin = self.claude_bin.trim();
        config::save(&Config {
            claude_bin: (!bin.is_empty()).then(|| bin.to_string()),
            keys: to_bindings(&self.global),
            apps: self.apps.iter().map(|(app, map)| (app.clone(), to_bindings(map))).collect(),
        })
    }
}

/// Look for a newer release in the background.
fn check_for_update(state: Arc<Mutex<Update>>, ctx: egui::Context) {
    *state.lock().unwrap() = Update::Checking;
    std::thread::spawn(move || {
        let found = match update::check() {
            Ok(Some(release)) => Update::Available(release),
            Ok(None) => Update::UpToDate,
            Err(e) => Update::Failed(e),
        };
        *state.lock().unwrap() = found;
        ctx.request_repaint();
    });
}

/// Download and install a release in the background; the app restarts itself on success.
fn install_update(state: Arc<Mutex<Update>>, release: Release, ctx: egui::Context) {
    *state.lock().unwrap() = Update::Installing(release.version.clone());
    std::thread::spawn(move || {
        if let Err(e) = update::install(&release) {
            *state.lock().unwrap() = Update::Failed(e);
            ctx.request_repaint();
        }
    });
}

/// Names of installed apps, for the pickers.
fn installed_apps() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let dirs = [
        "/Applications".to_string(),
        "/System/Applications".to_string(),
        "/System/Applications/Utilities".to_string(),
        format!("{home}/Applications"),
    ];
    let mut apps: Vec<String> = dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".app").map(str::to_string))
        .collect();
    apps.sort_by_key(|a| a.to_lowercase());
    apps.dedup();
    apps
}

/// A scrolling list of apps; returns the one clicked.
fn app_list(ui: &mut egui::Ui, apps: &[String]) -> Option<String> {
    let mut picked = None;
    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
        for app in apps {
            if ui.button(app).clicked() {
                picked = Some(app.clone());
                ui.close_menu();
            }
        }
    });
    picked
}

fn choice(ui: &mut egui::Ui, id: (&str, usize), value: &mut Option<String>, options: &[(&str, &str)]) {
    let current = value.clone().unwrap_or_else(|| options[0].0.to_string());
    let label = options.iter().find(|(v, _)| *v == current).map(|(_, l)| *l).unwrap_or(&current);
    let width = (ui.available_width() - 8.0).clamp(120.0, 270.0);
    egui::ComboBox::from_id_salt(id).selected_text(label).width(width).show_ui(ui, |ui| {
        for (v, l) in options {
            if ui.selectable_label(current == *v, *l).clicked() {
                *value = (*v != options[0].0).then(|| v.to_string());
            }
        }
    });
}

/// Editor for one action. Returns true if its remove button was clicked.
fn edit_action(ui: &mut egui::Ui, index: usize, a: &mut Action, apps: &[String]) -> bool {
    let mut remove = false;
    egui::Frame::group(ui.style()).fill(CARD).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}", index + 1)).color(DIM));
            let kind = kind_of(a);
            let label = KINDS.iter().find(|(k, _)| *k == kind).map(|(_, l)| *l).unwrap_or_default();
            let width = (ui.available_width() - 130.0).clamp(110.0, 230.0);
            egui::ComboBox::from_id_salt(("kind", index)).selected_text(label).width(width).show_ui(ui, |ui| {
                for (k, l) in KINDS {
                    if ui.selectable_label(k == kind, l).clicked() && k != kind {
                        *a = new_action(k);
                    }
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                remove = ui.button("Remove").clicked();
            });
        });
        ui.add_space(4.0);
        let wide = ui.available_width();
        match kind_of(a) {
            Kind::Open => {
                ui.horizontal(|ui| {
                    let name = a.open.get_or_insert_default();
                    ui.add(egui::TextEdit::singleline(name).margin(egui::Margin::symmetric(8, 6)).hint_text("App name").desired_width((wide - 120.0).max(80.0)));
                    ui.menu_button("Choose…", |ui| {
                        if let Some(app) = app_list(ui, apps) {
                            *name = app;
                        }
                    });
                });
            }
            Kind::Url => {
                ui.add(egui::TextEdit::singleline(a.url.get_or_insert_default()).margin(egui::Margin::symmetric(8, 6)).hint_text("https://…").desired_width(wide));
            }
            Kind::Keys => {
                ui.add(
                    egui::TextEdit::singleline(a.keys.get_or_insert_default()).margin(egui::Margin::symmetric(8, 6))
                        .hint_text("for example cmd+shift+4")
                        .desired_width(wide),
                );
                note(ui, "Modifiers: cmd, shift, ctrl, alt. Keys: letters, digits, f1 to f12, space, return, tab, escape, delete, arrows.");
            }
            Kind::Volume => {
                ui.add(egui::Slider::new(a.volume.get_or_insert(4.0), -25.0..=25.0).step_by(1.0).suffix("%"));
                note(ui, "Per press or knob click. Negative turns it down.");
            }
            Kind::Mute => {
                a.mute = Some(true);
                note(ui, "Toggles the sound on and off.");
            }
            Kind::Shell => {
                ui.add(
                    egui::TextEdit::multiline(a.shell.get_or_insert_default()).margin(egui::Margin::symmetric(8, 6))
                        .code_editor()
                        .desired_rows(3)
                        .desired_width(wide),
                );
            }
            Kind::Notify => {
                ui.add(egui::TextEdit::singleline(a.notify.get_or_insert_default()).margin(egui::Margin::symmetric(8, 6)).hint_text("Text to show").desired_width(wide));
            }
            Kind::Claude => {
                ui.add(
                    egui::TextEdit::multiline(a.ask.get_or_insert_default()).margin(egui::Margin::symmetric(8, 6))
                        .hint_text("What to ask")
                        .desired_rows(3)
                        .desired_width(wide),
                );
                let with = a.with.clone().unwrap_or_else(|| "claude".into());
                let app = actions::ASSISTANT_APPS.iter().find(|(name, _, _)| *name == with);
                {
                    caption(ui, "Assistant");
                    choice(
                        ui,
                        ("with", index),
                        &mut a.with,
                        &[
                            ("claude", "Claude, in the background"),
                            ("claude-app", "Claude desktop app"),
                            ("chatgpt", "ChatGPT desktop app"),
                            ("gemini", "Gemini desktop app"),
                            ("antigravity", "Antigravity, in the background"),
                            ("antigravity-app", "Antigravity desktop app"),
                        ],
                    );
                    caption(ui, "Send along");
                    choice(
                        ui,
                        ("input", index),
                        &mut a.input,
                        &[("none", "Nothing"), ("clipboard", "The clipboard"), ("selection", "The selected text")],
                    );
                    if let Some((_, _, usual)) = app {
                        caption(ui, "New chat shortcut");
                        let mut keys = a.shortcut.clone().unwrap_or_else(|| usual.to_string());
                        let width = (ui.available_width() - 20.0).clamp(100.0, 270.0);
                        if ui.add(egui::TextEdit::singleline(&mut keys).margin(egui::Margin::symmetric(8, 6)).desired_width(width)).changed() {
                            a.shortcut = (keys != *usual).then_some(keys);
                        }
                    } else {
                        caption(ui, "Answer goes to");
                        choice(
                            ui,
                            ("output", index),
                            &mut a.output,
                            &[("clipboard", "The clipboard"), ("paste", "Pasted into the app"), ("terminal", "A session in Terminal")],
                        );
                    }
                }
                if let Some((_, name, _)) = app {
                    note(ui, format!("Brings {name} forward, presses the new chat keys, pastes the prompt and presses return. Clear the keys to paste into the open chat."));
                }
            }
        }
    });
    remove
}

/// Where the self-updater stands.
#[derive(Clone, PartialEq)]
enum Update {
    Checking,
    UpToDate,
    Available(Release),
    Installing(String),
    Failed(String),
}

struct App {
    update: Arc<Mutex<Update>>,
    doc: Doc,
    saved: Doc,
    /// None is the global profile, Some(app) a per-app one.
    profile: Option<String>,
    selected: String,
    apps: Vec<String>,
    events: Receiver<Event>,
    /// Last control pressed, kept lit briefly so a knob step is visible.
    pressed: Option<(String, Instant)>,
    /// Controls physically held down right now.
    held: HashSet<String>,
    /// Run actions from this window on pad presses, including unsaved edits.
    testing: bool,
    /// When the daemon pause file was last refreshed; None if it is absent.
    pause_touched: Option<Instant>,
    /// Size of the pad picture relative to its natural 240 by 432 points.
    scale: f32,
    /// One of the PAD_* states below.
    pad: Arc<AtomicU8>,
    daemon_running: Arc<AtomicBool>,
    /// What the background service last reported; None if it has not said.
    service_status: Arc<Mutex<Option<service::Status>>>,
    message: String,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> App {
        apply_theme(&cc.egui_ctx);
        macos::request_input_monitoring();
        // Dark title bar to match.
        cc.egui_ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
        let mut message = String::new();
        if !config::path().exists() {
            let _ = config::Store::load(); // writes the starter config
        }
        let doc = Doc::load().unwrap_or_else(|e| {
            message = format!("Could not read the config file: {e}");
            Doc::default()
        });

        let (tx, events) = channel();
        let pad = Arc::new(AtomicU8::new(PAD_ABSENT));
        let daemon_running = Arc::new(AtomicBool::new(service::running()));

        // Listen to the pad so pressing a control selects it here.
        let (ctx, flag) = (cc.egui_ctx.clone(), pad.clone());
        std::thread::spawn(move || {
            let Ok(mut api) = HidApi::new() else { return };
            loop {
                let _ = device::pump(&mut api, false, || flag.store(PAD_CONNECTED, Ordering::Relaxed), |event| {
                    let _ = tx.send(event);
                    ctx.request_repaint();
                });
                let _ = api.refresh_devices();
                let present = !device::keyboard_interfaces(&api).is_empty();
                flag.store(if present { PAD_BLOCKED } else { PAD_ABSENT }, Ordering::Relaxed);
                ctx.request_repaint();
                std::thread::sleep(Duration::from_secs(2));
            }
        });

        let service_status = Arc::new(Mutex::new(service::read_status()));
        let (flag, status) = (daemon_running.clone(), service_status.clone());
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_secs(2));
            flag.store(service::running(), Ordering::Relaxed);
            *status.lock().unwrap() = service::read_status();
        });

        // Keep a login-item daemon in step with this app, then look for a newer release.
        if let Some(daemon) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("minipad"))) {
            service::sync(&daemon);
        }
        let update = Arc::new(Mutex::new(Update::Checking));
        check_for_update(update.clone(), cc.egui_ctx.clone());

        App {
            update,
            saved: doc.clone(),
            doc,
            profile: None,
            selected: "key1".into(),
            apps: installed_apps(),
            events,
            pressed: None,
            held: HashSet::new(),
            testing: false,
            pause_touched: None,
            scale: 1.0,
            pad,
            daemon_running,
            service_status,
            message,
        }
    }

    fn claude_bin(&self) -> String {
        let bin = self.doc.claude_bin.trim();
        if bin.is_empty() { "claude".into() } else { bin.into() }
    }

    /// Keep the daemon from acting on presses while this window is active.
    fn pause_daemon(&mut self, focused: bool) {
        if focused {
            if self.pause_touched.is_none_or(|t| t.elapsed() > Duration::from_secs(1)) {
                let _ = std::fs::write(config::pause_path(), "");
                self.pause_touched = Some(Instant::now());
            }
        } else if self.pause_touched.take().is_some() {
            let _ = std::fs::remove_file(config::pause_path());
        }
    }

    /// True if the background service is running but macOS will not let it
    /// read the pad, so presses do nothing outside this window.
    fn service_blocked(&self) -> bool {
        self.daemon_running.load(Ordering::Relaxed)
            && *self.service_status.lock().unwrap() == Some(service::Status::Blocked)
    }

    /// Explain the blocked service and offer the way out: the Input Monitoring
    /// list, the program to put in it, and a restart once it is allowed.
    fn service_blocked_notice(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("macOS is hiding key presses from the background service").color(WARN));
        ui.horizontal_wrapped(|ui| {
            if ui.button("Allow in Settings…").clicked() {
                open_input_monitoring();
                let _ = std::process::Command::new("open").args(["-R", &service::bin_path()]).spawn();
            }
            if service::installed() && ui.button("Restart Service").clicked() {
                let result = service::restart();
                self.report("Background service restarted", result);
            }
        });
        note(ui, "Under Input Monitoring, turn on minipad. If it is not listed, click +, \
            press Cmd+Shift+G and enter ~/.local/bin/minipad (it is also shown in the Finder window). \
            Then click Restart Service.");
    }

    /// Test switch and the hints shown under the pad picture.
    fn pad_footer(&mut self, ui: &mut egui::Ui) {
        if self.service_blocked() {
            self.service_blocked_notice(ui);
            ui.add_space(6.0);
        }
        if self.pad.load(Ordering::Relaxed) == PAD_BLOCKED {
            ui.label(RichText::new("macOS is hiding key presses from MiniMacPad").color(WARN));
            if ui.button("Open Privacy Settings…").clicked() {
                open_input_monitoring();
            }
            note(ui, "Turn MiniMacPad on under Input Monitoring, then reopen the app.");
            return;
        }
        let button = if self.testing {
            primary("Testing: Pad Is Live")
        } else {
            egui::Button::new("Test Changes")
        };
        if ui.add(button).clicked() {
            self.testing = !self.testing;
        }
        note(ui, if self.testing {
            "Pressing the pad runs what is shown here, unsaved edits included. Click again to stop."
        } else {
            "The pad only selects controls while this window is active. Switch to another window to use it normally."
        });
    }

    fn bindings(&self) -> &Bindings {
        self.profile.as_ref().and_then(|p| self.doc.apps.get(p)).unwrap_or(&self.doc.global)
    }

    /// What a control does in the current profile, falling back to global.
    fn effective(&self, control: &str) -> &[Action] {
        self.bindings()
            .get(control)
            .or_else(|| self.doc.global.get(control))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn report(&mut self, what: &str, result: Result<(), String>) {
        self.message = match result {
            Ok(()) => what.to_string(),
            Err(e) => format!("{what} failed: {e}"),
        };
    }

    /// One clickable control in the pad picture, drawn to match the real pad:
    /// white keycaps, a knurled metal knob, and a turn arrow on each side of it.
    fn control(&mut self, ui: &mut egui::Ui, rect: Rect, name: &str) {
        let response = ui.interact(rect, ui.id().with(name), Sense::click()).on_hover_text(title(name));
        if response.clicked() {
            self.selected = name.to_string();
        }
        let lit = self.held.contains(name)
            || self.pressed.as_ref().is_some_and(|(n, t)| n == name && t.elapsed() < Duration::from_millis(250));
        let selected = self.selected == name;
        let hovered = response.hovered();
        let grey = Color32::from_gray;
        let painter = ui.painter();
        let center = rect.center();
        let k = self.scale;

        match name {
            "knob_press" => {
                let r = rect.width() / 2.0;
                painter.circle_filled(center, r, grey(120));
                for i in 0..48 {
                    let angle = i as f32 * std::f32::consts::TAU / 48.0;
                    let dir = Vec2::new(angle.cos(), angle.sin());
                    painter.line_segment([center + dir * (r - 6.0 * k), center + dir * r], Stroke::new(1.5 * k, grey(70)));
                }
                let face = r - 7.0 * k;
                painter.circle_filled(center, face, if lit { ACCENT } else { grey(196) });
                for ring in 1..7 {
                    painter.circle_stroke(center, face * ring as f32 / 7.0, Stroke::new(1.0_f32, grey(168)));
                }
                painter.circle_filled(center, 2.5 * k, grey(235));
                // The white position mark on the rim.
                let dir = Vec2::new(0.35, 0.94);
                painter.line_segment([center + dir * (r - 6.0 * k), center + dir * r], Stroke::new(2.5 * k, Color32::WHITE));
                if selected || hovered {
                    painter.circle_stroke(center, r + 5.0 * k, Stroke::new(LINE, if selected { ACCENT } else { INK }));
                }
            }
            "knob_left" | "knob_right" => {
                let ink = if lit { Color32::WHITE } else if selected { ACCENT } else { INK };
                let glyph = if name == "knob_left" { "⟲" } else { "⟳" };
                painter.text(center, Align2::CENTER_CENTER, glyph, FontId::proportional(24.0 * k), ink);
                if selected || hovered {
                    painter.circle_stroke(center, rect.width() / 2.0, Stroke::new(LINE, if selected { ACCENT } else { INK }));
                }
            }
            _ => {
                // Keycap: darker skirt with the lighter top face set into it.
                let (top, skirt) = if lit {
                    (Color32::from_rgb(176, 210, 255), ACCENT)
                } else if hovered {
                    (Color32::WHITE, grey(214))
                } else {
                    (grey(238), grey(198))
                };
                painter.rect_filled(rect, 9.0 * k, skirt);
                let face = Rect::from_min_max(rect.min + Vec2::new(7.0, 5.0) * k, rect.max - Vec2::new(7.0, 12.0) * k);
                painter.rect_filled(face, 7.0 * k, top);
                if selected {
                    painter.rect_stroke(rect.expand(3.0 * k), 11.0 * k, Stroke::new(LINE, ACCENT), StrokeKind::Outside);
                }
                let number = name.trim_start_matches("key");
                painter.text(face.left_top() + Vec2::new(7.0, 6.0) * k, Align2::LEFT_TOP, number, FontId::proportional(10.0 * k), grey(130));
                painter.text(face.center() + Vec2::new(0.0, 4.0) * k, Align2::CENTER_CENTER, summary(self.effective(name)), FontId::proportional(12.0 * k), Color32::from_gray(20));
            }
        }
        // A dot marks a control this app profile overrides.
        if self.profile.is_some() && self.bindings().contains_key(name) {
            painter.circle_filled(rect.right_top() + Vec2::new(-14.0, 14.0) * k, 3.5 * k, ACCENT);
        }
    }

    /// Draw the pad as large as the panel allows, centred.
    fn pad_picture(&mut self, ui: &mut egui::Ui, room: Vec2) {
        let grey = Color32::from_gray;
        let k = (room.x / 240.0).min(room.y / 432.0).clamp(0.5, 1.5);
        self.scale = k;
        let (area, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 432.0 * k), Sense::hover());
        let body = Rect::from_center_size(area.center(), Vec2::new(240.0, 432.0) * k);
        let painter = ui.painter();

        // Smoked acrylic body with a screw in each corner.
        painter.rect(body, 16.0 * k, Color32::from_rgb(38, 33, 34), Stroke::new(1.5_f32, grey(84)), StrokeKind::Inside);
        for corner in [body.left_top(), body.right_top(), body.left_bottom(), body.right_bottom()] {
            let inward = body.center() - corner;
            let c = corner + Vec2::new(inward.x.signum(), inward.y.signum()) * 14.0 * k;
            painter.circle_filled(c, 5.0 * k, grey(150));
            painter.line_segment([c - Vec2::new(3.0 * k, 0.0), c + Vec2::new(3.0 * k, 0.0)], Stroke::new(1.0_f32, grey(60)));
            painter.line_segment([c - Vec2::new(0.0, 3.0 * k), c + Vec2::new(0.0, 3.0 * k)], Stroke::new(1.0_f32, grey(60)));
        }

        // The red backlight showing between the two columns of keys.
        for (width, alpha) in [(22.0, 12), (12.0, 28), (5.0, 70), (2.0, 200)] {
            let glow = Rect::from_center_size(Pos2::new(body.center().x, body.top() + 258.0 * k), Vec2::new(width, 296.0) * k);
            painter.rect_filled(glow, width * k / 2.0, Color32::from_rgba_unmultiplied(196, 66, 50, alpha));
        }

        let at = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(body.min + Vec2::new(x, y) * k, Vec2::new(w, h) * k);
        self.control(ui, at(81.0, 14.0, 78.0, 78.0), "knob_press");
        self.control(ui, at(26.0, 33.0, 40.0, 40.0), "knob_left");
        self.control(ui, at(174.0, 33.0, 40.0, 40.0), "knob_right");
        for (i, key) in KEYS.iter().enumerate() {
            let (col, row) = ((i % 2) as f32, (i / 2) as f32);
            self.control(ui, at(22.0 + col * 104.0, 110.0 + row * 104.0, 92.0, 92.0), key);
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            caption(ui, "Profile");
            let current = self.profile.clone().unwrap_or_else(|| "All Apps".into());
            egui::ComboBox::from_id_salt("profile").selected_text(current).width(180.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.profile, None, "All Apps");
                for app in self.doc.apps.keys() {
                    ui.selectable_value(&mut self.profile, Some(app.clone()), app);
                }
            });
            ui.menu_button("Add App Profile…", |ui| {
                if let Some(app) = app_list(ui, &self.apps) {
                    self.doc.apps.entry(app.clone()).or_default();
                    self.profile = Some(app);
                }
            });
            if let Some(app) = self.profile.clone() {
                if ui.button("Delete Profile").clicked() {
                    self.doc.apps.remove(&app);
                    self.profile = None;
                }
            }

            // Push the save buttons to the right edge when the row has room;
            // otherwise they wrap onto the next line with everything else.
            let dirty = self.doc != self.saved;
            let spare = ui.available_size_before_wrap().x - if dirty { 190.0 } else { 140.0 };
            if spare > 0.0 {
                ui.add_space(spare);
            }
            if dirty {
                ui.label(RichText::new("Edited").small().color(DIM));
            }
            if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
                self.doc = self.saved.clone();
                if self.profile.as_ref().is_some_and(|p| !self.doc.apps.contains_key(p)) {
                    self.profile = None;
                }
            }
            let save = if dirty { primary("Save") } else { egui::Button::new("Save") };
            if ui.add_enabled(dirty, save).clicked() {
                let result = self.doc.save();
                if result.is_ok() {
                    self.saved = self.doc.clone();
                }
                self.report("Saved", result);
            }
        });
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        let control = self.selected.clone();
        ui.heading(RichText::new(title(&control)).color(INK));
        match &self.profile {
            None => note(ui, "What this does in every app, unless an app profile overrides it."),
            Some(app) => note(ui, format!("What this does while {app} is in front.")),
        };
        ui.add_space(6.0);

        if let Some(app) = self.profile.clone() {
            if !self.doc.apps.get(&app).is_some_and(|m| m.contains_key(&control)) {
                ui.label(format!("Same as in all apps: {}", summary(self.effective(&control))));
                if ui.button(format!("Change It for {app}")).clicked() {
                    let start = self.doc.global.get(&control).cloned().unwrap_or_default();
                    self.doc.apps.entry(app).or_default().insert(control, start);
                }
                return;
            }
        }

        let apps = std::mem::take(&mut self.apps);
        let list = match &self.profile {
            Some(app) => self.doc.apps.entry(app.clone()).or_default().entry(control.clone()).or_default(),
            None => self.doc.global.entry(control.clone()).or_default(),
        };
        let mut remove = None;
        for (i, action) in list.iter_mut().enumerate() {
            if edit_action(ui, i, action, &apps) {
                remove = Some(i);
            }
        }
        if let Some(i) = remove {
            list.remove(i);
        }
        if list.is_empty() {
            note(ui, "Does nothing yet.");
        }
        let mut test = None;
        let mut reset = false;
        ui.horizontal_wrapped(|ui| {
            ui.menu_button("Add Action…", |ui| {
                for (k, l) in KINDS {
                    if ui.button(l).clicked() {
                        list.push(new_action(k));
                        ui.close_menu();
                    }
                }
            });
            if !list.is_empty() && ui.button("Try It Now").clicked() {
                test = Some(list.clone());
            }
            if self.profile.is_some() && ui.button("Use the All Apps Action").clicked() {
                reset = true;
            }
        });
        if list.len() > 1 {
            note(ui, "Actions run in order, top to bottom.");
        }
        self.apps = apps;
        if let Some(actions) = test {
            actions::dispatch(control.clone(), actions, self.claude_bin());
        }
        if reset {
            if let Some(map) = self.profile.as_ref().and_then(|p| self.doc.apps.get_mut(p)) {
                map.remove(&control);
            }
        }
    }

    fn device_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(RichText::new("Pad and Background Service").color(INK)).show(ui, |ui| {
            let pad = self.pad.load(Ordering::Relaxed);
            let connected = pad != PAD_ABSENT;
            let running = self.daemon_running.load(Ordering::Relaxed);
            {
                caption(ui, "Pad");
                ui.horizontal_wrapped(|ui| {
                    ui.label(match pad {
                        PAD_CONNECTED => "Connected",
                        PAD_BLOCKED => "Plugged in, but this app may not read its presses",
                        _ => "Not plugged in",
                    });
                    if pad == PAD_BLOCKED && ui.button("Open Privacy Settings…").clicked() {
                        open_input_monitoring();
                    }
                });

                caption(ui, "Background service");
                ui.horizontal_wrapped(|ui| {
                    let login = service::installed();
                    ui.label(match (running, login) {
                        (true, true) => "Running, starts at login",
                        (true, false) => "Running until you log out",
                        (false, true) => "Set to start at login, but not running",
                        (false, false) => "Not running: the pad does nothing",
                    });
                    if !login && ui.button("Start at Login").clicked() {
                        let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("minipad")));
                        let result = match exe {
                            Some(exe) if exe.exists() => service::install(&exe),
                            _ => Err("the minipad program is not next to this app".into()),
                        };
                        self.report("Background service started", result);
                    }
                    if login && running && ui.button("Restart").clicked() {
                        let result = service::restart();
                        self.report("Background service restarted", result);
                    }
                    if (login || running) && ui.button("Stop").clicked() {
                        let result = service::uninstall();
                        self.report("Background service stopped", result);
                    }
                    if ui.button("Show Log").clicked() {
                        let _ = std::process::Command::new("open").arg(service::log_path()).spawn();
                    }
                });
                if self.service_blocked() {
                    self.service_blocked_notice(ui);
                }

                caption(ui, "Backlight");
                ui.horizontal_wrapped(|ui| {
                    for mode in 0..=2u8 {
                        if ui.add_enabled(connected, egui::Button::new(format!("Mode {mode}"))).clicked() {
                            self.report(&format!("Backlight mode {mode} sent"), flash::set_led(mode));
                        }
                    }
                });

                caption(ui, "Pad memory");
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(connected, egui::Button::new("Program Pad Again")).clicked() {
                        self.report("Pad programmed", flash::run(&[]));
                    }
                    note(ui, "Only needed for a new or reset pad.");
                });

                caption(ui, "Version");
                ui.horizontal_wrapped(|ui| {
                    let state = self.update.lock().unwrap().clone();
                    ui.label(update::VERSION);
                    match &state {
                        Update::Checking => note(ui, "Checking for updates…"),
                        Update::UpToDate => note(ui, "Up to date."),
                        Update::Available(release) => note(ui, format!("Version {} is available.", release.version)),
                        Update::Installing(version) => note(ui, format!("Installing {version}…")),
                        Update::Failed(e) => note(ui, format!("Update check failed: {e}")),
                    }
                    let busy = matches!(state, Update::Checking | Update::Installing(_));
                    if let Update::Available(release) = &state {
                        if ui.add(primary("Update and Relaunch")).clicked() {
                            install_update(self.update.clone(), release.clone(), ui.ctx().clone());
                        }
                    } else if ui.add_enabled(!busy, egui::Button::new("Check for Updates")).clicked() {
                        check_for_update(self.update.clone(), ui.ctx().clone());
                    }
                });

                caption(ui, "Claude program");
                ui.add(egui::TextEdit::singleline(&mut self.doc.claude_bin).margin(egui::Margin::symmetric(8, 6)).hint_text("claude (found automatically)").desired_width((ui.available_width() - 20.0).clamp(100.0, 260.0)));
            }
        });
    }
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(config::pause_path());
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let focused = ctx.input(|i| i.focused);
        while let Ok(Event { control, pressed }) = self.events.try_recv() {
            if !pressed {
                self.held.remove(&control);
                continue;
            }
            if !device::CONTROLS.iter().any(|(n, _)| *n == control) {
                continue;
            }
            // The daemon is paused while this window is active, so in test
            // mode the press runs here, with whatever is on screen.
            if self.testing && focused {
                let actions = self.effective(&control).to_vec();
                if !actions.is_empty() {
                    actions::dispatch(control.clone(), actions, self.claude_bin());
                }
            }
            self.selected = control.clone();
            self.held.insert(control.clone());
            self.pressed = Some((control, Instant::now()));
        }
        self.pause_daemon(focused);
        if self.pressed.as_ref().is_some_and(|(_, t)| t.elapsed() < Duration::from_millis(300)) {
            ctx.request_repaint_after(Duration::from_millis(60));
        }
        ctx.request_repaint_after(Duration::from_secs(1)); // status lines and the pause file

        let bar = egui::Frame::new().fill(SIDEBAR).inner_margin(egui::Margin::symmetric(14, 9));
        let state = self.update.lock().unwrap().clone();
        if let Update::Available(release) = &state {
            egui::TopBottomPanel::top("update").frame(bar).show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("MiniMacPad {} is available. You have {}.", release.version, update::VERSION));
                    if ui.add(primary("Update and Relaunch")).clicked() {
                        install_update(self.update.clone(), release.clone(), ctx.clone());
                    }
                });
            });
        } else if let Update::Installing(version) = &state {
            egui::TopBottomPanel::top("update").frame(bar).show(ctx, |ui| {
                ui.label(format!("Installing MiniMacPad {version}. The app will reopen by itself."));
            });
        }
        egui::TopBottomPanel::top("top").frame(bar).show(ctx, |ui| self.top_bar(ui));
        egui::TopBottomPanel::bottom("status").frame(bar).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                let (pad, pad_ok) = match self.pad.load(Ordering::Relaxed) {
                    PAD_CONNECTED => ("Pad connected", true),
                    PAD_BLOCKED => ("Pad plugged in, presses hidden", false),
                    _ => ("Pad not plugged in", false),
                };
                let running = self.daemon_running.load(Ordering::Relaxed);
                let service = if running { "Service running" } else { "Service stopped" };
                for (text, ok) in [(pad, pad_ok), (service, running)] {
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 4.0, if ok { GOOD } else { WARN });
                    ui.label(RichText::new(text).small().color(DIM));
                    ui.add_space(10.0);
                }
                if !self.message.is_empty() {
                    ui.label(RichText::new(&self.message).small().color(DIM));
                }
            });
        });
        // The pad panel takes about a third of the window and the picture
        // scales to fit it; anything that still does not fit scrolls.
        let window = ctx.screen_rect().size();
        let narrow = window.x < 720.0;
        let margin = if narrow { 12 } else { 20 };
        let side = egui::Frame::new().fill(SIDEBAR).inner_margin(egui::Margin::same(margin));
        let side_width = (window.x * 0.36).clamp(210.0, 420.0);
        egui::SidePanel::left("pad").frame(side).exact_width(side_width).resizable(false).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let room = Vec2::new(ui.available_width(), ui.available_height() - 130.0);
                self.pad_picture(ui, room);
                ui.add_space(6.0);
                self.pad_footer(ui);
            });
        });
        let main = egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(margin));
        egui::CentralPanel::default().frame(main).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                self.editor(ui);
                ui.add_space(24.0);
                self.device_section(ui);
            });
        });
    }
}

fn main() -> eframe::Result {
    minipad::extend_path();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MiniMacPad")
            .with_inner_size([960.0, 680.0])
            .with_min_inner_size([600.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native("MiniMacPad", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
