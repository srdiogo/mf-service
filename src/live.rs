// SPDX-License-Identifier: MIT
//! Continuous typed controls reuse the core's full pre-write evidence and guard.
use super::*;
use mf_core::{DeviceOutcome, JournalStatus, Level, Mix, Operation};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Target {
    ChannelLevel { mix: Mix, source: String },
    ChannelPan { mix: Mix, source: String },
    MasterLevel { mix: Mix },
}
impl Target {
    fn operation(&self, value: &Value) -> Result<Operation> {
        let level = || match value {
            Value::Null => Ok(Level::Silence),
            Value::Number(n) => n
                .as_f64()
                .map(Level::Db)
                .ok_or_else(|| ApiError::invalid("Invalid level")),
            _ => Err(ApiError::invalid(
                "Level requires a number or null for silence",
            )),
        };
        match self {
            Self::MasterLevel { mix } => Ok(Operation::Master {
                mix: *mix,
                level: level()?,
            }),
            Self::ChannelLevel { mix, source } => Ok(Operation::Channel {
                mix: *mix,
                source: source.clone(),
                level: Some(level()?),
                pan: None,
                mute: None,
                solo: None,
            }),
            Self::ChannelPan { mix, source } => Ok(Operation::Channel {
                mix: *mix,
                source: source.clone(),
                level: None,
                pan: Some(
                    value
                        .as_f64()
                        .ok_or_else(|| ApiError::invalid("Pan requires a number"))?,
                ),
                mute: None,
                solo: None,
            }),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Begin {
    device_id: String,
    generation: String,
    expected_revision: String,
    target: Target,
    idempotency_key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Update {
    operation_id: String,
    sequence: u64,
    #[serde(deserialize_with = "required_value")]
    value: Value,
    finish: bool,
}
fn required_value<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Value, D::Error> {
    Value::deserialize(d)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cancel {
    operation_id: String,
}

pub(super) struct Control {
    pub connection: u64,
    owner: String,
    id: String,
    device: String,
    generation: String,
    revision: String,
    target: Target,
    directory: PathBuf,
    pub touched: Instant,
}
pub(super) struct Sample {
    plan: Plan,
    sequence: u64,
    finish: bool,
}
impl<H: Hardware> Coordinator<H> {
    pub(super) fn begin_control(
        &mut self,
        connection: u64,
        owner: &str,
        value: Value,
    ) -> Result<Value> {
        let p: Begin = protocol::params(value.clone())?;
        protocol::identifier(&p.idempotency_key)?;
        if let Some(record) = self
            .operations
            .values()
            .find(|v| v["client_token"] == owner && v["idempotency_key"] == p.idempotency_key)
        {
            if record["begin"] != value {
                return Err(ApiError::new(
                    "idempotency_conflict",
                    "Key belongs to a different control gesture",
                ));
            }
            return Ok(public_operation(record));
        }
        if self.live.is_some() || !self.work.is_empty() {
            return Err(ApiError::new(
                "control_busy",
                "Another operation owns the interface",
            ));
        }
        if self.operations.len() >= MAX_OPERATIONS {
            return Err(ApiError::new("queue_full", "Operation store is full"));
        }
        let i = self.identity(&p.device_id, &p.generation)?;
        self.revision(i, &p.expected_revision)?;
        // Validate the fixed target through the core before a fresh hardware read.
        mf_core::plan(
            &self.devices[i].descriptor.device,
            self.devices[i].state.as_ref().unwrap(),
            &[p.target.operation(&json!(0))?],
        )?;
        self.refresh()?;
        let i = self.identity(&p.device_id, &p.generation)?;
        self.revision(i, &p.expected_revision)?;
        let id = storage::token();
        let directory = self.paths.backups.join(format!("control-{id}"));
        storage::directory(&directory)
            .map_err(|e| ApiError::new("backup_failed", e.to_string()))?;
        let baseline = serde_json::to_value(mf_core::Snapshot::new(
            self.devices[i].descriptor.device.clone(),
            self.devices[i].state.as_ref().unwrap(),
        ))
        .unwrap();
        storage::write_new(&directory.join("initial.json"), &baseline)
            .map_err(|e| ApiError::new("backup_failed", e.to_string()))?;
        let record = json!({"schema_version":1,"kind":"live_control","operation_id":id,"client_token":owner,"idempotency_key":p.idempotency_key,"plan_id":id,"epoch":self.epoch,"device_id":p.device_id,"generation":p.generation,"status":"pending","begin":value,"samples":[],"backup_group":directory,"device_write_attempted":false});
        storage::write_new(&self.operation_path(&id), &record)
            .map_err(|e| ApiError::new("backup_failed", e.to_string()))?;
        self.operations.insert(id.clone(), record.clone());
        self.live = Some(Control {
            connection,
            owner: owner.into(),
            id: id.clone(),
            device: p.device_id.clone(),
            generation: p.generation,
            revision: p.expected_revision,
            target: p.target,
            directory,
            touched: Instant::now(),
        });
        self.emit(
            "operation_started",
            Some(&p.device_id),
            json!({"operation_id":id,"device_id":p.device_id}),
        );
        Ok(public_operation(&record))
    }
    pub(super) fn update_control(
        &mut self,
        connection: u64,
        owner: &str,
        value: Value,
    ) -> Result<Value> {
        let p: Update = protocol::params(value)?;
        let record = self
            .operations
            .get(&p.operation_id)
            .filter(|v| v["client_token"] == owner && v["kind"] == "live_control")
            .ok_or_else(|| ApiError::new("outcome_unknown", "Unknown control gesture"))?;
        let samples = record["samples"].as_array().unwrap();
        let intent = json!({"sequence":p.sequence,"value":p.value,"finish":p.finish});
        let receipt = json!({"operation_id":p.operation_id,"sequence":p.sequence,"accepted":true});
        if let Some(sample) = samples.iter().find(|s| s["sequence"] == p.sequence) {
            if sample != &intent {
                return Err(ApiError::new(
                    "idempotency_conflict",
                    "Sequence was already used for a different value",
                ));
            }
            return Ok(receipt);
        }
        if samples.len() >= 512 || p.sequence != samples.len() as u64 + 1 {
            return Err(ApiError::invalid(
                "Samples require contiguous sequences 1..512",
            ));
        }
        let c = self
            .live
            .as_ref()
            .filter(|c| c.id == p.operation_id && c.connection == connection && c.owner == owner)
            .ok_or_else(|| {
                ApiError::new("control_closed", "Gesture is closed; refresh current state")
            })?;
        if c.touched.elapsed() >= Duration::from_secs(30) {
            self.end_control("expired");
            return Err(ApiError::new("control_closed", "Gesture lease expired"));
        }
        if self.live_work.is_some() {
            return Err(ApiError::new(
                "control_busy",
                "Wait for the current sample to settle",
            ));
        }
        let i = self.identity(&c.device, &c.generation)?;
        let plan = mf_core::plan(
            &self.devices[i].descriptor.device,
            self.devices[i].state.as_ref().unwrap(),
            &[c.target.operation(&p.value)?],
        )?;
        let identity = (c.device.clone(), c.generation.clone(), c.revision.clone());
        let ready = self.refresh().and_then(|()| {
            let i = self.identity(&identity.0, &identity.1)?;
            self.revision(i, &identity.2)
        });
        if let Err(e) = ready {
            self.end_control("conflict");
            return Err(e);
        }
        if self.live.is_none() {
            return Err(ApiError::new(
                "control_closed",
                "Gesture lease expired during refresh",
            ));
        }
        let mut record = self.operations[&p.operation_id].clone();
        record["samples"].as_array_mut().unwrap().push(intent);
        record["sample_pending"] = p.sequence.into();
        storage::update(&self.operation_path(&p.operation_id), &record)
            .map_err(|e| ApiError::new("backup_failed", e.to_string()))?;
        self.operations.insert(p.operation_id, record);
        self.live.as_mut().unwrap().touched = Instant::now();
        self.live_work = Some(Sample {
            plan,
            sequence: p.sequence,
            finish: p.finish,
        });
        Ok(receipt)
    }
    fn operation_path(&self, id: &str) -> PathBuf {
        self.paths
            .metadata
            .join("operations")
            .join(format!("{id}.json"))
    }
    pub(super) fn cancel_control(&mut self, owner: &str, value: Value) -> Result<Value> {
        let p: Cancel = protocol::params(value)?;
        let record = self
            .operations
            .get(&p.operation_id)
            .filter(|v| v["client_token"] == owner && v["kind"] == "live_control")
            .ok_or_else(|| ApiError::new("outcome_unknown", "Unknown control gesture"))?;
        if record["status"] != "pending" {
            return Ok(public_operation(record));
        }
        if self.live.as_ref().is_none_or(|c| c.id != p.operation_id) {
            return Err(ApiError::new(
                "outcome_unknown",
                "Gesture cannot be resumed",
            ));
        }
        self.end_control("cancelled");
        Ok(public_operation(&self.operations[&p.operation_id]))
    }
    pub(super) fn end_control(&mut self, reason: &str) {
        let Some(c) = self.live.take() else {
            return;
        };
        let dropped = self.live_work.take().is_some();
        let mut record = self.operations[&c.id].clone();
        record["status"] = "completed".into();
        record["gesture_end"] = reason.into();
        if dropped {
            record["sample_pending"] = Value::Null;
        }
        if record.get("device_outcome").is_none() {
            record["device_outcome"] = "not_attempted".into();
            record["device_write_attempted"] = false.into();
            record["journal_status"] = "not_needed".into();
        }
        self.store_control(&c, record, true);
    }
    fn store_control(&mut self, c: &Control, mut record: Value, finished: bool) {
        if let Err(error) = storage::update(&self.operation_path(&c.id), &record) {
            record["status"] = "indeterminate".into();
            record["metadata_error"] = error.to_string().into();
            self.live = None;
        }
        self.operations.insert(c.id.clone(), record.clone());
        self.emit(
            if finished || record["status"] == "indeterminate" {
                "operation_finished"
            } else {
                "control_updated"
            },
            Some(&c.device),
            public_operation(&record),
        );
    }
    pub(super) fn execute_control(&mut self) {
        let Some(sample) = self.live_work.take() else {
            return;
        };
        let Some(mut c) = self.live.take() else {
            return;
        };
        let ready = self.refresh().and_then(|()| {
            let i = self.identity(&c.device, &c.generation)?;
            self.revision(i, &c.revision)
        });
        let outcome = ready.and_then(|()| {
            self.hardware
                .execute(&sample.plan, &c.directory)
                .map_err(ApiError::from)
        });
        let mut record = self.operations[&c.id].clone();
        record["sample_pending"] = Value::Null;
        record["sample_sequence"] = sample.sequence.into();
        let verified = match outcome {
            Ok(o) => {
                let verified = o.device_outcome == DeviceOutcome::Verified
                    && o.journal_status != JournalStatus::Failed;
                record["device_outcome"] = json!(o.device_outcome);
                record["device_write_attempted"] =
                    json!(if record["device_write_attempted"] == true {
                        Some(true)
                    } else {
                        o.device_write_attempted
                    });
                record["rollback_attempted"] = o.rollback_attempted.into();
                record["rollback_verified"] = o.rollback_verified.into();
                record["after_state_valid"] = o.after_state.is_some().into();
                record["state"] = o
                    .after_state
                    .as_ref()
                    .map(State::decoded)
                    .unwrap_or(Value::Null);
                record["journal_status"] = json!(o.journal_status);
                record["journal"] = json!(o.journal);
                record["backup"] = json!(o.backup);
                record["error"] = o
                    .error
                    .map(ApiError::from)
                    .map(|e| e.value())
                    .unwrap_or(Value::Null);
                record["journal_error"] = json!(o.journal_error);
                verified
            }
            Err(e) => {
                record["device_outcome"] = "rejected".into();
                record["error"] = e.value();
                record["after_state_valid"] = false.into();
                record["state"] = Value::Null;
                record["journal_status"] = "not_needed".into();
                record["rollback_attempted"] = false.into();
                record["rollback_verified"] = false.into();
                false
            }
        };
        // Associate only a readback equal to the prediction with this gesture.
        // Any difference remains an external event and closes the old revision.
        let event_start = self.pending_events.len();
        let refreshed = self.refresh().is_ok();
        let index = self.identity(&c.device, &c.generation).ok();
        let matches = refreshed
            && index
                .is_some_and(|i| self.devices[i].state.as_ref() == Some(sample.plan.predicted()));
        if verified && matches {
            c.revision = self.devices[index.unwrap()].revision.clone();
            for (_, event) in &mut self.pending_events[event_start..] {
                if event["event"] == "state_changed" && event["data"]["device_id"] == c.device {
                    event["data"]["operation_id"] = c.id.clone().into();
                }
            }
        }
        let finished = sample.finish || !verified || !matches || sample.sequence >= 512;
        if finished {
            record["status"] = "completed".into();
            record["gesture_end"] = (if !verified {
                "failed"
            } else if !matches {
                "conflict"
            } else if sample.finish {
                "finished"
            } else {
                "sample_limit"
            })
            .into();
        } else {
            c.touched = Instant::now();
            self.live = Some(Control {
                connection: c.connection,
                owner: c.owner.clone(),
                id: c.id.clone(),
                device: c.device.clone(),
                generation: c.generation.clone(),
                revision: c.revision.clone(),
                target: c.target.clone(),
                directory: c.directory.clone(),
                touched: c.touched,
            });
        }
        self.store_control(&c, record, finished);
    }
}
