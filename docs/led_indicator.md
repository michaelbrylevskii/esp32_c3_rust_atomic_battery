# `led_indicator`: асинхронная LED-индикация, паттерны и backend'ы

## Что это такое

Модуль [`led_indicator`](../src/common/drivers/led_indicator/mod.rs) это набор слоёв для управления одним или несколькими светодиодами:

- backend применяет уровни к физическим каналам
- pattern описывает шаги, переходы и повторения
- async controller исполняет желаемый режим в фоне

Модель уровней единая для всех backend'ов:

- `0` — канал выключен
- `255` — канал полностью включён

## Структура модуля

```text
src/common/drivers/led_indicator/
  mod.rs
  constants.rs
  backend.rs
  async_controller.rs
  digital_backend.rs
  easing.rs
  pattern.rs
  pwm_backend.rs
```

Что где лежит:

- `backend.rs` — `LedSink` и `LedPolarity`
- `async_controller.rs` — `AsyncLedController`, `AsyncLedConfig`, `AsyncLedError`
- `easing.rs` — кривые переходов
- `pattern.rs` — `LedPattern`, `LedPatternStep`, `RepeatMode`
- `digital_backend.rs` — цифровой backend для GPIO-led
- `pwm_backend.rs` — PWM backend на базе `LEDC`
- `constants.rs` — общие константы вроде `LEVEL_MAX`

## Основные типы

### `backend::LedPolarity`

Полярность отдельного канала:

- `LedPolarity::ActiveHigh`
- `LedPolarity::ActiveLow`

### `async_controller::AsyncLedController`

Основной неблокирующий API.

Полезные методы:

- `new(backend, config)`
- `set_levels([..])`
- `turn_off()`
- `play_pattern(pattern)`
- `last_worker_error()`

### `async_controller::AsyncLedConfig`

Настройки фонового worker'а:

```rust
pub struct AsyncLedConfig {
    pub worker_tick: Duration,
    pub thread_stack_size: usize,
}
```

Значения по умолчанию:

- `worker_tick = 20 ms`
- `thread_stack_size = 4096`

### `pattern::LedPattern`

Модель LED-анимации.

Паттерн состоит из шагов:

- `LedPatternStep::Hold { levels, duration }`
- `LedPatternStep::Transition { from, to, duration, easing }`

И поддерживает режимы повторения:

- `RepeatMode::Once`
- `RepeatMode::Times(n)`
- `RepeatMode::Forever`

Готовые фабрики:

- `LedPattern::blink(...)`
- `LedPattern::alternate(...)`
- `LedPattern::pulse(...)`
- `LedPattern::steady(...)`
- `LedPattern::off()`

### `easing::Easing`

Встроенные кривые переходов:

- `Linear`
- `EaseInQuad`
- `EaseOutQuad`
- `EaseInOutQuad`
- `EaseInCubic`
- `EaseOutCubic`
- `EaseInOutCubic`
- `EaseInOutSine`
- `Custom(fn(f32) -> f32)`

### `digital_backend::DigitalLedGroup`

Цифровой backend для обычных GPIO-светодиодов.

Любой уровень больше нуля трактуется как "включено".

### `pwm_backend::PwmLedGroup`

PWM backend для LEDC.

Он принимает:

- общий `LEDC timer`, который остаётся живым внутри backend'а
- массив уже созданных `LedcDriver`
- массив полярностей каналов

## Как работает async controller

`AsyncLedController` хранит желаемый режим и выполняет его в фоне:

- `set_levels([..])` задаёт статические уровни
- `play_pattern(pattern)` запускает паттерн
- новая команда полностью заменяет предыдущий режим

Основной цикл приложения не делает `delay` ради LED-эффектов. Он только задаёт состояние или паттерн.

## Digital backend

Пример для двух обычных GPIO-led:

```rust
use common::drivers::led_indicator::backend::LedPolarity;
use common::drivers::led_indicator::constants::LEVEL_MAX;
use common::drivers::led_indicator::async_controller::{AsyncLedConfig, AsyncLedController};
use common::drivers::led_indicator::digital_backend::DigitalLedGroup;

let group = DigitalLedGroup::new(
    [red_led, green_led],
    [LedPolarity::ActiveHigh, LedPolarity::ActiveHigh],
)?;

let indicator = AsyncLedController::<2>::new(group, AsyncLedConfig::default())?;
indicator.set_levels([LEVEL_MAX, 0])?;
```

## PWM backend

Пример для двух LEDC-каналов на общей PWM-timer:

```rust
use common::drivers::led_indicator::backend::LedPolarity;
use common::drivers::led_indicator::async_controller::{AsyncLedConfig, AsyncLedController};
use common::drivers::led_indicator::pwm_backend::PwmLedGroup;
use esp_idf_svc::hal::ledc::config::{Resolution, TimerConfig};
use esp_idf_svc::hal::ledc::{LedcDriver, LedcTimerDriver};
use esp_idf_svc::hal::units::Hertz;

let timer = LedcTimerDriver::new(
    peripherals.ledc.timer0,
    &TimerConfig::default()
        .frequency(Hertz(5_000))
        .resolution(Resolution::Bits8),
)?;

let red = LedcDriver::new(peripherals.ledc.channel0, &timer, peripherals.pins.gpio0)?;
let green = LedcDriver::new(peripherals.ledc.channel1, &timer, peripherals.pins.gpio1)?;

let group = PwmLedGroup::new(
    timer,
    [red, green],
    [LedPolarity::ActiveHigh, LedPolarity::ActiveHigh],
)?;

let indicator = AsyncLedController::<2>::new(group, AsyncLedConfig::default())?;
```

PWM backend использует все уровни `0..=255` и поэтому поддерживает плавные переходы и пульсации не только на уровне модели, но и на железе.

## Demo bin

Проект содержит отдельный demo-бинарник [src/bin/led_indicator_demo/main.rs](../src/bin/led_indicator_demo/main.rs).

Он показывает два независимых controller'а одновременно:

- on-board LED на `GPIO8` через `DigitalLedGroup` с `LedPolarity::ActiveLow`
- внешний красный и зелёный LED на `GPIO0` и `GPIO1` через `PwmLedGroup`

Внутри одного цикла demo показывает:

- статические уровни `red`, `green`, `both`, `half brightness`
- простое мигание
- попеременное переключение `red/green`
- пульсацию сразу двух PWM-каналов
- плавный crossfade между `red` и `green`
- разницу между `transition_linear(...)` и easing-кривыми
- `Custom(fn(f32) -> f32)` на финальной сцене
- параллельную работу digital backend и PWM backend

Запуск:

```bash
cargo espflash flash --bin led_indicator_demo --monitor
```

## Примеры паттернов

### Простое мигание

```rust
use common::drivers::led_indicator::constants::LEVEL_MAX;
use common::drivers::led_indicator::pattern::LedPattern;
use std::time::Duration;

let pattern = LedPattern::blink(
    [LEVEL_MAX, 0],
    [0, 0],
    Duration::from_millis(180),
    Duration::from_millis(180),
    3,
);
```

### Попеременное red/green

```rust
use common::drivers::led_indicator::constants::LEVEL_MAX;
use common::drivers::led_indicator::pattern::LedPattern;
use std::time::Duration;

let pattern = LedPattern::alternate(
    [LEVEL_MAX, 0],
    [0, LEVEL_MAX],
    Duration::from_millis(180),
    3,
)
.final_levels([0, 0]);
```

### Пульсация

```rust
use common::drivers::led_indicator::constants::LEVEL_MAX;
use common::drivers::led_indicator::pattern::LedPattern;
use std::time::Duration;

let pattern = LedPattern::pulse(
    [LEVEL_MAX, 0],
    Duration::from_millis(400),
    Duration::from_millis(400),
    2,
);
```

### Ручная сборка

```rust
use common::drivers::led_indicator::constants::LEVEL_MAX;
use common::drivers::led_indicator::easing::Easing;
use common::drivers::led_indicator::pattern::{LedPattern, RepeatMode};
use std::time::Duration;

let pattern = LedPattern::<2>::new()
    .hold([LEVEL_MAX, 0], Duration::from_millis(120))
    .transition(
        [LEVEL_MAX, 0],
        [0, LEVEL_MAX],
        Duration::from_millis(600),
        Easing::EaseInOutSine,
    )
    .hold([0, LEVEL_MAX], Duration::from_millis(120))
    .repeat(RepeatMode::Times(2))
    .final_levels([0, 0]);
```

### Пользовательская кривая

```rust
use common::drivers::led_indicator::easing::Easing;
use common::drivers::led_indicator::pattern::LedPattern;
use std::time::Duration;

fn snap_to_end(_: f32) -> f32 {
    1.0
}

let pattern = LedPattern::<1>::new().transition(
    [0],
    [255],
    Duration::from_millis(300),
    Easing::Custom(snap_to_end),
);
```

## Demo bin

Для модуля есть отдельный бинарник:

```bash
cargo espflash flash --bin led_indicator_demo --monitor
```

Он демонстрирует:

- статические состояния `red` и `green`
- половинную яркость через PWM
- `blink`
- `alternate`
- `pulse`
- ручной `transition` с crossfade

## Что важно помнить

- worker применяет только уже подготовленные состояния и паттерны
- `set_levels(...)` и `play_pattern(...)` полностью заменяют предыдущий режим
- скорость и гладкость анимации зависят от `worker_tick`
- цифровой backend показывает только on/off
- PWM backend показывает уровни и переходы аппаратно
