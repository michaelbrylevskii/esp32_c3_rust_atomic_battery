//! Универсальная асинхронная LED-индикация.
//!
//! Подробная документация на русском:
//! [docs/led_indicator.md](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/docs/led_indicator.md)

pub mod backend;
pub mod constants;
pub mod controller;
pub mod digital;
pub mod pattern;
pub mod pwm;
