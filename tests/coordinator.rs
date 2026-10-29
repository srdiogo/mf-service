// SPDX-License-Identifier: MIT
use mf_core::{Backend, Device, Error, ErrorKind, MF4_PID, Model, Outcome, Plan, State, VID};
use mf_service::{
    coordinator::Coordinator,
    hardware::{Descriptor, Hardware},
    protocol::{self, Result},
    storage::Paths,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone)]
struct Fake(Arc<Mutex<Data>>);
struct Data {
    descriptors: Vec<Descriptor>,
    state: State,
    reads: usize,
    writes: usize,
}
impl Fake {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Data {
            descriptors: vec![Descriptor {
                device: Device {
                    vendor_id: VID,
                    product_id: MF4_PID,
                    bus: 1,
                    address: 2,
                },
                model: Model::MiniFuse4,
                connection_marker: 1,
            }],
            state: State::from_hex(include_str!("fixtures/windows-b2.hex").trim()).unwrap(),
            reads: 0,
            writes: 0,
        })))
    }
}
impl Backend for Fake {
    fn transact(&mut self, plan: &Plan) -> std::result::Result<State, Error> {
        let mut data = self.0.lock().unwrap();
        if data.state != *plan.before() {
            return Err(Error::new(ErrorKind::StaleState, "External change"));
        }
        data.writes += 1;
        data.state = plan.predicted().clone();
        Ok(data.state.clone())
    }
}
impl Hardware for Fake {
    fn inventory(&mut self) -> Result<Vec<Descriptor>> {
        Ok(self.0.lock().unwrap().descriptors.clone())
    }
    fn read(&mut self) -> Result<(Device, State)> {
        let mut d = self.0.lock().unwrap();
        d.reads += 1;
        Ok((d.descriptors[0].device.clone(), d.state.clone()))
    }
    fn execute(&mut self, plan: &Plan, backups: &Path) -> std::result::Result<Outcome, Error> {
        mf_core::execute(self, plan, backups)
    }
}
struct Test {
    directory: PathBuf,
    hardware: Fake,
    coordinator: Coordinator<Fake>,
    token: String,
    hello: Value,
}
impl Test {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "mf-service-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let hardware = Fake::new();
        let coordinator = Coordinator::new(hardware.clone(), paths(&directory)).unwrap();
        let mut test = Self {
            directory,
            hardware,
            coordinator,
            token: String::new(),
            hello: Value::Null,
        };
        test.coordinator.connected(1);
        let hello = test.call("hello", json!({"client":"test","supported_api":[1]}));
        test.token = hello["result"]["client_token"].as_str().unwrap().into();
        test.hello = hello;
        test
    }
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.coordinator.request(
            1,
            protocol::Request {
                api: 1,
                id: "test-request".into(),
                method: method.into(),
                params,
            },
        )
    }
    fn state(&mut self) -> Value {
        let devices = self.call("list_devices", json!({}));
        let d = &devices["result"]["devices"][0];
        let result = self.call(
            "get_state",
            json!({"device_id":d["device_id"],"generation":d["generation"]}),
        );
        assert!(result.get("error").is_none(), "{result}");
        result["result"].clone()
    }
    fn plan(&mut self) -> String {
        let state = self.state();
        let result=self.call("plan_changes",json!({"device_id":state["device_id"],"generation":state["generation"],"expected_revision":state["revision"],"changes":[{"kind":"master","mix":"out12","level_db":null}]}));
        assert!(result.get("error").is_none(), "{result}");
        result["result"]["plan_id"].as_str().unwrap().into()
    }
}
fn paths(root: &Path) -> Paths {
    Paths {
        metadata: root.join("metadata"),
        presets: root.join("presets"),
        backups: root.join("backups"),
    }
}
impl Drop for Test {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn rejects_unsafe_wire_and_distinguishes_silence_from_omission() {
    for text in [
        r#"{"api":1,"id":"x","method":"hello","params":{},"id":"y"}"#,
        r#"{"api":1,"id":"x","method":"hello","params":{"nested":{"a":1,"a":2}}}"#,
        r#"{"api":1,"id":"x","method":"hello","params":[],"extra":0}"#,
    ] {
        assert!(protocol::parse(text.as_bytes()).is_err());
    }
    let mut test = Test::new();
    let state = test.state();
    let reads = test.hardware.0.lock().unwrap().reads;
    for change in [
        json!({"kind":"channel","mix":"out12","source":"analog1","pan":null}),
        json!({"kind":"master","mix":"out12"}),
        json!({"kind":"master","mix":"out12","level_db":1}),
        json!({"kind":"raw","selector":0x900,"value":0}),
    ] {
        let result=test.call("plan_changes",json!({"device_id":state["device_id"],"generation":state["generation"],"expected_revision":state["revision"],"changes":[change]}));
        assert_eq!(result["error"]["code"], "invalid_request", "{result}");
    }
    assert_eq!(test.hardware.0.lock().unwrap().reads, reads);
    assert_eq!(test.hardware.0.lock().unwrap().writes, 0);
    let content = serde_json::to_string(&mf_core::Snapshot::new(
        Device {
            vendor_id: VID,
            product_id: MF4_PID,
            bus: 0,
            address: 0,
        },
        &test.hardware.0.lock().unwrap().state,
    ))
    .unwrap();
    assert!(
        test.call("inspect_snapshot", json!({"snapshot":content}))
            .get("error")
            .is_none()
    );
    assert_eq!(test.hardware.0.lock().unwrap().reads, reads);
}
#[test]
fn external_full_state_changes_reject_without_dispatch() {
    let mut test = Test::new();
    let plan = test.plan();
    test.hardware.0.lock().unwrap().state.0[0] ^= 1;
    let result = test.call(
        "apply_plan",
        json!({"plan_id":plan,"idempotency_key":"first"}),
    );
    assert_eq!(result["error"]["code"], "stale_state");
    assert_eq!(test.hardware.0.lock().unwrap().writes, 0);
}
#[test]
fn durable_admission_deduplicates_after_disconnect_and_execution() {
    let mut test = Test::new();
    let plan = test.plan();
    let apply = json!({"plan_id":plan,"idempotency_key":"once"});
    let accepted = test.call("apply_plan", apply.clone());
    assert_eq!(accepted["result"]["status"], "pending");
    assert_eq!(test.hardware.0.lock().unwrap().writes, 0);
    let repeated = test.call("apply_plan", apply.clone());
    assert_eq!(
        accepted["result"]["operation_id"],
        repeated["result"]["operation_id"]
    );
    test.coordinator.execute_next();
    assert_eq!(test.hardware.0.lock().unwrap().writes, 1);
    test.coordinator.disconnected(1);
    test.coordinator.connected(1);
    let token = test.token.clone();
    assert!(
        test.call(
            "hello",
            json!({"client":"test","supported_api":[1],"client_token":token})
        )
        .get("error")
        .is_none()
    );
    let result = test.call("apply_plan", apply);
    assert_eq!(result["result"]["device_outcome"], "verified");
    assert_eq!(test.hardware.0.lock().unwrap().writes, 1);
    let conflict = test.call(
        "apply_plan",
        json!({"plan_id":"different-plan","idempotency_key":"once"}),
    );
    assert_eq!(conflict["error"]["code"], "idempotency_conflict");
    let query = test.call("get_operation", json!({"idempotency_key":"once"}));
    assert_eq!(query["result"]["device_outcome"], "verified");
    assert!(query["result"].get("client_token").is_none());
}
#[test]
fn restart_never_replays_prepared_operations() {
    let mut test = Test::new();
    let plan = test.plan();
    test.call(
        "apply_plan",
        json!({"plan_id":plan,"idempotency_key":"before-crash"}),
    );
    test.coordinator = Coordinator::new(test.hardware.clone(), paths(&test.directory)).unwrap();
    test.coordinator.connected(1);
    let token = test.token.clone();
    test.call(
        "hello",
        json!({"client":"test","supported_api":[1],"client_token":token}),
    );
    let result = test.call("get_operation", json!({"idempotency_key":"before-crash"}));
    assert_eq!(result["result"]["status"], "indeterminate");
    assert_eq!(result["result"]["device_outcome"], "unknown");
    test.coordinator.execute_next();
    assert_eq!(test.hardware.0.lock().unwrap().writes, 0);
}
#[test]
fn reconnection_invalidates_plans_without_inferred_other_model_reads() {
    let mut test = Test::new();
    let plan = test.plan();
    test.hardware.0.lock().unwrap().descriptors[0].connection_marker += 1;
    let result = test.call(
        "apply_plan",
        json!({"plan_id":plan,"idempotency_key":"old"}),
    );
    assert_eq!(result["error"]["code"], "device_changed");
    assert_eq!(test.hardware.0.lock().unwrap().writes, 0);
    let reads = test.hardware.0.lock().unwrap().reads;
    {
        let mut data = test.hardware.0.lock().unwrap();
        data.descriptors[0].model = Model::MiniFuse2;
        data.descriptors[0].device.product_id = 0xaf90;
    }
    let devices = test.call("list_devices", json!({}));
    let d = &devices["result"]["devices"][0];
    assert_eq!(d["capabilities"]["mixer"], false);
    let result = test.call(
        "get_state",
        json!({"device_id":d["device_id"],"generation":d["generation"]}),
    );
    assert_eq!(result["error"]["code"], "unsupported_capability");
    assert_eq!(test.hardware.0.lock().unwrap().reads, reads);
}
#[test]
fn subscriptions_have_an_atomic_baseline_and_contiguous_filtered_sequence() {
    let mut test = Test::new();
    test.state();
    let subscription = test.call(
        "subscribe",
        json!({"events":["state_changed","operation_finished"]}),
    );
    assert_eq!(subscription["result"]["seq"], 0);
    assert!(test.coordinator.pending_events.is_empty());
    test.hardware.0.lock().unwrap().state.0[0] ^= 1;
    test.coordinator.refresh().unwrap();
    let plan = test.plan();
    test.call(
        "apply_plan",
        json!({"plan_id":plan,"idempotency_key":"event"}),
    );
    test.coordinator.execute_next();
    let events: Vec<_> = test
        .coordinator
        .pending_events
        .iter()
        .map(|(_, v)| v.clone())
        .collect();
    for (i, event) in events.iter().enumerate() {
        assert_eq!(event["seq"], i + 1);
        assert!(event["data"].get("client_token").is_none());
    }
    assert_eq!(events.last().unwrap()["event"], "operation_finished");
}
#[test]
fn preset_paths_are_owned_exclusive_and_never_device_operations() {
    use std::os::unix::fs::PermissionsExt;
    let mut test = Test::new();
    let content = serde_json::to_string(&mf_core::Snapshot::new(
        Device {
            vendor_id: VID,
            product_id: MF4_PID,
            bus: 0,
            address: 0,
        },
        &test.hardware.0.lock().unwrap().state,
    ))
    .unwrap();
    let saved = test.call("save_preset", json!({"name":"Studio","snapshot":content}));
    let id = saved["result"]["preset_id"].as_str().unwrap();
    assert_eq!(
        fs::metadata(test.directory.join("presets").join(format!("{id}.json")))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(
        test.call("load_preset", json!({"preset_id":id}))
            .get("error")
            .is_none()
    );
    assert_eq!(
        test.call("load_preset", json!({"preset_id":"../../elsewhere"}))["error"]["code"],
        "invalid_request"
    );
    assert_eq!(test.hardware.0.lock().unwrap().reads, 0);
    assert_eq!(test.hardware.0.lock().unwrap().writes, 0);
}

#[test]
fn canonical_wire_fixture_matches_the_implemented_service() {
    fn normalize(value: &mut Value) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    if matches!(
                        key.as_str(),
                        "epoch"
                            | "device_id"
                            | "generation"
                            | "revision"
                            | "client_token"
                            | "plan_id"
                            | "operation_id"
                            | "subscription_id"
                            | "journal"
                            | "backup"
                    ) && value.is_string()
                    {
                        *value = format!("fixture-{key}").into();
                    } else {
                        normalize(value);
                    }
                }
            }
            Value::Array(array) => {
                for value in array {
                    normalize(value);
                }
            }
            Value::Number(number) if number.is_f64() => {
                *number = serde_json::Number::from_f64(
                    (number.as_f64().unwrap() * 1_000_000.0).round() / 1_000_000.0,
                )
                .unwrap();
            }
            _ => {}
        }
    }
    let mut test = Test::new();
    let state = test.state();
    let plan=test.call("plan_changes",json!({"device_id":state["device_id"],"generation":state["generation"],"expected_revision":state["revision"],"changes":[{"kind":"master","mix":"out12","level_db":null}]}));
    let accepted = test.call(
        "apply_plan",
        json!({"plan_id":plan["result"]["plan_id"],"idempotency_key":"fixture-operation"}),
    );
    test.coordinator.execute_next();
    let finished = test.call(
        "get_operation",
        json!({"idempotency_key":"fixture-operation"}),
    );
    let error = test.call("load_preset", json!({"preset_id":"../../outside"}));
    let mut fixture = json!({"fixture_version":1,"hello":test.hello,"state":{"api":1,"id":"test-request","result":state},"plan":plan,"accepted":accepted,"finished":finished,"error":error});
    normalize(&mut fixture);
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/contract/v1.json");
    if std::env::var_os("MF_SERVICE_UPDATE_FIXTURES").is_some() {
        fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&fixture).unwrap()),
        )
        .unwrap();
    }
    let stored = fs::read_to_string(path).unwrap();
    assert!(
        serde_json::to_string_pretty(&fixture).unwrap() == stored.trim_end(),
        "Update the canonical contract fixture deliberately when the API changes"
    );
}

#[test]
fn queued_work_is_rejected_when_a_device_changes_after_admission() {
    let mut test = Test::new();
    let plan = test.plan();
    test.call(
        "apply_plan",
        json!({"plan_id":plan,"idempotency_key":"queued"}),
    );
    test.hardware.0.lock().unwrap().descriptors[0].connection_marker += 1;
    test.coordinator.execute_next();
    let result = test.call("get_operation", json!({"idempotency_key":"queued"}));
    assert_eq!(result["result"]["device_outcome"], "rejected");
    assert_eq!(result["result"]["error"]["code"], "device_changed");
    assert_eq!(test.hardware.0.lock().unwrap().writes, 0);
}

#[test]
fn metadata_failure_prevents_admission_and_execution() {
    let mut test = Test::new();
    let plan = test.plan();
    let operations = test.directory.join("metadata/operations");
    fs::remove_dir(&operations).unwrap();
    fs::write(&operations, b"blocked").unwrap();
    let result = test.call(
        "apply_plan",
        json!({"plan_id":plan,"idempotency_key":"blocked"}),
    );
    assert_eq!(result["error"]["code"], "backup_failed");
    test.coordinator.execute_next();
    assert_eq!(test.hardware.0.lock().unwrap().writes, 0);
}
