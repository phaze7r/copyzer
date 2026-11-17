use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::env;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(target_os = "windows")]
const REQUIRED_TOOLS: &[&str] = &["idevice_id", "ideviceinfo", "idevicepair", "afcclient"];
#[cfg(not(target_os = "windows"))]
const REQUIRED_TOOLS: &[&str] = &["idevice_id", "ideviceinfo", "idevicepair"];

#[cfg(target_os = "linux")]
const DOCTOR_TOOLS: &[&str] = &[
    "idevice_id",
    "ideviceinfo",
    "idevicepair",
    "ifuse",
    "afcclient",
    "fusermount3",
];
#[cfg(target_os = "macos")]
const DOCTOR_TOOLS: &[&str] = &[
    "idevice_id",
    "ideviceinfo",
    "idevicepair",
    "ifuse",
    "afcclient",
    "umount",
];
#[cfg(target_os = "windows")]
const DOCTOR_TOOLS: &[&str] = &["idevice_id", "ideviceinfo", "idevicepair", "afcclient"];
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
const DOCTOR_TOOLS: &[&str] = &["idevice_id", "ideviceinfo", "idevicepair", "ifuse", "afcclient"];

const UNTRUSTED_MESSAGE: &str = "Unlock your iPhone and tap Trust";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeviceInfo {
    pub udid: String,
    pub name: String,
    pub product_type: String,
    pub ios_version: String,
}

impl DeviceInfo {
    /// Maps Apple `ProductType` identifiers (e.g. `iPhone17,1`) to human-readable model names.
    pub fn model_name(&self) -> Option<&'static str> {
        Some(match self.product_type.as_str() {
            "iPhone10,1" | "iPhone10,4" => "iPhone 8",
            "iPhone10,2" | "iPhone10,5" => "iPhone 8 Plus",
            "iPhone10,3" | "iPhone10,6" => "iPhone X",
            "iPhone11,2" => "iPhone XS",
            "iPhone11,4" | "iPhone11,6" => "iPhone XS Max",
            "iPhone11,8" => "iPhone XR",
            "iPhone12,1" => "iPhone 11",
            "iPhone12,3" => "iPhone 11 Pro",
            "iPhone12,5" => "iPhone 11 Pro Max",
            "iPhone13,1" => "iPhone 12 mini",
            "iPhone13,2" => "iPhone 12",
            "iPhone13,3" => "iPhone 12 Pro",
            "iPhone13,4" => "iPhone 12 Pro Max",
            "iPhone14,4" => "iPhone 13 mini",
            "iPhone14,5" => "iPhone 13",
            "iPhone14,2" => "iPhone 13 Pro",
            "iPhone14,3" => "iPhone 13 Pro Max",
            "iPhone14,7" => "iPhone 14",
            "iPhone14,8" => "iPhone 14 Plus",
            "iPhone15,2" => "iPhone 14 Pro",
            "iPhone15,3" => "iPhone 14 Pro Max",
            "iPhone15,4" => "iPhone 15",
            "iPhone15,5" => "iPhone 15 Plus",
            "iPhone16,1" => "iPhone 15 Pro",
            "iPhone16,2" => "iPhone 15 Pro Max",
            "iPhone17,1" => "iPhone 16 Pro",
            "iPhone17,2" => "iPhone 16 Pro Max",
            "iPhone17,3" => "iPhone 16",
            "iPhone17,4" => "iPhone 16 Plus",
            "iPhone17,5" => "iPhone 16e",
            "iPhone18,1" => "iPhone 17 Pro",
            "iPhone18,2" => "iPhone 17 Pro Max",
            "iPhone18,3" => "iPhone 17",
            "iPhone18,4" => "iPhone 17 Air",
            _ => return None,
        })
    }

    /// Returns a display label combining the custom iPhone name (including Pashto/Unicode names)
    /// and the hardware model name (e.g. `پښتو · iPhone 16 Pro` or `iPhone 16 Pro`).
    pub fn display_label(&self) -> String {
        let trimmed_name = self.name.trim();
        match self.model_name() {
            Some(model) => {
                if !trimmed_name.is_empty()
                    && !trimmed_name.eq_ignore_ascii_case("iPhone")
                    && !trimmed_name.eq_ignore_ascii_case(model)
                {
                    format!("{trimmed_name} · {model}")
                } else {
                    model.to_string()
                }
            }
            None => {
                if !trimmed_name.is_empty() {
                    trimmed_name.to_string()
                } else {
                    "iPhone".to_string()
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn resolve_notification_icon() -> String {
    const EMBEDDED_SVG: &str = include_str!("../pkgbuild/copyzer.svg");
    let base_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(env::temp_dir);
    let runtime_icon = base_dir.join("copyzer-logo.svg");
    if std::fs::write(&runtime_icon, EMBEDDED_SVG).is_ok() {
        return runtime_icon.to_string_lossy().into_owned();
    }
    let sys_icon = Path::new("/usr/share/icons/hicolor/scalable/apps/copyzer.svg");
    if sys_icon.is_file() {
        return sys_icon.to_string_lossy().into_owned();
    }
    "copyzer".to_string()
}

/// Sends a non-blocking desktop notification (via `notify-send` on Linux or `osascript` on macOS).
pub fn notify_desktop(summary: &str, body: &str) {
    let summary = summary.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        {
            if tool_in_path("osascript") {
                let script = format!(
                    "display notification {:?} with title {:?}",
                    body, summary
                );
                let _ = Command::new("osascript")
                    .arg("-e")
                    .arg(script)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            if tool_in_path("notify-send") {
                let icon = resolve_notification_icon();
                let _ = Command::new("notify-send")
                    .args([
                        "-a",
                        "Copyzer",
                        "-i",
                        &icon,
                        "-h",
                        "string:desktop-entry:copyzer",
                        &summary,
                        &body,
                    ])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    });
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum DeviceStatus {
    Connected(DeviceInfo),
    Untrusted { udid: String, message: String },
    NoDevice,
    MissingTools(Vec<String>),
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub tools: Vec<(String, bool)>,
    pub status: DeviceStatus,
    pub usb_a_fastcharge_warning: bool,
}

/// RAII guard for an `ifuse` mount directory or an `afcclient` staged directory.
/// Unmounts (when FUSE-mounted) and removes the temporary directory on drop when owned.
pub struct MountGuard {
    path: PathBuf,
    udid: Option<String>,
    owns_mount: bool,
    is_staged_dir: bool,
}

impl MountGuard {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn udid(&self) -> Option<&str> {
        self.udid.as_deref()
    }

    /// Wraps an already-mounted directory (e.g. `--mount-dir`) without unmounting on drop.
    pub fn from_existing(path: PathBuf) -> Self {
        Self {
            path,
            udid: None,
            owns_mount: false,
            is_staged_dir: false,
        }
    }
}

impl Drop for MountGuard {
    fn drop(&mut self) {
        if !self.owns_mount {
            return;
        }

        if self.is_staged_dir {
            let _ = std::fs::remove_dir_all(&self.path);
            return;
        }

        let unmounted = Command::new("fusermount3")
            .arg("-u")
            .arg(&self.path)
            .status()
            .is_ok_and(|s| s.success())
            || Command::new("fusermount3")
                .arg("-uz")
                .arg(&self.path)
                .status()
                .is_ok_and(|s| s.success())
            || Command::new("umount")
                .arg(&self.path)
                .status()
                .is_ok_and(|s| s.success())
            || Command::new("diskutil")
                .args(["unmount", "force"])
                .arg(&self.path)
                .status()
                .is_ok_and(|s| s.success());

        if unmounted {
            let _ = std::fs::remove_dir(&self.path);
        }
    }
}

fn tool_in_path(tool: &str) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };

    #[cfg(target_os = "windows")]
    let candidates = [format!("{tool}.exe"), tool.to_string()];
    #[cfg(not(target_os = "windows"))]
    let candidates = [tool.to_string()];

    env::split_paths(&path_var).any(|dir| {
        candidates.iter().any(|name| {
            let candidate = dir.join(name);
            #[cfg(unix)]
            {
                candidate
                    .metadata()
                    .is_ok_and(|meta| meta.is_file() && (meta.permissions().mode() & 0o111 != 0))
            }
            #[cfg(not(unix))]
            {
                candidate.metadata().is_ok_and(|meta| meta.is_file())
            }
        })
    })
}

pub fn doctor_report() -> DoctorReport {
    let tools = DOCTOR_TOOLS
        .iter()
        .map(|&tool| (tool.to_string(), tool_in_path(tool)))
        .collect();
    let status = detect_device();
    let usb_a_fastcharge_warning = is_apple_fastcharge_loaded();

    DoctorReport {
        tools,
        status,
        usb_a_fastcharge_warning,
    }
}

pub fn is_apple_fastcharge_loaded() -> bool {
    #[cfg(target_os = "linux")]
    {
        Path::new("/sys/module/apple_mfi_fastcharge").exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

fn retry_with_backoff<T, F>(attempts: usize, delays_ms: &[u64], mut op: F) -> Option<T>
where
    F: FnMut() -> Option<T>,
{
    for i in 0..attempts {
        if let Some(val) = op() {
            return Some(val);
        }
        if i + 1 < attempts {
            let delay = delays_ms.get(i).copied().unwrap_or(300);
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }
    }
    None
}

pub fn detect_device() -> DeviceStatus {
    let mut missing: Vec<String> = REQUIRED_TOOLS
        .iter()
        .filter(|&&tool| !tool_in_path(tool))
        .map(|&tool| tool.to_string())
        .collect();

    #[cfg(not(target_os = "windows"))]
    if !tool_in_path("ifuse") && !tool_in_path("afcclient") {
        missing.push("ifuse (or afcclient)".to_string());
    }

    if !missing.is_empty() {
        return DeviceStatus::MissingTools(missing);
    }

    // Retry UDID detection with short backoff for USB-A / USB 2.0 enumeration settle.
    let Some(udid) = retry_with_backoff(3, &[200, 400], || {
        let out = Command::new("idevice_id").arg("-l").output().ok()?;
        if !out.status.success() {
            return None;
        }
        parse_udid_list(&String::from_utf8_lossy(&out.stdout))
    }) else {
        return DeviceStatus::NoDevice;
    };

    // Fast path: query ideviceinfo with retry for USB-A / Lightning handshake settle.
    if let Some(status) = retry_with_backoff(3, &[200, 400], || {
        let info_out = Command::new("ideviceinfo").args(["-u", &udid]).output().ok()?;
        let stdout = String::from_utf8_lossy(&info_out.stdout);
        let stderr = String::from_utf8_lossy(&info_out.stderr);
        let combined = format!("{stdout}\n{stderr}");

        if info_out.status.success() && stdout.contains("ProductVersion:") {
            let info = parse_ideviceinfo(&stdout, &udid);
            return Some(DeviceStatus::Connected(info));
        }
        if is_untrusted_output(&combined) {
            return Some(DeviceStatus::Untrusted {
                udid: udid.clone(),
                message: UNTRUSTED_MESSAGE.to_string(),
            });
        }
        None
    }) {
        return status;
    }

    // Fallback: explicit pair validation to distinguish untrusted vs disconnected.
    let Ok(pair_out) = Command::new("idevicepair")
        .args(["validate", "-u", &udid])
        .output()
    else {
        return DeviceStatus::NoDevice;
    };

    let pair_combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&pair_out.stdout),
        String::from_utf8_lossy(&pair_out.stderr)
    );
    if !pair_out.status.success() || is_untrusted_output(&pair_combined) {
        return DeviceStatus::Untrusted {
            udid,
            message: UNTRUSTED_MESSAGE.to_string(),
        };
    }

    DeviceStatus::NoDevice
}

pub fn mount_iphone(udid: &str) -> Result<MountGuard> {
    cleanup_stale_mounts();

    let base_dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(env::temp_dir);

    let dir = tempfile::Builder::new()
        .prefix("copyzer-")
        .tempdir_in(&base_dir)
        .or_else(|_| tempfile::Builder::new().prefix("copyzer-").tempdir())
        .context("failed to create temporary mount directory")?
        .keep();

    #[cfg(unix)]
    {
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }

    if tool_in_path("ifuse") {
        let mut last_err = None;
        for attempt in 0..3 {
            match Command::new("ifuse").arg("-u").arg(udid).arg(&dir).output() {
                Ok(output) if output.status.success() => {
                    return Ok(MountGuard {
                        path: dir,
                        udid: Some(udid.to_string()),
                        owns_mount: true,
                        is_staged_dir: false,
                    });
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    let detail = if stderr.is_empty() {
                        format!("exit status {}", output.status)
                    } else {
                        stderr
                    };
                    last_err = Some(anyhow::anyhow!("ifuse failed to mount device {udid}: {detail}"));
                }
                Err(err) => {
                    last_err = Some(anyhow::Error::new(err).context("failed to execute ifuse"));
                }
            }
            if attempt < 2 {
                std::thread::sleep(std::time::Duration::from_millis(300 * (attempt as u64 + 1)));
            }
        }
        // If afcclient is not available, return the ifuse error immediately.
        if !tool_in_path("afcclient") {
            let _ = std::fs::remove_dir(&dir);
            return Err(last_err.unwrap_or_else(|| anyhow::anyhow!("ifuse failed to mount device {udid}")));
        }
    }

    if tool_in_path("afcclient") {
        let output = match Command::new("afcclient")
            .args(["-u", udid, "get", "-rf", "DCIM"])
            .arg(&dir)
            .output()
        {
            Ok(out) => out,
            Err(err) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Err(err).context("failed to execute afcclient");
            }
        };

        if !output.status.success() {
            let _ = std::fs::remove_dir_all(&dir);
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let detail = if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            };
            bail!("afcclient failed to access device {udid}: {detail}");
        }

        let photo_dir = dir.join("PhotoData");
        let _ = std::fs::create_dir_all(&photo_dir);
        for name in ["Photos.sqlite", "Photos.sqlite-wal", "Photos.sqlite-shm"] {
            let remote = format!("PhotoData/{name}");
            let _ = Command::new("afcclient")
                .args(["-u", udid, "get", &remote])
                .arg(photo_dir.join(name))
                .output();
        }

        return Ok(MountGuard {
            path: dir,
            udid: Some(udid.to_string()),
            owns_mount: true,
            is_staged_dir: true,
        });
    }

    let _ = std::fs::remove_dir(&dir);
    bail!("neither ifuse nor afcclient is available in PATH")
}

/// Cleans up orphaned `copyzer-*` mount directories left behind by killed sessions.
fn cleanup_stale_mounts() {
    let mut parents = vec![env::temp_dir()];
    if let Some(xdg) = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        if xdg.is_dir() && !parents.contains(&xdg) {
            parents.push(xdg);
        }
    }

    for parent in parents {
        let Ok(entries) = std::fs::read_dir(&parent) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name_str) = name.to_str() else {
                continue;
            };
            if !name_str.starts_with("copyzer-") {
                continue;
            }
            let path = entry.path();
            if !is_directory(&path) {
                continue;
            }
            match std::fs::read_dir(&path) {
                Err(_) => {
                    // Stale/disconnected FUSE mount (e.g. ENOTCONN / EIO)
                    let _ = Command::new("fusermount3").arg("-uz").arg(&path).status();
                    let _ = Command::new("umount").arg(&path).status();
                    let _ = std::fs::remove_dir(&path);
                }
                Ok(mut rd) => {
                    if rd.next().is_none() {
                        let _ = std::fs::remove_dir(&path);
                    }
                }
            }
        }
    }
}

fn is_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

pub fn parse_udid_list(raw: &str) -> Option<String> {
    raw.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

pub fn is_untrusted_output(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    [
        "trust",
        "passcode",
        "not paired",
        "unpaired",
        "pair again",
        "could not validate",
        "locked",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

pub fn parse_ideviceinfo(raw: &str, udid: &str) -> DeviceInfo {
    let mut name = None;
    let mut product_type = None;
    let mut ios_version = None;

    for line in raw.lines() {
        let Some((key, val)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim();
        if val.is_empty() {
            continue;
        }
        match key {
            "DeviceName" => name = Some(val.to_string()),
            "ProductType" => product_type = Some(val.to_string()),
            "ProductVersion" => ios_version = Some(val.to_string()),
            _ => {}
        }
    }

    DeviceInfo {
        udid: udid.to_string(),
        name: name.unwrap_or_else(|| "iPhone".to_string()),
        product_type: product_type.unwrap_or_else(|| "Unknown".to_string()),
        ios_version: ios_version.unwrap_or_else(|| "Unknown".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ideviceinfo_extracts_fields_with_fallbacks() {
        let sample = "\
DeviceName: User iPhone 16 Pro Max
ProductType: iPhone17,2
ProductVersion: 18.1.1
SerialNumber: ABC123
";
        let info = parse_ideviceinfo(sample, "00008140-TEST");
        assert_eq!(
            info,
            DeviceInfo {
                udid: "00008140-TEST".to_string(),
                name: "User iPhone 16 Pro Max".to_string(),
                product_type: "iPhone17,2".to_string(),
                ios_version: "18.1.1".to_string(),
            }
        );
        assert_eq!(
            info.display_label(),
            "User iPhone 16 Pro Max · iPhone 16 Pro Max"
        );

        let partial = "DeviceName:\n";
        let fallback = parse_ideviceinfo(partial, "UDID-FALLBACK");
        assert_eq!(
            fallback,
            DeviceInfo {
                udid: "UDID-FALLBACK".to_string(),
                name: "iPhone".to_string(),
                product_type: "Unknown".to_string(),
                ios_version: "Unknown".to_string(),
            }
        );
    }

    #[test]
    fn parse_udid_list_returns_first_non_empty_line() {
        assert_eq!(parse_udid_list(""), None);
        assert_eq!(parse_udid_list("   \n\n  \t "), None);
        assert_eq!(
            parse_udid_list("\n  00008140-0011223344556677  \nsecond-udid\n"),
            Some("00008140-0011223344556677".to_string())
        );
    }

    #[test]
    fn is_untrusted_output_detects_trust_and_passcode_errors() {
        assert!(is_untrusted_output(
            "ERROR: Please accept the trust dialog on the screen of device"
        ));
        assert!(is_untrusted_output(
            "ERROR: Could not validate with device because a passcode is set."
        ));
        assert!(is_untrusted_output(
            "ERROR: Device is not paired with this host"
        ));
        assert!(!is_untrusted_output(
            "SUCCESS: Validated pairing with device"
        ));
    }

    #[test]
    fn mount_guard_from_existing_preserves_directory_on_drop() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().to_path_buf();
        let sentinel = path.join("DCIM");
        std::fs::create_dir(&sentinel).unwrap();

        {
            let guard = MountGuard::from_existing(path.clone());
            assert_eq!(guard.path(), path.as_path());
            assert!(!guard.owns_mount);
        }

        assert!(path.exists());
        assert!(sentinel.exists());
    }

    #[test]
    fn mount_guard_carries_optional_udid_and_models_include_iphone_17() {
        let temp = tempfile::tempdir().unwrap();
        let guard = MountGuard::from_existing(temp.path().to_path_buf());
        assert_eq!(guard.udid(), None);

        let info = DeviceInfo {
            udid: "00008150-TEST".to_string(),
            name: "پښتو".to_string(),
            product_type: "iPhone18,1".to_string(),
            ios_version: "27.0.1".to_string(),
        };
        assert_eq!(info.display_label(), "پښتو · iPhone 17 Pro");
    }

    #[test]
    fn stale_mount_cleanup_only_accepts_directories() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("copyzer-mount");
        let file = temp.path().join("copyzer-logo.svg");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(&file, "<svg />").unwrap();

        assert!(is_directory(&directory));
        assert!(!is_directory(&file));
    }

    #[test]
    fn retry_with_backoff_retries_until_some() {
        let mut attempts = 0usize;
        let res = retry_with_backoff(3, &[1, 1], || {
            attempts += 1;
            if attempts < 3 {
                None
            } else {
                Some("connected")
            }
        });
        assert_eq!(res, Some("connected"));
        assert_eq!(attempts, 3);
    }
}
