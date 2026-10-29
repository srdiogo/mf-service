// SPDX-License-Identifier: MIT
use crate::State;
use serde::Serialize;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    UnsupportedCapability,
    InvalidInput,
    AmbiguousDevice,
    AccessDenied,
    ModuleUnavailable,
    ModuleIncompatible,
    DeviceUnavailable,
    DeviceChanged,
    StaleState,
    BackupFailed,
    TransferFailed,
    ReadbackFailed,
    JournalFailed,
}

/// Machine-readable failure information, independent of terminal wording.
#[derive(Clone, Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
    /// None means the transport cannot establish whether a write was attempted.
    pub device_write_attempted: Option<bool>,
    pub rollback_attempted: bool,
    pub rollback_verified: bool,
    pub after_state: Option<Box<State>>,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            device_write_attempted: Some(false),
            rollback_attempted: false,
            rollback_verified: false,
            after_state: None,
        }
    }

    pub(crate) fn io(error: std::io::Error, context: &str) -> Self {
        let kind = if error.kind() == std::io::ErrorKind::UnexpectedEof {
            ErrorKind::ModuleIncompatible
        } else {
            match error.raw_os_error() {
                Some(libc::EACCES | libc::EPERM) => ErrorKind::AccessDenied,
                Some(libc::ENOENT) => ErrorKind::ModuleUnavailable,
                Some(libc::ENOTTY | libc::EMSGSIZE) => ErrorKind::ModuleIncompatible,
                Some(libc::EBUSY) => ErrorKind::AmbiguousDevice,
                Some(libc::ENODEV | libc::ENXIO | libc::ESHUTDOWN) => ErrorKind::DeviceUnavailable,
                Some(libc::ESTALE) => ErrorKind::DeviceChanged,
                Some(libc::EAGAIN) => ErrorKind::StaleState,
                Some(libc::EOPNOTSUPP) => ErrorKind::UnsupportedCapability,
                _ => ErrorKind::TransferFailed,
            }
        };
        Self::new(kind, format!("{context}: {error}"))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}

// Internal validators retain their existing wording; their errors occur
// before dispatch and are never used to infer a transport outcome.
impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::new(ErrorKind::InvalidInput, message)
    }
}
impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::from(message.to_owned())
    }
}
