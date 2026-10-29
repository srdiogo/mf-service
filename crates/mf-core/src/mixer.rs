// SPDX-License-Identifier: MIT
use crate::state::{CHANNEL_NAMES, Change, LEFT, Mix, RIGHT, State, changed, level_pan};

pub fn channel_index(mix: Mix, name: &str) -> Result<usize, String> {
    CHANNEL_NAMES[..mix.channels()]
        .iter()
        .position(|&n| n == name)
        .ok_or_else(|| format!("Unsupported source '{name}' for {}", mix.name()))
}

pub fn db_word(text: &str, low: f64, high: f64) -> Result<u16, String> {
    if text == "-inf" {
        return Ok(0x8000);
    }
    let db: f64 = text.parse().map_err(|_| "Expected a dB value or -inf")?;
    if !db.is_finite() || db < low || db > high {
        return Err(format!("Level must be {low}..{high} dB or -inf"));
    }
    Ok(((db * 256.0).round() as i16) as u16)
}

pub fn channel(
    state: &State,
    mix: Mix,
    name: &str,
    level: Option<&str>,
    pan: Option<f64>,
    mute: Option<bool>,
    solo: Option<bool>,
) -> Result<Vec<Change>, String> {
    let ch = channel_index(mix, name)?;
    let i = mix.index();
    let mut changes = Vec::new();
    if level.is_some() || pan.is_some() {
        let (old_level, old_pan) = level_pan(
            state.db(18 + i * 40 + ch * 2),
            state.db(38 + i * 40 + ch * 2),
        );
        let p = pan.or(old_pan).unwrap_or(0.5);
        if !p.is_finite() || !(0.0..=1.0).contains(&p) {
            return Err("Pan must be 0 (left)..1 (right)".into());
        }
        let db = if let Some(text) = level {
            crate::state::decode_db(db_word(text, -60.0, 12.0)?)
        } else {
            old_level.map(|db| db.clamp(-60.0, 12.0))
        };
        let (l, r) = if let Some(db) = db {
            let encode = |amplitude: f64| -> u16 {
                if amplitude <= 0.0 {
                    return 0x8000;
                }
                ((db + 20.0 * amplitude.log10())
                    .mul_add(256.0, 0.0)
                    .trunc()
                    .clamp(-32767.0, 3072.0) as i16) as u16
            };
            if p == 0.0 {
                (encode(1.0), 0x8000)
            } else if p == 1.0 {
                (0x8000, encode(1.0))
            } else {
                (
                    encode((std::f64::consts::FRAC_PI_2 * p).cos()),
                    encode((std::f64::consts::FRAC_PI_2 * p).sin()),
                )
            }
        } else {
            (0x8000, 0x8000)
        };
        changes.push(Change {
            selector: (LEFT[i] << 8) | ch as u16,
            value: l,
        });
        changes.push(Change {
            selector: (RIGHT[i] << 8) | ch as u16,
            value: r,
        });
    }
    for (value, family) in [(mute, 0x4c), (solo, 0x4f)] {
        if let Some(value) = value {
            changes.push(Change {
                selector: ((family + i as u16) << 8) | ch as u16,
                value: u16::from(value),
            });
        }
    }
    if changes.is_empty() {
        return Err("Specify --level, --pan, --mute or --solo".into());
    }
    Ok(changes)
}

pub fn master(mix: Mix, text: &str) -> Result<Vec<Change>, String> {
    Ok(vec![Change {
        selector: (0x45 + mix.index() as u16) << 8,
        value: db_word(text, -64.0, 0.0)?,
    }])
}

pub fn route(mix: Mix, source: &str) -> Result<Vec<Change>, String> {
    let value = match (mix, source) {
        (Mix::Out12, "usb12") => 0,
        (Mix::Out12, "mix") => 1,
        (Mix::Out34, "usb34") => 2,
        (Mix::Out34, "mix") => 3,
        (Mix::Loopback, "usb56") => 4,
        (Mix::Loopback, "mix") => 5,
        (Mix::Loopback, "out12") => 6,
        (Mix::Loopback, "out34") => 7,
        _ => return Err(format!("Unsupported route '{source}' for {}", mix.name())),
    };
    Ok(vec![Change {
        selector: (0x58 + mix.index() as u16) << 8,
        value,
    }])
}

pub fn source(state: &State, mix: Mix, name: &str, included: bool) -> Result<Vec<Change>, String> {
    let i = mix.index();
    let (selector, channels) = if let Some(ch) = CHANNEL_NAMES[..4].iter().position(|&n| n == name)
    {
        if state.word(170 + i * 2) & (1 << (ch / 2)) != 0 {
            return Err("Analog mono/stereo conversion is not implemented; keep the current stereo topology".into());
        }
        (((0x52 + i as u16) << 8) | ch as u16, vec![ch])
    } else {
        let pair = match name {
            "usb12" => 2,
            "usb34" => 3,
            "usb56" if mix == Mix::Loopback => 4,
            _ => {
                return Err(
                    "Use analog1..analog4 or a supported USB pair (usb12/usb34/usb56)".into(),
                );
            }
        };
        if mix == Mix::Out34 {
            return Err("Stereo inclusion on out34 (0x56) is not validated".into());
        }
        if state.word(164 + i * 2) & (3 << (pair * 2)) != 0 {
            return Err("USB mono/stereo conversion is not implemented; the USB mono flags must already be clear".into());
        }
        (
            ((if mix == Mix::Out12 { 0x55 } else { 0x57 }) << 8) | pair,
            vec![pair as usize * 2, pair as usize * 2 + 1],
        )
    };
    let mut changes = Vec::new();
    if !included {
        for ch in channels {
            changes.push(Change {
                selector: ((0x4c + i as u16) << 8) | ch as u16,
                value: 1,
            });
            changes.push(Change {
                selector: ((0x4f + i as u16) << 8) | ch as u16,
                value: 0,
            });
        }
    }
    changes.push(Change {
        selector,
        value: u16::from(included),
    });
    Ok(changes)
}

/// Restore only captured controls. Topology that needs an unvalidated selector
/// is refused before any operation is sent. Opaque bytes and trims stay live.
pub fn restore(
    current: &State,
    saved: &State,
    selected: Option<Mix>,
) -> Result<Vec<Change>, String> {
    let mut plan = Vec::new();
    for mix in Mix::ALL
        .into_iter()
        .filter(|m| selected.is_none_or(|s| *m == s))
    {
        let i = mix.index();
        let stereo_diff = current.word(170 + i * 2) ^ saved.word(170 + i * 2);
        if stereo_diff & 3 != 0 {
            return Err("Snapshot requires unsupported analog mono/stereo conversion".into());
        }
        if mix == Mix::Out34 && stereo_diff != 0 {
            return Err("Snapshot requires unvalidated out34 stereo inclusion (0x56)".into());
        }
        for pair in 0..2 {
            if current.word(170 + i * 2) & (1 << pair) != 0
                && (current.word(164 + i * 2) ^ saved.word(164 + i * 2)) & (3 << (pair * 2)) != 0
            {
                return Err(
                    "Snapshot requires changing mono flags under an active analog stereo pair"
                        .into(),
                );
            }
        }
        for pair in 2..if mix == Mix::Loopback { 5 } else { 4 } {
            if stereo_diff & (1 << pair) != 0
                && (current.word(164 + i * 2) | saved.word(164 + i * 2)) & (3 << (pair * 2)) != 0
            {
                return Err("Snapshot requires unsupported USB mono/stereo conversion".into());
            }
        }
        for ch in 0..mix.channels() {
            for (family, base) in [
                (LEFT[i], 18),
                (RIGHT[i], 38),
                (0x4c + i as u16, 152),
                (0x4f + i as u16, 158),
            ] {
                let flag = base >= 152;
                let value = if flag {
                    u16::from(saved.word(base + i * 2) & (1 << ch) != 0)
                } else {
                    saved.word(base + i * 40 + ch * 2)
                };
                plan.push(Change {
                    selector: (family << 8) | ch as u16,
                    value,
                });
            }
            if ch < 4 {
                plan.push(Change {
                    selector: ((0x52 + i as u16) << 8) | ch as u16,
                    value: u16::from(saved.word(164 + i * 2) & (1 << ch) != 0),
                });
            }
        }
        if mix != Mix::Out34 {
            for pair in 2..if mix == Mix::Loopback { 5 } else { 4 } {
                plan.push(Change {
                    selector: ((if mix == Mix::Out12 { 0x55 } else { 0x57 }) << 8) | pair,
                    value: u16::from(saved.word(170 + i * 2) & (1 << pair) != 0),
                });
            }
        }
        plan.push(Change {
            selector: (0x45 + i as u16) << 8,
            value: saved.word(138 + i * 2),
        });
        plan.push(Change {
            selector: (0x58 + i as u16) << 8,
            value: saved.word(176 + i * 2),
        });
    }
    changed(current, plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> State {
        State::from_hex(include_str!("../tests/fixtures/windows-b2.hex").trim()).unwrap()
    }
    #[test]
    fn pan_fits_capture_center_and_extremes() {
        let s = fixture();
        let c = channel(&s, Mix::Out12, "analog1", Some("0"), Some(0.5), None, None).unwrap();
        assert_eq!((c[0].value, c[1].value), (0xfcfe, 0xfcfe));
        let c = channel(&s, Mix::Loopback, "usb6", Some("0"), Some(1.0), None, None).unwrap();
        assert_eq!((c[0].selector, c[0].value, c[1].value), (0x3109, 0x8000, 0));
    }
    #[test]
    fn mute_never_requantizes_gains() {
        let s = fixture();
        let c = channel(&s, Mix::Out34, "analog1", None, None, Some(false), None).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].selector, 0x4d00);
    }
    #[test]
    fn invalid_input_and_unconfirmed_membership_fail() {
        let s = fixture();
        assert!(channel(&s, Mix::Out12, "usb5", Some("0"), None, None, None).is_err());
        assert!(channel(&s, Mix::Out12, "analog1", None, Some(f64::NAN), None, None).is_err());
        for v in ["NaN", "inf", "13", "-61"] {
            assert!(db_word(v, -60.0, 12.0).is_err());
        }
        assert!(source(&s, Mix::Out34, "usb12", true).is_err());
        assert!(route(Mix::Out12, "usb34").is_err());
        assert!(route(Mix::Loopback, "out34").is_ok());
    }
    #[test]
    fn source_removal_mutes_and_clears_solo() {
        let mut s = fixture();
        s.set_word(152, 0);
        s.set_word(158, 1);
        let c = source(&s, Mix::Out12, "analog1", false).unwrap();
        let after = crate::state::predict(&s, &c).unwrap();
        assert_eq!(after.word(152) & 1, 1);
        assert_eq!(after.word(158) & 1, 0);
        assert_eq!(after.word(164) & 1, 0);
    }
    #[test]
    fn restore_preserves_opaque_bytes_trims_and_unselected_mixes() {
        let saved = fixture();
        let mut live = saved.clone();
        live.0[0] ^= 1;
        live.0[182] ^= 1;
        live.set_word(148, 128);
        live.set_word(18, 0);
        live.set_word(58, 0);
        let changes = restore(&live, &saved, Some(Mix::Out12)).unwrap();
        let out = crate::state::predict(&live, &changes).unwrap();
        assert_eq!(out.word(18), saved.word(18));
        assert_eq!(out.word(58), 0);
        assert_eq!(out.0[0], live.0[0]);
        assert_eq!(out.0[182], live.0[182]);
        assert_eq!(out.word(148), 128);
        let mut bad = saved;
        bad.set_word(172, bad.word(172) ^ 4);
        assert!(restore(&live, &bad, None).is_err());
    }
    #[test]
    fn refuses_partial_mono_stereo_conversions() {
        let mut s = fixture();
        s.set_word(164, s.word(164) | (1 << 4));
        assert!(source(&s, Mix::Out12, "usb12", false).is_err());
        let mut saved = s.clone();
        saved.set_word(170, saved.word(170) ^ (1 << 2));
        assert!(restore(&s, &saved, None).is_err());
        s.set_word(170, s.word(170) | 1);
        let mut saved = s.clone();
        saved.set_word(164, saved.word(164) ^ 1);
        assert!(restore(&s, &saved, None).is_err());
    }
    #[test]
    fn complete_restore_fits_one_bounded_transaction() {
        let saved = fixture();
        let mut current = saved.clone();
        for mix in Mix::ALL {
            let i = mix.index();
            for ch in 0..mix.channels() {
                current.set_word(18 + i * 40 + ch * 2, 0xf000);
                current.set_word(38 + i * 40 + ch * 2, 0xf000);
            }
            current.set_word(138 + i * 2, 0);
            let mask = (1u16 << mix.channels()) - 1;
            current.set_word(152 + i * 2, current.word(152 + i * 2) & !mask);
            current.set_word(158 + i * 2, (current.word(158 + i * 2) & !mask) | mask);
            current.set_word(164 + i * 2, 0);
            if mix != Mix::Out34 {
                current.set_word(170 + i * 2, 0);
            }
            current.set_word(176 + i * 2, [0, 2, 4][i]);
        }
        let plan = restore(&current, &saved, None).unwrap();
        assert!(plan.len() > 100 && plan.len() <= crate::state::MAX_CHANGES);
        let out = crate::state::predict(&current, &plan).unwrap();
        assert_eq!(out, saved);
    }
}
