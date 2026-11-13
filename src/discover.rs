use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use walkdir::WalkDir;

use crate::cmd::{run, which};
use crate::config::{saved_oculus_dir, DEFAULT_OCULUS_DIR};

pub fn is_windows() -> bool {
    cfg!(windows)
}

pub fn program_files() -> PathBuf {
    PathBuf::from(std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into()))
}

pub fn program_files_x86() -> PathBuf {
    PathBuf::from(
        std::env::var("ProgramFiles(x86)").unwrap_or_else(|_| r"C:\Program Files (x86)".into()),
    )
}

fn expand_vars(raw: &str) -> String {
    let mut out = raw.to_string();
    if let Ok(re) = Regex::new(r"%([^%]+)%") {
        for cap in re.captures_iter(raw) {
            if let Some(name) = cap.get(1) {
                if let Ok(value) = std::env::var(name.as_str()) {
                    out = out.replace(&format!("%{}%", name.as_str()), &value);
                }
            }
        }
    }
    out
}

pub fn looks_like_oculus(path: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !path.is_dir() {
        return false;
    }
    if path.join("Support").join("oculus-runtime").is_dir() {
        return true;
    }
    if path
        .join("Support")
        .join("oculus-runtime")
        .join("OVRServer_x64.exe")
        .is_file()
    {
        return true;
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    name != "oculus-runtime" && path.join("OVRServer_x64.exe").is_file()
}

pub fn as_oculus_root(path: &Path) -> Option<PathBuf> {
    let mut path = path.to_path_buf();
    if path.is_file() {
        if let Some(parent) = path.parent() {
            path = parent.to_path_buf();
        }
    }
    if looks_like_oculus(&path) {
        return Some(path);
    }
    let nested = path.join("Oculus");
    if looks_like_oculus(&nested) {
        return Some(nested);
    }
    if let Some(parent) = path.parent() {
        if looks_like_oculus(parent) {
            return Some(parent.to_path_buf());
        }
        if let Some(grand) = parent.parent() {
            if looks_like_oculus(grand) {
                return Some(grand.to_path_buf());
            }
        }
    }
    None
}

pub fn reg_query(key: &str, value: &str) -> Option<String> {
    if !is_windows() {
        return None;
    }
    let result = run(&["reg.exe", "query", key, "/v", value], None, 20);
    if result.code != 0 {
        return None;
    }
    for line in result.stdout.lines() {
        if line.to_ascii_lowercase().contains(&value.to_ascii_lowercase()) && line.contains("REG_") {
            if let Some((_, rest)) = line.split_once("REG_") {
                if let Some((_, data)) = rest.split_once(char::is_whitespace) {
                    let data = data.trim();
                    if !data.is_empty() {
                        return Some(data.to_string());
                    }
                }
            }
        }
    }
    None
}

fn service_bin_dir(service: &str) -> Option<PathBuf> {
    if !is_windows() {
        return None;
    }
    let result = run(&["sc.exe", "qc", service], None, 20);
    for line in result.stdout.lines() {
        if line.to_ascii_uppercase().contains("BINARY_PATH_NAME") {
            let raw = line.split_once(':').map(|(_, r)| r.trim().trim_matches('"')).unwrap_or("");
            let token = raw.split(" -").next().unwrap_or(raw).trim().trim_matches('"');
            let exe = PathBuf::from(token);
            if exe.is_file() {
                return exe.parent().map(Path::to_path_buf);
            }
            if exe.parent().is_some_and(|p| p.is_dir()) {
                return exe.parent().map(Path::to_path_buf);
            }
        }
    }
    None
}

pub fn oculus_root(preferred: Option<&Path>) -> Option<PathBuf> {
    let mut ordered: Vec<PathBuf> = Vec::new();
    let mut add = |path: Option<PathBuf>| {
        let Some(path) = path else { return };
        let path = PathBuf::from(expand_vars(&path.to_string_lossy()));
        if !ordered.iter().any(|p| p == &path) {
            ordered.push(path);
        }
    };

    add(preferred.map(Path::to_path_buf));
    add(saved_oculus_dir());
    add(std::env::var("OculusBase").ok().map(PathBuf::from));

    if is_windows() {
        for key in [
            r"HKLM\SOFTWARE\WOW6432Node\Oculus VR, LLC\Oculus",
            r"HKLM\SOFTWARE\Oculus VR, LLC\Oculus",
            r"HKCU\SOFTWARE\Oculus VR, LLC\Oculus",
            r"HKLM\SOFTWARE\WOW6432Node\Oculus VR, LLC\Oculus\Config",
        ] {
            for value in ["Base", "InitialAppBase", "InstallDir", "OculusBase"] {
                add(reg_query(key, value).map(PathBuf::from));
            }
        }
        if let Some(svc) = service_bin_dir("OVRService") {
            add(Some(svc.clone()));
            add(svc.parent().map(Path::to_path_buf));
            add(svc.parent().and_then(|p| p.parent()).map(Path::to_path_buf));
        }
        for exe_name in ["OVRServer_x64.exe", "OVRServiceLauncher.exe"] {
            if let Some(found) = which(exe_name) {
                add(found.parent().map(Path::to_path_buf));
            }
        }
    }

    add(Some(PathBuf::from(DEFAULT_OCULUS_DIR)));
    add(Some(PathBuf::from(r"C:\RiftS\Oculus")));
    add(Some(PathBuf::from(r"C:\Oculus")));
    add(Some(PathBuf::from(r"C:\Games\Oculus")));
    add(Some(PathBuf::from(r"C:\VR\Oculus")));
    add(Some(program_files().join("Oculus")));
    add(Some(program_files_x86().join("Oculus")));
    add(Some(program_files().join("Meta Horizon")));
    add(Some(program_files().join("Oculus Inc").join("Oculus")));
    add(Some(PathBuf::from(r"D:\MetaFixer\Oculus")));
    add(Some(PathBuf::from(r"D:\Oculus")));
    add(Some(PathBuf::from(r"D:\RiftS\Oculus")));
    add(Some(PathBuf::from(r"D:\Games\Oculus")));
    add(Some(PathBuf::from(r"E:\MetaFixer\Oculus")));
    add(Some(PathBuf::from(r"E:\Oculus")));
    add(Some(PathBuf::from(r"E:\RiftS\Oculus")));

    for path in &ordered {
        if let Some(found) = as_oculus_root(path) {
            return Some(found);
        }
    }

    let mut search_roots = vec![
        PathBuf::from(DEFAULT_OCULUS_DIR).parent().map(Path::to_path_buf),
        Some(program_files()),
        Some(program_files_x86()),
        Some(PathBuf::from("D:/MetaFixer")),
        Some(PathBuf::from("D:/Oculus")),
        Some(PathBuf::from("D:/RiftS")),
        Some(PathBuf::from("E:/MetaFixer")),
        Some(PathBuf::from("E:/Oculus")),
        Some(PathBuf::from("E:/RiftS")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    for drive in ["D:/", "E:/"] {
        let drive_path = PathBuf::from(drive);
        if drive_path.is_dir() {
            if let Ok(entries) = fs::read_dir(&drive_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
                    if path.is_dir()
                        && ["oculus", "rift", "meta", "horizon"]
                            .iter()
                            .any(|token| name.contains(token))
                    {
                        search_roots.push(path);
                    }
                }
            }
        }
    }

    for root in search_roots {
        if !root.is_dir() {
            continue;
        }
        let walker = WalkDir::new(&root).max_depth(6).into_iter().filter_entry(|e| {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            !matches!(
                name.as_str(),
                "windows" | "windows.old" | "$recycle.bin" | "system volume information" | "winsxs"
            )
        });
        for entry in walker.flatten() {
            let name = entry.file_name().to_string_lossy();
            if name.eq_ignore_ascii_case("OVRServer_x64.exe")
                || name.eq_ignore_ascii_case("OVRServiceLauncher.exe")
            {
                if let Some(found) = as_oculus_root(entry.path()) {
                    return Some(found);
                }
            }
        }
    }
    None
}

pub fn steam_exe() -> Option<PathBuf> {
    let candidates = [
        reg_query(r"HKCU\SOFTWARE\Valve\Steam", "SteamPath"),
        reg_query(r"HKLM\SOFTWARE\WOW6432Node\Valve\Steam", "InstallPath"),
        reg_query(r"HKLM\SOFTWARE\Valve\Steam", "InstallPath"),
        std::env::var("STEAM_PATH").ok(),
        Some(program_files_x86().join("Steam").to_string_lossy().into_owned()),
        Some(program_files().join("Steam").to_string_lossy().into_owned()),
        Some(
            PathBuf::from(DEFAULT_OCULUS_DIR)
                .parent()
                .unwrap_or(Path::new(r"C:\MetaFixer"))
                .join("Steam")
                .to_string_lossy()
                .into_owned(),
        ),
    ];
    for raw in candidates.into_iter().flatten() {
        let raw = raw.replace('/', r"\");
        let path = PathBuf::from(&raw);
        let exe = if path.file_name().is_some_and(|n| n.eq_ignore_ascii_case("steam.exe")) {
            path.clone()
        } else {
            path.join("steam.exe")
        };
        if exe.is_file() {
            return Some(exe);
        }
    }
    None
}

pub fn steam_root() -> Option<PathBuf> {
    steam_exe().and_then(|p| p.parent().map(Path::to_path_buf))
}

fn steam_libraries() -> Vec<PathBuf> {
    let mut libraries = Vec::new();
    let Some(root) = steam_root() else {
        return libraries;
    };
    libraries.push(root.clone());
    let vdf = root.join("steamapps").join("libraryfolders.vdf");
    if let Ok(text) = fs::read_to_string(vdf) {
        if let Ok(re) = Regex::new(r#""path"\s+"([^"]+)""#) {
            for cap in re.captures_iter(&text) {
                if let Some(path) = cap.get(1) {
                    libraries.push(PathBuf::from(path.as_str().replace(r"\\", r"\")));
                }
            }
        }
    }
    libraries
}

pub fn steamvr_root() -> Option<PathBuf> {
    for library in steam_libraries() {
        let path = library.join("steamapps").join("common").join("SteamVR");
        if path.is_dir() {
            return Some(path);
        }
    }
    None
}

pub fn steamvr_openxr_json() -> Option<PathBuf> {
    let root = steamvr_root()?;
    for rel in [
        PathBuf::from("steamxr_win64.json"),
        PathBuf::from("bin").join("win64").join("openvr_runtime.json"),
        PathBuf::from("bin").join("win64").join("steamxr_win64.json"),
    ] {
        let candidate = root.join(rel);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn oculus_openxr_json() -> Option<PathBuf> {
    let root = oculus_root(None)?;
    let path = root
        .join("Support")
        .join("oculus-runtime")
        .join("oculus_openxr_64.json");
    path.is_file().then_some(path)
}

pub fn client_exe(root: Option<&Path>) -> Option<PathBuf> {
    let root = root.map(Path::to_path_buf).or_else(|| oculus_root(None))?;
    let support = root.join("Support");
    let mut found = vec![
        support.join("oculus-client").join("OculusClient.exe"),
        support.join("oculus-client").join("Client.exe"),
    ];
    if support.is_dir() {
        for entry in WalkDir::new(&support).into_iter().flatten() {
            let name = entry.file_name().to_string_lossy();
            if name.eq_ignore_ascii_case("OculusClient.exe")
                || name.eq_ignore_ascii_case("Client.exe")
            {
                found.push(entry.path().to_path_buf());
            } else if name.eq_ignore_ascii_case("OculusClient.exe.disabled")
                || name.eq_ignore_ascii_case("Client.exe.disabled")
            {
                let live = entry.path().with_file_name(name.trim_end_matches(".disabled"));
                found.push(live);
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    for path in found {
        let key = path.to_string_lossy().to_ascii_lowercase();
        if !seen.insert(key) {
            continue;
        }
        let disabled = PathBuf::from(format!("{}.disabled", path.display()));
        if path.is_file() || disabled.is_file() {
            return Some(path);
        }
    }
    None
}

pub fn hosts_path() -> PathBuf {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    PathBuf::from(root)
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fake_runtime(root: &Path, client_rel: &str) -> PathBuf {
        let runtime = root.join("Support").join("oculus-runtime");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(runtime.join("OVRServer_x64.exe"), b"x").unwrap();
        fs::write(runtime.join("OVRServiceLauncher.exe"), b"x").unwrap();
        let client = root.join(client_rel);
        fs::create_dir_all(client.parent().unwrap()).unwrap();
        fs::write(&client, b"x").unwrap();
        root.to_path_buf()
    }

    #[test]
    fn default_install_folder() {
        assert_eq!(DEFAULT_OCULUS_DIR, r"C:\MetaFixer\Oculus");
    }

    #[test]
    fn finds_preferred_tree() {
        let tmp = tempfile();
        let root = fake_runtime(&tmp.join("Custom").join("Oculus"), "Support/oculus-client/Client.exe");
        let found = oculus_root(Some(&root)).unwrap();
        assert_eq!(found.canonicalize().unwrap(), root.canonicalize().unwrap());
    }

    #[test]
    fn walks_up_from_runtime_folder() {
        let tmp = tempfile();
        let root = fake_runtime(&tmp.join("Oculus"), "Support/oculus-client/Client.exe");
        let runtime = root.join("Support").join("oculus-runtime");
        assert_eq!(
            as_oculus_root(&runtime).unwrap().canonicalize().unwrap(),
            root.canonicalize().unwrap()
        );
        assert_eq!(
            oculus_root(Some(&runtime.join("OVRServer_x64.exe")))
                .unwrap()
                .canonicalize()
                .unwrap(),
            root.canonicalize().unwrap()
        );
    }

    #[test]
    fn client_exe_nested_and_disabled() {
        let tmp = tempfile();
        let root = fake_runtime(
            &tmp.join("Oculus"),
            "Support/oculus-dash/dash/bin/Client.exe",
        );
        let found = client_exe(Some(&root)).unwrap();
        assert_eq!(found.file_name().unwrap(), "Client.exe");
        assert!(found.is_file());

        let tmp2 = tempfile();
        let root = fake_runtime(&tmp2.join("Oculus"), "Support/oculus-client/Client.exe");
        let live = root.join("Support").join("oculus-client").join("Client.exe");
        let disabled = PathBuf::from(format!("{}.disabled", live.display()));
        fs::rename(&live, &disabled).unwrap();
        let found = client_exe(Some(&root)).unwrap();
        assert_eq!(found, live);
        assert!(disabled.is_file());
    }

    fn tempfile() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("metafixer-test-{}", std::process::id()));
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = dir.join(stamp.to_string());
        fs::create_dir_all(&path).unwrap();
        path
    }
}
