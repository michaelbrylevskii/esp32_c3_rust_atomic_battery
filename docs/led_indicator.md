# `led_indicator`: универсальная асинхронная LED-индикация

## Что это такое

Модуль [`led_indicator`](/mnt/data/Files/Projects/esp32_c3_rust_atomic_battery/src/common/drivers/led_indicator.rs) это неблокирующий driver для одного или нескольких светодиодов.

Он решает две задачи:

- хранит желаемое LED-состояние отдельно от основной логики приложения
- выполняет мигание и анимации в фоне, не блокируя основной цикл

Модуль специально сделан универсальным:

- количество каналов задаётся generic-параметром
- для каждого канала можно указать `active-high` или `active-low`
- уровни яркости задаются в диапазоне `0..=255`
- паттерны умеют хранить как простые удержания, так и переходы между уровнями

## Что умеет модуль

- статически выставлять уровни для группы LED
- выключать всю группу одним вызовом
- запускать конечные и бесконечные паттерны
- описывать паттерны как последовательность шагов
- поддерживать плавные переходы на уровне модели
- работать поверх разных backend'ов через trait `LedSink`

Сейчас в проекте реализован цифровой backend:

- `DigitalLedGroup`

Он трактует любой уровень больше нуля как "включено".

- для обычного GPIO-led всё уже работает
- API уже готово к будущему PWM backend'у
- если позже добавить backend на LEDC/PWM, менять формат паттернов не придётся

## Основные типы

### `LedPolarity`

Полярность отдельного канала:

- `LedPolarity::ActiveHigh`
- `LedPolarity::ActiveLow`

Это позволяет одинаково описывать:

- светодиоды, которые загораются при `HIGH`
- встроенные или внешние active-low светодиоды

### `DigitalLedGroup`

Цифровой backend для набора GPIO-пинов.

Создаётся из массива `PinDriver<'_, Output>` и массива полярностей:

```rust
let group = DigitalLedGroup::new(
    [red_led, green_led],
    [LedPolarity::ActiveHigh, LedPolarity::ActiveHigh],
)?;
```

### `AsyncLedController`

Основной async API.

Он принимает backend и запускает отдельный worker-поток:

```rust
let indicator = AsyncLedController::<2>::new(
    group,
    AsyncLedConfig::default(),
)?;
```

Основные методы:

- `set_levels([..])`
- `turn_off()`
- `play_pattern(pattern)`
- `last_worker_error()`

### `AsyncLedConfig`

Настройки фонового worker'а:

```rust
pub struct AsyncLedConfig {
    pub worker_tick: Duration,
    pub thread_stack_size: usize,
}
```

По умолчанию:

- `worker_tick = 20 ms`
- `thread_stack_size = 4096`

### `LedPattern`

Модель LED-анимации.

Паттерн состоит из шагов `LedPatternStep`:

- `Hold { levels, duration }`
- `Transition { from, to, duration, easing }`

Плюс задаётся режим повторения:

- `RepeatMode::Once`
- `RepeatMode::Times(n)`
- `RepeatMode::Forever`

И опционально финальные уровни после завершения:

- `final_levels([..])`

## Уровни яркости

Модель использует диапазон `0..=255`:

- `0` = канал выключен
- `255` = канал полностью включён

Для цифрового backend'а это означает:

- `0` превращается в off
- любой уровень больше `0` превращается в on

Модель уровней и переходов уже подходит для PWM backend'а. В цифровом backend'е GPIO-led покажет дискретное включение и выключение.

## Пример: статическая индикация

```rust
use common::drivers::led_indicator::{
    AsyncLedConfig, AsyncLedController, DigitalLedGroup, LedPolarity, LEVEL_MAX,
};

let group = DigitalLedGroup::new(
    [red_led, green_led],
    [LedPolarity::ActiveHigh, LedPolarity::ActiveHigh],
)?;

let indicator = AsyncLedController::<2>::new(group, AsyncLedConfig::default())?;

indicator.set_levels([LEVEL_MAX, 0])?;
```

Этот вызов включает только красный LED.

## Пример: попеременное мигание red/green

```rust
use common::drivers::led_indicator::{LedPattern, LEVEL_MAX};
use std::time::Duration;

let pattern = LedPattern::alternate(
    [LEVEL_MAX, 0],
    [0, LEVEL_MAX],
    Duration::from_millis(180),
    3,
)
.final_levels([0, 0]);

indicator.play_pattern(pattern)?;
```

Такой паттерн:

- 3 раза чередует красный и зелёный
- не блокирует основной цикл
- после завершения оставляет оба LED выключенными

## Пример: ручная сборка паттерна

```rust
use common::drivers::led_indicator::{LedPattern, RepeatMode, LEVEL_MAX};
use std::time::Duration;

let pattern = LedPattern::<2>::new()
    .hold([LEVEL_MAX, 0], Duration::from_millis(120))
    .hold([0, LEVEL_MAX], Duration::from_millis(120))
    .hold([0, 0], Duration::from_millis(120))
    .repeat(RepeatMode::Times(2))
    .final_levels([LEVEL_MAX, 0]);
```

Подходит для точной анимации без factory-хелперов.

## Пример: модель пульсации

```rust
use common::drivers::led_indicator::{LedPattern, LEVEL_MAX};
use std::time::Duration;

let pulse = LedPattern::pulse(
    [LEVEL_MAX, 0],
    Duration::from_millis(200),
    Duration::from_millis(400),
    2,
);

indicator.play_pattern(pulse)?;
```

С цифровым backend'ом это выглядит как ступенчатое переключение. С PWM backend'ом тот же паттерн даст плавную пульсацию.

## Что важно помнить

- worker-поток только исполняет уже подготовленный паттерн
- новый `set_levels(...)` или `play_pattern(...)` полностью заменяет предыдущий режим
- логика приложения не должна делать `delay` ради LED-эффектов
- лучше описывать желаемое состояние или готовый паттерн, а не дёргать GPIO вручную в основном цикле

## Когда это полезно

Подход удобен, когда:

- основной цикл не должен зависеть от индикации
- пара светодиодов логически работает как единый индикатор
- нужны повторяемые feedback-эффекты
- возможен переход с GPIO-led на PWM backend без переделки прикладной логики
