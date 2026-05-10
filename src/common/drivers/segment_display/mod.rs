//! Удобная обёртка над TM1637 для 4-разрядного индикатора с двоеточием посередине.
//!
//! Подробная документация на русском:
//! [docs/segment_display.md](../../../../docs/segment_display.md)

mod async_display;
mod constants;
mod frame;
mod sync_display;
mod types;
mod worker;

pub use async_display::AsyncSegmentDisplay4;
pub use sync_display::SegmentDisplay4;
pub use types::{
    Align, AsyncDisplayConfig, AsyncDisplayError, DisplayConfig, DisplayError, IntFormat,
};
