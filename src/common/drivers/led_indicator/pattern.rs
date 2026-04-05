use super::controller::AsyncLedError;
use core::time::Duration;
use std::array;

/// Режим повторения паттерна.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatMode {
    /// Выполнить паттерн один раз.
    Once,
    /// Выполнить паттерн указанное число раз.
    Times(u32),
    /// Крутить паттерн бесконечно.
    Forever,
}

/// Тип интерполяции между двумя наборами уровней.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Easing {
    /// Линейный переход.
    Linear,
}

/// Один шаг LED-паттерна.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedPatternStep<const N: usize> {
    /// Удерживать заданные уровни указанное время.
    Hold { levels: [u8; N], duration: Duration },
    /// Плавно перейти от одного набора уровней к другому.
    Transition {
        from: [u8; N],
        to: [u8; N],
        duration: Duration,
        easing: Easing,
    },
}

impl<const N: usize> LedPatternStep<N> {
    pub(crate) fn duration(&self) -> Duration {
        match self {
            LedPatternStep::Hold { duration, .. } => *duration,
            LedPatternStep::Transition { duration, .. } => *duration,
        }
    }

    pub(crate) fn terminal_levels(&self) -> [u8; N] {
        match self {
            LedPatternStep::Hold { levels, .. } => *levels,
            LedPatternStep::Transition { to, .. } => *to,
        }
    }
}

/// Последовательность LED-шагов, которую выполняет async worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedPattern<const N: usize> {
    steps: Vec<LedPatternStep<N>>,
    repeat: RepeatMode,
    final_levels: Option<[u8; N]>,
}

impl<const N: usize> LedPattern<N> {
    /// Создаёт пустой паттерн.
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            repeat: RepeatMode::Once,
            final_levels: None,
        }
    }

    /// Устанавливает режим повторения паттерна.
    pub fn repeat(mut self, repeat: RepeatMode) -> Self {
        self.repeat = repeat;
        self
    }

    /// Устанавливает финальные уровни после завершения конечного паттерна.
    pub fn final_levels(mut self, levels: [u8; N]) -> Self {
        self.final_levels = Some(levels);
        self
    }

    /// Добавляет шаг удержания.
    pub fn hold(mut self, levels: [u8; N], duration: Duration) -> Self {
        self.steps.push(LedPatternStep::Hold { levels, duration });
        self
    }

    /// Добавляет шаг линейного перехода.
    pub fn transition(mut self, from: [u8; N], to: [u8; N], duration: Duration) -> Self {
        self.steps.push(LedPatternStep::Transition {
            from,
            to,
            duration,
            easing: Easing::Linear,
        });
        self
    }

    /// Возвращает паттерн простого мигания между двумя состояниями.
    pub fn blink(
        on_levels: [u8; N],
        off_levels: [u8; N],
        on_duration: Duration,
        off_duration: Duration,
        times: u32,
    ) -> Self {
        Self::new()
            .hold(on_levels, on_duration)
            .hold(off_levels, off_duration)
            .repeat(RepeatMode::Times(times.max(1)))
            .final_levels(off_levels)
    }

    /// Возвращает паттерн попеременного переключения между двумя кадрами.
    pub fn alternate(
        first_levels: [u8; N],
        second_levels: [u8; N],
        phase_duration: Duration,
        cycles: u32,
    ) -> Self {
        Self::blink(
            first_levels,
            second_levels,
            phase_duration,
            phase_duration,
            cycles,
        )
    }

    /// Возвращает паттерн "пульсации" с линейным нарастанием и спадом.
    pub fn pulse(peak_levels: [u8; N], rise: Duration, fall: Duration, cycles: u32) -> Self {
        Self::new()
            .transition([0; N], peak_levels, rise)
            .transition(peak_levels, [0; N], fall)
            .repeat(RepeatMode::Times(cycles.max(1)))
            .final_levels([0; N])
    }

    /// Возвращает паттерн постоянного свечения.
    pub fn steady(levels: [u8; N]) -> Self {
        Self::new()
            .hold(levels, Duration::MAX)
            .repeat(RepeatMode::Forever)
    }

    /// Возвращает паттерн полного выключения.
    pub fn off() -> Self {
        Self::steady([0; N])
    }

    pub(crate) fn validate(&self) -> Result<(), AsyncLedError> {
        if self.steps.is_empty() {
            return Err(AsyncLedError::EmptyPattern);
        }

        if self.steps.iter().any(|step| step.duration().is_zero()) {
            return Err(AsyncLedError::InvalidStepDuration);
        }

        Ok(())
    }

    pub(crate) fn sample_levels(&self, elapsed: Duration) -> [u8; N] {
        if self.steps.is_empty() {
            return [0; N];
        }

        let cycle_duration_ms = self.total_duration().as_millis();
        if cycle_duration_ms == 0 {
            return self.default_final_levels();
        }

        let elapsed_ms = elapsed.as_millis();
        let cycle_elapsed_ms = match self.repeat {
            RepeatMode::Forever => elapsed_ms % cycle_duration_ms,
            RepeatMode::Once => {
                if elapsed_ms >= cycle_duration_ms {
                    return self.default_final_levels();
                }
                elapsed_ms
            }
            RepeatMode::Times(times) => {
                let repeats = u128::from(times.max(1));
                let total_duration_ms = cycle_duration_ms.saturating_mul(repeats);
                if elapsed_ms >= total_duration_ms {
                    return self.default_final_levels();
                }
                elapsed_ms % cycle_duration_ms
            }
        };

        let mut accumulated_ms = 0u128;
        for step in &self.steps {
            let step_duration_ms = step.duration().as_millis();
            if cycle_elapsed_ms < accumulated_ms.saturating_add(step_duration_ms) {
                let step_elapsed_ms = cycle_elapsed_ms.saturating_sub(accumulated_ms);
                return levels_from_step(step, step_elapsed_ms);
            }
            accumulated_ms = accumulated_ms.saturating_add(step_duration_ms);
        }

        self.default_final_levels()
    }

    fn total_duration(&self) -> Duration {
        self.steps
            .iter()
            .fold(Duration::ZERO, |sum, step| sum + step.duration())
    }

    fn default_final_levels(&self) -> [u8; N] {
        self.final_levels.unwrap_or_else(|| {
            self.steps
                .last()
                .map(LedPatternStep::terminal_levels)
                .unwrap_or([0; N])
        })
    }
}

impl<const N: usize> Default for LedPattern<N> {
    fn default() -> Self {
        Self::new()
    }
}

fn levels_from_step<const N: usize>(step: &LedPatternStep<N>, elapsed_ms: u128) -> [u8; N] {
    match step {
        LedPatternStep::Hold { levels, .. } => *levels,
        LedPatternStep::Transition {
            from,
            to,
            duration,
            easing,
        } => interpolate_levels(*from, *to, elapsed_ms, duration.as_millis(), *easing),
    }
}

fn interpolate_levels<const N: usize>(
    from: [u8; N],
    to: [u8; N],
    elapsed_ms: u128,
    duration_ms: u128,
    easing: Easing,
) -> [u8; N] {
    if duration_ms == 0 {
        return to;
    }

    match easing {
        Easing::Linear => array::from_fn(|index| {
            let from_level = i32::from(from[index]);
            let to_level = i32::from(to[index]);
            let delta = to_level - from_level;
            let progress = elapsed_ms.min(duration_ms) as i128;
            let value =
                i128::from(from_level) + (i128::from(delta) * progress) / duration_ms as i128;
            value.clamp(i128::from(u8::MIN), i128::from(u8::MAX)) as u8
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::led_indicator::constants::LEVEL_MAX;

    #[test]
    fn blink_returns_final_off_levels() {
        let pattern = LedPattern::<1>::blink(
            [LEVEL_MAX],
            [0],
            Duration::from_millis(100),
            Duration::from_millis(100),
            2,
        );

        assert_eq!(pattern.sample_levels(Duration::from_millis(0)), [LEVEL_MAX]);
        assert_eq!(pattern.sample_levels(Duration::from_millis(150)), [0]);
        assert_eq!(pattern.sample_levels(Duration::from_millis(450)), [0]);
    }

    #[test]
    fn transition_interpolates_linearly() {
        let pattern =
            LedPattern::<1>::new().transition([0], [LEVEL_MAX], Duration::from_millis(1000));

        assert_eq!(pattern.sample_levels(Duration::from_millis(0)), [0]);
        assert_eq!(pattern.sample_levels(Duration::from_millis(500)), [127]);
        assert_eq!(
            pattern.sample_levels(Duration::from_millis(1000)),
            [LEVEL_MAX]
        );
    }

    #[test]
    fn forever_pattern_wraps_cycle() {
        let pattern = LedPattern::<1>::new()
            .hold([10], Duration::from_millis(100))
            .hold([20], Duration::from_millis(100))
            .repeat(RepeatMode::Forever);

        assert_eq!(pattern.sample_levels(Duration::from_millis(50)), [10]);
        assert_eq!(pattern.sample_levels(Duration::from_millis(150)), [20]);
        assert_eq!(pattern.sample_levels(Duration::from_millis(250)), [10]);
    }
}
