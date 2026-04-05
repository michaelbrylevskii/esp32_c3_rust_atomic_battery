use core::f32::consts::PI;
use core::ptr;

/// Кривая интерполяции для переходов между уровнями.
#[derive(Clone, Copy, Debug)]
pub enum Easing {
    /// Линейный переход.
    Linear,
    /// Медленный старт, затем ускорение по квадратичной кривой.
    EaseInQuad,
    /// Быстрый старт и плавное замедление по квадратичной кривой.
    EaseOutQuad,
    /// Плавный старт и плавное завершение по квадратичной кривой.
    EaseInOutQuad,
    /// Медленный старт, затем ускорение по кубической кривой.
    EaseInCubic,
    /// Быстрый старт и плавное замедление по кубической кривой.
    EaseOutCubic,
    /// Плавный старт и плавное завершение по кубической кривой.
    EaseInOutCubic,
    /// Плавный старт и завершение по синусоидальной кривой.
    EaseInOutSine,
    /// Пользовательская функция easing.
    Custom(fn(f32) -> f32),
}

impl PartialEq for Easing {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Easing::Linear, Easing::Linear)
            | (Easing::EaseInQuad, Easing::EaseInQuad)
            | (Easing::EaseOutQuad, Easing::EaseOutQuad)
            | (Easing::EaseInOutQuad, Easing::EaseInOutQuad)
            | (Easing::EaseInCubic, Easing::EaseInCubic)
            | (Easing::EaseOutCubic, Easing::EaseOutCubic)
            | (Easing::EaseInOutCubic, Easing::EaseInOutCubic)
            | (Easing::EaseInOutSine, Easing::EaseInOutSine) => true,
            (Easing::Custom(left), Easing::Custom(right)) => ptr::fn_addr_eq(left, right),
            _ => false,
        }
    }
}

impl Easing {
    /// Применяет кривую к нормализованному прогрессу `0.0..=1.0`.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);

        let value = match self {
            Easing::Linear => t,
            Easing::EaseInQuad => t * t,
            Easing::EaseOutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
            Easing::EaseInCubic => t * t * t,
            Easing::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            Easing::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
            Easing::EaseInOutSine => -(f32::cos(PI * t) - 1.0) / 2.0,
            Easing::Custom(function) => function(t),
        };

        value.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::Easing;

    fn reverse_curve(t: f32) -> f32 {
        1.0 - t
    }

    #[test]
    fn linear_keeps_midpoint() {
        assert_eq!(Easing::Linear.apply(0.5), 0.5);
    }

    #[test]
    fn sine_curve_is_bounded() {
        let value = Easing::EaseInOutSine.apply(0.5);
        assert!(value > 0.0);
        assert!(value < 1.0);
    }

    #[test]
    fn custom_curve_is_used() {
        assert_eq!(Easing::Custom(reverse_curve).apply(0.25), 0.75);
    }
}
