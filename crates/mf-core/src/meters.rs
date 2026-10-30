// SPDX-License-Identifier: MIT
use crate::{Error, ErrorKind};
use serde::Serialize;

/// Device peak magnitudes, independent of configured gains. Mix peaks are
/// post-master, before destination route selection. Zero is represented as null.
#[derive(Clone, Debug, Serialize)]
pub struct Meters {
    /// Analog 1–4, then USB 1–6, in the same order as CHANNEL_NAMES.
    pub sources_dbfs: [Option<f64>; 10],
    /// Out12, out34, loopback; each pair is left, right.
    pub mixes_dbfs: [[Option<f64>; 2]; 3],
}
impl Meters {
    pub(crate) fn decode(data: &[u8]) -> Result<Self, Error> {
        if data.len() != 64 {
            return Err(Error::new(
                ErrorKind::ModuleIncompatible,
                "MF4 meter payload must contain 64 bytes",
            ));
        }
        let mut peaks = [None; 16];
        for (index, bytes) in data.chunks_exact(4).enumerate() {
            let value = u32::from_le_bytes(bytes.try_into().unwrap());
            if value > 0x8000_0000 {
                return Err(Error::new(
                    ErrorKind::ReadbackFailed,
                    "MF4 peak magnitude exceeds full scale",
                ));
            }
            peaks[index] = (value != 0).then(|| 20.0 * (f64::from(value) / 2147483648.0).log10());
        }
        Ok(Self {
            sources_dbfs: peaks[..10].try_into().unwrap(),
            mixes_dbfs: [
                [peaks[10], peaks[11]],
                [peaks[12], peaks[13]],
                [peaks[14], peaks[15]],
            ],
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn channel_order_scale_silence_and_bounds_are_explicit() {
        let mut payload = [0; 64];
        for (i, value) in [
            0, 0x80000000, 0x40000000, 214748, 429497, 1, 107631, 53944, 0, 0, 214752, 107631,
            53944, 0, 0x80000000, 1,
        ]
        .into_iter()
        .enumerate()
        {
            payload[i * 4..i * 4 + 4].copy_from_slice(&u32::to_le_bytes(value));
        }
        let meters = Meters::decode(&payload).unwrap();
        assert_eq!(meters.sources_dbfs[0], None);
        assert_eq!(meters.sources_dbfs[1], Some(0.0));
        assert!((meters.sources_dbfs[2].unwrap() + 6.020599913).abs() < 1e-8);
        assert!((meters.sources_dbfs[3].unwrap() + 80.0).abs() < 0.001);
        assert!((meters.mixes_dbfs[0][0].unwrap() + 80.0).abs() < 0.001);
        assert!((meters.mixes_dbfs[0][1].unwrap() + 86.0).abs() < 0.001);
        assert!((meters.mixes_dbfs[1][0].unwrap() + 92.0).abs() < 0.001);
        assert_eq!(meters.mixes_dbfs[1][1], None);
        assert_eq!(meters.mixes_dbfs[2][0], Some(0.0));
        assert!(meters.mixes_dbfs[2][1].unwrap().is_finite());
        assert!(Meters::decode(&payload[..63]).is_err());
        payload[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Meters::decode(&payload).is_err());
    }
}
