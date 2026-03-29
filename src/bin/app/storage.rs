use crate::errors::AppError;
use esp_idf_svc::nvs::{EspDefaultNvs, EspDefaultNvsPartition};

const NVS_NAMESPACE: &str = "atomic_app";
const KEY_CONSUMPTION_PER_SEC: &str = "cons_per_sec";
const KEY_ACTIVE_SESSION_ID: &str = "session_id";
const KEY_ACTIVE_BATTERY_UID: &str = "battery_uid";
const DEFAULT_CONSUMPTION_PER_SEC: u32 = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveSessionRecord {
    pub battery_uid_hex: String,
    pub session_id: u64,
}

pub struct AppStorage {
    nvs: EspDefaultNvs,
}

impl AppStorage {
    pub fn take() -> Result<Self, AppError> {
        let partition = EspDefaultNvsPartition::take()?;
        let nvs = EspDefaultNvs::new(partition, NVS_NAMESPACE, true)?;
        Ok(Self { nvs })
    }

    pub fn load_consumption_per_sec(&self) -> Result<u32, AppError> {
        Ok(self
            .nvs
            .get_u32(KEY_CONSUMPTION_PER_SEC)?
            .unwrap_or(DEFAULT_CONSUMPTION_PER_SEC))
    }

    pub fn save_consumption_per_sec(&self, value: u32) -> Result<(), AppError> {
        self.nvs.set_u32(KEY_CONSUMPTION_PER_SEC, value)?;
        Ok(())
    }

    pub fn load_active_session(&self) -> Result<Option<ActiveSessionRecord>, AppError> {
        let session_id = match self.nvs.get_u64(KEY_ACTIVE_SESSION_ID)? {
            Some(value) => value,
            None => return Ok(None),
        };

        let uid_len = match self.nvs.str_len(KEY_ACTIVE_BATTERY_UID)? {
            Some(value) => value,
            None => return Ok(None),
        };

        let mut uid_buf = vec![0u8; uid_len];
        let Some(battery_uid_hex) = self.nvs.get_str(KEY_ACTIVE_BATTERY_UID, &mut uid_buf)? else {
            return Ok(None);
        };

        Ok(Some(ActiveSessionRecord {
            battery_uid_hex: battery_uid_hex.to_owned(),
            session_id,
        }))
    }

    pub fn save_active_session(
        &self,
        battery_uid_hex: &str,
        session_id: u64,
    ) -> Result<(), AppError> {
        self.nvs.set_str(KEY_ACTIVE_BATTERY_UID, battery_uid_hex)?;
        self.nvs.set_u64(KEY_ACTIVE_SESSION_ID, session_id)?;
        Ok(())
    }

    pub fn clear_active_session(&self) -> Result<(), AppError> {
        let _ = self.nvs.remove(KEY_ACTIVE_BATTERY_UID)?;
        let _ = self.nvs.remove(KEY_ACTIVE_SESSION_ID)?;
        Ok(())
    }

    pub fn next_session_id(&self) -> Result<u64, AppError> {
        const KEY_COUNTER: &str = "session_ctr";
        let next_value = self.nvs.get_u64(KEY_COUNTER)?.unwrap_or(0).wrapping_add(1);
        self.nvs.set_u64(KEY_COUNTER, next_value)?;
        Ok(next_value)
    }
}
