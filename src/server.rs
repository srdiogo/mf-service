// SPDX-License-Identifier: MIT
use crate::{
    coordinator::Coordinator,
    hardware::Hardware,
    protocol::{self, ApiError},
    storage,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::Shutdown,
    os::{
        fd::AsRawFd,
        unix::{
            fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

struct SocketGuard {
    path: PathBuf,
    inode: u64,
    _lock: File,
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path).is_ok_and(|m| m.ino() == self.inode) {
            let _ = fs::remove_file(&self.path);
        }
    }
}
struct Peer {
    output: SyncSender<Value>,
    stream: UnixStream,
    alive: Arc<AtomicBool>,
}
enum Message {
    Add(u64, Peer),
    Request(u64, protocol::Request),
}

fn bind(directory: &Path) -> std::io::Result<(UnixListener, SocketGuard)> {
    storage::directory(directory)?;
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join("instance.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AddrInUse,
            "Another mf-service instance holds the runtime lock",
        ));
    }
    let path = directory.join("control.sock");
    match fs::symlink_metadata(&path) {
        Ok(meta) => {
            if !meta.file_type().is_socket() || meta.uid() != unsafe { libc::geteuid() } {
                return Err(std::io::Error::other(
                    "Refusing to replace an unowned or non-socket runtime entry",
                ));
            }
            match UnixStream::connect(&path) {
                Ok(_) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AddrInUse,
                        "A live service owns this socket",
                    ));
                }
                Err(error) if error.raw_os_error() == Some(libc::ECONNREFUSED) => {
                    fs::remove_file(&path)?
                }
                Err(error) => return Err(error),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    let inode = fs::symlink_metadata(&path)?.ino();
    listener.set_nonblocking(true)?;
    Ok((
        listener,
        SocketGuard {
            path,
            inode,
            _lock: lock,
        },
    ))
}
fn same_uid(stream: &UnixStream) -> std::io::Result<bool> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    } < 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(cred.uid == unsafe { libc::geteuid() })
}
fn error(id: Value, error: ApiError) -> Value {
    json!({"api":1,"id":id,"error":error.value()})
}

fn connection(
    id: u64,
    mut stream: UnixStream,
    queue: SyncSender<Message>,
    count: Arc<AtomicUsize>,
) -> std::io::Result<()> {
    let mut writer = stream.try_clone()?;
    writer.set_write_timeout(Some(Duration::from_secs(2)))?;
    stream.set_read_timeout(Some(Duration::from_millis(250)))?;
    let closer = stream.try_clone()?;
    let (tx, rx) = mpsc::sync_channel::<Value>(64);
    let alive = Arc::new(AtomicBool::new(true));
    let pending = Arc::new(Mutex::new(HashSet::<String>::new()));
    let peer = Peer {
        output: tx.clone(),
        stream: closer,
        alive: alive.clone(),
    };
    if queue.try_send(Message::Add(id, peer)).is_err() {
        return Err(std::io::Error::other("Service admission queue is full"));
    }
    let writing = alive.clone();
    let waiting = pending.clone();
    thread::spawn(move || {
        while let Ok(value) = rx.recv() {
            let Ok(mut data) = serde_json::to_vec(&value) else {
                break;
            };
            if data.len() + 1 > protocol::MAX_FRAME {
                break;
            }
            data.push(b'\n');
            if writer.write_all(&data).is_err() {
                break;
            }
            if let Some(id) = value["id"].as_str() {
                waiting.lock().unwrap().remove(id);
            }
        }
        writing.store(false, Ordering::Release);
        let _ = writer.shutdown(Shutdown::Both);
    });
    thread::spawn(move || {
        let mut frame = Vec::new();
        let mut start = None;
        let mut buffer = [0; 4096];
        'reading: while alive.load(Ordering::Acquire) {
            if start.is_some_and(|time: Instant| time.elapsed() > Duration::from_secs(2)) {
                break;
            }
            let size = match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => size,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(_) => break,
            };
            for &byte in &buffer[..size] {
                if byte == b'\n' {
                    let request = match protocol::parse(&frame) {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = tx.try_send(error(Value::Null, e));
                            break 'reading;
                        }
                    };
                    frame.clear();
                    start = None;
                    {
                        let mut ids = pending.lock().unwrap();
                        if ids.len() >= 32 || !ids.insert(request.id.clone()) {
                            let _ = tx.try_send(error(
                                request.id.into(),
                                ApiError::invalid("Duplicate or excessive outstanding request ID"),
                            ));
                            break 'reading;
                        }
                    }
                    let request_id = request.id.clone();
                    if queue.try_send(Message::Request(id, request)).is_err()
                        && tx
                            .try_send(error(
                                request_id.into(),
                                ApiError::new(
                                    "queue_full",
                                    "Service queue is full; no operation was admitted",
                                ),
                            ))
                            .is_err()
                    {
                        break 'reading;
                    }
                } else {
                    if frame.len() + 2 > protocol::MAX_FRAME {
                        break 'reading;
                    }
                    if start.is_none() {
                        start = Some(Instant::now());
                    }
                    frame.push(byte);
                }
            }
        }
        alive.store(false, Ordering::Release);
        count.fetch_sub(1, Ordering::AcqRel);
    });
    Ok(())
}

pub fn runtime_directory() -> std::io::Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            std::io::Error::other("XDG_RUNTIME_DIR must name the current user runtime directory")
        })?;
    let meta = fs::symlink_metadata(&runtime)?;
    if !runtime.is_absolute()
        || !meta.is_dir()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Invalid XDG_RUNTIME_DIR ownership or permissions",
        ));
    }
    Ok(runtime.join("mf-service"))
}

pub fn run<H: Hardware>(
    directory: &Path,
    mut coordinator: Coordinator<H>,
    stop: &'static AtomicBool,
) -> std::io::Result<()> {
    let (listener, _guard) = bind(directory)?;
    let (tx, rx) = mpsc::sync_channel::<Message>(32);
    let (done_tx, done_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut peers = HashMap::<u64, Peer>::new();
        let mut refreshed = Instant::now();
        let mut metered = Instant::now();
        while !stop.load(Ordering::Acquire) {
            let dead: Vec<_> = peers
                .iter()
                .filter(|(_, p)| !p.alive.load(Ordering::Acquire))
                .map(|(&id, _)| id)
                .collect();
            for id in dead {
                if let Some(peer) = peers.remove(&id) {
                    let _ = peer.stream.shutdown(Shutdown::Both);
                }
                coordinator.disconnected(id);
            }
            let wait = if coordinator.has_meter_subscribers() {
                25
            } else {
                100
            };
            match rx.recv_timeout(Duration::from_millis(wait)) {
                Ok(Message::Add(id, peer)) => {
                    coordinator.connected(id);
                    peers.insert(id, peer);
                }
                Ok(Message::Request(id, request)) => {
                    if let Some(peer) = peers.get(&id) {
                        let response = coordinator.request(id, request);
                        if peer.output.try_send(response).is_err() {
                            peer.alive.store(false, Ordering::Release);
                        }
                        // The durable accepted operation response is enqueued
                        // before blocking on the real transaction.
                        coordinator.execute_next();
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            let interval = if coordinator.has_subscribers() {
                Duration::from_secs(1)
            } else {
                Duration::from_secs(5)
            };
            if refreshed.elapsed() >= interval {
                let _ = coordinator.refresh();
                refreshed = Instant::now();
            }
            if coordinator.has_meter_subscribers() && metered.elapsed() >= Duration::from_millis(50)
            {
                coordinator.poll_meters();
                metered = Instant::now();
            }
            for (id, event) in coordinator.pending_events.drain(..) {
                if let Some(peer) = peers.get(&id) {
                    if peer.output.try_send(event).is_err() {
                        eprintln!("subscriber_overflow: client must reconnect for a new baseline");
                        peer.alive.store(false, Ordering::Release);
                        let _ = peer.stream.shutdown(Shutdown::Both);
                    }
                }
            }
        }
        for (_, peer) in peers {
            let _ = peer.stream.shutdown(Shutdown::Both);
        }
        let _ = done_tx.send(());
    });
    let count = Arc::new(AtomicUsize::new(0));
    let mut next = 0;
    while !stop.load(Ordering::Acquire) {
        if worker.is_finished() {
            stop.store(true, Ordering::Release);
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if !same_uid(&stream)? || count.load(Ordering::Acquire) >= 16 {
                    continue;
                }
                count.fetch_add(1, Ordering::AcqRel);
                next += 1;
                if connection(next, stream, tx.clone(), count.clone()).is_err() {
                    count.fetch_sub(1, Ordering::AcqRel);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(25))
            }
            Err(error) => return Err(error),
        }
    }
    drop(tx);
    done_rx.recv_timeout(Duration::from_secs(10)).map_err(|_|std::io::Error::new(std::io::ErrorKind::TimedOut,"Shutdown budget expired; a dispatched kernel transaction cannot be cancelled. Retain prepared records for reconciliation."))?;
    worker
        .join()
        .map_err(|_| std::io::Error::other("Coordinator worker failed"))?;
    Ok(())
}
