// SPDX-License-Identifier: MIT
use mf_service::{coordinator::Coordinator, hardware::Linux, server, storage::Paths};
use std::sync::atomic::{AtomicBool, Ordering};
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: libc::c_int) {
    STOP.store(true, Ordering::Release);
}
fn main() -> std::io::Result<()> {
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "--version" => println!("mf-service {}", env!("CARGO_PKG_VERSION")),
            "--help" | "-h" => println!(
                "mf-service\nRun the user-session MiniFuse control service in the foreground.\nSocket: $XDG_RUNTIME_DIR/mf-service/control.sock\nRequires the reviewed module and existing session access. No device restore occurs at startup."
            ),
            _ => return Err(std::io::Error::other("Unknown argument; use --help")),
        }
        return Ok(());
    }
    unsafe {
        libc::signal(libc::SIGTERM, stop as libc::sighandler_t);
        libc::signal(libc::SIGINT, stop as libc::sighandler_t);
    }
    let coordinator = Coordinator::new(Linux, Paths::environment()?)?;
    server::run(&server::runtime_directory()?, coordinator, &STOP)
}
