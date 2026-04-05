use crate::utils::kv_store::{KvFormatError, KvStore, KvValue};
use core::fmt;

pub const TAG_TYPE_KEY: &str = "tag_type";
pub const TAG_TYPE_BATTERY: &str = "battery";
pub const TAG_TYPE_SERVICE: &str = "service";

pub const BATTERY_CAPACITY_KEY: &str = "capacity";
pub const BATTERY_CHARGE_KEY: &str = "charge";
pub const BATTERY_HEALTHY_KEY: &str = "healthy";
pub const BATTERY_DIRTY_KEY: &str = "dirty";
pub const BATTERY_SESSION_ID_KEY: &str = "session_id";

pub const SERVICE_TYPE_KEY: &str = "service_type";
pub const SERVICE_TYPE_CONSUMPTION_CONFIG: &str = "consumption_config";
pub const SERVICE_CONSUMPTION_PER_SEC_KEY: &str = "consumption_per_sec";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatteryTag {
    pub capacity: u64,
    pub charge: u64,
    pub healthy: bool,
    pub dirty: bool,
    pub session_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceTag {
    pub consumption_per_sec: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AtomicTag {
    Battery(BatteryTag),
    Service(ServiceTag),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AtomicTagError {
    MissingField(&'static str),
    InvalidFieldType(&'static str),
    InvalidFieldValue(&'static str),
    InvalidTagType,
    InvalidServiceType,
}

impl fmt::Display for AtomicTagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AtomicTagError::MissingField(field) => write!(f, "missing field `{field}`"),
            AtomicTagError::InvalidFieldType(field) => write!(f, "invalid type for `{field}`"),
            AtomicTagError::InvalidFieldValue(field) => write!(f, "invalid value for `{field}`"),
            AtomicTagError::InvalidTagType => write!(f, "invalid atomic tag type"),
            AtomicTagError::InvalidServiceType => write!(f, "invalid service tag type"),
        }
    }
}

impl std::error::Error for AtomicTagError {}

impl BatteryTag {
    pub fn new(capacity: u64, charge: u64) -> Result<Self, AtomicTagError> {
        if capacity == 0 {
            return Err(AtomicTagError::InvalidFieldValue(BATTERY_CAPACITY_KEY));
        }

        if charge > capacity {
            return Err(AtomicTagError::InvalidFieldValue(BATTERY_CHARGE_KEY));
        }

        Ok(Self {
            capacity,
            charge,
            healthy: true,
            dirty: false,
            session_id: 0,
        })
    }

    pub fn is_usable(&self) -> bool {
        self.healthy && !self.dirty && self.charge > 0
    }

    pub fn open_session(&mut self, session_id: u64) {
        self.dirty = true;
        self.session_id = session_id;
    }

    pub fn close_session(&mut self) {
        self.dirty = false;
        self.session_id = 0;
    }

    pub fn mark_broken(&mut self) {
        self.healthy = false;
        self.dirty = false;
        self.session_id = 0;
    }

    pub fn consume(&mut self, amount: u64) -> u64 {
        let consumed = self.charge.min(amount);
        self.charge -= consumed;
        consumed
    }

    pub fn remaining_seconds(&self, consumption_per_sec: u32) -> u64 {
        if consumption_per_sec == 0 {
            return 0;
        }

        self.charge / u64::from(consumption_per_sec)
    }

    pub fn to_store(&self) -> Result<KvStore, KvFormatError> {
        let mut store = KvStore::new();
        store.insert_string(TAG_TYPE_KEY, TAG_TYPE_BATTERY)?;
        store.insert_u64(BATTERY_CAPACITY_KEY, self.capacity)?;
        store.insert_u64(BATTERY_CHARGE_KEY, self.charge)?;
        store.insert_bool(BATTERY_HEALTHY_KEY, self.healthy)?;
        store.insert_bool(BATTERY_DIRTY_KEY, self.dirty)?;
        store.insert_u64(BATTERY_SESSION_ID_KEY, self.session_id)?;
        Ok(store)
    }

    pub fn from_store(store: &KvStore) -> Result<Self, AtomicTagError> {
        let tag_type = get_str(store, TAG_TYPE_KEY)?;
        if tag_type != TAG_TYPE_BATTERY {
            return Err(AtomicTagError::InvalidTagType);
        }

        let capacity = get_u64(store, BATTERY_CAPACITY_KEY)?;
        let charge = get_u64(store, BATTERY_CHARGE_KEY)?;
        let healthy = get_bool(store, BATTERY_HEALTHY_KEY)?;
        let dirty = get_bool(store, BATTERY_DIRTY_KEY)?;
        let session_id = get_u64(store, BATTERY_SESSION_ID_KEY)?;

        if capacity == 0 {
            return Err(AtomicTagError::InvalidFieldValue(BATTERY_CAPACITY_KEY));
        }

        if charge > capacity {
            return Err(AtomicTagError::InvalidFieldValue(BATTERY_CHARGE_KEY));
        }

        Ok(Self {
            capacity,
            charge,
            healthy,
            dirty,
            session_id,
        })
    }
}

impl ServiceTag {
    pub fn new(consumption_per_sec: u32) -> Result<Self, AtomicTagError> {
        if consumption_per_sec == 0 {
            return Err(AtomicTagError::InvalidFieldValue(
                SERVICE_CONSUMPTION_PER_SEC_KEY,
            ));
        }

        Ok(Self {
            consumption_per_sec,
        })
    }

    pub fn to_store(&self) -> Result<KvStore, KvFormatError> {
        let mut store = KvStore::new();
        store.insert_string(TAG_TYPE_KEY, TAG_TYPE_SERVICE)?;
        store.insert_string(SERVICE_TYPE_KEY, SERVICE_TYPE_CONSUMPTION_CONFIG)?;
        store.insert_u32(SERVICE_CONSUMPTION_PER_SEC_KEY, self.consumption_per_sec)?;
        Ok(store)
    }

    pub fn from_store(store: &KvStore) -> Result<Self, AtomicTagError> {
        let tag_type = get_str(store, TAG_TYPE_KEY)?;
        if tag_type != TAG_TYPE_SERVICE {
            return Err(AtomicTagError::InvalidTagType);
        }

        let service_type = get_str(store, SERVICE_TYPE_KEY)?;
        if service_type != SERVICE_TYPE_CONSUMPTION_CONFIG {
            return Err(AtomicTagError::InvalidServiceType);
        }

        let consumption_per_sec = get_u32(store, SERVICE_CONSUMPTION_PER_SEC_KEY)?;
        Self::new(consumption_per_sec)
    }
}

impl AtomicTag {
    pub fn from_store(store: &KvStore) -> Result<Self, AtomicTagError> {
        match get_str(store, TAG_TYPE_KEY)?.as_str() {
            TAG_TYPE_BATTERY => Ok(Self::Battery(BatteryTag::from_store(store)?)),
            TAG_TYPE_SERVICE => Ok(Self::Service(ServiceTag::from_store(store)?)),
            _ => Err(AtomicTagError::InvalidTagType),
        }
    }
}

fn get_value<'a>(store: &'a KvStore, key: &'static str) -> Result<&'a KvValue, AtomicTagError> {
    store.get(key).ok_or(AtomicTagError::MissingField(key))
}

fn get_str(store: &KvStore, key: &'static str) -> Result<String, AtomicTagError> {
    match get_value(store, key)? {
        KvValue::Str(value) => Ok(value.clone()),
        _ => Err(AtomicTagError::InvalidFieldType(key)),
    }
}

fn get_bool(store: &KvStore, key: &'static str) -> Result<bool, AtomicTagError> {
    match get_value(store, key)? {
        KvValue::Bool(value) => Ok(*value),
        _ => Err(AtomicTagError::InvalidFieldType(key)),
    }
}

fn get_u32(store: &KvStore, key: &'static str) -> Result<u32, AtomicTagError> {
    match get_value(store, key)? {
        KvValue::U32(value) => Ok(*value),
        KvValue::U16(value) => Ok(u32::from(*value)),
        KvValue::U8(value) => Ok(u32::from(*value)),
        _ => Err(AtomicTagError::InvalidFieldType(key)),
    }
}

fn get_u64(store: &KvStore, key: &'static str) -> Result<u64, AtomicTagError> {
    match get_value(store, key)? {
        KvValue::U64(value) => Ok(*value),
        KvValue::U32(value) => Ok(u64::from(*value)),
        KvValue::U16(value) => Ok(u64::from(*value)),
        KvValue::U8(value) => Ok(u64::from(*value)),
        _ => Err(AtomicTagError::InvalidFieldType(key)),
    }
}
