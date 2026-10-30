// SPDX-License-Identifier: MIT
//! Blocking, capability-gated MiniFuse control. Planning never opens a device.
//! Execution retains durable backups and complete-state kernel verification.
mod config;
mod error;
mod meters;
mod mixer;
mod model;
mod state;
mod transport;

pub use config::{Snapshot, default_backup_dir};
pub use error::{Error, ErrorKind};
pub use meters::Meters;
pub use model::{MF4_PID, Model, VID};
pub use state::{CHANNEL_NAMES, Inputs, Mix, STATE_SIZE, State, hex};
pub use transport::{Device, Kernel, ModuleCapabilities};

use serde::{Deserialize, Serialize};
use std::{path::Path, str::FromStr};

impl FromStr for Mix {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|m| m.name() == value)
            .ok_or_else(|| "Expected out12, out34 or loopback".into())
    }
}

/// A configured gain, not a measured signal level.
#[derive(Clone, Copy, Debug)]
pub enum Level {
    Db(f64),
    Silence,
}
impl Level {
    fn text(self) -> String {
        match self {
            Self::Db(db) => db.to_string(),
            Self::Silence => "-inf".into(),
        }
    }
}
impl FromStr for Level {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self, Error> {
        if value == "-inf" {
            return Ok(Self::Silence);
        }
        let db: f64 = value
            .parse()
            .map_err(|_| Error::from("Expected a dB value or -inf"))?;
        if !db.is_finite() {
            return Err(Error::from("Expected a finite dB value or -inf"));
        }
        Ok(Self::Db(db))
    }
}

/// MF4 input controls. Phantom power is shared by physical inputs 1 and 2.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputSwitch {
    Inst1,
    Inst2,
    PhantomPower,
}

#[derive(Clone, Debug)]
pub enum Operation {
    InputSwitch {
        target: InputSwitch,
        enabled: bool,
    },
    Channel {
        mix: Mix,
        source: String,
        level: Option<Level>,
        pan: Option<f64>,
        mute: Option<bool>,
        solo: Option<bool>,
    },
    Master {
        mix: Mix,
        level: Level,
    },
    Route {
        mix: Mix,
        source: String,
    },
    Membership {
        mix: Mix,
        source: String,
        included: bool,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct MixCapabilities {
    pub mix: Mix,
    pub sources: Vec<&'static str>,
    pub routes: Vec<&'static str>,
    pub membership: Vec<&'static str>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Capabilities {
    pub mixer: bool,
    pub host_snapshots: bool,
    pub switch_readback: bool,
    pub input_switches: bool,
    pub signal_meters: bool,
    pub control_panel_button: bool,
    pub mixes: Vec<MixCapabilities>,
    pub limitations: Vec<&'static str>,
}
impl Model {
    pub fn capabilities(self) -> Capabilities {
        let mixer = self == Self::MiniFuse4;
        Capabilities {
            mixer,
            host_snapshots: mixer,
            switch_readback: mixer,
            input_switches: mixer,
            signal_meters: mixer,
            control_panel_button: false,
            mixes: if mixer {
                Mix::ALL
                    .into_iter()
                    .map(|mix| {
                        let mut membership = CHANNEL_NAMES[..4].to_vec();
                        if mix != Mix::Out34 {
                            membership.extend(["usb12", "usb34"]);
                        }
                        if mix == Mix::Loopback {
                            membership.push("usb56");
                        }
                        MixCapabilities {
                            mix,
                            sources: CHANNEL_NAMES[..mix.channels()].to_vec(),
                            routes: match mix {
                                Mix::Out12 => vec!["usb12", "mix"],
                                Mix::Out34 => vec!["usb34", "mix"],
                                Mix::Loopback => vec!["usb56", "mix", "out12", "out34"],
                            },
                            membership,
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            },
            limitations: vec![
                if mixer {
                    "Input writes and signal meters require matching module features; logo-button events are unavailable."
                } else {
                    "Switch readback, signal meters and button events are unavailable."
                },
                "Membership is subject to the current mono/stereo topology.",
                "Readback does not confirm nonvolatile saving.",
            ],
        }
    }
}

/// Immutable, validated proposal bound to an exact device record and payload.
#[derive(Clone, Debug)]
pub struct Plan {
    device: Device,
    before: State,
    predicted: State,
    changes: Vec<state::Change>,
    omitted: Vec<usize>,
}
impl Plan {
    fn new(
        device: &Device,
        before: &State,
        changes: Vec<state::Change>,
        saved: Option<&State>,
    ) -> Result<Self, Error> {
        device
            .require_mf4()
            .map_err(|e| Error::new(ErrorKind::UnsupportedCapability, e))?;
        let changes = state::changed(before, changes)?;
        let predicted = state::predict(before, &changes)?;
        let omitted = saved
            .map(|s| {
                (0..STATE_SIZE)
                    .filter(|&i| s.0[i] != predicted.0[i])
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            device: device.clone(),
            before: before.clone(),
            predicted,
            changes,
            omitted,
        })
    }
    pub fn device(&self) -> &Device {
        &self.device
    }
    pub fn before(&self) -> &State {
        &self.before
    }
    pub fn predicted(&self) -> &State {
        &self.predicted
    }
    pub fn write_count(&self) -> usize {
        self.changes.len()
    }
    pub fn requires_input_switches(&self) -> bool {
        self.changes
            .iter()
            .any(|change| matches!(change.selector, 0x0000 | 0x0001 | 0x0300))
    }
    pub fn omitted_offsets(&self) -> &[usize] {
        &self.omitted
    }
    /// Diagnostic compatibility with the existing CLI dry-run format.
    /// This is not an input format or an arbitrary-selector execution API.
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"usb_write_count": self.changes.len(), "changes": self.changes,
            "predicted_after_hex": hex(&self.predicted.0), "unrestored_byte_offsets": self.omitted,
            "note": "Only mapped mixer fields are restorable. Opaque bytes, input trims, INST, 48 V and LED settings are not replayed."})
    }
}

pub fn plan(device: &Device, before: &State, operations: &[Operation]) -> Result<Plan, Error> {
    device
        .require_mf4()
        .map_err(|e| Error::new(ErrorKind::UnsupportedCapability, e))?;
    if operations.len() > state::MAX_CHANGES {
        return Err(Error::from("Too many domain operations"));
    }
    let mut changes = Vec::new();
    for operation in operations {
        changes.extend(match operation {
            Operation::InputSwitch { target, enabled } => vec![state::Change {
                selector: match target {
                    InputSwitch::Inst1 => 0,
                    InputSwitch::Inst2 => 1,
                    InputSwitch::PhantomPower => 0x0300,
                },
                value: u16::from(*enabled),
            }],
            Operation::Channel {
                mix,
                source,
                level,
                pan,
                mute,
                solo,
            } => {
                let text = level.map(Level::text);
                mixer::channel(before, *mix, source, text.as_deref(), *pan, *mute, *solo)?
            }
            Operation::Master { mix, level } => mixer::master(*mix, &level.text())?,
            Operation::Route { mix, source } => mixer::route(*mix, source)?,
            Operation::Membership {
                mix,
                source,
                included,
            } => mixer::source(before, *mix, source, *included)?,
        });
    }
    Plan::new(device, before, changes, None)
}

pub fn plan_restore(
    device: &Device,
    before: &State,
    snapshot: &Snapshot,
    mix: Option<Mix>,
) -> Result<Plan, Error> {
    device
        .require_mf4()
        .map_err(|e| Error::new(ErrorKind::UnsupportedCapability, e))?;
    let saved = snapshot.state()?;
    Plan::new(
        device,
        before,
        mixer::restore(before, &saved, mix)?,
        Some(&saved),
    )
}

/// Implementations execute a validated plan or return explicit recovery facts.
/// The production implementation is Kernel. Test backends never ship as devices.
pub trait Backend {
    fn transact(&mut self, plan: &Plan) -> Result<State, Error>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceOutcome {
    Verified,
    FailedRestored,
    FailedUnrestored,
    Unknown,
    Rejected,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalStatus {
    NotNeeded,
    Finalized,
    Failed,
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub device_outcome: DeviceOutcome,
    pub device_write_attempted: Option<bool>,
    pub rollback_attempted: bool,
    pub rollback_verified: bool,
    pub after_state: Option<State>,
    pub journal_status: JournalStatus,
    pub journal: Option<std::path::PathBuf>,
    pub backup: Option<std::path::PathBuf>,
    pub error: Option<Error>,
    pub journal_error: Option<String>,
}
impl Outcome {
    /// Terminal adapter: a verified write with journal failure remains an error.
    pub fn require_success(self) -> Result<Self, Error> {
        if self.journal_status == JournalStatus::Failed {
            let mut error = Error::new(
                ErrorKind::JournalFailed,
                format!(
                    "Device result: {:?}. Could not finalize journal: {}. Backup: {}",
                    self.device_outcome,
                    self.journal_error.as_deref().unwrap_or("unknown"),
                    self.backup
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                ),
            );
            error.device_write_attempted = self.device_write_attempted;
            error.rollback_attempted = self.rollback_attempted;
            error.rollback_verified = self.rollback_verified;
            error.after_state = self.after_state.map(Box::new);
            return Err(error);
        }
        if let Some(mut error) = self.error {
            if let Some(path) = self.backup {
                error
                    .message
                    .push_str(&format!(" Backup and operation record: {}", path.display()));
            }
            return Err(error);
        }
        Ok(self)
    }
}

pub fn execute<B: Backend>(
    backend: &mut B,
    plan: &Plan,
    directory: &Path,
) -> Result<Outcome, Error> {
    config::execute(backend, plan, directory)
}
