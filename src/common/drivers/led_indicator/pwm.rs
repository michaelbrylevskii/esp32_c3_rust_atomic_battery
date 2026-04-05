use super::backend::{LedPolarity, LedSink};
use super::constants::LEVEL_MAX;
use esp_idf_svc::hal::ledc::LedcDriver;
use esp_idf_svc::sys::EspError;
use std::fmt;

/// Тип, который удерживает LEDC timer живым на время существования PWM backend'а.
pub trait PwmTimerGuard: Send {}

impl<T> PwmTimerGuard for T where T: Send {}

/// Ошибки PWM backend'а.
#[derive(Debug)]
pub enum PwmLedError {
    /// Ошибка драйвера LEDC.
    Esp(EspError),
    /// Каналы используют разные значения `max_duty`.
    MixedMaxDuty,
}

impl From<EspError> for PwmLedError {
    fn from(value: EspError) -> Self {
        Self::Esp(value)
    }
}

impl fmt::Display for PwmLedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PwmLedError::Esp(err) => write!(f, "pwm ledc error: {err}"),
            PwmLedError::MixedMaxDuty => write!(f, "pwm channels use different max_duty"),
        }
    }
}

impl std::error::Error for PwmLedError {}

/// PWM backend для группы LED-каналов на базе LEDC.
pub struct PwmLedGroup<'d, const N: usize> {
    _timer_guard: Box<dyn PwmTimerGuard + 'd>,
    leds: [LedcDriver<'d>; N],
    polarities: [LedPolarity; N],
    max_duty: u32,
}

impl<'d, const N: usize> PwmLedGroup<'d, N> {
    /// Создаёт PWM backend из общего timer guard и уже настроенных LEDC channels.
    pub fn new<T>(
        timer_guard: T,
        leds: [LedcDriver<'d>; N],
        polarities: [LedPolarity; N],
    ) -> Result<Self, PwmLedError>
    where
        T: PwmTimerGuard + 'd,
    {
        let max_duty = leds.first().map(LedcDriver::get_max_duty).unwrap_or(0);

        if leds.iter().any(|led| led.get_max_duty() != max_duty) {
            return Err(PwmLedError::MixedMaxDuty);
        }

        let mut group = Self {
            _timer_guard: Box::new(timer_guard),
            leds,
            polarities,
            max_duty,
        };
        group.write_levels([0; N])?;
        Ok(group)
    }
}

impl<'d, const N: usize> LedSink<N> for PwmLedGroup<'d, N> {
    type Error = PwmLedError;

    fn write_levels(&mut self, levels: [u8; N]) -> Result<(), Self::Error> {
        for ((led, polarity), level) in self
            .leds
            .iter_mut()
            .zip(self.polarities.iter())
            .zip(levels.iter())
        {
            let duty = scale_level_to_duty(*level, self.max_duty);
            let duty = match polarity {
                LedPolarity::ActiveHigh => duty,
                LedPolarity::ActiveLow => self.max_duty.saturating_sub(duty),
            };
            led.set_duty(duty)?;
        }
        Ok(())
    }
}

fn scale_level_to_duty(level: u8, max_duty: u32) -> u32 {
    if level == 0 || max_duty == 0 {
        return 0;
    }

    if level == LEVEL_MAX {
        return max_duty;
    }

    (u32::from(level) * max_duty) / u32::from(LEVEL_MAX)
}

#[cfg(test)]
mod tests {
    use super::scale_level_to_duty;

    #[test]
    fn scale_level_uses_full_range() {
        assert_eq!(scale_level_to_duty(0, 255), 0);
        assert_eq!(scale_level_to_duty(255, 255), 255);
        assert_eq!(scale_level_to_duty(128, 255), 128);
    }
}
