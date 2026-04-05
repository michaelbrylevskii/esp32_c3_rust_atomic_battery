use common::drivers::led_indicator::constants::LEVEL_MAX;
use std::time::Duration;

pub const LOOP_DELAY_MS: u32 = 50;
pub const CHARGE_PERSIST_INTERVAL: Duration = Duration::from_secs(60);
pub const NO_BAT_SCROLL_STEP: Duration = Duration::from_millis(250);
pub const ERROR_SCROLL_STEP: Duration = Duration::from_millis(250);
pub const SERVICE_SCROLL_STEP: Duration = Duration::from_millis(150);
pub const ACTIVE_COLON_PERIOD: Duration = Duration::from_secs(1);
pub const ACTIVE_COLON_ON_DURATION: Duration = Duration::from_millis(500);
pub const SERVICE_FEEDBACK_BLINK_CYCLES: u32 = 3;
pub const NO_BATTERY_TEXT: &str = "no bat";

pub const RED_ONLY_LEVELS: [u8; 2] = [LEVEL_MAX, 0];
pub const GREEN_ONLY_LEVELS: [u8; 2] = [0, LEVEL_MAX];
