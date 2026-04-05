use super::backend::{LedPolarity, LedSink};
use esp_idf_svc::hal::gpio::{Level, Output, PinDriver};
use esp_idf_svc::sys::EspError;

/// Цифровой backend для группы GPIO-светодиодов.
///
/// Любой уровень больше нуля трактуется как состояние "включено".
pub struct DigitalLedGroup<'d, const N: usize> {
    leds: [PinDriver<'d, Output>; N],
    polarities: [LedPolarity; N],
}

impl<'d, const N: usize> DigitalLedGroup<'d, N> {
    /// Создаёт цифровой backend из уже настроенных output-пинов.
    pub fn new(
        leds: [PinDriver<'d, Output>; N],
        polarities: [LedPolarity; N],
    ) -> Result<Self, EspError> {
        let mut group = Self { leds, polarities };
        group.write_levels([0; N])?;
        Ok(group)
    }
}

impl LedPolarity {
    pub(crate) fn to_gpio_level(self, level: u8) -> Level {
        let is_on = level > 0;
        match (self, is_on) {
            (LedPolarity::ActiveHigh, true) => Level::High,
            (LedPolarity::ActiveHigh, false) => Level::Low,
            (LedPolarity::ActiveLow, true) => Level::Low,
            (LedPolarity::ActiveLow, false) => Level::High,
        }
    }
}

impl<'d, const N: usize> LedSink<N> for DigitalLedGroup<'d, N> {
    type Error = EspError;

    fn write_levels(&mut self, levels: [u8; N]) -> Result<(), Self::Error> {
        for ((led, polarity), level) in self
            .leds
            .iter_mut()
            .zip(self.polarities.iter())
            .zip(levels.iter())
        {
            led.set_level(polarity.to_gpio_level(*level))?;
        }
        Ok(())
    }
}
