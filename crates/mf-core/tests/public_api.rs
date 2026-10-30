// SPDX-License-Identifier: MIT
// This integration test uses only the library's public API.
use mf_core::{
    Backend, Device, DeviceOutcome, Error, ErrorKind, InputSwitch, JournalStatus, Level, Mix,
    Model, Operation, Plan, Snapshot,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

fn fixture() -> Snapshot {
    Snapshot::load(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/windows-state.json"),
    )
    .unwrap()
}

#[test]
fn input_plans_preserve_unknown_bits_and_never_enter_snapshot_restore() {
    let snapshot = fixture();
    let mut before = snapshot.state().unwrap();
    before.set_word(0, 0xa5fc);
    before.set_word(6, 0x5aa0);
    let switch = |target, enabled| Operation::InputSwitch { target, enabled };
    let plan = mf_core::plan(
        &snapshot.device,
        &before,
        &[
            switch(InputSwitch::Inst1, true),
            switch(InputSwitch::Inst2, true),
            switch(InputSwitch::PhantomPower, true),
            master(),
        ],
    )
    .unwrap();
    assert!(plan.requires_input_switches());
    let after = plan.predicted();
    let inputs = after.inputs();
    assert!(inputs.inst1 && inputs.inst2 && inputs.phantom_power);
    let mut expected = before.clone();
    expected.set_word(0, 0xa5ff);
    expected.set_word(6, 0x5aa1);
    expected.set_word(138, 0x8000);
    assert_eq!(after, &expected);
    let restored = mf_core::plan_restore(&snapshot.device, after, &snapshot, None).unwrap();
    assert!(!restored.requires_input_switches());
    assert_eq!(restored.predicted().inputs(), inputs);
    assert_eq!(restored.predicted().word(0), 0xa5ff);
    assert_eq!(restored.predicted().word(6), 0x5aa1);
    for offset in [0, 1, 6, 7] {
        if snapshot.state().unwrap().0[offset] != after.0[offset] {
            assert!(restored.omitted_offsets().contains(&offset));
        }
    }
    assert!(
        mf_core::plan(
            &snapshot.device,
            &before,
            &[
                switch(InputSwitch::Inst1, false),
                switch(InputSwitch::Inst1, true),
            ]
        )
        .is_err()
    );
    for model in [Model::MiniFuse1, Model::MiniFuse2] {
        assert!(!model.capabilities().switch_readback);
        assert!(!model.capabilities().input_switches);
    }
    for pid in [0xaf80, 0xaf82, 0xaf90] {
        let device = Device {
            product_id: pid,
            ..snapshot.device.clone()
        };
        assert_eq!(
            mf_core::plan(&device, &before, &[switch(InputSwitch::Inst1, true)])
                .unwrap_err()
                .kind,
            ErrorKind::UnsupportedCapability
        );
    }
}
fn directory() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "mf-core-consumer-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&dir).unwrap();
    dir
}
fn master() -> Operation {
    Operation::Master {
        mix: Mix::Out12,
        level: Level::Silence,
    }
}

#[test]
fn external_consumer_plans_without_accessing_hardware_or_raw_selectors() {
    let snapshot = fixture();
    let before = snapshot.state().unwrap();
    let plan = mf_core::plan(&snapshot.device, &before, &[master()]).unwrap();
    assert_eq!(plan.device(), &snapshot.device);
    assert_eq!(plan.before(), &before);
    assert_eq!(plan.write_count(), 1);
    assert_eq!(
        plan.predicted().decoded()["mixes"][0]["master_db"],
        serde_json::Value::Null
    );
    assert_eq!(
        mf_core::plan_restore(&snapshot.device, &before, &snapshot, None)
            .unwrap()
            .write_count(),
        0
    );
    let other = Device {
        product_id: 0xaf90,
        ..snapshot.device
    };
    assert_eq!(
        mf_core::plan(&other, &before, &[master()])
            .unwrap_err()
            .kind,
        ErrorKind::UnsupportedCapability
    );
    assert!(Model::MiniFuse4.capabilities().mixer);
    assert!(!Model::MiniFuse2.capabilities().mixer);
    assert!(!Model::MiniFuse4.capabilities().control_panel_button);
}

#[test]
fn duplicate_requests_are_rejected_even_when_one_matches_the_current_value() {
    let snapshot = fixture();
    let before = snapshot.state().unwrap();
    let current = before.decoded()["mixes"][0]["master_db"].as_f64().unwrap();
    let first = Operation::Master {
        mix: Mix::Out12,
        level: Level::Db(current),
    };
    assert!(mf_core::plan(&snapshot.device, &before, &[first, master()]).is_err());
    let mute = before.decoded()["mixes"][0]["channels"][0]["mute"]
        .as_bool()
        .unwrap();
    let channel = |value| Operation::Channel {
        mix: Mix::Out12,
        source: "analog1".into(),
        level: None,
        pan: None,
        mute: Some(value),
        solo: None,
    };
    assert!(mf_core::plan(&snapshot.device, &before, &[channel(mute), channel(!mute)]).is_err());
}

struct Failing {
    restored: bool,
}
impl Backend for Failing {
    fn transact(&mut self, plan: &Plan) -> Result<mf_core::State, Error> {
        let mut failure = Error::new(ErrorKind::TransferFailed, "Injected transport failure");
        failure.device_write_attempted = Some(true);
        failure.rollback_attempted = true;
        failure.rollback_verified = self.restored;
        if self.restored {
            failure.after_state = Some(Box::new(plan.before().clone()));
        }
        Err(failure)
    }
}
#[test]
fn outcomes_preserve_verification_and_recovery_facts() {
    let snapshot = fixture();
    let before = snapshot.state().unwrap();
    let plan = mf_core::plan(&snapshot.device, &before, &[master()]).unwrap();
    for restored in [true, false] {
        let dir = directory();
        let result = mf_core::execute(&mut Failing { restored }, &plan, &dir).unwrap();
        assert_eq!(
            result.device_outcome,
            if restored {
                DeviceOutcome::FailedRestored
            } else {
                DeviceOutcome::FailedUnrestored
            }
        );
        assert!(result.rollback_attempted);
        assert_eq!(result.rollback_verified, restored);
        assert_eq!(result.journal_status, JournalStatus::Finalized);
        assert!(result.backup.as_ref().unwrap().is_file());
        assert!(result.require_success().is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn journal_failure_does_not_erase_a_verified_device_result() {
    struct JournalFailure(PathBuf);
    impl Backend for JournalFailure {
        fn transact(&mut self, plan: &Plan) -> Result<mf_core::State, Error> {
            let journal = fs::read_dir(&self.0)
                .unwrap()
                .map(|p| p.unwrap().path())
                .find(|p| p.to_string_lossy().ends_with(".operation.json"))
                .unwrap();
            let pending = journal.with_extension("pending");
            fs::create_dir(pending).unwrap();
            Ok(plan.predicted().clone())
        }
    }
    let snapshot = fixture();
    let plan = mf_core::plan(&snapshot.device, &snapshot.state().unwrap(), &[master()]).unwrap();
    let dir = directory();
    let result = mf_core::execute(&mut JournalFailure(dir.clone()), &plan, &dir).unwrap();
    assert_eq!(result.device_outcome, DeviceOutcome::Verified);
    assert_eq!(result.journal_status, JournalStatus::Failed);
    assert_eq!(result.after_state.as_ref(), Some(plan.predicted()));
    let error = result.require_success().unwrap_err();
    assert_eq!(error.kind, ErrorKind::JournalFailed);
    assert_eq!(error.after_state.as_deref(), Some(plan.predicted()));
    let prepared = fs::read_dir(&dir)
        .unwrap()
        .map(|p| p.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with(".operation.json"))
        .unwrap();
    let record: serde_json::Value = serde_json::from_slice(&fs::read(prepared).unwrap()).unwrap();
    assert_eq!(record["status"], "prepared");
    fs::remove_dir_all(dir).unwrap();
}
