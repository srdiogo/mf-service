// SPDX-License-Identifier: MIT
use crate::protocol::{ApiError, Result};
use mf_core::{Device, Kernel, Meters, Model, ModuleCapabilities, Outcome, Plan, State, VID};
use std::{fs, os::unix::fs::MetadataExt, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub device: Device,
    pub model: Model,
    pub connection_marker: u64,
}
pub trait Hardware: Send + 'static {
    fn inventory(&mut self) -> Result<Vec<Descriptor>>;
    fn read(&mut self) -> Result<(Device, State)>;
    fn module_capabilities(&mut self) -> Result<ModuleCapabilities> {
        Ok(ModuleCapabilities::default())
    }
    fn meters(&mut self) -> Result<(Device, Meters)> {
        Err(ApiError::new(
            "unsupported_capability",
            "Signal meters unavailable",
        ))
    }
    fn execute(
        &mut self,
        plan: &Plan,
        backups: &Path,
    ) -> std::result::Result<Outcome, mf_core::Error>;
}
pub struct Linux;
impl Hardware for Linux {
    fn inventory(&mut self) -> Result<Vec<Descriptor>> {
        let entries = fs::read_dir("/sys/bus/usb/devices")
            .map_err(|e| ApiError::new("device_unavailable", e.to_string()))?;
        let mut result = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let read = |name: &str, base| {
                fs::read_to_string(path.join(name))
                    .ok()
                    .and_then(|s| u16::from_str_radix(s.trim(), base).ok())
            };
            if read("idVendor", 16) != Some(VID) {
                continue;
            }
            let Some(pid) = read("idProduct", 16) else {
                continue;
            };
            let Some(model) = Model::from_pid(pid) else {
                continue;
            };
            let (Some(bus), Some(address)) = (read("busnum", 10), read("devnum", 10)) else {
                continue;
            };
            result.push(Descriptor {
                device: Device {
                    vendor_id: VID,
                    product_id: pid,
                    bus,
                    address,
                },
                model,
                connection_marker: fs::metadata(&path).map(|m| m.ino()).unwrap_or(0),
            });
        }
        result.sort_by_key(|d| (d.device.bus, d.device.address));
        Ok(result)
    }
    fn read(&mut self) -> Result<(Device, State)> {
        Kernel::open(false)?.read().map_err(Into::into)
    }
    fn module_capabilities(&mut self) -> Result<ModuleCapabilities> {
        Kernel::open(false)?
            .module_capabilities()
            .map_err(Into::into)
    }
    fn meters(&mut self) -> Result<(Device, Meters)> {
        Kernel::open(false)?.meters().map_err(Into::into)
    }
    fn execute(
        &mut self,
        plan: &Plan,
        backups: &Path,
    ) -> std::result::Result<Outcome, mf_core::Error> {
        mf_core::execute(&mut Kernel::open(true)?, plan, backups)
    }
}
