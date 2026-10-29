// SPDX-License-Identifier: MIT
use mf_core::{Device, MF4_PID, Model, Outcome, Plan, State, VID};
use mf_service::{
    coordinator::Coordinator,
    hardware::{Descriptor, Hardware},
    protocol::Result,
    server,
    storage::Paths,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

#[derive(Clone)]
struct DeviceReads(Arc<AtomicU64>);
impl Hardware for DeviceReads {
    fn inventory(&mut self) -> Result<Vec<Descriptor>> {
        Ok(vec![Descriptor {
            device: device(),
            model: Model::MiniFuse4,
            connection_marker: 1,
        }])
    }
    fn read(&mut self) -> Result<(Device, State)> {
        let mut state = State([0; 190]);
        state.0[0] = self.0.fetch_add(1, Ordering::Relaxed) as u8;
        Ok((device(), state))
    }
    fn execute(&mut self, _: &Plan, _: &Path) -> std::result::Result<Outcome, mf_core::Error> {
        panic!("Read-only socket tests must not dispatch a mutation")
    }
}
fn device() -> Device {
    Device {
        vendor_id: VID,
        product_id: MF4_PID,
        bus: 1,
        address: 2,
    }
}
struct Running {
    root: PathBuf,
    stop: &'static AtomicBool,
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
}
impl Running {
    fn start() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "mf-service-socket-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let paths = Paths {
            metadata: root.join("metadata"),
            presets: root.join("presets"),
            backups: root.join("backups"),
        };
        let coordinator =
            Coordinator::new(DeviceReads(Arc::new(AtomicU64::new(0))), paths).unwrap();
        let runtime = root.join("runtime");
        let socket = runtime.join("control.sock");
        let stop: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
        let worker = thread::spawn(move || server::run(&runtime, coordinator, stop));
        for _ in 0..200 {
            if worker.is_finished() {
                panic!(
                    "Service failed during startup: {:?}",
                    worker.join().unwrap()
                );
            }
            if socket.exists() {
                return Self {
                    root,
                    stop,
                    worker: Some(worker),
                };
            }
            thread::sleep(Duration::from_millis(5));
        }
        panic!("Service socket did not start");
    }
    fn connect(&self) -> BufReader<UnixStream> {
        let socket = UnixStream::connect(self.root.join("runtime/control.sock")).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        BufReader::new(socket)
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap().unwrap();
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn send(socket: &mut BufReader<UnixStream>, id: &str, method: &str, params: Value) -> Value {
    writeln!(
        socket.get_mut(),
        "{}",
        json!({"api":1,"id":id,"method":method,"params":params})
    )
    .unwrap();
    read(socket)
}
fn read(socket: &mut BufReader<UnixStream>) -> Value {
    let mut line = String::new();
    assert_ne!(socket.read_line(&mut line).unwrap(), 0);
    serde_json::from_str(&line).unwrap()
}
fn hello(socket: &mut BufReader<UnixStream>) {
    assert_eq!(
        send(
            socket,
            "hello",
            "hello",
            json!({"client":"socket-test","supported_api":[1]})
        )["result"]["selected_api"],
        1
    );
}

#[test]
fn framing_permissions_and_combined_requests_work_on_a_real_unix_socket() {
    let running = Running::start();
    let mut socket = running.connect();
    assert_eq!(
        fs::metadata(running.root.join("runtime/control.sock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    socket
        .get_mut()
        .write_all(b"{\"api\":1,\"id\":\"hello\",\"method\":\"hello\",")
        .unwrap();
    socket
        .get_mut()
        .write_all(b"\"params\":{\"client\":\"test\",\"supported_api\":[1]}}\n")
        .unwrap();
    assert_eq!(read(&mut socket)["result"]["selected_api"], 1);
    socket.get_mut().write_all(b"{\"api\":1,\"id\":\"one\",\"method\":\"list_devices\",\"params\":{}}\n{\"api\":1,\"id\":\"two\",\"method\":\"list_presets\",\"params\":{}}\n").unwrap();
    assert_eq!(read(&mut socket)["id"], "one");
    assert_eq!(read(&mut socket)["id"], "two");
}
#[test]
fn a_second_instance_cannot_replace_the_live_socket() {
    let running = Running::start();
    let paths = Paths {
        metadata: running.root.join("metadata"),
        presets: running.root.join("presets"),
        backups: running.root.join("backups"),
    };
    let coordinator = Coordinator::new(DeviceReads(Arc::new(AtomicU64::new(0))), paths).unwrap();
    let stop = Box::leak(Box::new(AtomicBool::new(false)));
    assert_eq!(
        server::run(&running.root.join("runtime"), coordinator, stop)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::AddrInUse
    );
    let mut socket = running.connect();
    hello(&mut socket);
}
#[test]
fn incomplete_frames_have_a_deadline() {
    let running = Running::start();
    let mut socket = running.connect();
    socket.get_mut().write_all(b"{").unwrap();
    thread::sleep(Duration::from_millis(2350));
    let mut line = String::new();
    assert_eq!(socket.read_line(&mut line).unwrap(), 0);
}
#[test]
fn slow_subscribers_do_not_block_other_clients() {
    let running = Running::start();
    let mut slow = running.connect();
    hello(&mut slow);
    assert!(
        send(
            &mut slow,
            "subscribe",
            "subscribe",
            json!({"events":["state_changed"]})
        )
        .get("result")
        .is_some()
    );
    let mut fast = running.connect();
    hello(&mut fast);
    let inventory = send(&mut fast, "devices", "list_devices", json!({}));
    let device = &inventory["result"]["devices"][0];
    for index in 0..180 {
        let result = send(
            &mut fast,
            &format!("state-{index}"),
            "get_state",
            json!({"device_id":device["device_id"],"generation":device["generation"]}),
        );
        assert!(result.get("result").is_some(), "{result}");
    }
    // EOF after draining already delivered frames confirms bounded eviction.
    let mut line = String::new();
    loop {
        line.clear();
        if slow.read_line(&mut line).unwrap() == 0 {
            break;
        }
    }
}
