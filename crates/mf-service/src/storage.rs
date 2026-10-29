// SPDX-License-Identifier: MIT
use crate::protocol::{ApiError, Result};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

pub fn token() -> String {
    let mut bytes = [0; 24];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .expect("Linux random source unavailable");
    mf_core::hex(&bytes)
}
pub fn safe_id(id: &str) -> Result<()> {
    if id.len() != 48
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(ApiError::invalid("Invalid opaque identifier"));
    }
    Ok(())
}
pub fn directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.permissions().mode() & 0o077 != 0
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Private directory must be owned by the current user with mode 0700",
        ));
    }
    Ok(())
}
pub fn write_new(path: &Path, value: &Value) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    File::open(path.parent().expect("Owned file has parent"))?.sync_all()
}
pub fn update(path: &Path, value: &Value) -> std::io::Result<()> {
    let temp = path.with_extension(format!("{}.pending", token()));
    write_new(&temp, value)?;
    fs::rename(temp, path)?;
    File::open(path.parent().expect("Owned file has parent"))?.sync_all()
}
pub fn read(path: &Path) -> std::io::Result<Value> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    if file.metadata()?.len() > 256 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Stored record is too large",
        ));
    }
    let mut bytes = Vec::new();
    file.take(256 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 256 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Stored record is too large",
        ));
    }
    serde_json::from_slice(&bytes).map_err(std::io::Error::other)
}

pub struct Paths {
    pub metadata: PathBuf,
    pub presets: PathBuf,
    pub backups: PathBuf,
}
impl Paths {
    pub fn environment() -> std::io::Result<Self> {
        let home = std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| std::io::Error::other("HOME must be set"))?;
        let xdg = |key: &str, fallback: &str| {
            std::env::var_os(key)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(fallback))
        };
        let paths = Self {
            metadata: xdg("XDG_STATE_HOME", ".local/state").join("mf-service"),
            presets: xdg("XDG_DATA_HOME", ".local/share").join("minifuse/presets"),
            backups: mf_core::default_backup_dir(),
        };
        paths.prepare()?;
        Ok(paths)
    }
    pub fn prepare(&self) -> std::io::Result<()> {
        directory(&self.metadata)?;
        directory(&self.metadata.join("clients"))?;
        directory(&self.metadata.join("operations"))?;
        directory(&self.presets)
    }
}
