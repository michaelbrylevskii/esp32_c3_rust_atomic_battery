use core::time::Duration;

/// Максимальный логический уровень яркости.
pub const LEVEL_MAX: u8 = u8::MAX;

/// Размер стека фонового LED worker по умолчанию.
pub const DEFAULT_WORKER_STACK_SIZE: usize = 4096;

/// Период обслуживания фонового LED worker по умолчанию.
pub const DEFAULT_WORKER_TICK: Duration = Duration::from_millis(20);
