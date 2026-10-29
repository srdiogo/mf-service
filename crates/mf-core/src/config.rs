// SPDX-License-Identifier: MIT
use crate::{
    Backend, Device, DeviceOutcome, Error, ErrorKind, JournalStatus, Outcome, Plan, State, hex,
    model::MF4_PID,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u32,
    pub layout: String,
    pub captured_at_unix: u64,
    pub device: Device,
    pub payload_hex: String,
}

impl Snapshot {
    pub fn new(device: Device, state: &State) -> Self {
        Self {
            schema_version: 1,
            layout: "minifuse4-b2-190-v1".into(),
            captured_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            device,
            payload_hex: hex(&state.0),
        }
    }
    pub fn state(&self) -> Result<State, String> {
        if self.schema_version != 1
            || self.layout != "minifuse4-b2-190-v1"
            || self.device.product_id != MF4_PID
        {
            return Err("Unsupported snapshot schema, model or B2 layout".into());
        }
        self.device.require_mf4()?;
        State::from_hex(&self.payload_hex)
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        // Bounded input: this format contains a 380-character hex payload.
        if fs::metadata(path).map_err(|e| e.to_string())?.len() > 64 * 1024 {
            return Err("Snapshot exceeds 64 KiB".into());
        }
        let snapshot: Self = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        snapshot.state()?;
        Ok(snapshot)
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        save_new(
            path,
            &serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
    }
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}
fn sync_parent(path: &Path) -> Result<(), String> {
    File::open(parent(path))
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}

fn create_backup_dir(path: &Path) -> Result<(), String> {
    let mut missing = Vec::new();
    let mut cursor = path;
    while !cursor.as_os_str().is_empty() && !cursor.exists() {
        missing.push(cursor.to_path_buf());
        cursor = parent(cursor);
    }
    fs::create_dir_all(path).map_err(|e| format!("Cannot create backup directory: {e}"))?;
    // Sync newly created directory entries as well as the eventual files.
    for directory in missing.into_iter().rev() {
        File::open(&directory)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        sync_parent(&directory)?;
    }
    Ok(())
}

pub fn save_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("Cannot create {}: {e}", path.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    sync_parent(path)
}

pub fn default_backup_dir() -> PathBuf {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(state).join("mf-cli");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/state/mf-cli")
}

pub(crate) fn execute<B: Backend>(
    backend: &mut B,
    plan: &Plan,
    directory: &Path,
) -> Result<Outcome, Error> {
    if plan.changes.is_empty() {
        return Ok(Outcome {
            device_outcome: DeviceOutcome::Verified,
            device_write_attempted: Some(false),
            rollback_attempted: false,
            rollback_verified: false,
            after_state: Some(plan.before.clone()),
            journal_status: JournalStatus::NotNeeded,
            journal: None,
            backup: None,
            error: None,
            journal_error: None,
        });
    }
    let prepare = || -> Result<(PathBuf, PathBuf, serde_json::Value, String), String> {
        create_backup_dir(directory)?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let base = format!("mf4-{stamp}-{}", std::process::id());
        let backup = directory.join(format!("{base}.json"));
        Snapshot::new(plan.device.clone(), &plan.before).save(&backup)?;
        let journal_path = directory.join(format!("{base}.operation.json"));
        let journal = serde_json::json!({ "schema_version": 1, "status": "prepared", "backup": backup,
            "device": plan.device, "changes": plan.changes, "expected_after_hex": hex(&plan.predicted.0) });
        save_new(
            &journal_path,
            &serde_json::to_vec_pretty(&journal).map_err(|e| e.to_string())?,
        )?;
        Ok((backup, journal_path, journal, base))
    };
    let (backup, journal_path, mut journal, base) =
        prepare().map_err(|e| Error::new(ErrorKind::BackupFailed, e))?;
    // Snapshot and prepared operation record are durable before dispatch.
    let result = backend.transact(plan).and_then(|after| {
        if after != plan.predicted {
            let mut error = Error::new(
                ErrorKind::ReadbackFailed,
                "Backend returned an unverified state",
            );
            error.device_write_attempted = Some(true);
            Err(error)
        } else {
            Ok(after)
        }
    });
    let (device_outcome, attempted, rollback_attempted, rollback_verified, after) = match &result {
        Ok(after) => (
            DeviceOutcome::Verified,
            Some(true),
            false,
            false,
            Some(after.clone()),
        ),
        Err(error) => {
            let restored =
                error.rollback_verified && error.after_state.as_deref() == Some(&plan.before);
            let outcome = if restored {
                DeviceOutcome::FailedRestored
            } else if error.rollback_attempted {
                DeviceOutcome::FailedUnrestored
            } else if error.device_write_attempted == Some(false) {
                DeviceOutcome::Rejected
            } else {
                DeviceOutcome::Unknown
            };
            (
                outcome,
                error.device_write_attempted,
                error.rollback_attempted,
                restored,
                error.after_state.as_deref().cloned(),
            )
        }
    };
    journal["status"] = if result.is_ok() { "verified" } else { "failed" }.into();
    if let Some(state) = &after {
        journal["after_hex"] = hex(&state.0).into();
    }
    if let Err(error) = &result {
        journal["error"] = error.to_string().into();
    }
    journal["device_outcome"] = serde_json::to_value(device_outcome).expect("enum serialization");
    journal["rollback_attempted"] = rollback_attempted.into();
    journal["rollback_verified"] = rollback_verified.into();
    let temp = directory.join(format!("{base}.operation.pending"));
    let persist = serde_json::to_vec_pretty(&journal)
        .map_err(|e| e.to_string())
        .and_then(|data| save_new(&temp, &data))
        .and_then(|_| fs::rename(&temp, &journal_path).map_err(|e| e.to_string()))
        .and_then(|_| sync_parent(&journal_path));
    Ok(Outcome {
        device_outcome,
        device_write_attempted: attempted,
        rollback_attempted,
        rollback_verified,
        after_state: after,
        journal_status: if persist.is_ok() {
            JournalStatus::Finalized
        } else {
            JournalStatus::Failed
        },
        journal: Some(journal_path),
        backup: Some(backup),
        error: result.err(),
        journal_error: persist.err(),
    })
}

#[cfg(test)]
fn apply<B: Backend>(
    backend: &mut B,
    device: &Device,
    before: &State,
    changes: Vec<crate::state::Change>,
    directory: &Path,
) -> Result<Option<PathBuf>, String> {
    let plan = Plan::new(device, before, changes, None).map_err(|e| e.to_string())?;
    execute(backend, &plan, directory)
        .and_then(Outcome::require_success)
        .map(|outcome| outcome.journal)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Change, predict};
    struct Fake {
        calls: usize,
        fail: bool,
        directory: PathBuf,
    }
    impl Backend for Fake {
        fn transact(&mut self, plan: &Plan) -> Result<State, Error> {
            let s = plan.before();
            let c = &plan.changes;
            self.calls += 1;
            let paths: Vec<_> = fs::read_dir(&self.directory).unwrap().collect();
            assert_eq!(
                paths.len(),
                2,
                "Backup and prepared journal must precede writes"
            );
            if self.fail {
                Err("Simulated USB failure; rollback verified".into())
            } else {
                predict(s, c).map_err(Error::from)
            }
        }
    }
    fn temp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "mf-cli-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&p).unwrap();
        p
    }
    #[test]
    fn snapshot_roundtrip_and_no_overwrite() {
        let dir = temp();
        let path = dir.join("state.json");
        let state = State([0; 190]);
        let snap = Snapshot::new(crate::transport::reference_device(), &state);
        snap.save(&path).unwrap();
        assert!(snap.save(&path).is_err());
        assert_eq!(Snapshot::load(&path).unwrap().state().unwrap(), state);
        let mut bad = snap;
        bad.device.product_id = 0xaf90;
        assert!(bad.state().is_err());
        bad.device.product_id = MF4_PID;
        bad.schema_version = 2;
        assert!(bad.state().is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn journals_success_failure_and_noops() {
        for fail in [false, true] {
            let directory = temp();
            let mut fake = Fake {
                calls: 0,
                fail,
                directory: directory.clone(),
            };
            let state = State([0; 190]);
            let dev = crate::transport::reference_device();
            assert!(
                apply(&mut fake, &dev, &state, vec![], &directory)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(fake.calls, 0);
            let result = apply(
                &mut fake,
                &dev,
                &state,
                vec![Change {
                    selector: 0x0900,
                    value: 0x8000,
                }],
                &directory,
            );
            assert_eq!(result.is_err(), fail);
            assert_eq!(fake.calls, 1);
            let journal = fs::read_dir(&directory)
                .unwrap()
                .map(|p| p.unwrap().path())
                .find(|p| p.to_string_lossy().ends_with("operation.json"))
                .unwrap();
            let data: serde_json::Value =
                serde_json::from_slice(&fs::read(journal).unwrap()).unwrap();
            assert_eq!(data["status"], if fail { "failed" } else { "verified" });
            fs::remove_dir_all(directory).unwrap();
        }
    }
    #[test]
    fn invalid_plan_or_unwritable_backup_never_reaches_backend() {
        let directory = temp();
        let mut fake = Fake {
            calls: 0,
            fail: false,
            directory: directory.clone(),
        };
        let state = State([0; 190]);
        let dev = crate::transport::reference_device();
        assert!(
            apply(
                &mut fake,
                &dev,
                &state,
                vec![Change {
                    selector: 0x5600,
                    value: 1
                }],
                &directory
            )
            .is_err()
        );
        let file = directory.join("file");
        fs::write(&file, b"x").unwrap();
        assert!(
            apply(
                &mut fake,
                &dev,
                &state,
                vec![Change {
                    selector: 0x0900,
                    value: 0x8000
                }],
                &file
            )
            .is_err()
        );
        assert_eq!(fake.calls, 0);
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn wrong_readback_is_never_reported_as_verified() {
        struct Wrong;
        impl Backend for Wrong {
            fn transact(&mut self, plan: &Plan) -> Result<State, Error> {
                Ok(plan.before().clone())
            }
        }
        let dir = temp();
        let result = apply(
            &mut Wrong,
            &crate::transport::reference_device(),
            &State([0; 190]),
            vec![Change {
                selector: 0x0900,
                value: 0x8000,
            }],
            &dir,
        );
        assert!(result.is_err());
        let journal = fs::read_dir(&dir)
            .unwrap()
            .map(|p| p.unwrap().path())
            .find(|p| p.to_string_lossy().ends_with("operation.json"))
            .unwrap();
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(journal).unwrap()).unwrap();
        assert_eq!(record["status"], "failed");
        fs::remove_dir_all(dir).unwrap();
    }
}
