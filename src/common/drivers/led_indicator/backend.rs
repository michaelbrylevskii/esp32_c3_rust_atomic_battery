use core::fmt;

/// Полярность подключения отдельного светодиода.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedPolarity {
    /// Светодиод загорается при высоком уровне сигнала.
    ActiveHigh,
    /// Светодиод загорается при низком уровне сигнала.
    ActiveLow,
}

/// Интерфейс backend'а, который умеет применять логические уровни яркости к группе каналов.
pub trait LedSink<const N: usize>: Send {
    /// Ошибка backend'а.
    type Error: fmt::Display + Send + 'static;

    /// Применяет логические уровни яркости `0..=255` к группе каналов.
    fn write_levels(&mut self, levels: [u8; N]) -> Result<(), Self::Error>;
}
