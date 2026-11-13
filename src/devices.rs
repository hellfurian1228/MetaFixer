use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// Meta / Oculus USB vendor ID.
#[allow(dead_code)]
pub const META_VID: u16 = 0x2833;

#[derive(Clone, Debug, Default)]
pub struct UsbDevice {
    pub name: String,
    pub model: String,
    pub status: String,
    pub instance_id: String,
    pub present: bool,
    pub pid: Option<u16>,
}

impl UsbDevice {
    pub fn kind(&self) -> &'static str {
        let n = format!("{} {}", self.name, self.model).to_ascii_lowercase();
        if n.contains("hub") {
            "Hub"
        } else if n.contains("camera") || n.contains("sensor") {
            "Camera"
        } else if n.contains("audio") || n.contains("headset ear") {
            "Audio"
        } else if n.contains("controller") {
            "Controller"
        } else {
            "Headset"
        }
    }

    pub fn is_ok(&self) -> bool {
        self.status.eq_ignore_ascii_case("OK") && self.present
    }

    pub fn is_ghost(&self) -> bool {
        !self.present
    }
}

#[derive(Clone, Debug, Default)]
pub struct ControllerStatus {
    pub name: String,
    pub connected: bool,
    pub raw_state: String,
}

#[derive(Clone, Debug, Default)]
pub struct HeadsetMotion {
    pub model: String,
    pub connection: String,
    pub operational: String,
    pub problems: Vec<String>,
    pub worn: Option<bool>,
    pub camera_frames: Option<i32>,
    pub tracking: String,
    pub log_age: String,
}

#[derive(Deserialize)]
struct CacheFile {
    #[serde(default)]
    devices: Vec<CacheDevice>,
}

#[derive(Deserialize, Default)]
struct CacheDevice {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    subtype: String,
    #[serde(rename = "connectionState", default)]
    connection_state: String,
    #[serde(rename = "operationalState", default)]
    operational_state: String,
    #[serde(default)]
    problems: Value,
    #[serde(default)]
    model: String,
    #[serde(rename = "productName", default)]
    product_name: String,
    #[serde(rename = "hmdType", default)]
    hmd_type: String,
}

fn cache_problems(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                if let Some(s) = item.as_str() {
                    return Some(s.to_string());
                }
                item.get("error").and_then(Value::as_str).map(str::to_string)
            })
            .filter(|s| !s.is_empty())
            .collect(),
        Value::String(s) if !s.is_empty() => vec![s.clone()],
        _ => Vec::new(),
    }
}

pub fn model_from_pid(pid: u16) -> &'static str {
    match pid {
        0x0001 => "Rift DK1",
        0x0021 => "Rift DK2",
        0x0031 | 0x0101 | 0x0211 => "Rift CV1",
        0x0137 => "Quest",
        0x0182 => "Quest",
        0x0183 | 0x1031 => "Quest 2",
        0x0186 | 0x2050 | 0x2051 | 0x2052 => "Rift S",
        0x0201 | 0x3370 => "Quest 3",
        0x3230 | 0x0187 => "Quest Pro",
        0x3371 | 0x3372 => "Quest 3S",
        _ => "Meta VR",
    }
}

pub fn infer_model_from_name(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    if n.contains("quest 3s") || n.contains("quest3s") {
        Some("Quest 3S")
    } else if n.contains("quest 3") || n.contains("quest3") {
        Some("Quest 3")
    } else if n.contains("quest pro") {
        Some("Quest Pro")
    } else if n.contains("quest 2") || n.contains("quest2") {
        Some("Quest 2")
    } else if n.contains("quest") {
        Some("Quest")
    } else if n.contains("rift s") || n.contains("rift-s") {
        Some("Rift S")
    } else if n.contains("cv1") || n.contains("rift cv") {
        Some("Rift CV1")
    } else if n.contains("dk2") {
        Some("Rift DK2")
    } else if n.contains("dk1") || n.contains("developer kit") {
        Some("Rift DK1")
    } else {
        None
    }
}

pub fn parse_vid_pid(instance_id: &str) -> Option<(u16, u16)> {
    let up = instance_id.to_ascii_uppercase();
    let vid = capture_hex(&up, "VID_")?;
    let pid = capture_hex(&up, "PID_")?;
    Some((vid, pid))
}

fn capture_hex(hay: &str, prefix: &str) -> Option<u16> {
    let i = hay.find(prefix)?;
    let rest = hay.get(i + prefix.len()..)?;
    let digits: String = rest.chars().take(4).collect();
    u16::from_str_radix(&digits, 16).ok()
}

pub fn label_usb(name: &str, pid: Option<u16>) -> String {
    if let Some(model) = infer_model_from_name(name) {
        if name.chars().count() < 8 || name.to_ascii_lowercase().contains("composite") {
            return model.to_string();
        }
        return name.trim().to_string();
    }
    if let Some(pid) = pid {
        let model = model_from_pid(pid);
        if model != "Meta VR" {
            if name.is_empty() || name.to_ascii_lowercase().contains("composite") {
                return model.to_string();
            }
            return name.trim().to_string();
        }
    }
    if name.is_empty() {
        "Meta USB device".into()
    } else {
        name.to_string()
    }
}

pub fn detect_headset_model(usb: &[UsbDevice], cache_model: &str) -> String {
    if let Some(m) = infer_model_from_name(cache_model) {
        return m.to_string();
    }
    if !cache_model.is_empty() {
        return cache_model.to_string();
    }
    for dev in usb {
        if let Some(m) = infer_model_from_name(&dev.name) {
            return m.to_string();
        }
        if let Some(pid) = dev.pid {
            let m = model_from_pid(pid);
            if m != "Meta VR" && dev.kind() != "Hub" {
                return m.to_string();
            }
        }
    }
    String::new()
}

pub fn parse_usb_snapshot(json: &str) -> (Vec<UsbDevice>, Vec<UsbDevice>) {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return (Vec::new(), Vec::new());
    };
    let devices = value
        .get("devices")
        .or_else(|| value.get("Devices"))
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    let errors = value
        .get("usbErrors")
        .or_else(|| value.get("UsbErrors"))
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    (parse_usb_array(&devices), parse_usb_array(&errors))
}

fn parse_usb_array(value: &Value) -> Vec<UsbDevice> {
    let arr = match value {
        Value::Array(a) => a.clone(),
        Value::Object(_) => vec![value.clone()],
        _ => Vec::new(),
    };
    arr.into_iter()
        .filter_map(|item| {
            let name = json_str(&item, &["name", "Name", "FriendlyName"]);
            let status = json_str(&item, &["status", "Status"]);
            let id = json_str(&item, &["id", "Id", "InstanceId"]);
            if name.is_empty() && id.is_empty() {
                return None;
            }
            let present = item
                .get("present")
                .or_else(|| item.get("Present"))
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let pid = parse_vid_pid(&id).map(|(_, pid)| pid);
            Some(UsbDevice {
                model: label_usb(&name, pid),
                name: if name.is_empty() {
                    label_usb(&name, pid)
                } else {
                    name
                },
                status: if status.is_empty() {
                    "Unknown".into()
                } else {
                    status
                },
                instance_id: id,
                present,
                pid,
            })
        })
        .collect()
}

fn json_str(value: &Value, keys: &[&str]) -> String {
    for key in keys {
        if let Some(v) = value.get(*key) {
            match v {
                Value::String(s) => return s.trim().to_string(),
                Value::Number(n) => return n.to_string(),
                Value::Bool(b) => return b.to_string(),
                _ => {}
            }
        }
    }
    String::new()
}

pub fn read_device_cache() -> (HeadsetMotion, Vec<ControllerStatus>, String) {
    let mut headset = HeadsetMotion::default();
    let mut controllers = Vec::new();
    let mut cache_model = String::new();
    let Some(base) = std::env::var_os("LOCALAPPDATA") else {
        return (headset, controllers, cache_model);
    };
    let path = PathBuf::from(base).join("Oculus").join("DeviceCache.json");
    let Ok(text) = fs::read_to_string(path) else {
        return (headset, controllers, cache_model);
    };
    let Ok(cache) = serde_json::from_str::<CacheFile>(&text) else {
        return (headset, controllers, cache_model);
    };
    for dev in cache.devices {
        let kind = dev.kind.to_ascii_lowercase();
        if kind.contains("headset") || kind.contains("hmd") {
            cache_model = first_nonempty(&[&dev.product_name, &dev.model, &dev.hmd_type]);
            headset.connection = dev.connection_state;
            headset.operational = dev.operational_state;
            headset.problems = cache_problems(&dev.problems);
        } else if kind.contains("controller") {
            let connected = dev.connection_state.eq_ignore_ascii_case("connected");
            let name = match dev.subtype.to_ascii_lowercase().as_str() {
                "llcon" | "left" => "Left controller".into(),
                "rlcon" | "right" => "Right controller".into(),
                other if !other.is_empty() => format!("Controller ({other})"),
                _ => "Controller".into(),
            };
            controllers.push(ControllerStatus {
                name,
                connected,
                raw_state: dev.connection_state,
            });
        }
    }
    (headset, controllers, cache_model)
}

pub fn read_service_telemetry(headset: &mut HeadsetMotion) {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else {
        return;
    };
    let dir = PathBuf::from(base).join("Oculus");
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let mut logs: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("Service_") && n.ends_with(".txt"))
        })
        .collect();
    logs.sort_by_key(|p| fs::metadata(p).and_then(|m| m.modified()).ok());
    let Some(latest) = logs.pop() else {
        return;
    };
    if let Ok(modified) = fs::metadata(&latest).and_then(|m| m.modified()) {
        if let Ok(elapsed) = modified.elapsed() {
            let secs = elapsed.as_secs();
            headset.log_age = if secs < 60 {
                format!("{secs}s ago")
            } else {
                format!("{}m ago", secs / 60)
            };
        }
    }
    let Ok(text) = fs::read_to_string(latest) else {
        return;
    };
    if let Ok(re) = Regex::new(r"valid_frames=(\d+)") {
        if let Some(cap) = re.captures_iter(&text).last() {
            headset.camera_frames = cap.get(1).and_then(|m| m.as_str().parse().ok());
        }
    }
    if let Ok(re) = Regex::new(r"is_proximity_active=(\d+)") {
        if let Some(cap) = re.captures_iter(&text).last() {
            headset.worn = cap.get(1).map(|m| m.as_str() != "0");
        }
    }
    headset.tracking = match headset.camera_frames {
        Some(n) if n > 0 => "Tracking".into(),
        Some(0) => "Idle".into(),
        _ => "Unknown".into(),
    };
}

fn first_nonempty(parts: &[&str]) -> String {
    parts
        .iter()
        .find(|s| !s.is_empty())
        .map(|s| (*s).to_string())
        .unwrap_or_default()
}

pub const USB_SNAPSHOT_PS: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$devs = @(Get-PnpDevice | Where-Object { $_.InstanceId -like '*VID_2833*' } | ForEach-Object {
  $presentProp = Get-PnpDeviceProperty -InstanceId $_.InstanceId -KeyName 'DEVPKEY_Device_IsPresent'
  $present = $true
  if ($null -ne $presentProp -and $null -ne $presentProp.Data) { $present = [bool]$presentProp.Data }
  [ordered]@{
    name = [string]$_.FriendlyName
    status = [string]$_.Status
    id = [string]$_.InstanceId
    class = [string]$_.Class
    present = $present
  }
})
$errs = @(Get-PnpDevice -Status Error | Where-Object { $_.Class -eq 'USB' -or $_.InstanceId -like '*VID_2833*' } | ForEach-Object {
  [ordered]@{
    name = [string]$_.FriendlyName
    status = [string]$_.Status
    id = [string]$_.InstanceId
    present = $true
  }
})
@{ devices = @($devs); usbErrors = @($errs) } | ConvertTo-Json -Compress -Depth 6
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rift_s_instance_id() {
        let id = r"USB\VID_2833&PID_2051\123";
        assert_eq!(parse_vid_pid(id), Some((0x2833, 0x2051)));
        assert_eq!(model_from_pid(0x2051), "Rift S");
        assert_eq!(model_from_pid(0x0031), "Rift CV1");
        assert_eq!(model_from_pid(0x0183), "Quest 2");
        assert_eq!(model_from_pid(0x3370), "Quest 3");
        assert_eq!(model_from_pid(0x3371), "Quest 3S");
    }

    #[test]
    fn parses_snapshot_json() {
        let json = r#"{"devices":[{"name":"Oculus Rift S","status":"OK","id":"USB\\VID_2833&PID_2051\\A","present":true},{"name":"Rift S USB Hub","status":"Error","id":"USB\\VID_2833&PID_2052\\B","present":false}],"usbErrors":[{"name":"USB Root Hub","status":"Error","id":"USB\\ROOT_HUB30\\1"}]}"#;
        let (devs, errs) = parse_usb_snapshot(json);
        assert_eq!(devs.len(), 2);
        assert!(devs[0].is_ok());
        assert!(devs[1].is_ghost());
        assert_eq!(errs.len(), 1);
        assert_eq!(detect_headset_model(&devs, ""), "Rift S");
    }
}
