// SPDX-License-Identifier: MIT
use mf_core::{InputSwitch, Level, Mix, Operation};
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};
use std::fmt;

pub const MAX_FRAME: usize = 256 * 1024;
pub const MAX_SNAPSHOT: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
    pub device_write_attempted: Option<bool>,
}
impl ApiError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            device_write_attempted: Some(false),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_request", message)
    }
    pub fn value(&self) -> Value {
        serde_json::json!({"code":self.code,"message":self.message,"details":{"device_write_attempted":self.device_write_attempted,"refresh_required":matches!(self.code,"stale_state"|"device_changed"|"device_unavailable"|"outcome_unknown"|"operation_failed")}})
    }
}
impl From<mf_core::Error> for ApiError {
    fn from(error: mf_core::Error) -> Self {
        use mf_core::ErrorKind::*;
        let code = match error.kind {
            UnsupportedCapability => "unsupported_capability",
            InvalidInput => "invalid_request",
            AmbiguousDevice => "ambiguous_device",
            AccessDenied => "access_denied",
            ModuleUnavailable | DeviceUnavailable => "device_unavailable",
            ModuleIncompatible => "module_incompatible",
            DeviceChanged => "device_changed",
            StaleState => "stale_state",
            BackupFailed => "backup_failed",
            JournalFailed => "journal_failed",
            TransferFailed | ReadbackFailed => "operation_failed",
        };
        let mut result = Self::new(code, error.message);
        result.device_write_attempted = error.device_write_attempted;
        result
    }
}
pub type Result<T> = std::result::Result<T, ApiError>;

/// Unlike serde_json::Value, this visitor rejects duplicate keys at every depth.
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Unique;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON with unique object keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Unique, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| Unique(Value::Number(n)))
                    .ok_or_else(|| E::custom("Nonfinite number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Unique, E> {
                Ok(Unique(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Unique, A::Error> {
                let mut out = Vec::new();
                while let Some(Unique(v)) = a.next_element()? {
                    out.push(v);
                }
                Ok(Unique(Value::Array(out)))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Unique, A::Error> {
                let mut out = Map::new();
                while let Some((k, Unique(v))) = a.next_entry::<String, Unique>()? {
                    if out.insert(k, v).is_some() {
                        return Err(de::Error::custom("Duplicate JSON key"));
                    }
                }
                Ok(Unique(Value::Object(out)))
            }
        }
        d.deserialize_any(V)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub api: u32,
    pub id: String,
    pub method: String,
    pub params: Value,
}
pub fn parse(bytes: &[u8]) -> Result<Request> {
    if bytes.len() + 1 > MAX_FRAME {
        return Err(ApiError::invalid("Frame exceeds 256 KiB"));
    }
    let Unique(value) =
        serde_json::from_slice(bytes).map_err(|e| ApiError::invalid(e.to_string()))?;
    let request: Request =
        serde_json::from_value(value).map_err(|e| ApiError::invalid(e.to_string()))?;
    if request.api != 1 {
        return Err(ApiError::new(
            "unsupported_api",
            "This service requires API major 1",
        ));
    }
    identifier(&request.id)?;
    if !request.params.is_object() {
        return Err(ApiError::invalid("params must be an object"));
    }
    Ok(request)
}
pub fn identifier(value: &str) -> Result<()> {
    if value.is_empty() || value.chars().count() > 128 || value.chars().any(char::is_control) {
        return Err(ApiError::invalid(
            "Identifiers require 1–128 printable characters",
        ));
    }
    Ok(())
}
pub fn params<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|e| ApiError::invalid(e.to_string()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub client: String,
    pub supported_api: Vec<u32>,
    pub client_token: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub device_id: String,
    pub generation: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Changes {
    pub device_id: String,
    pub generation: String,
    pub expected_revision: String,
    pub changes: Vec<WireChange>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Restore {
    pub device_id: String,
    pub generation: String,
    pub revision: String,
    pub snapshot: String,
    pub mix: Option<Mix>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Apply {
    pub plan_id: String,
    pub idempotency_key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub operation_id: Option<String>,
    pub idempotency_key: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub snapshot: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Save {
    pub name: String,
    pub snapshot: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Load {
    pub preset_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subscribe {
    pub events: Vec<String>,
    pub device_id: Option<String>,
    pub generation: Option<String>,
}

fn level<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<Level>, D::Error> {
    Ok(Some(match Option::<f64>::deserialize(d)? {
        Some(db) => Level::Db(db),
        None => Level::Silence,
    }))
}
fn number<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<f64>, D::Error> {
    f64::deserialize(d).map(Some)
}
fn flag<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<bool>, D::Error> {
    bool::deserialize(d).map(Some)
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireChange {
    InputSwitch {
        target: InputSwitch,
        enabled: bool,
    },
    Channel {
        mix: Mix,
        source: String,
        #[serde(default, deserialize_with = "level")]
        level_db: Option<Level>,
        #[serde(default, deserialize_with = "number")]
        pan: Option<f64>,
        #[serde(default, deserialize_with = "flag")]
        mute: Option<bool>,
        #[serde(default, deserialize_with = "flag")]
        solo: Option<bool>,
    },
    Master {
        mix: Mix,
        #[serde(deserialize_with = "level")]
        level_db: Option<Level>,
    },
    Route {
        mix: Mix,
        source: String,
    },
    Membership {
        mix: Mix,
        source: String,
        included: bool,
    },
}
impl From<WireChange> for Operation {
    fn from(change: WireChange) -> Self {
        match change {
            WireChange::InputSwitch { target, enabled } => Self::InputSwitch { target, enabled },
            WireChange::Channel {
                mix,
                source,
                level_db,
                pan,
                mute,
                solo,
            } => Self::Channel {
                mix,
                source,
                level: level_db,
                pan,
                mute,
                solo,
            },
            WireChange::Master { mix, level_db } => Self::Master {
                mix,
                level: level_db.expect("Present level deserializes to Some"),
            },
            WireChange::Route { mix, source } => Self::Route { mix, source },
            WireChange::Membership {
                mix,
                source,
                included,
            } => Self::Membership {
                mix,
                source,
                included,
            },
        }
    }
}
pub fn snapshot(content: &str) -> Result<mf_core::Snapshot> {
    if content.len() > MAX_SNAPSHOT {
        return Err(ApiError::invalid("Snapshot exceeds 64 KiB"));
    }
    let Unique(value) =
        serde_json::from_str(content).map_err(|e| ApiError::invalid(e.to_string()))?;
    let result: mf_core::Snapshot =
        serde_json::from_value(value).map_err(|e| ApiError::invalid(e.to_string()))?;
    result.state().map_err(ApiError::invalid)?;
    Ok(result)
}
