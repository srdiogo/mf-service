// SPDX-License-Identifier: MIT
use serde::{Deserialize, Serialize};

pub const STATE_SIZE: usize = 190;
pub const CHANNEL_NAMES: [&str; 10] = [
    "analog1", "analog2", "analog3", "analog4", "usb1", "usb2", "usb3", "usb4", "usb5", "usb6",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mix {
    Out12,
    Out34,
    Loopback,
}

impl Mix {
    pub const ALL: [Self; 3] = [Self::Out12, Self::Out34, Self::Loopback];
    pub fn index(self) -> usize {
        match self {
            Self::Out12 => 0,
            Self::Out34 => 1,
            Self::Loopback => 2,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Out12 => "out12",
            Self::Out34 => "out34",
            Self::Loopback => "loopback",
        }
    }
    pub fn channels(self) -> usize {
        if self == Self::Loopback { 10 } else { 8 }
    }
    pub fn route_name(self, value: u16) -> &'static str {
        match (self, value) {
            (Self::Out12, 0) => "usb12",
            (Self::Out12, 1) => "mix",
            (Self::Out34, 2) => "usb34",
            (Self::Out34, 3) => "mix",
            (Self::Loopback, 4) => "usb56",
            (Self::Loopback, 5) => "mix",
            (Self::Loopback, 6) => "out12",
            (Self::Loopback, 7) => "out34",
            _ => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State(pub [u8; STATE_SIZE]);

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, x| {
            write!(out, "{x:02x}").expect("Writing to String cannot fail");
            out
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Inputs {
    pub inst1: bool,
    pub inst2: bool,
    pub phantom_power: bool,
}

impl State {
    pub fn inputs(&self) -> Inputs {
        Inputs {
            inst1: self.word(0) & 1 != 0,
            inst2: self.word(0) & 2 != 0,
            phantom_power: self.word(6) & 1 != 0,
        }
    }
    pub fn from_hex(raw: &str) -> Result<Self, String> {
        if raw.len() != STATE_SIZE * 2 || !raw.is_ascii() {
            return Err("B2 state must contain exactly 190 bytes encoded as hexadecimal".into());
        }
        let mut data = [0; STATE_SIZE];
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16)
                .map_err(|_| "Invalid hexadecimal B2 state")?;
        }
        Ok(Self(data))
    }
    pub fn word(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.0[offset], self.0[offset + 1]])
    }
    pub fn set_word(&mut self, offset: usize, word: u16) {
        self.0[offset..offset + 2].copy_from_slice(&word.to_le_bytes());
    }
    pub fn db(&self, offset: usize) -> Option<f64> {
        decode_db(self.word(offset))
    }
    pub fn decoded(&self) -> serde_json::Value {
        let mixes: Vec<_> =
            Mix::ALL
                .into_iter()
                .map(|mix| {
                    let i = mix.index();
                    let channels: Vec<_> = (0..mix.channels()).map(|ch| {
                let l = self.db(18 + i * 40 + ch * 2);
                let r = self.db(38 + i * 40 + ch * 2);
                let (level, pan) = level_pan(l, r);
                serde_json::json!({
                    "channel": CHANNEL_NAMES[ch], "left_db": l, "right_db": r,
                    "derived_level_db": level, "derived_pan": pan,
                    "mute": self.word(152 + i * 2) & (1 << ch) != 0,
                    "solo": self.word(158 + i * 2) & (1 << ch) != 0,
                    "mono_included": self.word(164 + i * 2) & (1 << ch) != 0,
                    "stereo_pair_included": self.word(170 + i * 2) & (1 << (ch / 2)) != 0,
                })
            }).collect();
                    let route = self.word(176 + i * 2);
                    serde_json::json!({ "mix": mix.name(), "channels": channels,
                "master_db": self.db(138 + i * 2), "route_value": route,
                "route": mix.route_name(route), "mono_mask": self.word(164 + i * 2),
                "stereo_mask": self.word(170 + i * 2) })
                })
                .collect();
        serde_json::json!({ "inputs": self.inputs(), "mixes": mixes, "input3_trim_db": self.db(148),
            "input4_trim_db": self.db(150), "opaque_header_hex": hex(&self.0[..18]),
            "opaque_tail_hex": hex(&self.0[182..]),
            "note": "Levels/pan are derived from stored gains, not signal meters. Trims are read-only." })
    }
}

pub fn decode_db(word: u16) -> Option<f64> {
    if word == 0x8000 {
        None
    } else {
        Some(f64::from(word as i16) / 256.0)
    }
}

pub fn level_pan(left: Option<f64>, right: Option<f64>) -> (Option<f64>, Option<f64>) {
    let l = left.map(|db| 10f64.powf(db / 10.0)).unwrap_or(0.0);
    let r = right.map(|db| 10f64.powf(db / 10.0)).unwrap_or(0.0);
    if l + r == 0.0 {
        return (None, None);
    }
    (
        Some(10.0 * (l + r).log10()),
        Some(2.0 * r.sqrt().atan2(l.sqrt()) / std::f64::consts::PI),
    )
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Change {
    pub selector: u16,
    pub value: u16,
}

pub const MAX_CHANGES: usize = 160;
pub const LEFT: [u16; 3] = [0x09, 0x1d, 0x31];
pub const RIGHT: [u16; 3] = [0x13, 0x27, 0x3b];

/// Map a captured selector to its state word and optional bitmask.
pub fn target(change: Change) -> Result<(usize, u16), String> {
    let feature = change.selector >> 8;
    let channel = usize::from(change.selector & 0xff);
    let signed = change.value as i16;
    if feature == 0 && channel < 2 {
        if change.value > 1 {
            return Err("INST requires a boolean".into());
        }
        return Ok((0, 1 << channel));
    }
    if feature == 3 && channel == 0 {
        if change.value > 1 {
            return Err("Phantom power requires a boolean".into());
        }
        return Ok((6, 1));
    }
    for (bus, base) in [(LEFT, 18), (RIGHT, 38)] {
        if let Some(mix) = bus.iter().position(|&f| f == feature) {
            if channel >= if mix == 2 { 10 } else { 8 } || signed > 12 * 256 {
                return Err("Unsupported source channel or gain".into());
            }
            return Ok((base + mix * 40 + channel * 2, 0));
        }
    }
    if (0x45..=0x47).contains(&feature) {
        if channel != 0 || signed > 0 || (signed < -64 * 256 && signed != i16::MIN) {
            return Err("Master must be -64..0 dB or -inf".into());
        }
        return Ok((138 + usize::from(feature - 0x45) * 2, 0));
    }
    for (first, base) in [(0x4c, 152), (0x4f, 158), (0x52, 164)] {
        if (first..=first + 2).contains(&feature) {
            let mix = usize::from(feature - first);
            let limit = if first == 0x52 {
                4
            } else if mix == 2 {
                10
            } else {
                8
            };
            if channel >= limit || change.value > 1 {
                return Err("Invalid channel or flag".into());
            }
            return Ok((base + mix * 2, 1 << channel));
        }
    }
    if feature == 0x55 || feature == 0x57 {
        let mix = if feature == 0x55 { 0 } else { 2 };
        if channel >= if mix == 0 { 4 } else { 5 } || change.value > 1 {
            return Err("Invalid stereo pair or flag".into());
        }
        return Ok((170 + mix * 2, 1 << channel));
    }
    if (0x58..=0x5a).contains(&feature) {
        let mix = usize::from(feature - 0x58);
        let valid = [(0..=1), (2..=3), (4..=7)];
        if channel != 0 || !valid[mix].contains(&change.value) {
            return Err("Unsupported route".into());
        }
        return Ok((176 + mix * 2, 0));
    }
    Err(format!(
        "Selector 0x{:04x} is not supported by the MF4 mixer",
        change.selector
    ))
}

pub fn original(state: &State, change: Change) -> Result<u16, String> {
    let (offset, mask) = target(change)?;
    let word = state.word(offset);
    Ok(if mask == 0 {
        word
    } else {
        u16::from(word & mask != 0)
    })
}

pub fn predict(state: &State, changes: &[Change]) -> Result<State, String> {
    if changes.len() > MAX_CHANGES {
        return Err("Too many changes for one transaction".into());
    }
    let mut out = state.clone();
    let mut seen = std::collections::HashSet::new();
    for &change in changes {
        if !seen.insert(change.selector) {
            return Err("Duplicate selector in transaction".into());
        }
        let (offset, mask) = target(change)?;
        let old = out.word(offset);
        out.set_word(
            offset,
            if mask == 0 {
                change.value
            } else {
                (old & !mask) | if change.value == 1 { mask } else { 0 }
            },
        );
        // Every original must also be accepted by the rollback path.
        target(Change {
            value: original(state, change)?,
            ..change
        })?;
    }
    Ok(out)
}

pub fn changed(state: &State, changes: Vec<Change>) -> Result<Vec<Change>, String> {
    predict(state, &changes)?;
    Ok(changes
        .into_iter()
        .filter(|c| original(state, *c).ok() != Some(c.value))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_and_rust_validators_and_ioctl_match() {
        let output =
            std::env::temp_dir().join(format!("mf-core-protocol-probe-{}", std::process::id()));
        let status = std::process::Command::new("cc")
            .args([
                "-std=c11",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-O2",
                concat!(env!("CARGO_MANIFEST_DIR"), "/tests/protocol_probe.c"),
                "-o",
            ])
            .arg(&output)
            .status()
            .expect("C compiler required for kernel/userspace ABI tests");
        assert!(status.success());
        let result = std::process::Command::new(&output).output().unwrap();
        std::fs::remove_file(output).unwrap();
        assert!(result.status.success());
        let text = String::from_utf8(result.stdout).unwrap();
        let parts: Vec<u64> = text
            .split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(
            parts[0],
            libc::_IOWR::<[u8; crate::transport::TX_SIZE]>(b'M' as u32, 0x41) as u64
        );
        assert_eq!(parts[2], libc::_IOR::<[u8; 16]>(b'M' as u32, 0x42) as u64);
        assert_eq!(parts[3], libc::_IOR::<[u8; 80]>(b'M' as u32, 0x43) as u64);
        let mut hash = 14695981039346656037u64;
        let mut fold = |v: u16| {
            hash = (hash ^ u64::from(v)).wrapping_mul(1099511628211);
        };
        for selector in 0..=u16::MAX {
            for value in [
                0, 1, 2, 3, 4, 5, 6, 7, 3072, 3073, 0x8000, 0xc000, 0xbfff, 0xffff,
            ] {
                match target(Change { selector, value }) {
                    Ok((offset, mask)) => {
                        fold(1);
                        fold(offset as u16);
                        fold(mask);
                    }
                    Err(_) => fold(0),
                }
            }
        }
        assert_eq!(
            hash, parts[1],
            "C and Rust disagree on selector/value validation"
        );
    }

    #[test]
    fn replay_capture_rows_into_the_state_model_without_usb() {
        let mut state = fixture();
        let mut supported = 0;
        let mut excluded = 0;
        for line in include_str!("../../../docs/mf4/control-writes.csv")
            .lines()
            .skip(1)
        {
            let cells: Vec<_> = line.split(',').collect();
            assert_eq!(cells.len(), 11);
            assert_eq!(cells[3], "0x21");
            assert_eq!(cells[4], "0x22");
            assert_eq!(cells[6], "0x0000");
            assert_eq!(cells[7], "2");
            assert_eq!(cells[10], "0x00000000");
            let selector = u16::from_str_radix(cells[5].trim_start_matches("0x"), 16).unwrap();
            let lo = u8::from_str_radix(&cells[9][..2], 16).unwrap();
            let hi = u8::from_str_radix(&cells[9][2..], 16).unwrap();
            let change = Change {
                selector,
                value: u16::from_le_bytes([lo, hi]),
            };
            if target(change).is_ok() {
                // Decode/model captured writes; this is not a hardware replay.
                let (offset, mask) = target(change).unwrap();
                let expected = if mask == 0 {
                    change.value
                } else {
                    (state.word(offset) & !mask) | if change.value == 1 { mask } else { 0 }
                };
                state = predict(&state, &[change]).unwrap();
                assert_eq!(state.word(offset), expected);
                supported += 1;
            } else {
                assert!(
                    matches!(selector >> 8, 0x06 | 0x48 | 0x56)
                        || ((0x52..=0x54).contains(&(selector >> 8)) && selector & 255 >= 4),
                    "unexpected excluded selector {selector:04x}"
                );
                excluded += 1;
            }
        }
        assert_eq!(supported + excluded, 712);
        assert_eq!(supported, 686);
        assert_eq!(excluded, 26);
    }
    pub fn fixture() -> State {
        State::from_hex(include_str!("../tests/fixtures/windows-b2.hex").trim()).unwrap()
    }
    #[test]
    fn captured_layout_and_routes() {
        let state = fixture();
        assert_eq!(state.word(138), 0xfa06);
        assert_eq!(state.word(176), 1);
        assert_eq!(state.word(178), 3);
        assert_eq!(state.word(180), 5);
        assert_eq!(Mix::Out12.route_name(0), "usb12");
        assert_eq!(Mix::Out34.route_name(2), "usb34");
        assert_eq!(Mix::Loopback.route_name(7), "out34");
    }
    #[test]
    fn exact_gain_and_bit_updates_preserve_every_other_byte() {
        let state = fixture();
        for mix in Mix::ALL {
            let i = mix.index();
            let c = Change {
                selector: LEFT[i] << 8,
                value: 0x8000,
            };
            let out = predict(&state, &[c]).unwrap();
            let mut expected = state.clone();
            expected.set_word(18 + i * 40, 0x8000);
            assert_eq!(out, expected);
        }
        let c = Change {
            selector: 0x4c00,
            value: 0,
        };
        let out = predict(&state, &[c]).unwrap();
        assert_eq!(&state.0[..152], &out.0[..152]);
        assert_eq!(state.word(152) & !1, out.word(152));
    }
    #[test]
    fn input_rollback_values_restore_the_complete_original_state() {
        let mut before = fixture();
        before.set_word(0, 0xfffd);
        before.set_word(6, 0xfffe);
        let changes = [
            Change {
                selector: 0,
                value: 0,
            },
            Change {
                selector: 1,
                value: 1,
            },
            Change {
                selector: 0x0300,
                value: 1,
            },
        ];
        let after = predict(&before, &changes).unwrap();
        assert_eq!(after.word(0), 0xfffe);
        assert_eq!(after.word(6), 0xffff);
        let undo: Vec<_> = changes
            .iter()
            .rev()
            .map(|&change| Change {
                value: original(&before, change).unwrap(),
                ..change
            })
            .collect();
        assert_eq!(predict(&after, &undo).unwrap(), before);
    }
    #[test]
    fn unsupported_selectors_channels_and_values_are_rejected() {
        for (s, v) in [
            (0x5601, 1),
            (0x0908, 0),
            (0x1d08, 0),
            (0x4802, 0),
            (0x0300, 2),
            (0x0301, 1),
            (0x0002, 1),
            (0x0000, 2),
            (0x0800, 2),
            (0x5800, 2),
            (0x5900, 1),
            (0x5a00, 8),
            (0x4501, 0),
            (0x4500, 1),
            (0x4c00, 2),
            (0x5204, 1),
            (0x0900, 3073),
        ] {
            assert!(
                target(Change {
                    selector: s,
                    value: v
                })
                .is_err(),
                "{s:04x} {v}"
            );
        }
        assert!(
            target(Change {
                selector: 0x3109,
                value: 0x8000
            })
            .is_ok()
        );
        assert!(State::from_hex("00").is_err());
        assert!(State::from_hex(&"é".repeat(190)).is_err());
        assert!(State::from_hex(&"xx".repeat(190)).is_err());
    }
    #[test]
    fn transactions_refuse_duplicates_and_unrestorable_originals() {
        let state = fixture();
        let c = Change {
            selector: 0x0900,
            value: 0,
        };
        assert!(predict(&state, &[c, c]).is_err());
        let mut bad = state;
        bad.set_word(18, 3073);
        assert!(predict(&bad, &[c]).is_err());
    }
}
