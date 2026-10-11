//! One-shot, opt-in system location. No IP lookup or sample-coordinate fallback.
use super::Coordinate;
use std::sync::mpsc::{self, Receiver};
#[cfg(not(target_os = "android"))]
use std::time::{Duration, Instant};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SystemLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy: f64,
    #[serde(default)]
    pub city: Option<String>,
}

impl SystemLocation {
    pub fn coordinate(&self) -> Result<Coordinate, String> {
        Coordinate::new(self.latitude, self.longitude, self.accuracy)
    }

    /// Only location fields change. Missing city must not leave a stale city behind.
    pub fn apply(
        &self,
        identity: &mut crate::crypto::header::HeaderIdentity,
    ) -> Result<(), String> {
        let coordinate = self.coordinate()?;
        identity.anchor_lat = coordinate.latitude;
        identity.anchor_lon = coordinate.longitude;
        identity.city = self.city.as_deref().unwrap_or("").trim().to_string();
        Ok(())
    }
}

pub fn request() -> Receiver<Result<SystemLocation, String>> {
    let (sender, receiver) = mpsc::channel();
    #[cfg(target_os = "android")]
    crate::android::request_location(sender);
    #[cfg(not(target_os = "android"))]
    std::thread::spawn(move || {
        let _ = sender.send(locate());
    });
    receiver
}

/// Blocking entry point for desktop CLI; GUI always uses `request`.
pub fn locate() -> Result<SystemLocation, String> {
    #[cfg(target_os = "windows")]
    {
        let mut command = std::process::Command::new("powershell.exe");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            include_str!("windows.ps1"),
        ]);
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        run(command)
    }
    #[cfg(target_os = "linux")]
    {
        let mut command = std::process::Command::new("/usr/bin/python3");
        command.args(["-c", include_str!("linux.py")]);
        run(command)
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::PermissionsExt;
        let directory =
            std::env::temp_dir().join(format!("neko-location-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
        let helper = directory.join("location");
        let result = (|| {
            std::fs::write(
                &helper,
                include_bytes!(concat!(env!("OUT_DIR"), "/location-helper")),
            )
            .map_err(|e| e.to_string())?;
            std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
            run(std::process::Command::new(&helper))
        })();
        let _ = std::fs::remove_file(&helper);
        let _ = std::fs::remove_dir(&directory);
        result
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        Err("此平台请使用应用内的系统定位按钮".into())
    }
}

#[cfg(not(target_os = "android"))]
fn run(mut command: std::process::Command) -> Result<SystemLocation, String> {
    use std::io::Read;
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动系统定位服务：{e}"))?;
    // Drain both pipes while waiting so a verbose service cannot deadlock the timeout.
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let read = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    };
    let output = read(Box::new(stdout));
    let errors = read(Box::new(stderr));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < Duration::from_secs(65) => {
                std::thread::sleep(Duration::from_millis(100))
            }
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(error) => format!("系统定位失败：{error}"),
                    _ => "系统定位超时，请检查位置服务和应用权限".into(),
                });
            }
        }
    };
    let output = output.join().map_err(|_| "定位输出读取失败")?;
    let errors = errors.join().map_err(|_| "定位错误读取失败")?;
    if !status.success() {
        return Err(format!(
            "系统定位失败：{}",
            String::from_utf8_lossy(&errors).trim()
        ));
    }
    decode(&String::from_utf8_lossy(&output))
}

pub(crate) fn decode(json: &str) -> Result<SystemLocation, String> {
    let location: SystemLocation = serde_json::from_str(json.trim().trim_start_matches('\u{feff}'))
        .map_err(|e| format!("系统定位返回格式无效：{e}"))?;
    location.coordinate()?;
    Ok(location)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_results_do_not_modify_identity() {
        let mut identity = crate::crypto::header::HeaderIdentity::default();
        identity.device_id = "stable-id".into();
        let original = serde_json::to_value(&identity).unwrap();
        for json in [
            r#"{"latitude":91,"longitude":1,"accuracy":1}"#,
            r#"{"latitude":1,"longitude":1,"accuracy":-1}"#,
            "{}",
        ] {
            assert!(decode(json).is_err());
        }
        let invalid = SystemLocation {
            latitude: f64::NAN,
            longitude: 0.0,
            accuracy: 0.0,
            city: None,
        };
        assert!(invalid.apply(&mut identity).is_err());
        assert_eq!(serde_json::to_value(identity).unwrap(), original);
    }
    #[test]
    fn location_updates_preserve_device_identity_and_clear_stale_city() {
        let mut identity = crate::crypto::header::HeaderIdentity::default();
        identity.device_id = "stable-id".into();
        identity.app_install_time = 123;
        decode(r#"{"latitude":39.9,"longitude":116.4,"accuracy":12}"#)
            .unwrap()
            .apply(&mut identity)
            .unwrap();
        assert_eq!(identity.device_id, "stable-id");
        assert_eq!(identity.app_install_time, 123);
        assert!(identity.city.is_empty());
        assert_eq!(identity.anchor_lat, 39.9);
        identity.city = "大连市".into();
        assert!(!identity.has_unconfigured_default_location(), "actual configured cities must not be rejected by name");
    }
}
