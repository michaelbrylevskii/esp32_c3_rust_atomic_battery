//! Универсальная асинхронная LED-индикация.
//!
//! Подробная документация на русском:
//! [docs/led_indicator.md](../../../../docs/led_indicator.md)

pub mod async_controller;
pub mod backend;
pub mod constants;
pub mod digital_backend;
pub mod easing;
pub mod pattern;
pub mod pwm_backend;
