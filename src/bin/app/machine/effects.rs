use common::utils::atomic_tags::BatteryTag;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppEffect {
    RequestNextSessionId,
    SaveConsumptionPerSec(u32),
    WriteBattery {
        expected_uid: Vec<u8>,
        battery: BatteryTag,
    },
}
