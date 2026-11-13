use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::cmd::{powershell, run, which, Log};
use crate::config::{save_config_value, saved_oculus_dir, DEFAULT_OCULUS_DIR};
use crate::devices::{
    detect_headset_model, parse_usb_snapshot, read_device_cache, read_service_telemetry, ControllerStatus,
    HeadsetMotion, UsbDevice, USB_SNAPSHOT_PS,
};
use crate::discover::{
    client_exe, hosts_path, oculus_openxr_json, oculus_root, steam_exe, steamvr_openxr_json, steamvr_root,
};

pub use crate::discover::is_windows;

pub const HOSTS_DOMAINS: &[&str] = &[
    "graph.oculus.com",
    "edge-mqtt.facebook.com",
    "scontent.oculuscdn.com",
    "securecdn.oculus.com",
    "graph.facebook.com",
    "platform.oculus.com",
];

const OCULUS_CONFIG_KEY: &str = r"HKLM\SOFTWARE\WOW6432Node\Oculus VR, LLC\Oculus\Config";
const OPENXR_KEY: &str = r"HKLM\SOFTWARE\Khronos\OpenXR\1";
const OPENXR_AVAILABLE: &str = r"HKLM\SOFTWARE\Khronos\OpenXR\1\AvailableRuntimes";
const FIREWALL_PREFIX: &str = "MetaFixer - ";
const TRACKFIX_PS1: &str = include_str!("../scripts/TrackFix.ps1");

#[derive(Clone, Debug, Default)]
pub struct SystemStatus {
    pub windows: bool,
    #[allow(dead_code)]
    pub admin: bool,
    pub oculus_installed: bool,
    pub oculus_path: String,
    pub steam_installed: bool,
    pub steamvr_installed: bool,
    pub ovr_running: bool,
    pub ovr_start_type: String,
    pub usb_ok: u32,
    pub usb_bad: u32,
    pub usb_total: u32,
    pub usb_ghosts: u32,
    pub usb_port_errors: u32,
    pub detected_model: String,
    pub devices: Vec<UsbDevice>,
    pub usb_errors: Vec<UsbDevice>,
    pub headset: HeadsetMotion,
    pub controllers: Vec<ControllerStatus>,
    pub client_disabled: bool,
    pub hosts_blocked: bool,
    pub dash_blocked: bool,
    pub openxr: String,
    pub steamvr_running: bool,
    pub notes: Vec<String>,
}

pub fn is_admin() -> bool {
    if !is_windows() {
        return false;
    }
    #[cfg(windows)]
    {
        #[link(name = "shell32")]
        extern "system" {
            fn IsUserAnAdmin() -> i32;
        }
        unsafe { IsUserAnAdmin() != 0 }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
pub fn elevate_and_exit() -> ! {
    use std::os::windows::ffi::OsStrExt;
    let exe = std::env::current_exe().unwrap_or_default();
    let wide: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
    let runas: Vec<u16> = "runas\0".encode_utf16().collect();
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut core::ffi::c_void,
            op: *const u16,
            file: *const u16,
            params: *const u16,
            dir: *const u16,
            show: i32,
        ) -> isize;
    }
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            runas.as_ptr(),
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        );
    }
    std::process::exit(0);
}

pub fn probe_status() -> SystemStatus {
    let mut status = SystemStatus {
        windows: is_windows(),
        admin: is_admin(),
        ..SystemStatus::default()
    };
    if !status.windows {
        status
            .notes
            .push("Preview only.".into());
        return status;
    }

    let root = oculus_root(None);
    status.oculus_installed = root.is_some();
    if let Some(root) = &root {
        status.oculus_path = root.display().to_string();
        save_config_value(root);
    }
    status.steam_installed = steam_exe().is_some();
    status.steamvr_installed = steamvr_root().is_some();
    status.steamvr_running = process_running("vrserver");

    let svc = run(&["sc.exe", "query", "OVRService"], None, 20);
    status.ovr_running = svc.stdout.contains("RUNNING");
    let cfg = run(&["sc.exe", "qc", "OVRService"], None, 20);
    status.ovr_start_type = if cfg.stdout.contains("DEMAND_START") {
        "manual".into()
    } else if cfg.stdout.contains("AUTO_START") {
        "automatic".into()
    } else if cfg.stdout.contains("DISABLED") {
        "disabled".into()
    } else {
        String::new()
    };

    let usb = powershell(USB_SNAPSHOT_PS, None, 40);
    let json = usb
        .stdout
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with('{') || line.starts_with('['))
        .unwrap_or(usb.stdout.trim());
    let (devices, errors) = parse_usb_snapshot(json);
    status.usb_total = devices.len() as u32;
    status.usb_ok = devices.iter().filter(|d| d.is_ok()).count() as u32;
    status.usb_ghosts = devices.iter().filter(|d| d.is_ghost()).count() as u32;
    status.usb_bad = status.usb_total.saturating_sub(status.usb_ok);
    status.usb_port_errors = errors.len() as u32;
    status.devices = devices;
    status.usb_errors = errors;

    let (mut headset, controllers, cache_model) = read_device_cache();
    read_service_telemetry(&mut headset);
    status.detected_model = detect_headset_model(&status.devices, &cache_model);
    if status.detected_model.is_empty() && !cache_model.is_empty() {
        status.detected_model = cache_model;
    }
    headset.model = status.detected_model.clone();
    status.headset = headset;
    status.controllers = controllers;

    let client = client_exe(None);
    let disabled = client.as_ref().map(|p| PathBuf::from(format!("{}.disabled", p.display())));
    status.client_disabled = disabled
        .as_ref()
        .is_some_and(|p| p.is_file())
        && client.as_ref().is_some_and(|p| !p.is_file());

    let hosts = fs::read_to_string(hosts_path()).unwrap_or_default().to_ascii_lowercase();
    status.hosts_blocked = HOSTS_DOMAINS
        .iter()
        .all(|domain| hosts.contains(&domain.to_ascii_lowercase()));

    let dash = run(
        &["reg.exe", "query", OCULUS_CONFIG_KEY, "/v", "PreventDashLaunch"],
        None,
        20,
    );
    status.dash_blocked = dash.stdout.contains("0x1");

    let xr = run(&["reg.exe", "query", OPENXR_KEY, "/v", "ActiveRuntime"], None, 20);
    for line in xr.stdout.lines() {
        if line.contains("ActiveRuntime") && line.contains("REG_SZ") {
            if let Some((_, rest)) = line.split_once("REG_SZ") {
                status.openxr = rest.trim().to_string();
            }
        }
    }
    status
}

fn process_running(name: &str) -> bool {
    if !is_windows() {
        return false;
    }
    let result = run(
        &["tasklist.exe", "/FI", &format!("IMAGENAME eq {name}.exe")],
        None,
        20,
    );
    result.stdout.to_ascii_lowercase().contains(&name.to_ascii_lowercase())
}

fn wait_until(pred: impl Fn() -> bool, timeout: Duration, log: &Log) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    log.line("Timed out waiting.");
    false
}

fn winget_install(package_id: &str, log: &Log, location: Option<&Path>) -> Result<(), String> {
    let winget = which("winget").ok_or_else(|| {
        "winget is not available. Install App Installer from the Microsoft Store.".to_string()
    })?;
    let winget = winget.to_string_lossy().into_owned();
    let mut args = vec![
        winget.clone(),
        "install".into(),
        "-e".into(),
        "--id".into(),
        package_id.into(),
        "--accept-package-agreements".into(),
        "--accept-source-agreements".into(),
        "--disable-interactivity".into(),
    ];
    if let Some(location) = location {
        args.push("--location".into());
        args.push(location.display().to_string());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = run(&refs, Some(log), 900);
    let already = [0u32, 0x8A15_002B, 0x8A15_004B];
    if !already.contains(&(result.code as u32)) && location.is_some() {
        log.line("winget --location was not accepted; retrying without a custom path, then we will still bind whatever folder appears.");
        let retry = [
            winget.as_str(),
            "install",
            "-e",
            "--id",
            package_id,
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ];
        let result = run(&retry, Some(log), 900);
        if !already.contains(&(result.code as u32)) {
            log.line(format!(
                "winget returned {} for {package_id} (continuing if the app is already present).",
                result.code
            ));
        }
        return Ok(());
    }
    if !already.contains(&(result.code as u32)) {
        log.line(format!(
            "winget returned {} for {package_id} (continuing if the app is already present).",
            result.code
        ));
    }
    Ok(())
}

fn remember_oculus_location(root: &Path, log: &Log) {
    log.line(format!("Recording Oculus location as {}", root.display()));
    std::env::set_var("OculusBase", root);
    run(
        &["setx.exe", "OculusBase", &root.display().to_string(), "/M"],
        Some(log),
        30,
    );
    let value = format!("{}\\", root.display());
    run(
        &[
            "reg.exe",
            "add",
            r"HKLM\SOFTWARE\WOW6432Node\Oculus VR, LLC\Oculus",
            "/v",
            "Base",
            "/t",
            "REG_SZ",
            "/d",
            &value,
            "/f",
        ],
        Some(log),
        20,
    );
}

pub fn apply_meta_block(log: &Log, root: Option<&Path>) -> Result<(), String> {
    let root = oculus_root(root).ok_or_else(|| {
        "Cannot block Meta: Oculus folder was not found. Choose the install folder and press Install."
            .to_string()
    })?;
    save_config_value(&root);
    log.line(format!("Using Oculus folder {}", root.display()));
    log.line("Blocking Dash auto-launch and software updates…");
    run(
        &[
            "reg.exe",
            "add",
            OCULUS_CONFIG_KEY,
            "/v",
            "PreventDashLaunch",
            "/t",
            "REG_DWORD",
            "/d",
            "1",
            "/f",
        ],
        Some(log),
        20,
    );
    run(
        &[
            "reg.exe",
            "add",
            OCULUS_CONFIG_KEY,
            "/v",
            "CoreChannel",
            "/t",
            "REG_SZ",
            "/d",
            "NO_UPDATES",
            "/f",
        ],
        Some(log),
        20,
    );

    let path = hosts_path();
    log.line(format!("Sinkholing Meta domains in {}…", path.display()));
    let text = fs::read_to_string(&path).unwrap_or_default();
    let backup = path.with_file_name("hosts.metafixer-backup");
    let legacy_backup = path.with_file_name("hosts.rift-s-control-backup");
    if path.is_file() && !backup.is_file() && !legacy_backup.is_file() {
        let _ = fs::copy(&path, &backup);
        log.line(format!("Hosts backup: {}", backup.display()));
    }
    let lower = text.to_ascii_lowercase();
    let mut lines = Vec::new();
    for domain in HOSTS_DOMAINS {
        if !lower.contains(&domain.to_ascii_lowercase()) {
            lines.push(format!("127.0.0.1 {domain}"));
        }
    }
    if !lines.is_empty() {
        let mut body = text;
        if !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str("\n# Meta Fixer\n");
        body.push_str(&lines.join("\n"));
        body.push('\n');
        fs::write(&path, body).map_err(|e| format!("Could not write hosts file: {e}"))?;
    }

    let client = client_exe(Some(&root));
    if let Some(client) = client {
        let dest = PathBuf::from(format!("{}.disabled", client.display()));
        if client.is_file() {
            log.line(format!(
                "Disabling Meta Horizon Link login app ({})…",
                client.display()
            ));
            if dest.exists() {
                let _ = fs::remove_file(&dest);
            }
            fs::rename(&client, &dest).map_err(|e| format!("Could not disable Client.exe: {e}"))?;
        } else if dest.is_file() {
            log.line("Client.exe is already disabled.");
        }
    } else {
        log.line("No Client.exe under this Oculus folder (already removed or a layout we have not seen).");
    }

    log.line("Adding outbound firewall blocks for every Oculus executable…");
    let escaped = root.display().to_string().replace('\'', "''");
    let ps = format!(
        r#"
$root = '{escaped}'
Get-ChildItem -Path $root -Recurse -Filter *.exe -File -ErrorAction SilentlyContinue | ForEach-Object {{
  $dn = '{FIREWALL_PREFIX}' + $_.FullName
  if (-not (Get-NetFirewallRule -DisplayName $dn -ErrorAction SilentlyContinue)) {{
    New-NetFirewallRule -DisplayName $dn -Direction Outbound -Action Block -Program $_.FullName -Profile Any | Out-Null
  }}
}}
Get-ChildItem -Path $root -Recurse -Filter *.exe.disabled -File -ErrorAction SilentlyContinue | ForEach-Object {{
  $original = $_.FullName -replace '\.disabled$',''
  $dn = '{FIREWALL_PREFIX}' + $original
  if (-not (Get-NetFirewallRule -DisplayName $dn -ErrorAction SilentlyContinue)) {{
    New-NetFirewallRule -DisplayName $dn -Direction Outbound -Action Block -Program $original -Profile Any | Out-Null
  }}
}}
"#
    );
    powershell(&ps, Some(log), 180);
    log.line("Meta block applied. Runtime stays local; store/login will not work.");
    Ok(())
}

fn harden_usb(log: &Log) {
    log.line("Disabling USB selective suspend…");
    run(
        &[
            "powercfg",
            "/SETACVALUEINDEX",
            "SCHEME_CURRENT",
            "SUB_USB",
            "USBSELECTIVESUSPEND",
            "0",
        ],
        Some(log),
        20,
    );
    run(
        &[
            "powercfg",
            "/SETDCVALUEINDEX",
            "SCHEME_CURRENT",
            "SUB_USB",
            "USBSELECTIVESUSPEND",
            "0",
        ],
        Some(log),
        20,
    );
    run(&["powercfg", "/SETACTIVE", "SCHEME_CURRENT"], Some(log), 20);
    log.line("Clearing USB power-saving on Oculus and SuperSpeed hubs…");
    let ps = r#"
Get-PnpDevice -ErrorAction SilentlyContinue | Where-Object {
  $_.InstanceId -like '*VID_2833*' -or $_.FriendlyName -match 'SuperSpeed|USB 3|Rift S USB Hub|Oculus'
} | ForEach-Object {
  $inst = $_.InstanceId
  $wmi = Get-CimInstance -ClassName MSPower_DeviceEnable -Namespace root\wmi -ErrorAction SilentlyContinue |
    Where-Object { $_.InstanceName -and ($inst -replace '\\','\\') -and $_.InstanceName -like ('*' + ($inst.Split('\')[-1]) + '*') }
  foreach ($row in $wmi) {
    try { $row.Enable = $false; $row | Set-CimInstance | Out-Null } catch {}
  }
}
"#;
    powershell(ps, Some(log), 60);
}

fn allow_desktop_camera(log: &Log) {
    log.line("Allowing desktop apps to use cameras (headset tracking cameras)…");
    for key in [
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam",
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam\NonPackaged",
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam",
        r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\webcam\NonPackaged",
    ] {
        run(
            &["reg.exe", "add", key, "/v", "Value", "/t", "REG_SZ", "/d", "Allow", "/f"],
            Some(log),
            20,
        );
    }
}

fn set_ovr_manual(log: &Log) {
    log.line("Setting OVRService to Manual so Start/Stop own the runtime…");
    run(&["sc.exe", "config", "OVRService", "start=", "demand"], Some(log), 20);
    run(&["sc.exe", "stop", "OVRService"], Some(log), 20);
}

fn set_openxr_steamvr(log: &Log) {
    let Some(json_path) = steamvr_openxr_json() else {
        log.line("SteamVR OpenXR manifest not found yet — skip ActiveRuntime.");
        return;
    };
    log.line(format!("Setting OpenXR ActiveRuntime to {}", json_path.display()));
    run(
        &[
            "reg.exe",
            "add",
            OPENXR_KEY,
            "/v",
            "ActiveRuntime",
            "/t",
            "REG_SZ",
            "/d",
            &json_path.display().to_string(),
            "/f",
        ],
        Some(log),
        20,
    );
    if let Some(oculus_json) = oculus_openxr_json() {
        run(
            &[
                "reg.exe",
                "add",
                OPENXR_AVAILABLE,
                "/v",
                &oculus_json.display().to_string(),
                "/t",
                "REG_DWORD",
                "/d",
                "0",
                "/f",
            ],
            Some(log),
            20,
        );
    }
}

pub fn install_stack(log: &Log, oculus_dir: Option<&Path>) -> Result<(), String> {
    if !is_windows() {
        return Err("Install only runs on Windows.".into());
    }
    if !is_admin() {
        return Err("Administrator rights are required.".into());
    }
    let mut target = oculus_dir
        .map(Path::to_path_buf)
        .or_else(saved_oculus_dir)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_OCULUS_DIR));
    fs::create_dir_all(&target).map_err(|e| format!("Cannot create {}: {e}", target.display()))?;
    save_config_value(&target);
    log.line(format!("Oculus install folder: {}", target.display()));

    if let Some(existing) = oculus_root(Some(&target)) {
        log.line(format!("Found an existing runtime at {} — using it.", existing.display()));
        target = existing;
        save_config_value(&target);
    } else {
        log.line("Installing Meta Quest Link / Oculus PC runtime via winget…");
        winget_install("Meta.Oculus", log, Some(&target))?;
        log.line("Waiting for Oculus files…");
        wait_until(|| oculus_root(Some(&target)).is_some(), Duration::from_secs(240), log);
    }

    let root = oculus_root(Some(&target)).ok_or_else(|| {
        format!(
            "Oculus runtime not found in {}. Pick the folder that contains Support\\oculus-runtime and press Install again.",
            target.display()
        )
    })?;
    save_config_value(&root);
    remember_oculus_location(&root, log);

    let driver = root.join("Support").join("oculus-drivers").join("oculus-driver.exe");
    if driver.is_file() {
        log.line("Installing / repairing Oculus USB and audio drivers…");
        run(&[&driver.display().to_string()], Some(log), 300);
    }
    let launcher = root
        .join("Support")
        .join("oculus-runtime")
        .join("OVRServiceLauncher.exe");
    if launcher.is_file() {
        log.line("Registering OVRService…");
        run(
            &[&launcher.display().to_string(), "-install", "-start"],
            Some(log),
            120,
        );
    }

    log.line("Installing Steam via winget…");
    winget_install("Valve.Steam", log, None)?;
    if steam_exe().is_none() {
        wait_until(|| steam_exe().is_some(), Duration::from_secs(180), log);
    }
    let exe = steam_exe().ok_or_else(|| "Steam is not installed.".to_string())?;

    log.line("Asking Steam to install SteamVR (app 250820). Sign into Steam if a login window appears.");
    run(
        &[&exe.display().to_string(), "-silent", "steam://install/250820"],
        Some(log),
        30,
    );
    if steamvr_root().is_none() {
        log.line("Waiting for SteamVR files (this can take several minutes)…");
        wait_until(|| steamvr_root().is_some(), Duration::from_secs(900), log);
    }
    if steamvr_root().is_none() {
        log.line("SteamVR is not on disk yet. Sign into Steam, install SteamVR, then press Install again to finish blocking Meta.");
        return Ok(());
    }

    apply_meta_block(log, Some(&root))?;
    harden_usb(log);
    allow_desktop_camera(log);
    set_ovr_manual(log);
    set_openxr_steamvr(log);
    log.line("Install finished. Plug the headset into a rear USB 3 port (Rift S / CV1 also need GPU DisplayPort), then press Start.");
    Ok(())
}

pub fn start_runtime(log: &Log, start_steamvr: bool) -> Result<(), String> {
    if !is_windows() {
        return Err("Start only runs on Windows.".into());
    }
    let mut status = probe_status();
    if !status.oculus_installed {
        return Err("Oculus runtime is missing. Press Install first.".into());
    }
    if status.usb_total == 0 {
        log.line("No Meta USB devices yet. Plug a Rift or Quest Link cable into a rear USB 3 port, wait a few seconds, then press Start again. Air Link will not show USB devices.");
    }
    log.line("Starting OVRService…");
    run(&["sc.exe", "start", "OVRService"], Some(log), 30);
    std::thread::sleep(Duration::from_secs(5));
    status = probe_status();
    if status.usb_bad > 0 || (status.usb_total > 0 && status.usb_ok == 0) {
        log.line("USB looks unhealthy after start — running Fix…");
        fix_connection(log)?;
    }
    if start_steamvr {
        if !status.steamvr_installed {
            log.line("SteamVR is not installed. Press Install, or install SteamVR from Steam.");
            return Ok(());
        }
        log.line("Starting SteamVR…");
        if let Some(exe) = steam_exe() {
            run(&[&exe.display().to_string(), "-applaunch", "250820"], Some(log), 30);
        }
    }
    log.line("Start complete. Put the headset on. Tracking cameras / Link should come alive within a few seconds.");
    Ok(())
}

pub fn stop_runtime(log: &Log) -> Result<(), String> {
    if !is_windows() {
        return Err("Stop only runs on Windows.".into());
    }
    log.line("Stopping SteamVR…");
    for name in ["vrmonitor", "vrserver", "vrcompositor", "vrstartup", "vrwebhelper"] {
        run(&["taskkill.exe", "/F", "/IM", &format!("{name}.exe")], Some(log), 20);
    }
    log.line("Stopping OVRService…");
    run(&["sc.exe", "stop", "OVRService"], Some(log), 20);
    log.line("Stop complete. Headset runtime is down.");
    Ok(())
}

fn trackfix_path() -> PathBuf {
    let dir = std::env::temp_dir().join("MetaFixer");
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("TrackFix.ps1");
    let _ = fs::write(&path, TRACKFIX_PS1);
    path
}

pub fn fix_connection(log: &Log) -> Result<(), String> {
    if !is_windows() {
        return Err("Fix only runs on Windows.".into());
    }
    let script = trackfix_path();
    log.line("Fix restarts Meta USB devices and may power-cycle the USB root hub.");
    log.line("The PC can freeze for several seconds — mouse and keyboard included. That is expected.");
    log.line("Closing SteamVR for a clean USB/service reset…");
    for name in ["vrmonitor", "vrserver", "vrcompositor", "vrstartup"] {
        run(&["taskkill.exe", "/F", "/IM", &format!("{name}.exe")], Some(log), 20);
    }
    std::thread::sleep(Duration::from_secs(2));
    log.line("Running TrackFix (bounce devices, hub restart, OVRService)…");
    let result = run(
        &[
            "powershell.exe",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            &script.display().to_string(),
            "-Fix",
            "-NoPause",
        ],
        Some(log),
        180,
    );
    if result.code != 0 {
        return Err(format!("TrackFix exited {}", result.code));
    }
    log.line("Fix finished. Press Start if you want SteamVR again.");
    Ok(())
}
