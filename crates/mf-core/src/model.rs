// SPDX-License-Identifier: MIT
use serde::{Deserialize, Serialize};

pub const VID: u16 = 0x1c75;
pub const MF4_PID: u16 = 0xaf70;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Model {
    MiniFuse1,
    MiniFuse2,
    MiniFuse4,
}

impl Model {
    pub fn name(self) -> &'static str {
        match self {
            Self::MiniFuse1 => "MiniFuse 1",
            Self::MiniFuse2 => "MiniFuse 2",
            Self::MiniFuse4 => "MiniFuse 4",
        }
    }
    pub fn targets(self) -> &'static str {
        if self == Self::MiniFuse4 {
            "inst1, inst2 or 48v"
        } else {
            "inst, 48v or monitor"
        }
    }
    pub fn legacy_selector(self, target: &str) -> Option<u16> {
        match (self, target) {
            (Self::MiniFuse4, "inst" | "inst1") => Some(0x0000),
            (Self::MiniFuse4, "inst2") => Some(0x0001),
            (Self::MiniFuse4, "48v") => Some(0x0300),
            (Self::MiniFuse1 | Self::MiniFuse2, "inst") => Some(0x0000),
            (Self::MiniFuse1 | Self::MiniFuse2, "48v") => Some(0x0400),
            (Self::MiniFuse1 | Self::MiniFuse2, "monitor") => Some(0x0500),
            _ => None,
        }
    }
    pub fn from_pid(pid: u16) -> Option<Self> {
        match pid {
            0xaf80 | 0xaf82 => Some(Self::MiniFuse1),
            0xaf90 => Some(Self::MiniFuse2),
            MF4_PID => Some(Self::MiniFuse4),
            _ => None,
        }
    }

    pub fn require_mixer(self) -> Result<(), String> {
        if self == Self::MiniFuse4 {
            Ok(())
        } else {
            Err("Mixer/status/config commands require a MiniFuse 4. The MiniFuse 1/2 state layout is not established; no B2 request was sent.".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_gate_is_explicit() {
        for pid in [0xaf80, 0xaf82, 0xaf90] {
            assert!(Model::from_pid(pid).unwrap().require_mixer().is_err());
        }
        assert!(Model::from_pid(MF4_PID).unwrap().require_mixer().is_ok());
        assert!(Model::from_pid(0xaf72).is_none());
        assert!(Model::from_pid(0x1234).is_none());
    }
    #[test]
    fn legacy_selectors_preserve_all_three_models() {
        for model in [Model::MiniFuse1, Model::MiniFuse2] {
            assert_eq!(model.legacy_selector("inst"), Some(0));
            assert_eq!(model.legacy_selector("48v"), Some(0x0400));
            assert_eq!(model.legacy_selector("monitor"), Some(0x0500));
            assert_eq!(model.legacy_selector("inst2"), None);
        }
        assert_eq!(Model::MiniFuse4.legacy_selector("48v"), Some(0x0300));
        assert_eq!(Model::MiniFuse4.legacy_selector("inst2"), Some(1));
        assert_eq!(Model::MiniFuse4.legacy_selector("monitor"), None);
    }
}
