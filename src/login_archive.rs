//! Portable login backup; never extract archive-controlled paths.
use crate::api::model::{Config, HeaderIdentity, Session};
use std::io::{Cursor, Read, Write};

pub const LIMIT: u64 = 4 * 1024 * 1024;
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub username: String,
    pub password: String,
    pub remember: bool,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Backup {
    version: u32,
    pub session: Session,
    pub identity: HeaderIdentity,
    pub account: Account,
}
pub fn export(
    session: &Session,
    identity: &HeaderIdentity,
    config: &Config,
    passphrase: &str,
) -> Result<Vec<u8>, String> {
    if passphrase.chars().count() < 12 {
        return Err("归档密码至少需要 12 个字符".into());
    }
    let backup = Backup {
        version: 2,
        session: session.clone(),
        identity: identity.clone(),
        account: Account {
            username: config.username.clone(),
            password: if config.remember {
                config.password.clone()
            } else {
                String::new()
            },
            remember: config.remember,
        },
    };
    validate(&backup)?;
    let json = serde_json::to_vec_pretty(&backup).map_err(|e| e.to_string())?;
    if json.len() as u64 > LIMIT {
        return Err("登录信息过大".into());
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "login.json",
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .with_aes_encryption(zip::AesMode::Aes256, passphrase),
    )
    .map_err(|e| e.to_string())?;
    zip.write_all(&json).map_err(|e| e.to_string())?;
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}
fn validate(b: &Backup) -> Result<(), String> {
    if b.version != 2 {
        return Err("不支持的登录备份版本".into());
    }
    if !b.session.is_logged_in()
        || b.identity.device_id.trim().is_empty()
        || b.session.device_id != b.identity.device_id
    {
        return Err("登录凭证缺失或设备身份不匹配".into());
    }
    Ok(())
}
pub fn decode(bytes: &[u8], passphrase: &str) -> Result<Backup, String> {
    if bytes.len() as u64 > LIMIT {
        return Err("ZIP 超过 4 MB 限制".into());
    }
    let mut zip =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "不是有效的 ZIP 登录备份")?;
    if zip.len() != 1 {
        return Err("登录备份应仅包含 login.json".into());
    }
    let aes = zip
        .get_aes_verification_key_and_salt(0)
        .map_err(|_| "登录归档加密信息损坏")?;
    if !aes.is_some_and(|info| info.aes_mode == zip::AesMode::Aes256) {
        return Err("仅支持 AES-256 加密登录归档，不接受旧版明文 ZIP".into());
    }
    let entry = zip
        .by_index_decrypt(0, passphrase.as_bytes())
        .map_err(|_| "归档密码错误或文件损坏")?;
    if entry.name() != "login.json" || entry.size() > LIMIT {
        return Err("登录备份内容不合法".into());
    }
    let mut json = Vec::new();
    entry
        .take(LIMIT + 1)
        .read_to_end(&mut json)
        .map_err(|_| "读取登录备份失败")?;
    if json.len() as u64 > LIMIT {
        return Err("登录备份内容过大".into());
    }
    let backup = serde_json::from_slice(&json).map_err(|_| "登录备份 JSON 格式错误")?;
    validate(&backup)?;
    Ok(backup)
}
pub fn import(bytes: &[u8], config: &Config, passphrase: &str) -> Result<(Backup, Config), String> {
    let b = decode(bytes, passphrase)?;
    let mut config = config.clone();
    config.username = b.account.username.clone();
    config.remember = b.account.remember;
    config.password = if config.remember {
        b.account.password.clone()
    } else {
        String::new()
    };
    let dir = crate::platform::data_dir();
    let files = [
        ("identity.json", serde_json::to_vec_pretty(&b.identity)),
        ("session.json", serde_json::to_vec_pretty(&b.session)),
        ("config.json", serde_json::to_vec_pretty(&config)),
        // Account-derived caches must not survive an account switch.
        ("points_cache.json", Ok(b"{}".to_vec())),
        ("fence_cache.json", Ok(b"{}".to_vec())),
        ("ai_sports.json", Ok(b"[]".to_vec())),
    ];
    // Snapshot all destinations before writing; restore on an ordinary I/O failure.
    let mut old = Vec::new();
    for (name, _) in &files {
        let path = dir.join(name);
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err("登录文件不是普通文件，未修改任何信息".into());
            }
        }
        let data = match std::fs::read(&path) {
            Ok(data) => Some(data),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("无法备份原登录信息：{e}")),
        };
        old.push((path, data));
    }
    for (i, (_, bytes)) in files.into_iter().enumerate() {
        let bytes = bytes.map_err(|e| e.to_string())?;
        if let Err(e) = write_private(&old[i].0, &bytes) {
            let mut rollback_failed = false;
            for (path, data) in &old[..=i] {
                let restored = match data {
                    Some(data) => write_private(path, data),
                    None => {
                        if path.exists() {
                            std::fs::remove_file(path)
                        } else {
                            Ok(())
                        }
                    }
                };
                rollback_failed |= restored.is_err();
            }
            return Err(format!(
                "保存登录信息失败：{e}{}",
                if rollback_failed {
                    "；恢复原文件失败，请检查存储空间"
                } else {
                    "；已恢复原文件"
                }
            ));
        }
    }
    Ok((b, config))
}

// Restore only fixed application files; no plaintext archive/temp copies are written.
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        if path.exists() {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    const PASS: &str = "synthetic-test-passphrase";
    fn zip_json(name: &str, json: &[u8]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .with_aes_encryption(zip::AesMode::Aes256, PASS),
        )
        .unwrap();
        z.write_all(json).unwrap();
        z.finish().unwrap().into_inner()
    }
    fn zip_json_unencrypted() -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file("login.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"{}").unwrap();
        z.finish().unwrap().into_inner()
    }
    #[test]
    fn hostile_archives_are_rejected() {
        assert!(decode(&zip_json("../session.json", b"{}"), PASS).is_err());
        assert!(decode(&zip_json("login.json", b"not json"), PASS).is_err());
        assert!(decode(
            &zip_json("login.json", &vec![b' '; LIMIT as usize + 1]),
            PASS
        )
        .is_err());
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        z.start_file("login.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"{}").unwrap();
        z.start_file("extra.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        assert!(decode(&z.finish().unwrap().into_inner(), PASS).is_err());
    }
    #[test]
    fn import_preserves_settings_and_invalid_input_preserves_files() {
        use crate::api::model;
        let directory =
            std::env::temp_dir().join(format!("neko-login-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        crate::platform::TEST_DATA_DIR.with(|p| *p.borrow_mut() = Some(directory.clone()));
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                crate::platform::TEST_DATA_DIR.with(|p| *p.borrow_mut() = None);
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(directory.clone());
        let identity = model::load_identity();
        let session = Session {
            uid: 42,
            token: "test-token".into(),
            device_id: identity.device_id.clone(),
            ..Default::default()
        };
        let source = Config {
            username: "new-user".into(),
            password: "not-remembered".into(),
            remember: false,
            ..Default::default()
        };
        let bytes = export(&session, &identity, &source, PASS).unwrap();
        assert!(decode(&bytes, PASS).unwrap().account.password.is_empty());
        let current = Config {
            dist_min: 7.0,
            osm_path: "keep.osm".into(),
            ..Default::default()
        };
        let (_, restored) = import(&bytes, &current, PASS).unwrap();
        assert_eq!(restored.dist_min, 7.0);
        assert_eq!(restored.osm_path, "keep.osm");
        assert_eq!(model::load_session().token, session.token);
        assert_eq!(model::load_config().username, "new-user");
        let saved = std::fs::read(directory.join("session.json")).unwrap();
        assert!(import(b"bad zip", &current, PASS).is_err());
        assert_eq!(
            std::fs::read(directory.join("session.json")).unwrap(),
            saved
        );
        assert!(import(&bytes, &current, "incorrect-passphrase").is_err());
        assert_eq!(
            std::fs::read(directory.join("session.json")).unwrap(),
            saved
        );
        #[cfg(windows)]
        {
            // Force a write failure after identity/session/config were updated.
            let cache = directory.join("points_cache.json");
            let mut permission = std::fs::metadata(&cache).unwrap().permissions();
            permission.set_readonly(true);
            std::fs::set_permissions(&cache, permission).unwrap();
            let changed_session = Session {
                token: "replacement-token".into(),
                ..session.clone()
            };
            let replacement = export(&changed_session, &identity, &source, PASS).unwrap();
            assert!(import(&replacement, &current, PASS).is_err());
            assert_eq!(
                std::fs::read(directory.join("session.json")).unwrap(),
                saved
            );
            let mut permission = std::fs::metadata(&cache).unwrap().permissions();
            permission.set_readonly(false);
            std::fs::set_permissions(&cache, permission).unwrap();
        }
        // A non-file destination is detected before any existing file is modified.
        std::fs::remove_file(directory.join("config.json")).unwrap();
        std::fs::create_dir(directory.join("config.json")).unwrap();
        assert!(import(&bytes, &current, PASS).is_err());
        assert_eq!(
            std::fs::read(directory.join("session.json")).unwrap(),
            saved
        );
        let mut value = serde_json::to_value(decode(&bytes, PASS).unwrap()).unwrap();
        value["version"] = 999.into();
        assert!(decode(
            &zip_json("login.json", &serde_json::to_vec(&value).unwrap()),
            PASS
        )
        .is_err());
    }
    #[test]
    fn roundtrip_and_rejection() {
        let mut identity = HeaderIdentity::default();
        identity.device_id = "backup-test".into();
        let session = Session {
            uid: 42,
            token: "test-token".into(),
            device_id: identity.device_id.clone(),
            ..Default::default()
        };
        let config = Config {
            username: "test".into(),
            password: "secret".into(),
            remember: true,
            ..Default::default()
        };
        let bytes = export(&session, &identity, &config, PASS).unwrap();
        let b = decode(&bytes, PASS).unwrap();
        assert_eq!(b.account.password, "secret");
        assert_eq!(b.session.token, session.token);
        assert!(decode(&bytes, "wrong-passphrase").is_err());
        assert!(zip::ZipArchive::new(Cursor::new(&bytes))
            .unwrap()
            .by_index(0)
            .is_err());
        assert!(export(&session, &identity, &config, "short").is_err());
        assert!(decode(&zip_json_unencrypted(), PASS).is_err());
        let mut damaged = bytes.clone();
        let start = zip::ZipArchive::new(Cursor::new(&bytes))
            .unwrap()
            .by_index_raw(0)
            .unwrap()
            .data_start() as usize;
        damaged[start + 20] ^= 1;
        assert!(decode(&damaged, PASS).is_err());
        assert!(decode(b"not zip", PASS).is_err());
        assert!(decode(&bytes[..bytes.len() / 2], PASS).is_err());
        identity.device_id = "other".into();
        assert!(export(&session, &identity, &config, PASS).is_err());
    }
}
