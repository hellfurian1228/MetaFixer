use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Color32, FontId, Frame, Layout, Margin, RichText, Rounding, Stroke, TextEdit,
    Vec2,
};

use crate::cmd::Log;
use crate::config::{saved_oculus_dir, DEFAULT_OCULUS_DIR};
use crate::discover::oculus_root;
use crate::winops::{
    fix_connection, install_stack, probe_status, start_runtime, stop_runtime, SystemStatus,
};

const BG: Color32 = Color32::from_rgb(15, 17, 21);
const SURFACE: Color32 = Color32::from_rgb(24, 27, 33);
const CARD: Color32 = Color32::from_rgb(30, 34, 42);
const BORDER: Color32 = Color32::from_rgb(48, 54, 64);
const TEXT: Color32 = Color32::from_rgb(236, 238, 241);
const MUTED: Color32 = Color32::from_rgb(148, 157, 172);
const HINT: Color32 = Color32::from_rgb(122, 130, 144);
const LOG_BG: Color32 = Color32::from_rgb(12, 14, 18);
const ACCENT: Color32 = Color32::from_rgb(110, 168, 255);
const INSTALL: Color32 = Color32::from_rgb(47, 111, 232);
const START: Color32 = Color32::from_rgb(31, 158, 90);
const STOP: Color32 = Color32::from_rgb(196, 62, 62);
const FIX: Color32 = Color32::from_rgb(201, 122, 28);
const BROWSE: Color32 = Color32::from_rgb(58, 66, 80);
const WHITE: Color32 = Color32::from_rgb(255, 255, 255);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Control,
    HowToUse,
    HowItWorks,
}

#[derive(Clone, Copy)]
enum TaskKind {
    Install,
    Start,
    Stop,
    Fix,
}

struct Task {
    handle: JoinHandle<Result<(), String>>,
}

pub struct MetaFixerApp {
    path: String,
    log: Log,
    status: SystemStatus,
    busy: bool,
    launch_steamvr: bool,
    last_probe: Instant,
    task: Option<Task>,
    notes_shown: bool,
    tab: Tab,
    logs_open: bool,
}

impl MetaFixerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = BG;
        visuals.extreme_bg_color = LOG_BG;
        visuals.override_text_color = Some(TEXT);
        visuals.widgets.noninteractive.rounding = Rounding::same(8.0);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(18, 21, 28);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(32, 37, 46);
        visuals.widgets.inactive.rounding = Rounding::same(8.0);
        visuals.widgets.hovered.rounding = Rounding::same(8.0);
        visuals.widgets.active.rounding = Rounding::same(8.0);
        visuals.widgets.open.rounding = Rounding::same(8.0);
        cc.egui_ctx.set_visuals(visuals);

        let detected = oculus_root(None)
            .or_else(saved_oculus_dir)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_OCULUS_DIR));

        let log = Log::default();
        log.line("Ready.");

        let mut app = Self {
            path: detected.display().to_string(),
            log,
            status: probe_status(),
            busy: false,
            launch_steamvr: true,
            last_probe: Instant::now(),
            task: None,
            notes_shown: false,
            tab: Tab::Control,
            logs_open: false,
        };
        app.apply_notes();
        app
    }

    fn apply_notes(&mut self) {
        if self.notes_shown {
            return;
        }
        for note in &self.status.notes {
            if note.contains("Preview only") {
                continue;
            }
            self.log.line(note.clone());
        }
        self.notes_shown = true;
    }

    fn poll_task(&mut self) {
        if let Some(task) = self.task.take() {
            if task.handle.is_finished() {
                match task.handle.join() {
                    Ok(Ok(())) => self.log.line("Done."),
                    Ok(Err(err)) => self.log.line(format!("Error: {err}")),
                    Err(_) => self.log.line("Error: task panicked."),
                }
                self.busy = false;
                self.status = probe_status();
                self.apply_notes();
            } else {
                self.task = Some(task);
            }
        }
    }

    fn maybe_probe(&mut self) {
        if self.busy {
            return;
        }
        if self.last_probe.elapsed() >= Duration::from_secs(4) {
            self.status = probe_status();
            self.apply_notes();
            self.last_probe = Instant::now();
            if !self.status.oculus_path.is_empty() {
                self.path = self.status.oculus_path.clone();
            }
        }
    }

    fn browse(&mut self) {
        let start = if self.path.trim().is_empty() {
            DEFAULT_OCULUS_DIR.to_string()
        } else {
            self.path.clone()
        };
        if let Some(picked) = rfd::FileDialog::new()
            .set_title("Oculus runtime folder")
            .set_directory(&start)
            .pick_folder()
        {
            self.path = picked.display().to_string();
            crate::config::save_config_value(&picked);
        }
    }

    fn kick(&mut self, kind: TaskKind) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.logs_open = true;
        let title = match kind {
            TaskKind::Install => "Install",
            TaskKind::Start => "Start",
            TaskKind::Stop => "Stop",
            TaskKind::Fix => "Fix",
        };
        self.log.line(format!("{title}…"));
        let log = self.log.clone();
        let path = PathBuf::from(self.path.trim());
        let launch = self.launch_steamvr;
        crate::config::save_config_value(&path);
        let handle = std::thread::spawn(move || match kind {
            TaskKind::Install => install_stack(&log, Some(&path)),
            TaskKind::Start => start_runtime(&log, launch),
            TaskKind::Stop => stop_runtime(&log),
            TaskKind::Fix => fix_connection(&log),
        });
        self.task = Some(Task { handle });
    }
}

impl eframe::App for MetaFixerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_task();
        self.maybe_probe();
        ctx.request_repaint_after(if self.busy {
            Duration::from_millis(250)
        } else {
            Duration::from_secs(1)
        });

        egui::TopBottomPanel::top("chrome")
            .show_separator_line(false)
            .frame(
                Frame::none()
                    .fill(BG)
                    .inner_margin(Margin::symmetric(20.0, 16.0)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                ui.spacing_mut().button_padding = Vec2::new(10.0, 6.0);
                header(ui);
                ui.add_space(8.0);
                self.tab_bar(ui);
            });

        if self.logs_open {
            egui::SidePanel::right("logs")
                .resizable(true)
                .default_width(300.0)
                .min_width(220.0)
                .max_width(480.0)
                .show_separator_line(false)
                .frame(
                    Frame::none()
                        .fill(BG)
                        .inner_margin(Margin {
                            left: 4.0,
                            right: 16.0,
                            top: 0.0,
                            bottom: 16.0,
                        }),
                )
                .show(ctx, |ui| {
                    self.log_panel(ui);
                });
        }

        egui::CentralPanel::default()
            .frame(
                Frame::none()
                    .fill(BG)
                    .inner_margin(Margin::symmetric(20.0, 8.0)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.tab {
                        Tab::Control => self.control_tab(ui),
                        Tab::HowToUse => how_to_use(ui),
                        Tab::HowItWorks => how_it_works(ui),
                    });
            });
    }
}

impl MetaFixerApp {
    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (label, value, width) in [
                ("Control", Tab::Control, 96.0),
                ("How to use", Tab::HowToUse, 112.0),
                ("How it works", Tab::HowItWorks, 128.0),
            ] {
                tab_button(ui, label, width, self.tab == value, |on| {
                    if on {
                        self.tab = value;
                    }
                });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                tab_button(ui, "Logs", 80.0, self.logs_open, |on| {
                    if on {
                        self.logs_open = !self.logs_open;
                    }
                });
            });
        });
    }

    fn control_tab(&mut self, ui: &mut egui::Ui) {
        banner(
            ui,
            "Fix restarts the USB hub. The PC — including mouse and keyboard — can freeze for several seconds. That is normal.",
        );
        if !self.status.windows {
            ui.add_space(4.0);
            banner(ui, "Preview only on this machine. Install, Start, Stop, and Fix run on Windows.");
        }
        ui.add_space(6.0);
        status_grid(ui, &self.status);
        ui.add_space(6.0);
        devices_panel(ui, &self.status);
        ui.add_space(4.0);
        self.folder_row(ui);
        ui.add_space(2.0);
        self.actions_row(ui);
    }

    fn folder_row(&mut self, ui: &mut egui::Ui) {
        Frame::none()
            .fill(SURFACE)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .rounding(Rounding::same(10.0))
            .inner_margin(Margin::symmetric(12.0, 10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Folder").size(12.0).color(MUTED));
                    let width = (ui.available_width() - 84.0).max(80.0);
                    let edit = TextEdit::singleline(&mut self.path)
                        .font(FontId::monospace(12.0))
                        .desired_width(width)
                        .hint_text(r"C:\MetaFixer\Oculus");
                    ui.add_enabled(!self.busy, edit);
                    ui.add_enabled_ui(!self.busy, |ui| {
                        if slim_button(ui, "Browse", BROWSE, 76.0).clicked() {
                            self.browse();
                        }
                    });
                });
                ui.label(
                    RichText::new("Default C:\\MetaFixer\\Oculus. Meta-block finds the runtime and Client.exe from here.")
                        .size(11.0)
                        .color(HINT),
                );
            });
    }

    fn actions_row(&mut self, ui: &mut egui::Ui) {
        let windows = self.status.windows;
        let enabled = !self.busy && windows;
        let mut action = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let checkbox_w = 168.0;
            let gaps = 8.0 * 4.0;
            let buttons_w = (ui.available_width() - checkbox_w - gaps).max(240.0);
            let btn_w = (buttons_w / 4.0).max(64.0);
            let specs = [
                ("Install", "First-time setup: runtime, SteamVR, Meta-block", INSTALL, TaskKind::Install),
                ("Start", "Start the headset runtime and SteamVR", START, TaskKind::Start),
                ("Stop", "Stop SteamVR and OVRService", STOP, TaskKind::Stop),
                ("Fix", "Restart Meta USB devices and the USB hub. The PC may freeze for several seconds.", FIX, TaskKind::Fix),
            ];
            ui.add_enabled_ui(enabled, |ui| {
                for (label, hint, color, kind) in specs {
                    if slim_button(ui, label, color, btn_w).on_hover_text(hint).clicked() {
                        action = Some(kind);
                    }
                }
            });
            ui.add_space(4.0);
            ui.checkbox(&mut self.launch_steamvr, "Start SteamVR");
        });
        if let Some(kind) = action {
            self.kick(kind);
        }
    }

    fn log_panel(&self, ui: &mut egui::Ui) {
        let height = ui.available_height();
        Frame::none()
            .fill(LOG_BG)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .rounding(Rounding::same(10.0))
            .inner_margin(Margin::same(10.0))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(height);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("LOG").size(10.0).strong().color(HINT));
                    if self.busy {
                        ui.label(RichText::new("working").size(10.0).color(ACCENT));
                    }
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        ui.set_width(ui.available_width());
                        let text = self.log.snapshot().join("\n");
                        ui.add(
                            egui::Label::new(
                                RichText::new(text)
                                    .font(FontId::monospace(11.5))
                                    .color(Color32::from_rgb(186, 193, 204)),
                            )
                            .wrap(),
                        );
                    });
            });
    }
}

fn tab_button(ui: &mut egui::Ui, label: &str, width: f32, on: bool, mut clicked: impl FnMut(bool)) {
    let fill = if on { ACCENT } else { CARD };
    let color = if on {
        Color32::from_rgb(12, 18, 28)
    } else {
        TEXT
    };
    if ui
        .add_sized(
            Vec2::new(width, 32.0),
            egui::Button::new(RichText::new(label).size(13.0).strong().color(color))
                .fill(fill)
                .rounding(Rounding::same(8.0))
                .min_size(Vec2::ZERO),
        )
        .clicked()
    {
        clicked(true);
    }
}

fn header(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        Frame::none()
            .fill(ACCENT)
            .rounding(Rounding::same(2.0))
            .show(ui, |ui| {
                ui.set_min_size(Vec2::new(3.0, 22.0));
            });
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("Meta Fixer").size(20.0).strong().color(TEXT));
            ui.label(
                RichText::new("Rift CV1 · Rift S · Quest · Quest 2 · Quest 3 / 3S · Quest Pro")
                    .size(12.0)
                    .color(MUTED),
            );
        });
    });
}

fn banner(ui: &mut egui::Ui, text: &str) {
    Frame::none()
        .fill(Color32::from_rgb(42, 36, 22))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(90, 72, 32)))
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::symmetric(10.0, 8.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(Color32::from_rgb(232, 201, 140)));
        });
}

fn how_to_use(ui: &mut egui::Ui) {
    banner(
        ui,
        "Fix restarts the USB hub. The PC — including mouse and keyboard — can freeze for several seconds. That is normal. Wait; do not hit the power button.",
    );
    ui.add_space(8.0);
    explain(
        ui,
        "1. Plug in",
        "Rift S / CV1: rear USB 3 plus GPU DisplayPort. Quest: USB Link cable. Air Link also works — then no USB device will show, but Install / Start / Stop still apply.",
    );
    explain(
        ui,
        "2. Folder",
        "Leave the folder as C:\\MetaFixer\\Oculus, or Browse if the PC runtime is already installed. Meta-block finds the runtime and Client.exe from here.",
    );
    explain(
        ui,
        "3. Install once",
        "Press Install on Control. Sign into Steam if it asks so SteamVR can download. This also applies the Meta-block (no login, Horizon, or store).",
    );
    explain(
        ui,
        "4. Start and Stop",
        "Press Start. Put the headset on. Leave Start SteamVR checked unless you already launched it. Press Stop when you are done so the runtime is fully down.",
    );
    explain(
        ui,
        "5. Fix if it dies",
        "If USB errors, disconnects, or tracking die, press Fix. Expect a short PC freeze while the USB hub restarts. That is expected.",
    );
    explain(
        ui,
        "Logs",
        "Toggle Logs on the right of the tab bar to watch Install, Start, Stop, and Fix. The panel opens on its own when you press one of those buttons.",
    );
}

fn how_it_works(ui: &mut egui::Ui) {
    Frame::none()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::symmetric(14.0, 14.0))
        .show(ui, |ui| {
            ui.label(RichText::new("Headset  ->  OVRService  ->  SteamVR  ->  game").size(15.0).strong().color(TEXT));
            ui.add_space(4.0);
            ui.label(
                RichText::new("Local runtime only. No Meta login, Horizon Home, or store.")
                    .size(12.0)
                    .color(MUTED),
            );
        });
    ui.add_space(10.0);
    explain(
        ui,
        "What this is",
        "Meta Fixer is a small Windows panel for using a Rift or Quest on PC without talking to Meta. Install once, then Start / Stop / Fix own the local Oculus runtime so SteamVR can see the headset.",
    );
    explain(
        ui,
        "How the picture gets to the game",
        "The headset sends tracking and video over USB (plus DisplayPort on Rift CV1 / Rift S). Windows hands that to the Oculus PC runtime (OVRService). SteamVR reads the runtime and feeds the game. Quest can also use Air Link — then there is no USB device to list, but Start, Stop, and Fix still manage the runtime.",
    );
    explain(
        ui,
        "Install",
        "Installs the Meta / Oculus PC runtime, USB drivers, Steam, and SteamVR into the folder you pick (default C:\\MetaFixer\\Oculus). Then it applies the Meta-block: sinkhole login/update hosts, outbound firewall on Oculus executables, disable Dash, and rename Client.exe so Horizon cannot launch. OVRService is set to Manual so this app owns Start and Stop.",
    );
    explain(
        ui,
        "Start and Stop",
        "Start brings up OVRService (and SteamVR if the checkbox is on). If USB already looks bad, Start runs Fix first. Stop kills SteamVR and the service so the headset is fully down.",
    );
    explain(
        ui,
        "Fix — and the freeze",
        "Windows often leaves ghost USB entries after a Rift or Quest blip. Fix closes SteamVR, restarts the stuck Meta USB devices, and if ghosts remain it power-cycles the USB root hub — a software unplug/replug of everything on that controller. Mouse, keyboard, and the headset can freeze for several seconds. That is expected. Wait; do not hit the power button. Then OVRService restarts.",
    );
    explain(
        ui,
        "Status and devices",
        "The Control tab watches USB (OK, errors, ghosts), the headset (connected, worn, camera frames / motion), and controllers (connected vs asleep). It finds the real Oculus folder from the box you typed, saved config, the registry, OVRService, and disk — it does not assume C:\\Program Files\\Oculus.",
    );
    explain(
        ui,
        "What it will not do",
        "It cannot repair a bad cable, a USB port that is not SuperSpeed, or a GPU with no DisplayPort for Rift. It will not talk to Meta after Install. Firmware updates and the store stay blocked on purpose.",
    );
    explain(
        ui,
        "Open source",
        "Meta Fixer is MIT licensed. USB repair uses TrackFix from Kilvoctu/rift_s_no_account (also MIT). Not affiliated with Meta.",
    );
}

fn explain(ui: &mut egui::Ui, title: &str, body: &str) {
    Frame::none()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.label(RichText::new(title).size(13.0).strong().color(ACCENT));
            ui.add_space(4.0);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.add(
                egui::Label::new(RichText::new(body).size(13.0).color(TEXT)).wrap(),
            );
        });
    ui.add_space(8.0);
}

fn step(ui: &mut egui::Ui, num: &str, text: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(num).size(12.0).strong().color(ACCENT));
        ui.label(RichText::new(text).size(12.5).color(TEXT));
    });
}

fn status_grid(ui: &mut egui::Ui, status: &SystemStatus) {
    let cards = [
        ("RUNTIME", oculus_card(status)),
        ("STEAMVR", steam_card(status)),
        ("SERVICE", service_card(status)),
        ("USB", usb_card(status)),
        ("META", meta_card(status)),
        ("OPENXR", openxr_card(status)),
    ];
    for row in cards.chunks(3) {
        ui.columns(3, |cols| {
            for (i, (kicker, value)) in row.iter().enumerate() {
                compact_card(&mut cols[i], kicker, value);
            }
        });
    }
}

fn slim_button(ui: &mut egui::Ui, label: &str, fill: Color32, width: f32) -> egui::Response {
    ui.add_sized(
        Vec2::new(width, 36.0),
        egui::Button::new(RichText::new(label).size(13.0).strong().color(WHITE))
            .fill(fill)
            .rounding(Rounding::same(8.0))
            .min_size(Vec2::ZERO),
    )
}

fn compact_card(ui: &mut egui::Ui, kicker: &str, value: &str) {
    Frame::none()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(Rounding::same(9.0))
        .inner_margin(Margin::symmetric(10.0, 8.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical(|ui| {
                ui.label(RichText::new(kicker).size(10.0).strong().color(HINT));
                ui.label(RichText::new(value).size(13.0).strong().color(TEXT).line_height(Some(16.0)));
            });
        });
}

fn oculus_card(status: &SystemStatus) -> String {
    if !status.detected_model.is_empty() {
        status.detected_model.clone()
    } else if status.oculus_installed {
        "Runtime ready".into()
    } else {
        "Missing".into()
    }
}

fn steam_card(status: &SystemStatus) -> String {
    if status.steamvr_running {
        "Running".into()
    } else if status.steamvr_installed {
        "Ready".into()
    } else if status.steam_installed {
        "No SteamVR".into()
    } else {
        "Missing".into()
    }
}

fn service_card(status: &SystemStatus) -> String {
    if status.ovr_running {
        if status.ovr_start_type.is_empty() {
            "Running".into()
        } else {
            format!("Running · {}", status.ovr_start_type)
        }
    } else if status.oculus_installed {
        "Stopped".into()
    } else {
        "Missing".into()
    }
}

fn usb_card(status: &SystemStatus) -> String {
    if status.usb_total == 0 {
        if status.usb_port_errors > 0 {
            format!("Unplugged · {} port errors", status.usb_port_errors)
        } else {
            "Unplugged".into()
        }
    } else {
        let mut parts = vec![format!("{} OK", status.usb_ok)];
        if status.usb_bad > 0 {
            parts.push(format!("{} bad", status.usb_bad));
        }
        if status.usb_ghosts > 0 {
            parts.push(format!("{} ghost", status.usb_ghosts));
        }
        if status.usb_port_errors > 0 {
            parts.push(format!("{} port", status.usb_port_errors));
        }
        parts.join(" · ")
    }
}

fn meta_card(status: &SystemStatus) -> String {
    if status.hosts_blocked && status.dash_blocked && status.client_disabled {
        "Blocked".into()
    } else if !status.hosts_blocked && !status.dash_blocked && !status.client_disabled {
        "Open".into()
    } else {
        "Partial".into()
    }
}

fn devices_panel(ui: &mut egui::Ui, status: &SystemStatus) {
    Frame::none()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::symmetric(14.0, 12.0))
        .show(ui, |ui| {
            ui.label(RichText::new("DEVICES").size(10.0).strong().color(ACCENT));
            ui.add_space(4.0);
            let hs = &status.headset;
            if !hs.model.is_empty() || !hs.connection.is_empty() || hs.camera_frames.is_some() {
                let worn = match hs.worn {
                    Some(true) => "worn",
                    Some(false) => "not worn",
                    None => "wear unknown",
                };
                let frames = match hs.camera_frames {
                    Some(n) if n > 0 => format!("{n} cam frames/min"),
                    Some(0) => "cameras idle".into(),
                    _ => "no camera telemetry".into(),
                };
                let conn = if hs.connection.is_empty() {
                    "USB / Link unknown".into()
                } else {
                    format!("headset {}", hs.connection)
                };
                let track = if hs.tracking.is_empty() {
                    "motion unknown".into()
                } else {
                    format!("motion {}", hs.tracking.to_ascii_lowercase())
                };
                step(ui, "HMD", &format!("{conn} · {track} · {worn} · {frames}"));
                if !hs.operational.is_empty() {
                    ui.label(RichText::new(format!("Operational: {}", hs.operational)).size(12.0).color(MUTED));
                }
                if !hs.problems.is_empty() {
                    ui.label(
                        RichText::new(format!("Headset errors: {}", hs.problems.join(", ")))
                            .size(12.5)
                            .color(Color32::from_rgb(232, 160, 140)),
                    );
                }
                if !hs.log_age.is_empty() {
                    ui.label(RichText::new(format!("Runtime log {}", hs.log_age)).size(11.0).color(HINT));
                }
            } else {
                ui.label(RichText::new("No headset telemetry yet. Plug in a Rift or Quest Link cable, or start the runtime.").size(12.5).color(MUTED));
            }
            ui.add_space(6.0);
            if status.controllers.is_empty() {
                ui.label(RichText::new("Controllers: none reported (asleep, unplugged, or Quest inside-out with no PC cache yet).").size(12.5).color(MUTED));
            } else {
                for ctrl in &status.controllers {
                    let state = if ctrl.connected {
                        "connected · motion expected when tracking"
                    } else if ctrl.raw_state.is_empty() {
                        "disconnected"
                    } else {
                        "asleep / disconnected — press the Meta button to wake"
                    };
                    step(ui, "CTL", &format!("{} · {state}", ctrl.name));
                }
            }
            ui.add_space(6.0);
            if status.devices.is_empty() {
                ui.label(RichText::new("USB: no Meta devices on the bus (Air Link / unplugged).").size(12.5).color(MUTED));
            } else {
                for dev in &status.devices {
                    let mut flags = vec![dev.status.clone()];
                    if dev.is_ghost() {
                        flags.push("disconnected / ghost".into());
                    } else if !dev.is_ok() {
                        flags.push("USB error".into());
                    }
                    if let Some(pid) = dev.pid {
                        flags.push(format!("PID {pid:04X}"));
                    }
                    let line = format!("{} · {} · {}", dev.kind(), dev.name, flags.join(" · "));
                    let color = if dev.is_ok() { TEXT } else { Color32::from_rgb(232, 160, 140) };
                    ui.label(RichText::new(line).size(12.5).color(color));
                }
            }
            for err in &status.usb_errors {
                if status.devices.iter().any(|d| d.instance_id == err.instance_id) {
                    continue;
                }
                ui.label(
                    RichText::new(format!("USB port error · {} · {}", err.name, err.status))
                        .size(12.5)
                        .color(Color32::from_rgb(232, 201, 140)),
                );
            }
        });
}

fn openxr_card(status: &SystemStatus) -> String {
    if status.openxr.is_empty() {
        "—".into()
    } else {
        status
            .openxr
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(&status.openxr)
            .to_string()
    }
}
