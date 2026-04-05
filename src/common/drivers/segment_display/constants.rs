use core::time::Duration;

pub(crate) const COLON_MASK: u8 = 0b1000_0000;
pub(crate) const DISPLAY_WIDTH: usize = 4;
pub(crate) const STATIC_ERROR_TEXT: &str = "Err ";
pub(crate) const SCROLL_ERROR_TEXT: &str = "Error";
pub(crate) const DEFAULT_WORKER_STACK_SIZE: usize = 4096;
pub(crate) const DEFAULT_WORKER_TICK: Duration = Duration::from_millis(20);
